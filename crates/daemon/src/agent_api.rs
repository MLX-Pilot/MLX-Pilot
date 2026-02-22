//! `/agent/*` endpoints — full agent runtime API.

use crate::secrets_vault::SecretsVault;
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
use std::path::{Path, PathBuf};
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
    pub integrity: String,
    pub sha256: Option<String>,
    pub capabilities: Vec<String>,
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

const AGENT_API_KEY_SECRET_REF: &str = "vault://agent.api_key";
const AGENT_API_KEY_SECRET_KEY: &str = "agent.api_key";
const SKILL_INTEGRITY_STATE_FILE: &str = "agent_skill_integrity_state.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SkillIntegrityState {
    #[serde(default)]
    hashes: BTreeMap<String, String>,
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

fn build_policy_config(
    cfg: &super::config::AgentUiConfig,
    mode: ExecutionMode,
    workspace_root: &Path,
    known_skill_hashes: BTreeMap<String, String>,
) -> PolicyConfig {
    let security_mode = cfg.security.security_mode.trim().to_ascii_lowercase();
    let paranoid_mode = security_mode == "paranoid";
    let enterprise_mode = paranoid_mode || security_mode == "enterprise";

    PolicyConfig {
        default_mode: mode,
        tool_allowlist: cfg.security.tool_allowlist.clone(),
        tool_denylist: cfg.security.tool_denylist.clone(),
        exec_safe_bins: cfg.security.exec_safe_bins.clone(),
        exec_deny_patterns: cfg.security.exec_deny_patterns.clone(),
        file_deny_paths: cfg.security.sensitive_paths.clone(),
        network_allow_domains: cfg.security.egress_allow_domains.clone(),
        block_direct_ip_egress: paranoid_mode || cfg.security.block_direct_ip_egress,
        airgapped_mode: paranoid_mode || cfg.security.airgapped,
        owner_only_mode: paranoid_mode || cfg.security.owner_only,
        workspace_root: Some(workspace_root.to_path_buf()),
        min_trust_level: if paranoid_mode {
            mlx_agent_skills::TrustLevel::Community
        } else if enterprise_mode {
            mlx_agent_skills::TrustLevel::Local
        } else {
            mlx_agent_skills::TrustLevel::Unknown
        },
        require_capabilities: enterprise_mode || cfg.security.require_capabilities,
        skill_sha256_pins: cfg.security.skill_sha256_pins.clone(),
        known_skill_hashes,
    }
}

fn settings_dir() -> PathBuf {
    let settings = super::config::AppConfig::get_settings_path();
    settings
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn skill_integrity_state_path() -> PathBuf {
    settings_dir().join(SKILL_INTEGRITY_STATE_FILE)
}

fn load_skill_integrity_state() -> BTreeMap<String, String> {
    let path = skill_integrity_state_path();
    if !path.exists() {
        return BTreeMap::new();
    }
    let Ok(raw) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    serde_json::from_str::<SkillIntegrityState>(&raw)
        .map(|state| state.hashes)
        .unwrap_or_default()
}

fn save_skill_integrity_state(hashes: BTreeMap<String, String>) {
    let path = skill_integrity_state_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let state = SkillIntegrityState { hashes };
    if let Ok(raw) = serde_json::to_string_pretty(&state) {
        let _ = std::fs::write(path, raw);
    }
}

fn open_secrets_vault() -> Result<SecretsVault, AgentApiError> {
    SecretsVault::open(&settings_dir()).map_err(|error| {
        AgentApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "secrets_vault_error",
            Some(error.to_string()),
        )
    })
}

fn resolve_agent_api_key(
    request_key: Option<String>,
    cfg: &super::config::AgentUiConfig,
) -> Result<String, AgentApiError> {
    if let Some(value) = request_key
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        return Ok(value);
    }

    if !cfg.api_key.trim().is_empty() {
        return Ok(cfg.api_key.trim().to_string());
    }

    if cfg.security.use_secrets_vault {
        if let Some(reference) = cfg.api_key_ref.as_deref().map(str::trim) {
            if !reference.is_empty() {
                let key = reference
                    .strip_prefix("vault://")
                    .unwrap_or(AGENT_API_KEY_SECRET_KEY);
                let vault = open_secrets_vault()?;
                if let Some(secret) = vault.get_secret(key).map_err(|error| {
                    AgentApiError::new(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "secrets_vault_error",
                        Some(error.to_string()),
                    )
                })? {
                    return Ok(secret);
                }
            }
        }
    }

    Ok(String::new())
}

fn is_local_base_url(url: &str) -> bool {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return false;
    }
    let without_scheme = if let Some(idx) = trimmed.find("://") {
        &trimmed[(idx + 3)..]
    } else {
        trimmed
    };
    let host = without_scheme
        .split('/')
        .next()
        .unwrap_or_default()
        .split('@')
        .next_back()
        .unwrap_or_default();
    let host = if let Some(stripped) = host.strip_prefix('[') {
        stripped.split(']').next().unwrap_or_default()
    } else {
        host.split(':').next().unwrap_or_default()
    }
    .to_ascii_lowercase();

    matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1")
}

