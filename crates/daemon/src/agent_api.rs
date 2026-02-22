//! `/agent/*` endpoints — full agent runtime API.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use http_llm_provider::{HttpApiKind, HttpLlmProvider, HttpLlmProviderConfig};
use mlx_agent_core::approval::{
    ApprovalDecision, ApprovalMode, ApprovalService, DefaultApprovalService,
};
use mlx_agent_core::audit::{AuditLog, AuditLogEntry};
use mlx_agent_core::events::EventBus;
use mlx_agent_core::policy::{DefaultPolicyEngine, PolicyConfig, PolicyEngine};
use mlx_agent_core::registry::ToolRegistry;
use mlx_agent_core::{AgentError, AgentLoop, AgentLoopConfig};
use mlx_agent_tools::ExecutionMode;
use mlx_ollama_core::{ModelProvider, RuntimeProviderConfig};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};

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
    /// Provider id.
    #[serde(default)]
    pub provider: Option<String>,
    /// Model ID.
    #[serde(default)]
    pub model_id: Option<String>,
    /// Optional per-request API key.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Optional per-request base URL.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Optional per-request headers.
    #[serde(default)]
    pub custom_headers: Option<BTreeMap<String, String>>,
    /// Enables streaming mode when supported.
    #[serde(default)]
    pub streaming: Option<bool>,
    /// Optional provider fallback toggle.
    #[serde(default)]
    pub fallback_enabled: Option<bool>,
    /// Optional fallback provider id.
    #[serde(default)]
    pub fallback_provider: Option<String>,
    /// Optional fallback model id.
    #[serde(default)]
    pub fallback_model_id: Option<String>,
    /// Execution mode: "full" | "read_only" | "locked" | "dry_run".
    #[serde(default)]
    pub execution_mode: Option<String>,
    /// Approval mode: auto | ask | deny.
    #[serde(default)]
    pub approval_mode: Option<String>,
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
    /// Optional enabled skills.
    #[serde(default)]
    pub enabled_skills: Option<Vec<String>>,
    /// Optional enabled tools.
    #[serde(default)]
    pub enabled_tools: Option<Vec<String>>,
    /// Workspace root override.
    #[serde(default)]
    pub workspace_root: Option<String>,
}

/// POST /agent/run response.
#[derive(Debug, Serialize)]
pub struct AgentRunResponse {
    pub session_id: String,
    pub audit_id: Option<String>,
    pub provider: String,
    pub model_id: String,
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
    #[serde(skip)]
    status: StatusCode,
}

impl AgentApiError {
    fn new(status: StatusCode, error: impl Into<String>, details: Option<String>) -> Self {
        Self {
            error: error.into(),
            details,
            status,
        }
    }

    fn bad_request(error: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, error, None)
    }

    fn from_agent_error(err: AgentError) -> Self {
        match err {
            AgentError::MaxIterations { max } => Self::new(
                StatusCode::BAD_REQUEST,
                "max_iterations_exceeded",
                Some(format!("agent exceeded {max} iterations")),
            ),
            AgentError::ProviderError { message } => {
                Self::new(StatusCode::BAD_GATEWAY, "provider_error", Some(message))
            }
            AgentError::ToolError { tool, message } => Self::new(
                StatusCode::BAD_REQUEST,
                "tool_error",
                Some(format!("tool '{tool}': {message}")),
            ),
            AgentError::PolicyDenied { reason } => {
                Self::new(StatusCode::FORBIDDEN, "policy_denied", Some(reason))
            }
            AgentError::Other(error) => Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                Some(error.to_string()),
            ),
        }
    }
}

impl IntoResponse for AgentApiError {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(self)).into_response()
    }
}

/// POST /agent/approve request body.
#[derive(Debug, Deserialize)]
pub struct AgentApproveRequest {
    pub id: String,
    #[serde(flatten)]
    pub decision: ApprovalDecision,
}

#[derive(Debug, Serialize)]
pub struct AgentProviderInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub requires_api_key: bool,
    pub supports_tool_calling: bool,
    pub supports_streaming: bool,
    pub default_base_url: Option<String>,
    pub models: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AgentSkillInfo {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub source: String,
}

