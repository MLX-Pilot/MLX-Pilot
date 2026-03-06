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
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_budget: Option<mlx_agent_core::ContextBudgetTelemetry>,
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
    pub active: bool,
    pub eligible: bool,
    pub source: String,
    pub bundled: bool,
    pub integrity: String,
    pub sha256: Option<String>,
    pub capabilities: Vec<String>,
    pub missing: Vec<String>,
    pub install_options: Vec<AgentSkillInstallOption>,
    pub primary_env: Option<String>,
    pub configured_env: Vec<String>,
    pub configured_config: Vec<String>,
    pub os: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AgentSkillInstallOption {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub bins: Vec<String>,
    pub os: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AgentSkillsCheckSummary {
    pub total: usize,
    pub eligible: usize,
    pub active: usize,
    pub missing_dependencies: usize,
    pub missing_configuration: usize,
    pub configure_now: bool,
    pub installable: usize,
    pub node_manager: String,
}

#[derive(Debug, Serialize)]
pub struct AgentSkillsCheckResponse {
    pub summary: AgentSkillsCheckSummary,
    pub skills: Vec<AgentSkillInfo>,
}

#[derive(Debug, Deserialize)]
pub struct AgentSkillsInstallRequest {
    #[serde(default)]
    pub skill: Option<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub install_ids: Vec<String>,
    #[serde(default)]
    pub node_manager: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AgentSkillInstallExecution {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub ok: bool,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AgentSkillInstallResult {
    pub skill: String,
    pub installs: Vec<AgentSkillInstallExecution>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AgentSkillsInstallResponse {
    pub node_manager: String,
    pub results: Vec<AgentSkillInstallResult>,
}

#[derive(Debug, Deserialize)]
pub struct AgentSkillToggleRequest {
    #[serde(default)]
    pub skill: Option<String>,
    #[serde(default)]
    pub skills: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct AgentSkillConfigRequest {
    pub skill: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub clear_env: Vec<String>,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
    #[serde(default)]
    pub clear_config: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AgentToolInfo {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub policy: String,
}

#[derive(Debug, Deserialize)]
pub struct ToolPolicyQuery {
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AgentToolProfileRequest {
    pub profile: String,
}

#[derive(Debug, Deserialize)]
pub struct AgentToolAllowDenyRequest {
    pub scope: String,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
    #[serde(default)]
    pub replace: bool,
}

#[derive(Debug, Deserialize)]
pub struct ContextBudgetQuery {
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    #[serde(default)]
    pub limit: Option<usize>,
    pub since: Option<String>,
    pub session_id: Option<String>,
    pub event_type: Option<String>,
    pub tool_name: Option<String>,
    pub status: Option<String>,
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
    pub memory: Arc<mlx_agent_core::MemoryStore>,
    pub budget_tracker:
        Arc<tokio::sync::RwLock<BTreeMap<String, mlx_agent_core::ContextBudgetTelemetry>>>,
}

const AGENT_API_KEY_SECRET_REF: &str = "vault://agent.api_key";
const AGENT_API_KEY_SECRET_KEY: &str = "agent.api_key";
const SKILL_INTEGRITY_STATE_FILE: &str = "agent_skill_integrity_state.json";
const INSTALL_COMMAND_TIMEOUT_SECS_DEFAULT: u64 = 180;
const INSTALL_DOWNLOAD_TIMEOUT_SECS_DEFAULT: u64 = 60;
const DEFAULT_AGENT_ID: &str = "default";

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

fn normalize_scope_key(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn parse_tool_profile(value: Option<&str>) -> mlx_agent_core::ToolProfileName {
    value
        .unwrap_or("coding")
        .parse::<mlx_agent_core::ToolProfileName>()
        .unwrap_or_default()
}

fn to_rule_set(
    override_cfg: &crate::config::AgentToolScopeOverride,
) -> mlx_agent_core::ToolRuleSet {
    mlx_agent_core::ToolRuleSet {
        allow: override_cfg.allow.clone(),
        deny: override_cfg.deny.clone(),
    }
}

fn build_tool_policy_state(
    agent_cfg: &super::config::AgentUiConfig,
    session_id: Option<&str>,
    request_enabled_tools: Option<&[String]>,
) -> mlx_agent_core::ToolPolicyState {
    let mut agents = agent_cfg
        .tool_policy
        .agent_overrides
        .iter()
        .map(|(key, value)| (normalize_scope_key(key), to_rule_set(value)))
        .collect::<BTreeMap<_, _>>();

    if agents.is_empty() && !agent_cfg.enabled_tools.is_empty() {
        agents.insert(
            DEFAULT_AGENT_ID.to_string(),
            mlx_agent_core::ToolRuleSet {
                allow: agent_cfg.enabled_tools.clone(),
                deny: Vec::new(),
            },
        );
    }

    let mut sessions = agent_cfg
        .tool_policy
        .session_overrides
        .iter()
        .map(|(key, value)| (normalize_scope_key(key), to_rule_set(value)))
        .collect::<BTreeMap<_, _>>();

    if let Some(enabled_tools) = request_enabled_tools.filter(|value| !value.is_empty()) {
        if let Some(session) = session_id.map(normalize_scope_key) {
            sessions.insert(
                session,
                mlx_agent_core::ToolRuleSet {
                    allow: enabled_tools.to_vec(),
                    deny: Vec::new(),
                },
            );
        }
    }

    mlx_agent_core::ToolPolicyState {
        profile: parse_tool_profile(Some(agent_cfg.tool_policy.profile.as_str())),
        global: mlx_agent_core::ToolRuleSet {
            allow: agent_cfg.security.tool_allowlist.clone(),
            deny: agent_cfg.security.tool_denylist.clone(),
        },
        agents,
        sessions,
    }
}

fn sync_legacy_enabled_tools(agent_cfg: &mut super::config::AgentUiConfig) {
    let effective = mlx_agent_core::resolve_effective_tool_policy(
        &build_tool_policy_state(agent_cfg, None, None),
        DEFAULT_AGENT_ID,
        None,
    );
    agent_cfg.enabled_tools = effective
        .entries
        .into_iter()
        .filter(|entry| entry.allowed && entry.implemented)
        .map(|entry| entry.name)
        .collect();
}

fn session_messages_to_chat_history(
    messages: &[mlx_agent_core::SessionMessage],
) -> Vec<mlx_ollama_core::ChatMessage> {
    messages
        .iter()
        .map(|message| {
            let role = match message.role.trim().to_ascii_lowercase().as_str() {
                "system" => mlx_ollama_core::MessageRole::System,
                "assistant" => mlx_ollama_core::MessageRole::Assistant,
                "tool" => mlx_ollama_core::MessageRole::Tool,
                _ => mlx_ollama_core::MessageRole::User,
            };
            if matches!(role, mlx_ollama_core::MessageRole::Tool) {
                if let Some(tool_call_id) = message.tool_call_id.as_deref() {
                    return mlx_ollama_core::ChatMessage::tool_result(
                        tool_call_id,
                        message.content.clone(),
                    );
                }
            }
            mlx_ollama_core::ChatMessage::text(role, message.content.clone())
        })
        .collect()
}

fn summary_artifact_to_memory_record(
    artifact: &mlx_agent_core::ContextSummaryArtifact,
) -> mlx_agent_core::MemoryRecord {
    mlx_agent_core::MemoryRecord {
        id: artifact.id.clone(),
        session_id: artifact.session_id.clone(),
        kind: artifact
            .metadata
            .get("kind")
            .cloned()
            .unwrap_or_else(|| "history_summary".to_string()),
        title: artifact.title.clone(),
        content: artifact.content.clone(),
        created_at: artifact.created_at,
        metadata: artifact.metadata.clone(),
    }
}

fn merge_rules(
    allow_target: &mut Vec<String>,
    deny_target: &mut Vec<String>,
    allow: &[String],
    deny: &[String],
) {
    allow_target.extend(
        allow
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
    );
    deny_target.extend(
        deny.iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
    );
    allow_target.sort();
    allow_target.dedup();
    deny_target.sort();
    deny_target.dedup();
}

fn build_policy_config(
    cfg: &super::config::AgentUiConfig,
    mode: ExecutionMode,
    workspace_root: &Path,
    known_skill_hashes: BTreeMap<String, String>,
    tool_policy: mlx_agent_core::ToolPolicyState,
    session_id: Option<&str>,
) -> PolicyConfig {
    let security_mode = cfg.security.security_mode.trim().to_ascii_lowercase();
    let paranoid_mode = security_mode == "paranoid";
    let enterprise_mode = paranoid_mode || security_mode == "enterprise";

    PolicyConfig {
        default_mode: mode,
        tool_allowlist: Vec::new(),
        tool_denylist: Vec::new(),
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
        tool_policy,
        agent_id: DEFAULT_AGENT_ID.to_string(),
        session_id: session_id.map(normalize_scope_key),
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

fn normalize_skill_name(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn install_command_timeout_secs() -> u64 {
    std::env::var("APP_AGENT_INSTALL_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(INSTALL_COMMAND_TIMEOUT_SECS_DEFAULT)
}

fn install_download_timeout_secs() -> u64 {
    std::env::var("APP_AGENT_INSTALL_DOWNLOAD_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(INSTALL_DOWNLOAD_TIMEOUT_SECS_DEFAULT)
}

fn normalize_node_manager(value: Option<&str>, fallback: &str) -> String {
    match value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_ascii_lowercase()
        .as_str()
    {
        "pnpm" => "pnpm".to_string(),
        "bun" => "bun".to_string(),
        _ => "npm".to_string(),
    }
}

fn collect_requested_skill_names(skill: &Option<String>, skills: &[String]) -> Vec<String> {
    let mut values = Vec::new();
    if let Some(one) = skill
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        values.push(one.to_string());
    }
    values.extend(
        skills
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
    );
    values.sort();
    values.dedup();
    values
}

fn skill_override<'a>(
    cfg: &'a super::config::AgentUiConfig,
    name: &str,
) -> Option<&'a super::config::AgentSkillOverride> {
    cfg.skill_overrides.get(&normalize_skill_name(name))
}

fn skill_override_mut<'a>(
    cfg: &'a mut super::config::AgentUiConfig,
    name: &str,
) -> &'a mut super::config::AgentSkillOverride {
    cfg.skill_overrides
        .entry(normalize_skill_name(name))
        .or_default()
}

fn is_secret_like_key(key: &str) -> bool {
    let normalized = key.trim().to_ascii_uppercase();
    normalized.contains("KEY")
        || normalized.contains("TOKEN")
        || normalized.contains("SECRET")
        || normalized.contains("PASSWORD")
}

fn skill_secret_ref(skill: &str, env_key: &str) -> String {
    format!(
        "agent.skills.{}.{}",
        normalize_skill_name(skill),
        env_key.trim().to_ascii_lowercase()
    )
}

fn apply_skill_config_update(
    settings_dir: &Path,
    agent_cfg: &mut super::config::AgentUiConfig,
    request: &AgentSkillConfigRequest,
) -> Result<(), AgentApiError> {
    let skill_name = request.skill.trim();
    if skill_name.is_empty() {
        return Err(AgentApiError::bad_request("skill cannot be empty"));
    }

    let use_vault = agent_cfg.security.use_secrets_vault;
    let vault = if use_vault {
        Some(SecretsVault::open(settings_dir).map_err(|error| {
            AgentApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "secrets_vault_error",
                Some(error.to_string()),
            )
        })?)
    } else {
        None
    };

    let override_entry = skill_override_mut(agent_cfg, skill_name);
    if let Some(enabled) = request.enabled {
        override_entry.enabled = Some(enabled);
    }

    for key in &request.clear_env {
        let normalized_key = key.trim();
        if normalized_key.is_empty() {
            continue;
        }
        if let Some(reference) = override_entry.env_refs.remove(normalized_key) {
            if let Some(vault) = vault.as_ref() {
                let _ =
                    vault.remove_secret(reference.strip_prefix("vault://").unwrap_or(&reference));
            }
        }
        override_entry.env.remove(normalized_key);
    }

    for (key, value) in &request.env {
        let normalized_key = key.trim();
        if normalized_key.is_empty() {
            continue;
        }
        if value.trim().is_empty() {
            override_entry.env.remove(normalized_key);
            if let Some(reference) = override_entry.env_refs.remove(normalized_key) {
                if let Some(vault) = vault.as_ref() {
                    let _ = vault
                        .remove_secret(reference.strip_prefix("vault://").unwrap_or(&reference));
                }
            }
            continue;
        }

        if use_vault && is_secret_like_key(normalized_key) {
            let secret_key = skill_secret_ref(skill_name, normalized_key);
            if let Some(vault) = vault.as_ref() {
                vault.set_secret(&secret_key, value).map_err(|error| {
                    AgentApiError::new(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "secrets_vault_error",
                        Some(error.to_string()),
                    )
                })?;
            }
            override_entry.env.remove(normalized_key);
            override_entry
                .env_refs
                .insert(normalized_key.to_string(), format!("vault://{secret_key}"));
        } else {
            override_entry
                .env
                .insert(normalized_key.to_string(), value.clone());
            override_entry.env_refs.remove(normalized_key);
        }
    }

    for key in &request.clear_config {
        let normalized_key = key.trim();
        if !normalized_key.is_empty() {
            override_entry.config.remove(normalized_key);
        }
    }

    for (key, value) in &request.config {
        let normalized_key = key.trim();
        if normalized_key.is_empty() {
            continue;
        }
        if value.trim().is_empty() {
            override_entry.config.remove(normalized_key);
        } else {
            override_entry
                .config
                .insert(normalized_key.to_string(), value.clone());
        }
    }

    Ok(())
}

fn effective_skill_enabled(cfg: &super::config::AgentUiConfig, name: &str) -> bool {
    if let Some(flag) = skill_override(cfg, name).and_then(|entry| entry.enabled) {
        return flag;
    }

    let enabled = enabled_set(&cfg.enabled_skills);
    enabled.is_empty() || enabled.contains(&normalize_skill_name(name))
}

fn install_option_supported(spec: &mlx_agent_skills::InstallSpec) -> bool {
    spec.os.is_empty()
        || spec
            .os
            .iter()
            .any(|value| value.eq_ignore_ascii_case(mlx_agent_skills::current_os_tag()))
}

fn skill_install_options(skill: &mlx_agent_skills::SkillPackage) -> Vec<AgentSkillInstallOption> {
    skill
        .install
        .iter()
        .filter(|spec| install_option_supported(spec))
        .map(|spec| AgentSkillInstallOption {
            id: spec.id.clone().unwrap_or_else(|| {
                format!(
                    "{}:{}",
                    skill.name,
                    format!("{:?}", spec.kind).to_ascii_lowercase()
                )
            }),
            kind: format!("{:?}", spec.kind).to_ascii_lowercase(),
            label: spec
                .label
                .clone()
                .or_else(|| {
                    spec.formula
                        .clone()
                        .or_else(|| spec.package.clone())
                        .or_else(|| spec.module.clone())
                        .or_else(|| spec.url.clone())
                })
                .unwrap_or_else(|| format!("{:?}", spec.kind)),
            bins: spec.bins.clone(),
            os: spec.os.clone(),
        })
        .collect()
}

fn build_skill_requirement_context(
    cfg: &super::config::AgentUiConfig,
) -> Result<mlx_agent_skills::RequirementContext, AgentApiError> {
    let vault = if cfg.security.use_secrets_vault {
        Some(open_secrets_vault()?)
    } else {
        None
    };

    let mut env_keys = BTreeSet::new();
    let mut config_keys = BTreeSet::new();

    for entry in cfg.skill_overrides.values() {
        for (key, value) in &entry.env {
            if !value.trim().is_empty() {
                env_keys.insert(mlx_agent_skills::normalize_env_key(key));
            }
        }
        for (key, reference) in &entry.env_refs {
            if reference.trim().is_empty() {
                continue;
            }
            let ref_key = reference.strip_prefix("vault://").unwrap_or(reference);
            let present = if let Some(vault) = vault.as_ref() {
                vault
                    .get_secret(ref_key)
                    .map_err(|error| {
                        AgentApiError::new(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "secrets_vault_error",
                            Some(error.to_string()),
                        )
                    })?
                    .is_some()
            } else {
                false
            };
            if present {
                env_keys.insert(mlx_agent_skills::normalize_env_key(key));
            }
        }
        for (key, value) in &entry.config {
            if !value.trim().is_empty() {
                config_keys.insert(mlx_agent_skills::normalize_config_key(key));
            }
        }
    }

    Ok(mlx_agent_skills::RequirementContext::from_current_env()
        .with_env_keys(env_keys)
        .with_config_keys(config_keys))
}

async fn evaluate_skill_integrity_for_packages(
    skills: &[mlx_agent_skills::SkillPackage],
    policy: &(dyn PolicyEngine + Send + Sync),
) -> BTreeMap<String, String> {
    let mut statuses = BTreeMap::new();
    for skill in skills {
        match policy.check_skill_load(skill).await {
            mlx_agent_core::policy::PolicyDecision::Allow => {
                statuses.insert(skill.name.clone(), "ok".to_string());
            }
            mlx_agent_core::policy::PolicyDecision::Ask { prompt, .. } => {
                warn!(skill = %skill.name, warning = %prompt, "skill integrity warning");
                statuses.insert(skill.name.clone(), "changed".to_string());
            }
            mlx_agent_core::policy::PolicyDecision::Deny { reason } => {
                warn!(skill = %skill.name, reason = %reason, "skill blocked by integrity policy");
                statuses.insert(skill.name.clone(), "blocked".to_string());
            }
        }
    }
    statuses
}

struct LoadedSkillCatalog {
    discovered: Vec<mlx_agent_skills::DiscoveredSkill>,
    items: Vec<AgentSkillInfo>,
}

async fn load_skill_catalog(
    state: &super::AppState,
    cfg: &super::config::AppConfig,
) -> Result<LoadedSkillCatalog, AgentApiError> {
    let workspace = cfg
        .agent
        .workspace_root
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| state.agent_state.default_workspace.clone());

    let context = build_skill_requirement_context(&cfg.agent)?;
    let loader = mlx_agent_skills::SkillLoader::from_workspace(
        &workspace,
        mlx_agent_skills::SkillLimits::default(),
    );
    let discovered = loader
        .discover_all_with_context(&context)
        .await
        .map_err(|error| {
            AgentApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "skills_load_failed",
                Some(error.to_string()),
            )
        })?;

    let known_skill_hashes = load_skill_integrity_state();
    let mode = parse_execution_mode(Some(cfg.agent.execution_mode.as_str()));
    let tool_policy = build_tool_policy_state(&cfg.agent, None, None);
    let policy_cfg = build_policy_config(
        &cfg.agent,
        mode,
        &workspace,
        known_skill_hashes,
        tool_policy,
        None,
    );
    let policy = DefaultPolicyEngine::new(policy_cfg);
    let packages = discovered
        .iter()
        .map(|entry| entry.package.clone())
        .collect::<Vec<_>>();
    save_skill_integrity_state(
        packages
            .iter()
            .filter_map(|package| {
                package
                    .sha256
                    .as_ref()
                    .map(|hash| (package.name.clone(), hash.clone()))
            })
            .collect(),
    );
    let integrity = evaluate_skill_integrity_for_packages(&packages, &policy).await;

    let mut items = discovered
        .iter()
        .map(|entry| {
            let package = &entry.package;
            let eligible = entry.requirements.satisfied;
            let enabled = effective_skill_enabled(&cfg.agent, &package.name);
            let integrity_status = integrity
                .get(&package.name)
                .cloned()
                .unwrap_or_else(|| "unknown".to_string());
            let active = enabled && eligible && integrity_status != "blocked";

            AgentSkillInfo {
                name: package.name.clone(),
                description: package.description.clone(),
                enabled,
                active,
                eligible,
                source: format!("{:?}", package.source).to_ascii_lowercase(),
                bundled: matches!(package.source, mlx_agent_skills::SkillSource::Bundled),
                integrity: integrity_status,
                sha256: package.sha256.clone(),
                capabilities: skill_capability_labels(package),
                missing: entry.requirements.missing_items(),
                install_options: skill_install_options(package),
                primary_env: package.primary_env.clone(),
                configured_env: package
                    .requires
                    .env
                    .iter()
                    .filter(|key| context.has_env(key))
                    .cloned()
                    .collect(),
                configured_config: package
                    .requires
                    .config
                    .iter()
                    .filter(|key| context.has_config(key))
                    .cloned()
                    .collect(),
                os: package.os.clone(),
            }
        })
        .collect::<Vec<_>>();

    items.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(LoadedSkillCatalog { discovered, items })
}

fn build_skills_check_response(
    skills: Vec<AgentSkillInfo>,
    node_manager: &str,
) -> AgentSkillsCheckResponse {
    let summary = AgentSkillsCheckSummary {
        total: skills.len(),
        eligible: skills.iter().filter(|skill| skill.eligible).count(),
        active: skills.iter().filter(|skill| skill.active).count(),
        missing_dependencies: skills
            .iter()
            .filter(|skill| {
                skill.missing.iter().any(|item| {
                    item.starts_with("bin:")
                        || item.starts_with("anyBin:")
                        || item.starts_with("os:")
                })
            })
            .count(),
        missing_configuration: skills
            .iter()
            .filter(|skill| {
                skill
                    .missing
                    .iter()
                    .any(|item| item.starts_with("env:") || item.starts_with("config:"))
            })
            .count(),
        configure_now: skills.iter().any(|skill| {
            skill
                .missing
                .iter()
                .any(|item| item.starts_with("env:") || item.starts_with("config:"))
        }),
        installable: skills
            .iter()
            .filter(|skill| !skill.install_options.is_empty() && !skill.eligible)
            .count(),
        node_manager: node_manager.to_string(),
    };

    AgentSkillsCheckResponse { summary, skills }
}

fn install_spec_matches_selection(
    spec: &mlx_agent_skills::InstallSpec,
    install_ids: &HashSet<String>,
) -> bool {
    if install_ids.is_empty() {
        return true;
    }
    spec.id
        .as_deref()
        .map(normalize_skill_name)
        .is_some_and(|value| install_ids.contains(&value))
}

fn install_spec_is_relevant(
    spec: &mlx_agent_skills::InstallSpec,
    requirements: &mlx_agent_skills::RequirementCheck,
) -> bool {
    if requirements.satisfied {
        return false;
    }

    if spec.bins.is_empty() {
        return true;
    }

    let mut missing_bins = requirements
        .missing_bins
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    missing_bins.extend(requirements.missing_any_bins.iter().cloned());
    spec.bins.iter().any(|bin| missing_bins.contains(bin))
}

fn build_install_command(
    spec: &mlx_agent_skills::InstallSpec,
    node_manager: &str,
) -> Result<Option<(String, Vec<String>)>, String> {
    match spec.kind {
        mlx_agent_skills::InstallKind::Brew => {
            let formula = spec
                .formula
                .clone()
                .or_else(|| spec.package.clone())
                .or_else(|| spec.module.clone())
                .ok_or_else(|| "missing formula/package".to_string())?;
            Ok(Some((
                "brew".to_string(),
                vec!["install".to_string(), formula],
            )))
        }
        mlx_agent_skills::InstallKind::Go => {
            let module = spec
                .module
                .clone()
                .or_else(|| spec.package.clone())
                .or_else(|| spec.formula.clone())
                .ok_or_else(|| "missing module/package".to_string())?;
            let target = if module.contains('@') {
                module
            } else {
                format!("{module}@latest")
            };
            Ok(Some((
                "go".to_string(),
                vec!["install".to_string(), target],
            )))
        }
        mlx_agent_skills::InstallKind::Node => {
            let package = spec
                .package
                .clone()
                .or_else(|| spec.module.clone())
                .or_else(|| spec.formula.clone())
                .ok_or_else(|| "missing package/module".to_string())?;
            let command = match node_manager {
                "pnpm" => (
                    "pnpm".to_string(),
                    vec!["add".to_string(), "-g".to_string(), package],
                ),
                "bun" => (
                    "bun".to_string(),
                    vec!["add".to_string(), "-g".to_string(), package],
                ),
                _ => (
                    "npm".to_string(),
                    vec!["install".to_string(), "-g".to_string(), package],
                ),
            };
            Ok(Some(command))
        }
        mlx_agent_skills::InstallKind::Uv => {
            let package = spec
                .package
                .clone()
                .or_else(|| spec.module.clone())
                .or_else(|| spec.formula.clone())
                .ok_or_else(|| "missing package/module".to_string())?;
            Ok(Some((
                "uv".to_string(),
                vec!["tool".to_string(), "install".to_string(), package],
            )))
        }
        mlx_agent_skills::InstallKind::Download | mlx_agent_skills::InstallKind::Manual => Ok(None),
    }
}

async fn run_install_command(program: &str, args: &[String]) -> AgentSkillInstallExecution {
    let timeout_secs = install_command_timeout_secs();
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        Command::new(program).args(args).output(),
    )
    .await;
    match output {
        Ok(Ok(output)) => AgentSkillInstallExecution {
            id: format!("{program}:{}", args.join(" ")),
            kind: "command".to_string(),
            label: format!("{program} {}", args.join(" ")).trim().to_string(),
            ok: output.status.success(),
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            warnings: Vec::new(),
        },
        Ok(Err(error)) => AgentSkillInstallExecution {
            id: format!("{program}:{}", args.join(" ")),
            kind: "command".to_string(),
            label: format!("{program} {}", args.join(" ")).trim().to_string(),
            ok: false,
            code: None,
            stdout: String::new(),
            stderr: error.to_string(),
            warnings: Vec::new(),
        },
        Err(_) => AgentSkillInstallExecution {
            id: format!("{program}:{}", args.join(" ")),
            kind: "command".to_string(),
            label: format!("{program} {}", args.join(" ")).trim().to_string(),
            ok: false,
            code: None,
            stdout: String::new(),
            stderr: format!("command timed out after {timeout_secs}s"),
            warnings: vec!["timeout".to_string()],
        },
    }
}

async fn execute_install_spec(
    skill_name: &str,
    spec: &mlx_agent_skills::InstallSpec,
    node_manager: &str,
) -> AgentSkillInstallExecution {
    let id = spec.id.clone().unwrap_or_else(|| {
        format!(
            "{}:{}",
            skill_name,
            format!("{:?}", spec.kind).to_ascii_lowercase()
        )
    });
    let label = spec
        .label
        .clone()
        .or_else(|| {
            spec.formula
                .clone()
                .or_else(|| spec.package.clone())
                .or_else(|| spec.module.clone())
                .or_else(|| spec.url.clone())
        })
        .unwrap_or_else(|| format!("{:?}", spec.kind));

    if !install_option_supported(spec) {
        return AgentSkillInstallExecution {
            id,
            kind: format!("{:?}", spec.kind).to_ascii_lowercase(),
            label,
            ok: false,
            code: None,
            stdout: String::new(),
            stderr: "install option unsupported on current OS".to_string(),
            warnings: vec!["unsupported_os".to_string()],
        };
    }

    match spec.kind {
        mlx_agent_skills::InstallKind::Brew
        | mlx_agent_skills::InstallKind::Go
        | mlx_agent_skills::InstallKind::Node
        | mlx_agent_skills::InstallKind::Uv => match build_install_command(spec, node_manager) {
            Ok(Some((program, args))) => {
                let mut result = run_install_command(&program, &args).await;
                result.id = id;
                result.kind = format!("{:?}", spec.kind).to_ascii_lowercase();
                result.label = label;
                result
            }
            Ok(None) => AgentSkillInstallExecution {
                id,
                kind: format!("{:?}", spec.kind).to_ascii_lowercase(),
                label,
                ok: false,
                code: None,
                stdout: String::new(),
                stderr: "no install command generated".to_string(),
                warnings: vec!["invalid_install_spec".to_string()],
            },
            Err(error) => AgentSkillInstallExecution {
                id,
                kind: format!("{:?}", spec.kind).to_ascii_lowercase(),
                label,
                ok: false,
                code: None,
                stdout: String::new(),
                stderr: error,
                warnings: vec!["invalid_install_spec".to_string()],
            },
        },
        mlx_agent_skills::InstallKind::Download => {
            let Some(url) = spec.url.clone() else {
                return AgentSkillInstallExecution {
                    id,
                    kind: "download".to_string(),
                    label,
                    ok: false,
                    code: None,
                    stdout: String::new(),
                    stderr: "missing url".to_string(),
                    warnings: vec!["invalid_install_spec".to_string()],
                };
            };

            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(
                    install_download_timeout_secs(),
                ))
                .build();
            let client = match client {
                Ok(client) => client,
                Err(error) => {
                    return AgentSkillInstallExecution {
                        id,
                        kind: "download".to_string(),
                        label,
                        ok: false,
                        code: None,
                        stdout: String::new(),
                        stderr: error.to_string(),
                        warnings: Vec::new(),
                    }
                }
            };
            let response = match client.get(url.clone()).send().await {
                Ok(response) => response,
                Err(error) => {
                    return AgentSkillInstallExecution {
                        id,
                        kind: "download".to_string(),
                        label,
                        ok: false,
                        code: None,
                        stdout: String::new(),
                        stderr: error.to_string(),
                        warnings: Vec::new(),
                    }
                }
            };
            let status = response.status();
            let bytes = match response.bytes().await {
                Ok(bytes) => bytes,
                Err(error) => {
                    return AgentSkillInstallExecution {
                        id,
                        kind: "download".to_string(),
                        label,
                        ok: false,
                        code: status.as_u16().try_into().ok(),
                        stdout: String::new(),
                        stderr: error.to_string(),
                        warnings: Vec::new(),
                    }
                }
            };

            let downloads_dir = settings_dir()
                .join("skill-downloads")
                .join(normalize_skill_name(skill_name));
            if let Err(error) = tokio::fs::create_dir_all(&downloads_dir).await {
                return AgentSkillInstallExecution {
                    id,
                    kind: "download".to_string(),
                    label,
                    ok: false,
                    code: None,
                    stdout: String::new(),
                    stderr: error.to_string(),
                    warnings: Vec::new(),
                };
            }

            let file_name = url
                .split('/')
                .next_back()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("download.bin");
            let path = downloads_dir.join(file_name);
            let mut file = match tokio::fs::File::create(&path).await {
                Ok(file) => file,
                Err(error) => {
                    return AgentSkillInstallExecution {
                        id,
                        kind: "download".to_string(),
                        label,
                        ok: false,
                        code: None,
                        stdout: String::new(),
                        stderr: error.to_string(),
                        warnings: Vec::new(),
                    }
                }
            };
            if let Err(error) = file.write_all(bytes.as_ref()).await {
                return AgentSkillInstallExecution {
                    id,
                    kind: "download".to_string(),
                    label,
                    ok: false,
                    code: None,
                    stdout: String::new(),
                    stderr: error.to_string(),
                    warnings: Vec::new(),
                };
            }

            AgentSkillInstallExecution {
                id,
                kind: "download".to_string(),
                label,
                ok: status.is_success(),
                code: status.as_u16().try_into().ok(),
                stdout: path.display().to_string(),
                stderr: String::new(),
                warnings: vec!["artifact_downloaded_only".to_string()],
            }
        }
        mlx_agent_skills::InstallKind::Manual => AgentSkillInstallExecution {
            id,
            kind: "manual".to_string(),
            label,
            ok: false,
            code: None,
            stdout: spec.url.clone().unwrap_or_default(),
            stderr: "manual install required".to_string(),
            warnings: vec!["manual_install_required".to_string()],
        },
    }
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

fn build_tool_registry(state: &super::AppState) -> ToolRegistry {
    use mlx_agent_tools::{EditFileTool, ExecTool, ListDirTool, ReadFileTool, WriteFileTool};

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ReadFileTool::new()));
    registry.register(Arc::new(WriteFileTool::new()));
    registry.register(Arc::new(EditFileTool::new()));
    registry.register(Arc::new(ListDirTool::new()));
    registry.register(Arc::new(ExecTool::new()));

    crate::agent_runtime_tools::register_runtime_tools(
        &mut registry,
        &crate::agent_runtime_tools::RuntimeToolServices {
            sessions: state.session_store.clone(),
            channels: state.channel_service.clone(),
            memory: state.agent_state.memory.clone(),
            budget_tracker: state.agent_state.budget_tracker.clone(),
        },
    );

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
    let session_id = request
        .session_id
        .clone()
        .unwrap_or_else(mlx_agent_core::SessionStore::new_session_id);

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
    let tool_policy = build_tool_policy_state(
        agent_cfg,
        Some(&session_id),
        request.enabled_tools.as_deref(),
    );
    let policy_config = build_policy_config(
        agent_cfg,
        mode,
        &workspace,
        known_skill_hashes,
        tool_policy,
        Some(&session_id),
    );
    let policy: Arc<dyn PolicyEngine> = Arc::new(DefaultPolicyEngine::new(policy_config));

    let tool_registry = build_tool_registry(state);

    let mut skill_runtime = mlx_agent_core::runtime::SkillRuntime::new();
    let skill_context = build_skill_requirement_context(agent_cfg)?;
    skill_runtime
        .load_from_workspace_with_context(&workspace, &skill_context)
        .await;
    let _ = evaluate_skill_integrity(&mut skill_runtime, policy.as_ref(), true).await;
    save_skill_integrity_state(collect_skill_hashes(&skill_runtime));

    let enabled_skills = if request
        .enabled_skills
        .as_ref()
        .map(|values| !values.is_empty())
        .unwrap_or(false)
    {
        request.enabled_skills.clone().unwrap_or_default()
    } else {
        skill_runtime
            .names()
            .into_iter()
            .filter(|name| effective_skill_enabled(agent_cfg, name))
            .collect()
    };

    let config = AgentLoopConfig {
        session_id: session_id.clone(),
        model_id: resolved.model_id.clone(),
        workspace_root: workspace.clone(),
        initial_history: session_messages_to_chat_history(
            &state
                .session_store
                .load(&session_id)
                .await
                .unwrap_or_default(),
        ),
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
        tool_profile: parse_tool_profile(Some(agent_cfg.tool_policy.profile.as_str())),
        skill_filter: Some(enabled_skills),
    };

    info!(
        provider = %resolved.provider_name,
        model = %resolved.model_id,
        mode = ?mode,
        approval = ?approval_mode,
        session_id = ?request.session_id,
        "starting agent run"
    );

    let _ = state.session_store.ensure_session(&session_id, None).await;

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

    let _ = state
        .session_store
        .append(
            &session_id,
            &mlx_agent_core::session::SessionMessage {
                role: "user".to_string(),
                content: request.message.clone(),
                tool_call_id: None,
                tool_name: None,
                timestamp: chrono::Utc::now(),
            },
        )
        .await;

    let _ = state
        .session_store
        .append(
            &session_id,
            &mlx_agent_core::session::SessionMessage {
                role: "assistant".to_string(),
                content: response.content.clone(),
                tool_call_id: None,
                tool_name: None,
                timestamp: chrono::Utc::now(),
            },
        )
        .await;

    {
        let mut budget = state.agent_state.budget_tracker.write().await;
        budget.insert(session_id.clone(), response.budget.clone());
    }

    let records = response
        .summary_artifacts
        .iter()
        .map(summary_artifact_to_memory_record)
        .collect::<Vec<_>>();
    let _ = state.agent_state.memory.upsert(&records).await;

    Ok(AgentRunResponse {
        provider: resolved.provider_name,
        model_id: resolved.model_id,
        session_id: session_id.clone(),
        audit_id: Some(session_id),
        content: response.content,
        iterations: response.iterations,
        tool_calls_made: response.tool_calls_made,
        prompt_tokens: response.usage.prompt_tokens,
        completion_tokens: response.usage.completion_tokens,
        total_tokens: response.usage.total_tokens,
        latency_ms: response.latency_ms,
        context_budget: Some(response.budget),
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
    sync_legacy_enabled_tools(&mut cfg.agent);
    if cfg.agent.security.use_secrets_vault
        && cfg.agent.api_key.trim().is_empty()
        && cfg
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
    Ok(Json(cfg.agent))
}

/// POST /agent/config
pub async fn agent_update_config(
    State(state): State<super::AppState>,
    Json(new_agent_cfg): Json<super::config::AgentUiConfig>,
) -> Result<Json<super::config::AgentUiConfig>, AgentApiError> {
    let mut cfg = super::config::AppConfig::load_settings().apply_env();
    let mut merged = new_agent_cfg.clone();
    if merged.skill_overrides.is_empty() {
        merged.skill_overrides = cfg.agent.skill_overrides.clone();
    }
    if merged.tool_policy.agent_overrides.is_empty() {
        merged.tool_policy.agent_overrides = cfg.agent.tool_policy.agent_overrides.clone();
    }
    if merged.tool_policy.session_overrides.is_empty() {
        merged.tool_policy.session_overrides = cfg.agent.tool_policy.session_overrides.clone();
    }
    if merged.tool_policy.profile.trim().is_empty() {
        merged.tool_policy.profile = cfg.agent.tool_policy.profile.clone();
    }
    merged.node_package_manager = normalize_node_manager(
        Some(merged.node_package_manager.as_str()),
        &cfg.agent.node_package_manager,
    );

    if merged.tool_policy.agent_overrides.is_empty() && !merged.enabled_tools.is_empty() {
        merged.tool_policy.agent_overrides.insert(
            DEFAULT_AGENT_ID.to_string(),
            crate::config::AgentToolScopeOverride {
                allow: merged.enabled_tools.clone(),
                deny: Vec::new(),
            },
        );
    }
    sync_legacy_enabled_tools(&mut merged);

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
    let catalog = load_skill_catalog(&state, &cfg).await?;
    Ok(Json(catalog.items))
}

/// GET /agent/skills/check
pub async fn agent_check_skills(
    State(state): State<super::AppState>,
) -> Result<Json<AgentSkillsCheckResponse>, AgentApiError> {
    let cfg = super::config::AppConfig::load_settings().apply_env();
    let catalog = load_skill_catalog(&state, &cfg).await?;
    Ok(Json(build_skills_check_response(
        catalog.items,
        &normalize_node_manager(None, &cfg.agent.node_package_manager),
    )))
}

/// POST /agent/skills/reload
pub async fn agent_reload_skills(
    State(state): State<super::AppState>,
) -> Result<Json<Vec<AgentSkillInfo>>, AgentApiError> {
    agent_list_skills(State(state)).await
}

/// POST /agent/skills/install
pub async fn agent_install_skills(
    State(state): State<super::AppState>,
    Json(request): Json<AgentSkillsInstallRequest>,
) -> Result<Json<AgentSkillsInstallResponse>, AgentApiError> {
    let cfg = super::config::AppConfig::load_settings().apply_env();
    let catalog = load_skill_catalog(&state, &cfg).await?;
    let requested_names = collect_requested_skill_names(&request.skill, &request.skills);
    let requested_set = requested_names
        .iter()
        .map(|value| normalize_skill_name(value))
        .collect::<HashSet<_>>();
    let install_ids = request
        .install_ids
        .iter()
        .map(|value| normalize_skill_name(value))
        .collect::<HashSet<_>>();
    let node_manager = normalize_node_manager(
        request.node_manager.as_deref(),
        &cfg.agent.node_package_manager,
    );

    let mut results = Vec::new();

    for entry in &catalog.discovered {
        let skill = &entry.package;
        if !requested_set.is_empty() && !requested_set.contains(&normalize_skill_name(&skill.name))
        {
            continue;
        }

        let mut installs = Vec::new();
        let mut warnings = Vec::new();

        for spec in skill
            .install
            .iter()
            .filter(|spec| install_spec_matches_selection(spec, &install_ids))
            .filter(|spec| install_spec_is_relevant(spec, &entry.requirements))
        {
            installs.push(execute_install_spec(&skill.name, spec, &node_manager).await);
        }

        if installs.is_empty() {
            warnings.push("no_relevant_install_options".to_string());
        }

        results.push(AgentSkillInstallResult {
            skill: skill.name.clone(),
            installs,
            warnings,
        });
    }

    Ok(Json(AgentSkillsInstallResponse {
        node_manager,
        results,
    }))
}

/// POST /agent/skills/enable
pub async fn agent_enable_skills(
    Json(request): Json<AgentSkillToggleRequest>,
) -> Result<Json<serde_json::Value>, AgentApiError> {
    let requested = collect_requested_skill_names(&request.skill, &request.skills);
    if requested.is_empty() {
        return Err(AgentApiError::bad_request("no skills provided"));
    }

    let mut cfg = super::config::AppConfig::load_settings().apply_env();
    for skill in requested {
        skill_override_mut(&mut cfg.agent, &skill).enabled = Some(true);
    }
    cfg.save_settings().map_err(|error| {
        AgentApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "save_failed",
            Some(error.to_string()),
        )
    })?;

    Ok(Json(serde_json::json!({ "status": "ok" })))
}

/// POST /agent/skills/disable
pub async fn agent_disable_skills(
    Json(request): Json<AgentSkillToggleRequest>,
) -> Result<Json<serde_json::Value>, AgentApiError> {
    let requested = collect_requested_skill_names(&request.skill, &request.skills);
    if requested.is_empty() {
        return Err(AgentApiError::bad_request("no skills provided"));
    }

    let mut cfg = super::config::AppConfig::load_settings().apply_env();
    for skill in requested {
        skill_override_mut(&mut cfg.agent, &skill).enabled = Some(false);
    }
    cfg.save_settings().map_err(|error| {
        AgentApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "save_failed",
            Some(error.to_string()),
        )
    })?;

    Ok(Json(serde_json::json!({ "status": "ok" })))
}

/// POST /agent/skills/config
pub async fn agent_configure_skill(
    State(state): State<super::AppState>,
    Json(request): Json<AgentSkillConfigRequest>,
) -> Result<Json<AgentSkillInfo>, AgentApiError> {
    let skill_name = request.skill.trim();
    let mut cfg = super::config::AppConfig::load_settings().apply_env();
    apply_skill_config_update(&settings_dir(), &mut cfg.agent, &request)?;

    cfg.save_settings().map_err(|error| {
        AgentApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "save_failed",
            Some(error.to_string()),
        )
    })?;

    let refreshed_cfg = super::config::AppConfig::load_settings().apply_env();
    let catalog = load_skill_catalog(&state, &refreshed_cfg).await?;
    let Some(item) = catalog
        .items
        .into_iter()
        .find(|item| item.name.eq_ignore_ascii_case(skill_name))
    else {
        return Err(AgentApiError::new(
            StatusCode::NOT_FOUND,
            "skill_not_found",
            Some(skill_name.to_string()),
        ));
    };

    Ok(Json(item))
}

/// GET /agent/tools
pub async fn agent_list_tools() -> Result<Json<Vec<AgentToolInfo>>, AgentApiError> {
    let cfg = super::config::AppConfig::load_settings().apply_env();
    let effective = mlx_agent_core::resolve_effective_tool_policy(
        &build_tool_policy_state(&cfg.agent, None, None),
        DEFAULT_AGENT_ID,
        None,
    );

    Ok(Json(
        effective
            .entries
            .into_iter()
            .map(|entry| AgentToolInfo {
                name: entry.name,
                description: entry.description,
                enabled: entry.allowed,
                policy: if entry.allowed { "allow" } else { "deny" }.to_string(),
            })
            .collect(),
    ))
}

/// GET /agent/tools/catalog
pub async fn agent_tools_catalog() -> Result<Json<serde_json::Value>, AgentApiError> {
    let profiles = [
        mlx_agent_core::ToolProfileName::Minimal,
        mlx_agent_core::ToolProfileName::Coding,
        mlx_agent_core::ToolProfileName::Messaging,
        mlx_agent_core::ToolProfileName::Full,
    ]
    .into_iter()
    .map(|profile| {
        serde_json::json!({
            "id": profile.as_str(),
            "tools": mlx_agent_core::profile_tool_names(profile).into_iter().collect::<Vec<_>>(),
        })
    })
    .collect::<Vec<_>>();

    Ok(Json(serde_json::json!({
        "profiles": profiles,
        "entries": mlx_agent_core::tool_catalog(),
    })))
}

/// GET /agent/tools/effective-policy
pub async fn agent_tools_effective_policy(
    Query(query): Query<ToolPolicyQuery>,
) -> Result<Json<mlx_agent_core::EffectiveToolPolicy>, AgentApiError> {
    let cfg = super::config::AppConfig::load_settings().apply_env();
    let agent_id = query
        .agent_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(DEFAULT_AGENT_ID);
    Ok(Json(mlx_agent_core::resolve_effective_tool_policy(
        &build_tool_policy_state(&cfg.agent, query.session_id.as_deref(), None),
        agent_id,
        query.session_id.as_deref(),
    )))
}

/// POST /agent/tools/profile
pub async fn agent_tools_profile(
    Json(request): Json<AgentToolProfileRequest>,
) -> Result<Json<mlx_agent_core::EffectiveToolPolicy>, AgentApiError> {
    let mut cfg = super::config::AppConfig::load_settings().apply_env();
    let profile = parse_tool_profile(Some(request.profile.as_str()));
    cfg.agent.tool_policy.profile = profile.as_str().to_string();

    let agent_rules = cfg
        .agent
        .tool_policy
        .agent_overrides
        .entry(DEFAULT_AGENT_ID.to_string())
        .or_default();
    agent_rules.allow = mlx_agent_core::profile_tool_names(profile)
        .into_iter()
        .collect();
    agent_rules.deny.clear();
    sync_legacy_enabled_tools(&mut cfg.agent);

    cfg.save_settings().map_err(|error| {
        AgentApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "save_failed",
            Some(error.to_string()),
        )
    })?;

    Ok(Json(mlx_agent_core::resolve_effective_tool_policy(
        &build_tool_policy_state(&cfg.agent, None, None),
        DEFAULT_AGENT_ID,
        None,
    )))
}

