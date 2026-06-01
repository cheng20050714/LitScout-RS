use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use std::convert::Infallible;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::{wrappers::ReceiverStream, Stream, StreamExt};
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use uuid::Uuid;

use crate::agent::{planner, report_chat};
use crate::config::{AppConfig, LlmConfig};
use crate::error::{AppError, Result};
use crate::llm::deepseek::{DeepSeekClient, DeepSeekConfig};
use crate::model::{SearchAspect, SearchPlan, SearchQuery};
use crate::workflow;

use super::dto::{
    ChatStreamEvent, FrontendConfig, PlanRequest, PlanResponse, PlanReviseRequest,
    ReportChatRequest, ReportChatResponse, ReportTranslateRequest, ReportTranslateResponse,
    RunRequest, RunResponse, RunStreamEvent,
};
use super::state::AppState;

pub fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/api/health", get(health))
        .route("/api/plan", post(create_plan))
        .route("/api/plan/revise", post(revise_plan))
        .route("/api/run", post(run_research))
        .route("/api/run/stream", post(run_research_stream))
        .route("/api/report/chat", post(chat_with_report))
        .route("/api/report/chat/stream", post(chat_with_report_stream))
        .route("/api/report/translate", post(translate_report))
        .fallback_service(ServeDir::new("web/dist"))
        .layer(cors)
        .with_state(state)
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "litscout-rs",
        llm_enabled: state.llm_config.enabled,
        github_token_configured: state.app_config.github_token.is_some(),
    })
}

async fn create_plan(
    State(state): State<AppState>,
    Json(req): Json<PlanRequest>,
) -> Result<Json<PlanResponse>> {
    let plan = planner::generate_chinese_plan(
        &req.topic,
        req.github_limit,
        req.arxiv_limit,
        &effective_llm_config(&state, &req.config),
    )
    .await?;
    let mut response = PlanResponse::from(plan);
    response.language = req.language;
    Ok(Json(response))
}

async fn revise_plan(
    State(state): State<AppState>,
    Json(req): Json<PlanReviseRequest>,
) -> Result<Json<PlanResponse>> {
    let mut current_plan = planner::PlanOutput::from(req.current_plan);
    current_plan.plan_id = req.plan_id;
    let revised = planner::revise_chinese_plan(
        &current_plan,
        &req.user_feedback,
        &effective_llm_config(&state, &req.config),
    )
    .await?;
    Ok(Json(PlanResponse::from(revised)))
}

async fn run_research(
    State(state): State<AppState>,
    Json(req): Json<RunRequest>,
) -> Result<Json<RunResponse>> {
    let plan_id = req.plan_id.clone();
    let language = req.language.clone();
    let plan_output = planner::PlanOutput::from(req.current_plan);
    let query = SearchQuery {
        topic: plan_output.original_topic.clone(),
        github_limit: total_github_limit(&plan_output),
        arxiv_limit: total_arxiv_limit(&plan_output),
    };
    let search_plan = to_search_plan(plan_output);
    let app_config = effective_app_config(&state, &req.config);
    let llm_config = effective_llm_config(&state, &req.config);
    let result =
        workflow::run_with_plan_for_report(query, search_plan, app_config, llm_config).await?;
    let report_markdown = tokio::fs::read_to_string(&result.output_path).await?;

    Ok(Json(RunResponse {
        session_id: format!("{language}-{plan_id}-{}", Uuid::new_v4()),
        output_report: result.output_path.display().to_string(),
        session_path: result
            .session_path
            .as_ref()
            .map(|path| path.display().to_string()),
        warnings: result.report.quality.warnings.clone(),
        citations: result.report.citations.citations.clone(),
        ranked_items: result.report.ranked_items.clone(),
        quality: result.report.quality.clone(),
        report_markdown,
    }))
}