fn provider_allowed_in_airgap(provider_id: &str, base_url: &str) -> bool {
    matches!(
        provider_id.trim().to_ascii_lowercase().as_str(),
        "mlx" | "llamacpp" | "ollama"
    ) || (provider_id.trim().eq_ignore_ascii_case("custom") && is_local_base_url(base_url))
}

fn canonical_or_normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| {
        let mut out = PathBuf::new();
        for component in path.components() {
            match component {
                std::path::Component::ParentDir => {
                    let _ = out.pop();
                }
                std::path::Component::CurDir => {}
                other => out.push(other.as_os_str()),
            }
        }
        out
    })
}

fn workspace_allowed_in_owner_mode(project_root: &Path, workspace: &Path) -> bool {
    let root = canonical_or_normalize_path(project_root);
    let candidate = canonical_or_normalize_path(workspace);
    candidate.starts_with(&root)
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

fn collect_skill_hashes(
    runtime: &mlx_agent_core::runtime::SkillRuntime,
) -> BTreeMap<String, String> {
    let mut hashes = BTreeMap::new();
    for skill in runtime.all() {
        if let Some(hash) = skill
            .sha256
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            hashes.insert(skill.name.clone(), hash.to_string());
        }
    }
    hashes
}

fn skill_capability_labels(skill: &mlx_agent_skills::SkillPackage) -> Vec<String> {
    let mut items = Vec::new();
    if skill.capabilities.allows_fs_read() {
        items.push("fs_read".to_string());
    }
    if skill.capabilities.allows_fs_write() {
        items.push("fs_write".to_string());
    }
    if skill.capabilities.allows_network() {
        items.push("network".to_string());
    }
    if skill.capabilities.allows_exec() {
        items.push("exec".to_string());
    }
    if skill.capabilities.allows_secrets_access() {
        items.push("secrets_access".to_string());
    }
    items
}

