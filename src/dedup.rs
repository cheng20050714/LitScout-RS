use std::collections::HashSet;

use crate::model::SourceItem;

pub fn dedup_by_id(items: Vec<SourceItem>) -> Vec<SourceItem> {
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|item| seen.insert(item.id.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use super::dedup_by_id;
    use crate::model::{ArxivPaper, GitHubRepo, SourceItem};

    fn dt() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-05-30T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn deduplicates_by_stable_id() {
        let repo = GitHubRepo {
            owner: "rust-lang".to_string(),
            name: "rust".to_string(),
            full_name: "rust-lang/rust".to_string(),
            html_url: "https://github.com/rust-lang/rust".to_string(),
            description: Some("Rust compiler".to_string()),
            stars: 1,
            forks: 1,
            language: Some("Rust".to_string()),
            updated_at: dt(),
            topics: vec![],
            readme_excerpt: None,
        };
        let paper = ArxivPaper {
            arxiv_id: "2501.00001v2".to_string(),
            title: "Paper".to_string(),
            authors: vec![],
            summary: "Summary".to_string(),
            published_at: dt(),
            updated_at: None,
            categories: vec!["cs.SE".to_string()],
            abs_url: "https://arxiv.org/abs/2501.00001v2".to_string(),
            pdf_url: None,
        };

        let items = vec![
            SourceItem::from(&repo),
            SourceItem::from(&repo),
            SourceItem::from(&paper),
            SourceItem::from(&paper),
        ];
        let deduped = dedup_by_id(items);

        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].id, "github:rust-lang/rust");
        assert_eq!(deduped[1].id, "arxiv:2501.00001");
    }
}