/// POST /agent/tools/allow-deny
pub async fn agent_tools_allow_deny(
    Json(request): Json<AgentToolAllowDenyRequest>,
) -> Result<Json<mlx_agent_core::EffectiveToolPolicy>, AgentApiError> {
    let mut cfg = super::config::AppConfig::load_settings().apply_env();
    let scope = request.scope.trim().to_ascii_lowercase();

    match scope.as_str() {
        "global" => {
            if request.replace {
                cfg.agent.security.tool_allowlist = request.allow.clone();
                cfg.agent.security.tool_denylist = request.deny.clone();
            } else {
                merge_rules(
                    &mut cfg.agent.security.tool_allowlist,
                    &mut cfg.agent.security.tool_denylist,
                    &request.allow,
                    &request.deny,
                );
            }
        }
        "agent" => {
            let agent_id = request
                .agent_id
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(DEFAULT_AGENT_ID);
            let entry = cfg
                .agent
                .tool_policy
                .agent_overrides
                .entry(normalize_scope_key(agent_id))
                .or_default();
            if request.replace {
                entry.allow = request.allow.clone();
                entry.deny = request.deny.clone();
            } else {
                merge_rules(
                    &mut entry.allow,
                    &mut entry.deny,
                    &request.allow,
                    &request.deny,
                );
            }
        }
        "session" => {
            let session_id = request
                .session_id
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    AgentApiError::bad_request("session_id is required for session scope")
                })?;
            let entry = cfg
                .agent
                .tool_policy
                .session_overrides
                .entry(normalize_scope_key(session_id))
                .or_default();
            if request.replace {
                entry.allow = request.allow.clone();
                entry.deny = request.deny.clone();
            } else {
                merge_rules(
                    &mut entry.allow,
                    &mut entry.deny,
                    &request.allow,
                    &request.deny,
                );
            }
        }
        _ => {
            return Err(AgentApiError::bad_request(
                "scope must be global, agent, or session",
            ))
        }
    }

    sync_legacy_enabled_tools(&mut cfg.agent);
    cfg.save_settings().map_err(|error| {
        AgentApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "save_failed",
            Some(error.to_string()),
        )
    })?;

    let effective = mlx_agent_core::resolve_effective_tool_policy(
        &build_tool_policy_state(&cfg.agent, request.session_id.as_deref(), None),
        request.agent_id.as_deref().unwrap_or(DEFAULT_AGENT_ID),
        request.session_id.as_deref(),
    );
    Ok(Json(effective))
}

