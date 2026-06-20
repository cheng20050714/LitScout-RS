use std::collections::BTreeMap;

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
    extra_source_items: Vec<SourceItem>,
    query_attempts: Vec<QueryAttempt>,
    source_lineage: Vec<SourceQueryLineage>,
) -> Result<EvidenceBuildResult> {
    let mut source_items = github_repos
        .iter()
        .map(SourceItem::from)
        .collect::<Vec<SourceItem>>();
    source_items.extend(arxiv_papers.iter().map(SourceItem::from));
    source_items.extend(extra_source_items);
    let deduped = dedup::canonical_merge(source_items, source_lineage);
    let mut ranked_items = ranking::rank_items(query, deduped.items);
    let rules = classify::load_rules(app_config.tags_file.as_deref())?;
    classify::classify_items_with_rules(&mut ranked_items, &rules);
    let groups = classify::group_by_tags(&ranked_items, &rules);
    let citations = CitationLedger::from_items(&ranked_items);
    let lineage_by_source = lineage_by_source(deduped.source_lineage);
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
            source_lineage: lineage_by_source.values().cloned().collect(),
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
    lineage_by_source: &BTreeMap<String, SourceQueryLineage>,
) -> Option<EvidenceItem> {
    let citation = citations
        .citations
        .iter()
        .find(|citation| citation.source_item_id == item.id)?;
    let lineage = lineage_by_source.get(&item.id)?;
    let matching_attempts = attempts
        .iter()
        .filter(|attempt| attempt.error.is_none())
        .filter(|attempt| lineage.query_attempt_ids.contains(&attempt.query_id))
        .collect::<Vec<_>>();
    let query_attempt_ids = matching_attempts
        .iter()
        .map(|attempt| attempt.query_id.clone())
        .collect::<Vec<_>>();
    if query_attempt_ids.is_empty() {
        return None;
    }
    let chapter_ids = matching_attempts
        .iter()
        .map(|attempt| attempt.chapter_id.clone())
        .fold(Vec::new(), |mut ids, id| {
            if !ids.iter().any(|existing| existing == &id) {
                ids.push(id);
            }
            ids
        })
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
) -> BTreeMap<String, SourceQueryLineage> {
    let mut by_source: BTreeMap<String, SourceQueryLineage> = BTreeMap::new();
    for lineage in source_lineage {
        let entry = by_source
            .entry(lineage.source_item_id.clone())
            .or_insert_with(|| SourceQueryLineage {
                lineage_id: if lineage.lineage_id.is_empty() {
                    format!("lin-{}", lineage.source_item_id)
                } else {
                    lineage.lineage_id.clone()
                },
                source_item_id: lineage.source_item_id.clone(),
                chapter_id: lineage.chapter_id.clone(),
                source_kind: lineage.source_kind,
                query_attempt_ids: Vec::new(),
                returned_item_ids: Vec::new(),
                merged_from_item_ids: Vec::new(),
            });
        if entry.chapter_id.is_none() {
            entry.chapter_id = lineage.chapter_id;
        }
        if entry.source_kind.is_none() {
            entry.source_kind = lineage.source_kind;
        }
        extend_unique(&mut entry.query_attempt_ids, lineage.query_attempt_ids);
        extend_unique(&mut entry.returned_item_ids, lineage.returned_item_ids);
        extend_unique(
            &mut entry.merged_from_item_ids,
            lineage.merged_from_item_ids,
        );
    }
    by_source
}

fn extend_unique(target: &mut Vec<String>, values: Vec<String>) {
    for value in values {
        if !target.iter().any(|existing| existing == &value) {
            target.push(value);
        }
    }
}

