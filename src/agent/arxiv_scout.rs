use std::time::Duration;

use chrono::Utc;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::error::Result;
use crate::model::{ArxivPaper, QueryAttempt, QueryPortfolio, SearchQuery};
use crate::sources::arxiv;

const ARXIV_QUERY_DELAY: Duration = Duration::from_secs(3);

pub async fn scout_arxiv(
    portfolio: &[QueryPortfolio],
    config: &AppConfig,
    round: usize,
) -> Result<(Vec<ArxivPaper>, Vec<QueryAttempt>)> {
    let mut papers = Vec::new();
    let mut attempts = Vec::new();
    let mut query_index = 0usize;

    for item in portfolio {
        for query in &item.arxiv_queries {
            if query_index > 0 {
                tokio::time::sleep(ARXIV_QUERY_DELAY).await;
            }
            query_index += 1;
            let started_at = Utc::now();
            let query_id = format!("ax-{}", Uuid::new_v4());
            let search_query = SearchQuery {
                topic: query.clone(),
                github_limit: item.budget.max(1),
                arxiv_limit: item.budget.max(1),
            };
            match arxiv::search_papers(&search_query, config).await {
                Ok(mut result) => {
                    let result_count = result.len();
                    papers.append(&mut result);
                    attempts.push(QueryAttempt {
                        query_id,
                        source: "arxiv".to_string(),
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
                        source: "arxiv".to_string(),
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

    Ok((papers, attempts))
}
