#![allow(dead_code)]

use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::header::RETRY_AFTER;
use roxmltree::{Document, Node};
use tracing::warn;

use crate::config::AppConfig;
use crate::error::{AppError, Result};
use crate::model::{arxiv_id_from_abs_url, ArxivPaper, SearchQuery};

const ARXIV_API_URL: &str = "https://export.arxiv.org/api/query";
const ATOM_NS: &str = "http://www.w3.org/2005/Atom";
const USER_AGENT_VALUE: &str = "litscout-rs/0.1";
const ARXIV_MAX_ATTEMPTS: usize = 3;

pub async fn search_papers(query: &SearchQuery, config: &AppConfig) -> Result<Vec<ArxivPaper>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout_secs))
        .user_agent(USER_AGENT_VALUE)
        .build()?;

    let search_query = format!("all:{}", query.topic);
    let max_results = query.arxiv_limit.clamp(1, 100).to_string();
    let params = [
        ("search_query", search_query),
        ("start", "0".to_string()),
        ("max_results", max_results),
        ("sortBy", "relevance".to_string()),
        ("sortOrder", "descending".to_string()),
    ];

    let mut last_error = None;
    for attempt in 1..=ARXIV_MAX_ATTEMPTS {
        match client.get(ARXIV_API_URL).query(&params).send().await {
            Ok(response) => {
                let status = response.status();
                let retry_after = retry_after_delay(response.headers().get(RETRY_AFTER));
                let body = response.text().await?;

                if status.is_success() {
                    return parse_arxiv_atom(&body);
                }

                if status.as_u16() == 429 {
                    let delay = retry_after
                        .unwrap_or_else(|| arxiv_backoff_delay(attempt))
                        .min(Duration::from_secs(30));
                    let err = AppError::RateLimit {
                        service: "arXiv",
                        reset: format!("; retry after about {}s", delay.as_secs()),
                    };
                    if attempt < ARXIV_MAX_ATTEMPTS {
                        warn!(
                            "arXiv rate limited request on attempt {attempt}/{ARXIV_MAX_ATTEMPTS}; retrying after {:?}",
                            delay
                        );
                        last_error = Some(err);
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    return Err(err);
                }

                return Err(AppError::HttpStatus {
                    service: "arXiv",
                    status: status.as_u16(),
                    body,
                });
            }
            Err(err) if attempt < ARXIV_MAX_ATTEMPTS => {
                let delay = arxiv_backoff_delay(attempt);
                warn!(
                    "arXiv request failed on attempt {attempt}/{ARXIV_MAX_ATTEMPTS}; retrying after {:?}: {err}",
                    delay
                );
                last_error = Some(AppError::Http(err));
                tokio::time::sleep(delay).await;
            }
            Err(err) => return Err(AppError::Http(err)),
        }
    }

    Err(last_error.unwrap_or_else(|| AppError::Workflow("arXiv request failed".to_string())))
}

fn arxiv_backoff_delay(attempt: usize) -> Duration {
    Duration::from_secs(match attempt {
        0 | 1 => 3,
        2 => 8,
        _ => 15,
    })
}

fn retry_after_delay(value: Option<&reqwest::header::HeaderValue>) -> Option<Duration> {
    value
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
}

pub fn parse_arxiv_atom(xml: &str) -> Result<Vec<ArxivPaper>> {
    let doc = Document::parse(xml).map_err(|err| AppError::Xml(err.to_string()))?;
    let mut papers = Vec::new();

    for entry in doc
        .descendants()
        .filter(|node| has_tag(*node, ATOM_NS, "entry"))
    {
        papers.push(parse_entry(entry)?);
    }

    Ok(papers)
}

