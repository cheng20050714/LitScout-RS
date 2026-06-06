use std::time::Duration;

use chrono::Utc;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::error::Result;
use crate::model::{
    stable_arxiv_id, ArxivPaper, QueryAttempt, QueryPortfolio, SearchQuery, SourceQueryLineage,
};
use crate::sources::arxiv;

const ARXIV_QUERY_DELAY: Duration = Duration::from_secs(3);
const RELEVANT_ARXIV_PREFIXES: &[&str] = &["cs.", "stat.ML", "math.OC"];

pub async fn scout_arxiv(
    portfolio: &[QueryPortfolio],
    config: &AppConfig,
    round: usize,
) -> Result<(Vec<ArxivPaper>, Vec<QueryAttempt>, Vec<SourceQueryLineage>)> {
    let mut papers = Vec::new();
    let mut attempts = Vec::new();
    let mut source_lineage = Vec::new();
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
                    let before_filter = result.len();
                    result.retain(is_relevant_paper);
                    let result_count = result.len();
                    if before_filter > result_count {
                        tracing::info!(
                            "arXiv filter: kept {result_count}/{before_filter} papers for query `{query}` (filtered {} irrelevant by category)",
                            before_filter - result_count
                        );
                    }
                    source_lineage.extend(result.iter().map(|paper| SourceQueryLineage {
                        source_item_id: format!("arxiv:{}", stable_arxiv_id(&paper.arxiv_id)),
                        query_attempt_ids: vec![query_id.clone()],
                    }));
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

    Ok((papers, attempts, source_lineage))
}

fn is_relevant_paper(paper: &ArxivPaper) -> bool {
    if paper.categories.is_empty() {
        return true;
    }

    paper.categories.iter().any(|category| {
        RELEVANT_ARXIV_PREFIXES
            .iter()
            .any(|prefix| category.starts_with(prefix))
    })
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use super::is_relevant_paper;
    use crate::model::ArxivPaper;

    fn paper_with_categories(categories: &[&str]) -> ArxivPaper {
        ArxivPaper {
            arxiv_id: "2601.00001".to_string(),
            title: "Test Paper".to_string(),
            authors: vec!["Ada Lovelace".to_string()],
            summary: "A test summary.".to_string(),
            published_at: DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            updated_at: None,
            categories: categories
                .iter()
                .map(|category| category.to_string())
                .collect(),
            abs_url: "https://arxiv.org/abs/2601.00001".to_string(),
            pdf_url: None,
        }
    }

    #[test]
    fn keeps_computer_science_and_ml_adjacent_papers() {
        assert!(is_relevant_paper(&paper_with_categories(&["cs.AI"])));
        assert!(is_relevant_paper(&paper_with_categories(&["stat.ML"])));
        assert!(is_relevant_paper(&paper_with_categories(&["math.OC"])));
        assert!(is_relevant_paper(&paper_with_categories(&[
            "astro-ph.GA",
            "cs.LG"
        ])));
    }

    #[test]
    fn filters_obviously_irrelevant_arxiv_categories() {
        assert!(!is_relevant_paper(&paper_with_categories(&["astro-ph.GA"])));
        assert!(!is_relevant_paper(&paper_with_categories(&[
            "physics.bio-ph"
        ])));
    }

    #[test]
    fn keeps_papers_without_categories() {
        assert!(is_relevant_paper(&paper_with_categories(&[])));
    }
}
