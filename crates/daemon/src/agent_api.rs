//! `/agent/*` endpoints — run the agent loop via the daemon API.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use mlx_agent_core::approval::{ApprovalDecision, ApprovalService};
use mlx_agent_core::audit::AuditLog;
use mlx_agent_core::events::EventBus;
use mlx_agent_core::policy::PolicyEngine;
use mlx_agent_core::registry::ToolRegistry;
use mlx_agent_core::{AgentError, AgentLoop, AgentLoopConfig};
use mlx_agent_tools::ExecutionMode;
use mlx_ollama_core::ModelProvider;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

// ── Request / Response types ─────────────────────────────────────

/// POST /agent/run request body.
#[derive(Debug, Deserialize)]
pub struct AgentRunRequest {
    /// Optional session ID — new UUID if omitted.
    #[serde(default)]
    #[allow(dead_code)]
    pub session_id: Option<String>,
    /// User message to send to the agent.
    pub message: String,
    /// Model ID to use (e.g. "qwen2.5:7b").
    #[serde(default)]
    pub model_id: Option<String>,
    /// Provider to use (ollama, mlx, llamacpp). Defaults to ollama.
    #[serde(default)]
    pub provider: Option<String>,
    /// Execution mode: "full" | "read_only" | "locked" | "dry_run".
    #[serde(default)]
    pub execution_mode: Option<String>,
    /// System prompt override.
    #[serde(default)]
    pub system_prompt: Option<String>,
    /// Max iterations (default 25).
    #[serde(default)]
    pub max_iterations: Option<usize>,
    /// Max tokens allocated to system + history + tools.
    #[serde(default)]
    pub max_prompt_tokens: Option<usize>,
    /// Max history messages kept in the sliding window.
    #[serde(default)]
    pub max_history_messages: Option<usize>,
    /// Max tools sent in a single prompt.
    #[serde(default)]
    pub max_tools_in_prompt: Option<usize>,
    /// Optional temperature override.
    #[serde(default)]
    pub temperature: Option<f32>,
    /// Restrict prompt tools to likely-relevant ones.
    #[serde(default)]
    pub aggressive_tool_filtering: Option<bool>,
    /// Enables one short fallback reprompt for tool-call JSON.
    #[serde(default)]
    pub enable_tool_call_fallback: Option<bool>,
    /// Workspace root override.
    #[serde(default)]
    pub workspace_root: Option<String>,
}

/// POST /agent/run response.
#[derive(Debug, Serialize)]
pub struct AgentRunResponse {
    pub session_id: String,
    pub audit_id: Option<String>,
    #[serde(rename = "final_response")]
    pub content: String,
    pub iterations: usize,
    pub tool_calls_made: usize,
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
    pub latency_ms: u64,
}

/// Error response from agent endpoints.
#[derive(Debug, Serialize)]
pub struct AgentApiError {
    error: String,
    details: Option<String>,
}

impl IntoResponse for AgentApiError {
    fn into_response(self) -> axum::response::Response {
        let status = StatusCode::INTERNAL_SERVER_ERROR;
        (status, Json(self)).into_response()
    }
}

/// POST /agent/approve request body.
#[derive(Debug, Deserialize)]
pub struct AgentApproveRequest {
    pub id: String,
    #[serde(flatten)]
    pub decision: ApprovalDecision,
}

// ── State types ──────────────────────────────────────────────────

/// Agent-specific state, held inside AppState.
#[derive(Clone)]
pub struct AgentState {
    pub default_model_id: String,
    pub default_workspace: PathBuf,
    pub policy: Arc<dyn PolicyEngine>,
    pub approval: Arc<dyn ApprovalService>,
    pub event_bus: Arc<EventBus>,
    pub audit: Arc<AuditLog>,
}

// ── Helpers ──────────────────────────────────────────────────────

fn parse_execution_mode(s: Option<&str>) -> ExecutionMode {
    match s.map(str::to_lowercase).as_deref() {
        Some("read_only") | Some("readonly") => ExecutionMode::ReadOnly,
        Some("locked") => ExecutionMode::Locked,
        Some("dry_run") | Some("dryrun") => ExecutionMode::DryRun,
        _ => ExecutionMode::Full,
    }
}

// ── Handlers ─────────────────────────────────────────────────────

