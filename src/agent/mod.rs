//! Stage 3 agent node map.
//!
//! LLM-backed in Stage 3: planner, only when LLM config is enabled and usable.
//! Deterministic rule nodes in Stage 3: scoper, plan_critic, scouts,
//! evidence, coverage_critic, writer, citation_auditor, and followup_router.
//! Writer keeps citation-preserving rule drafting as the Stage 3 delivery;
//! full LLM drafting is reserved for the next stage.

pub mod arxiv_scout;
pub mod citation_auditor;
pub mod coverage_critic;
pub mod evidence;
pub mod followup_router;
pub mod github_scout;
pub mod middleware;
pub mod orchestrator;
pub mod plan_critic;
pub mod planner;
pub mod report_chat;
pub mod scoper;
pub mod writer;
