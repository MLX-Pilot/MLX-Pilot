//! Deep Research endpoints — async job-based research with SSE progress.
//!
//! Endpoints:
//! - `POST /api/research/start` — create research job, returns `{job_id}`
//! - `GET  /api/research/stream/{job_id}` — SSE progress stream
//! - `POST /api/research/cancel/{job_id}` — cancel running research
//! - `GET  /api/research/result/{job_id}` — get completed result
//! - `GET  /api/research/report/{id}` — serve HTML report
//! - `GET  /api/research/library` — list completed research sessions
//! - `POST /api/research/spinoff/{id}` — create chat session seeded with report
//! - `POST /api/research/{id}/hide-image` — hide an image
//! - `POST /api/research/{id}/unhide-images` — unhide all images
//! - `DELETE /api/research/{id}` — delete a research session

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::Sse;
use axum::Json;
use futures_util::Stream;
use mlx_ollama_core::{ChatMessage, ChatRequest, MessageRole};
use mlx_research::{
    self, hide_image, load_session, persist_session, ResearchConfig,
    ResearchEngine, ResearchStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tracing::{error, warn};

use crate::jobs::{self, JobCtx, JobId};
use crate::search::{self, SearchQuery};

// ── Request/response types ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ResearchStartRequest {
    pub query: String,
    #[serde(default = "default_max_rounds")]
    pub max_rounds: usize,
    #[serde(default = "default_max_time")]
    pub max_time_secs: u64,
    pub search_provider: Option<String>,
    pub model_id: Option<String>,
    pub category: Option<String>,
    pub owner: Option<String>,
}

fn default_max_rounds() -> usize {
    5
}
fn default_max_time() -> u64 {
    300
}

