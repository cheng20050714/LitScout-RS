use std::collections::{BTreeMap, BTreeSet};

use crate::config::AppConfig;
use crate::error::Result;
use crate::model::{
    ArxivPaper, CategoryGroup, CitationLedger, EvidenceItem, EvidenceMemory, GitHubRepo,
    QueryAttempt, SearchQuery, SourceItem, SourceKind, SourceQueryLineage,
};
use crate::{classify, dedup, ranking};

const EVIDENCE_NOTE_MAX_CHARS: usize = 150;

#[derive(Debug, Clone)]
pub struct EvidenceBuildResult {
    pub memory: EvidenceMemory,
    pub ranked_items: Vec<SourceItem>,
    pub groups: Vec<CategoryGroup>,
    pub citations: CitationLedger,
}

pub fn build_evidence_memory(
    query: &SearchQuery,
    app_config: &AppConfig,
    github_repos: Vec<GitHubRepo>,
    arxiv_papers: Vec<ArxivPaper>,
    query_attempts: Vec<QueryAttempt>,
    source_lineage: Vec<SourceQueryLineage>,
) -> Result<EvidenceBuildResult> {
    let mut source_items = github_repos
        .iter()
        .map(SourceItem::from)
        .collect::<Vec<SourceItem>>();
    source_items.extend(arxiv_papers.iter().map(SourceItem::from));
    let deduped = dedup::dedup_by_id(source_items);
    let mut ranked_items = ranking::rank_items(query, deduped);
    let rules = classify::load_rules(app_config.tags_file.as_deref())?;
    classify::classify_items_with_rules(&mut ranked_items, &rules);
    let groups = classify::group_by_tags(&ranked_items, &rules);
    let citations = CitationLedger::from_items(&ranked_items);
    let lineage_by_source = lineage_by_source(source_lineage);
    let items = ranked_items
        .iter()
        .filter_map(|item| {
            evidence_from_source_item(item, &citations, &query_attempts, &lineage_by_source)
        })
        .collect::<Vec<_>>();

    Ok(EvidenceBuildResult {
        memory: EvidenceMemory {
            items,
            query_attempts,
            source_lineage: lineage_by_source
                .into_iter()
                .map(|(source_item_id, query_attempt_ids)| SourceQueryLineage {
                    source_item_id,
                    query_attempt_ids: query_attempt_ids.into_iter().collect(),
                })
                .collect(),
        },
        ranked_items,
        groups,
        citations,
    })
}

fn evidence_from_source_item(
    item: &SourceItem,
    citations: &CitationLedger,
    attempts: &[QueryAttempt],
    lineage_by_source: &BTreeMap<String, BTreeSet<String>>,
) -> Option<EvidenceItem> {
    let citation = citations
        .citations
        .iter()
        .find(|citation| citation.source_item_id == item.id)?;
    let matching_query_ids = lineage_by_source.get(&item.id);
    let matching_attempts = attempts
        .iter()
        .filter(|attempt| attempt.error.is_none())
        .filter(|attempt| match matching_query_ids {
            Some(ids) => ids.contains(&attempt.query_id),
            None => attempt.source == source_name(item.kind),
        })
        .collect::<Vec<_>>();
    let query_attempt_ids = matching_attempts
        .iter()
        .map(|attempt| attempt.query_id.clone())
        .collect::<Vec<_>>();
    let chapter_ids = matching_attempts
        .iter()
        .map(|attempt| attempt.chapter_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(1)
        .collect::<Vec<_>>();

    Some(EvidenceItem {
        evidence_id: format!("ev-{}", citation.id),
        source_item_id: item.id.clone(),
        citation_id: citation.id.clone(),
        chapter_ids,
        query_attempt_ids,
        source_kind: item.kind,
        title: item.title.clone(),
        url: item.url.clone(),
        evidence_note_zh: build_evidence_note(item),
        evidence_snippet: item.evidence_snippet.clone(),
        support_score: None,
    })
}

fn lineage_by_source(
    source_lineage: Vec<SourceQueryLineage>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut by_source: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for lineage in source_lineage {
        let ids = by_source.entry(lineage.source_item_id).or_default();
        ids.extend(lineage.query_attempt_ids);
    }
    by_source
}

fn source_name(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::GitHub => "github",
        SourceKind::Arxiv => "arxiv",
    }
}

