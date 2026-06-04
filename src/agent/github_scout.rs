use chrono::Utc;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::error::Result;
use crate::model::{GitHubRepo, QueryAttempt, QueryPortfolio, SearchQuery};
use crate::sources::github;

pub async fn scout_github(
    portfolio: &[QueryPortfolio],
    config: &AppConfig,
    round: usize,
) -> Result<(Vec<GitHubRepo>, Vec<QueryAttempt>)> {
    let mut repos = Vec::new();
    let mut attempts = Vec::new();

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
                    let result_count = result.len();
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
                        error: Some(err.to_string()),
                    });
                }
            }
        }
    }

    Ok((repos, attempts))
}