async fn run_research_stream(
    State(state): State<AppState>,
    Json(req): Json<RunRequest>,
) -> Sse<impl Stream<Item = std::result::Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<RunStreamEvent>(64);

    tokio::spawn(async move {
        let stream_result = run_research_for_stream(state, req, tx.clone()).await;
        if let Err(err) = stream_result {
            let _ = tx
                .send(RunStreamEvent::RunFailed {
                    error: err.to_string(),
                })
                .await;
        }
    });

    let stream = ReceiverStream::new(rx).map(|event| {
        let event_name = match &event {
            RunStreamEvent::Workflow(workflow_event) => match workflow_event {
                workflow::WorkflowEvent::FetchStarted { .. } => "fetch_started",
                workflow::WorkflowEvent::SourceFinished { .. } => "source_finished",
                workflow::WorkflowEvent::RankingFinished { .. } => "ranking_finished",
                workflow::WorkflowEvent::ClassificationFinished { .. } => "classification_finished",
                workflow::WorkflowEvent::SynthesisStarted => "synthesis_started",
                workflow::WorkflowEvent::QualityWarning { .. } => "quality_warning",
                workflow::WorkflowEvent::ReportReady { .. } => "workflow_report_ready",
            },
            RunStreamEvent::ReportReady(_) => "report_ready",
            RunStreamEvent::RunFailed { .. } => "run_failed",
        };
        Ok(Event::default()
            .event(event_name)
            .json_data(event)
            .unwrap_or_else(|err| Event::default().event("run_failed").data(err.to_string())))
    });

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

async fn run_research_for_stream(
    state: AppState,
    req: RunRequest,
    tx: mpsc::Sender<RunStreamEvent>,
) -> Result<()> {
    let plan_id = req.plan_id.clone();
    let language = req.language.clone();
    let plan_output = planner::PlanOutput::from(req.current_plan);
    let query = SearchQuery {
        topic: plan_output.original_topic.clone(),
        github_limit: total_github_limit(&plan_output),
        arxiv_limit: total_arxiv_limit(&plan_output),
    };
    let search_plan = to_search_plan(plan_output);
    let app_config = effective_app_config(&state, &req.config);
    let llm_config = effective_llm_config(&state, &req.config);
    let result =
        workflow::run_with_plan_events(query, search_plan, app_config, llm_config, |event| {
            let tx = tx.clone();
            tokio::spawn(async move {
                let _ = tx.send(RunStreamEvent::Workflow(event)).await;
            });
        })
        .await?;
    let report_markdown = tokio::fs::read_to_string(&result.output_path).await?;
    let response = RunResponse {
        session_id: format!("{language}-{plan_id}-{}", Uuid::new_v4()),
        output_report: result.output_path.display().to_string(),
        session_path: result
            .session_path
            .as_ref()
            .map(|path| path.display().to_string()),
        report_markdown,
        warnings: result.report.quality.warnings.clone(),
        citations: result.report.citations.citations.clone(),
        ranked_items: result.report.ranked_items.clone(),
        quality: result.report.quality.clone(),
    };
    tx.send(RunStreamEvent::ReportReady(response))
        .await
        .map_err(|_| AppError::Workflow("failed to send report_ready event".to_string()))?;
    Ok(())
}

async fn chat_with_report(
    State(state): State<AppState>,
    Json(req): Json<ReportChatRequest>,
) -> Result<Json<ReportChatResponse>> {
    let answer = report_chat::answer_report_question(
        &req.report_markdown,
        &req.question,
        &effective_llm_config(&state, &req.config),
    )
    .await?;
    Ok(Json(ReportChatResponse { answer }))
}

async fn chat_with_report_stream(
    State(state): State<AppState>,
    Json(req): Json<ReportChatRequest>,
) -> Sse<impl Stream<Item = std::result::Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<ChatStreamEvent>(32);

    tokio::spawn(async move {
        let result = report_chat::answer_report_question_streaming(
            &req.report_markdown,
            &req.question,
            &effective_llm_config(&state, &req.config),
        )
        .await;

        match result {
            Ok(answer) => {
                for chunk in markdown_chunks(&answer) {
                    if tx
                        .send(ChatStreamEvent::Delta { text: chunk })
                        .await
                        .is_err()
                    {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(45)).await;
                }
                let _ = tx.send(ChatStreamEvent::Done).await;
            }
            Err(err) => {
                let _ = tx
                    .send(ChatStreamEvent::Failed {
                        error: err.to_string(),
                    })
                    .await;
            }
        }
    });

    let stream = ReceiverStream::new(rx).map(|event| {
        let event_name = match event {
            ChatStreamEvent::Delta { .. } => "delta",
            ChatStreamEvent::Done => "done",
            ChatStreamEvent::Failed { .. } => "failed",
        };
        Ok(Event::default()
            .event(event_name)
            .json_data(event)
            .unwrap_or_else(|err| Event::default().event("failed").data(err.to_string())))
    });

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