fn parse_entry(entry: Node<'_, '_>) -> Result<ArxivPaper> {
    let id_url = child_text(entry, ATOM_NS, "id").ok_or_else(|| missing_field("id"))?;
    let arxiv_id = arxiv_id_from_abs_url(&id_url);
    let title = child_text(entry, ATOM_NS, "title").ok_or_else(|| missing_field("title"))?;
    let summary = child_text(entry, ATOM_NS, "summary").ok_or_else(|| missing_field("summary"))?;
    let published_raw =
        child_text(entry, ATOM_NS, "published").ok_or_else(|| missing_field("published"))?;
    let published_at = parse_datetime(&published_raw, "published")?;
    let updated_at = child_text(entry, ATOM_NS, "updated")
        .map(|value| parse_datetime(&value, "updated"))
        .transpose()?;

    let authors = entry
        .children()
        .filter(|node| has_tag(*node, ATOM_NS, "author"))
        .filter_map(|author| child_text(author, ATOM_NS, "name"))
        .collect::<Vec<_>>();

    let categories = entry
        .children()
        .filter(|node| has_tag(*node, ATOM_NS, "category"))
        .filter_map(|node| node.attribute("term").map(str::to_string))
        .collect::<Vec<_>>();

    let (abs_url, pdf_url) = parse_links(entry, &id_url);

    Ok(ArxivPaper {
        arxiv_id,
        title,
        authors,
        summary,
        published_at,
        updated_at,
        categories,
        abs_url,
        pdf_url,
    })
}

fn parse_links(entry: Node<'_, '_>, id_url: &str) -> (String, Option<String>) {
    let mut abs_url = None;
    let mut pdf_url = None;

    for link in entry
        .children()
        .filter(|node| has_tag(*node, ATOM_NS, "link"))
    {
        let Some(href) = link.attribute("href") else {
            continue;
        };
        let rel = link.attribute("rel").unwrap_or_default();
        let title = link.attribute("title").unwrap_or_default();

        if rel == "alternate" {
            abs_url = Some(href.to_string());
        }
        if title.eq_ignore_ascii_case("pdf") || href.contains("/pdf/") {
            pdf_url = Some(href.to_string());
        }
    }

    (abs_url.unwrap_or_else(|| id_url.to_string()), pdf_url)
}

fn child_text(parent: Node<'_, '_>, namespace: &str, name: &str) -> Option<String> {
    parent
        .children()
        .find(|node| has_tag(*node, namespace, name))
        .and_then(|node| node.text())
        .map(clean_text)
        .filter(|value| !value.is_empty())
}

fn has_tag(node: Node<'_, '_>, namespace: &str, name: &str) -> bool {
    node.is_element()
        && node.tag_name().name() == name
        && node.tag_name().namespace() == Some(namespace)
}

fn parse_datetime(value: &str, field: &'static str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|err| AppError::Xml(format!("invalid {field} datetime `{value}`: {err}")))
}

fn missing_field(field: &'static str) -> AppError {
    AppError::Xml(format!("missing required arXiv field `{field}`"))
}

fn clean_text(text: &str) -> String {
    decode_common_entities(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn decode_common_entities(text: &str) -> String {
    text.replace("&#xA;", "\n")
        .replace("&#10;", "\n")
        .replace("&#xA0;", " ")
        .replace("&#160;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use reqwest::header::HeaderValue;

    use super::{parse_arxiv_atom, retry_after_delay};

    #[test]
    fn parses_arxiv_atom_fixture() {
        let xml = include_str!("../../tests/fixtures/arxiv_search.xml");
        let papers = parse_arxiv_atom(xml).expect("fixture should parse");

        assert_eq!(papers.len(), 2);
        assert_eq!(papers[0].arxiv_id, "2401.12345v2");
        assert_eq!(papers[0].title, "Rust Agents & Tool Calling");
        assert_eq!(papers[0].authors, vec!["Ada Lovelace", "Alan Turing"]);
        assert_eq!(
            papers[0].summary,
            "This paper studies Rust-based agents with tool calling."
        );
        assert_eq!(papers[0].categories, vec!["cs.AI", "cs.SE"]);
        assert_eq!(papers[0].abs_url, "https://arxiv.org/abs/2401.12345v2");
        assert_eq!(
            papers[0].pdf_url.as_deref(),
            Some("https://arxiv.org/pdf/2401.12345v2")
        );
        assert_eq!(papers[1].arxiv_id, "2501.00001");
        assert!(papers[1].pdf_url.is_none());
    }

    #[test]
    fn parses_retry_after_seconds() {
        let value = HeaderValue::from_static("7");

        assert_eq!(
            retry_after_delay(Some(&value)),
            Some(Duration::from_secs(7))
        );
    }
}
