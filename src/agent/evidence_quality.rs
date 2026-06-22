use std::collections::{BTreeMap, BTreeSet};

use crate::model::{
    EvidenceSelectionReport, QueryAttempt, RejectedEvidenceItem, RejectionReasonCount, SearchQuery,
    SourceItem, SourceKind, SourceKindCount, SourceMetadata, SourceQueryLineage,
};

const REASON_EMPTY_CONTENT: &str = "empty_content_without_verifiable_metadata";
const REASON_NO_SUCCESSFUL_LINEAGE: &str = "no_successful_lineage";
const REASON_NO_TOPIC_MATCH: &str = "no_topic_match";
const REASON_ACADEMIC_INDEX_NO_TITLE_OR_SUMMARY_MATCH: &str =
    "academic_index_no_title_or_summary_match";
const REASON_BIBLIOGRAPHY_WEAK_TITLE_MATCH: &str = "bibliography_weak_title_match";
const REASON_BIBLIOGRAPHY_MISSING_METADATA: &str = "bibliography_missing_metadata";
const REASON_BIBLIOGRAPHY_RATIO_LIMIT: &str = "bibliography_ratio_limit";
const REASON_RUST_PLANT_DISEASE_AMBIGUITY: &str = "rust_plant_disease_ambiguity";

#[derive(Debug, Clone)]
pub struct EvidenceQualityResult {
    pub accepted_items: Vec<SourceItem>,
    pub rejected_items: Vec<RejectedEvidenceItem>,
    pub selection_report: EvidenceSelectionReport,
}

pub fn apply_quality_gate(
    query: &SearchQuery,
    ranked_items: &[SourceItem],
    source_lineage: &[SourceQueryLineage],
    query_attempts: &[QueryAttempt],
    raw_item_count: usize,
    merged_item_count: usize,
) -> EvidenceQualityResult {
    let terms = query_terms(&query.topic);
    let strong_representative_ids = strong_representative_ids(ranked_items, source_lineage);
    let successful_attempt_ids = query_attempts
        .iter()
        .filter(|attempt| attempt.error.is_none())
        .map(|attempt| attempt.query_id.as_str())
        .collect::<BTreeSet<_>>();
    let lineage_by_source = source_lineage
        .iter()
        .map(|lineage| (lineage.source_item_id.as_str(), lineage))
        .collect::<BTreeMap<_, _>>();

    let mut accepted_items = Vec::new();
    let mut rejected_items = Vec::new();
    for item in ranked_items {
        let lineage = lineage_by_source.get(item.id.as_str());
        let has_successful_lineage = lineage.is_some_and(|lineage| {
            lineage
                .query_attempt_ids
                .iter()
                .any(|id| successful_attempt_ids.contains(id.as_str()))
        });
        if !has_successful_lineage
            && matches!(
                item.kind,
                SourceKind::AcademicIndex | SourceKind::Bibliography
            )
        {
            rejected_items.push(rejected_item(item, REASON_NO_SUCCESSFUL_LINEAGE));
            continue;
        }

        match rejection_reason(item, &terms, &strong_representative_ids) {
            Some(reason) => rejected_items.push(rejected_item(item, reason)),
            None => accepted_items.push(item.clone()),
        }
    }

    enforce_bibliography_ratio(&mut accepted_items, &mut rejected_items);

    let selection_report = build_selection_report(
        raw_item_count,
        merged_item_count,
        ranked_items.len(),
        &accepted_items,
        &rejected_items,
    );

    EvidenceQualityResult {
        accepted_items,
        rejected_items,
        selection_report,
    }
}

fn rejection_reason(
    item: &SourceItem,
    terms: &[String],
    strong_representative_ids: &BTreeSet<String>,
) -> Option<&'static str> {
    if !matches!(
        item.kind,
        SourceKind::AcademicIndex | SourceKind::Bibliography
    ) {
        return None;
    }
    if strong_representative_ids.contains(&item.id) {
        return None;
    }
    if content_is_empty(item) && !has_verifiable_metadata(item) {
        return Some(REASON_EMPTY_CONTENT);
    }
    if terms.is_empty() {
        return None;
    }
    if item.score_breakdown.keyword_score == 0.0 {
        return Some(REASON_NO_TOPIC_MATCH);
    }
    if rust_language_query_looks_like_plant_disease_result(item, terms) {
        return Some(REASON_RUST_PLANT_DISEASE_AMBIGUITY);
    }

    let title_hits = keyword_hits(&item.title, terms);
    let summary_hits =
        keyword_hits(&item.summary, terms) + keyword_hits(&item.evidence_snippet, terms);
    match item.kind {
        SourceKind::AcademicIndex if title_hits + summary_hits == 0 => {
            Some(REASON_ACADEMIC_INDEX_NO_TITLE_OR_SUMMARY_MATCH)
        }
        SourceKind::Bibliography if title_hits < 2 => Some(REASON_BIBLIOGRAPHY_WEAK_TITLE_MATCH),
        SourceKind::Bibliography if !has_bibliography_metadata(item) => {
            Some(REASON_BIBLIOGRAPHY_MISSING_METADATA)
        }
        _ => None,
    }
}

