#![allow(dead_code)]

use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::header::{HeaderMap, ACCEPT, AUTHORIZATION};
use serde::Deserialize;
use tracing::warn;

use crate::config::AppConfig;
use crate::error::{AppError, Result};
use crate::model::{GitHubRepo, SearchQuery};

const GITHUB_REPOSITORY_SEARCH_URL: &str = "https://api.github.com/search/repositories";
const GITHUB_REPOSITORY_README_URL_BASE: &str = "https://api.github.com/repos";
const USER_AGENT_VALUE: &str = "litscout-rs/0.1";
const README_EXCERPT_MAX_CHARS: usize = 2000;
const GITHUB_README_DELAY: Duration = Duration::from_millis(200);

pub async fn search_repositories(
    query: &SearchQuery,
    config: &AppConfig,
) -> Result<Vec<GitHubRepo>> {
    let per_page = query.github_limit.clamp(1, 100).to_string();
    let github_query = format!("{} in:name,description", query.topic);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout_secs))
        .user_agent(USER_AGENT_VALUE)
        .build()?;

    let mut request = client
        .get(GITHUB_REPOSITORY_SEARCH_URL)
        .header(ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .query(&[
            ("q", github_query.as_str()),
            ("sort", "stars"),
            ("order", "desc"),
            ("per_page", per_page.as_str()),
            ("page", "1"),
        ]);

    if let Some(token) = &config.github_token {
        request = request.header(AUTHORIZATION, format!("Bearer {token}"));
    }

    let response = request.send().await?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.text().await?;

    if !status.is_success() {
        if is_rate_limited(status.as_u16(), &headers) {
            return Err(AppError::RateLimit {
                service: "GitHub",
                reset: rate_limit_reset_message(&headers),
            });
        }
        return Err(AppError::HttpStatus {
            service: "GitHub",
            status: status.as_u16(),
            body,
        });
    }

    parse_search_response(&body)
}

pub fn parse_search_response(body: &str) -> Result<Vec<GitHubRepo>> {
    let response: GitHubSearchResponse = serde_json::from_str(body)?;
    if response.incomplete_results {
        tracing::warn!("GitHub search returned incomplete results");
    }
    Ok(response.items.into_iter().map(GitHubRepo::from).collect())
}

pub async fn fetch_readme(owner: &str, repo: &str, config: &AppConfig) -> Result<Option<String>> {
    let url = format!("{GITHUB_REPOSITORY_README_URL_BASE}/{owner}/{repo}/readme");
    fetch_readme_url(&url, config).await
}

pub async fn enrich_repositories(repos: &mut [GitHubRepo], config: &AppConfig) {
    if !config.enrich {
        return;
    }

    for repo in repos {
        match fetch_readme(&repo.owner, &repo.name, config).await {
            Ok(Some(readme)) => {
                repo.readme_excerpt = Some(readme);
            }
            Ok(None) => {}
            Err(err) => {
                warn!(
                    "GitHub README enrich skipped for `{}`: {err}",
                    repo.full_name
                );
            }
        }
        tokio::time::sleep(GITHUB_README_DELAY).await;
    }
}

async fn fetch_readme_url(url: &str, config: &AppConfig) -> Result<Option<String>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout_secs))
        .user_agent(USER_AGENT_VALUE)
        .build()?;

    let mut request = client
        .get(url)
        .header(ACCEPT, "application/vnd.github.raw+json")
        .header("X-GitHub-Api-Version", "2022-11-28");

    if let Some(token) = &config.github_token {
        request = request.header(AUTHORIZATION, format!("Bearer {token}"));
    }

    let response = request.send().await?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.text().await?;

    readme_excerpt_from_response(status.as_u16(), &headers, body)
}

