#![allow(dead_code)]

use std::time::Duration;

use reqwest::header::{HeaderMap, RETRY_AFTER};
use serde::Deserialize;

use crate::config::AppConfig;
use crate::error::{AppError, Result};
use crate::model::{SearchQuery, SourceItem, SourceKind, SourceMetadata};

const CROSSREF_WORKS_URL: &str = "https://api.crossref.org/works";
const USER_AGENT_VALUE: &str = "litscout-rs/0.1";

pub async fn search_works(query: &SearchQuery, config: &AppConfig) -> Result<Vec<SourceItem>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout_secs))
        .user_agent(USER_AGENT_VALUE)
        .build()?;
    let rows = query.arxiv_limit.clamp(1, 100).to_string();
    let mut request = client.get(CROSSREF_WORKS_URL).query(&[
        ("query.bibliographic", query.topic.as_str()),
        ("rows", rows.as_str()),
        ("select", "DOI,title,author,container-title,published-print,published-online,published,issued,type,URL,abstract,is-referenced-by-count"),
    ]);
    if let Some(mailto) = config
        .crossref_mailto
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        request = request.query(&[("mailto", mailto)]);
    }
    let response = request.send().await?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.text().await?;

    if status.is_success() {
        return parse_search_response(&body);
    }

    if status.as_u16() == 429 {
        return Err(AppError::RateLimit {
            service: "Crossref",
            reset: retry_after_message(&headers),
        });
    }

    Err(AppError::HttpStatus {
        service: "Crossref",
        status: status.as_u16(),
        body,
    })
}

pub fn parse_search_response(body: &str) -> Result<Vec<SourceItem>> {
    let response: CrossrefSearchResponse = serde_json::from_str(body)?;
    Ok(response
        .message
        .items
        .into_iter()
        .filter_map(CrossrefWork::into_source_item)
        .collect())
}

