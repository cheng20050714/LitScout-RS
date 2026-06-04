use std::path::{Path, PathBuf};

use chrono::Utc;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::agent::{
    arxiv_scout, citation_auditor, coverage_critic, evidence, followup_router, github_scout,
    middleware::{
        AgentContext, AgentMiddleware, CitationGuard, JsonSchemaGuard, TokenBudgetTracker,
    },
    plan_critic, planner, scoper, writer,
};
use crate::checkpoint::{self, Checkpoint, CheckpointSnapshot};
use crate::config::{AppConfig, LlmConfig};
use crate::error::{AppError, Result};
use crate::model::{
    ChapterNode, CoverageReport, EvidenceMemory, QueryAttempt, QueryPortfolio, SearchQuery,
};
use crate::run_policy::RunPolicy;
use crate::trace::{stable_hash, token_estimate, TraceEvent, TraceWriter};
use crate::workflow_state::{ResearchRunRecord, ResearchRunState};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum StatefulRunEvent {
    StateChanged {
        state: ResearchRunState,
    },
    CheckpointCreated {
        checkpoint_id: String,
        state: ResearchRunState,
    },
    EvidenceReady {
        total: usize,
    },
    CoverageReady {
        gaps: usize,
    },
    CitationAuditReady {
        citation_coverage_ratio: f64,
    },
    Completed {
        run_id: String,
    },
    Failed {
        error: String,
    },
}

pub async fn create_run(
    topic: String,
    app_config: AppConfig,
    llm_config: LlmConfig,
    policy: RunPolicy,
) -> Result<ResearchRunRecord> {
    let run_id = format!("run-{}", Uuid::new_v4());
    let run_dir = run_dir(&app_config, &run_id);
    tokio::fs::create_dir_all(&run_dir).await?;
    let trace = TraceWriter::new(&run_dir).await?;
    let mut run = ResearchRunRecord::new(run_id, topic.clone(), policy.bounded());
    save_run(&app_config, &run).await?;
    let token_budget = TokenBudgetTracker::new();
    let json_guard = JsonSchemaGuard;

    let scoper_input = format!("topic={topic}; policy={:?}", run.policy);
    let scoper_ctx = agent_context("scoper", scoper_input.clone(), &run.policy, Vec::new());
    token_budget.before_llm_call(&scoper_ctx)?;
    trace_llm_started(
        &trace,
        "scoper",
        side_model_name(&llm_config),
        &scoper_input,
    )
    .await?;
    let brief = scoper::generate_research_brief(&topic, &run.policy, &llm_config).await?;
    let brief_json = serde_json::to_string(&brief)?;
    json_guard.after_llm_call(&scoper_ctx, &brief_json)?;
    token_budget.after_llm_call(&scoper_ctx, &brief_json)?;
    trace_llm_finished(&trace, "scoper", &scoper_input, &brief_json).await?;

    let planner_input = serde_json::to_string(&brief)?;
    let planner_ctx = agent_context("planner", planner_input.clone(), &run.policy, Vec::new());
    token_budget.before_llm_call(&planner_ctx)?;
    trace_llm_started(
        &trace,
        "planner",
        side_model_name(&llm_config),
        &planner_input,
    )
    .await?;
    let plan = planner::generate_chapter_plan(&brief, &run.policy, &llm_config).await?;
    let plan_json = serde_json::to_string(&plan)?;
    json_guard.after_llm_call(&planner_ctx, &plan_json)?;
    token_budget.after_llm_call(&planner_ctx, &plan_json)?;
    trace_llm_finished(&trace, "planner", &planner_input, &plan_json).await?;

    let critic_input = serde_json::to_string(&(&brief, &plan.chapters, &plan.query_portfolio))?;
    let critic_ctx = agent_context("plan_critic", critic_input.clone(), &run.policy, Vec::new());
    token_budget.before_llm_call(&critic_ctx)?;
    trace_llm_started(
        &trace,
        "plan_critic",
        side_model_name(&llm_config),
        &critic_input,
    )
    .await?;
    let critique =
        plan_critic::critique_plan(&brief, &plan.chapters, &plan.query_portfolio, &run.policy);
    let critique_json = serde_json::to_string(&critique)?;
    json_guard.after_llm_call(&critic_ctx, &critique_json)?;
    token_budget.after_llm_call(&critic_ctx, &critique_json)?;
    trace_llm_finished(&trace, "plan_critic", &critic_input, &critique_json).await?;
    let from = run.state.clone();
    if !run.transition_to(ResearchRunState::PlanReady) {
        return Err(invalid_transition(&from, &ResearchRunState::PlanReady));
    }
    run.brief = Some(brief);
    run.chapters = plan.chapters;
    run.query_portfolio = plan.query_portfolio;
    run.plan_warnings = plan
        .warnings
        .into_iter()
        .chain(critique.warnings)
        .chain(critique.suggestions)
        .collect();
    run.warnings.push(format!(
        "Stage 3 agent planning used {} bounded LLM node call(s).",
        token_budget.calls()
    ));
    trace_transition(&trace, from, ResearchRunState::PlanReady).await?;
    let checkpoint = checkpoint::write_checkpoint(&run_dir, &run).await?;
    trace_checkpoint(&trace, &checkpoint).await?;
    save_run(&app_config, &run).await?;
    Ok(run)
}

