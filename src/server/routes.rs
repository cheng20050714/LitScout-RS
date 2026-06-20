use axum::{
    extract::{Path, State},
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use std::convert::Infallible;
use std::pin::Pin;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::{wrappers::ReceiverStream, Stream, StreamExt};
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

use crate::agent::orchestrator;
use crate::checkpoint;
use crate::config::{AppConfig, LlmConfig};
use crate::error::{AppError, Result};
use crate::llm::deepseek::{DeepSeekClient, DeepSeekConfig};
use crate::reading::models::{NoteQuality, PaperChatMessage, ReadingStatus, TextCoverage};
use crate::reading::{
    fetcher as full_text_fetcher, library as reading_library, note_writer, paper_chat,
};

use super::dto::{
    AddReadingLibraryItemRequest, BranchRunRequest, ChatStreamEvent, CheckpointListResponse,
    CitationAuditResponse, ContinueStatefulRunRequest, CoverageResponse, CreateStatefulRunRequest,
    EvidenceResponse, FrontendConfig, GenerateReadingNoteRequest, PaperChatRequest,
    ReadingLibraryItemResponse, ReadingLibraryResponse, ReportTranslateRequest,
    ReportTranslateResponse, ReviseStatefulPlanRequest, StatefulRunResponse,
    StatefulRunStreamEvent,
};
use super::state::AppState;

pub fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/api/health", get(health))
        .route("/api/runs", post(create_stateful_run))
        .route("/api/runs/{run_id}", get(get_stateful_run))
        .route("/api/runs/{run_id}/events", get(stream_stateful_run_events))
        .route(
            "/api/runs/{run_id}/approve-plan",
            post(continue_stateful_run),
        )
        .route("/api/runs/{run_id}/continue", post(continue_stateful_run))
        .route("/api/runs/{run_id}/revise-plan", post(revise_stateful_plan))
        .route("/api/runs/{run_id}/evidence", get(get_stateful_evidence))
        .route("/api/runs/{run_id}/coverage", get(get_stateful_coverage))
        .route(
            "/api/runs/{run_id}/citation-audit",
            get(get_stateful_citation_audit),
        )
        .route(
            "/api/runs/{run_id}/checkpoints",
            get(list_stateful_checkpoints),
        )
        .route(
            "/api/runs/{run_id}/branch-from-checkpoint",
            post(branch_stateful_run),
        )
        .route("/api/report/translate", post(translate_report))
        .route("/api/library", get(list_reading_library))
        .route("/api/library/items", post(add_reading_library_item))
        .route(
            "/api/library/items/{paper_key}",
            get(get_reading_library_item).delete(delete_reading_library_item),
        )
        .route(
            "/api/library/items/{paper_key}/generate-note",
            post(generate_reading_note),
        )
        .route(
            "/api/library/items/{paper_key}/chat/stream",
            post(chat_with_paper_stream),
        )
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

async fn create_stateful_run(
    State(state): State<AppState>,
    Json(req): Json<CreateStatefulRunRequest>,
) -> Result<Json<StatefulRunResponse>> {
    let app_config = effective_app_config(&state, &req.config);
    let llm_config = effective_llm_config(&state, &req.config);
    let run = orchestrator::create_run(req.topic, app_config, llm_config, req.policy).await?;
    Ok(Json(StatefulRunResponse {
        run_id: run.run_id.clone(),
        topic: run.topic.clone(),
        state: run.state.clone(),
        run,
    }))
}

async fn get_stateful_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<StatefulRunResponse>> {
    let run = orchestrator::load_run(&state.app_config, &run_id).await?;
    Ok(Json(StatefulRunResponse {
        run_id: run.run_id.clone(),
        topic: run.topic.clone(),
        state: run.state.clone(),
        run,
    }))
}

async fn stream_stateful_run_events(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Sse<impl Stream<Item = std::result::Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<StatefulRunStreamEvent>(16);
    tokio::spawn(async move {
        match orchestrator::load_run(&state.app_config, &run_id).await {
            Ok(run) => {
                let response = StatefulRunResponse {
                    run_id: run.run_id.clone(),
                    topic: run.topic.clone(),
                    state: run.state.clone(),
                    run,
                };
                let _ = tx
                    .send(StatefulRunStreamEvent::RunReady(Box::new(response)))
                    .await;
            }
            Err(err) => {
                let _ = tx
                    .send(StatefulRunStreamEvent::RunFailed {
                        error: err.to_string(),
                    })
                    .await;
            }
        }
    });
    let stream = ReceiverStream::new(rx).map(|event| {
        let event_name = match &event {
            StatefulRunStreamEvent::Agent(_) => "agent",
            StatefulRunStreamEvent::RunReady(_) => "run_ready",
            StatefulRunStreamEvent::RunFailed { .. } => "run_failed",
        };
        Ok(Event::default()
            .event(event_name)
            .json_data(event)
            .unwrap_or_else(|err| Event::default().event("run_failed").data(err.to_string())))
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

async fn continue_stateful_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    req: Option<Json<ContinueStatefulRunRequest>>,
) -> Sse<impl Stream<Item = std::result::Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<StatefulRunStreamEvent>(64);
    tokio::spawn(async move {
        let (event_tx, mut event_rx) = mpsc::channel(64);
        let forward_tx = tx.clone();
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if forward_tx
                    .send(StatefulRunStreamEvent::Agent(event))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
        match orchestrator::continue_run(
            &run_id,
            effective_app_config(
                &state,
                req.as_ref()
                    .map(|Json(req)| &req.config)
                    .unwrap_or(&FrontendConfig::default()),
            ),
            effective_llm_config(
                &state,
                req.as_ref()
                    .map(|Json(req)| &req.config)
                    .unwrap_or(&FrontendConfig::default()),
            ),
            Some(event_tx),
        )
        .await
        {
            Ok(run) => {
                let response = StatefulRunResponse {
                    run_id: run.run_id.clone(),
                    topic: run.topic.clone(),
                    state: run.state.clone(),
                    run,
                };
                let _ = tx
                    .send(StatefulRunStreamEvent::RunReady(Box::new(response)))
                    .await;
            }
            Err(err) => {
                let _ = tx
                    .send(StatefulRunStreamEvent::RunFailed {
                        error: err.to_string(),
                    })
                    .await;
            }
        }
    });
    let stream = ReceiverStream::new(rx).map(|event| {
        let event_name = match &event {
            StatefulRunStreamEvent::Agent(_) => "agent",
            StatefulRunStreamEvent::RunReady(_) => "run_ready",
            StatefulRunStreamEvent::RunFailed { .. } => "run_failed",
        };
        Ok(Event::default()
            .event(event_name)
            .json_data(event)
            .unwrap_or_else(|err| Event::default().event("run_failed").data(err.to_string())))
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

async fn revise_stateful_plan(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(req): Json<ReviseStatefulPlanRequest>,
) -> Result<Json<StatefulRunResponse>> {
    let run = orchestrator::revise_plan(
        &state.app_config,
        &run_id,
        req.chapters,
        req.query_portfolio,
        req.user_feedback,
    )
    .await?;
    Ok(Json(StatefulRunResponse {
        run_id: run.run_id.clone(),
        topic: run.topic.clone(),
        state: run.state.clone(),
        run,
    }))
}

async fn get_stateful_evidence(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<EvidenceResponse>> {
    let evidence_memory = orchestrator::evidence_for_run(&state.app_config, &run_id).await?;
    Ok(Json(EvidenceResponse {
        run_id,
        evidence_memory,
    }))
}

async fn get_stateful_coverage(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<CoverageResponse>> {
    let coverage_report = orchestrator::coverage_for_run(&state.app_config, &run_id).await?;
    Ok(Json(CoverageResponse {
        run_id,
        coverage_report,
    }))
}

async fn get_stateful_citation_audit(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<CitationAuditResponse>> {
    let run = orchestrator::load_run(&state.app_config, &run_id).await?;
    let citation_audit = run
        .citation_audit
        .ok_or_else(|| AppError::Workflow("run has no CitationAuditReport yet".to_string()))?;
    Ok(Json(CitationAuditResponse {
        run_id,
        citation_audit,
    }))
}

async fn list_stateful_checkpoints(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<CheckpointListResponse>> {
    let mut checkpoints = Vec::new();
    let checkpoint_dir = state
        .app_config
        .session_dir
        .join(&run_id)
        .join("checkpoints");
    if let Ok(mut entries) = tokio::fs::read_dir(checkpoint_dir).await {
        while let Some(entry) = entries.next_entry().await? {
            let body = tokio::fs::read_to_string(entry.path()).await?;
            let snapshot: checkpoint::CheckpointSnapshot = serde_json::from_str(&body)?;
            checkpoints.push(snapshot.checkpoint);
        }
    }
    checkpoints.sort_by_key(|checkpoint| checkpoint.created_at);
    Ok(Json(CheckpointListResponse {
        run_id,
        checkpoints,
    }))
}

async fn branch_stateful_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(req): Json<BranchRunRequest>,
) -> Result<Json<StatefulRunResponse>> {
    let run = orchestrator::branch_from_plan_ready(
        (*state.app_config).clone(),
        &run_id,
        &req.checkpoint_id,
    )
    .await?;
    Ok(Json(StatefulRunResponse {
        run_id: run.run_id.clone(),
        topic: run.topic.clone(),
        state: run.state.clone(),
        run,
    }))
}

async fn list_reading_library(
    State(state): State<AppState>,
) -> Result<Json<ReadingLibraryResponse>> {
    let items = reading_library::list_items(&state.app_config).await?;
    Ok(Json(ReadingLibraryResponse { items }))
}

async fn add_reading_library_item(
    State(state): State<AppState>,
    Json(req): Json<AddReadingLibraryItemRequest>,
) -> Result<Json<ReadingLibraryItemResponse>> {
    let item = reading_library::add_item(&state.app_config, req.run_id, req.evidence).await?;
    Ok(Json(ReadingLibraryItemResponse { item }))
}

async fn get_reading_library_item(
    State(state): State<AppState>,
    Path(paper_key): Path<String>,
) -> Result<Json<ReadingLibraryItemResponse>> {
    let item = reading_library::get_item(&state.app_config, &paper_key).await?;
    Ok(Json(ReadingLibraryItemResponse { item }))
}

async fn delete_reading_library_item(
    State(state): State<AppState>,
    Path(paper_key): Path<String>,
) -> Result<Json<ReadingLibraryResponse>> {
    reading_library::delete_item(&state.app_config, &paper_key).await?;
    let items = reading_library::list_items(&state.app_config).await?;
    Ok(Json(ReadingLibraryResponse { items }))
}

async fn generate_reading_note(
    State(state): State<AppState>,
    Path(paper_key): Path<String>,
    Json(req): Json<GenerateReadingNoteRequest>,
) -> Result<Json<ReadingLibraryItemResponse>> {
    let mut item = reading_library::get_item(&state.app_config, &paper_key).await?;
    item.error = None;
    item.note_quality = None;
    item.updated_at = chrono::Utc::now();
    reading_library::save_item(&state.app_config, &item).await?;

    let fetch_item = item.clone();
    let callback_config = state.app_config.clone();
    let callback_paper_key = paper_key.clone();
    let mut status_callback = move |status: ReadingStatus| -> Pin<
        Box<dyn std::future::Future<Output = Result<()>> + Send>,
    > {
        let app_config = callback_config.clone();
        let paper_key = callback_paper_key.clone();
        Box::pin(async move {
            let mut item = reading_library::get_item(&app_config, &paper_key).await?;
            item.status = status;
            item.updated_at = chrono::Utc::now();
            reading_library::save_item(&app_config, &item).await
        })
    };

    let text = match full_text_fetcher::fetch_full_text(
        &state.app_config,
        &fetch_item,
        Some(&mut status_callback),
    )
    .await
    {
        Ok(text) => text,
        Err(err) => {
            item.status = ReadingStatus::TextFailed;
            item.error = Some(err.to_string());
            item.updated_at = chrono::Utc::now();
            reading_library::save_item(&state.app_config, &item).await?;
            return Ok(Json(ReadingLibraryItemResponse { item }));
        }
    };
    item = reading_library::get_item(&state.app_config, &paper_key).await?;
    item.text = Some(text.text);
    item.text_source_url = Some(text.source_url);
    item.text_coverage = Some(text.coverage);
    item.text_meta = Some(text.meta);
    if matches!(
        item.text_coverage,
        Some(TextCoverage::Failed | TextCoverage::AbstractOnly)
    ) {
        item.status = ReadingStatus::TextFailed;
        item.error = Some("全文获取失败，未生成详细笔记。".to_string());
        item.updated_at = chrono::Utc::now();
        reading_library::save_item(&state.app_config, &item).await?;
        return Ok(Json(ReadingLibraryItemResponse { item }));
    }

    item.status = ReadingStatus::TextReady;
    item.updated_at = chrono::Utc::now();
    reading_library::save_item(&state.app_config, &item).await?;

    item.status = ReadingStatus::GeneratingNote;
    item.updated_at = chrono::Utc::now();
    reading_library::save_item(&state.app_config, &item).await?;

    let llm_config = effective_llm_config(&state, &req.config);
    match note_writer::generate_note(&item, &llm_config).await {
        Ok(note) => {
            item.note = Some(note);
            item.note_quality = Some(NoteQuality::FullText);
            item.status = ReadingStatus::Ready;
            item.error = None;
        }
        Err(err) => {
            item.status = ReadingStatus::NoteFailed;
            item.error = Some(err.to_string());
        }
    }
    item.updated_at = chrono::Utc::now();
    reading_library::save_item(&state.app_config, &item).await?;
    Ok(Json(ReadingLibraryItemResponse { item }))
}

async fn chat_with_paper_stream(
    State(state): State<AppState>,
    Path(paper_key): Path<String>,
    Json(req): Json<PaperChatRequest>,
) -> Sse<impl Stream<Item = std::result::Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<ChatStreamEvent>(32);

    tokio::spawn(async move {
        let mut item = match reading_library::get_item(&state.app_config, &paper_key).await {
            Ok(item) => item,
            Err(err) => {
                let _ = tx
                    .send(ChatStreamEvent::Failed {
                        error: err.to_string(),
                    })
                    .await;
                return;
            }
        };
        let llm_config = effective_llm_config(&state, &req.config);
        let request = match paper_chat::build_paper_chat_request(&item, &req.question, &llm_config)
        {
            Ok(request) => request,
            Err(err) => {
                let _ = tx
                    .send(ChatStreamEvent::Failed {
                        error: err.to_string(),
                    })
                    .await;
                return;
            }
        };
        let config = match DeepSeekConfig::from_llm_config(&llm_config) {
            Some(config) => config,
            None => {
                let _ = tx
                    .send(ChatStreamEvent::Failed {
                        error: "论文追问需要 DeepSeek API Key。".to_string(),
                    })
                    .await;
                return;
            }
        };
        let client = match DeepSeekClient::new(config) {
            Ok(client) => client,
            Err(err) => {
                let _ = tx
                    .send(ChatStreamEvent::Failed {
                        error: err.to_string(),
                    })
                    .await;
                return;
            }
        };
        let (delta_tx, mut delta_rx) = mpsc::channel::<String>(32);
        let forward_tx = tx.clone();
        tokio::spawn(async move {
            while let Some(text) = delta_rx.recv().await {
                if forward_tx
                    .send(ChatStreamEvent::Delta { text })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
        match client
            .chat_completions_stream_text(request, Some(delta_tx))
            .await
        {
            Ok(answer) => {
                let now = chrono::Utc::now();
                item.chat_history.push(PaperChatMessage {
                    role: "user".to_string(),
                    content: req.question,
                    created_at: now,
                });
                item.chat_history.push(PaperChatMessage {
                    role: "assistant".to_string(),
                    content: answer,
                    created_at: chrono::Utc::now(),
                });
                item.updated_at = chrono::Utc::now();
                let _ = reading_library::save_item(&state.app_config, &item).await;
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

fn effective_app_config(state: &AppState, frontend: &FrontendConfig) -> AppConfig {
    let mut app_config = (*state.app_config).clone();
    if let Some(token) = non_empty(frontend.github_token.as_deref()) {
        app_config.github_token = Some(token.to_string());
    }
    if let Some(key) = non_empty(frontend.semantic_scholar_api_key.as_deref()) {
        app_config.semantic_scholar_api_key = Some(key.to_string());
    }
    if let Some(key) = non_empty(frontend.openalex_api_key.as_deref()) {
        app_config.openalex_api_key = Some(key.to_string());
    }
    if let Some(mailto) = non_empty(frontend.crossref_mailto.as_deref()) {
        app_config.crossref_mailto = Some(mailto.to_string());
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
            semantic_scholar_api_key: None,
            openalex_api_key: None,
            crossref_mailto: None,
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