#[derive(Debug, Serialize)]
pub struct AgentToolInfo {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub policy: String,
}

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct AgentAuditResponse {
    pub entries: Vec<AuditLogEntry>,
}

// ── State types ──────────────────────────────────────────────────

/// Agent-specific state, held inside AppState.
#[derive(Clone)]
pub struct AgentState {
    pub default_workspace: PathBuf,
    pub approval: Arc<DefaultApprovalService>,
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

fn parse_approval_mode(s: Option<&str>) -> ApprovalMode {
    match s.map(str::to_ascii_lowercase).as_deref() {
        Some("auto") => ApprovalMode::Auto,
        Some("deny") => ApprovalMode::Deny,
        _ => ApprovalMode::Ask,
    }
}

fn enabled_set(values: &[String]) -> HashSet<String> {
    values
        .iter()
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty())
        .collect()
}

fn merged_value(primary: Option<String>, fallback: &str) -> String {
    primary
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn merged_vec(primary: Option<Vec<String>>, fallback: &[String]) -> Vec<String> {
    primary
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| fallback.to_vec())
}

fn build_policy_config(cfg: &super::config::AgentUiConfig, mode: ExecutionMode) -> PolicyConfig {
    PolicyConfig {
        default_mode: mode,
        tool_allowlist: cfg.security.tool_allowlist.clone(),
        tool_denylist: cfg.security.tool_denylist.clone(),
        exec_safe_bins: cfg.security.exec_safe_bins.clone(),
        exec_deny_patterns: cfg.security.exec_deny_patterns.clone(),
        file_deny_paths: cfg.security.sensitive_paths.clone(),
        network_allow_domains: cfg.security.egress_allow_domains.clone(),
        min_trust_level: mlx_agent_skills::TrustLevel::Unknown,
        require_capabilities: false,
    }
}

fn configured_runtime(
    api_key: &str,
    base_url: &str,
    headers: &BTreeMap<String, String>,
) -> Option<RuntimeProviderConfig> {
    if api_key.trim().is_empty() && base_url.trim().is_empty() && headers.is_empty() {
        return None;
    }

    Some(RuntimeProviderConfig {
        base_url: if base_url.trim().is_empty() {
            None
        } else {
            Some(base_url.trim().to_string())
        },
        api_key: if api_key.trim().is_empty() {
            None
        } else {
            Some(api_key.trim().to_string())
        },
        headers: headers.clone(),
    })
}

#[derive(Clone)]
struct ResolvedProvider {
    provider_name: String,
    model_id: String,
    provider: Arc<dyn ModelProvider>,
    runtime: Option<RuntimeProviderConfig>,
}

#[derive(Debug, Clone, Default)]
pub struct AgentProviderRegistry;

impl AgentProviderRegistry {
    fn resolve(
        &self,
        state: &super::AppState,
        provider: &str,
        model_id: &str,
        api_key: &str,
        base_url: &str,
        headers: &BTreeMap<String, String>,
    ) -> Result<ResolvedProvider, AgentApiError> {
        resolve_provider(state, provider, model_id, api_key, base_url, headers)
    }
}

