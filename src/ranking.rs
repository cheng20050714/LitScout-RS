use chrono::{DateTime, Utc};

use crate::model::{SearchQuery, SourceItem, SourceKind, SourceMetadata};

pub fn rank_items(query: &SearchQuery, items: Vec<SourceItem>) -> Vec<SourceItem> {
    let now = Utc::now();
    let mut items = items
        .into_iter()
        .map(|mut item| {
            score_item(&mut item, query, now);
            item
        })
        .collect::<Vec<_>>();

    items.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    items
}

pub fn score_item(item: &mut SourceItem, query: &SearchQuery, now: DateTime<Utc>) {
    let terms = query_terms(&query.topic);
    let title_score = keyword_hits(&item.title, &terms) as f64 * 4.0;
    let summary_score = keyword_hits(&item.summary, &terms) as f64 * 2.0;
    let metadata_score = metadata_keyword_score(item, &terms);
    let keyword_score = title_score + summary_score + metadata_score;
    let popularity_score = popularity_score(item);
    let recency_score = recency_score(item.published_or_updated_at, now, item.kind);
    let source_bonus = source_bonus(item);

    item.score_breakdown.keyword_score = keyword_score;
    item.score_breakdown.popularity_score = popularity_score;
    item.score_breakdown.recency_score = recency_score;
    item.score_breakdown.source_bonus = source_bonus;
    item.score = keyword_score + popularity_score + recency_score + source_bonus;
    item.score_reasons = build_score_reasons(item);
}

fn query_terms(topic: &str) -> Vec<String> {
    topic
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter_map(|term| {
            let term = term.trim().to_ascii_lowercase();
            (term.len() >= 2).then_some(term)
        })
        .collect()
}

fn keyword_hits(text: &str, terms: &[String]) -> usize {
    let text = text.to_ascii_lowercase();
    terms
        .iter()
        .filter(|term| text.contains(term.as_str()))
        .count()
}

fn metadata_keyword_score(item: &SourceItem, terms: &[String]) -> f64 {
    let metadata_text = match &item.metadata {
        SourceMetadata::GitHub {
            language, topics, ..
        } => format!(
            "{} {}",
            language.as_deref().unwrap_or_default(),
            topics.join(" ")
        ),
        SourceMetadata::Arxiv {
            authors: _,
            categories,
        } => categories.join(" "),
    };
    keyword_hits(&metadata_text, terms) as f64
}

fn popularity_score(item: &SourceItem) -> f64 {
    match &item.metadata {
        SourceMetadata::GitHub { stars, .. } => ((*stars as f64) + 1.0).ln() * 1.5,
        SourceMetadata::Arxiv { .. } => 0.0,
    }
}

fn recency_score(date: Option<DateTime<Utc>>, now: DateTime<Utc>, kind: SourceKind) -> f64 {
    let Some(date) = date else {
        return 0.0;
    };
    let days = (now - date).num_days().max(0);
    let base = if days <= 30 {
        8.0
    } else if days <= 180 {
        5.0
    } else if days <= 365 {
        3.0
    } else {
        1.0
    };

    match kind {
        SourceKind::GitHub => base * 0.8,
        SourceKind::Arxiv => base,
    }
}

fn source_bonus(item: &SourceItem) -> f64 {
    match &item.metadata {
        SourceMetadata::GitHub {
            language, topics, ..
        } => {
            let mut bonus = 0.0;
            if language.as_deref() == Some("Rust") {
                bonus += 2.0;
            }
            if topics
                .iter()
                .any(|topic| topic.eq_ignore_ascii_case("rust"))
            {
                bonus += 1.0;
            }
            bonus
        }
        SourceMetadata::Arxiv { categories, .. } => {
            if categories
                .iter()
                .any(|cat| matches!(cat.as_str(), "cs.AI" | "cs.CL" | "cs.SE" | "cs.LG"))
            {
                1.0
            } else {
                0.0
            }
        }
    }
}

fn build_score_reasons(item: &SourceItem) -> Vec<String> {
    let mut reasons = Vec::new();
    if item.score_breakdown.keyword_score > 0.0 {
        reasons.push(format!(
            "keyword match +{:.1}",
            item.score_breakdown.keyword_score
        ));
    }
    if item.score_breakdown.popularity_score > 0.0 {
        reasons.push(format!(
            "popularity +{:.1}",
            item.score_breakdown.popularity_score
        ));
    }
    if item.score_breakdown.recency_score > 0.0 {
        reasons.push(format!(
            "recency +{:.1}",
            item.score_breakdown.recency_score
        ));
    }
    if item.score_breakdown.source_bonus > 0.0 {
        reasons.push(format!(
            "source bonus +{:.1}",
            item.score_breakdown.source_bonus
        ));
    }
    reasons
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use super::{rank_items, score_item};
    use crate::model::{GitHubRepo, SearchQuery, SourceItem};

    fn dt(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn repo(full_name: &str, stars: u64, description: &str) -> SourceItem {
        SourceItem::from(&GitHubRepo {
            owner: full_name.split('/').next().unwrap().to_string(),
            name: full_name.split('/').nth(1).unwrap().to_string(),
            full_name: full_name.to_string(),
            html_url: format!("https://github.com/{full_name}"),
            description: Some(description.to_string()),
            stars,
            forks: 1,
            language: Some("Rust".to_string()),
            updated_at: dt("2026-05-20T00:00:00Z"),
            topics: vec!["rust".to_string(), "agent".to_string()],
            readme_excerpt: None,
        })
    }

    #[test]
    fn scores_item_with_keyword_popularity_and_recency() {
        let query = SearchQuery {
            topic: "rust agent".to_string(),
            github_limit: 10,
            arxiv_limit: 10,
        };
        let mut item = repo("acme/rust-agent", 1000, "Agent framework");

        score_item(&mut item, &query, dt("2026-05-30T00:00:00Z"));

        assert!(item.score > 0.0);
        assert!(item.score_breakdown.keyword_score > 0.0);
        assert!(item.score_breakdown.popularity_score > 0.0);
        assert!(!item.score_reasons.is_empty());
    }

    #[test]
    fn ranking_is_deterministic_for_equal_scores() {
        let query = SearchQuery {
            topic: "unmatched".to_string(),
            github_limit: 10,
            arxiv_limit: 10,
        };
        let mut a = repo("b/repo", 0, "");
        let mut b = repo("a/repo", 0, "");
        a.published_or_updated_at = None;
        b.published_or_updated_at = None;

        let ranked = rank_items(&query, vec![a, b]);

        assert_eq!(ranked[0].id, "github:a/repo");
        assert_eq!(ranked[1].id, "github:b/repo");
    }
}
