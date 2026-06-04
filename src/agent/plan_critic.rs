use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::model::{ChapterNode, QueryPortfolio, ResearchBrief};
use crate::run_policy::RunPolicy;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanCritique {
    pub warnings: Vec<String>,
    pub suggestions: Vec<String>,
}

pub fn critique_plan(
    _brief: &ResearchBrief,
    chapters: &[ChapterNode],
    portfolio: &[QueryPortfolio],
    policy: &RunPolicy,
) -> PlanCritique {
    if policy.skip_plan_critic {
        return PlanCritique {
            warnings: Vec::new(),
            suggestions: vec!["PlanCritic 已按 RunPolicy 跳过。".to_string()],
        };
    }

    let mut warnings = Vec::new();
    let mut suggestions = Vec::new();
    if chapters.is_empty() {
        warnings.push("章节计划为空。".to_string());
    }
    if chapters.len() > policy.max_aspects_per_round {
        warnings.push(format!(
            "章节数量 {} 超过策略上限 {}。",
            chapters.len(),
            policy.max_aspects_per_round
        ));
    }

    let chapter_ids = chapters
        .iter()
        .map(|chapter| chapter.id.as_str())
        .collect::<HashSet<_>>();
    for item in portfolio {
        if !chapter_ids.contains(item.chapter_id.as_str()) {
            warnings.push(format!(
                "QueryPortfolio 指向未知章节 `{}`。",
                item.chapter_id
            ));
        }
        if item.github_queries.is_empty() && item.arxiv_queries.is_empty() {
            warnings.push(format!("章节 `{}` 没有可执行查询。", item.chapter_id));
        }
    }
    if warnings.is_empty() {
        suggestions.push("计划结构完整，可以进入用户确认。".to_string());
    }

    PlanCritique {
        warnings,
        suggestions,
    }
}