fn resolve_provider(
    state: &super::AppState,
    provider: &str,
    model_id: &str,
    api_key: &str,
    base_url: &str,
    headers: &BTreeMap<String, String>,
) -> Result<ResolvedProvider, AgentApiError> {
    let provider_id = provider.trim().to_ascii_lowercase();
    let model = model_id.trim();
    if model.is_empty() {
        return Err(AgentApiError::bad_request("model_id cannot be empty"));
    }

    let runtime = configured_runtime(api_key, base_url, headers);

    match provider_id.as_str() {
        "mlx" => Ok(ResolvedProvider {
            provider_name: "mlx".to_string(),
            model_id: model.to_string(),
            provider: state.mlx_provider.clone(),
            runtime: None,
        }),
        "llamacpp" | "llama" | "llama.cpp" => Ok(ResolvedProvider {
            provider_name: "llamacpp".to_string(),
            model_id: model.to_string(),
            provider: state.llamacpp_provider.clone(),
            runtime: None,
        }),
        "ollama" => Ok(ResolvedProvider {
            provider_name: "ollama".to_string(),
            model_id: model.to_string(),
            provider: state.ollama_provider.clone(),
            runtime: None,
        }),
        "anthropic" => {
            let provider = HttpLlmProvider::new(HttpLlmProviderConfig {
                provider_name: "anthropic".to_string(),
                api_kind: HttpApiKind::Anthropic,
                base_url: "https://api.anthropic.com/v1".to_string(),
                api_key: None,
                default_headers: BTreeMap::new(),
                timeout: std::time::Duration::from_secs(120),
                default_models: vec![
                    "claude-3-5-sonnet-latest".to_string(),
                    "claude-3-7-sonnet-latest".to_string(),
                ],
            });

            Ok(ResolvedProvider {
                provider_name: "anthropic".to_string(),
                model_id: model.to_string(),
                provider: Arc::new(provider),
                runtime,
            })
        }
        "openai" | "openai_compat" => {
            let provider = HttpLlmProvider::new(HttpLlmProviderConfig {
                provider_name: "openai".to_string(),
                api_kind: HttpApiKind::OpenAiCompatible,
                base_url: "https://api.openai.com/v1".to_string(),
                api_key: None,
                default_headers: BTreeMap::new(),
                timeout: std::time::Duration::from_secs(120),
                default_models: vec!["gpt-4o-mini".to_string(), "gpt-4.1-mini".to_string()],
            });

            Ok(ResolvedProvider {
                provider_name: "openai".to_string(),
                model_id: model.to_string(),
                provider: Arc::new(provider),
                runtime,
            })
        }
        "groq" => {
            let provider = HttpLlmProvider::new(HttpLlmProviderConfig {
                provider_name: "groq".to_string(),
                api_kind: HttpApiKind::OpenAiCompatible,
                base_url: "https://api.groq.com/openai/v1".to_string(),
                api_key: None,
                default_headers: BTreeMap::new(),
                timeout: std::time::Duration::from_secs(120),
                default_models: vec![
                    "llama-3.3-70b-versatile".to_string(),
                    "qwen-qwq-32b".to_string(),
                ],
            });

            Ok(ResolvedProvider {
                provider_name: "groq".to_string(),
                model_id: model.to_string(),
                provider: Arc::new(provider),
                runtime,
            })
        }
        "openrouter" => {
            let mut default_headers = BTreeMap::new();
            default_headers.insert(
                "HTTP-Referer".to_string(),
                "https://mlx-pilot.local".to_string(),
            );
            default_headers.insert("X-Title".to_string(), "MLX-Pilot Agent".to_string());

            let provider = HttpLlmProvider::new(HttpLlmProviderConfig {
                provider_name: "openrouter".to_string(),
                api_kind: HttpApiKind::OpenAiCompatible,
                base_url: "https://openrouter.ai/api/v1".to_string(),
                api_key: None,
                default_headers,
                timeout: std::time::Duration::from_secs(120),
                default_models: vec![
                    "openai/gpt-4o-mini".to_string(),
                    "anthropic/claude-3.5-sonnet".to_string(),
                ],
            });

            Ok(ResolvedProvider {
                provider_name: "openrouter".to_string(),
                model_id: model.to_string(),
                provider: Arc::new(provider),
                runtime,
            })
        }
        "deepseek" => {
            let provider = HttpLlmProvider::new(HttpLlmProviderConfig {
                provider_name: "deepseek".to_string(),
                api_kind: HttpApiKind::OpenAiCompatible,
                base_url: "https://api.deepseek.com/v1".to_string(),
                api_key: None,
                default_headers: BTreeMap::new(),
                timeout: std::time::Duration::from_secs(120),
                default_models: vec!["deepseek-chat".to_string(), "deepseek-reasoner".to_string()],
            });

            Ok(ResolvedProvider {
                provider_name: "deepseek".to_string(),
                model_id: model.to_string(),
                provider: Arc::new(provider),
                runtime,
            })
        }
        "custom" => {
            let provider = HttpLlmProvider::new(HttpLlmProviderConfig {
                provider_name: "custom".to_string(),
                api_kind: HttpApiKind::OpenAiCompatible,
                base_url: if base_url.trim().is_empty() {
                    "https://api.openai.com/v1".to_string()
                } else {
                    base_url.trim().to_string()
                },
                api_key: None,
                default_headers: BTreeMap::new(),
                timeout: std::time::Duration::from_secs(120),
                default_models: vec![model.to_string()],
            });

            Ok(ResolvedProvider {
                provider_name: "custom".to_string(),
                model_id: model.to_string(),
                provider: Arc::new(provider),
                runtime,
            })
        }
        _ => Err(AgentApiError::bad_request(format!(
            "unknown provider '{provider}'"
        ))),
    }
}