#[derive(Debug, Deserialize)]
pub struct HideImageRequest {
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct ResearchResultResponse {
    pub id: String,
    pub query: String,
    pub status: ResearchStatus,
    pub report_md: Option<String>,
    pub report_html: Option<String>,
    pub sources: Vec<mlx_research::Source>,
    pub findings: Vec<mlx_research::Finding>,
    pub stats: Option<mlx_research::ResearchStats>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LibraryEntry {
    pub id: String,
    pub query: String,
    pub status: ResearchStatus,
    pub created_at: String,
    pub finished_at: Option<String>,
    pub rounds: usize,
    pub sources: usize,
    pub category: Option<String>,
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn research_data_dir() -> PathBuf {
    crate::AppConfig::get_settings_path()
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("agent")
}

/// Resolve a model_id for research. If "auto" or empty, picks the first available.
/// Cloud model IDs (e.g., `deepseek:deepseek-chat`) flow through to chat_with_routing.
async fn resolve_research_model(
    state: &crate::AppState,
    requested: Option<&str>,
) -> Result<String, String> {
    let requested = requested.filter(|s| !s.is_empty() && *s != "auto");

    if let Some(id) = requested {
        // Cloud-prefixed model IDs (deepseek:, openai:, anthropic:, etc.) pass through as-is
        let known_prefixes = ["deepseek:", "openai:", "anthropic:", "groq:", "openrouter:"];
        if known_prefixes.iter().any(|p| id.starts_with(p)) || id.contains("::") {
            return Ok(id.to_string());
        }
        // Local provider prefix (mlx::, ollama::, llama::) also passes through
        let local_prefixes = ["mlx::", "ollama::", "llama::"];
        if local_prefixes.iter().any(|p| id.starts_with(p)) {
            return Ok(id.to_string());
        }
        // Bare model ID — check if it exists locally
        let local = crate::list_chat_models(state)
            .await
            .map_err(|e| format!("{e}"))?;
        if local.iter().any(|m| m.id == id || m.path == id) {
            return Ok(id.to_string());
        }
        // Not local — try each configured cloud provider, qualify with provider prefix
        if let Some(ref vault) = state.vault {
            for cfg in crate::model_catalog::cloud_provider_configs() {
                if crate::model_catalog::get_api_key(Some(vault), &cfg).is_some() {
                    return Ok(format!("{}:{}", cfg.provider_key, id));
                }
            }
        }
        return Err(format!(
            "Modelo '{}' não encontrado. Verifique se ele está carregado ou disponível como cloud.",
            id
        ));
    }

    // "auto" — pick first available local model
    let local = crate::list_chat_models(state)
        .await
        .map_err(|e| format!("{e}"))?;
    if let Some(m) = local.first() {
        let model_id = if !m.path.is_empty() { &m.path } else { &m.id };
        return Ok(model_id.clone());
    }

    // No local model — pick first configured cloud provider with its first known model
    if let Some(ref vault) = state.vault {
        for cfg in crate::model_catalog::cloud_provider_configs() {
            if crate::model_catalog::get_api_key(Some(vault), &cfg).is_some() {
                // Use the provider's default/first model. chat_with_routing with the
                // "provider:" prefix (without a specific model) won't work, so pick
                // a reasonable default per provider.
                let default_model = match cfg.provider_key.as_str() {
                    "deepseek" => "deepseek-chat",
                    "openai" => "gpt-4.1",
                    "anthropic" => "claude-sonnet-4-6",
                    "groq" => "llama-3.3-70b-versatile",
                    "openrouter" => "openai/gpt-4.1",
                    _ => "",
                };
                if !default_model.is_empty() {
                    return Ok(format!("{}:{}", cfg.provider_key, default_model));
                }
            }
        }
    }

    Err(
        "Nenhum modelo disponível. Carregue um modelo local ou configure uma chave API cloud (ex.: DEEPSEEK_API_KEY) no cofre de segredos."
            .to_string(),
    )
}

// ── POST /api/research/start ───────────────────────────────────────────────

pub async fn research_start(
    State(state): State<crate::AppState>,
    Json(req): Json<ResearchStartRequest>,
) -> Result<Json<Value>, StatusCode> {
    if req.query.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    // ── Fail-fast: check model availability ──
    let model_id = match resolve_research_model(&state, req.model_id.as_deref()).await {
        Ok(id) => id,
        Err(msg) => {
            return Ok(Json(json!({
                "error": true,
                "message": msg,
                "hint": "Carregue um modelo local ou configure uma API key cloud (DEEPSEEK_API_KEY, OPENAI_API_KEY, etc.) no cofre de segredos."
            })));
        }
    };

    // Pre-flight: verify the model actually exists via a quick probe
    // (skip full probe for cloud-prefixed models — they'll resolve at call time)
    if !model_id.contains(':') && !model_id.contains("::") {
        let local = crate::list_chat_models(&state).await.unwrap_or_default();
        if !local.iter().any(|m| m.id == model_id || m.path == model_id) {
            return Ok(Json(json!({
                "error": true,
                "message": format!("Modelo '{}' não está carregado no momento.", model_id),
                "hint": "Carregue o modelo ou use 'auto' para seleção automática."
            })));
        }
    }

    let max_rounds = req.max_rounds.min(10).max(1);
    let max_time_secs = req.max_time_secs.min(600).max(30);
    let query = req.query.trim().to_string();
    let search_provider = req.search_provider.clone();
    let category = req.category.clone();
    let owner = req.owner.clone();

    let data_dir = research_data_dir();
    let search_service = state.search_service.clone();
    let state_clone = state.clone();
    let search_config = state.search_config.clone();

    let config = ResearchConfig {
        max_rounds,
        max_time_secs,
        ..Default::default()
    };

    let job_id: JobId = state
        .jobs
        .spawn("deep_research", move |ctx: JobCtx| {
            let query = query.clone();
            let model_id = model_id.clone();
            let search_provider = search_provider.clone();
            let category = category.clone();
            let owner = owner.clone();
            let data_dir = data_dir.clone();
            let search_service = search_service.clone();
            let state = state_clone.clone();
            let search_config = search_config.clone();

            async move {
                // ── Build injected functions ──

                // Search function wrapping the daemon's search service
                let search_fn: mlx_research::SearchFn = {
                    let svc = search_service.clone();
                    Arc::new(
                        move |q: String, provider: Option<String>, max_results: usize| {
                            let svc = svc.clone();
                            Box::pin(async move {
                                let sq = SearchQuery {
                                    q,
                                    provider,
                                    max_results: Some(max_results.min(10)),
                                    safe_search: Some(true),
                                };
                                match svc.search(&sq).await {
                                    Ok(results) => Ok(results
                                        .into_iter()
                                        .map(|r| mlx_research::Source {
                                            url: r.url,
                                            title: r.title,
                                            snippet: r.snippet,
                                            provider: r.provider,
                                            relevance: None,
                                        })
                                        .collect()),
                                    Err(e) => {
                                        warn!("Search error in research: {e}");
                                        Ok(vec![])
                                    }
                                }
                            })
                        },
                    )
                };

                // Fetch function wrapping the daemon's fetch_and_extract
                let fetch_fn: mlx_research::FetchFn = {
                    let cfg = search_config.clone();
                    Arc::new(move |url: String| {
                        let cfg = cfg.clone();
                        Box::pin(async move {
                            match search::fetch_and_extract(
                                &url,
                                cfg.fetch_timeout_secs,
                                cfg.fetch_max_bytes,
                            )
                            .await
                            {
                                Ok(result) => Ok((result.title, result.text)),
                                Err(e) => {
                                    warn!("Fetch error in research for {}: {e}", url);
                                    Ok((String::new(), String::new()))
                                }
                            }
                        })
                    })
                };

                // LLM function wrapping chat_with_routing
                let llm_model_id = model_id.clone();
                let llm_fn: mlx_research::LlmFn = {
                    let state = state.clone();
                    Arc::new(move |system_prompt: String, user_prompt: String| {
                        let state = state.clone();
                        let model_id = llm_model_id.clone();
                        Box::pin(async move {
                            let request = ChatRequest {
                                model_id: model_id.clone(),
                                messages: vec![
                                    ChatMessage::text(MessageRole::System, system_prompt),
                                    ChatMessage::text(MessageRole::User, user_prompt),
                                ],
                                options: Default::default(),
                            };

                            match crate::chat_with_routing(&state, request).await {
                                Ok(response) => Ok(response.message.content),
                                Err(e) => {
                                    warn!("LLM call error in research: {e}");
                                    Err(format!("LLM error: {e}"))
                                }
                            }
                        })
                    })
                };

                let engine = ResearchEngine::new(config, search_fn, fetch_fn, llm_fn);

                let session = engine
                    .run(
                        &query,
                        &model_id,
                        search_provider,
                        category,
                        owner,
                        ctx.token.clone(),
                        {
                            let ctx = ctx.clone();
                            move |event| {
                                let pct = event.percent;
                                let phase = event.phase;
                                let msg = event.message;
                                ctx.progress(pct, &phase, &msg);
                            }
                        },
                    )
                    .await;

                // Persist the completed session
                if let Err(e) = persist_session(&data_dir, &session) {
                    error!("Failed to persist research session {}: {e}", session.id);
                }

                // Return result as JSON
                let result = json!({
                    "session_id": session.id,
                    "status": session.status,
                    "rounds": session.rounds.len(),
                    "sources": session.all_sources.len(),
                    "findings": session.all_findings.len(),
                    "report_html": session.final_report_html,
                });

                match session.status {
                    ResearchStatus::Done => Ok(result),
                    ResearchStatus::Cancelled => Err("Research cancelled".to_string()),
                    ResearchStatus::Error => Err(session.error.unwrap_or_else(|| "Unknown error".to_string())),
                    _ => Err("Research incomplete".to_string()),
                }
            }
        })
        .await;

    Ok(Json(json!({"job_id": job_id})))
}

// ── GET /api/research/stream/{job_id} ──────────────────────────────────────

pub async fn research_stream(
    State(state): State<crate::AppState>,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Sse<impl Stream<Item = Result<axum::response::sse::Event, io::Error>>>, StatusCode> {
    let rx = state
        .jobs
        .subscribe(&job_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;

    let stream = BroadcastStream::new(rx).map(|result| match result {
        Ok(progress) => {
            let json = serde_json::to_string(&progress).unwrap_or_default();
            Ok(axum::response::sse::Event::default().data(json))
        }
        Err(_) => Ok(axum::response::sse::Event::default().comment("stream ended")),
    });

    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}

// ── POST /api/research/cancel/{job_id} ─────────────────────────────────────

pub async fn research_cancel(
    State(state): State<crate::AppState>,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    state
        .jobs
        .cancel(&job_id)
        .await
        .map(|record| Json(json!({"cancelled": true, "job": record})))
        .ok_or(StatusCode::NOT_FOUND)
}

// ── GET /api/research/result/{job_id} ──────────────────────────────────────

pub async fn research_result(
    State(state): State<crate::AppState>,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Json<ResearchResultResponse>, StatusCode> {
    let job = state.jobs.get(&job_id).await.ok_or(StatusCode::NOT_FOUND)?;

    let session_id = if let Some(ref result) = job.result {
        result.get("session_id").and_then(|v| v.as_str()).map(|s| s.to_string())
    } else {
        Some(job_id.clone())
    };

    let session_id = session_id.ok_or(StatusCode::NOT_FOUND)?;

    let data_dir = research_data_dir();
    match load_session(&data_dir, &session_id) {
        Ok(session) => Ok(Json(ResearchResultResponse {
            id: session.id,
            query: session.query,
            status: session.status,
            report_md: session.final_report_md,
            report_html: session.final_report_html,
            sources: session.all_sources,
            findings: session.all_findings,
            stats: session.stats,
            error: session.error,
        })),
        Err(_) => {
            Ok(Json(ResearchResultResponse {
                id: session_id,
                query: String::new(),
                status: match job.status {
                    jobs::JobStatus::Done => ResearchStatus::Done,
                    jobs::JobStatus::Error => ResearchStatus::Error,
                    jobs::JobStatus::Cancelled => ResearchStatus::Cancelled,
                    jobs::JobStatus::Running => ResearchStatus::Running,
                    jobs::JobStatus::Queued => ResearchStatus::Queued,
                },
                report_md: None,
                report_html: None,
                sources: vec![],
                findings: vec![],
                stats: None,
                error: job.error,
            }))
        }
    }
}

// ── GET /api/research/report/{id} ──────────────────────────────────────────

pub async fn research_report(
    AxumPath(id): AxumPath<String>,
) -> Result<axum::response::Html<String>, StatusCode> {
    let data_dir = research_data_dir();
    match load_session(&data_dir, &id) {
        Ok(session) => {
            if let Some(html) = session.final_report_html {
                Ok(axum::response::Html(html))
            } else if let Some(md) = session.final_report_md {
                let title = mlx_research::extract_title_from_markdown(&md);
                let html = mlx_research::generate_html_report(
                    &md,
                    &title,
                    &session.all_sources,
                    session.stats.as_ref(),
                    &session.hidden_images,
                    session.category.as_deref(),
                );
                Ok(axum::response::Html(html))
            } else {
                Err(StatusCode::NOT_FOUND)
            }
        }
        Err(_) => Err(StatusCode::NOT_FOUND),
    }
}

// ── GET /api/research/library ──────────────────────────────────────────────

pub async fn research_library() -> Result<Json<Vec<LibraryEntry>>, StatusCode> {
    let data_dir = research_data_dir();
    match mlx_research::list_sessions(&data_dir) {
        Ok(sessions) => {
            let entries: Vec<LibraryEntry> = sessions
                .into_iter()
                .map(|s| LibraryEntry {
                    id: s.id,
                    query: s.query,
                    status: s.status,
                    created_at: s.created_at.to_rfc3339(),
                    finished_at: s.finished_at.map(|t| t.to_rfc3339()),
                    rounds: s.rounds.len(),
                    sources: s.all_sources.len(),
                    category: s.category,
                })
                .collect();
            Ok(Json(entries))
        }
        Err(e) => {
            error!("Failed to list research sessions: {e}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// ── POST /api/research/spinoff/{id} ────────────────────────────────────────

pub async fn research_spinoff(
    State(state): State<crate::AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let data_dir = research_data_dir();
    let session = load_session(&data_dir, &id).map_err(|_| StatusCode::NOT_FOUND)?;

    let report_text = session
        .final_report_md
        .unwrap_or_else(|| session.query.clone());
    let session_name = format!(
        "Research: {}",
        &session.query[..session.query.len().min(50)]
    );

    let new_id = mlx_agent_core::session::SessionStore::new_session_id();

    if let Err(e) = state
        .session_store
        .ensure_session(&new_id, Some(session_name.clone()))
        .await
    {
        error!("Failed to create spinoff session: {e}");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Add the research report as a system message
    let context_msg = format!(
        "I just completed research on: {}\n\n# Research Report\n\n{}\n\nSources: {}",
        session.query,
        report_text,
        session.all_sources.len()
    );

    let sys_msg = mlx_agent_core::session::SessionMessage::user(context_msg);

    if let Err(e) = state.session_store.append(&new_id, &sys_msg).await {
        error!("Failed to add context to spinoff session: {e}");
    }

    Ok(Json(json!({
        "session_id": new_id,
        "name": session_name,
        "query": session.query,
    })))
}

// ── POST /api/research/{id}/hide-image ─────────────────────────────────────

pub async fn research_hide_image(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<HideImageRequest>,
) -> Result<Json<Value>, StatusCode> {
    let data_dir = research_data_dir();
    hide_image(&data_dir, &id, &req.url).map_err(|e| {
        error!("Failed to hide image: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(json!({"ok": true})))
}

// ── POST /api/research/{id}/unhide-images ──────────────────────────────────

pub async fn research_unhide_images(
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let data_dir = research_data_dir();
    mlx_research::unhide_all_images(&data_dir, &id).map_err(|e| {
        error!("Failed to unhide images: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(json!({"ok": true})))
}

// ── DELETE /api/research/{id} ──────────────────────────────────────────────

pub async fn research_delete(
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let data_dir = research_data_dir();
    mlx_research::delete_session(&data_dir, &id).map_err(|e| {
        error!("Failed to delete research session: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(json!({"ok": true})))
}