fn build_evidence_note(item: &SourceItem) -> String {
    let source = match item.kind {
        SourceKind::GitHub => "GitHub 仓库",
        SourceKind::Arxiv => "arXiv 论文",
    };
    format!(
        "{} `{}`：{}",
        source,
        item.title,
        truncate_for_note(&item.evidence_snippet, EVIDENCE_NOTE_MAX_CHARS)
    )
}

fn truncate_for_note(text: &str, max_chars: usize) -> String {
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
    use chrono::{DateTime, Utc};

    use super::{build_evidence_memory, truncate_for_note};
    use crate::config::AppConfig;
    use crate::model::{GitHubRepo, QueryAttempt, SearchQuery, SourceQueryLineage};

    fn dt() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-05-30T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn assigns_each_source_to_one_chapter_even_if_multiple_attempts_match() {
        let query = SearchQuery {
            topic: "rust agent".to_string(),
            github_limit: 10,
            arxiv_limit: 10,
        };
        let attempts = vec![
            sample_attempt("gh-1", "ch-1"),
            sample_attempt("gh-2", "ch-2"),
        ];

        let result = build_evidence_memory(
            &query,
            &test_config(),
            vec![sample_repo()],
            Vec::new(),
            attempts,
            vec![
                SourceQueryLineage {
                    source_item_id: "github:acme/rust-agent".to_string(),
                    query_attempt_ids: vec!["gh-1".to_string()],
                },
                SourceQueryLineage {
                    source_item_id: "github:acme/rust-agent".to_string(),
                    query_attempt_ids: vec!["gh-2".to_string()],
                },
            ],
        )
        .expect("evidence memory should build");

        assert_eq!(result.memory.items.len(), 1);
        assert_eq!(result.memory.items[0].chapter_ids.len(), 1);
        assert_eq!(result.memory.items[0].chapter_ids[0], "ch-1");
        assert_eq!(
            result.memory.items[0].query_attempt_ids,
            vec!["gh-1".to_string(), "gh-2".to_string()]
        );
    }

    #[test]
    fn binds_source_to_chapter_from_query_lineage() {
        let query = SearchQuery {
            topic: "rust agent".to_string(),
            github_limit: 10,
            arxiv_limit: 10,
        };
        let attempts = vec![
            sample_attempt("gh-1", "ch-1"),
            sample_attempt("gh-2", "ch-2"),
        ];

        let result = build_evidence_memory(
            &query,
            &test_config(),
            vec![sample_repo()],
            Vec::new(),
            attempts,
            vec![SourceQueryLineage {
                source_item_id: "github:acme/rust-agent".to_string(),
                query_attempt_ids: vec!["gh-2".to_string()],
            }],
        )
        .expect("evidence memory should build");

        assert_eq!(result.memory.items[0].chapter_ids, vec!["ch-2".to_string()]);
        assert_eq!(
            result.memory.items[0].query_attempt_ids,
            vec!["gh-2".to_string()]
        );
    }

    #[test]
    fn evidence_note_truncation_is_character_safe() {
        let text = "字".repeat(151);

        let note = truncate_for_note(&text, 150);

        assert_eq!(note.chars().count(), 153);
        assert!(note.ends_with("..."));
    }

    fn sample_repo() -> GitHubRepo {
        GitHubRepo {
            owner: "acme".to_string(),
            name: "rust-agent".to_string(),
            full_name: "acme/rust-agent".to_string(),
            html_url: "https://github.com/acme/rust-agent".to_string(),
            description: Some("Rust agent framework with tool calling".to_string()),
            stars: 100,
            forks: 10,
            language: Some("Rust".to_string()),
            updated_at: dt(),
            topics: vec!["rust".to_string(), "agent".to_string()],
            readme_excerpt: None,
        }
    }

    fn sample_attempt(query_id: &str, chapter_id: &str) -> QueryAttempt {
        QueryAttempt {
            query_id: query_id.to_string(),
            source: "github".to_string(),
            query: "rust agent".to_string(),
            chapter_id: chapter_id.to_string(),
            round: 1,
            started_at: dt(),
            finished_at: Some(dt()),
            result_count: 1,
            error: None,
        }
    }

    fn test_config() -> AppConfig {
        AppConfig {
            github_token: Some("token".to_string()),
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
