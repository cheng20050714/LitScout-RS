use serde::{Deserialize, Serialize};

use crate::agent::followup_router::FollowupRoute;
use crate::agent::orchestrator::StatefulRunEvent;
use crate::agent::planner::{AspectOutput, PlanOutput};
use crate::checkpoint::Checkpoint;
use crate::model::{
    ChapterNode, Citation, CitationAuditReport, CoverageReport, EvidenceMemory, QualityReport,
    QueryPortfolio, SourceItem,
};
use crate::run_policy::RunPolicy;
use crate::workflow::WorkflowEvent;
use crate::workflow_state::{ResearchRunRecord, ResearchRunState};

#[derive(Debug, Clone, Deserialize)]
pub struct PlanRequest {
    pub topic: String,
    #[serde(default = "default_github_limit")]
    pub github_limit: usize,
    #[serde(default = "default_arxiv_limit")]
    pub arxiv_limit: usize,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub config: FrontendConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanResponse {
    pub plan_id: String,
    pub original_topic: String,
    pub language: String,
    pub aspects: Vec<AspectDto>,
    pub llm_generated: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AspectDto {
    pub name_zh: String,
    pub rationale_zh: String,
    pub github_query: String,
    pub arxiv_query: String,
    pub github_limit: usize,
    pub arxiv_limit: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlanReviseRequest {
    pub plan_id: String,
    pub current_plan: PlanResponse,
    pub user_feedback: String,
    #[serde(default)]
    pub config: FrontendConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RunRequest {
    pub plan_id: String,
    pub current_plan: PlanResponse,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub config: FrontendConfig,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunResponse {
    pub session_id: String,
    pub output_report: String,
    pub session_path: Option<String>,
    pub report_markdown: String,
    pub warnings: Vec<String>,
    pub citations: Vec<Citation>,
    pub ranked_items: Vec<SourceItem>,
    pub quality: QualityReport,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateStatefulRunRequest {
    pub topic: String,
    #[serde(default)]
    pub policy: RunPolicy,
    #[serde(default)]
    pub config: FrontendConfig,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatefulRunResponse {
    pub run_id: String,
    pub topic: String,
    pub state: ResearchRunState,
    pub run: ResearchRunRecord,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum StatefulRunStreamEvent {
    Agent(StatefulRunEvent),
    RunReady(Box<StatefulRunResponse>),
    RunFailed { error: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct EvidenceResponse {
    pub run_id: String,
    pub evidence_memory: EvidenceMemory,
}

#[derive(Debug, Clone, Serialize)]
pub struct CoverageResponse {
    pub run_id: String,
    pub coverage_report: CoverageReport,
}

#[derive(Debug, Clone, Serialize)]
pub struct CitationAuditResponse {
    pub run_id: String,
    pub citation_audit: CitationAuditReport,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BranchRunRequest {
    pub checkpoint_id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReviseStatefulPlanRequest {
    pub chapters: Option<Vec<ChapterNode>>,
    pub query_portfolio: Option<Vec<QueryPortfolio>>,
    pub user_feedback: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StatefulFollowupRequest {
    pub question: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatefulFollowupResponse {
    pub run_id: String,
    pub route: FollowupRoute,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckpointListResponse {
    pub run_id: String,
    pub checkpoints: Vec<Checkpoint>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum RunStreamEvent {
    Workflow(WorkflowEvent),
    ReportReady(RunResponse),
    RunFailed { error: String },
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReportChatRequest {
    pub question: String,
    pub report_markdown: String,
    #[serde(default)]
    pub config: FrontendConfig,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportChatResponse {
    pub answer: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReportTranslateRequest {
    pub report_markdown: String,
    #[serde(default)]
    pub config: FrontendConfig,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportTranslateResponse {
    pub translated_markdown: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum ChatStreamEvent {
    Delta { text: String },
    Done,
    Failed { error: String },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FrontendConfig {
    pub deepseek_api_key: Option<String>,
    pub deepseek_base_url: Option<String>,
    pub deepseek_model: Option<String>,
    pub deepseek_side_model: Option<String>,
    pub github_token: Option<String>,
}

impl From<PlanOutput> for PlanResponse {
    fn from(plan: PlanOutput) -> Self {
        Self {
            plan_id: plan.plan_id,
            original_topic: plan.original_topic,
            language: default_language(),
            aspects: plan.aspects.into_iter().map(AspectDto::from).collect(),
            llm_generated: plan.llm_generated,
            warnings: plan.warnings,
        }
    }
}

impl From<PlanResponse> for PlanOutput {
    fn from(plan: PlanResponse) -> Self {
        Self {
            plan_id: plan.plan_id,
            original_topic: plan.original_topic,
            aspects: plan.aspects.into_iter().map(AspectOutput::from).collect(),
            llm_generated: plan.llm_generated,
            warnings: plan.warnings,
        }
    }
}

impl From<AspectOutput> for AspectDto {
    fn from(aspect: AspectOutput) -> Self {
        Self {
            name_zh: aspect.name_zh,
            rationale_zh: aspect.rationale_zh,
            github_query: aspect.github_query,
            arxiv_query: aspect.arxiv_query,
            github_limit: aspect.github_limit,
            arxiv_limit: aspect.arxiv_limit,
        }
    }
}

impl From<AspectDto> for AspectOutput {
    fn from(aspect: AspectDto) -> Self {
        Self {
            name_zh: aspect.name_zh,
            rationale_zh: aspect.rationale_zh,
            github_query: aspect.github_query,
            arxiv_query: aspect.arxiv_query,
            github_limit: aspect.github_limit,
            arxiv_limit: aspect.arxiv_limit,
        }
    }
}

fn default_github_limit() -> usize {
    10
}

fn default_arxiv_limit() -> usize {
    10
}

fn default_language() -> String {
    "zh-CN".to_string()
}

#[cfg(test)]
mod tests {
    use super::{AspectDto, PlanResponse};
    use crate::agent::planner::PlanOutput;

    #[test]
    fn converts_plan_response_to_internal_plan() {
        let response = PlanResponse {
            plan_id: "plan-1".to_string(),
            original_topic: "Rust Agent".to_string(),
            language: "zh-CN".to_string(),
            aspects: vec![AspectDto {
                name_zh: "核心方向".to_string(),
                rationale_zh: "测试".to_string(),
                github_query: "rust agent".to_string(),
                arxiv_query: "rust agent".to_string(),
                github_limit: 5,
                arxiv_limit: 4,
            }],
            llm_generated: true,
            warnings: Vec::new(),
        };

        let plan = PlanOutput::from(response);

        assert_eq!(plan.plan_id, "plan-1");
        assert_eq!(plan.aspects[0].github_query, "rust agent");
    }
}