/// GET /agent/context/budget
pub async fn agent_context_budget(
    State(state): State<super::AppState>,
    Query(query): Query<ContextBudgetQuery>,
) -> Result<Json<mlx_agent_core::ContextBudgetTelemetry>, AgentApiError> {
    let tracker = state.agent_state.budget_tracker.read().await;

    if let Some(session_id) = query
        .session_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let key = normalize_scope_key(session_id);
        if let Some(entry) = tracker
            .get(&key)
            .cloned()
            .or_else(|| tracker.get(session_id).cloned())
        {
            return Ok(Json(entry));
        }
        return Err(AgentApiError::new(
            StatusCode::NOT_FOUND,
            "budget_not_found",
            Some(format!("no budget telemetry for session '{}'", session_id)),
        ));
    }

    let latest = tracker
        .values()
        .cloned()
        .max_by(|left, right| left.last_updated.cmp(&right.last_updated));
    latest.map(Json).ok_or_else(|| {
        AgentApiError::new(
            StatusCode::NOT_FOUND,
            "budget_not_found",
            Some("no budget telemetry available".to_string()),
        )
    })
}

/// GET /agent/audit
pub async fn agent_audit(
    State(state): State<super::AppState>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<AgentAuditResponse>, AgentApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    let mut entries = read_recent_audit_entries(&state.agent_state.audit.log_dir, limit, &query)
        .map_err(|e| {
            AgentApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "audit_read_failed",
                Some(e.to_string()),
            )
        })?;

    entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    Ok(Json(AgentAuditResponse { entries }))
}