fn retry_after_message(headers: &HeaderMap) -> String {
    headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .map(|seconds| format!("; retry after about {seconds}s"))
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
struct CrossrefSearchResponse {
    message: CrossrefMessage,
}

#[derive(Debug, Deserialize)]
struct CrossrefMessage {
    #[serde(default)]
    items: Vec<CrossrefWork>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct CrossrefWork {
    #[serde(rename = "DOI")]
    doi: Option<String>,
    #[serde(default)]
    title: Vec<String>,
    #[serde(default)]
    author: Vec<CrossrefAuthor>,
    #[serde(default)]
    container_title: Vec<String>,
    published_print: Option<CrossrefDateParts>,
    published_online: Option<CrossrefDateParts>,
    published: Option<CrossrefDateParts>,
    issued: Option<CrossrefDateParts>,
    #[serde(rename = "URL")]
    url: Option<String>,
    #[serde(rename = "abstract")]
    abstract_text: Option<String>,
    is_referenced_by_count: Option<u64>,
}

impl CrossrefWork {
    fn into_source_item(self) -> Option<SourceItem> {
        let doi = normalize_doi(self.doi.as_deref())?;
        let title = clean_required(self.title.into_iter().next())?;
        let authors = self
            .author
            .into_iter()
            .filter_map(CrossrefAuthor::display_name)
            .collect::<Vec<_>>();
        let venue = self
            .container_title
            .into_iter()
            .find_map(|value| clean_optional(Some(value)));
        let year = self
            .published_print
            .as_ref()
            .or(self.published_online.as_ref())
            .or(self.published.as_ref())
            .or(self.issued.as_ref())
            .and_then(CrossrefDateParts::year);
        let url = clean_optional(self.url).unwrap_or_else(|| format!("https://doi.org/{doi}"));
        let abstract_text = clean_crossref_abstract(self.abstract_text);
        let summary = abstract_text
            .unwrap_or_else(|| bibliographic_summary(&authors, venue.as_deref(), year));
        let external_ids = vec![format!("doi:{doi}")];

        Some(SourceItem {
            id: format!("crossref:{doi}"),
            kind: SourceKind::Bibliography,
            title,
            url,
            summary: summary.clone(),
            evidence_snippet: excerpt(&summary, 420),
            tags: venue.clone().into_iter().collect(),
            score: 0.0,
            score_reasons: Vec::new(),
            classification_reasons: Vec::new(),
            score_breakdown: Default::default(),
            published_or_updated_at: None,
            metadata: SourceMetadata::Bibliography {
                authors,
                venue,
                year,
                doi: Some(doi.clone()),
                citation_count: self.is_referenced_by_count,
                native_id: doi,
                source_name: "crossref".to_string(),
                external_ids,
            },
        })
    }
}

#[derive(Debug, Deserialize)]
struct CrossrefAuthor {
    given: Option<String>,
    family: Option<String>,
    name: Option<String>,
}

impl CrossrefAuthor {
    fn display_name(self) -> Option<String> {
        if let Some(name) = clean_optional(self.name) {
            return Some(name);
        }
        let name = format!(
            "{} {}",
            self.given.unwrap_or_default(),
            self.family.unwrap_or_default()
        );
        clean_optional(Some(name))
    }
}

#[derive(Debug, Deserialize)]
struct CrossrefDateParts {
    #[serde(rename = "date-parts", default)]
    date_parts: Vec<Vec<Option<i32>>>,
}

impl CrossrefDateParts {
    fn year(&self) -> Option<i32> {
        self.date_parts
            .first()
            .and_then(|parts| parts.first())
            .and_then(|year| *year)
    }
}

fn clean_required(value: Option<String>) -> Option<String> {
    clean_optional(value).filter(|value| !value.is_empty())
}

fn clean_optional(value: Option<String>) -> Option<String> {
    let cleaned = value?
        .replace("<jats:p>", "")
        .replace("</jats:p>", "")
        .replace("<jats:title>", "")
        .replace("</jats:title>", "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!cleaned.is_empty()).then_some(cleaned)
}

fn clean_crossref_abstract(value: Option<String>) -> Option<String> {
    clean_optional(value)
}

fn normalize_doi(value: Option<&str>) -> Option<String> {
    value
        .map(|doi| {
            doi.trim()
                .trim_start_matches("https://doi.org/")
                .trim_start_matches("http://doi.org/")
                .trim_start_matches("doi:")
                .to_ascii_lowercase()
        })
        .filter(|doi| !doi.is_empty())
}

fn bibliographic_summary(authors: &[String], venue: Option<&str>, year: Option<i32>) -> String {
    let author_text = if authors.is_empty() {
        "unknown authors".to_string()
    } else {
        authors
            .iter()
            .take(4)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    };
    let venue_text = venue.unwrap_or("unknown venue");
    let year_text = year
        .map(|year| year.to_string())
        .unwrap_or_else(|| "unknown year".to_string());
    format!("Bibliographic metadata: {author_text}. {venue_text}, {year_text}.")
}

fn excerpt(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let mut out = trimmed.chars().take(max_chars).collect::<String>();
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::parse_search_response;
    use crate::model::{SourceKind, SourceMetadata};

    #[test]
    fn parses_crossref_fixture() {
        let body = include_str!("../../tests/fixtures/crossref_search.json");
        let items = parse_search_response(body).expect("fixture should parse");

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "crossref:10.1234/tool-agent");
        assert_eq!(items[0].kind, SourceKind::Bibliography);
        assert_eq!(items[0].title, "Tool Calling Agents in Rust");
        assert!(items[0].summary.contains("Bibliographic metadata"));
        assert!(matches!(
            items[0].metadata,
            SourceMetadata::Bibliography {
                citation_count: Some(9),
                ref source_name,
                ref external_ids,
                ..
            } if source_name == "crossref" && external_ids.iter().any(|id| id == "doi:10.1234/tool-agent")
        ));
    }

    #[test]
    fn parses_empty_crossref_response() {
        let items =
            parse_search_response(r#"{"message":{"items":[]}}"#).expect("empty response parses");

        assert!(items.is_empty());
    }

    #[test]
    fn skips_crossref_records_without_required_fields() {
        let items = parse_search_response(
            r#"{"message":{"items":[{"DOI":"10.1000/missing-title"},{"title":["missing doi"]}]}}"#,
        )
        .expect("partial response parses");

        assert!(items.is_empty());
    }

    #[test]
    fn tolerates_crossref_null_date_parts() {
        let items = parse_search_response(
            r#"{"message":{"items":[{"DOI":"10.1000/null-date","title":["Nullable Date"],"issued":{"date-parts":[[null,5,1]]}}]}}"#,
        )
        .expect("null date parts should parse");

        assert_eq!(items.len(), 1);
        assert!(matches!(
            items[0].metadata,
            SourceMetadata::Bibliography { year: None, .. }
        ));
    }

    #[test]
    fn rejects_invalid_crossref_response() {
        let err = parse_search_response(r#"{"message":{"items":"not an array"}}"#)
            .expect_err("invalid response should fail");

        assert!(err.to_string().contains("JSON"));
    }
}
