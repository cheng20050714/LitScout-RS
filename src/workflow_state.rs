use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::model::{
    CitationAuditReport, CoverageReport, EvidenceMemory, ReportDraft, ResearchBrief,
};
use crate::run_policy::RunPolicy;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResearchRunState {
    Created,
    PlanReady,
    Fetching,
    EvidenceReady,
    SynthesisReady,
    Completed,
    Failed,
}

impl ResearchRunState {
    pub fn can_transition_to(&self, next: &ResearchRunState) -> bool {
        use ResearchRunState::*;
        matches!(
            (self, next),
            (Created, PlanReady)
                | (PlanReady, Fetching)
                | (Fetching, EvidenceReady)
                | (EvidenceReady, SynthesisReady)
                | (SynthesisReady, Completed)
                | (Created, Failed)
                | (PlanReady, Failed)
                | (Fetching, Failed)
                | (EvidenceReady, Failed)
                | (SynthesisReady, Failed)
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchRunRecord {
    pub run_id: String,
    pub topic: String,
    pub state: ResearchRunState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub policy: RunPolicy,
    pub brief: Option<ResearchBrief>,
    pub chapters: Vec<crate::model::ChapterNode>,
    pub query_portfolio: Vec<crate::model::QueryPortfolio>,
    pub plan_warnings: Vec<String>,
    pub evidence_memory: Option<EvidenceMemory>,
    pub coverage_report: Option<CoverageReport>,
    pub report_draft: Option<ReportDraft>,
    pub citation_audit: Option<CitationAuditReport>,
    pub report_markdown: Option<String>,
    #[serde(default)]
    pub output_report: Option<String>,
    pub warnings: Vec<String>,
    pub origin_run_id: Option<String>,
    pub origin_checkpoint_id: Option<String>,
}

impl ResearchRunRecord {
    pub fn new(run_id: String, topic: String, policy: RunPolicy) -> Self {
        let now = Utc::now();
        Self {
            run_id,
            topic,
            state: ResearchRunState::Created,
            created_at: now,
            updated_at: now,
            policy: policy.bounded(),
            brief: None,
            chapters: Vec::new(),
            query_portfolio: Vec::new(),
            plan_warnings: Vec::new(),
            evidence_memory: None,
            coverage_report: None,
            report_draft: None,
            citation_audit: None,
            report_markdown: None,
            output_report: None,
            warnings: Vec::new(),
            origin_run_id: None,
            origin_checkpoint_id: None,
        }
    }

    pub fn transition_to(&mut self, next: ResearchRunState) -> bool {
        if self.state.can_transition_to(&next) {
            self.state = next;
            self.updated_at = Utc::now();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ResearchRunState::*;

    #[test]
    fn state_machine_accepts_linear_path_and_failures() {
        assert!(Created.can_transition_to(&PlanReady));
        assert!(PlanReady.can_transition_to(&Fetching));
        assert!(Fetching.can_transition_to(&EvidenceReady));
        assert!(EvidenceReady.can_transition_to(&SynthesisReady));
        assert!(SynthesisReady.can_transition_to(&Completed));
        assert!(Fetching.can_transition_to(&Failed));
    }

    #[test]
    fn state_machine_rejects_loops_and_backwards_edges() {
        assert!(!EvidenceReady.can_transition_to(&Fetching));
        assert!(!Completed.can_transition_to(&Failed));
        assert!(!PlanReady.can_transition_to(&Completed));
    }
}