pub async fn continue_run(
    run_id: &str,
    app_config: AppConfig,
    llm_config: LlmConfig,
    tx: Option<mpsc::Sender<StatefulRunEvent>>,
) -> Result<ResearchRunRecord> {
    let mut run = load_run(&app_config, run_id).await?;
    match run.state {
        ResearchRunState::PlanReady => {
            continue_from_plan_ready(&mut run, app_config, llm_config, tx).await
        }
        ResearchRunState::Completed => Ok(run),
        _ => Err(AppError::Workflow(format!(
            "run `{run_id}` is in state {:?} and cannot continue from this endpoint",
            run.state
        ))),
    }
}

async fn continue_from_plan_ready(
    run: &mut ResearchRunRecord,
    app_config: AppConfig,
    _llm_config: LlmConfig,
    tx: Option<mpsc::Sender<StatefulRunEvent>>,
) -> Result<ResearchRunRecord> {
    let run_dir = run_dir(&app_config, &run.run_id);
    let trace = TraceWriter::new(&run_dir).await?;
    transition(
        run,
        &trace,
        &app_config,
        ResearchRunState::Fetching,
        tx.as_ref(),
    )
    .await?;
    trace_planned_tool_calls(&trace, &run.query_portfolio).await?;

    let (github_result, arxiv_result) = tokio::join!(
        github_scout::scout_github(&run.query_portfolio, &app_config, 1),
        arxiv_scout::scout_arxiv(&run.query_portfolio, &app_config, 1)
    );
    let (github_repos, github_attempts) = github_result?;
    let (arxiv_papers, arxiv_attempts) = arxiv_result?;
    let query_attempts = github_attempts
        .into_iter()
        .chain(arxiv_attempts.into_iter())
        .collect::<Vec<QueryAttempt>>();
    for attempt in &query_attempts {
        trace
            .append(&TraceEvent::ToolCallFinished {
                tool: attempt.source.clone(),
                result_count: attempt.result_count,
                error: attempt.error.clone(),
                at: Utc::now(),
            })
            .await?;
    }
    if github_repos.is_empty() && arxiv_papers.is_empty() {
        let message = "GitHub 与 arXiv 均未返回可用结果，无法生成 stateful report。".to_string();
        run.warnings.push(message.clone());
        transition(
            run,
            &trace,
            &app_config,
            ResearchRunState::Failed,
            tx.as_ref(),
        )
        .await?;
        let checkpoint = checkpoint::write_checkpoint(&run_dir, run).await?;
        trace_checkpoint(&trace, &checkpoint).await?;
        send_checkpoint(tx.as_ref(), &checkpoint).await;
        if let Some(tx) = tx.as_ref() {
            let _ = tx
                .send(StatefulRunEvent::Failed {
                    error: message.clone(),
                })
                .await;
        }
        return Err(AppError::Workflow(message));
    }

    let query = SearchQuery {
        topic: run.topic.clone(),
        github_limit: run.policy.github_budget,
        arxiv_limit: run.policy.arxiv_budget,
    };
    let evidence = evidence::build_evidence_memory(
        &query,
        &app_config,
        github_repos,
        arxiv_papers,
        query_attempts,
    )?;
    let ranked_total = evidence.ranked_items.len();
    let group_total = evidence.groups.len();
    let coverage = if run.policy.skip_coverage_critic {
        CoverageReport::pass()
    } else {
        coverage_critic::evaluate_coverage(
            &run.chapters,
            &run.query_portfolio,
            &evidence.memory,
            &run.policy,
        )
    };
    for gap in &coverage.gaps {
        trace
            .append(&TraceEvent::CoverageGapDetected {
                chapter_id: gap.chapter_id.clone(),
                gap_kind: format!("{:?}", gap.gap_kind),
                at: Utc::now(),
            })
            .await?;
    }
    run.evidence_memory = Some(evidence.memory.clone());
    run.coverage_report = Some(coverage.clone());
    transition(
        run,
        &trace,
        &app_config,
        ResearchRunState::EvidenceReady,
        tx.as_ref(),
    )
    .await?;
    let checkpoint = checkpoint::write_checkpoint(&run_dir, run).await?;
    trace_checkpoint(&trace, &checkpoint).await?;
    send_checkpoint(tx.as_ref(), &checkpoint).await;
    if let Some(tx) = tx.as_ref() {
        let _ = tx
            .send(StatefulRunEvent::EvidenceReady {
                total: evidence.memory.items.len(),
            })
            .await;
        let _ = tx
            .send(StatefulRunEvent::CoverageReady {
                gaps: coverage.gaps.len(),
            })
            .await;
    }
    trace
        .append(&TraceEvent::QualityWarning {
            message: format!(
                "EvidenceBuilder ranked {ranked_total} item(s) into {group_total} group(s)."
            ),
            at: Utc::now(),
        })
        .await?;

    let draft = writer::draft_report(&run.topic, &run.chapters, &evidence.memory);
    let audit = if run.policy.require_citation_audit {
        citation_auditor::audit_citations(&draft, &evidence.citations, &evidence.memory)
    } else {
        crate::model::CitationAuditReport::pass()
    };
    let markdown = writer::render_report_markdown(&draft, &evidence.citations);
    let citation_ctx = agent_context(
        "writer",
        "render_report_markdown".to_string(),
        &run.policy,
        evidence
            .citations
            .citations
            .iter()
            .map(|citation| citation.url.clone())
            .collect(),
    );
    CitationGuard.after_llm_call(&citation_ctx, &markdown)?;
    run.report_draft = Some(draft);
    run.citation_audit = Some(audit.clone());
    run.report_markdown = Some(markdown.clone());
    if !audit.url_whitelist_passed {
        run.warnings
            .push("Citation audit failed URL whitelist check.".to_string());
    }
    transition(
        run,
        &trace,
        &app_config,
        ResearchRunState::SynthesisReady,
        tx.as_ref(),
    )
    .await?;
    let checkpoint = checkpoint::write_checkpoint(&run_dir, run).await?;
    trace_checkpoint(&trace, &checkpoint).await?;
    send_checkpoint(tx.as_ref(), &checkpoint).await;
    if let Some(tx) = tx.as_ref() {
        let _ = tx
            .send(StatefulRunEvent::CitationAuditReady {
                citation_coverage_ratio: audit.citation_coverage_ratio,
            })
            .await;
    }

    write_report_file(&app_config, run, &markdown).await?;
    transition(
        run,
        &trace,
        &app_config,
        ResearchRunState::Completed,
        tx.as_ref(),
    )
    .await?;
    let checkpoint = checkpoint::write_checkpoint(&run_dir, run).await?;
    trace_checkpoint(&trace, &checkpoint).await?;
    send_checkpoint(tx.as_ref(), &checkpoint).await;
    if let Some(tx) = tx {
        let _ = tx
            .send(StatefulRunEvent::Completed {
                run_id: run.run_id.clone(),
            })
            .await;
    }
    Ok(run.clone())
}

