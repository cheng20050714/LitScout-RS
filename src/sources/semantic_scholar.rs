#![allow(dead_code)]

use std::time::Duration;

use reqwest::header::{HeaderMap, RETRY_AFTER};
use serde::{Deserialize, Deserializer};

use crate::config::AppConfig;
use crate::error::{AppError, Result};
use crate::model::{SearchQuery, SourceItem, SourceKind, SourceMetadata};

const SEMANTIC_SCHOLAR_SEARCH_URL: &str = "https://api.semanticscholar.org/graph/v1/paper/search";
const USER_AGENT_VALUE: &str = "litscout-rs/0.1";
const FIELDS: &str =
    "paperId,title,abstract,year,venue,citationCount,url,authors,externalIds,fieldsOfStudy";

pub async fn search_papers(query: &SearchQuery, config: &AppConfig) -> Result<Vec<SourceItem>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout_secs))
        .user_agent(USER_AGENT_VALUE)
        .build()?;
    let limit = query.arxiv_limit.clamp(1, 100).to_string();
    let mut request = client.get(SEMANTIC_SCHOLAR_SEARCH_URL).query(&[
        ("query", query.topic.as_str()),
        ("limit", limit.as_str()),
        ("fields", FIELDS),
    ]);

    if let Some(key) = &config.semantic_scholar_api_key {
        request = request.header("x-api-key", key);
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
            service: "Semantic Scholar",
            reset: retry_after_message(&headers),
        });
    }

    Err(AppError::HttpStatus {
        service: "Semantic Scholar",
        status: status.as_u16(),
        body,
    })
}

pub fn parse_search_response(body: &str) -> Result<Vec<SourceItem>> {
    let response: SemanticScholarSearchResponse = serde_json::from_str(body)?;
    Ok(response
        .data
        .into_iter()
        .filter_map(SemanticScholarPaper::into_source_item)
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
struct SemanticScholarSearchResponse {
    #[serde(default)]
    data: Vec<SemanticScholarPaper>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SemanticScholarPaper {
    paper_id: Option<String>,
    title: Option<String>,
    #[serde(default)]
    #[serde(rename = "abstract")]
    abstract_text: Option<String>,
    year: Option<i32>,
    venue: Option<String>,
    citation_count: Option<u64>,
    url: Option<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    authors: Vec<SemanticScholarAuthor>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    external_ids: SemanticScholarExternalIds,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    fields_of_study: Vec<String>,
}

impl SemanticScholarPaper {
    fn into_source_item(self) -> Option<SourceItem> {
        let paper_id = clean_required(self.paper_id)?;
        let title = clean_required(self.title)?;
        let authors = self
            .authors
            .into_iter()
            .filter_map(|author| clean_required(author.name))
            .collect::<Vec<_>>();
        let venue = clean_optional(self.venue);
        let external_ids = semantic_scholar_external_ids(&self.external_ids);
        let doi = clean_optional(self.external_ids.doi.clone());
        let summary = clean_optional(self.abstract_text)
            .unwrap_or_else(|| bibliographic_summary(&authors, venue.as_deref(), self.year));
        let evidence_snippet = excerpt(&summary, 420);
        let url = self
            .url
            .unwrap_or_else(|| format!("https://www.semanticscholar.org/paper/{paper_id}"));

        Some(SourceItem {
            id: format!("semantic_scholar:{paper_id}"),
            kind: SourceKind::AcademicIndex,
            title,
            url,
            summary,
            evidence_snippet,
            tags: self.fields_of_study,
            score: 0.0,
            score_reasons: Vec::new(),
            classification_reasons: Vec::new(),
            score_breakdown: Default::default(),
            published_or_updated_at: None,
            metadata: SourceMetadata::AcademicIndex {
                authors,
                venue,
                year: self.year,
                doi,
                citation_count: self.citation_count,
                native_id: paper_id,
                source_name: "semantic_scholar".to_string(),
                external_ids,
            },
        })
    }
}

#[derive(Debug, Default, Deserialize)]
struct SemanticScholarExternalIds {
    #[serde(rename = "DOI")]
    doi: Option<String>,
    #[serde(rename = "ArXiv")]
    arxiv: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SemanticScholarAuthor {
    name: Option<String>,
}

fn deserialize_null_default<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

fn clean_required(value: Option<String>) -> Option<String> {
    clean_optional(value).filter(|value| !value.is_empty())
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|value| !value.is_empty())
}

fn semantic_scholar_external_ids(external_ids: &SemanticScholarExternalIds) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(doi) = clean_optional(external_ids.doi.clone()) {
        ids.push(format!("doi:{}", doi.to_ascii_lowercase()));
    }
    if let Some(arxiv) = clean_optional(external_ids.arxiv.clone()) {
        ids.push(format!("arxiv:{}", crate::model::stable_arxiv_id(&arxiv)));
    }
    ids
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
    fn parses_semantic_scholar_fixture() {
        let body = include_str!("../../tests/fixtures/semantic_scholar_search.json");
        let items = parse_search_response(body).expect("fixture should parse");

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "semantic_scholar:abc123");
        assert_eq!(items[0].kind, SourceKind::AcademicIndex);
        assert_eq!(items[0].title, "Tool Calling Agents in Rust");
        assert!(items[0].summary.contains("tool-calling agents"));
        assert!(matches!(
            items[0].metadata,
            SourceMetadata::AcademicIndex {
                citation_count: Some(42),
                ref source_name,
                ..
            } if source_name == "semantic_scholar"
        ));
    }

    #[test]
    fn parses_empty_semantic_scholar_response() {
        let items =
            parse_search_response(r#"{"total":0,"data":[]}"#).expect("empty response parses");

        assert!(items.is_empty());
    }

    #[test]
    fn skips_semantic_scholar_records_without_required_fields() {
        let items = parse_search_response(
            r#"{"data":[{"paperId":"missing-title"},{"title":"missing id"}]}"#,
        )
        .expect("malformed records should be skipped");

        assert!(items.is_empty());
    }

    #[test]
    fn tolerates_semantic_scholar_null_lists() {
        let items = parse_search_response(
            r#"{"data":[{
                "paperId":"abc-null",
                "title":"Nullable Semantic Scholar Lists",
                "authors":null,
                "fieldsOfStudy":null,
                "externalIds":{},
                "abstract":null
            }]}"#,
        )
        .expect("null list fields should parse");

        assert_eq!(items.len(), 1);
        assert!(items[0].tags.is_empty());
        assert!(items[0].summary.contains("Bibliographic metadata"));
    }
}
