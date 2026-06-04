use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;

use crate::model::{CitationAuditReport, CitationLedger, EvidenceMemory, ReportDraft};

pub fn audit_citations(
    draft: &ReportDraft,
    ledger: &CitationLedger,
    memory: &EvidenceMemory,
) -> CitationAuditReport {
    let evidence_ids = memory
        .items
        .iter()
        .map(|item| item.evidence_id.as_str())
        .collect::<HashSet<_>>();
    let allowed_urls = ledger
        .citations
        .iter()
        .map(|citation| citation.url.as_str())
        .collect::<HashSet<_>>();
    let mut total_paragraphs = 0usize;
    let mut cited_paragraphs = 0usize;
    let mut unsupported_paragraph_warnings = Vec::new();
    let mut external_url_violations = Vec::new();

    for chapter in &draft.chapters {
        for (index, paragraph) in chapter.paragraphs.iter().enumerate() {
            total_paragraphs += 1;
            if paragraph.cited_evidence_ids.is_empty() {
                unsupported_paragraph_warnings.push(format!(
                    "章节 `{}` 第 {} 段未标注 evidence 引用。",
                    chapter.title_zh,
                    index + 1
                ));
            } else if paragraph
                .cited_evidence_ids
                .iter()
                .all(|id| evidence_ids.contains(id.as_str()))
            {
                cited_paragraphs += 1;
            } else {
                unsupported_paragraph_warnings.push(format!(
                    "章节 `{}` 第 {} 段包含未知 evidence id。",
                    chapter.title_zh,
                    index + 1
                ));
            }
            for url in extract_urls(&paragraph.text_zh) {
                if !allowed_urls.contains(url.as_str()) {
                    external_url_violations.push(url);
                }
            }
        }
    }

    let citation_coverage_ratio = if total_paragraphs == 0 {
        1.0
    } else {
        cited_paragraphs as f64 / total_paragraphs as f64
    };
    let source_kinds = memory
        .items
        .iter()
        .map(|item| item.source_kind)
        .collect::<HashSet<_>>();
    let source_diversity_score = (source_kinds.len() as f64 / 2.0).min(1.0);
    let mut freshness_warnings = Vec::new();
    if memory.items.is_empty() {
        freshness_warnings.push("当前 EvidenceMemory 为空，无法评估来源新鲜度。".to_string());
    }

    CitationAuditReport {
        url_whitelist_passed: external_url_violations.is_empty(),
        citation_coverage_ratio,
        source_diversity_score,
        freshness_warnings,
        unsupported_paragraph_warnings,
        external_url_violations,
    }
}

fn extract_urls(text: &str) -> Vec<String> {
    static URL_RE: OnceLock<Regex> = OnceLock::new();
    let url_re = URL_RE.get_or_init(|| {
        Regex::new(r#"https?://[A-Za-z0-9._~:/?#@!$&*+,;=%-]+"#).expect("URL regex should compile")
    });
    url_re
        .find_iter(text)
        .map(|match_| {
            match_
                .as_str()
                .trim_matches(|ch: char| {
                    matches!(
                        ch,
                        ')' | ']' | '}' | ',' | '.' | ';' | '。' | '，' | '；' | '）'
                    )
                })
                .to_string()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::model::{
        ChapterDraft, CitationAuditReport, CitationLedger, ParagraphWithCitations, ReportDraft,
    };
    use chrono::Utc;

    use super::audit_citations;

    #[test]
    fn audit_warns_on_uncited_paragraph() {
        let draft = ReportDraft {
            title_zh: "测试".to_string(),
            chapters: vec![ChapterDraft {
                chapter_id: "ch-1".to_string(),
                title_zh: "章节".to_string(),
                paragraphs: vec![ParagraphWithCitations {
                    text_zh: "无引用段落".to_string(),
                    cited_evidence_ids: Vec::new(),
                }],
            }],
            global_summary_zh: "摘要".to_string(),
            written_at: Utc::now(),
        };

        let report: CitationAuditReport =
            audit_citations(&draft, &CitationLedger::default(), &Default::default());

        assert_eq!(report.citation_coverage_ratio, 0.0);
        assert!(!report.unsupported_paragraph_warnings.is_empty());
    }
}