pub async fn load_run(app_config: &AppConfig, run_id: &str) -> Result<ResearchRunRecord> {
    let body = tokio::fs::read_to_string(run_summary_path(app_config, run_id)).await?;
    Ok(serde_json::from_str(&body)?)
}

pub async fn save_run(app_config: &AppConfig, run: &ResearchRunRecord) -> Result<()> {
    tokio::fs::create_dir_all(&app_config.session_dir).await?;
    let body = serde_json::to_string_pretty(run)?;
    tokio::fs::write(run_summary_path(app_config, &run.run_id), body).await?;
    Ok(())
}

pub async fn evidence_for_run(app_config: &AppConfig, run_id: &str) -> Result<EvidenceMemory> {
    load_run(app_config, run_id)
        .await?
        .evidence_memory
        .ok_or_else(|| AppError::Workflow("run has no EvidenceMemory yet".to_string()))
}

pub async fn coverage_for_run(app_config: &AppConfig, run_id: &str) -> Result<CoverageReport> {
    load_run(app_config, run_id)
        .await?
        .coverage_report
        .ok_or_else(|| AppError::Workflow("run has no CoverageReport yet".to_string()))
}

pub async fn revise_plan(
    app_config: &AppConfig,
    run_id: &str,
    chapters: Option<Vec<ChapterNode>>,
    query_portfolio: Option<Vec<QueryPortfolio>>,
    user_feedback: Option<String>,
) -> Result<ResearchRunRecord> {
    let mut run = load_run(app_config, run_id).await?;
    if !matches!(run.state, ResearchRunState::PlanReady) {
        return Err(AppError::Workflow(
            "only PlanReady runs can revise the Stage 3 plan".to_string(),
        ));
    }
    if let Some(chapters) = chapters {
        if chapters.is_empty() {
            return Err(AppError::Workflow(
                "revised chapter plan cannot be empty".to_string(),
            ));
        }
        run.chapters = chapters;
    }
    if let Some(query_portfolio) = query_portfolio {
        if query_portfolio.is_empty() {
            return Err(AppError::Workflow(
                "revised query portfolio cannot be empty".to_string(),
            ));
        }
        run.query_portfolio = query_portfolio;
    }
    if let Some(feedback) = user_feedback.map(|value| value.trim().to_string()) {
        if !feedback.is_empty() {
            run.plan_warnings
                .push(format!("用户修订计划备注：{feedback}"));
        }
    }
    run.updated_at = Utc::now();
    let run_dir = run_dir(app_config, run_id);
    let trace = TraceWriter::new(&run_dir).await?;
    trace
        .append(&TraceEvent::QualityWarning {
            message: "用户已修订 PlanReady 搜索计划，并创建新的 checkpoint。".to_string(),
            at: Utc::now(),
        })
        .await?;
    let checkpoint = checkpoint::write_checkpoint(&run_dir, &run).await?;
    trace_checkpoint(&trace, &checkpoint).await?;
    save_run(app_config, &run).await?;
    Ok(run)
}

