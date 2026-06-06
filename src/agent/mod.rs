//! Stage 3 agent node map.
//!
//! LLM-backed in Stage 3.1: planner and writer, only when LLM config is
//! enabled and usable.
//! Deterministic rule nodes: scoper, plan_critic, scouts, evidence,
//! coverage_critic, citation_auditor, and followup_router.
//! Writer refuses to produce a final template report when LLM drafting fails.

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
