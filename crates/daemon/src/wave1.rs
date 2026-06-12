//! Wave-1 feature endpoints: presets, persistent memory CRUD, session/history
//! organization + editing, and side-by-side model comparison.
//!
//! All persistence rides on the shared agent `state.sqlite` via the public
//! `PresetStore`, `CompareStore`, `MemoryStore`, and `SessionStore` from
//! `mlx-agent-core`. LLM calls reuse the daemon's existing provider routing
//! (`crate::chat_with_routing`) so these features work with every local and
//! remote provider the app already supports — keeping everything native.

use std::fmt::Display;
use std::time::Instant;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::AppState;
use mlx_agent_core::{
    Comparison, ComparisonEntry, MemoryRecord, MemorySearchHit, Preset, SessionMessage,
};
use mlx_ollama_core::{ChatMessage, ChatRequest, GenerationOptions, MessageRole};

// ─────────────────────────────────────────────────────────────────────────
// Error handling
// ─────────────────────────────────────────────────────────────────────────

pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

fn internal(error: impl Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

fn not_found(message: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, message)
}

fn bad_request(message: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, message)
}

type ApiResult<T> = Result<Json<T>, ApiError>;

fn ok() -> Json<Value> {
    Json(json!({ "ok": true }))
}

// ─────────────────────────────────────────────────────────────────────────
// Presets
// ─────────────────────────────────────────────────────────────────────────

pub async fn list_presets(State(state): State<AppState>) -> ApiResult<Vec<Preset>> {
    let presets = state.presets.list().await.map_err(internal)?;
    Ok(Json(presets))
}

pub async fn save_preset(
    State(state): State<AppState>,
    Json(mut preset): Json<Preset>,
) -> ApiResult<Preset> {
    if preset.name.trim().is_empty() {
        return Err(bad_request("preset name cannot be empty"));
    }
    let now = Utc::now();
    if preset.id.trim().is_empty() {
        preset.id = uuid::Uuid::new_v4().to_string();
        preset.created_at = now;
    } else if state
        .presets
        .get(&preset.id)
        .await
        .map_err(internal)?
        .is_none()
    {
        preset.created_at = now;
    }
    preset.updated_at = now;
    state.presets.save(&preset).await.map_err(internal)?;
    Ok(Json(preset))
}

