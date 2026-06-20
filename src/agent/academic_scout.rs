use chrono::Utc;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::error::{AppError, Result};
use crate::model::{
    QueryAttempt, QueryPortfolio, SearchQuery, SourceItem, SourceKind, SourceQueryLineage,
};
use crate::sources::{crossref, dblp, openalex, semantic_scholar};

pub async fn scout_academic_extra(
    portfolio: &[QueryPortfolio],
    config: &AppConfig,
    academic_budget: usize,
    round: usize,
) -> Result<(Vec<SourceItem>, Vec<QueryAttempt>, Vec<SourceQueryLineage>)> {
    let mut items = Vec::new();
    let mut attempts = Vec::new();
    let mut source_lineage = Vec::new();
    let budget = academic_budget.max(1);

    for portfolio_item in portfolio {
        for query in academic_queries(portfolio_item) {
            let search_query = SearchQuery {
                topic: query.clone(),
                github_limit: budget,
                arxiv_limit: budget,
            };

            for adapter in AcademicAdapter::all() {
                let started_at = Utc::now();
                if let Some(reason) = adapter.unavailable_reason(config) {
                    attempts.push(skipped_source_attempt(
                        adapter,
                        &query,
                        &portfolio_item.chapter_id,
                        round,
                        started_at,
                        reason,
                    ));
                    continue;
                }
                let result = adapter.search(&search_query, config).await;
                let (adapter_items, attempt) = run_source_attempt(
                    adapter,
                    &query,
                    &portfolio_item.chapter_id,
                    round,
                    started_at,
                    result,
                );
                source_lineage.extend(lineage_for_items(
                    &attempt.query_id,
                    &portfolio_item.chapter_id,
                    &adapter_items,
                ));
                items.extend(adapter_items);
                attempts.push(attempt);
            }
        }
    }

    Ok((items, attempts, source_lineage))
}

pub fn academic_source_names() -> Vec<&'static str> {
    AcademicAdapter::all()
        .into_iter()
        .map(AcademicAdapter::source_name)
        .collect()
}

fn academic_queries(item: &QueryPortfolio) -> Vec<String> {
    let mut queries = item.arxiv_queries.clone();
    for query in &item.github_queries {
        if !queries.iter().any(|existing| existing == query) {
            queries.push(query.clone());
        }
    }
    queries
        .into_iter()
        .map(|query| query.trim().to_string())
        .filter(|query| !query.is_empty())
        .collect()
}

#[derive(Debug, Clone, Copy)]
enum AcademicAdapter {
    SemanticScholar,
    Dblp,
    OpenAlex,
    Crossref,
}

impl AcademicAdapter {
    fn all() -> [Self; 4] {
        [
            Self::SemanticScholar,
            Self::Dblp,
            Self::OpenAlex,
            Self::Crossref,
        ]
    }

    fn source_name(self) -> &'static str {
        match self {
            Self::SemanticScholar => "semantic_scholar",
            Self::Dblp => "dblp",
            Self::OpenAlex => "openalex",
            Self::Crossref => "crossref",
        }
    }

    fn source_kind(self) -> SourceKind {
        match self {
            Self::SemanticScholar | Self::OpenAlex => SourceKind::AcademicIndex,
            Self::Dblp | Self::Crossref => SourceKind::Bibliography,
        }
    }

    fn attempt_prefix(self) -> &'static str {
        match self {
            Self::SemanticScholar => "ss",
            Self::Dblp => "db",
            Self::OpenAlex => "oa",
            Self::Crossref => "cr",
        }
    }

    fn unavailable_reason(self, config: &AppConfig) -> Option<&'static str> {
        match self {
            Self::OpenAlex
                if config
                    .openalex_api_key
                    .as_deref()
                    .is_none_or(|key| key.trim().is_empty()) =>
            {
                Some("OPENALEX_API_KEY is not configured; skipped OpenAlex search")
            }
            _ => None,
        }
    }

    async fn search(self, query: &SearchQuery, config: &AppConfig) -> Result<Vec<SourceItem>> {
        match self {
            Self::SemanticScholar => semantic_scholar::search_papers(query, config).await,
            Self::Dblp => dblp::search_publications(query, config).await,
            Self::OpenAlex => openalex::search_works(query, config).await,
            Self::Crossref => crossref::search_works(query, config).await,
        }
    }
}

