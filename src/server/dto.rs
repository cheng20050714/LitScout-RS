use serde::{Deserialize, Serialize};

use crate::agent::orchestrator::StatefulRunEvent;
use crate::checkpoint::Checkpoint;
use crate::model::{
    ChapterNode, CitationAuditReport, CoverageReport, EvidenceMemory, QueryPortfolio,
};
use crate::run_policy::RunPolicy;
use crate::workflow_state::{ResearchRunRecord, ResearchRunState};

#[derive(Debug, Clone, Deserialize)]
pub struct CreateStatefulRunRequest {
    pub topic: String,
    #[serde(default)]
    pub policy: RunPolicy,
    #[serde(default)]
    pub config: FrontendConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ContinueStatefulRunRequest {
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

#[derive(Debug, Clone, Serialize)]
pub struct CheckpointListResponse {
    pub run_id: String,
    pub checkpoints: Vec<Checkpoint>,
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

#[derive(Debug, Clone, Serialize)]
pub struct ReadingLibraryResponse {
    pub items: Vec<crate::reading::models::ReadingLibrarySummary>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddReadingLibraryItemRequest {
    pub run_id: Option<String>,
    pub evidence: crate::model::EvidenceItem,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReadingLibraryItemResponse {
    pub item: crate::reading::models::ReadingLibraryItem,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GenerateReadingNoteRequest {
    #[serde(default)]
    pub config: FrontendConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PaperChatRequest {
    pub question: String,
    #[serde(default)]
    pub config: FrontendConfig,
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
    pub semantic_scholar_api_key: Option<String>,
}