fn readme_excerpt_from_response(
    status: u16,
    headers: &HeaderMap,
    body: String,
) -> Result<Option<String>> {
    if status == 404 {
        return Ok(None);
    }

    if !(200..300).contains(&status) {
        if is_rate_limited(status, headers) {
            return Err(AppError::RateLimit {
                service: "GitHub",
                reset: rate_limit_reset_message(headers),
            });
        }
        return Err(AppError::HttpStatus {
            service: "GitHub",
            status,
            body,
        });
    }

    let excerpt = truncate_chars(&body, README_EXCERPT_MAX_CHARS);
    Ok((!excerpt.trim().is_empty()).then_some(excerpt))
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    trimmed.chars().take(max_chars).collect()
}

fn is_rate_limited(status: u16, headers: &HeaderMap) -> bool {
    if status == 429 {
        return true;
    }

    status == 403
        && headers
            .get("x-ratelimit-remaining")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|remaining| remaining == "0")
}

fn rate_limit_reset_message(headers: &HeaderMap) -> String {
    headers
        .get("x-ratelimit-reset")
        .and_then(|value| value.to_str().ok())
        .map(|reset| format!("; reset epoch: {reset}"))
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
struct GitHubSearchResponse {
    #[serde(default)]
    incomplete_results: bool,
    #[serde(default)]
    items: Vec<GitHubRepoItem>,
}

#[derive(Debug, Deserialize)]
struct GitHubRepoItem {
    name: String,
    full_name: String,
    html_url: String,
    description: Option<String>,
    stargazers_count: u64,
    forks_count: u64,
    language: Option<String>,
    updated_at: DateTime<Utc>,
    #[serde(default)]
    topics: Vec<String>,
    owner: GitHubOwner,
}

#[derive(Debug, Deserialize)]
struct GitHubOwner {
    login: String,
}

impl From<GitHubRepoItem> for GitHubRepo {
    fn from(item: GitHubRepoItem) -> Self {
        Self {
            owner: item.owner.login,
            name: item.name,
            full_name: item.full_name,
            html_url: item.html_url,
            description: item.description,
            stars: item.stargazers_count,
            forks: item.forks_count,
            language: item.language,
            updated_at: item.updated_at,
            topics: item.topics,
            readme_excerpt: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use reqwest::header::HeaderMap;

    use super::{parse_search_response, readme_excerpt_from_response, truncate_chars};

    #[test]
    fn parses_github_repository_search_fixture() {
        let body = include_str!("../../tests/fixtures/github_search.json");
        let repos = parse_search_response(body).expect("fixture should parse");

        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].owner, "rust-lang");
        assert_eq!(repos[0].name, "rust");
        assert_eq!(repos[0].full_name, "rust-lang/rust");
        assert_eq!(repos[0].stars, 100000);
        assert_eq!(repos[0].forks, 12000);
        assert_eq!(repos[0].language.as_deref(), Some("Rust"));
        assert_eq!(repos[0].topics, vec!["compiler", "rust"]);
        assert!(repos[1].topics.is_empty());
    }

    #[test]
    fn truncates_readme_excerpt_by_character_boundary() {
        let text = format!("{}{}", "好".repeat(2001), "tail");

        let excerpt = truncate_chars(&text, 2000);

        assert_eq!(excerpt.chars().count(), 2000);
        assert!(excerpt.chars().all(|ch| ch == '好'));
    }

    #[test]
    fn extracts_raw_readme_excerpt_from_api_response() {
        let readme = readme_excerpt_from_response(
            200,
            &HeaderMap::new(),
            "# Project\n\nREADME body.".to_string(),
        )
        .expect("README response should parse");

        assert_eq!(readme.as_deref(), Some("# Project\n\nREADME body."));
    }

    #[test]
    fn readme_404_is_optional() {
        let readme = readme_excerpt_from_response(
            404,
            &HeaderMap::new(),
            "{\"message\":\"Not Found\"}".to_string(),
        )
        .expect("404 should not fail README enrich");

        assert!(readme.is_none());
    }
}