fn enforce_bibliography_ratio(
    accepted_items: &mut Vec<SourceItem>,
    rejected_items: &mut Vec<RejectedEvidenceItem>,
) {
    if accepted_items.is_empty() {
        return;
    }

    let allowed_bibliography = accepted_items.len().div_ceil(5).max(1);
    let mut bibliography_items = accepted_items
        .iter()
        .filter(|item| item.kind == SourceKind::Bibliography)
        .cloned()
        .collect::<Vec<_>>();
    if bibliography_items.len() <= allowed_bibliography {
        return;
    }

    bibliography_items.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    let keep_bibliography_ids = bibliography_items
        .into_iter()
        .take(allowed_bibliography)
        .map(|item| item.id)
        .collect::<BTreeSet<_>>();

    let mut filtered = Vec::with_capacity(accepted_items.len());
    for item in accepted_items.drain(..) {
        if item.kind == SourceKind::Bibliography && !keep_bibliography_ids.contains(&item.id) {
            rejected_items.push(rejected_item(&item, REASON_BIBLIOGRAPHY_RATIO_LIMIT));
            continue;
        }
        filtered.push(item);
    }
    *accepted_items = filtered;
}

fn build_selection_report(
    raw_item_count: usize,
    merged_item_count: usize,
    ranked_item_count: usize,
    accepted_items: &[SourceItem],
    rejected_items: &[RejectedEvidenceItem],
) -> EvidenceSelectionReport {
    EvidenceSelectionReport {
        raw_item_count,
        merged_item_count,
        ranked_item_count,
        accepted_item_count: accepted_items.len(),
        rejected_item_count: rejected_items.len(),
        accepted_by_source_kind: source_kind_counts(accepted_items.iter().map(|item| item.kind)),
        rejected_by_source_kind: source_kind_counts(
            rejected_items.iter().map(|item| item.source_kind),
        ),
        rejection_reasons: rejection_reason_counts(rejected_items),
        rejected_items: rejected_items.to_vec(),
    }
}

fn source_kind_counts(kinds: impl Iterator<Item = SourceKind>) -> Vec<SourceKindCount> {
    let mut counts = BTreeMap::<String, (SourceKind, usize)>::new();
    for kind in kinds {
        let key = format!("{kind:?}");
        let entry = counts.entry(key).or_insert((kind, 0));
        entry.1 += 1;
    }
    counts
        .into_values()
        .map(|(source_kind, count)| SourceKindCount { source_kind, count })
        .collect()
}

fn rejection_reason_counts(rejected_items: &[RejectedEvidenceItem]) -> Vec<RejectionReasonCount> {
    let mut counts = BTreeMap::<String, usize>::new();
    for item in rejected_items {
        *counts.entry(item.reason.clone()).or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(reason, count)| RejectionReasonCount { reason, count })
        .collect()
}

fn rejected_item(item: &SourceItem, reason: &'static str) -> RejectedEvidenceItem {
    RejectedEvidenceItem {
        source_item_id: item.id.clone(),
        title: item.title.clone(),
        source_kind: item.kind,
        source_name: source_name(item),
        score: item.score,
        reason: reason.to_string(),
    }
}

fn strong_representative_ids(
    ranked_items: &[SourceItem],
    source_lineage: &[SourceQueryLineage],
) -> BTreeSet<String> {
    let strong_ids = ranked_items
        .iter()
        .filter(|item| matches!(item.kind, SourceKind::GitHub | SourceKind::Arxiv))
        .map(|item| item.id.as_str())
        .collect::<BTreeSet<_>>();
    source_lineage
        .iter()
        .filter(|lineage| strong_ids.contains(lineage.source_item_id.as_str()))
        .flat_map(|lineage| lineage.merged_from_item_ids.iter().cloned())
        .collect()
}