pub async fn branch_from_plan_ready(
    app_config: AppConfig,
    run_id: &str,
    checkpoint_id: &str,
) -> Result<ResearchRunRecord> {
    let snapshot = load_checkpoint_snapshot_by_id(&app_config, run_id, checkpoint_id).await?;
    if !snapshot.checkpoint.rollback_allowed
        || !matches!(snapshot.checkpoint.state, ResearchRunState::PlanReady)
    {
        return Err(AppError::Workflow(
            "Stage 3 currently supports rollback branches only from PlanReady checkpoints"
                .to_string(),
        ));
    }
    let mut branch = snapshot.run;
    branch.run_id = format!("run-{}", Uuid::new_v4());
    branch.created_at = Utc::now();
    branch.updated_at = Utc::now();
    branch.state = ResearchRunState::PlanReady;
    branch.origin_run_id = Some(run_id.to_string());
    branch.origin_checkpoint_id = Some(checkpoint_id.to_string());
    branch.evidence_memory = None;
    branch.coverage_report = None;
    branch.report_draft = None;
    branch.citation_audit = None;
    branch.report_markdown = None;
    branch.output_report = None;
    let branch_dir = run_dir(&app_config, &branch.run_id);
    tokio::fs::create_dir_all(&branch_dir).await?;
    let trace = TraceWriter::new(&branch_dir).await?;
    trace
        .append(&TraceEvent::RollbackBranchCreated {
            origin_run_id: run_id.to_string(),
            new_run_id: branch.run_id.clone(),
            at: Utc::now(),
        })
        .await?;
    let checkpoint = checkpoint::write_checkpoint(&branch_dir, &branch).await?;
    trace_checkpoint(&trace, &checkpoint).await?;
    save_run(&app_config, &branch).await?;
    Ok(branch)
}