async fn evaluate_skill_integrity(
    runtime: &mut mlx_agent_core::runtime::SkillRuntime,
    policy: &(dyn PolicyEngine + Send + Sync),
    remove_denied: bool,
) -> BTreeMap<String, String> {
    let mut statuses = BTreeMap::new();
    let names = runtime.names();
    let mut denied = Vec::new();

    for name in names {
        let Some(skill) = runtime.get(&name) else {
            continue;
        };

        match policy.check_skill_load(skill).await {
            mlx_agent_core::policy::PolicyDecision::Allow => {
                statuses.insert(name, "ok".to_string());
            }
            mlx_agent_core::policy::PolicyDecision::Ask { prompt, .. } => {
                warn!(skill = %name, warning = %prompt, "skill integrity warning");
                statuses.insert(name, "changed".to_string());
            }
            mlx_agent_core::policy::PolicyDecision::Deny { reason } => {
                warn!(skill = %name, reason = %reason, "skill blocked by integrity policy");
                statuses.insert(name.clone(), "blocked".to_string());
                if remove_denied {
                    denied.push(name);
                }
            }
        }
    }

    if remove_denied {
        for name in denied {
            runtime.remove(&name);
        }
    }

    statuses
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

    let known_skill_hashes = load_skill_integrity_state();
    let policy_config = build_policy_config(agent_cfg, mode, &workspace, known_skill_hashes);
    let policy: Arc<dyn PolicyEngine> = Arc::new(DefaultPolicyEngine::new(policy_config));

    let enabled_tools = merged_vec(request.enabled_tools.clone(), &agent_cfg.enabled_tools);
    let tool_registry = build_tool_registry(&enabled_tools);

    if tool_registry.is_empty() {
        return Err(AgentApiError::bad_request("no tools enabled for agent run"));
    }

    let mut skill_runtime = mlx_agent_core::runtime::SkillRuntime::new();
    skill_runtime.load_from_workspace(&workspace).await;
    let _ = evaluate_skill_integrity(&mut skill_runtime, policy.as_ref(), true).await;
    save_skill_integrity_state(collect_skill_hashes(&skill_runtime));

    let enabled_skills = merged_vec(request.enabled_skills.clone(), &agent_cfg.enabled_skills);

    let config = AgentLoopConfig {
        model_id: resolved.model_id.clone(),
        workspace_root: workspace.clone(),
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
    let base_url = merged_value(request.base_url.clone(), &agent_cfg.base_url);
    let api_key = resolve_agent_api_key(request.api_key.clone(), &agent_cfg)?;
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

    let security_mode = agent_cfg.security.security_mode.trim().to_ascii_lowercase();
    let airgapped_mode = agent_cfg.security.airgapped || security_mode == "paranoid";
    let owner_only_mode = agent_cfg.security.owner_only || security_mode == "paranoid";

    if owner_only_mode
        && !workspace_allowed_in_owner_mode(&state.agent_state.default_workspace, &workspace)
    {
        return Err(AgentApiError::new(
            StatusCode::FORBIDDEN,
            "owner_only_block",
            Some(format!(
                "workspace '{}' is outside project root '{}'",
                workspace.display(),
                state.agent_state.default_workspace.display()
            )),
        ));
    }

    if airgapped_mode && !provider_allowed_in_airgap(&provider, &base_url) {
        return Err(AgentApiError::new(
            StatusCode::FORBIDDEN,
            "airgapped_block",
            Some(format!(
                "provider '{}' is blocked in airgapped mode; only local providers are allowed",
                provider
            )),
        ));
    }

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

    if airgapped_mode && !provider_allowed_in_airgap(&fallback_provider, &base_url) {
        return Err(AgentApiError::new(
            StatusCode::FORBIDDEN,
            "airgapped_block",
            Some(format!(
                "fallback provider '{}' is blocked in airgapped mode",
                fallback_provider
            )),
        ));
    }

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
    let mut cfg = super::config::AppConfig::load_settings().apply_env();
    if cfg.agent.security.use_secrets_vault && cfg.agent.api_key.trim().is_empty() {
        if cfg
            .agent
            .api_key_ref
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .is_some()
        {
            let vault = open_secrets_vault()?;
            if let Some(secret) = vault
                .get_secret(AGENT_API_KEY_SECRET_KEY)
                .map_err(|error| {
                    AgentApiError::new(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "secrets_vault_error",
                        Some(error.to_string()),
                    )
                })?
            {
                cfg.agent.api_key = secret;
            }
        }
    }
    Ok(Json(cfg.agent))
}

/// POST /agent/config
pub async fn agent_update_config(
    State(state): State<super::AppState>,
    Json(new_agent_cfg): Json<super::config::AgentUiConfig>,
) -> Result<Json<super::config::AgentUiConfig>, AgentApiError> {
    let mut cfg = super::config::AppConfig::load_settings().apply_env();
    let mut merged = new_agent_cfg.clone();

    if merged.security.use_secrets_vault {
        let vault = open_secrets_vault()?;
        if !merged.api_key.trim().is_empty() {
            vault
                .set_secret(AGENT_API_KEY_SECRET_KEY, merged.api_key.trim())
                .map_err(|error| {
                    AgentApiError::new(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "secrets_vault_error",
                        Some(error.to_string()),
                    )
                })?;
            merged.api_key.clear();
            merged.api_key_ref = Some(AGENT_API_KEY_SECRET_REF.to_string());
        } else if merged
            .api_key_ref
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .is_none()
        {
            let _ = vault.remove_secret(AGENT_API_KEY_SECRET_KEY);
            merged.api_key_ref = None;
        }
    } else {
        merged.api_key_ref = None;
    }

    cfg.agent = merged.clone();
    cfg.save_settings().map_err(|e| {
        AgentApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "save_failed",
            Some(e.to_string()),
        )
    })?;

    let approval_mode = parse_approval_mode(Some(cfg.agent.approval_mode.as_str()));
    state.agent_state.approval.set_mode(approval_mode);

    let mut response = merged;
    if response.security.use_secrets_vault && response.api_key.trim().is_empty() {
        if let Ok(vault) = open_secrets_vault() {
            if let Ok(Some(secret)) = vault.get_secret(AGENT_API_KEY_SECRET_KEY) {
                response.api_key = secret;
            }
        }
    }

    Ok(Json(response))
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
    let known_skill_hashes = load_skill_integrity_state();
    let mode = parse_execution_mode(Some(cfg.agent.execution_mode.as_str()));
    let policy_cfg = build_policy_config(&cfg.agent, mode, &workspace, known_skill_hashes);
    let policy = DefaultPolicyEngine::new(policy_cfg);
    let integrity = evaluate_skill_integrity(&mut runtime, &policy, false).await;
    save_skill_integrity_state(collect_skill_hashes(&runtime));

    let enabled = enabled_set(&cfg.agent.enabled_skills);
    let mut items = runtime
        .all()
        .map(|s| AgentSkillInfo {
            name: s.name.clone(),
            description: s.description.clone(),
            enabled: enabled.is_empty() || enabled.contains(&s.name.to_ascii_lowercase()),
            source: format!("{:?}", s.source).to_ascii_lowercase(),
            integrity: integrity
                .get(&s.name)
                .cloned()
                .unwrap_or_else(|| "unknown".to_string()),
            sha256: s.sha256.clone(),
            capabilities: skill_capability_labels(s),
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
    let workspace = cfg
        .agent
        .workspace_root
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let known_hashes = load_skill_integrity_state();
    let policy_cfg = build_policy_config(&cfg.agent, mode, &workspace, known_hashes);
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
