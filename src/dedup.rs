use std::collections::{BTreeMap, HashSet};

use crate::model::{SourceItem, SourceKind, SourceMetadata, SourceQueryLineage};

#[derive(Debug, Clone)]
pub struct DedupResult {
    pub items: Vec<SourceItem>,
    pub source_lineage: Vec<SourceQueryLineage>,
}

#[cfg(test)]
pub fn dedup_by_id(items: Vec<SourceItem>) -> Vec<SourceItem> {
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|item| seen.insert(item.id.clone()))
        .collect()
}

pub fn canonical_merge(
    items: Vec<SourceItem>,
    source_lineage: Vec<SourceQueryLineage>,
) -> DedupResult {
    let lineage_by_source = lineage_map(source_lineage);
    let mut canonical_items: Vec<SourceItem> = Vec::new();
    let mut groups: Vec<MergeGroup> = Vec::new();
    let mut group_by_key: BTreeMap<String, usize> = BTreeMap::new();
    let mut group_by_source_id: BTreeMap<String, usize> = BTreeMap::new();

    for item in items {
        let keys = canonical_keys(&item);
        let group_index = keys
            .iter()
            .find_map(|key| group_by_key.get(key).copied())
            .or_else(|| group_by_source_id.get(&item.id).copied());

        match group_index {
            Some(index) => {
                let item_id = item.id.clone();
                if representative_rank(&item) > representative_rank(&canonical_items[index]) {
                    let old_representative = std::mem::replace(&mut canonical_items[index], item);
                    groups[index]
                        .merged_from_item_ids
                        .push(old_representative.id);
                } else {
                    groups[index].merged_from_item_ids.push(item_id.clone());
                }
                groups[index].source_item_ids.push(item_id.clone());
                for key in keys {
                    group_by_key.insert(key, index);
                }
                group_by_source_id.insert(item_id, index);
            }
            None => {
                let index = canonical_items.len();
                for key in &keys {
                    group_by_key.insert(key.clone(), index);
                }
                group_by_source_id.insert(item.id.clone(), index);
                groups.push(MergeGroup {
                    source_item_ids: vec![item.id.clone()],
                    merged_from_item_ids: Vec::new(),
                });
                canonical_items.push(item);
            }
        }
    }

    let source_lineage = groups
        .into_iter()
        .zip(canonical_items.iter())
        .map(|(group, item)| merged_lineage(item, group, &lineage_by_source))
        .collect();

    DedupResult {
        items: canonical_items,
        source_lineage,
    }
}

#[derive(Debug, Clone)]
struct MergeGroup {
    source_item_ids: Vec<String>,
    merged_from_item_ids: Vec<String>,
}

fn lineage_map(source_lineage: Vec<SourceQueryLineage>) -> BTreeMap<String, SourceQueryLineage> {
    let mut by_source = BTreeMap::new();
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

fn merged_lineage(
    item: &SourceItem,
    group: MergeGroup,
    lineage_by_source: &BTreeMap<String, SourceQueryLineage>,
) -> SourceQueryLineage {
    let mut merged = SourceQueryLineage {
        lineage_id: format!("lin-{}", item.id),
        source_item_id: item.id.clone(),
        chapter_id: None,
        source_kind: Some(item.kind),
        query_attempt_ids: Vec::new(),
        returned_item_ids: Vec::new(),
        merged_from_item_ids: Vec::new(),
    };

    for source_item_id in &group.source_item_ids {
        if let Some(lineage) = lineage_by_source.get(source_item_id) {
            if merged.chapter_id.is_none() {
                merged.chapter_id = lineage.chapter_id.clone();
            }
            extend_unique(
                &mut merged.query_attempt_ids,
                lineage.query_attempt_ids.clone(),
            );
            extend_unique(
                &mut merged.returned_item_ids,
                lineage.returned_item_ids.clone(),
            );
            extend_unique(
                &mut merged.merged_from_item_ids,
                lineage.merged_from_item_ids.clone(),
            );
        } else {
            extend_unique(&mut merged.returned_item_ids, vec![source_item_id.clone()]);
        }
    }
    extend_unique(&mut merged.returned_item_ids, group.source_item_ids);
    extend_unique(&mut merged.merged_from_item_ids, group.merged_from_item_ids);
    merged
        .merged_from_item_ids
        .retain(|merged_id| merged_id != &item.id);
    merged
}

fn extend_unique(target: &mut Vec<String>, values: Vec<String>) {
    for value in values {
        if !target.iter().any(|existing| existing == &value) {
            target.push(value);
        }
    }
}

fn canonical_keys(item: &SourceItem) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(doi) = metadata_doi(item).and_then(|doi| normalize_doi(&doi)) {
        keys.push(format!("doi:{doi}"));
    }
    for external_id in metadata_external_ids(item) {
        let external_id = normalize_external_id(&external_id);
        if external_id.starts_with("doi:") || external_id.starts_with("arxiv:") {
            keys.push(external_id);
        }
    }
    if item.kind == SourceKind::Arxiv {
        let id = item
            .id
            .strip_prefix("arxiv:")
            .unwrap_or(item.id.as_str())
            .to_string();
        keys.push(format!("arxiv:{}", crate::model::stable_arxiv_id(&id)));
    }
    if let Some(dblp_key) = dblp_key(item) {
        keys.push(format!("dblp:{dblp_key}"));
    }
    if let Some(title_key) = title_author_year_key(item) {
        keys.push(title_key);
    }
    if keys.is_empty() {
        keys.push(format!("id:{}", item.id));
    }
    dedup_strings(keys)
}