async fn load_checkpoint_snapshot_by_id(
    app_config: &AppConfig,
    run_id: &str,
    checkpoint_id: &str,
) -> Result<CheckpointSnapshot> {
    let run_dir = run_dir(app_config, run_id);
    let checkpoint_dir = run_dir.join("checkpoints");
    let mut entries = tokio::fs::read_dir(checkpoint_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let body = tokio::fs::read_to_string(entry.path()).await?;
        let snapshot: CheckpointSnapshot = serde_json::from_str(&body)?;
        if snapshot.checkpoint.checkpoint_id == checkpoint_id {
            return checkpoint::read_checkpoint(&run_dir, &snapshot.checkpoint).await;
        }
    }
    Err(AppError::Workflow(format!(
        "checkpoint `{checkpoint_id}` was not found for run `{run_id}`"
    )))
}

pub fn route_followup_for_run(
    question: &str,
    memory: Option<&EvidenceMemory>,
    report_markdown: Option<&str>,
) -> followup_router::FollowupRoute {
    match (memory, report_markdown) {
        (Some(memory), Some(markdown)) => {
            followup_router::route_followup(question, memory, markdown)
        }
        _ => followup_router::FollowupRoute::IncrementalResearchRequired {
            reason: "当前 run 尚未完成 evidence/report，无法基于现有证据回答。".to_string(),
        },
    }
}

async fn transition(
    run: &mut ResearchRunRecord,
    trace: &TraceWriter,
    app_config: &AppConfig,
    next: ResearchRunState,
    tx: Option<&mpsc::Sender<StatefulRunEvent>>,
) -> Result<()> {
    let from = run.state.clone();
    if !run.transition_to(next.clone()) {
        return Err(invalid_transition(&from, &next));
    }
    trace_transition(trace, from, next.clone()).await?;
    save_run(app_config, run).await?;
    if let Some(tx) = tx {
        let _ = tx
            .send(StatefulRunEvent::StateChanged { state: next })
            .await;
    }
    Ok(())
}

async fn trace_transition(
    trace: &TraceWriter,
    from: ResearchRunState,
    to: ResearchRunState,
) -> Result<()> {
    trace
        .append(&TraceEvent::StateTransition {
            from,
            to,
            at: Utc::now(),
        })
        .await
}