fn content_is_empty(item: &SourceItem) -> bool {
    item.summary.trim().is_empty() && item.evidence_snippet.trim().is_empty()
}

fn has_verifiable_metadata(item: &SourceItem) -> bool {
    match &item.metadata {
        SourceMetadata::AcademicIndex {
            venue, year, doi, ..
        }
        | SourceMetadata::Bibliography {
            venue, year, doi, ..
        } => {
            doi.as_deref().is_some_and(|doi| !doi.trim().is_empty())
                || venue
                    .as_deref()
                    .is_some_and(|venue| !venue.trim().is_empty())
                || year.is_some()
        }
        _ => true,
    }
}

fn has_bibliography_metadata(item: &SourceItem) -> bool {
    match &item.metadata {
        SourceMetadata::Bibliography {
            authors,
            venue,
            year,
            doi,
            ..
        } => {
            doi.as_deref().is_some_and(|doi| !doi.trim().is_empty())
                || venue
                    .as_deref()
                    .is_some_and(|venue| !venue.trim().is_empty())
                || year.is_some()
                || !authors.is_empty()
        }
        _ => true,
    }
}

fn source_name(item: &SourceItem) -> String {
    match &item.metadata {
        SourceMetadata::AcademicIndex { source_name, .. }
        | SourceMetadata::Bibliography { source_name, .. } => source_name.clone(),
        SourceMetadata::GitHub { .. } => "github".to_string(),
        SourceMetadata::Arxiv { .. } => "arxiv".to_string(),
    }
}