fn metadata_doi(item: &SourceItem) -> Option<String> {
    match &item.metadata {
        SourceMetadata::AcademicIndex { doi, .. } | SourceMetadata::Bibliography { doi, .. } => {
            doi.clone()
        }
        _ => None,
    }
}

fn metadata_external_ids(item: &SourceItem) -> Vec<String> {
    match &item.metadata {
        SourceMetadata::AcademicIndex { external_ids, .. }
        | SourceMetadata::Bibliography { external_ids, .. } => external_ids.clone(),
        _ => Vec::new(),
    }
}

fn dblp_key(item: &SourceItem) -> Option<String> {
    match &item.metadata {
        SourceMetadata::Bibliography {
            source_name,
            native_id,
            ..
        } if source_name == "dblp" => Some(native_id.clone()),
        _ => item.id.strip_prefix("dblp:").map(ToString::to_string),
    }
}

fn title_author_year_key(item: &SourceItem) -> Option<String> {
    let title = normalize_title(&item.title);
    if title.is_empty() {
        return None;
    }
    let first_author = metadata_authors(item)
        .first()
        .map(|author| normalize_author(author))
        .filter(|author| !author.is_empty())?;
    let year = metadata_year(item)?;
    Some(format!("tay:{title}:{first_author}:{year}"))
}

fn metadata_authors(item: &SourceItem) -> Vec<String> {
    match &item.metadata {
        SourceMetadata::Arxiv { authors, .. }
        | SourceMetadata::AcademicIndex { authors, .. }
        | SourceMetadata::Bibliography { authors, .. } => authors.clone(),
        _ => Vec::new(),
    }
}

fn metadata_year(item: &SourceItem) -> Option<i32> {
    match &item.metadata {
        SourceMetadata::AcademicIndex { year, .. } | SourceMetadata::Bibliography { year, .. } => {
            *year
        }
        _ => item
            .published_or_updated_at
            .map(|date| date.date_naive().year()),
    }
}

fn representative_rank(item: &SourceItem) -> i32 {
    let content_rank = if has_strong_summary(item) { 100 } else { 0 };
    let source_rank = match item.kind {
        SourceKind::Arxiv => 40,
        SourceKind::AcademicIndex => 30,
        SourceKind::GitHub => 20,
        SourceKind::Bibliography => 10,
    };
    content_rank + source_rank
}

fn has_strong_summary(item: &SourceItem) -> bool {
    let summary = item.summary.trim();
    !summary.is_empty() && !summary.starts_with("Bibliographic metadata:")
}

fn normalize_external_id(value: &str) -> String {
    let value = value.trim().to_ascii_lowercase();
    if let Some(doi) = value.strip_prefix("doi:") {
        return format!(
            "doi:{}",
            normalize_doi(doi).unwrap_or_else(|| doi.to_string())
        );
    }
    if let Some(arxiv) = value.strip_prefix("arxiv:") {
        return format!("arxiv:{}", crate::model::stable_arxiv_id(arxiv));
    }
    value
}

fn normalize_doi(value: &str) -> Option<String> {
    let doi = value
        .trim()
        .trim_start_matches("https://doi.org/")
        .trim_start_matches("http://doi.org/")
        .trim_start_matches("doi:")
        .to_ascii_lowercase();
    (!doi.is_empty()).then_some(doi)
}

fn normalize_title(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_author(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphabetic())
        .flat_map(char::to_lowercase)
        .collect()
}

fn dedup_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