fn build_tool_registry(enabled_tools: &[String]) -> ToolRegistry {
    use mlx_agent_tools::{EditFileTool, ExecTool, ListDirTool, ReadFileTool, WriteFileTool};

    let enabled = enabled_set(enabled_tools);
    let mut registry = ToolRegistry::new();

    if enabled.is_empty() || enabled.contains("read_file") {
        registry.register(Arc::new(ReadFileTool::new()));
    }
    if enabled.is_empty() || enabled.contains("write_file") {
        registry.register(Arc::new(WriteFileTool::new()));
    }
    if enabled.is_empty() || enabled.contains("edit_file") {
        registry.register(Arc::new(EditFileTool::new()));
    }
    if enabled.is_empty() || enabled.contains("list_dir") {
        registry.register(Arc::new(ListDirTool::new()));
    }
    if enabled.is_empty() || enabled.contains("exec") {
        registry.register(Arc::new(ExecTool::new()));
    }

    registry
}

async fn run_agent_once(
    state: &super::AppState,
    agent_cfg: &super::config::AgentUiConfig,
    request: &AgentRunRequest,
    resolved: ResolvedProvider,
    workspace: PathBuf,
) -> Result<AgentRunResponse, AgentApiError> {
    let mode = parse_execution_mode(
        request
            .execution_mode
            .as_deref()
            .or(Some(agent_cfg.execution_mode.as_str())),
    );

    let approval_mode = parse_approval_mode(
        request
            .approval_mode
            .as_deref()
            .or(Some(agent_cfg.approval_mode.as_str())),
    );
    state.agent_state.approval.set_mode(approval_mode);

    let policy_config = build_policy_config(agent_cfg, mode);
    let policy: Arc<dyn PolicyEngine> = Arc::new(DefaultPolicyEngine::new(policy_config));

    let enabled_tools = merged_vec(request.enabled_tools.clone(), &agent_cfg.enabled_tools);
    let tool_registry = build_tool_registry(&enabled_tools);

    if tool_registry.is_empty() {
        return Err(AgentApiError::bad_request("no tools enabled for agent run"));
    }

    let mut skill_runtime = mlx_agent_core::runtime::SkillRuntime::new();
    skill_runtime.load_from_workspace(&workspace).await;

    let enabled_skills = merged_vec(request.enabled_skills.clone(), &agent_cfg.enabled_skills);

    let config = AgentLoopConfig {
        model_id: resolved.model_id.clone(),
        workspace_root: workspace,
        system_prompt: request.system_prompt.clone(),
        max_iterations: request.max_iterations.unwrap_or(25),
        max_prompt_tokens: request.max_prompt_tokens.or(agent_cfg.max_prompt_tokens),
        max_history_messages: request
            .max_history_messages
            .or(agent_cfg.max_history_messages),
        max_tools_in_prompt: request
            .max_tools_in_prompt
            .or(agent_cfg.max_tools_in_prompt),
        provider_runtime: resolved.runtime.clone(),
        max_tokens_per_turn: 4096,
        temperature: request.temperature.or(agent_cfg.temperature),
        aggressive_tool_filtering: request
            .aggressive_tool_filtering
            .unwrap_or(agent_cfg.aggressive_tool_filtering),
        enable_tool_call_fallback: request
            .enable_tool_call_fallback
            .unwrap_or(agent_cfg.enable_tool_call_fallback),
        mode,
        skill_filter: if enabled_skills.is_empty() {
            None
        } else {
            Some(enabled_skills)
        },
    };

    info!(
        provider = %resolved.provider_name,
        model = %resolved.model_id,
        mode = ?mode,
        approval = ?approval_mode,
        "starting agent run"
    );

    let mut agent = AgentLoop::new(
        config,
        resolved.provider,
        tool_registry,
        skill_runtime,
        policy,
        state.agent_state.approval.clone(),
        state.agent_state.event_bus.clone(),
        state.agent_state.audit.clone(),
    );

    let response = agent
        .run(request.message.trim())
        .await
        .map_err(AgentApiError::from_agent_error)?;

    Ok(AgentRunResponse {
        provider: resolved.provider_name,
        model_id: resolved.model_id,
        session_id: response.session_id.clone(),
        audit_id: Some(response.session_id),
        content: response.content,
        iterations: response.iterations,
        tool_calls_made: response.tool_calls_made,
        prompt_tokens: response.usage.prompt_tokens,
        completion_tokens: response.usage.completion_tokens,
        total_tokens: response.usage.total_tokens,
        latency_ms: response.latency_ms,
    })
}

