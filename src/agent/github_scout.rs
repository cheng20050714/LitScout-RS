use chrono::Utc;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::error::Result;
use crate::model::{
    GitHubRepo, QueryAttempt, QueryPortfolio, SearchQuery, SourceKind, SourceQueryLineage,
};
use crate::sources::github;

pub async fn scout_github(
    portfolio: &[QueryPortfolio],
    config: &AppConfig,
    round: usize,
) -> Result<(Vec<GitHubRepo>, Vec<QueryAttempt>, Vec<SourceQueryLineage>)> {
    let mut repos = Vec::new();
    let mut attempts = Vec::new();
    let mut source_lineage = Vec::new();

    for item in portfolio {
        for query in &item.github_queries {
            let started_at = Utc::now();
            let query_id = format!("gh-{}", Uuid::new_v4());
            let search_query = SearchQuery {
                topic: query.clone(),
                github_limit: item.budget.max(1),
                arxiv_limit: item.budget.max(1),
            };
            match github::search_repositories(&search_query, config).await {
                Ok(mut result) => {
                    github::enrich_repositories(&mut result, config).await;
                    let result_count = result.len();
                    source_lineage.extend(result.iter().map(|repo| SourceQueryLineage {
                        lineage_id: format!("lin-{}-{}", query_id, repo.full_name),
                        source_item_id: format!("github:{}", repo.full_name),
                        chapter_id: Some(item.chapter_id.clone()),
                        source_kind: Some(SourceKind::GitHub),
                        query_attempt_ids: vec![query_id.clone()],
                        returned_item_ids: vec![format!("github:{}", repo.full_name)],
                        merged_from_item_ids: Vec::new(),
                    }));
                    repos.append(&mut result);
                    attempts.push(QueryAttempt {
                        query_id,
                        source: "github".to_string(),
                        query: query.clone(),
                        chapter_id: item.chapter_id.clone(),
                        round,
                        started_at,
                        finished_at: Some(Utc::now()),
                        result_count,
                        source_kind: Some(SourceKind::GitHub),
                        http_status: None,
                        rate_limit_info: None,
                        parser_error: None,
                        is_citeable: true,
                        error: None,
                    });
                }
                Err(err) => {
                    attempts.push(QueryAttempt {
                        query_id,
                        source: "github".to_string(),
                        query: query.clone(),
                        chapter_id: item.chapter_id.clone(),
                        round,
                        started_at,
                        finished_at: Some(Utc::now()),
                        result_count: 0,
                        source_kind: Some(SourceKind::GitHub),
                        http_status: None,
                        rate_limit_info: None,
                        parser_error: None,
                        is_citeable: false,
                        error: Some(err.to_string()),
                    });
                }
            }
        }
    }

    Ok((repos, attempts, source_lineage))
}
