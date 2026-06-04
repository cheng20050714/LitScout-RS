use chrono::Utc;

use crate::model::{
    ChapterDraft, ChapterNode, CitationLedger, EvidenceItem, EvidenceMemory,
    ParagraphWithCitations, ReportDraft,
};

pub fn draft_report(topic: &str, chapters: &[ChapterNode], memory: &EvidenceMemory) -> ReportDraft {
    let chapter_drafts = chapters
        .iter()
        .map(|chapter| draft_chapter(chapter, &memory.by_chapter(&chapter.id)))
        .collect::<Vec<_>>();
    let global_summary_zh =
        format!("本报告围绕 `{topic}`，基于当前程序已经抓取到的 GitHub 与 arXiv 证据生成。");
    ReportDraft {
        title_zh: format!("LitScout-RS 调研报告：{topic}"),
        chapters: chapter_drafts,
        global_summary_zh,
        written_at: Utc::now(),
    }
}

fn draft_chapter(chapter: &ChapterNode, evidence_items: &[EvidenceItem]) -> ChapterDraft {
    let paragraphs = if evidence_items.is_empty() {
        vec![ParagraphWithCitations {
            text_zh: format!(
                "当前章节 `{}` 尚未收集到足够证据，建议查看 CoverageCritic 的缺口说明后决定是否增量研究。",
                chapter.title_zh
            ),
            cited_evidence_ids: Vec::new(),
        }]
    } else {
        evidence_items
            .iter()
            .take(4)
            .map(|item| ParagraphWithCitations {
                text_zh: format!(
                    "`{}` 是本章节的关键来源之一。{} 来源链接：{}",
                    item.title, item.evidence_note_zh, item.url
                ),
                cited_evidence_ids: vec![item.evidence_id.clone()],
            })
            .collect()
    };

    ChapterDraft {
        chapter_id: chapter.id.clone(),
        title_zh: chapter.title_zh.clone(),
        paragraphs,
    }
}

pub fn render_report_markdown(draft: &ReportDraft, citations: &CitationLedger) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", draft.title_zh));
    out.push_str("## 1. 全局摘要\n\n");
    out.push_str(&draft.global_summary_zh);
    out.push_str("\n\n");

    for (index, chapter) in draft.chapters.iter().enumerate() {
        out.push_str(&format!("## {}. {}\n\n", index + 2, chapter.title_zh));
        for paragraph in &chapter.paragraphs {
            out.push_str(&paragraph.text_zh);
            if !paragraph.cited_evidence_ids.is_empty() {
                out.push_str(" ");
                out.push_str(
                    &paragraph
                        .cited_evidence_ids
                        .iter()
                        .map(|id| format!("`{id}`"))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            }
            out.push_str("\n\n");
        }
    }

    out.push_str("## 引用账本\n\n");
    if citations.citations.is_empty() {
        out.push_str("- 暂无引用。\n");
    } else {
        for citation in &citations.citations {
            out.push_str(&format!(
                "- `{}` [{}]({}) ({:?})\n",
                citation.id, citation.title, citation.url, citation.source_kind
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::model::{ChapterNode, EvidenceMemory};

    use super::draft_report;

    #[test]
    fn writer_drafts_empty_chapter_with_warning_text() {
        let chapters = vec![ChapterNode {
            id: "ch-1".to_string(),
            parent_id: None,
            title_zh: "核心方向".to_string(),
            research_question: "测试".to_string(),
            required_evidence_kinds: vec!["github".to_string()],
            evidence_quota: 1,
            sort_order: 1,
        }];

        let draft = draft_report("Rust Agent", &chapters, &EvidenceMemory::default());

        assert_eq!(draft.chapters.len(), 1);
        assert!(draft.chapters[0].paragraphs[0]
            .text_zh
            .contains("尚未收集到足够证据"));
    }
}