// ── Handlers ─────────────────────────────────────────────────────

/// POST /agent/run — run the agent loop and return the final response.
pub async fn agent_run(
    State(state): State<super::AppState>,
    Json(request): Json<AgentRunRequest>,
) -> Result<Json<AgentRunResponse>, AgentApiError> {
    let message = request.message.trim();
    if message.is_empty() {
        return Err(AgentApiError::bad_request("message cannot be empty"));
    }

    let cfg = super::config::AppConfig::load_settings().apply_env();
    let agent_cfg = cfg.agent.clone();

    let provider = merged_value(request.provider.clone(), &agent_cfg.provider);
    let model_id = merged_value(request.model_id.clone(), &agent_cfg.model_id);
    let api_key = merged_value(request.api_key.clone(), &agent_cfg.api_key);
    let base_url = merged_value(request.base_url.clone(), &agent_cfg.base_url);
    let streaming_enabled = request.streaming.unwrap_or(agent_cfg.streaming);
    let headers = request
        .custom_headers
        .clone()
        .unwrap_or_else(|| agent_cfg.custom_headers.clone());

    let workspace = request
        .workspace_root
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            agent_cfg
                .workspace_root
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| state.agent_state.default_workspace.clone());

    let registry = AgentProviderRegistry;
    let primary = registry.resolve(&state, &provider, &model_id, &api_key, &base_url, &headers)?;

    let fallback_enabled = request
        .fallback_enabled
        .unwrap_or(agent_cfg.fallback_enabled);

    info!(
        provider = %provider,
        model = %model_id,
        streaming = streaming_enabled,
        fallback = fallback_enabled,
        "agent run request received"
    );

    let fallback_provider = merged_value(
        request.fallback_provider.clone(),
        &agent_cfg.fallback_provider,
    );
    let fallback_model = merged_value(
        request.fallback_model_id.clone(),
        if agent_cfg.fallback_model_id.trim().is_empty() {
            &agent_cfg.model_id
        } else {
            &agent_cfg.fallback_model_id
        },
    );

    let primary_result =
        run_agent_once(&state, &agent_cfg, &request, primary, workspace.clone()).await;
    match primary_result {
        Ok(response) => Ok(Json(response)),
        Err(err) if !fallback_enabled || err.error != "provider_error" => Err(err),
        Err(err) => {
            warn!("primary provider failed, trying fallback: {}", err.error);
            let fallback = registry.resolve(
                &state,
                &fallback_provider,
                &fallback_model,
                &api_key,
                &base_url,
                &headers,
            )?;

            let fallback_result =
                run_agent_once(&state, &agent_cfg, &request, fallback, workspace).await;
            fallback_result.map(Json).map_err(|fallback_err| {
                AgentApiError::new(
                    StatusCode::BAD_GATEWAY,
                    "provider_error",
                    Some(format!(
                        "primary failed: {}. fallback failed: {}",
                        err.details.unwrap_or(err.error),
                        fallback_err.details.unwrap_or(fallback_err.error)
                    )),
                )
            })
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
            "details": "agent streaming will be implemented on top of the EventBus stream"
        })),
    )
}

