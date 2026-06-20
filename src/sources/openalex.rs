#![allow(dead_code)]

use std::time::Duration;

use reqwest::header::{HeaderMap, RETRY_AFTER};
use serde::{Deserialize, Deserializer};

use crate::config::AppConfig;
use crate::error::{AppError, Result};
use crate::model::{SearchQuery, SourceItem, SourceKind, SourceMetadata};

const OPENALEX_WORKS_URL: &str = "https://api.openalex.org/works";
const USER_AGENT_VALUE: &str = "litscout-rs/0.1";

pub async fn search_works(query: &SearchQuery, config: &AppConfig) -> Result<Vec<SourceItem>> {
    let Some(api_key) = config
        .openalex_api_key
        .as_deref()
        .filter(|key| !key.trim().is_empty())
    else {
        return Err(AppError::InvalidConfig(
            "OPENALEX_API_KEY is required for OpenAlex academic extra searches".to_string(),
        ));
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout_secs))
        .user_agent(USER_AGENT_VALUE)
        .build()?;
    let per_page = query.arxiv_limit.clamp(1, 100).to_string();
    let response = client
        .get(OPENALEX_WORKS_URL)
        .query(&[
            ("search", query.topic.as_str()),
            ("per_page", per_page.as_str()),
            ("api_key", api_key),
            ("select", "id,doi,title,display_name,authorships,primary_location,publication_year,cited_by_count,concepts,abstract_inverted_index"),
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
            service: "OpenAlex",
            reset: retry_after_message(&headers),
        });
    }

    Err(AppError::HttpStatus {
        service: "OpenAlex",
        status: status.as_u16(),
        body,
    })
}

