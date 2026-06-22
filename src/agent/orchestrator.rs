use std::path::{Path, PathBuf};

use chrono::Utc;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::agent::{
    academic_scout, arxiv_scout, citation_auditor, coverage_critic, evidence, github_scout,
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
    let brief = scoper::generate_research_brief(&topic, &run.policy, &llm_config).await?;
    let brief_json = serde_json::to_string(&brief)?;
    json_guard.after_llm_call(&scoper_ctx, &brief_json)?;
    trace_agent_decision(
        &trace,
        "scoper",
        &scoper_input,
        &brief_json,
        "deterministic ResearchBrief scoping",
    )
    .await?;

    let planner_input = serde_json::to_string(&brief)?;
    let planner_ctx = agent_context("planner", planner_input.clone(), &run.policy, Vec::new());
    let plan = if llm_config_can_call(&llm_config) {
        token_budget.before_llm_call(&planner_ctx)?;
        trace_llm_started(
            &trace,
            "planner",
            side_model_name(&llm_config),
            &planner_input,
        )
        .await?;
        match planner::generate_chapter_plan(&brief, &run.policy, &llm_config).await {
            Ok(plan) => {
                let plan_json = serde_json::to_string(&plan)?;
                json_guard.after_llm_call(&planner_ctx, &plan_json)?;
                token_budget.after_llm_call(&planner_ctx, &plan_json)?;
                trace_llm_finished(&trace, "planner", &planner_input, &plan_json).await?;
                plan
            }
            Err(err) => {
                let reason = err.to_string();
                trace
                    .append(&TraceEvent::QualityWarning {
                        message: format!("Planner LLM 调用失败，已降级为确定性默认计划：{reason}"),
                        at: Utc::now(),
                    })
                    .await?;
                let plan = fallback_chapter_plan(&brief.topic, &run.policy, Some(&reason));
                let plan_json = serde_json::to_string(&plan)?;
                json_guard.after_llm_call(&planner_ctx, &plan_json)?;
                trace_agent_decision(
                    &trace,
                    "planner_fallback",
                    &planner_input,
                    &plan_json,
                    "planner LLM failed; deterministic default plan",
                )
                .await?;
                plan
            }
        }
    } else {
        let reason = llm_config
            .enabled
            .then_some("LLM 已启用但缺少 API key，已使用确定性默认计划。");
        let plan = fallback_chapter_plan(&brief.topic, &run.policy, reason);
        let plan_json = serde_json::to_string(&plan)?;
        json_guard.after_llm_call(&planner_ctx, &plan_json)?;
        trace_agent_decision(
            &trace,
            "planner",
            &planner_input,
            &plan_json,
            "deterministic default plan",
        )
        .await?;
        plan
    };

    let critic_input = serde_json::to_string(&(&brief, &plan.chapters, &plan.query_portfolio))?;
    let critic_ctx = agent_context("plan_critic", critic_input.clone(), &run.policy, Vec::new());
    let critique =
        plan_critic::critique_plan(&brief, &plan.chapters, &plan.query_portfolio, &run.policy);
    let critique_json = serde_json::to_string(&critique)?;
    json_guard.after_llm_call(&critic_ctx, &critique_json)?;
    trace_agent_decision(
        &trace,
        "plan_critic",
        &critic_input,
        &critique_json,
        "deterministic plan consistency checks",
    )
    .await?;
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
        "Stage 3 agent planning used {} real LLM request attempt(s); deterministic nodes are traced as AgentDecision events.",
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
    llm_config: LlmConfig,
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
    trace_planned_tool_calls(
        &trace,
        &run.query_portfolio,
        run.policy.academic_extra_enabled,
    )
    .await?;

    let mut github_config = app_config.clone();
    github_config.enrich = app_config.enrich || run.policy.allow_github_enrich;
    let (github_result, arxiv_result) = tokio::join!(
        github_scout::scout_github(&run.query_portfolio, &github_config, 1),
        arxiv_scout::scout_arxiv(&run.query_portfolio, &app_config, 1)
    );
    let (github_repos, github_attempts, github_lineage) = github_result?;
    let (arxiv_papers, arxiv_attempts, arxiv_lineage) = arxiv_result?;
    let (academic_items, academic_attempts, academic_lineage) = if run.policy.academic_extra_enabled
    {
        academic_scout::scout_academic_extra(
            &run.query_portfolio,
            &app_config,
            run.policy.academic_budget,
            1,
        )
        .await?
    } else {
        (Vec::new(), Vec::new(), Vec::new())
    };
    let query_attempts = github_attempts
        .into_iter()
        .chain(arxiv_attempts)
        .chain(academic_attempts)
        .collect::<Vec<QueryAttempt>>();
    let source_lineage = github_lineage
        .into_iter()
        .chain(arxiv_lineage)
        .chain(academic_lineage)
        .collect::<Vec<_>>();
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
    if github_repos.is_empty() && arxiv_papers.is_empty() && academic_items.is_empty() {
        let message =
            "GitHub、arXiv 与已启用的扩展学术源均未返回可用结果，无法生成 stateful report。"
                .to_string();
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
        academic_items,
        query_attempts,
        source_lineage,
    )?;
    let ranked_total = evidence.ranked_items.len();
    let group_total = evidence.groups.len();
    let selected_total = evidence.memory.items.len();
    let rejected_total = evidence.rejected_items.len();
    let top_rejection_reasons = evidence
        .selection_report
        .rejection_reasons
        .iter()
        .take(3)
        .map(|reason| format!("{}={}", reason.reason, reason.count))
        .collect::<Vec<_>>()
        .join(", ");
    if run.policy.academic_extra_enabled
        && evidence.selection_report.rejected_item_count > 0
        && evidence.memory.items.iter().all(|item| {
            !matches!(
                item.source_kind,
                crate::model::SourceKind::AcademicIndex | crate::model::SourceKind::Bibliography
            )
        })
    {
        run.warnings.push(
            "扩展学术源返回了候选结果，但 EvidenceQualityGate 未筛出可引用证据；报告将继续使用 GitHub/arXiv 证据。"
                .to_string(),
        );
    }
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
                "EvidenceQualityGate ranked {ranked_total} item(s), accepted {selected_total}, rejected {rejected_total}, grouped {group_total}. Top rejection reasons: {}",
                if top_rejection_reasons.is_empty() {
                    "none".to_string()
                } else {
                    top_rejection_reasons
                }
            ),
            at: Utc::now(),
        })
        .await?;

    let writer_input = serde_json::to_string(&(&run.topic, &run.chapters, &evidence.memory))?;
    let writer_ctx = agent_context("writer", writer_input.clone(), &run.policy, Vec::new());
    if !llm_config_can_call(&llm_config) {
        let message = "Writer LLM 需要 DeepSeek API key；拒绝生成模板化最终报告。".to_string();
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
        return Err(AppError::InvalidConfig(message));
    }
    let writer_model = llm_config.main_model.clone();
    let writer_budget = TokenBudgetTracker::new();
    writer_budget.before_llm_call(&writer_ctx)?;
    trace_llm_started(&trace, "writer", writer_model, &writer_input).await?;
    let draft_result =
        writer::draft_report_with_llm(&run.topic, &run.chapters, &evidence.memory, &llm_config)
            .await;
    let draft = match draft_result {
        Ok(draft) => draft,
        Err(err) => {
            let message = format!("Writer LLM 写作失败，拒绝生成模板化最终报告：{err}");
            run.warnings.push(message.clone());
            trace
                .append(&TraceEvent::QualityWarning {
                    message: message.clone(),
                    at: Utc::now(),
                })
                .await?;
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
            return Err(AppError::Llm(message));
        }
    };
    let draft_json = serde_json::to_string(&draft)?;
    trace_llm_finished(&trace, "writer", &writer_input, &draft_json).await?;
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

