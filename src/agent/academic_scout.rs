use chrono::Utc;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::error::{AppError, Result};
use crate::model::{QueryAttempt, QueryPortfolio, SearchQuery, SourceItem, SourceQueryLineage};
use crate::sources::{dblp, semantic_scholar};

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

            let semantic_started_at = Utc::now();
            let semantic_result = semantic_scholar::search_papers(&search_query, config).await;
            let (semantic_items, semantic_attempt) = run_source_attempt(
                "semantic_scholar",
                &query,
                &portfolio_item.chapter_id,
                round,
                semantic_started_at,
                semantic_result,
            );
            source_lineage.extend(lineage_for_items(
                &semantic_attempt.query_id,
                &portfolio_item.chapter_id,
                &semantic_items,
            ));
            items.extend(semantic_items);
            attempts.push(semantic_attempt);

            let dblp_started_at = Utc::now();
            let dblp_result = dblp::search_publications(&search_query, config).await;
            let (dblp_items, dblp_attempt) = run_source_attempt(
                "dblp",
                &query,
                &portfolio_item.chapter_id,
                round,
                dblp_started_at,
                dblp_result,
            );
            source_lineage.extend(lineage_for_items(
                &dblp_attempt.query_id,
                &portfolio_item.chapter_id,
                &dblp_items,
            ));
            items.extend(dblp_items);
            attempts.push(dblp_attempt);
        }
    }

    Ok((items, attempts, source_lineage))
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

fn run_source_attempt(
    source: &str,
    query: &str,
    chapter_id: &str,
    round: usize,
    started_at: chrono::DateTime<Utc>,
    result: Result<Vec<SourceItem>>,
) -> (Vec<SourceItem>, QueryAttempt) {
    let query_id = format!("{}-{}", source_prefix(source), Uuid::new_v4());
    match result {
        Ok(items) => {
            let result_count = items.len();
            let source_kind = items.first().map(|item| item.kind);
            (
                items,
                QueryAttempt {
                    query_id,
                    source: source.to_string(),
                    query: query.to_string(),
                    chapter_id: chapter_id.to_string(),
                    round,
                    started_at,
                    finished_at: Some(Utc::now()),
                    result_count,
                    source_kind,
                    http_status: None,
                    rate_limit_info: None,
                    parser_error: None,
                    is_citeable: true,
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
                source: source.to_string(),
                query: query.to_string(),
                chapter_id: chapter_id.to_string(),
                round,
                started_at,
                finished_at: Some(Utc::now()),
                result_count: 0,
                source_kind: source_kind_for_source(source),
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

fn source_kind_for_source(source: &str) -> Option<crate::model::SourceKind> {
    match source {
        "semantic_scholar" => Some(crate::model::SourceKind::AcademicIndex),
        "dblp" => Some(crate::model::SourceKind::Bibliography),
        _ => None,
    }
}

fn source_prefix(source: &str) -> &'static str {
    match source {
        "semantic_scholar" => "ss",
        "dblp" => "db",
        _ => "src",
    }
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

    use super::{academic_queries, run_source_attempt};
    use crate::error::AppError;
    use crate::model::QueryPortfolio;

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
    fn failed_academic_attempt_records_status_and_rate_limit() {
        let (_items, attempt) = run_source_attempt(
            "semantic_scholar",
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
            "dblp",
            "rust agent",
            "ch-1",
            1,
            Utc::now(),
            Err(AppError::Json(err)),
        );

        assert_eq!(attempt.http_status, None);
        assert!(attempt.parser_error.is_some());
        assert!(attempt.error.is_some());
        assert!(!attempt.is_citeable);
    }
}