/// POST /agent/run — run the agent loop and return the final response.
pub async fn agent_run(
    State(state): State<super::AppState>,
    Json(request): Json<AgentRunRequest>,
) -> Result<Json<AgentRunResponse>, AgentApiError> {
    let message = request.message.trim();
    if message.is_empty() {
        return Err(AgentApiError {
            error: "message cannot be empty".into(),
            details: None,
        });
    }

    // Resolve provider.
    let provider_name = request
        .provider
        .as_deref()
        .unwrap_or("ollama")
        .to_lowercase();

    let provider: Arc<dyn ModelProvider> = match provider_name.as_str() {
        "mlx" => state.mlx_provider.clone(),
        "llamacpp" | "llama" => state.llamacpp_provider.clone(),
        _ => state.ollama_provider.clone(),
    };

    let model_id = request
        .model_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&state.agent_state.default_model_id)
        .to_string();

    let workspace = request
        .workspace_root
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| state.agent_state.default_workspace.clone());

    let mode = parse_execution_mode(request.execution_mode.as_deref());

    let config = AgentLoopConfig {
        model_id: model_id.clone(),
        workspace_root: workspace.clone(),
        system_prompt: request.system_prompt.clone(),
        max_iterations: request.max_iterations.unwrap_or(25),
        max_prompt_tokens: request.max_prompt_tokens,
        max_history_messages: request.max_history_messages,
        max_tools_in_prompt: request.max_tools_in_prompt,
        max_tokens_per_turn: 4096,
        temperature: request.temperature,
        aggressive_tool_filtering: request.aggressive_tool_filtering.unwrap_or(false),
        enable_tool_call_fallback: request.enable_tool_call_fallback.unwrap_or(true),
        mode,
        skill_filter: None,
    };

    info!(
        model = %model_id,
        provider = %provider_name,
        mode = ?mode,
        "starting agent run"
    );

    let mut skill_runtime = mlx_agent_core::runtime::SkillRuntime::new();
    skill_runtime.load_from_workspace(&workspace).await;

    let mut agent = AgentLoop::new(
        config,
        provider,
        ToolRegistry::with_builtins(),
        skill_runtime,
        state.agent_state.policy.clone(),
        state.agent_state.approval.clone(),
        state.agent_state.event_bus.clone(),
        state.agent_state.audit.clone(),
    );

    let result = agent.run(message).await;

    match result {
        Ok(response) => {
            info!(
                session = %response.session_id,
                iterations = response.iterations,
                tool_calls = response.tool_calls_made,
                latency_ms = response.latency_ms,
                "agent run completed"
            );
            let audit_id = response.session_id.clone();
            Ok(Json(AgentRunResponse {
                session_id: response.session_id,
                audit_id: Some(audit_id),
                content: response.content,
                iterations: response.iterations,
                tool_calls_made: response.tool_calls_made,
                prompt_tokens: response.usage.prompt_tokens,
                completion_tokens: response.usage.completion_tokens,
                total_tokens: response.usage.total_tokens,
                latency_ms: response.latency_ms,
            }))
        }
        Err(err) => {
            let (error, details) = match &err {
                AgentError::MaxIterations { max } => (
                    "max_iterations_exceeded".into(),
                    Some(format!("agent exceeded {max} iterations")),
                ),
                AgentError::ProviderError { message } => {
                    ("provider_error".into(), Some(message.clone()))
                }
                AgentError::ToolError { tool, message } => (
                    "tool_error".into(),
                    Some(format!("tool '{tool}': {message}")),
                ),
                AgentError::PolicyDenied { reason } => {
                    ("policy_denied".into(), Some(reason.clone()))
                }
                AgentError::Other(e) => ("internal_error".into(), Some(e.to_string())),
            };

            Err(AgentApiError { error, details })
        }
    }
}

/// POST /agent/stream — streaming agent run (stub).
pub async fn agent_stream(
    State(_state): State<super::AppState>,
    Json(_request): Json<AgentRunRequest>,
) -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "not_implemented",
            "details": "agent streaming will be implemented in Phase 2"
        })),
    )
}

// ── Approval handler ─────────────────────────────────────────────

pub async fn agent_approve(
    State(state): State<super::AppState>,
    Json(payload): Json<AgentApproveRequest>,
) -> Result<Json<serde_json::Value>, AgentApiError> {
    state
        .agent_state
        .approval
        .resolve(&payload.id, payload.decision)
        .await
        .map_err(|e| AgentApiError {
            error: "Failed to resolve approval".into(),
            details: Some(e.to_string()),
        })?;

    Ok(Json(serde_json::json!({ "status": "ok" })))
}