/// GET /agent/audit/:id
pub async fn agent_audit_get_id(
    State(state): State<super::AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<AuditLogEntry>, AgentApiError> {
    let limit = 500;
    let query = AuditQuery {
        limit: Some(limit),
        since: None,
        session_id: None,
        event_type: None,
        tool_name: None,
        status: None,
    };

    let entries = read_recent_audit_entries(&state.agent_state.audit.log_dir, limit, &query)
        .map_err(|e| {
            AgentApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "audit_read_failed",
                Some(e.to_string()),
            )
        })?;

    for entry in entries {
        if entry.id == id {
            return Ok(Json(entry));
        }
    }

    Err(AgentApiError::new(
        StatusCode::NOT_FOUND,
        "entry_not_found",
        Some(format!("audit entry with id {} not found", id)),
    ))
}

/// GET /agent/audit/export
pub async fn agent_audit_export(
    State(state): State<super::AppState>,
    Query(query): Query<AuditQuery>,
) -> Result<axum::response::Response, AgentApiError> {
    let limit = query.limit.unwrap_or(10000).clamp(1, 10000); // Allow bigger limit for exports
    let entries = read_recent_audit_entries(&state.agent_state.audit.log_dir, limit, &query)
        .map_err(|e| {
            AgentApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "audit_read_failed",
                Some(e.to_string()),
            )
        })?;

    let json_bytes = serde_json::to_vec_pretty(&entries).map_err(|e| {
        AgentApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "json_serialize_error",
            Some(e.to_string()),
        )
    })?;

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let filename = format!("agent_audit_export_{}.json", timestamp);

    let response = axum::response::Response::builder()
        .status(StatusCode::OK)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .header(
            axum::http::header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        )
        .body(axum::body::Body::from(json_bytes))
        .map_err(|e| {
            AgentApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "response_build_error",
                Some(e.to_string()),
            )
        })?;

    Ok(response)
}

