#![allow(dead_code)]

use std::time::Duration;

use reqwest::header::{HeaderMap, RETRY_AFTER};
use serde::Deserialize;

use crate::config::AppConfig;
use crate::error::{AppError, Result};
use crate::model::{SearchQuery, SourceItem, SourceKind, SourceMetadata};

const DBLP_SEARCH_URL: &str = "https://dblp.org/search/publ/api";
const USER_AGENT_VALUE: &str = "litscout-rs/0.1";

pub async fn search_publications(
    query: &SearchQuery,
    config: &AppConfig,
) -> Result<Vec<SourceItem>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout_secs))
        .user_agent(USER_AGENT_VALUE)
        .build()?;
    let hit_count = query.arxiv_limit.clamp(1, 100).to_string();
    let response = client
        .get(DBLP_SEARCH_URL)
        .query(&[
            ("q", query.topic.as_str()),
            ("format", "json"),
            ("h", hit_count.as_str()),
        ])
        .send()
        .await?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.text().await?;

    if status.is_success() {
        return parse_search_response(&body);
    }

    if status.as_u16() == 429 {
        return Err(AppError::RateLimit {
            service: "DBLP",
            reset: retry_after_message(&headers),
        });
    }

    Err(AppError::HttpStatus {
        service: "DBLP",
        status: status.as_u16(),
        body,
    })
}

pub fn parse_search_response(body: &str) -> Result<Vec<SourceItem>> {
    let response: DblpSearchResponse = serde_json::from_str(body)?;
    let hits = response
        .result
        .hits
        .hit
        .unwrap_or_default()
        .into_iter()
        .filter_map(DblpHit::into_source_item)
        .collect();
    Ok(hits)
}

fn retry_after_message(headers: &HeaderMap) -> String {
    headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .map(|seconds| format!("; retry after about {seconds}s"))
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
struct DblpSearchResponse {
    result: DblpResult,
}

#[derive(Debug, Deserialize)]
struct DblpResult {
    hits: DblpHits,
}

#[derive(Debug, Deserialize)]
struct DblpHits {
    #[serde(default)]
    hit: Option<Vec<DblpHit>>,
}

#[derive(Debug, Deserialize)]
struct DblpHit {
    info: DblpInfo,
}

impl DblpHit {
    fn into_source_item(self) -> Option<SourceItem> {
        let key = clean_required(self.info.key)?;
        let title = clean_required(self.info.title)?;
        let authors = self.info.authors.into_names();
        let venue = clean_optional(self.info.venue);
        let year = self.info.year.and_then(|year| year.parse::<i32>().ok());
        let doi = clean_optional(self.info.doi);
        let url = self
            .info
            .ee
            .or(self.info.url)
            .unwrap_or_else(|| format!("https://dblp.org/rec/{key}"));
        let summary = bibliographic_summary(&authors, venue.as_deref(), year);

        Some(SourceItem {
            id: format!("dblp:{key}"),
            kind: SourceKind::Bibliography,
            title,
            url,
            summary: summary.clone(),
            evidence_snippet: summary,
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
                doi,
                citation_count: None,
                native_id: key,
                source_name: "dblp".to_string(),
            },
        })
    }
}

#[derive(Debug, Deserialize)]
struct DblpInfo {
    key: Option<String>,
    title: Option<String>,
    venue: Option<String>,
    year: Option<String>,
    doi: Option<String>,
    ee: Option<String>,
    url: Option<String>,
    #[serde(default)]
    authors: DblpAuthors,
}

#[derive(Debug, Default, Deserialize)]
struct DblpAuthors {
    #[serde(default)]
    author: DblpAuthorList,
}

impl DblpAuthors {
    fn into_names(self) -> Vec<String> {
        match self.author {
            DblpAuthorList::One(author) => clean_optional(Some(author)).into_iter().collect(),
            DblpAuthorList::Many(authors) => authors
                .into_iter()
                .filter_map(|author| clean_optional(Some(author)))
                .collect(),
            DblpAuthorList::None => Vec::new(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(untagged)]
enum DblpAuthorList {
    One(String),
    Many(Vec<String>),
    #[default]
    None,
}

fn clean_required(value: Option<String>) -> Option<String> {
    clean_optional(value).filter(|value| !value.is_empty())
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| {
            value
                .replace("<b>", "")
                .replace("</b>", "")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|value| !value.is_empty())
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

#[cfg(test)]
mod tests {
    use super::parse_search_response;
    use crate::model::{SourceKind, SourceMetadata};

    #[test]
    fn parses_dblp_fixture() {
        let body = include_str!("../../tests/fixtures/dblp_search.json");
        let items = parse_search_response(body).expect("fixture should parse");

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "dblp:conf/test/AgentRust2026");
        assert_eq!(items[0].kind, SourceKind::Bibliography);
        assert_eq!(items[0].title, "Tool Calling Agents in Rust.");
        assert!(items[0].summary.contains("Bibliographic metadata"));
        assert!(matches!(
            items[0].metadata,
            SourceMetadata::Bibliography {
                year: Some(2026),
                ref source_name,
                ..
            } if source_name == "dblp"
        ));
    }

    #[test]
    fn parses_empty_dblp_response() {
        let items =
            parse_search_response(r#"{"result":{"hits":{"@total":"0"}}}"#).expect("empty parses");

        assert!(items.is_empty());
    }

    #[test]
    fn skips_dblp_records_without_required_fields() {
        let items = parse_search_response(
            r#"{"result":{"hits":{"hit":[{"info":{"key":"missing-title"}}]}}}"#,
        )
        .expect("malformed records should be skipped");

        assert!(items.is_empty());
    }
}
