use crate::model::{
    ChapterNode, CoverageGap, CoverageRecommendation, CoverageReport, EvidenceMemory, GapKind,
    QueryPortfolio,
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
        if items.len() >= chapter.evidence_quota.min(2) {
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
            out_of_scope_notice.push(format!(
                "章节 `{}` 在 GitHub/arXiv 当前查询下没有可用结果，可能超出当前双源能力或遇到远端错误。",
                chapter.title_zh
            ));
            gaps.push(CoverageGap {
                chapter_id: chapter.id.clone(),
                gap_kind: GapKind::SourceGap,
                explanation: "当前 GitHub/arXiv 查询未取得可用证据。".to_string(),
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

#[cfg(test)]
mod tests {
    use super::evaluate_coverage;
    use crate::model::{ChapterNode, CoverageRecommendation, EvidenceMemory, QueryPortfolio};
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
}