// ── Session API Handlers ─────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RenameSessionRequest {
    pub name: String,
}

pub async fn agent_list_sessions(
    State(state): State<super::AppState>,
) -> Result<Json<Vec<mlx_agent_core::session::SessionMeta>>, AgentApiError> {
    let sessions = state.session_store.list_sessions().await.map_err(|e| {
        AgentApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "session_error",
            Some(format!("Failed to list sessions: {e}")),
        )
    })?;
    Ok(Json(sessions))
}

pub async fn agent_create_session(
    State(state): State<super::AppState>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<Json<mlx_agent_core::session::SessionMeta>, AgentApiError> {
    let session_id = mlx_agent_core::SessionStore::new_session_id();
    state
        .session_store
        .ensure_session(&session_id, req.name)
        .await
        .map_err(|e| {
            AgentApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "session_error",
                Some(format!("Failed to create session: {e}")),
            )
        })?;

    // Fetch the newly created meta
    let sessions = state
        .session_store
        .list_sessions()
        .await
        .unwrap_or_default();
    let meta = sessions
        .into_iter()
        .find(|s| s.id == session_id)
        .unwrap_or_else(|| mlx_agent_core::session::SessionMeta {
            id: session_id,
            name: "Nova conversa".to_string(),
            updated_at: chrono::Utc::now(),
            message_count: 0,
        });

    Ok(Json(meta))
}