pub async fn get_preset(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Preset> {
    let preset = state
        .presets
        .get(&id)
        .await
        .map_err(internal)?
        .ok_or_else(|| not_found(format!("preset {id} not found")))?;
    Ok(Json(preset))
}

pub async fn delete_preset(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Value> {
    state.presets.delete(&id).await.map_err(internal)?;
    Ok(ok())
}

// ─────────────────────────────────────────────────────────────────────────
// Persistent memory
// ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct MemoryListQuery {
    pub scope: Option<String>,
    pub kind: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct MemorySearchQuery {
    pub q: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct AddMemoryRequest {
    pub title: String,
    pub content: String,
    pub kind: Option<String>,
    pub scope: Option<String>,
    pub namespace: Option<String>,
    pub tags: Option<Vec<String>>,
    pub importance: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct PinRequest {
    pub pinned: bool,
}

pub async fn list_memory(
    State(state): State<AppState>,
    Query(query): Query<MemoryListQuery>,
) -> ApiResult<Vec<MemoryRecord>> {
    let records = state
        .agent_state
        .memory
        .list(
            query.scope.as_deref().filter(|s| !s.is_empty()),
            query.kind.as_deref().filter(|s| !s.is_empty()),
            query.pinned,
            query.limit.unwrap_or(0),
        )
        .await
        .map_err(internal)?;
    Ok(Json(records))
}

pub async fn search_memory(
    State(state): State<AppState>,
    Query(query): Query<MemorySearchQuery>,
) -> ApiResult<Vec<MemorySearchHit>> {
    let hits = state
        .agent_state
        .memory
        .search(&query.q, query.limit.unwrap_or(20))
        .await
        .map_err(internal)?;
    Ok(Json(hits))
}

pub async fn add_memory(
    State(state): State<AppState>,
    Json(req): Json<AddMemoryRequest>,
) -> ApiResult<MemoryRecord> {
    if req.title.trim().is_empty() && req.content.trim().is_empty() {
        return Err(bad_request("memory needs a title or content"));
    }
    let now = Utc::now();
    let record = MemoryRecord {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: String::new(),
        source_session_id: String::new(),
        scope: req
            .scope
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "long_term".to_string()),
        namespace: req
            .namespace
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "default".to_string()),
        kind: req
            .kind
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "note".to_string()),
        title: req.title,
        content: req.content,
        tags: req.tags.unwrap_or_default(),
        created_at: now,
        metadata: Default::default(),
        importance: req.importance.unwrap_or(0),
        last_accessed_at: None,
        pin_state: "pinned".to_string(),
        promotion_source: "manual".to_string(),
        summary_ref: String::new(),
    };
    state.agent_state.memory.save(&record).await.map_err(internal)?;
    Ok(Json(record))
}

pub async fn get_memory(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<MemoryRecord> {
    let record = state
        .agent_state
        .memory
        .get(&id)
        .await
        .map_err(internal)?
        .ok_or_else(|| not_found(format!("memory {id} not found")))?;
    Ok(Json(record))
}

pub async fn update_memory(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(mut record): Json<MemoryRecord>,
) -> ApiResult<MemoryRecord> {
    let existing = state
        .agent_state
        .memory
        .get(&id)
        .await
        .map_err(internal)?
        .ok_or_else(|| not_found(format!("memory {id} not found")))?;
    record.id = id;
    record.created_at = existing.created_at;
    state.agent_state.memory.save(&record).await.map_err(internal)?;
    Ok(Json(record))
}

pub async fn delete_memory(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Value> {
    state.agent_state.memory.delete(&id).await.map_err(internal)?;
    Ok(ok())
}

pub async fn pin_memory(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<PinRequest>,
) -> ApiResult<Value> {
    let found = state
        .agent_state
        .memory
        .set_pin(&id, req.pinned)
        .await
        .map_err(internal)?;
    if !found {
        return Err(not_found(format!("memory {id} not found")));
    }
    Ok(Json(json!({ "ok": true, "pinned": req.pinned })))
}

// ─────────────────────────────────────────────────────────────────────────
// Sessions / history organization + editing
// ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct MessageWithId {
    pub event_id: i64,
    #[serde(flatten)]
    pub message: SessionMessage,
}

#[derive(Debug, Deserialize)]
pub struct SessionFlagsRequest {
    pub folder: Option<String>,
    pub archived: Option<bool>,
    pub pinned: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ForkRequest {
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TruncateRequest {
    pub event_id: i64,
}

#[derive(Debug, Deserialize)]
pub struct EditMessageRequest {
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct ExportQuery {
    #[serde(default)]
    pub format: Option<String>,
}

pub async fn session_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Vec<MessageWithId>> {
    let rows = state.session_store.load_with_ids(&id).await.map_err(internal)?;
    let out = rows
        .into_iter()
        .map(|(event_id, message)| MessageWithId { event_id, message })
        .collect();
    Ok(Json(out))
}

pub async fn session_set_flags(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<SessionFlagsRequest>,
) -> ApiResult<Value> {
    if let Some(folder) = req.folder {
        state.session_store.set_folder(&id, &folder).await.map_err(internal)?;
    }
    if let Some(archived) = req.archived {
        state.session_store.set_archived(&id, archived).await.map_err(internal)?;
    }
    if let Some(pinned) = req.pinned {
        state.session_store.set_pinned(&id, pinned).await.map_err(internal)?;
    }
    Ok(ok())
}

pub async fn session_fork(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ForkRequest>,
) -> ApiResult<Value> {
    let new_id = state
        .session_store
        .fork(&id, req.name)
        .await
        .map_err(internal)?
        .ok_or_else(|| not_found(format!("session {id} not found")))?;
    Ok(Json(json!({ "ok": true, "new_session_id": new_id })))
}

pub async fn session_truncate(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<TruncateRequest>,
) -> ApiResult<Value> {
    let removed = state
        .session_store
        .truncate_after(&id, req.event_id)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "ok": true, "removed": removed })))
}

pub async fn session_edit_message(
    State(state): State<AppState>,
    Path((_id, event_id)): Path<(String, i64)>,
    Json(req): Json<EditMessageRequest>,
) -> ApiResult<Value> {
    state
        .session_store
        .edit_message(event_id, &req.content)
        .await
        .map_err(internal)?;
    Ok(ok())
}

pub async fn session_delete_message(
    State(state): State<AppState>,
    Path((id, event_id)): Path<(String, i64)>,
) -> ApiResult<Value> {
    state
        .session_store
        .delete_message(&id, event_id)
        .await
        .map_err(internal)?;
    Ok(ok())
}

pub async fn session_export(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ExportQuery>,
) -> Result<Response, ApiError> {
    let meta = state
        .session_store
        .get_meta(&id)
        .await
        .map_err(internal)?
        .ok_or_else(|| not_found(format!("session {id} not found")))?;
    let messages = state.session_store.load(&id).await.map_err(internal)?;
    let format = query.format.unwrap_or_else(|| "md".to_string());
    let (content_type, ext, body) = match format.as_str() {
        "json" => (
            "application/json; charset=utf-8",
            "json",
            serde_json::to_string_pretty(&messages).map_err(internal)?,
        ),
        "txt" => (
            "text/plain; charset=utf-8",
            "txt",
            export_text(&meta.name, &messages),
        ),
        "html" => (
            "text/html; charset=utf-8",
            "html",
            export_html(&meta.name, &messages),
        ),
        _ => (
            "text/markdown; charset=utf-8",
            "md",
            export_markdown(&meta.name, &messages),
        ),
    };
    let filename = format!("{}.{}", sanitize_filename(&meta.name), ext);
    Ok((
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        body,
    )
        .into_response())
}

fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|ch| if ch.is_alphanumeric() || ch == '-' || ch == '_' { ch } else { '_' })
        .collect();
    let trimmed = cleaned.trim_matches('_');
    if trimmed.is_empty() {
        "session".to_string()
    } else {
        trimmed.chars().take(60).collect()
    }
}

