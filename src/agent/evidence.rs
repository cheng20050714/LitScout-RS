use std::collections::BTreeSet;

use crate::config::AppConfig;
use crate::error::Result;
use crate::model::{
    ArxivPaper, CategoryGroup, CitationLedger, EvidenceItem, EvidenceMemory, GitHubRepo,
    QueryAttempt, SearchQuery, SourceItem, SourceKind,
};
use crate::{classify, dedup, ranking};

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
    let items = ranked_items
        .iter()
        .filter_map(|item| evidence_from_source_item(item, &citations, &query_attempts))
        .collect::<Vec<_>>();

    Ok(EvidenceBuildResult {
        memory: EvidenceMemory {
            items,
            query_attempts,
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
) -> Option<EvidenceItem> {
    let citation = citations
        .citations
        .iter()
        .find(|citation| citation.source_item_id == item.id)?;
    let source = match item.kind {
        SourceKind::GitHub => "github",
        SourceKind::Arxiv => "arxiv",
    };
    let matching_attempts = attempts
        .iter()
        .filter(|attempt| attempt.source == source && attempt.error.is_none())
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

fn build_evidence_note(item: &SourceItem) -> String {
    let source = match item.kind {
        SourceKind::GitHub => "GitHub 仓库",
        SourceKind::Arxiv => "arXiv 论文",
    };
    format!(
        "{} `{}` 提供了与主题相关的证据：{}",
        source, item.title, item.evidence_snippet
    )
}