fn skipped_source_attempt(
    adapter: AcademicAdapter,
    query: &str,
    chapter_id: &str,
    round: usize,
    started_at: chrono::DateTime<Utc>,
    reason: &'static str,
) -> QueryAttempt {
    QueryAttempt {
        query_id: format!("{}-{}", adapter.attempt_prefix(), Uuid::new_v4()),
        source: adapter.source_name().to_string(),
        query: query.to_string(),
        chapter_id: chapter_id.to_string(),
        round,
        started_at,
        finished_at: Some(Utc::now()),
        result_count: 0,
        source_kind: Some(adapter.source_kind()),
        http_status: None,
        rate_limit_info: None,
        parser_error: None,
        is_citeable: false,
        error: Some(reason.to_string()),
    }
}

fn run_source_attempt(
    adapter: AcademicAdapter,
    query: &str,
    chapter_id: &str,
    round: usize,
    started_at: chrono::DateTime<Utc>,
    result: Result<Vec<SourceItem>>,
) -> (Vec<SourceItem>, QueryAttempt) {
    let query_id = format!("{}-{}", adapter.attempt_prefix(), Uuid::new_v4());
    match result {
        Ok(items) => {
            let result_count = items.len();
            (
                items,
                QueryAttempt {
                    query_id,
                    source: adapter.source_name().to_string(),
                    query: query.to_string(),
                    chapter_id: chapter_id.to_string(),
                    round,
                    started_at,
                    finished_at: Some(Utc::now()),
                    result_count,
                    source_kind: Some(adapter.source_kind()),
                    http_status: None,
                    rate_limit_info: None,
                    parser_error: None,
                    is_citeable: result_count > 0,
                    error: None,
                },
            )
        }
        Err(err) => (Vec::new(), {
            let http_status = http_status_from_error(&err);
            let rate_limit_info = rate_limit_info_from_error(&err);
            let parser_error = parser_error_from_error(&err);
            QueryAttempt {
                query_id,
                source: adapter.source_name().to_string(),
                query: query.to_string(),
                chapter_id: chapter_id.to_string(),
                round,
                started_at,
                finished_at: Some(Utc::now()),
                result_count: 0,
                source_kind: Some(adapter.source_kind()),
                http_status,
                rate_limit_info,
                parser_error,
                is_citeable: false,
                error: Some(err.to_string()),
            }
        }),
    }
}

fn lineage_for_items(
    query_id: &str,
    chapter_id: &str,
    items: &[SourceItem],
) -> Vec<SourceQueryLineage> {
    items
        .iter()
        .map(|item| SourceQueryLineage {
            lineage_id: format!("lin-{query_id}-{}", item.id),
            source_item_id: item.id.clone(),
            chapter_id: Some(chapter_id.to_string()),
            source_kind: Some(item.kind),
            query_attempt_ids: vec![query_id.to_string()],
            returned_item_ids: vec![item.id.clone()],
            merged_from_item_ids: Vec::new(),
        })
        .collect()
}

fn http_status_from_error(err: &AppError) -> Option<u16> {
    match err {
        AppError::HttpStatus { status, .. } => Some(*status),
        AppError::RateLimit { .. } => Some(429),
        _ => None,
    }
}

fn rate_limit_info_from_error(err: &AppError) -> Option<String> {
    match err {
        AppError::RateLimit { reset, .. } if !reset.trim().is_empty() => Some(reset.clone()),
        AppError::RateLimit { .. } => Some("rate limited".to_string()),
        _ => None,
    }
}