use chrono::Datelike;

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use super::{canonical_merge, dedup_by_id};
    use crate::model::{
        ArxivPaper, GitHubRepo, SourceItem, SourceKind, SourceMetadata, SourceQueryLineage,
    };

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

    #[test]
    fn canonical_merge_prefers_strong_summary_over_bibliography_by_doi() {
        let semantic = academic_item(
            "semantic_scholar:ss1",
            SourceKind::AcademicIndex,
            "semantic_scholar",
            Some("10.1234/tool-agent"),
            "A full abstract about tool calling agents.",
        );
        let crossref = academic_item(
            "crossref:10.1234/tool-agent",
            SourceKind::Bibliography,
            "crossref",
            Some("10.1234/tool-agent"),
            "Bibliographic metadata: Ada Lovelace. TestConf, 2026.",
        );

        let result = canonical_merge(
            vec![crossref, semantic],
            vec![
                lineage("cr-1", "crossref:10.1234/tool-agent"),
                lineage("ss-1", "semantic_scholar:ss1"),
            ],
        );

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].id, "semantic_scholar:ss1");
        assert_eq!(
            result.source_lineage[0].query_attempt_ids,
            vec!["cr-1".to_string(), "ss-1".to_string()]
        );
        assert!(result.source_lineage[0]
            .merged_from_item_ids
            .contains(&"crossref:10.1234/tool-agent".to_string()));
    }

    #[test]
    fn canonical_merge_uses_title_author_year_without_doi() {
        let openalex = academic_item(
            "openalex:W1",
            SourceKind::AcademicIndex,
            "openalex",
            None,
            "A full abstract.",
        );
        let dblp = academic_item(
            "dblp:conf/test/ToolAgent2026",
            SourceKind::Bibliography,
            "dblp",
            None,
            "Bibliographic metadata: Ada Lovelace. TestConf, 2026.",
        );

        let result = canonical_merge(
            vec![openalex, dblp],
            vec![
                lineage("oa-1", "openalex:W1"),
                lineage("db-1", "dblp:conf/test/ToolAgent2026"),
            ],
        );

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].id, "openalex:W1");
        assert!(result.source_lineage[0]
            .returned_item_ids
            .contains(&"dblp:conf/test/ToolAgent2026".to_string()));
    }

    fn academic_item(
        id: &str,
        kind: SourceKind,
        source_name: &str,
        doi: Option<&str>,
        summary: &str,
    ) -> SourceItem {
        let metadata = match kind {
            SourceKind::AcademicIndex => SourceMetadata::AcademicIndex {
                authors: vec!["Ada Lovelace".to_string()],
                venue: Some("TestConf".to_string()),
                year: Some(2026),
                doi: doi.map(ToString::to_string),
                citation_count: Some(10),
                native_id: id
                    .split_once(':')
                    .map(|(_, native)| native)
                    .unwrap_or(id)
                    .to_string(),
                source_name: source_name.to_string(),
                external_ids: doi
                    .map(|doi| vec![format!("doi:{doi}")])
                    .unwrap_or_default(),
            },
            SourceKind::Bibliography => SourceMetadata::Bibliography {
                authors: vec!["Ada Lovelace".to_string()],
                venue: Some("TestConf".to_string()),
                year: Some(2026),
                doi: doi.map(ToString::to_string),
                citation_count: None,
                native_id: id
                    .split_once(':')
                    .map(|(_, native)| native)
                    .unwrap_or(id)
                    .to_string(),
                source_name: source_name.to_string(),
                external_ids: doi
                    .map(|doi| vec![format!("doi:{doi}")])
                    .unwrap_or_default(),
            },
            _ => unreachable!("test helper only builds academic items"),
        };
        SourceItem {
            id: id.to_string(),
            kind,
            title: "Tool Calling Agents in Rust".to_string(),
            url: "https://example.test/tool-agent".to_string(),
            summary: summary.to_string(),
            evidence_snippet: summary.to_string(),
            tags: vec![],
            score: 0.0,
            score_reasons: Vec::new(),
            classification_reasons: Vec::new(),
            score_breakdown: Default::default(),
            published_or_updated_at: None,
            metadata,
        }
    }

    fn lineage(query_id: &str, source_item_id: &str) -> SourceQueryLineage {
        SourceQueryLineage {
            lineage_id: format!("lin-{query_id}"),
            source_item_id: source_item_id.to_string(),
            chapter_id: Some("ch-1".to_string()),
            source_kind: None,
            query_attempt_ids: vec![query_id.to_string()],
            returned_item_ids: vec![source_item_id.to_string()],
            merged_from_item_ids: Vec::new(),
        }
    }
}