pub fn parse_search_response(body: &str) -> Result<Vec<SourceItem>> {
    let response: OpenAlexSearchResponse = serde_json::from_str(body)?;
    Ok(response
        .results
        .into_iter()
        .filter_map(OpenAlexWork::into_source_item)
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
struct OpenAlexSearchResponse {
    #[serde(default)]
    results: Vec<OpenAlexWork>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexWork {
    id: Option<String>,
    doi: Option<String>,
    title: Option<String>,
    display_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    authorships: Vec<OpenAlexAuthorship>,
    primary_location: Option<OpenAlexPrimaryLocation>,
    publication_year: Option<i32>,
    cited_by_count: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    concepts: Vec<OpenAlexConcept>,
    #[serde(default)]
    abstract_inverted_index: Option<serde_json::Value>,
}

impl OpenAlexWork {
    fn into_source_item(self) -> Option<SourceItem> {
        let id = clean_required(self.id)?;
        let native_id = openalex_native_id(&id);
        let title = clean_required(self.title.or(self.display_name))?;
        let authors = self
            .authorships
            .into_iter()
            .filter_map(|authorship| authorship.author.display_name)
            .filter_map(|name| clean_required(Some(name)))
            .collect::<Vec<_>>();
        let doi = normalize_doi(self.doi.as_deref());
        let venue = self
            .primary_location
            .as_ref()
            .and_then(|location| location.source.as_ref())
            .and_then(|source| clean_optional(source.display_name.clone()));
        let url = self
            .primary_location
            .as_ref()
            .and_then(|location| clean_optional(location.landing_page_url.clone()))
            .or_else(|| doi.as_ref().map(|doi| format!("https://doi.org/{doi}")))
            .unwrap_or_else(|| id.clone());
        let abstract_text = abstract_from_inverted_index(self.abstract_inverted_index.as_ref());
        let summary = abstract_text.unwrap_or_else(|| {
            bibliographic_summary(&authors, venue.as_deref(), self.publication_year)
        });
        let tags = self
            .concepts
            .into_iter()
            .filter_map(|concept| clean_optional(concept.display_name))
            .collect::<Vec<_>>();
        let mut external_ids = Vec::new();
        external_ids.push(format!("openalex:{native_id}"));
        if let Some(doi) = &doi {
            external_ids.push(format!("doi:{doi}"));
        }

        Some(SourceItem {
            id: format!("openalex:{native_id}"),
            kind: SourceKind::AcademicIndex,
            title,
            url,
            summary: summary.clone(),
            evidence_snippet: excerpt(&summary, 420),
            tags,
            score: 0.0,
            score_reasons: Vec::new(),
            classification_reasons: Vec::new(),
            score_breakdown: Default::default(),
            published_or_updated_at: None,
            metadata: SourceMetadata::AcademicIndex {
                authors,
                venue,
                year: self.publication_year,
                doi,
                citation_count: self.cited_by_count,
                native_id,
                source_name: "openalex".to_string(),
                external_ids,
            },
        })
    }
}

#[derive(Debug, Deserialize)]
struct OpenAlexAuthorship {
    author: OpenAlexAuthor,
}

#[derive(Debug, Deserialize)]
struct OpenAlexAuthor {
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexPrimaryLocation {
    landing_page_url: Option<String>,
    source: Option<OpenAlexSource>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexSource {
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexConcept {
    display_name: Option<String>,
}

fn clean_required(value: Option<String>) -> Option<String> {
    clean_optional(value).filter(|value| !value.is_empty())
}

fn deserialize_null_default<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|value| !value.is_empty())
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

fn openalex_native_id(id: &str) -> String {
    id.rsplit_once('/')
        .map(|(_, native)| native)
        .unwrap_or(id)
        .trim()
        .to_string()
}

fn abstract_from_inverted_index(value: Option<&serde_json::Value>) -> Option<String> {
    let object = value?.as_object()?;
    let mut positioned = object
        .iter()
        .flat_map(|(word, positions)| {
            positions
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|position| position.as_u64())
                .map(|position| (position as usize, word.clone()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    positioned.sort_by_key(|(position, _)| *position);
    let text = positioned
        .into_iter()
        .map(|(_, word)| word)
        .collect::<Vec<_>>()
        .join(" ");
    clean_optional(Some(text))
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
    fn parses_openalex_fixture() {
        let body = include_str!("../../tests/fixtures/openalex_search.json");
        let items = parse_search_response(body).expect("fixture should parse");

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "openalex:W1234567890");
        assert_eq!(items[0].kind, SourceKind::AcademicIndex);
        assert_eq!(items[0].title, "Tool Calling Agents in Rust");
        assert!(items[0].summary.contains("tool calling agents"));
        assert!(matches!(
            items[0].metadata,
            SourceMetadata::AcademicIndex {
                citation_count: Some(128),
                ref source_name,
                ref external_ids,
                ..
            } if source_name == "openalex" && external_ids.iter().any(|id| id == "doi:10.1234/tool-agent")
        ));
    }

    #[test]
    fn parses_empty_openalex_response() {
        let items = parse_search_response(r#"{"results":[]}"#).expect("empty response parses");

        assert!(items.is_empty());
    }

    #[test]
    fn skips_openalex_records_without_required_fields() {
        let items = parse_search_response(
            r#"{"results":[{"id":"https://openalex.org/W1"},{"title":"missing id"}]}"#,
        )
        .expect("partial response parses");

        assert!(items.is_empty());
    }

    #[test]
    fn tolerates_openalex_null_lists() {
        let items = parse_search_response(
            r#"{"results":[{
                "id":"https://openalex.org/WNULL",
                "title":"Nullable OpenAlex Lists",
                "authorships":null,
                "concepts":null,
                "abstract_inverted_index":null
            }]}"#,
        )
        .expect("null list fields should parse");

        assert_eq!(items.len(), 1);
        assert!(items[0].tags.is_empty());
        assert!(items[0].summary.contains("Bibliographic metadata"));
    }

    #[test]
    fn rejects_invalid_openalex_response() {
        let err = parse_search_response(r#"{"results":"not an array"}"#)
            .expect_err("invalid response should fail");

        assert!(err.to_string().contains("JSON"));
    }
}