fn rust_language_query_looks_like_plant_disease_result(
    item: &SourceItem,
    terms: &[String],
) -> bool {
    if !terms.iter().any(|term| term == "rust") {
        return false;
    }
    let text =
        format!("{} {} {}", item.title, item.summary, item.evidence_snippet).to_ascii_lowercase();
    if text.contains("rust language")
        || text.contains("rust programming")
        || text.contains("rust-lang")
        || text.contains("github")
        || text.contains("crate")
        || text.contains("compiler")
    {
        return false;
    }
    let disease_terms = [
        "leaf rust",
        "coffee rust",
        "coffee leaf",
        "wheat rust",
        "stripe rust",
        "stem rust",
        "yellow rust",
        "plant disease",
        "fungal",
        "fungus",
        "pathogen",
        "cultivar",
        "cultivars",
        "genome",
        "genomic",
        "transposable",
        "wheat",
        "coffee",
        "barley",
        "soybean",
        "puccinia",
        "hemileia",
    ];
    disease_terms.iter().any(|term| text.contains(term))
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

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use super::apply_quality_gate;
    use crate::model::{
        QueryAttempt, ScoreBreakdown, SearchQuery, SourceItem, SourceKind, SourceMetadata,
        SourceQueryLineage,
    };

    fn dt() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-05-30T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn rejects_high_citation_academic_index_without_topic_match() {
        let mut item = academic_index(
            "semantic_scholar:off-topic",
            "Diffusion Models",
            "Image generation",
        );
        item.metadata = SourceMetadata::AcademicIndex {
            authors: vec!["Ada".to_string()],
            venue: Some("CVPR".to_string()),
            year: Some(2024),
            doi: Some("10.1/off-topic".to_string()),
            citation_count: Some(10_000),
            native_id: "off-topic".to_string(),
            source_name: "semantic_scholar".to_string(),
            external_ids: Vec::new(),
        };

        let result = apply_quality_gate(
            &query(),
            &[item],
            &[lineage("semantic_scholar:off-topic", "ss-1")],
            &[attempt("ss-1")],
            1,
            1,
        );

        assert!(result.accepted_items.is_empty());
        assert_eq!(result.rejected_items[0].reason, "no_topic_match");
    }

    #[test]
    fn accepts_academic_index_with_title_or_summary_match() {
        let item = academic_index(
            "semantic_scholar:agent",
            "Tool Calling Agents",
            "A survey of agent planning.",
        );

        let result = apply_quality_gate(
            &query(),
            &[item],
            &[lineage("semantic_scholar:agent", "ss-1")],
            &[attempt("ss-1")],
            1,
            1,
        );

        assert_eq!(result.accepted_items.len(), 1);
        assert!(result.rejected_items.is_empty());
    }

    #[test]
    fn rejects_bibliography_when_only_metadata_matches() {
        let mut item = bibliography("crossref:1", "Unrelated Systems", "Bibliographic metadata");
        item.metadata = SourceMetadata::Bibliography {
            authors: vec!["Rust Agent".to_string()],
            venue: Some("TestConf".to_string()),
            year: Some(2025),
            doi: Some("10.1/unrelated".to_string()),
            citation_count: None,
            native_id: "10.1/unrelated".to_string(),
            source_name: "crossref".to_string(),
            external_ids: Vec::new(),
        };
        item.score_breakdown.keyword_score = 1.0;

        let result = apply_quality_gate(
            &query(),
            &[item],
            &[lineage("crossref:1", "cr-1")],
            &[attempt("cr-1")],
            1,
            1,
        );

        assert!(result.accepted_items.is_empty());
        assert_eq!(
            result.rejected_items[0].reason,
            "bibliography_weak_title_match"
        );
    }

    #[test]
    fn accepts_bibliography_with_strong_title_and_metadata() {
        let item = bibliography(
            "dblp:1",
            "Rust Agent Tool Calling",
            "Bibliographic metadata: TestConf, 2025.",
        );

        let result = apply_quality_gate(
            &query(),
            &[item],
            &[lineage("dblp:1", "db-1")],
            &[attempt("db-1")],
            1,
            1,
        );

        assert_eq!(result.accepted_items.len(), 1);
    }

    #[test]
    fn bibliography_ratio_keeps_highest_scored_records() {
        let strong = bibliography(
            "crossref:strong",
            "Rust Agent Framework for Tool Calling",
            "Bibliographic metadata: TestConf, 2025.",
        );
        let mut weak = bibliography(
            "crossref:weak",
            "Rust Agent Runtime Metadata",
            "Bibliographic metadata: TestConf, 2024.",
        );
        weak.score = 1.0;
        let mut strong = strong;
        strong.score = 20.0;
        let github = github_item();
        let arxiv = arxiv_item();

        let result = apply_quality_gate(
            &query(),
            &[weak, github, arxiv, strong],
            &[
                lineage("crossref:weak", "cr-1"),
                lineage("crossref:strong", "cr-2"),
            ],
            &[attempt("cr-1"), attempt("cr-2")],
            4,
            4,
        );

        assert!(result
            .accepted_items
            .iter()
            .any(|item| item.id == "crossref:strong"));
        assert!(!result
            .accepted_items
            .iter()
            .any(|item| item.id == "crossref:weak"));
        assert_eq!(
            result
                .rejected_items
                .iter()
                .find(|item| item.source_item_id == "crossref:weak")
                .expect("weak bibliography should be rejected")
                .reason,
            "bibliography_ratio_limit"
        );
    }

    #[test]
    fn rejects_plant_disease_rust_false_positive_for_rust_language_query() {
        let item = bibliography(
            "crossref:coffee-rust",
            "Rust Agent Genomic Features in Coffee Leaf Rust",
            "Coffee leaf rust plant disease study.",
        );

        let result = apply_quality_gate(
            &query(),
            &[item],
            &[lineage("crossref:coffee-rust", "cr-1")],
            &[attempt("cr-1")],
            1,
            1,
        );

        assert!(result.accepted_items.is_empty());
        assert_eq!(
            result.rejected_items[0].reason,
            "rust_plant_disease_ambiguity"
        );
    }

    #[test]
    fn keeps_github_and_arxiv_even_without_topic_match() {
        let github = github_item();

        let result = apply_quality_gate(&query(), &[github], &[], &[], 1, 1);

        assert_eq!(result.accepted_items.len(), 1);
        assert!(result.rejected_items.is_empty());
    }

    fn query() -> SearchQuery {
        SearchQuery {
            topic: "rust agent".to_string(),
            github_limit: 10,
            arxiv_limit: 10,
        }
    }

    fn attempt(query_id: &str) -> QueryAttempt {
        QueryAttempt {
            query_id: query_id.to_string(),
            source: "semantic_scholar".to_string(),
            query: "rust agent".to_string(),
            chapter_id: "ch-1".to_string(),
            round: 1,
            started_at: dt(),
            finished_at: Some(dt()),
            result_count: 1,
            source_kind: Some(SourceKind::AcademicIndex),
            http_status: None,
            rate_limit_info: None,
            parser_error: None,
            is_citeable: true,
            error: None,
        }
    }

    fn lineage(source_item_id: &str, query_id: &str) -> SourceQueryLineage {
        SourceQueryLineage {
            lineage_id: format!("lin-{source_item_id}"),
            source_item_id: source_item_id.to_string(),
            chapter_id: Some("ch-1".to_string()),
            source_kind: Some(SourceKind::AcademicIndex),
            query_attempt_ids: vec![query_id.to_string()],
            returned_item_ids: vec![source_item_id.to_string()],
            merged_from_item_ids: Vec::new(),
        }
    }

    fn academic_index(id: &str, title: &str, summary: &str) -> SourceItem {
        SourceItem {
            id: id.to_string(),
            kind: SourceKind::AcademicIndex,
            title: title.to_string(),
            url: "https://example.com/paper".to_string(),
            summary: summary.to_string(),
            evidence_snippet: summary.to_string(),
            tags: Vec::new(),
            score: 0.0,
            score_reasons: Vec::new(),
            classification_reasons: Vec::new(),
            score_breakdown: ScoreBreakdown {
                keyword_score: if title.to_ascii_lowercase().contains("agent")
                    || summary.to_ascii_lowercase().contains("agent")
                    || title.to_ascii_lowercase().contains("rust")
                    || summary.to_ascii_lowercase().contains("rust")
                {
                    4.0
                } else {
                    0.0
                },
                popularity_score: 0.0,
                recency_score: 0.0,
                source_bonus: 0.0,
            },
            published_or_updated_at: None,
            metadata: SourceMetadata::AcademicIndex {
                authors: vec!["Ada".to_string()],
                venue: Some("TestConf".to_string()),
                year: Some(2025),
                doi: None,
                citation_count: Some(10),
                native_id: id.to_string(),
                source_name: "semantic_scholar".to_string(),
                external_ids: Vec::new(),
            },
        }
    }

    fn bibliography(id: &str, title: &str, summary: &str) -> SourceItem {
        SourceItem {
            id: id.to_string(),
            kind: SourceKind::Bibliography,
            title: title.to_string(),
            url: "https://doi.org/10.1/test".to_string(),
            summary: summary.to_string(),
            evidence_snippet: summary.to_string(),
            tags: Vec::new(),
            score: 0.0,
            score_reasons: Vec::new(),
            classification_reasons: Vec::new(),
            score_breakdown: ScoreBreakdown {
                keyword_score: 8.0,
                popularity_score: 0.0,
                recency_score: 0.0,
                source_bonus: 0.0,
            },
            published_or_updated_at: None,
            metadata: SourceMetadata::Bibliography {
                authors: vec!["Ada".to_string()],
                venue: Some("TestConf".to_string()),
                year: Some(2025),
                doi: Some("10.1/test".to_string()),
                citation_count: None,
                native_id: id.to_string(),
                source_name: "dblp".to_string(),
                external_ids: Vec::new(),
            },
        }
    }

    fn github_item() -> SourceItem {
        SourceItem {
            id: "github:acme/unrelated".to_string(),
            kind: SourceKind::GitHub,
            title: "acme/unrelated".to_string(),
            url: "https://github.com/acme/unrelated".to_string(),
            summary: "No matching words".to_string(),
            evidence_snippet: "No matching words".to_string(),
            tags: Vec::new(),
            score: 0.0,
            score_reasons: Vec::new(),
            classification_reasons: Vec::new(),
            score_breakdown: ScoreBreakdown::default(),
            published_or_updated_at: None,
            metadata: SourceMetadata::GitHub {
                stars: 1,
                forks: 0,
                language: None,
                topics: Vec::new(),
            },
        }
    }

    fn arxiv_item() -> SourceItem {
        SourceItem {
            id: "arxiv:2601.00001".to_string(),
            kind: SourceKind::Arxiv,
            title: "Unrelated arXiv Paper".to_string(),
            url: "https://arxiv.org/abs/2601.00001".to_string(),
            summary: "No matching words".to_string(),
            evidence_snippet: "No matching words".to_string(),
            tags: Vec::new(),
            score: 0.0,
            score_reasons: Vec::new(),
            classification_reasons: Vec::new(),
            score_breakdown: ScoreBreakdown::default(),
            published_or_updated_at: None,
            metadata: SourceMetadata::Arxiv {
                authors: Vec::new(),
                categories: Vec::new(),
            },
        }
    }
}
