use crate::model::{
    ChapterNode, CoverageGap, CoverageRecommendation, CoverageReport, EvidenceMemory, GapKind,
    QueryPortfolio, SourceKind,
};
use crate::run_policy::RunPolicy;

pub fn evaluate_coverage(
    chapters: &[ChapterNode],
    portfolio: &[QueryPortfolio],
    memory: &EvidenceMemory,
    policy: &RunPolicy,
) -> CoverageReport {
    if policy.skip_coverage_critic {
        return CoverageReport::pass();
    }

    let mut gaps = Vec::new();
    let mut out_of_scope_notice = Vec::new();
    for chapter in chapters {
        let items = memory.by_chapter(&chapter.id);
        if items.len() >= chapter.evidence_quota.min(2) && required_kinds_satisfied(chapter, &items)
        {
            continue;
        }
        let attempts = memory
            .query_attempts
            .iter()
            .filter(|attempt| attempt.chapter_id == chapter.id)
            .collect::<Vec<_>>();
        let all_failed =
            !attempts.is_empty() && attempts.iter().all(|attempt| attempt.error.is_some());
        if all_failed {
            let source_scope = if policy.academic_extra_enabled {
                "GitHub/arXiv/扩展学术源"
            } else {
                "GitHub/arXiv"
            };
            out_of_scope_notice.push(format!(
                "章节 `{}` 在 {source_scope} 当前查询下没有可用结果，可能超出当前来源能力或遇到远端错误。",
                chapter.title_zh,
            ));
            gaps.push(CoverageGap {
                chapter_id: chapter.id.clone(),
                gap_kind: GapKind::SourceGap,
                explanation: format!("当前 {source_scope} 查询未取得可用证据。"),
                recommended_queries: Vec::new(),
                severity: "medium".to_string(),
            });
        } else {
            let recommended_queries = portfolio
                .iter()
                .find(|item| item.chapter_id == chapter.id)
                .map(|item| {
                    item.github_queries
                        .iter()
                        .chain(item.arxiv_queries.iter())
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            gaps.push(CoverageGap {
                chapter_id: chapter.id.clone(),
                gap_kind: GapKind::QueryGap,
                explanation: format!(
                    "章节 `{}` 当前证据数量不足，建议由用户确认后创建增量研究运行。",
                    chapter.title_zh
                ),
                recommended_queries,
                severity: "low".to_string(),
            });
        }
    }

    let covered = chapters.len().saturating_sub(gaps.len());
    let overall_coverage_score = if chapters.is_empty() {
        1.0
    } else {
        covered as f64 / chapters.len() as f64
    };
    let recommendation = if gaps
        .iter()
        .any(|gap| matches!(gap.gap_kind, GapKind::SourceGap))
    {
        CoverageRecommendation::OutOfScope
    } else if gaps.is_empty() {
        CoverageRecommendation::NoAction
    } else {
        CoverageRecommendation::SuggestNewQuery
    };

    CoverageReport {
        gaps,
        out_of_scope_notice,
        overall_coverage_score,
        recommendation,
    }
}

fn required_kinds_satisfied(chapter: &ChapterNode, items: &[crate::model::EvidenceItem]) -> bool {
    if chapter.required_evidence_kinds.is_empty() {
        return true;
    }

    chapter.required_evidence_kinds.iter().all(|required| {
        items
            .iter()
            .any(|item| source_kind_matches(required, item.source_kind))
    })
}

fn source_kind_matches(required: &str, actual: SourceKind) -> bool {
    let required = required.trim().to_ascii_lowercase();
    match required.as_str() {
        "github" | "implementation" | "artifact" => actual == SourceKind::GitHub,
        "arxiv" | "preprint" => actual == SourceKind::Arxiv,
        "academic_index" | "academic-index" | "academicindex" | "semantic_scholar"
        | "semantic-scholar" => actual == SourceKind::AcademicIndex,
        "bibliography" | "dblp" => actual == SourceKind::Bibliography,
        "academic" => matches!(
            actual,
            SourceKind::Arxiv | SourceKind::AcademicIndex | SourceKind::Bibliography
        ),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::evaluate_coverage;
    use crate::model::{
        ChapterNode, CoverageRecommendation, EvidenceItem, EvidenceMemory, QueryPortfolio,
        SourceKind,
    };
    use crate::run_policy::RunPolicy;

    #[test]
    fn coverage_reports_query_gap_for_empty_chapter() {
        let chapters = vec![ChapterNode {
            id: "ch-1".to_string(),
            parent_id: None,
            title_zh: "核心方向".to_string(),
            research_question: "测试".to_string(),
            required_evidence_kinds: vec!["github".to_string()],
            evidence_quota: 2,
            sort_order: 1,
        }];
        let portfolio = vec![QueryPortfolio {
            chapter_id: "ch-1".to_string(),
            github_queries: vec!["rust agent".to_string()],
            arxiv_queries: Vec::new(),
            rationale: "测试".to_string(),
            budget: 2,
        }];

        let report = evaluate_coverage(
            &chapters,
            &portfolio,
            &EvidenceMemory::default(),
            &RunPolicy::default(),
        );

        assert_eq!(
            report.recommendation,
            CoverageRecommendation::SuggestNewQuery
        );
        assert_eq!(report.gaps.len(), 1);
    }

    #[test]
    fn coverage_accepts_academic_requirement_from_academic_index() {
        let chapters = vec![ChapterNode {
            id: "ch-1".to_string(),
            parent_id: None,
            title_zh: "论文方向".to_string(),
            research_question: "测试".to_string(),
            required_evidence_kinds: vec!["academic".to_string()],
            evidence_quota: 1,
            sort_order: 1,
        }];
        let memory = EvidenceMemory {
            items: vec![sample_evidence(SourceKind::AcademicIndex)],
            ..Default::default()
        };

        let report = evaluate_coverage(&chapters, &[], &memory, &RunPolicy::default());

        assert_eq!(report.recommendation, CoverageRecommendation::NoAction);
        assert!(report.gaps.is_empty());
    }

    #[test]
    fn coverage_rejects_bibliography_requirement_from_github_only() {
        let chapters = vec![ChapterNode {
            id: "ch-1".to_string(),
            parent_id: None,
            title_zh: "书目校验".to_string(),
            research_question: "测试".to_string(),
            required_evidence_kinds: vec!["bibliography".to_string()],
            evidence_quota: 1,
            sort_order: 1,
        }];
        let memory = EvidenceMemory {
            items: vec![sample_evidence(SourceKind::GitHub)],
            ..Default::default()
        };

        let report = evaluate_coverage(&chapters, &[], &memory, &RunPolicy::default());

        assert_eq!(
            report.recommendation,
            CoverageRecommendation::SuggestNewQuery
        );
        assert_eq!(report.gaps.len(), 1);
    }

    fn sample_evidence(source_kind: SourceKind) -> EvidenceItem {
        EvidenceItem {
            evidence_id: "ev-C1".to_string(),
            source_item_id: "test:item".to_string(),
            citation_id: "C1".to_string(),
            chapter_ids: vec!["ch-1".to_string()],
            query_attempt_ids: vec!["q-1".to_string()],
            source_kind,
            title: "Test Evidence".to_string(),
            url: "https://example.test/evidence".to_string(),
            evidence_note_zh: "测试证据".to_string(),
            evidence_snippet: "测试证据".to_string(),
            support_score: None,
        }
    }
}