fn build_evidence_note(item: &SourceItem) -> String {
    let source = match item.kind {
        SourceKind::GitHub => "GitHub 仓库",
        SourceKind::Arxiv => "arXiv 论文",
        SourceKind::AcademicIndex => "学术索引记录",
        SourceKind::Bibliography => "书目元数据",
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
            Vec::new(),
            attempts,
            vec![
                SourceQueryLineage {
                    lineage_id: "lin-gh-1".to_string(),
                    source_item_id: "github:acme/rust-agent".to_string(),
                    chapter_id: Some("ch-1".to_string()),
                    source_kind: Some(crate::model::SourceKind::GitHub),
                    query_attempt_ids: vec!["gh-1".to_string()],
                    returned_item_ids: vec!["github:acme/rust-agent".to_string()],
                    merged_from_item_ids: Vec::new(),
                },
                SourceQueryLineage {
                    lineage_id: "lin-gh-2".to_string(),
                    source_item_id: "github:acme/rust-agent".to_string(),
                    chapter_id: Some("ch-2".to_string()),
                    source_kind: Some(crate::model::SourceKind::GitHub),
                    query_attempt_ids: vec!["gh-2".to_string()],
                    returned_item_ids: vec!["github:acme/rust-agent".to_string()],
                    merged_from_item_ids: Vec::new(),
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
            Vec::new(),
            attempts,
            vec![SourceQueryLineage {
                lineage_id: "lin-gh-2".to_string(),
                source_item_id: "github:acme/rust-agent".to_string(),
                chapter_id: Some("ch-2".to_string()),
                source_kind: Some(crate::model::SourceKind::GitHub),
                query_attempt_ids: vec!["gh-2".to_string()],
                returned_item_ids: vec!["github:acme/rust-agent".to_string()],
                merged_from_item_ids: Vec::new(),
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
    fn preserves_source_lineage_metadata_after_merge() {
        let query = SearchQuery {
            topic: "rust agent".to_string(),
            github_limit: 10,
            arxiv_limit: 10,
        };
        let result = build_evidence_memory(
            &query,
            &test_config(),
            vec![sample_repo()],
            Vec::new(),
            Vec::new(),
            vec![
                sample_attempt("gh-1", "ch-1"),
                sample_attempt("gh-2", "ch-2"),
            ],
            vec![
                SourceQueryLineage {
                    lineage_id: "lin-gh-1".to_string(),
                    source_item_id: "github:acme/rust-agent".to_string(),
                    chapter_id: Some("ch-1".to_string()),
                    source_kind: Some(crate::model::SourceKind::GitHub),
                    query_attempt_ids: vec!["gh-1".to_string()],
                    returned_item_ids: vec!["github:acme/rust-agent".to_string()],
                    merged_from_item_ids: vec!["github:acme/rust-agent-duplicate".to_string()],
                },
                SourceQueryLineage {
                    lineage_id: "lin-gh-2".to_string(),
                    source_item_id: "github:acme/rust-agent".to_string(),
                    chapter_id: Some("ch-2".to_string()),
                    source_kind: Some(crate::model::SourceKind::GitHub),
                    query_attempt_ids: vec!["gh-2".to_string()],
                    returned_item_ids: vec!["github:acme/rust-agent".to_string()],
                    merged_from_item_ids: Vec::new(),
                },
            ],
        )
        .expect("evidence memory should build");

        let lineage = result
            .memory
            .source_lineage
            .iter()
            .find(|lineage| lineage.source_item_id == "github:acme/rust-agent")
            .expect("lineage should be preserved");

        assert_eq!(lineage.chapter_id.as_deref(), Some("ch-1"));
        assert_eq!(lineage.source_kind, Some(crate::model::SourceKind::GitHub));
        assert_eq!(
            lineage.query_attempt_ids,
            vec!["gh-1".to_string(), "gh-2".to_string()]
        );
        assert_eq!(
            lineage.returned_item_ids,
            vec!["github:acme/rust-agent".to_string()]
        );
        assert_eq!(
            lineage.merged_from_item_ids,
            vec!["github:acme/rust-agent-duplicate".to_string()]
        );
    }

    #[test]
    fn evidence_note_truncation_is_character_safe() {
        let text = "字".repeat(151);

        let note = truncate_for_note(&text, 150);

        assert_eq!(note.chars().count(), 153);
        assert!(note.ends_with("..."));
    }

    #[test]
    fn extra_source_without_lineage_stays_outside_evidence_memory() {
        let query = SearchQuery {
            topic: "rust agent".to_string(),
            github_limit: 10,
            arxiv_limit: 10,
        };
        let result = build_evidence_memory(
            &query,
            &test_config(),
            Vec::new(),
            Vec::new(),
            vec![academic_item()],
            vec![sample_attempt("ss-1", "ch-1")],
            Vec::new(),
        )
        .expect("evidence memory should build");

        assert!(result.memory.items.is_empty());
        assert_eq!(result.ranked_items.len(), 1);
    }

    #[test]
    fn canonical_academic_merge_preserves_attempt_lineage_in_evidence_memory() {
        let query = SearchQuery {
            topic: "tool calling agents".to_string(),
            github_limit: 10,
            arxiv_limit: 10,
        };
        let mut semantic = academic_item();
        if let crate::model::SourceMetadata::AcademicIndex {
            doi, external_ids, ..
        } = &mut semantic.metadata
        {
            *doi = Some("10.1234/tool-agent".to_string());
            *external_ids = vec!["doi:10.1234/tool-agent".to_string()];
        }
        let crossref = crate::model::SourceItem {
            id: "crossref:10.1234/tool-agent".to_string(),
            kind: crate::model::SourceKind::Bibliography,
            title: "Tool Calling Agents in Rust".to_string(),
            url: "https://doi.org/10.1234/tool-agent".to_string(),
            summary: "Bibliographic metadata: Ada Lovelace. TestConf, 2026.".to_string(),
            evidence_snippet: "Bibliographic metadata: Ada Lovelace. TestConf, 2026.".to_string(),
            tags: vec!["TestConf".to_string()],
            score: 0.0,
            score_reasons: Vec::new(),
            classification_reasons: Vec::new(),
            score_breakdown: Default::default(),
            published_or_updated_at: None,
            metadata: crate::model::SourceMetadata::Bibliography {
                authors: vec!["Ada Lovelace".to_string()],
                venue: Some("TestConf".to_string()),
                year: Some(2026),
                doi: Some("10.1234/tool-agent".to_string()),
                citation_count: None,
                native_id: "10.1234/tool-agent".to_string(),
                source_name: "crossref".to_string(),
                external_ids: vec!["doi:10.1234/tool-agent".to_string()],
            },
        };

        let result = build_evidence_memory(
            &query,
            &test_config(),
            Vec::new(),
            Vec::new(),
            vec![crossref, semantic],
            vec![
                sample_attempt("cr-1", "ch-1"),
                sample_attempt("ss-1", "ch-1"),
            ],
            vec![
                SourceQueryLineage {
                    lineage_id: "lin-cr-1".to_string(),
                    source_item_id: "crossref:10.1234/tool-agent".to_string(),
                    chapter_id: Some("ch-1".to_string()),
                    source_kind: Some(crate::model::SourceKind::Bibliography),
                    query_attempt_ids: vec!["cr-1".to_string()],
                    returned_item_ids: vec!["crossref:10.1234/tool-agent".to_string()],
                    merged_from_item_ids: Vec::new(),
                },
                SourceQueryLineage {
                    lineage_id: "lin-ss-1".to_string(),
                    source_item_id: "semantic_scholar:abc123".to_string(),
                    chapter_id: Some("ch-1".to_string()),
                    source_kind: Some(crate::model::SourceKind::AcademicIndex),
                    query_attempt_ids: vec!["ss-1".to_string()],
                    returned_item_ids: vec!["semantic_scholar:abc123".to_string()],
                    merged_from_item_ids: Vec::new(),
                },
            ],
        )
        .expect("evidence memory should build");

        assert_eq!(result.ranked_items.len(), 1);
        assert_eq!(result.ranked_items[0].id, "semantic_scholar:abc123");
        assert_eq!(result.memory.items.len(), 1);
        assert_eq!(
            result.memory.items[0].query_attempt_ids,
            vec!["cr-1".to_string(), "ss-1".to_string()]
        );
        let lineage = &result.memory.source_lineage[0];
        assert!(lineage
            .merged_from_item_ids
            .contains(&"crossref:10.1234/tool-agent".to_string()));
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
            source_kind: Some(crate::model::SourceKind::GitHub),
            http_status: None,
            rate_limit_info: None,
            parser_error: None,
            is_citeable: true,
            error: None,
        }
    }

    fn academic_item() -> crate::model::SourceItem {
        crate::model::SourceItem {
            id: "semantic_scholar:abc123".to_string(),
            kind: crate::model::SourceKind::AcademicIndex,
            title: "Tool Calling Agents in Rust".to_string(),
            url: "https://www.semanticscholar.org/paper/abc123".to_string(),
            summary: "A paper about tool-calling agents.".to_string(),
            evidence_snippet: "A paper about tool-calling agents.".to_string(),
            tags: vec!["Computer Science".to_string()],
            score: 0.0,
            score_reasons: Vec::new(),
            classification_reasons: Vec::new(),
            score_breakdown: Default::default(),
            published_or_updated_at: None,
            metadata: crate::model::SourceMetadata::AcademicIndex {
                authors: vec!["Ada Lovelace".to_string()],
                venue: Some("TestConf".to_string()),
                year: Some(2026),
                doi: None,
                citation_count: Some(5),
                native_id: "abc123".to_string(),
                source_name: "semantic_scholar".to_string(),
                external_ids: Vec::new(),
            },
        }
    }

    fn test_config() -> AppConfig {
        AppConfig {
            github_token: Some("token".to_string()),
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
