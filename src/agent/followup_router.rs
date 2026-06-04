use serde::{Deserialize, Serialize};

use crate::model::EvidenceMemory;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FollowupRoute {
    Answer { answer: String },
    IncrementalResearchRequired { reason: String },
}

pub fn route_followup(
    question: &str,
    memory: &EvidenceMemory,
    report_markdown: &str,
) -> FollowupRoute {
    if memory.items.is_empty() {
        return FollowupRoute::IncrementalResearchRequired {
            reason: "当前运行没有可用 EvidenceMemory，无法基于证据回答。".to_string(),
        };
    }
    let asks_for_more = ["补充", "更多", "最新", "重新搜索", "增量", "没有提到"]
        .iter()
        .any(|marker| question.contains(marker));
    if asks_for_more {
        return FollowupRoute::IncrementalResearchRequired {
            reason: "该问题可能需要新增 GitHub/arXiv 证据，建议创建增量研究运行。".to_string(),
        };
    }

    let top_sources = memory
        .items
        .iter()
        .take(3)
        .map(|item| format!("- [{}]({})", item.title, item.url))
        .collect::<Vec<_>>()
        .join("\n");
    let summary = report_markdown
        .lines()
        .take(8)
        .collect::<Vec<_>>()
        .join("\n");
    FollowupRoute::Answer {
        answer: format!(
            "基于当前 session 已有证据，可以先从以下来源回答：\n{top_sources}\n\n报告上下文摘录：\n{summary}"
        ),
    }
}