fn parser_error_from_error(err: &AppError) -> Option<String> {
    match err {
        AppError::Json(err) => Some(err.to_string()),
        AppError::Xml(err) => Some(err.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{
        academic_queries, academic_source_names, run_source_attempt, skipped_source_attempt,
        AcademicAdapter,
    };
    use crate::config::AppConfig;
    use crate::error::AppError;
    use crate::model::{QueryPortfolio, SourceKind};

    #[test]
    fn academic_queries_deduplicate_arxiv_and_github_queries() {
        let item = QueryPortfolio {
            chapter_id: "ch-1".to_string(),
            github_queries: vec!["rust agent".to_string(), "code agent".to_string()],
            arxiv_queries: vec!["rust agent".to_string()],
            rationale: "test".to_string(),
            budget: 3,
        };

        let queries = academic_queries(&item);

        assert_eq!(queries, vec!["rust agent", "code agent"]);
    }

    #[test]
    fn academic_registry_uses_stable_source_order() {
        assert_eq!(
            academic_source_names(),
            vec!["semantic_scholar", "dblp", "openalex", "crossref"]
        );
    }

    #[test]
    fn failed_academic_attempt_records_status_and_rate_limit() {
        let (_items, attempt) = run_source_attempt(
            AcademicAdapter::SemanticScholar,
            "rust agent",
            "ch-1",
            1,
            Utc::now(),
            Err(AppError::RateLimit {
                service: "Semantic Scholar",
                reset: "; retry after about 60s".to_string(),
            }),
        );

        assert_eq!(attempt.result_count, 0);
        assert_eq!(attempt.source_kind, Some(SourceKind::AcademicIndex));
        assert_eq!(attempt.http_status, Some(429));
        assert_eq!(
            attempt.rate_limit_info.as_deref(),
            Some("; retry after about 60s")
        );
        assert!(attempt.error.is_some());
        assert!(!attempt.is_citeable);
    }

    #[test]
    fn failed_academic_attempt_records_parser_error() {
        let err = serde_json::from_str::<serde_json::Value>("{").expect_err("invalid json");
        let (_items, attempt) = run_source_attempt(
            AcademicAdapter::Dblp,
            "rust agent",
            "ch-1",
            1,
            Utc::now(),
            Err(AppError::Json(err)),
        );

        assert_eq!(attempt.source_kind, Some(SourceKind::Bibliography));
        assert_eq!(attempt.http_status, None);
        assert!(attempt.parser_error.is_some());
        assert!(attempt.error.is_some());
        assert!(!attempt.is_citeable);
    }

    #[test]
    fn empty_successful_attempt_is_not_citeable() {
        let (_items, attempt) = run_source_attempt(
            AcademicAdapter::Crossref,
            "rust agent",
            "ch-1",
            1,
            Utc::now(),
            Ok(Vec::new()),
        );

        assert_eq!(attempt.result_count, 0);
        assert_eq!(attempt.source_kind, Some(SourceKind::Bibliography));
        assert!(attempt.error.is_none());
        assert!(!attempt.is_citeable);
    }

    #[test]
    fn openalex_without_key_is_recorded_as_skipped_not_http_failure() {
        let config = test_config();

        let reason = AcademicAdapter::OpenAlex
            .unavailable_reason(&config)
            .expect("OpenAlex should require an explicit key");
        let attempt = skipped_source_attempt(
            AcademicAdapter::OpenAlex,
            "rust agent",
            "ch-1",
            1,
            Utc::now(),
            reason,
        );

        assert_eq!(attempt.source, "openalex");
        assert_eq!(attempt.source_kind, Some(SourceKind::AcademicIndex));
        assert_eq!(attempt.http_status, None);
        assert_eq!(attempt.parser_error, None);
        assert!(attempt.error.as_deref().unwrap_or("").contains("skipped"));
        assert!(!attempt.is_citeable);
    }

    fn test_config() -> AppConfig {
        AppConfig {
            github_token: None,
            semantic_scholar_api_key: None,
            openalex_api_key: None,
            crossref_mailto: None,
            output: std::env::temp_dir(),
            cache_dir: std::env::temp_dir(),
            session_dir: std::env::temp_dir(),
            tags_file: None,
            use_cache: false,
            cache_ttl_hours: 24,
            timeout_secs: 30,
            enrich: false,
        }
    }
}