/// GET /agent/providers
pub async fn agent_providers(
    State(state): State<super::AppState>,
) -> Result<Json<Vec<AgentProviderInfo>>, AgentApiError> {
    let mut providers = vec![
        AgentProviderInfo {
            id: "mlx".to_string(),
            name: "MLX".to_string(),
            kind: "local".to_string(),
            requires_api_key: false,
            supports_tool_calling: false,
            supports_streaming: false,
            default_base_url: None,
            models: state
                .mlx_provider
                .list_models()
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|m| m.id)
                .collect(),
        },
        AgentProviderInfo {
            id: "llamacpp".to_string(),
            name: "llama.cpp".to_string(),
            kind: "local".to_string(),
            requires_api_key: false,
            supports_tool_calling: false,
            supports_streaming: false,
            default_base_url: None,
            models: state
                .llamacpp_provider
                .list_models()
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|m| m.id)
                .collect(),
        },
        AgentProviderInfo {
            id: "ollama".to_string(),
            name: "Ollama".to_string(),
            kind: "local".to_string(),
            requires_api_key: false,
            supports_tool_calling: true,
            supports_streaming: false,
            default_base_url: None,
            models: state
                .ollama_provider
                .list_models()
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|m| m.id)
                .collect(),
        },
        AgentProviderInfo {
            id: "openai".to_string(),
            name: "OpenAI-compatible".to_string(),
            kind: "remote".to_string(),
            requires_api_key: true,
            supports_tool_calling: true,
            supports_streaming: false,
            default_base_url: Some("https://api.openai.com/v1".to_string()),
            models: vec!["gpt-4o-mini".to_string(), "gpt-4.1-mini".to_string()],
        },
        AgentProviderInfo {
            id: "anthropic".to_string(),
            name: "Anthropic".to_string(),
            kind: "remote".to_string(),
            requires_api_key: true,
            supports_tool_calling: true,
            supports_streaming: false,
            default_base_url: Some("https://api.anthropic.com/v1".to_string()),
            models: vec![
                "claude-3-5-sonnet-latest".to_string(),
                "claude-3-7-sonnet-latest".to_string(),
            ],
        },
        AgentProviderInfo {
            id: "groq".to_string(),
            name: "Groq".to_string(),
            kind: "remote".to_string(),
            requires_api_key: true,
            supports_tool_calling: true,
            supports_streaming: false,
            default_base_url: Some("https://api.groq.com/openai/v1".to_string()),
            models: vec![
                "llama-3.3-70b-versatile".to_string(),
                "qwen-qwq-32b".to_string(),
            ],
        },
        AgentProviderInfo {
            id: "openrouter".to_string(),
            name: "OpenRouter".to_string(),
            kind: "remote".to_string(),
            requires_api_key: true,
            supports_tool_calling: true,
            supports_streaming: false,
            default_base_url: Some("https://openrouter.ai/api/v1".to_string()),
            models: vec![
                "openai/gpt-4o-mini".to_string(),
                "anthropic/claude-3.5-sonnet".to_string(),
            ],
        },
        AgentProviderInfo {
            id: "deepseek".to_string(),
            name: "DeepSeek".to_string(),
            kind: "remote".to_string(),
            requires_api_key: true,
            supports_tool_calling: true,
            supports_streaming: false,
            default_base_url: Some("https://api.deepseek.com/v1".to_string()),
            models: vec!["deepseek-chat".to_string(), "deepseek-reasoner".to_string()],
        },
        AgentProviderInfo {
            id: "custom".to_string(),
            name: "Custom Endpoint".to_string(),
            kind: "remote".to_string(),
            requires_api_key: false,
            supports_tool_calling: true,
            supports_streaming: false,
            default_base_url: None,
            models: vec![],
        },
    ];

    providers.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Json(providers))
}

/// GET /agent/config
pub async fn agent_get_config() -> Result<Json<super::config::AgentUiConfig>, AgentApiError> {
    let cfg = super::config::AppConfig::load_settings().apply_env();
    Ok(Json(cfg.agent))
}

/// POST /agent/config
pub async fn agent_update_config(
    State(state): State<super::AppState>,
    Json(new_agent_cfg): Json<super::config::AgentUiConfig>,
) -> Result<Json<super::config::AgentUiConfig>, AgentApiError> {
    let mut cfg = super::config::AppConfig::load_settings().apply_env();
    cfg.agent = new_agent_cfg.clone();
    cfg.save_settings().map_err(|e| {
        AgentApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "save_failed",
            Some(e.to_string()),
        )
    })?;

    let approval_mode = parse_approval_mode(Some(cfg.agent.approval_mode.as_str()));
    state.agent_state.approval.set_mode(approval_mode);

    Ok(Json(new_agent_cfg))
}