fn role_label(message: &SessionMessage) -> &'static str {
    match message.role.to_ascii_lowercase().as_str() {
        "user" => "Usuário",
        "assistant" => "Assistente",
        "tool" => "Ferramenta",
        "system" => "Sistema",
        _ => "Mensagem",
    }
}

fn export_markdown(name: &str, messages: &[SessionMessage]) -> String {
    let mut out = format!("# {name}\n\n");
    for message in messages {
        if message.content.trim().is_empty() {
            continue;
        }
        out.push_str(&format!("**{}**\n\n{}\n\n", role_label(message), message.content));
    }
    out
}

fn export_text(name: &str, messages: &[SessionMessage]) -> String {
    let mut out = format!("{name}\n{}\n\n", "=".repeat(name.chars().count().max(3)));
    for message in messages {
        if message.content.trim().is_empty() {
            continue;
        }
        out.push_str(&format!("[{}] {}\n\n", role_label(message), message.content));
    }
    out
}

fn export_html(name: &str, messages: &[SessionMessage]) -> String {
    let mut body = String::new();
    for message in messages {
        if message.content.trim().is_empty() {
            continue;
        }
        body.push_str(&format!(
            "<div class=\"msg {}\"><div class=\"role\">{}</div><div class=\"content\">{}</div></div>\n",
            message.role.to_ascii_lowercase(),
            role_label(message),
            html_escape(&message.content)
        ));
    }
    format!(
        "<!doctype html><html lang=\"pt-br\"><head><meta charset=\"utf-8\">\
<title>{title}</title><style>body{{font-family:system-ui,sans-serif;max-width:760px;\
margin:32px auto;padding:0 16px;color:#1a1a2e;background:#fafafe}}h1{{font-size:22px}}\
.msg{{margin:16px 0;padding:12px 16px;border-radius:10px;background:#fff;\
border:1px solid #e6e6f0}}.role{{font-weight:600;font-size:12px;text-transform:uppercase;\
letter-spacing:.04em;color:#6b6b8a;margin-bottom:6px}}.content{{white-space:pre-wrap;\
line-height:1.55}}.msg.assistant{{background:#f3f6ff}}</style></head><body>\
<h1>{title}</h1>{body}</body></html>",
        title = html_escape(name),
        body = body
    )
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// ─────────────────────────────────────────────────────────────────────────
// Compare — fan one prompt to N models, optionally blind, with voting
// ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CompareRunRequest {
    pub prompt: String,
    #[serde(default)]
    pub system_prompt: String,
    pub models: Vec<String>,
    #[serde(default = "default_blind")]
    pub blind: bool,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

fn default_blind() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct CompareVoteRequest {
    pub winner_label: String,
}

#[derive(Debug, Deserialize)]
pub struct CompareSynthesizeRequest {
    pub model_id: String,
}

#[derive(Debug, Deserialize)]
pub struct CompareHistoryQuery {
    pub limit: Option<usize>,
}

fn label_for(index: usize) -> String {
    if index < 26 {
        ((b'A' + index as u8) as char).to_string()
    } else {
        format!("M{}", index + 1)
    }
}

async fn run_one(
    state: &AppState,
    label: String,
    model_id: String,
    system_prompt: &str,
    prompt: &str,
    options: GenerationOptions,
) -> ComparisonEntry {
    let mut messages = Vec::new();
    if !system_prompt.trim().is_empty() {
        messages.push(ChatMessage::text(MessageRole::System, system_prompt.to_string()));
    }
    messages.push(ChatMessage::text(MessageRole::User, prompt.to_string()));
    let request = ChatRequest {
        model_id: model_id.clone(),
        messages,
        options,
    };
    let started = Instant::now();
    match crate::chat_with_routing(state, request).await {
        Ok(response) => ComparisonEntry {
            label,
            provider_id: response.provider,
            model_id,
            content: response.message.content,
            latency_ms: started.elapsed().as_millis() as u64,
            error: String::new(),
        },
        Err(error) => ComparisonEntry {
            label,
            provider_id: String::new(),
            model_id,
            content: String::new(),
            latency_ms: started.elapsed().as_millis() as u64,
            error: error.to_string(),
        },
    }
}

pub async fn compare_run(
    State(state): State<AppState>,
    Json(req): Json<CompareRunRequest>,
) -> ApiResult<Comparison> {
    if req.prompt.trim().is_empty() {
        return Err(bad_request("prompt cannot be empty"));
    }
    let models: Vec<String> = req
        .models
        .iter()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .collect();
    if models.len() < 2 {
        return Err(bad_request("choose at least two models to compare"));
    }

    let options = GenerationOptions {
        temperature: req.temperature,
        max_tokens: req.max_tokens,
        ..Default::default()
    };

    let futures = models.into_iter().enumerate().map(|(index, model_id)| {
        let state = &state;
        let system_prompt = req.system_prompt.clone();
        let prompt = req.prompt.clone();
        let options = options.clone();
        async move {
            run_one(state, label_for(index), model_id, &system_prompt, &prompt, options).await
        }
    });
    let entries = join_all(futures).await;

    let mut comparison = Comparison::new(req.prompt, req.system_prompt, req.blind);
    comparison.entries = entries;
    state.compare.save(&comparison).await.map_err(internal)?;
    Ok(Json(comparison))
}

pub async fn compare_history(
    State(state): State<AppState>,
    Query(query): Query<CompareHistoryQuery>,
) -> ApiResult<Vec<Comparison>> {
    let items = state
        .compare
        .list(query.limit.unwrap_or(100))
        .await
        .map_err(internal)?;
    Ok(Json(items))
}

pub async fn compare_get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Comparison> {
    let item = state
        .compare
        .get(&id)
        .await
        .map_err(internal)?
        .ok_or_else(|| not_found(format!("comparison {id} not found")))?;
    Ok(Json(item))
}

pub async fn compare_delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Value> {
    state.compare.delete(&id).await.map_err(internal)?;
    Ok(ok())
}

pub async fn compare_vote(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CompareVoteRequest>,
) -> ApiResult<Value> {
    if state.compare.get(&id).await.map_err(internal)?.is_none() {
        return Err(not_found(format!("comparison {id} not found")));
    }
    state
        .compare
        .set_vote(&id, &req.winner_label)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "ok": true, "winner_label": req.winner_label })))
}