async fn trace_agent_decision(
    trace: &TraceWriter,
    actor: &str,
    input: &str,
    output: &str,
    rationale: &str,
) -> Result<()> {
    let (input_hash, output_hash, _tokens) = decision_hash(input, output);
    trace
        .append(&TraceEvent::AgentDecision {
            actor: actor.to_string(),
            input_hash,
            output_hash,
            rationale: rationale.to_string(),
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

async fn trace_planned_tool_calls(
    trace: &TraceWriter,
    portfolio: &[QueryPortfolio],
    include_academic_extra: bool,
) -> Result<()> {
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
        if include_academic_extra {
            for query in academic_queries_for_trace(item) {
                for source in academic_scout::academic_source_names() {
                    trace
                        .append(&TraceEvent::ToolCallStarted {
                            tool: source.to_string(),
                            query: query.clone(),
                            at: Utc::now(),
                        })
                        .await?;
                }
            }
        }
    }
    Ok(())
}

fn academic_queries_for_trace(item: &QueryPortfolio) -> Vec<String> {
    let mut queries = item.arxiv_queries.clone();
    for query in &item.github_queries {
        if !queries.iter().any(|existing| existing == query) {
            queries.push(query.clone());
        }
    }
    queries
}

fn llm_config_can_call(llm_config: &LlmConfig) -> bool {
    llm_config.enabled
        && llm_config
            .api_key
            .as_deref()
            .map(str::trim)
            .is_some_and(|key| !key.is_empty())
}

fn fallback_chapter_plan(
    topic: &str,
    policy: &RunPolicy,
    warning: Option<&str>,
) -> planner::ChapterPlanOutput {
    let default_plan = planner::default_plan(topic, policy.github_budget, policy.arxiv_budget);
    let mut plan = planner::chapter_plan_from_plan_output(&default_plan, policy);
    if let Some(warning) = warning {
        plan.warnings.push(warning.to_string());
    }
    plan
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

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::create_run;
    use crate::config::{AppConfig, LlmConfig};
    use crate::run_policy::RunPolicy;
    use crate::workflow_state::ResearchRunState;

    #[tokio::test]
    async fn create_run_falls_back_without_llm_key_and_traces_rule_nodes() {
        let root = temp_root("stage3-create-run-fallback");
        let app_config = AppConfig {
            github_token: None,
            semantic_scholar_api_key: None,
            openalex_api_key: None,
            crossref_mailto: None,
            output: root.join("reports"),
            cache_dir: root.join("cache"),
            session_dir: root.join("sessions"),
            tags_file: None,
            use_cache: false,
            cache_ttl_hours: 24,
            timeout_secs: 30,
            enrich: false,
        };
        let llm_config = LlmConfig {
            enabled: true,
            api_key: None,
            base_url: Some("https://api.deepseek.com".to_string()),
            main_model: "deepseek-v4-pro".to_string(),
            side_model: Some("deepseek-v4-flash".to_string()),
            max_tokens: 4096,
            timeout_secs: 30,
        };

        let run = create_run(
            "Rust agent framework".to_string(),
            app_config.clone(),
            llm_config,
            RunPolicy::default(),
        )
        .await
        .expect("create_run should degrade to a deterministic plan");

        assert_eq!(run.state, ResearchRunState::PlanReady);
        assert!(run
            .plan_warnings
            .iter()
            .any(|warning| warning.contains("API key")));
        assert!(run
            .warnings
            .iter()
            .any(|warning| warning.contains("0 real LLM request attempt")));

        let trace_path = app_config.session_dir.join(&run.run_id).join("trace.jsonl");
        let trace = std::fs::read_to_string(trace_path).expect("trace should be readable");
        assert!(trace.contains("\"event\":\"agent_decision\""));
        assert!(trace.contains("\"actor\":\"scoper\""));
        assert!(trace.contains("\"actor\":\"plan_critic\""));
        assert!(!trace.contains("\"event\":\"llm_request_started\""));
    }

    fn temp_root(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "litscout-rs-{name}-{}-{unique}",
            std::process::id()
        ))
    }
}