/// GET /agent/skills
pub async fn agent_list_skills(
    State(state): State<super::AppState>,
) -> Result<Json<Vec<AgentSkillInfo>>, AgentApiError> {
    let cfg = super::config::AppConfig::load_settings().apply_env();
    let workspace = cfg
        .agent
        .workspace_root
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| state.agent_state.default_workspace.clone());

    let mut runtime = mlx_agent_core::runtime::SkillRuntime::new();
    runtime.load_from_workspace(&workspace).await;

    let enabled = enabled_set(&cfg.agent.enabled_skills);
    let mut items = runtime
        .all()
        .map(|s| AgentSkillInfo {
            name: s.name.clone(),
            description: s.description.clone(),
            enabled: enabled.is_empty() || enabled.contains(&s.name.to_ascii_lowercase()),
            source: format!("{:?}", s.source).to_ascii_lowercase(),
        })
        .collect::<Vec<_>>();

    items.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Json(items))
}

/// POST /agent/skills/reload
pub async fn agent_reload_skills(
    State(state): State<super::AppState>,
) -> Result<Json<Vec<AgentSkillInfo>>, AgentApiError> {
    agent_list_skills(State(state)).await
}

/// GET /agent/tools
pub async fn agent_list_tools() -> Result<Json<Vec<AgentToolInfo>>, AgentApiError> {
    let cfg = super::config::AppConfig::load_settings().apply_env();
    let mode = parse_execution_mode(Some(cfg.agent.execution_mode.as_str()));
    let policy_cfg = build_policy_config(&cfg.agent, mode);
    let policy = DefaultPolicyEngine::new(policy_cfg);

    let enabled = enabled_set(&cfg.agent.enabled_tools);

    let mut tools = Vec::new();
    for t in ToolRegistry::with_builtins().definitions() {
        let name = t.name.clone();
        let active = enabled.is_empty() || enabled.contains(&name.to_ascii_lowercase());
        let decision = policy
            .check_tool_call(&name, &serde_json::json!({}), None, mode)
            .await;
        let policy_status = match decision {
            mlx_agent_core::policy::PolicyDecision::Allow => "allow",
            mlx_agent_core::policy::PolicyDecision::Ask { .. } => "ask",
            mlx_agent_core::policy::PolicyDecision::Deny { .. } => "deny",
        }
        .to_string();

        tools.push(AgentToolInfo {
            name,
            description: t.description,
            enabled: active,
            policy: policy_status,
        });
    }

    tools.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Json(tools))
}

/// GET /agent/audit
pub async fn agent_audit(
    State(state): State<super::AppState>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<AgentAuditResponse>, AgentApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    let mut entries =
        read_recent_audit_entries(&state.agent_state.audit.log_dir, limit).map_err(|e| {
            AgentApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "audit_read_failed",
                Some(e.to_string()),
            )
        })?;

    entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    entries.truncate(limit);

    Ok(Json(AgentAuditResponse { entries }))
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
        .map_err(|e| {
            AgentApiError::new(
                StatusCode::NOT_FOUND,
                "approval_not_found",
                Some(e.to_string()),
            )
        })?;

    Ok(Json(serde_json::json!({ "status": "ok" })))
}

fn read_recent_audit_entries(
    log_dir: &std::path::Path,
    limit: usize,
) -> Result<Vec<AuditLogEntry>, std::io::Error> {
    if !log_dir.exists() {
        return Ok(Vec::new());
    }

    let mut files = std::fs::read_dir(log_dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().map(|e| e == "jsonl").unwrap_or(false))
        .collect::<Vec<_>>();
    files.sort();

    let mut entries = Vec::new();
    for path in files.into_iter().rev() {
        let content = std::fs::read_to_string(path)?;
        for line in content.lines().rev() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<AuditLogEntry>(line) {
                entries.push(entry);
                if entries.len() >= limit {
                    return Ok(entries);
                }
            }
        }
    }

    Ok(entries)
}