pub async fn compare_synthesize(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CompareSynthesizeRequest>,
) -> ApiResult<Comparison> {
    let mut comparison = state
        .compare
        .get(&id)
        .await
        .map_err(internal)?
        .ok_or_else(|| not_found(format!("comparison {id} not found")))?;
    if req.model_id.trim().is_empty() {
        return Err(bad_request("model_id is required for synthesis"));
    }

    let mut prompt = format!(
        "You are an impartial judge. A user asked the following prompt:\n\n\"{}\"\n\n\
Below are {} anonymous answers. Compare them on correctness, clarity, and completeness, \
then state which label is best and briefly why. Be concise.\n\n",
        comparison.prompt,
        comparison.entries.len()
    );
    for entry in &comparison.entries {
        let body = if entry.error.is_empty() {
            entry.content.as_str()
        } else {
            "(no answer — provider error)"
        };
        prompt.push_str(&format!("### Answer {}\n{}\n\n", entry.label, body));
    }

    let request = ChatRequest {
        model_id: req.model_id,
        messages: vec![ChatMessage::text(MessageRole::User, prompt)],
        options: GenerationOptions::default(),
    };
    let response = crate::chat_with_routing(&state, request)
        .await
        .map_err(|error| ApiError::new(StatusCode::BAD_GATEWAY, error.to_string()))?;
    comparison.synthesis = response.message.content;
    state.compare.save(&comparison).await.map_err(internal)?;
    Ok(Json(comparison))
}