pub async fn agent_get_session(
    State(state): State<super::AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<Vec<mlx_agent_core::session::SessionMessage>>, AgentApiError> {
    let messages = state.session_store.load(&id).await.map_err(|e| {
        AgentApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "session_error",
            Some(format!("Failed to load session: {e}")),
        )
    })?;
    Ok(Json(messages))
}

pub async fn agent_rename_session(
    State(state): State<super::AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<RenameSessionRequest>,
) -> Result<Json<serde_json::Value>, AgentApiError> {
    state
        .session_store
        .rename(&id, &req.name)
        .await
        .map_err(|e| {
            AgentApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "session_error",
                Some(format!("Failed to rename session: {e}")),
            )
        })?;
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn agent_delete_session(
    State(state): State<super::AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, AgentApiError> {
    state.session_store.delete(&id).await.map_err(|e| {
        AgentApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "session_error",
            Some(format!("Failed to delete session: {e}")),
        )
    })?;
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn agent_export_session(
    State(state): State<super::AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<axum::response::Response, AgentApiError> {
    let json_str = state.session_store.export(&id).await.map_err(|e| {
        AgentApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "session_error",
            Some(format!("Failed to export session: {e}")),
        )
    })?;

    Ok(axum::response::Response::builder()
        .header("Content-Type", "application/json")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"session_{}.json\"", id),
        )
        .body(axum::body::Body::from(json_str))
        .unwrap())
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
    query: &AuditQuery,
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

    let since_ts = query.since.as_deref().and_then(|s| {
        chrono::DateTime::parse_from_rfc3339(s)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .ok()
    });

    let filter_session = query.session_id.as_deref().filter(|s| !s.is_empty());
    let filter_event = query.event_type.as_deref().filter(|s| !s.is_empty());
    let filter_tool = query.tool_name.as_deref().filter(|s| !s.is_empty());

    let mut entries = Vec::new();
    for path in files.into_iter().rev() {
        let content = std::fs::read_to_string(path)?;
        for line in content.lines().rev() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<AuditLogEntry>(line) {
                // Apply filters
                if let Some(ts) = since_ts {
                    if entry.timestamp < ts {
                        continue;
                    }
                }
                if let Some(sess) = filter_session {
                    if entry.session_id != sess {
                        continue;
                    }
                }
                if let Some(ev) = filter_event {
                    let event_str = serde_json::to_string(&entry.event_type)
                        .unwrap_or_default()
                        .replace('"', "");
                    if event_str != ev {
                        continue;
                    }
                }
                if let Some(tool) = filter_tool {
                    if entry.tool_name.as_deref() != Some(tool) {
                        continue;
                    }
                }
                if let Some(status) = query.status.as_deref().filter(|s| !s.is_empty()) {
                    let has_error = entry.error.is_some() || entry.error_summary.is_some();
                    let is_denied = entry.decision.as_deref() == Some("deny");

                    match status {
                        "error" if !has_error => continue,
                        "success" if has_error || is_denied => continue,
                        "denied" if !is_denied => continue,
                        _ => {}
                    }
                }

                entries.push(entry);
                if entries.len() >= limit {
                    return Ok(entries);
                }
            }
        }
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    use tempfile::tempdir;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn normalize_node_manager_defaults_to_npm() {
        assert_eq!(normalize_node_manager(None, "npm"), "npm");
        assert_eq!(normalize_node_manager(Some("pnpm"), "npm"), "pnpm");
        assert_eq!(normalize_node_manager(Some("bun"), "npm"), "bun");
        assert_eq!(normalize_node_manager(Some("yarn"), "npm"), "npm");
    }

    #[test]
    fn effective_skill_enabled_respects_override_flag() {
        let mut cfg = crate::config::AgentUiConfig::default();
        cfg.enabled_skills = vec!["github".to_string()];
        cfg.skill_overrides.insert(
            "github".to_string(),
            crate::config::AgentSkillOverride {
                enabled: Some(false),
                ..Default::default()
            },
        );
        assert!(!effective_skill_enabled(&cfg, "github"));

        cfg.skill_overrides.insert(
            "weather".to_string(),
            crate::config::AgentSkillOverride {
                enabled: Some(true),
                ..Default::default()
            },
        );
        assert!(effective_skill_enabled(&cfg, "weather"));
    }

    #[test]
    fn build_install_command_supports_node_managers() {
        let spec = mlx_agent_skills::InstallSpec {
            id: Some("github-node".to_string()),
            kind: mlx_agent_skills::InstallKind::Node,
            label: Some("GitHub CLI".to_string()),
            bins: vec!["gh".to_string()],
            os: Vec::new(),
            formula: None,
            package: Some("@github/gh".to_string()),
            module: None,
            url: None,
        };

        let npm = build_install_command(&spec, "npm").unwrap().unwrap();
        assert_eq!(npm.0, "npm");
        assert_eq!(npm.1, vec!["install", "-g", "@github/gh"]);

        let pnpm = build_install_command(&spec, "pnpm").unwrap().unwrap();
        assert_eq!(pnpm.0, "pnpm");
        assert_eq!(pnpm.1, vec!["add", "-g", "@github/gh"]);

        let bun = build_install_command(&spec, "bun").unwrap().unwrap();
        assert_eq!(bun.0, "bun");
        assert_eq!(bun.1, vec!["add", "-g", "@github/gh"]);
    }

    #[test]
    fn skills_check_summary_counts_missing_state() {
        let response = build_skills_check_response(
            vec![
                AgentSkillInfo {
                    name: "obsidian".to_string(),
                    description: String::new(),
                    enabled: true,
                    active: true,
                    eligible: true,
                    source: "workspace".to_string(),
                    bundled: false,
                    integrity: "ok".to_string(),
                    sha256: None,
                    capabilities: Vec::new(),
                    missing: Vec::new(),
                    install_options: Vec::new(),
                    primary_env: None,
                    configured_env: Vec::new(),
                    configured_config: Vec::new(),
                    os: Vec::new(),
                },
                AgentSkillInfo {
                    name: "github".to_string(),
                    description: String::new(),
                    enabled: true,
                    active: false,
                    eligible: false,
                    source: "workspace".to_string(),
                    bundled: false,
                    integrity: "ok".to_string(),
                    sha256: None,
                    capabilities: Vec::new(),
                    missing: vec!["bin:gh".to_string(), "env:GITHUB_TOKEN".to_string()],
                    install_options: vec![AgentSkillInstallOption {
                        id: "github-brew".to_string(),
                        kind: "brew".to_string(),
                        label: "gh".to_string(),
                        bins: vec!["gh".to_string()],
                        os: Vec::new(),
                    }],
                    primary_env: Some("GITHUB_TOKEN".to_string()),
                    configured_env: Vec::new(),
                    configured_config: Vec::new(),
                    os: Vec::new(),
                },
            ],
            "npm",
        );

        assert_eq!(response.summary.total, 2);
        assert_eq!(response.summary.eligible, 1);
        assert_eq!(response.summary.active, 1);
        assert_eq!(response.summary.missing_dependencies, 1);
        assert_eq!(response.summary.missing_configuration, 1);
        assert!(response.summary.configure_now);
        assert_eq!(response.summary.installable, 1);
    }

    #[test]
    fn build_tool_policy_state_applies_session_override_last() {
        let mut cfg = crate::config::AgentUiConfig::default();
        cfg.tool_policy.profile = "minimal".to_string();
        cfg.security.tool_allowlist = vec!["exec".to_string()];
        cfg.tool_policy.session_overrides.insert(
            "session-a".to_string(),
            crate::config::AgentToolScopeOverride {
                allow: Vec::new(),
                deny: vec!["exec".to_string()],
            },
        );

        let effective = mlx_agent_core::resolve_effective_tool_policy(
            &build_tool_policy_state(&cfg, Some("session-a"), None),
            DEFAULT_AGENT_ID,
            Some("session-a"),
        );
        let exec = effective
            .entries
            .into_iter()
            .find(|entry| entry.name == "exec")
            .unwrap();

        assert!(!exec.allowed);
        assert_eq!(exec.final_rule, "session:session-a:deny:exec");
    }

    #[test]
    fn sync_legacy_enabled_tools_matches_effective_policy() {
        let mut cfg = crate::config::AgentUiConfig::default();
        cfg.tool_policy.profile = "messaging".to_string();
        cfg.tool_policy.agent_overrides.insert(
            DEFAULT_AGENT_ID.to_string(),
            crate::config::AgentToolScopeOverride {
                allow: mlx_agent_core::profile_tool_names(
                    mlx_agent_core::ToolProfileName::Messaging,
                )
                .into_iter()
                .collect(),
                deny: Vec::new(),
            },
        );

        sync_legacy_enabled_tools(&mut cfg);

        assert!(cfg.enabled_tools.iter().any(|tool| tool == "message"));
        assert!(!cfg.enabled_tools.iter().any(|tool| tool == "exec"));
    }

    #[test]
    fn enable_disable_roundtrips_after_restart() {
        let _guard = env_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");
        std::env::set_var("APP_SETTINGS_PATH", &settings_path);

        let mut cfg = crate::config::AppConfig::default();
        cfg.agent.enabled_skills = vec!["obsidian".to_string()];
        cfg.agent.node_package_manager = "pnpm".to_string();
        cfg.save_settings_to(&settings_path).unwrap();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let _ = agent_disable_skills(Json(AgentSkillToggleRequest {
                skill: Some("obsidian".to_string()),
                skills: Vec::new(),
            }))
            .await
            .unwrap();
            let _ = agent_enable_skills(Json(AgentSkillToggleRequest {
                skill: Some("gog".to_string()),
                skills: Vec::new(),
            }))
            .await
            .unwrap();
        });

        let restarted = crate::config::AppConfig::load_settings_from(&settings_path);
        assert_eq!(restarted.agent.node_package_manager, "pnpm");
        assert_eq!(
            restarted
                .agent
                .skill_overrides
                .get("obsidian")
                .and_then(|entry| entry.enabled),
            Some(false)
        );
        assert_eq!(
            restarted
                .agent
                .skill_overrides
                .get("gog")
                .and_then(|entry| entry.enabled),
            Some(true)
        );

        std::env::remove_var("APP_SETTINGS_PATH");
    }

    #[test]
    fn apply_skill_config_update_uses_vault_for_secret_env() {
        let dir = tempdir().unwrap();
        let settings_dir = dir.path().join("settings");
        std::fs::create_dir_all(&settings_dir).unwrap();
        let mut agent_cfg = crate::config::AgentUiConfig::default();
        agent_cfg.security.use_secrets_vault = true;

        apply_skill_config_update(
            &settings_dir,
            &mut agent_cfg,
            &AgentSkillConfigRequest {
                skill: "github".to_string(),
                enabled: Some(true),
                env: BTreeMap::from([("GITHUB_TOKEN".to_string(), "ghp_test_secret".to_string())]),
                clear_env: Vec::new(),
                config: BTreeMap::new(),
                clear_config: Vec::new(),
            },
        )
        .unwrap();

        let override_entry = agent_cfg.skill_overrides.get("github").unwrap();
        assert_eq!(override_entry.enabled, Some(true));
        assert!(override_entry.env.get("GITHUB_TOKEN").is_none());
        let reference = override_entry.env_refs.get("GITHUB_TOKEN").unwrap();
        assert!(reference.starts_with("vault://"));

        let vault = crate::secrets_vault::SecretsVault::open(&settings_dir).unwrap();
        let secret = vault
            .get_secret(reference.trim_start_matches("vault://"))
            .unwrap();
        assert_eq!(secret.as_deref(), Some("ghp_test_secret"));
    }

    #[test]
    fn run_install_command_reports_permission_failures() {
        let dir = tempdir().unwrap();
        let blocked = dir.path().join("blocked-command");
        std::fs::write(&blocked, "#!/bin/sh\necho nope\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&blocked).unwrap().permissions();
            perms.set_mode(0o644);
            std::fs::set_permissions(&blocked, perms).unwrap();
        }

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let result = runtime.block_on(run_install_command(blocked.to_string_lossy().as_ref(), &[]));
        assert!(!result.ok);
        assert!(result.code.is_none());
        assert!(result.stderr.to_ascii_lowercase().contains("permission"));
    }
}