async fn trace_llm_started(
    trace: &TraceWriter,
    actor: &str,
    model: String,
    input: &str,
) -> Result<()> {
    trace
        .append(&TraceEvent::LlmRequestStarted {
            actor: actor.to_string(),
            model,
            input_hash: stable_hash(input),
            at: Utc::now(),
        })
        .await
}

async fn trace_llm_finished(
    trace: &TraceWriter,
    actor: &str,
    input: &str,
    output: &str,
) -> Result<()> {
    let (_input_hash, output_hash, tokens) = decision_hash(input, output);
    trace
        .append(&TraceEvent::LlmRequestFinished {
            actor: actor.to_string(),
            output_hash,
            token_estimate: tokens,
            at: Utc::now(),
        })
        .await
}

async fn trace_checkpoint(trace: &TraceWriter, checkpoint: &Checkpoint) -> Result<()> {
    trace
        .append(&TraceEvent::CheckpointCreated {
            checkpoint_id: checkpoint.checkpoint_id.clone(),
            state: checkpoint.state.clone(),
            at: Utc::now(),
        })
        .await
}

async fn send_checkpoint(tx: Option<&mpsc::Sender<StatefulRunEvent>>, checkpoint: &Checkpoint) {
    if let Some(tx) = tx {
        let _ = tx
            .send(StatefulRunEvent::CheckpointCreated {
                checkpoint_id: checkpoint.checkpoint_id.clone(),
                state: checkpoint.state.clone(),
            })
            .await;
    }
}

async fn trace_planned_tool_calls(trace: &TraceWriter, portfolio: &[QueryPortfolio]) -> Result<()> {
    for item in portfolio {
        for query in &item.github_queries {
            trace
                .append(&TraceEvent::ToolCallStarted {
                    tool: "github".to_string(),
                    query: query.clone(),
                    at: Utc::now(),
                })
                .await?;
        }
        for query in &item.arxiv_queries {
            trace
                .append(&TraceEvent::ToolCallStarted {
                    tool: "arxiv".to_string(),
                    query: query.clone(),
                    at: Utc::now(),
                })
                .await?;
        }
    }
    Ok(())
}

fn agent_context(
    actor: &str,
    input: String,
    policy: &RunPolicy,
    allowed_urls: Vec<String>,
) -> AgentContext {
    AgentContext {
        actor: actor.to_string(),
        input,
        allowed_urls,
        max_llm_calls: policy.max_llm_calls_per_run,
    }
}

fn side_model_name(llm_config: &LlmConfig) -> String {
    llm_config
        .side_model
        .clone()
        .unwrap_or_else(|| llm_config.main_model.clone())
}

fn invalid_transition(from: &ResearchRunState, to: &ResearchRunState) -> AppError {
    AppError::Workflow(format!("invalid state transition from {from:?} to {to:?}"))
}

fn run_summary_path(app_config: &AppConfig, run_id: &str) -> PathBuf {
    app_config.session_dir.join(format!("{run_id}.json"))
}

fn run_dir(app_config: &AppConfig, run_id: &str) -> PathBuf {
    app_config.session_dir.join(run_id)
}

async fn write_report_file(
    app_config: &AppConfig,
    run: &mut ResearchRunRecord,
    markdown: &str,
) -> Result<()> {
    let path = resolve_report_path(&app_config.output, &run.topic);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&path, markdown).await?;
    run.output_report = Some(path.display().to_string());
    Ok(())
}

fn resolve_report_path(output: &Path, topic: &str) -> PathBuf {
    if output.extension().is_some() {
        return output.to_path_buf();
    }
    output.join(format!(
        "{}-stage3-{}.md",
        slugify(topic),
        Utc::now().format("%Y%m%d-%H%M%S")
    ))
}

fn slugify(topic: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in topic.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "report".to_string()
    } else {
        slug.to_string()
    }
}

pub fn decision_hash(input: &str, output: &str) -> (String, String, usize) {
    (
        stable_hash(input),
        stable_hash(output),
        token_estimate(input) + token_estimate(output),
    )
}