async fn translate_report(
    State(state): State<AppState>,
    Json(req): Json<ReportTranslateRequest>,
) -> Result<Json<ReportTranslateResponse>> {
    let llm_config = effective_llm_config(&state, &req.config);
    let config = DeepSeekConfig::from_llm_config(&llm_config).ok_or_else(|| {
        AppError::InvalidConfig(
            "翻译报告需要 DeepSeek API Key，请在阶段 1 配置或设置 DEEPSEEK_API_KEY。".to_string(),
        )
    })?;
    let client = DeepSeekClient::new(config)?;
    let translated = client
        .translate_report_to_chinese(&req.report_markdown)
        .await?;
    Ok(Json(ReportTranslateResponse {
        translated_markdown: translated,
    }))
}

fn markdown_chunks(answer: &str) -> Vec<String> {
    let mut chunks = answer
        .split_inclusive('\n')
        .map(str::to_string)
        .filter(|chunk| !chunk.is_empty())
        .collect::<Vec<_>>();
    if chunks.is_empty() && !answer.is_empty() {
        chunks.push(answer.to_string());
    }
    chunks
}

fn effective_app_config(state: &AppState, frontend: &FrontendConfig) -> AppConfig {
    let mut app_config = (*state.app_config).clone();
    if let Some(token) = non_empty(frontend.github_token.as_deref()) {
        app_config.github_token = Some(token.to_string());
    }
    app_config
}

fn effective_llm_config(state: &AppState, frontend: &FrontendConfig) -> LlmConfig {
    let mut llm_config = (*state.llm_config).clone();
    if let Some(api_key) = non_empty(frontend.deepseek_api_key.as_deref()) {
        llm_config.enabled = true;
        llm_config.api_key = Some(api_key.to_string());
    }
    if let Some(base_url) = non_empty(frontend.deepseek_base_url.as_deref()) {
        llm_config.base_url = Some(base_url.to_string());
    }
    if let Some(model) = non_empty(frontend.deepseek_model.as_deref()) {
        llm_config.main_model = model.to_string();
    }
    if let Some(side_model) = non_empty(frontend.deepseek_side_model.as_deref()) {
        llm_config.side_model = Some(side_model.to_string());
    }
    llm_config
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn to_search_plan(plan: planner::PlanOutput) -> SearchPlan {
    SearchPlan {
        original_topic: plan.original_topic,
        llm_generated: plan.llm_generated,
        aspects: plan
            .aspects
            .into_iter()
            .map(|aspect| SearchAspect {
                name: aspect.name_zh,
                github_query: aspect.github_query,
                arxiv_query: aspect.arxiv_query,
                github_limit: aspect.github_limit,
                arxiv_limit: aspect.arxiv_limit,
                rationale: Some(aspect.rationale_zh),
            })
            .collect(),
    }
}

fn total_github_limit(plan: &planner::PlanOutput) -> usize {
    plan.aspects
        .iter()
        .map(|aspect| aspect.github_limit)
        .sum::<usize>()
        .max(1)
}

fn total_arxiv_limit(plan: &planner::PlanOutput) -> usize {
    plan.aspects
        .iter()
        .map(|aspect| aspect.arxiv_limit)
        .sum::<usize>()
        .max(1)
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
    llm_enabled: bool,
    github_token_configured: bool,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::build_router;
    use crate::config::{AppConfig, LlmConfig};
    use crate::server::state::AppState;

    #[test]
    fn builds_router_for_health_endpoint() {
        let state = AppState::new(app_config(), LlmConfig::from_env(false, 30));

        let _router = build_router(state);
    }

    fn app_config() -> AppConfig {
        AppConfig {
            github_token: None,
            output: PathBuf::from("reports"),
            cache_dir: PathBuf::from(".litscout-cache"),
            session_dir: PathBuf::from("sessions"),
            tags_file: None,
            use_cache: true,
            cache_ttl_hours: 24,
            timeout_secs: 30,
            enrich: false,
        }
    }
}
