//! `/agent/*` endpoints — full agent runtime API.

use crate::secrets_vault::SecretsVault;
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use bytes::Bytes;
use http_llm_provider::{HttpApiKind, HttpLlmProvider, HttpLlmProviderConfig};
use mlx_agent_core::approval::{
    ApprovalDecision, ApprovalMode, ApprovalService, DefaultApprovalService,
};
use mlx_agent_core::audit::{AuditLog, AuditLogEntry};
use mlx_agent_core::capabilities::{
    CapabilityAuthority, CapabilityBinding, CapabilityManifest, CapabilityScopeRules,
    CapabilityScopes, CapabilitySubject,
};
use mlx_agent_core::events::{AgentEvent, EventBus};
use mlx_agent_core::policy::{DefaultPolicyEngine, PolicyConfig, PolicyEngine};
use mlx_agent_core::registry::ToolRegistry;
use mlx_agent_core::{
    AgentError, AgentLoop, AgentLoopConfig, AgentRuntime, AgentRuntimeConfig, RuntimeVariant,
};
use mlx_agent_tools::ExecutionMode;
use mlx_ollama_core::{
    ChatMessage, FunctionDef, MessageRole, ModelProvider, RuntimeProviderConfig,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
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
    /// Runtime variant: classic | hermes_inspired.
    #[serde(default)]
    pub runtime_variant: Option<String>,
    /// Persist structured tool events in the session store.
    #[serde(default)]
    pub persist_tool_events: Option<bool>,
    /// Enable cross-session recall in Hermes-inspired runtime.
    #[serde(default)]
    pub session_search_enabled: Option<bool>,
    /// Memory hydration profile: minimal | balanced | full.
    #[serde(default)]
    pub memory_profile: Option<String>,
    /// Snapshot persistence mode for Hermes-inspired memory lifecycle.
    #[serde(default)]
    pub memory_snapshot_mode: Option<String>,
    /// Optional parent/delegation/session context envelope.
    #[serde(default)]
    pub session_context: Option<mlx_agent_core::SessionContextEnvelope>,
    /// Optional gateway metadata merged into the session context envelope.
    #[serde(default)]
    pub gateway_context: Option<mlx_agent_core::GatewayContext>,
    /// Current delegation depth for nested runs.
    #[serde(default)]
    pub delegate_depth: Option<usize>,
    /// Optional enabled skills.
    #[serde(default)]
    pub enabled_skills: Option<Vec<String>>,
    /// Optional enabled tools.
    #[serde(default)]
    pub enabled_tools: Option<Vec<String>>,
    /// Optional named toolset to constrain runtime tools for this request.
    #[serde(default)]
    pub toolset_id: Option<String>,
    /// Optional provider profile id to resolve local/remote provider settings.
    #[serde(default)]
    pub provider_profile_id: Option<String>,
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

#[derive(Debug, Serialize)]
struct AgentStreamFrame {
    event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    delta: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    completion_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_tokens: Option<usize>,
}

impl AgentStreamFrame {
    fn status(value: &str, session_id: Option<String>) -> Self {
        Self {
            event: "status".to_string(),
            status: Some(value.to_string()),
            delta: None,
            message: None,
            session_id,
            tool: None,
            latency_ms: None,
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
        }
    }

    fn thinking(delta: String, session_id: String) -> Self {
        Self {
            event: "thinking_delta".to_string(),
            status: None,
            delta: Some(delta),
            message: None,
            session_id: Some(session_id),
            tool: None,
            latency_ms: None,
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
        }
    }

    fn answer(delta: String, session_id: String) -> Self {
        Self {
            event: "answer_delta".to_string(),
            status: None,
            delta: Some(delta),
            message: None,
            session_id: Some(session_id),
            tool: None,
            latency_ms: None,
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
        }
    }

    fn tool(event: &str, session_id: String, tool: String, message: Option<String>) -> Self {
        Self {
            event: event.to_string(),
            status: None,
            delta: None,
            message,
            session_id: Some(session_id),
            tool: Some(tool),
            latency_ms: None,
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
        }
    }

    fn done(response: &AgentRunResponse) -> Self {
        Self {
            event: "done".to_string(),
            status: Some("completed".to_string()),
            delta: None,
            message: None,
            session_id: Some(response.session_id.clone()),
            tool: None,
            latency_ms: Some(response.latency_ms),
            prompt_tokens: Some(response.prompt_tokens),
            completion_tokens: Some(response.completion_tokens),
            total_tokens: Some(response.total_tokens),
        }
    }

    fn error(message: String, session_id: Option<String>) -> Self {
        Self {
            event: "error".to_string(),
            status: Some("error".to_string()),
            delta: None,
            message: Some(message),
            session_id,
            tool: None,
            latency_ms: None,
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
        }
    }
}

#[derive(Debug, Clone)]
struct CapabilityInventorySnapshot {
    provider: String,
    model_id: String,
    tools: Vec<(String, String)>,
    skills: Vec<(String, String)>,
    plugins: Vec<(String, String, String)>,
    supports_exec: bool,
    supports_web: bool,
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

#[derive(Debug, Serialize)]
pub struct AgentCompatSummary {
    pub total_checks: usize,
    pub passed_checks: usize,
    pub coverage_percent: f32,
    pub critical_gaps: usize,
    pub warning_gaps: usize,
}

#[derive(Debug, Serialize)]
pub struct AgentCompatMigrationStatus {
    pub schema_version: u32,
    pub current_schema_version: u32,
    pub migrated: bool,
    pub backward_compatible: bool,
    pub migration_flags: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AgentCompatGap {
    pub area: String,
    pub id: String,
    pub severity: String,
    pub message: String,
    pub action: String,
}

#[derive(Debug, Serialize)]
pub struct AgentCompatChannelEntry {
    pub id: String,
    pub name: String,
    pub state: String,
    pub local_testable: bool,
    pub requires_external_activation: bool,
    pub protocol_family: String,
    pub protocol_version: String,
    pub account_count: usize,
    pub configured_accounts: usize,
    pub healthy_accounts: usize,
    pub capabilities: Vec<String>,
    pub notes: Vec<String>,
    pub activation_checklist: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AgentCompatPluginEntry {
    pub id: String,
    pub name: String,
    pub state: String,
    pub enabled: bool,
    pub configured: bool,
    pub loaded: bool,
    pub health: String,
    pub capabilities: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AgentCompatSkillEntry {
    pub name: String,
    pub status: String,
    pub eligible: bool,
    pub enabled: bool,
    pub active: bool,
    pub integrity: String,
    pub installable: bool,
    pub missing: Vec<String>,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AgentCompatSkillsSection {
    pub subsystem_status: String,
    pub summary: AgentSkillsCheckSummary,
    pub entries: Vec<AgentCompatSkillEntry>,
}

#[derive(Debug, Serialize)]
pub struct AgentCompatToolProfileEntry {
    pub id: String,
    pub tools_in_profile: usize,
    pub implemented_tools: usize,
    pub allowed_tools: usize,
    pub blocked_tools: usize,
    pub coverage_percent: f32,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct AgentCompatToolsSection {
    pub selected_profile: String,
    pub profiles: Vec<AgentCompatToolProfileEntry>,
    pub effective_policy: mlx_agent_core::EffectiveToolPolicy,
}

#[derive(Debug, Serialize)]
pub struct AgentCompatEndpointEntry {
    pub method: String,
    pub path: String,
    pub status: String,
    pub backward_compatible: bool,
    pub notes: String,
}

#[derive(Debug, Serialize)]
pub struct AgentCompatContextBenchmark {
    pub provider_id: String,
    pub model_id: String,
    pub model_profile: String,
    pub tool_profile: String,
    pub status: String,
    pub max_prompt_tokens: usize,
    pub prompt_tokens_before_compression: usize,
    pub prompt_tokens_after_compression: usize,
    pub history_messages_total: usize,
    pub history_messages_used: usize,
    pub summarized_messages: usize,
    pub summary_entries: usize,
    pub tools_considered: usize,
    pub tools_in_prompt: usize,
    pub response_style: String,
    pub critical: bool,
    pub recommendation: String,
}

#[derive(Debug, Serialize)]
pub struct AgentCompatReport {
    pub mode: String,
    pub target: String,
    pub generated_at: String,
    pub coverage_methodology: String,
    pub summary: AgentCompatSummary,
    pub migration: AgentCompatMigrationStatus,
    pub channels: Vec<AgentCompatChannelEntry>,
    pub plugins: Vec<AgentCompatPluginEntry>,
    pub skills: AgentCompatSkillsSection,
    pub tools: AgentCompatToolsSection,
    pub endpoint_compatibility: Vec<AgentCompatEndpointEntry>,
    pub context_benchmark: AgentCompatContextBenchmark,
    pub gaps: Vec<AgentCompatGap>,
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
const COMPAT_ENDPOINTS: &[(&str, &str, &str)] = &[
    (
        "GET",
        "/agent/config",
        "existing agent configuration contract preserved",
    ),
    (
        "POST",
        "/agent/config",
        "existing configuration mutation contract preserved",
    ),
    ("GET", "/agent/skills", "skill catalog listing preserved"),
    (
        "GET",
        "/agent/skills/check",
        "skill eligibility inspection preserved",
    ),
    (
        "POST",
        "/agent/skills/install",
        "skill dependency install flow preserved",
    ),
    (
        "POST",
        "/agent/skills/enable",
        "skill toggle flow preserved",
    ),
    (
        "POST",
        "/agent/skills/disable",
        "skill toggle flow preserved",
    ),
    ("GET", "/agent/tools", "effective tool list preserved"),
    (
        "GET",
        "/agent/tools/catalog",
        "tool catalog contract preserved",
    ),
    (
        "GET",
        "/agent/tools/effective-policy",
        "effective policy endpoint preserved",
    ),
    (
        "POST",
        "/agent/tools/profile",
        "profile switch endpoint preserved",
    ),
    (
        "POST",
        "/agent/tools/allow-deny",
        "allow/deny mutation preserved",
    ),
    ("GET", "/agent/plugins", "plugin inventory preserved"),
    (
        "POST",
        "/agent/plugins/enable",
        "plugin enable mutation preserved",
    ),
    (
        "POST",
        "/agent/plugins/disable",
        "plugin disable mutation preserved",
    ),
    ("GET", "/agent/channels", "channel inventory preserved"),
    (
        "GET",
        "/agent/channels/catalog",
        "channel catalog preserved",
    ),
    (
        "POST",
        "/agent/channels/upsert-account",
        "channel account mutation preserved",
    ),
    (
        "POST",
        "/agent/channels/login",
        "channel login flow preserved",
    ),
    (
        "POST",
        "/agent/channels/logout",
        "channel logout flow preserved",
    ),
    (
        "POST",
        "/agent/channels/probe",
        "channel probe flow preserved",
    ),
    (
        "GET",
        "/agent/channels/status",
        "channel status listing preserved",
    ),
    (
        "GET",
        "/agent/channels/capabilities",
        "channel capabilities preserved",
    ),
    (
        "GET",
        "/agent/context/budget",
        "budget telemetry endpoint preserved",
    ),
    ("POST", "/agent/run", "agent loop execution preserved"),
];

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

fn selected_toolset_id(
    agent_cfg: &super::config::AgentUiConfig,
    request: &AgentRunRequest,
) -> String {
    request
        .toolset_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(agent_cfg.default_toolset_id.as_str())
        .trim()
        .to_string()
}

fn resolve_toolset_profile(
    agent_cfg: &super::config::AgentUiConfig,
    request: &AgentRunRequest,
) -> mlx_agent_core::ToolsetProfile {
    mlx_agent_core::toolset_profile(&selected_toolset_id(agent_cfg, request)).unwrap_or_else(|| {
        mlx_agent_core::toolset_profile("general")
            .unwrap_or_else(|| mlx_agent_core::toolset_profiles()[0].clone())
    })
}

fn allowed_tool_subset(
    agent_cfg: &super::config::AgentUiConfig,
    request: &AgentRunRequest,
    toolset: &mlx_agent_core::ToolsetProfile,
) -> BTreeSet<String> {
    let mut allowed = toolset
        .enabled_tools
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let config_allowed = effective_enabled_tools(agent_cfg)
        .into_iter()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    allowed = allowed
        .intersection(&config_allowed)
        .cloned()
        .collect::<BTreeSet<_>>();

    if let Some(requested) = request
        .enabled_tools
        .as_ref()
        .filter(|items| !items.is_empty())
    {
        let requested = requested.iter().cloned().collect::<BTreeSet<_>>();
        allowed = allowed.intersection(&requested).cloned().collect();
    }

    allowed
}

fn resolve_provider_profile<'a>(
    agent_cfg: &'a super::config::AgentUiConfig,
    request: &AgentRunRequest,
) -> Option<&'a super::config::AgentProviderProfileConfig> {
    let selected = request
        .provider_profile_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            if agent_cfg.provider_profile_id.trim().is_empty() {
                None
            } else {
                Some(agent_cfg.provider_profile_id.as_str())
            }
        })?;

    agent_cfg
        .provider_profiles
        .iter()
        .find(|profile| profile.id.eq_ignore_ascii_case(selected))
}

fn merged_session_context(
    request: &AgentRunRequest,
) -> Option<mlx_agent_core::SessionContextEnvelope> {
    let mut context = request.session_context.clone().unwrap_or_default();
    if let Some(gateway) = request.gateway_context.as_ref() {
        if context.source_channel.trim().is_empty() {
            context.source_channel = gateway.source_channel.clone();
        }
        if context.thread_id.trim().is_empty() {
            context.thread_id = gateway.thread_id.clone();
        }
        if context.sender_id.trim().is_empty() {
            context.sender_id = gateway.sender_id.clone();
        }
        if context.correlation_id.trim().is_empty() {
            context.correlation_id = gateway.correlation_id.clone();
        }
    }

    if context.origin_kind.trim().is_empty()
        && context.parent_session_id.is_none()
        && context.metadata.is_empty()
        && context.source_channel.trim().is_empty()
        && context.thread_id.trim().is_empty()
        && context.sender_id.trim().is_empty()
        && context.correlation_id.trim().is_empty()
    {
        None
    } else {
        Some(context)
    }
}

fn build_delegate_system_prompt(
    goal: Option<&str>,
    handoff_summary: Option<&str>,
    toolset_id: &str,
) -> String {
    let mut lines = vec![
        "You are a delegated MLX-Pilot subagent.".to_string(),
        format!("Use only the active toolset: {toolset_id}."),
        "Return a concise completion summary for the parent session.".to_string(),
    ];
    if let Some(goal) = goal.filter(|value| !value.trim().is_empty()) {
        lines.push(format!("Goal: {}", goal.trim()));
    }
    if let Some(summary) = handoff_summary.filter(|value| !value.trim().is_empty()) {
        lines.push(format!("Parent handoff: {}", summary.trim()));
    }
    lines.join("\n")
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
        .filter(|message| {
            matches!(
                message.kind.trim().to_ascii_lowercase().as_str(),
                "user" | "assistant" | "tool_result" | "message"
            )
        })
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
        source_session_id: artifact.session_id.clone(),
        scope: "session".to_string(),
        namespace: "history".to_string(),
        kind: artifact
            .metadata
            .get("kind")
            .cloned()
            .unwrap_or_else(|| "history_summary".to_string()),
        title: artifact.title.clone(),
        content: artifact.content.clone(),
        tags: Vec::new(),
        created_at: artifact.created_at,
        metadata: artifact.metadata.clone(),
        importance: 25,
        last_accessed_at: Some(chrono::Utc::now()),
        pin_state: "auto".to_string(),
        promotion_source: "summary_artifact".to_string(),
        summary_ref: artifact.session_id.clone(),
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

fn parse_runtime_variant(value: Option<&str>) -> RuntimeVariant {
    match value
        .map(str::trim)
        .unwrap_or("classic")
        .to_ascii_lowercase()
        .as_str()
    {
        "hermes" | "hermes_inspired" | "hermes-inspired" => RuntimeVariant::HermesInspired,
        _ => RuntimeVariant::Classic,
    }
}

fn parse_memory_snapshot_mode(value: Option<&str>) -> mlx_agent_core::MemorySnapshotMode {
    match value
        .map(str::trim)
        .unwrap_or("session")
        .to_ascii_lowercase()
        .as_str()
    {
        "off" | "disabled" => mlx_agent_core::MemorySnapshotMode::Off,
        _ => mlx_agent_core::MemorySnapshotMode::Session,
    }
}

fn build_policy_config(
    cfg: &super::config::AgentUiConfig,
    mode: ExecutionMode,
    workspace_root: &Path,
    known_skill_hashes: BTreeMap<String, String>,
    tool_policy: mlx_agent_core::ToolPolicyState,
    session_id: Option<&str>,
    allowed_tool_subset: BTreeSet<String>,
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
        allowed_tool_subset,
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
        BTreeSet::new(),
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

fn percent_ratio(passed: usize, total: usize) -> f32 {
    if total == 0 {
        return 100.0;
    }
    ((passed as f32 / total as f32) * 1000.0).round() / 10.0
}

fn is_capability_inventory_request(message: &str) -> bool {
    let text = message.trim().to_ascii_lowercase();
    if text.is_empty() {
        return false;
    }

    let inventory_nouns = [
        "tool",
        "tools",
        "skill",
        "skills",
        "plugin",
        "plugins",
        "capability",
        "capabilities",
        "ferramenta",
        "ferramentas",
        "habilidade",
        "habilidades",
        "capacidade",
        "capacidades",
    ];
    let inventory_verbs = [
        "quais",
        "qual",
        "liste",
        "listar",
        "list",
        "show",
        "mostre",
        "tem",
        "possui",
        "available",
        "disponiveis",
        "disponíveis",
        "what do you have",
    ];
    let identity_clues = ["qual seu nome", "quem e voce", "quem é você", "who are you"];
    let python_clues = [
        "rodar python",
        "run python",
        "executar python",
        "python tool",
        "python?",
    ];
    let web_clues = [
        "pesquisa na web",
        "research na web",
        "search na web",
        "internet",
        "web search",
        "browse",
        "browser",
        "pesquisa web",
    ];

    let has_inventory_noun = inventory_nouns.iter().any(|term| text.contains(term));
    let has_inventory_verb = inventory_verbs.iter().any(|term| text.contains(term));
    let asks_identity = identity_clues.iter().any(|term| text.contains(term));
    let asks_python = python_clues.iter().any(|term| text.contains(term));
    let asks_web = web_clues.iter().any(|term| text.contains(term));

    (has_inventory_noun && (has_inventory_verb || asks_identity)) || asks_python || asks_web
}

fn render_capability_inventory_markdown(snapshot: &CapabilityInventorySnapshot) -> String {
    let tools = snapshot
        .tools
        .iter()
        .map(|(name, description)| format!("- `{name}` - {description}"))
        .collect::<Vec<_>>();
    let skills = if snapshot.skills.is_empty() {
        vec!["- Nenhuma skill ativa no momento.".to_string()]
    } else {
        snapshot
            .skills
            .iter()
            .map(|(name, description)| format!("- `{name}` - {description}"))
            .collect::<Vec<_>>()
    };
    let plugins = if snapshot.plugins.is_empty() {
        vec!["- Nenhum plugin registrado no runtime.".to_string()]
    } else {
        snapshot
            .plugins
            .iter()
            .map(|(name, status, capabilities)| {
                if capabilities.is_empty() {
                    format!("- `{name}` - status `{status}`")
                } else {
                    format!("- `{name}` - status `{status}` - caps: {capabilities}")
                }
            })
            .collect::<Vec<_>>()
    };

    let python_status = if snapshot.supports_exec {
        "Sim, via `exec` para shell/PowerShell/Python dentro do workspace."
    } else {
        "Nao. A tool `exec` nao esta liberada neste perfil."
    };
    let web_status = if snapshot.supports_web {
        "Sim. Existe pelo menos uma tool de busca/browse ativa."
    } else {
        "Nao. O Agent ainda nao expoe uma tool nativa de busca web/browser."
    };

    [
        "**Nome:** MLX-Pilot Agent".to_string(),
        format!(
            "**Runtime atual:** provider `{}` com modelo `{}`",
            snapshot.provider, snapshot.model_id
        ),
        String::new(),
        format!("**Skills ativas ({})**", snapshot.skills.len()),
        skills.join("\n"),
        String::new(),
        format!("**Plugins registrados ({})**", snapshot.plugins.len()),
        plugins.join("\n"),
        String::new(),
        format!("**Tools ativas ({})**", snapshot.tools.len()),
        tools.join("\n"),
        String::new(),
        "**Capacidades rapidas**".to_string(),
        format!("- Python/exec: {python_status}"),
        format!("- Pesquisa na web: {web_status}"),
    ]
    .join("\n")
}

async fn persist_agent_exchange(
    state: &super::AppState,
    session_id: &str,
    user_message: &str,
    assistant_message: &str,
) {
    let _ = state.session_store.ensure_session(session_id, None).await;
    let _ = state
        .session_store
        .append(
            session_id,
            &mlx_agent_core::session::SessionMessage::user(user_message.to_string()),
        )
        .await;
    let _ = state
        .session_store
        .append(
            session_id,
            &mlx_agent_core::session::SessionMessage::assistant(assistant_message.to_string()),
        )
        .await;
}

async fn build_capability_inventory_snapshot(
    state: &super::AppState,
    cfg: &super::config::AppConfig,
    agent_cfg: &super::config::AgentUiConfig,
    request: &AgentRunRequest,
    provider: &str,
    model_id: &str,
    session_id: &str,
) -> Result<CapabilityInventorySnapshot, AgentApiError> {
    let policy = build_tool_policy_state(
        agent_cfg,
        Some(session_id),
        request.enabled_tools.as_deref(),
    );
    let toolset = resolve_toolset_profile(agent_cfg, request);
    let allowed_subset = allowed_tool_subset(agent_cfg, request, &toolset);
    let effective_tools =
        mlx_agent_core::resolve_effective_tool_policy(&policy, DEFAULT_AGENT_ID, Some(session_id));
    let mut tools = effective_tools
        .entries
        .into_iter()
        .filter(|entry| {
            entry.allowed
                && entry.implemented
                && (allowed_subset.is_empty() || allowed_subset.contains(&entry.name))
        })
        .map(|entry| (entry.name, entry.description))
        .collect::<Vec<_>>();
    tools.sort_by(|left, right| left.0.cmp(&right.0));

    let supports_exec = tools.iter().any(|(name, _)| name == "exec");
    let supports_web = tools.iter().any(|(name, description)| {
        name.contains("web")
            || name.contains("browser")
            || description.to_ascii_lowercase().contains("web")
            || description.to_ascii_lowercase().contains("browser")
    });

    let requested_skills = request
        .enabled_skills
        .as_ref()
        .filter(|values| !values.is_empty())
        .map(|values| enabled_set(values));
    let node_manager = normalize_node_manager(None, &cfg.agent.node_package_manager);
    let skills_catalog = load_skill_catalog(state, cfg).await?;
    let mut skills = build_skills_check_response(skills_catalog.items, &node_manager)
        .skills
        .into_iter()
        .filter(|skill| {
            if let Some(requested) = requested_skills.as_ref() {
                skill.eligible && requested.contains(&normalize_skill_name(&skill.name))
            } else {
                skill.active
            }
        })
        .map(|skill| (skill.name, skill.description))
        .collect::<Vec<_>>();
    skills.sort_by(|left, right| left.0.cmp(&right.0));

    let mut plugins = state
        .plugin_manager
        .list_plugins()
        .await
        .into_iter()
        .map(|plugin| {
            let status = plugin_health_status(&plugin);
            let capabilities = plugin.capabilities.join(", ");
            (plugin.id, status, capabilities)
        })
        .collect::<Vec<_>>();
    plugins.sort_by(|left, right| left.0.cmp(&right.0));

    Ok(CapabilityInventorySnapshot {
        provider: provider.to_string(),
        model_id: model_id.to_string(),
        tools,
        skills,
        plugins,
        supports_exec,
        supports_web,
    })
}

fn classify_channel_support(
    channel: &crate::channels::ChannelView,
) -> (String, bool, bool, Vec<String>, Vec<String>) {
    let protocol_family = channel.protocol_family.trim().to_ascii_lowercase();
    let has_bridge = protocol_family.contains("bridge");
    let has_webhook = protocol_family.contains("webhook");
    let has_bot_token = channel.capabilities.iter().any(|cap| cap == "bot-token");
    let has_qr = channel.capabilities.iter().any(|cap| cap == "qr-login");
    let has_probe = channel.capabilities.iter().any(|cap| cap == "probe");

    let mut notes = Vec::new();
    let mut activation_checklist = Vec::new();

    if has_bridge || has_webhook {
        notes.push(
            "Adapter completo disponível; a ativação real depende de bridge/webhook externo."
                .to_string(),
        );
        activation_checklist.push("Provisionar o bridge/webhook real do conector.".to_string());
        activation_checklist
            .push("Configurar endpoint/token no account ou adapter_config.".to_string());
        activation_checklist
            .push("Executar /agent/channels/login e /probe no ambiente alvo.".to_string());
        return (
            "adapter_ready_external".to_string(),
            false,
            true,
            notes,
            activation_checklist,
        );
    }

    if has_bot_token {
        notes.push("Fluxo validável localmente com credenciais do tipo bot-token.".to_string());
    }
    if has_qr {
        notes.push(
            "Fluxo local baseado em sessão/QR disponível para onboarding e probe.".to_string(),
        );
    }
    if has_probe {
        notes.push("Probe de saúde suportado pelo adapter.".to_string());
    }

    (
        "supported_local".to_string(),
        true,
        false,
        notes,
        activation_checklist,
    )
}

fn plugin_health_status(plugin: &crate::plugins::PluginView) -> String {
    if !plugin.errors.is_empty() {
        return "degraded".to_string();
    }
    if plugin.loaded {
        return "loaded".to_string();
    }
    if plugin.enabled {
        return "enabled".to_string();
    }
    "disabled".to_string()
}

fn skill_status(skill: &AgentSkillInfo) -> String {
    if skill.integrity == "blocked" {
        return "blocked_integrity".to_string();
    }
    if skill.active {
        return "active".to_string();
    }
    if skill.eligible {
        return "eligible".to_string();
    }
    if skill
        .missing
        .iter()
        .any(|item| item.starts_with("env:") || item.starts_with("config:"))
    {
        return "missing_configuration".to_string();
    }
    if skill.missing.iter().any(|item| {
        item.starts_with("bin:") || item.starts_with("anyBin:") || item.starts_with("os:")
    }) {
        return "missing_dependencies".to_string();
    }
    "pending".to_string()
}

fn build_tool_profile_entry(
    profile: mlx_agent_core::ToolProfileName,
    policy: &mlx_agent_core::ToolPolicyState,
) -> AgentCompatToolProfileEntry {
    let total_catalog = mlx_agent_core::tool_catalog();
    let tools_in_profile = total_catalog
        .iter()
        .filter(|entry| entry.enabled_in_profile(profile))
        .count();
    let implemented_tools = total_catalog
        .iter()
        .filter(|entry| entry.enabled_in_profile(profile) && entry.implemented)
        .count();
    let effective = mlx_agent_core::resolve_effective_tool_policy(policy, DEFAULT_AGENT_ID, None);
    let profile_tools = total_catalog
        .iter()
        .filter(|entry| entry.enabled_in_profile(profile))
        .map(|entry| entry.name.clone())
        .collect::<BTreeSet<_>>();
    let allowed_tools = effective
        .entries
        .iter()
        .filter(|entry| entry.allowed && entry.implemented && profile_tools.contains(&entry.name))
        .count();
    let blocked_tools = tools_in_profile.saturating_sub(allowed_tools);

    AgentCompatToolProfileEntry {
        id: profile.as_str().to_string(),
        tools_in_profile,
        implemented_tools,
        allowed_tools,
        blocked_tools,
        coverage_percent: percent_ratio(implemented_tools, tools_in_profile),
        status: if implemented_tools == tools_in_profile {
            "covered".to_string()
        } else {
            "partial".to_string()
        },
    }
}

fn synthetic_context_messages() -> Vec<ChatMessage> {
    let mut messages = Vec::new();
    for index in 0..10 {
        messages.push(ChatMessage::text(
            MessageRole::User,
            format!(
                "Iteracao {index}: revisar onboarding do agente Hermes, mapear gaps de channel/plugin/skill/tool e manter um resumo objetivo do estado do sistema."
            ),
        ));
        messages.push(ChatMessage::text(
            MessageRole::Assistant,
            format!(
                "Resumo {index}: channels ativos, plugins monitorados, skills elegiveis e politica efetiva registradas. Continuar reduzindo contexto sem perder os checks criticos."
            ),
        ));
    }
    messages.push(ChatMessage::text(
        MessageRole::User,
        "Executar o agent loop local com foco em budget, compressao e politica efetiva para um modelo pequeno."
            .to_string(),
    ));
    messages
}

fn synthetic_context_tools() -> Vec<FunctionDef> {
    mlx_agent_core::tool_catalog()
        .into_iter()
        .filter(|entry| entry.implemented)
        .map(|entry| FunctionDef {
            name: entry.name,
            description: entry.description,
            parameters: serde_json::json!({
                "type": "object",
                "additionalProperties": true,
                "properties": {
                    "input": { "type": "string" }
                }
            }),
        })
        .collect()
}

fn build_context_benchmark(
    agent_cfg: &super::config::AgentUiConfig,
) -> AgentCompatContextBenchmark {
    let provider_id = "ollama";
    let model_id = "qwen2.5-coder:7b";
    let tool_profile = parse_tool_profile(Some(agent_cfg.tool_policy.profile.as_str()));
    let profile = mlx_agent_core::select_model_prompt_profile(provider_id, model_id);
    let tools = synthetic_context_tools();
    let conversation = synthetic_context_messages();
    let skill_summaries = vec![
        "compat-report: matriz automatizada e regressao dos endpoints antigos".to_string(),
        "onboarding: setup nao interativo com channels/plugins/skills/tools".to_string(),
        "runtime: benchmark de contexto para modelos locais pequenos".to_string(),
    ];
    let budget_manager = mlx_agent_core::ContextBudgetManager;
    let budget = budget_manager.build(mlx_agent_core::ContextBudgetInput {
        session_id: "compat-benchmark",
        provider_id,
        model_id,
        tool_profile,
        execution_mode: parse_execution_mode(Some(agent_cfg.execution_mode.as_str())),
        profile: &profile,
        system_prompt_override: None,
        conversation: &conversation,
        skill_summaries: &skill_summaries,
        tools: &tools,
        aggressive_tool_filtering: agent_cfg.aggressive_tool_filtering,
    });

    let telemetry = budget.telemetry;
    AgentCompatContextBenchmark {
        provider_id: telemetry.provider_id.clone(),
        model_id: telemetry.model_id.clone(),
        model_profile: telemetry.model_profile.clone(),
        tool_profile: telemetry.tool_profile.clone(),
        status: if telemetry.critical {
            "tight".to_string()
        } else {
            "ok".to_string()
        },
        max_prompt_tokens: telemetry.max_prompt_tokens,
        prompt_tokens_before_compression: telemetry.prompt_tokens_before_compression,
        prompt_tokens_after_compression: telemetry.prompt_tokens_estimate,
        history_messages_total: telemetry.history_messages_total,
        history_messages_used: telemetry.history_messages_used,
        summarized_messages: telemetry.summarized_messages,
        summary_entries: telemetry.summary_entries,
        tools_considered: telemetry.tools_considered,
        tools_in_prompt: telemetry.tools_in_prompt,
        response_style: format!("{:?}", telemetry.response_style).to_ascii_lowercase(),
        critical: telemetry.critical,
        recommendation: if telemetry.critical {
            "Reduzir ferramentas expostas e manter profile small_local com compressao agressiva."
                .to_string()
        } else {
            "Budget adequado para loops locais curtos com compressao automatica.".to_string()
        },
    }
}

fn compatibility_endpoints() -> Vec<AgentCompatEndpointEntry> {
    COMPAT_ENDPOINTS
        .iter()
        .map(|(method, path, notes)| AgentCompatEndpointEntry {
            method: (*method).to_string(),
            path: (*path).to_string(),
            status: "preserved".to_string(),
            backward_compatible: true,
            notes: (*notes).to_string(),
        })
        .collect()
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

#[derive(Clone)]
struct DelegateExecutor {
    state: super::AppState,
    agent_cfg: super::config::AgentUiConfig,
    resolved: ResolvedProvider,
    workspace: PathBuf,
    runtime_variant: RuntimeVariant,
    persist_tool_events: bool,
    session_search_enabled: bool,
    memory_profile: String,
    memory_snapshot_mode: String,
    delegate_depth: usize,
    enabled_tools: Vec<String>,
    toolset_id: String,
}

#[async_trait::async_trait]
impl crate::agent_runtime_tools::DelegateSessionExecutor for DelegateExecutor {
    async fn execute(
        &self,
        request: mlx_agent_core::DelegateTaskRequest,
        ctx: &mlx_agent_tools::ToolContext,
    ) -> Result<serde_json::Value, String> {
        if self.delegate_depth >= 1 {
            return Err(
                "delegate depth exceeded (grandchildren are disabled in this cycle)".to_string(),
            );
        }

        let child_request = AgentRunRequest {
            session_id: Some(mlx_agent_core::SessionStore::new_session_id()),
            message: request.prompt,
            provider: Some(self.resolved.provider_name.clone()),
            model_id: Some(self.resolved.model_id.clone()),
            api_key: None,
            base_url: self
                .resolved
                .runtime
                .as_ref()
                .and_then(|runtime| runtime.base_url.clone()),
            custom_headers: self
                .resolved
                .runtime
                .as_ref()
                .map(|runtime| runtime.headers.clone()),
            streaming: Some(false),
            fallback_enabled: Some(false),
            fallback_provider: None,
            fallback_model_id: None,
            execution_mode: Some("full".to_string()),
            approval_mode: Some(self.agent_cfg.approval_mode.clone()),
            system_prompt: Some(build_delegate_system_prompt(
                request.goal.as_deref(),
                request.handoff_summary.as_deref(),
                request
                    .toolset_id
                    .as_deref()
                    .unwrap_or(self.toolset_id.as_str()),
            )),
            max_iterations: request.max_iterations.or(Some(16)),
            max_prompt_tokens: self.agent_cfg.max_prompt_tokens,
            max_history_messages: self.agent_cfg.max_history_messages,
            max_tools_in_prompt: self.agent_cfg.max_tools_in_prompt,
            temperature: self.agent_cfg.temperature,
            aggressive_tool_filtering: Some(self.agent_cfg.aggressive_tool_filtering),
            enable_tool_call_fallback: Some(self.agent_cfg.enable_tool_call_fallback),
            runtime_variant: Some(match self.runtime_variant {
                RuntimeVariant::HermesInspired => "hermes_inspired".to_string(),
                RuntimeVariant::Classic => "classic".to_string(),
            }),
            persist_tool_events: Some(self.persist_tool_events),
            session_search_enabled: Some(self.session_search_enabled),
            memory_profile: Some(self.memory_profile.clone()),
            memory_snapshot_mode: Some(self.memory_snapshot_mode.clone()),
            session_context: Some(mlx_agent_core::SessionContextEnvelope {
                origin_kind: "delegated".to_string(),
                parent_session_id: Some(ctx.session_id.clone()),
                source_channel: String::new(),
                thread_id: String::new(),
                sender_id: String::new(),
                correlation_id: String::new(),
                metadata: BTreeMap::from([(
                    "workspace".to_string(),
                    ctx.workspace_root.display().to_string(),
                )]),
            }),
            gateway_context: None,
            delegate_depth: Some(self.delegate_depth + 1),
            enabled_skills: None,
            enabled_tools: Some(
                self.enabled_tools
                    .iter()
                    .filter(|tool| tool.as_str() != "delegate_session")
                    .cloned()
                    .collect(),
            ),
            toolset_id: Some(
                request
                    .toolset_id
                    .clone()
                    .unwrap_or_else(|| self.toolset_id.clone()),
            ),
            provider_profile_id: None,
            workspace_root: Some(self.workspace.display().to_string()),
        };

        let response = run_agent_once(
            &self.state,
            &self.agent_cfg,
            &child_request,
            self.resolved.clone(),
            self.workspace.clone(),
        )
        .await
        .map_err(|error| error.details.unwrap_or(error.error))?;

        self.state
            .agent_state
            .event_bus
            .emit(AgentEvent::SessionHandoff {
                parent_session_id: ctx.session_id.clone(),
                child_session_id: response.session_id.clone(),
                handoff_summary: request
                    .handoff_summary
                    .clone()
                    .unwrap_or_else(|| response.content.clone()),
            });

        Ok(json!({
            "session_id": response.session_id,
            "summary": response.content,
            "provider": response.provider,
            "model_id": response.model_id,
            "iterations": response.iterations,
            "tool_calls_made": response.tool_calls_made,
            "toolset_id": request.toolset_id.unwrap_or_else(|| self.toolset_id.clone()),
        }))
    }
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
    let normalized_model = normalize_agent_model_id(&provider_id, model_id);
    let model = normalized_model.trim();
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
            runtime,
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

fn normalize_agent_model_id(provider: &str, model_id: &str) -> String {
    let trimmed = model_id.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let stripped_suffix = trimmed
        .strip_suffix(" [Ollama]")
        .or_else(|| trimmed.strip_suffix(" [MLX]"))
        .or_else(|| trimmed.strip_suffix(" [llama.cpp]"))
        .unwrap_or(trimmed)
        .trim();

    match provider {
        "ollama" => stripped_suffix
            .strip_prefix("ollama::")
            .unwrap_or(stripped_suffix)
            .trim()
            .to_string(),
        "mlx" => stripped_suffix
            .strip_prefix("mlx::")
            .unwrap_or(stripped_suffix)
            .trim()
            .to_string(),
        "llamacpp" | "llama" | "llama.cpp" => stripped_suffix
            .strip_prefix("llama::")
            .unwrap_or(stripped_suffix)
            .trim()
            .to_string(),
        _ => stripped_suffix.to_string(),
    }
}

fn build_tool_registry(
    state: &super::AppState,
    agent_cfg: &super::config::AgentUiConfig,
    workspace_root: &Path,
    resolved: &ResolvedProvider,
    request: &AgentRunRequest,
    allowed_tools: &BTreeSet<String>,
    toolset: &mlx_agent_core::ToolsetProfile,
) -> Result<ToolRegistry, AgentApiError> {
    use mlx_agent_tools::{
        EditFileTool, ExecTool, GlobTool, GrepTool, ListDirTool, ReadFileTool, WriteFileTool,
    };

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ReadFileTool::new()));
    registry.register(Arc::new(WriteFileTool::new()));
    registry.register(Arc::new(EditFileTool::new()));
    registry.register(Arc::new(ListDirTool::new()));
    registry.register(Arc::new(GlobTool::new()));
    registry.register(Arc::new(GrepTool::new()));
    registry.register(Arc::new(ExecTool::new()));

    let runtime_variant = parse_runtime_variant(
        request
            .runtime_variant
            .as_deref()
            .or(Some(agent_cfg.runtime_variant.as_str())),
    );
    let delegate_executor = if runtime_variant == RuntimeVariant::HermesInspired {
        Some(Arc::new(DelegateExecutor {
            state: state.clone(),
            agent_cfg: agent_cfg.clone(),
            resolved: resolved.clone(),
            workspace: workspace_root.to_path_buf(),
            runtime_variant,
            persist_tool_events: request
                .persist_tool_events
                .unwrap_or(agent_cfg.persist_tool_events),
            session_search_enabled: request
                .session_search_enabled
                .unwrap_or(agent_cfg.session_search_enabled),
            memory_profile: request
                .memory_profile
                .clone()
                .unwrap_or_else(|| agent_cfg.memory_profile.clone()),
            memory_snapshot_mode: request
                .memory_snapshot_mode
                .clone()
                .unwrap_or_else(|| agent_cfg.memory_snapshot_mode.clone()),
            delegate_depth: request.delegate_depth.unwrap_or(0),
            enabled_tools: allowed_tools.iter().cloned().collect(),
            toolset_id: toolset.id.clone(),
        })
            as Arc<
                dyn crate::agent_runtime_tools::DelegateSessionExecutor,
            >)
    } else {
        None
    };

    crate::agent_runtime_tools::register_runtime_tools(
        &mut registry,
        &crate::agent_runtime_tools::RuntimeToolServices {
            sessions: state.session_store.clone(),
            channels: state.channel_service.clone(),
            memory: state.agent_state.memory.clone(),
            budget_tracker: state.agent_state.budget_tracker.clone(),
            delegate_executor,
        },
    );

    registry.retain(|name| allowed_tools.contains(name));

    if agent_cfg.security.require_capabilities {
        let mut authority = CapabilityAuthority::new();
        let manifest_id = if agent_cfg.security.capability_manifest_paths.is_empty() {
            let manifest = build_default_agent_capability_manifest(agent_cfg, workspace_root);
            authority
                .insert_manifest(manifest)
                .map_err(|error| {
                    AgentApiError::new(
                        StatusCode::BAD_REQUEST,
                        "invalid_capability_manifest",
                        Some(error.to_string()),
                    )
                })?
                .identifier
                .clone()
        } else {
            let mut first_id = None;
            for raw_path in &agent_cfg.security.capability_manifest_paths {
                let path = PathBuf::from(raw_path);
                let manifest = authority.load_manifest_path(&path).map_err(|error| {
                    AgentApiError::new(
                        StatusCode::BAD_REQUEST,
                        "invalid_capability_manifest",
                        Some(error.to_string()),
                    )
                })?;
                if first_id.is_none() {
                    first_id = Some(manifest.identifier.clone());
                }
            }
            first_id.unwrap_or_else(|| "agent-default".to_string())
        };

        registry.bind_capabilities(
            Arc::new(authority),
            CapabilityBinding {
                manifest_id,
                subject: CapabilitySubject::agent(),
            },
        );
    }

    Ok(registry)
}

fn build_default_agent_capability_manifest(
    agent_cfg: &super::config::AgentUiConfig,
    workspace_root: &Path,
) -> CapabilityManifest {
    let workspace = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/");
    let enabled_tools = effective_enabled_tools(agent_cfg);
    let has_fs_read = ["read_file", "list_dir", "glob", "grep", "checkpoints_list"]
        .iter()
        .any(|tool| enabled_tools.contains(*tool));
    let has_fs_write = ["write_file", "edit_file", "checkpoint_restore"]
        .iter()
        .any(|tool| enabled_tools.contains(*tool));
    let has_process = enabled_tools.contains("exec");

    let mut permissions = Vec::new();
    if has_fs_read {
        permissions.push("fs:read".to_string());
    }
    if has_fs_write {
        permissions.push("fs:write".to_string());
    }
    if has_process {
        permissions.push("process:spawn".to_string());
    }

    CapabilityManifest {
        identifier: "agent-default".to_string(),
        version: "1.0.0".to_string(),
        permissions,
        contexts: vec![mlx_agent_core::CapabilityContextKind::Agent],
        windows: Vec::new(),
        platforms: Vec::new(),
        scopes: CapabilityScopes {
            fs: CapabilityScopeRules {
                allow: vec![format!("{workspace}/**/*"), format!("{workspace}/*")],
                deny: Vec::new(),
            },
            process: CapabilityScopeRules {
                allow: agent_cfg.security.exec_safe_bins.clone(),
                deny: Vec::new(),
            },
            network: CapabilityScopeRules::default(),
        },
    }
}

fn effective_enabled_tools(agent_cfg: &super::config::AgentUiConfig) -> HashSet<&str> {
    agent_cfg
        .enabled_tools
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>()
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
    let provider_profile = resolve_provider_profile(agent_cfg, request).cloned();
    let runtime_variant = parse_runtime_variant(
        request
            .runtime_variant
            .as_deref()
            .or(provider_profile
                .as_ref()
                .map(|profile| profile.runtime_variant.as_str()))
            .or(Some(agent_cfg.runtime_variant.as_str())),
    );

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
    let toolset = resolve_toolset_profile(agent_cfg, request);
    let allowed_tools = allowed_tool_subset(agent_cfg, request, &toolset);
    let session_context = merged_session_context(request);
    let policy_config = build_policy_config(
        agent_cfg,
        mode,
        &workspace,
        known_skill_hashes,
        tool_policy,
        Some(&session_id),
        allowed_tools.clone(),
    );
    let policy: Arc<dyn PolicyEngine> = Arc::new(DefaultPolicyEngine::new(policy_config));

    let tool_registry = build_tool_registry(
        state,
        &agent_cfg,
        &workspace,
        &resolved,
        request,
        &allowed_tools,
        &toolset,
    )?;

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

    let loop_config = AgentLoopConfig {
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

    let _ = state
        .session_store
        .ensure_session_with_meta(mlx_agent_core::session::SessionMeta {
            id: session_id.clone(),
            name: "Nova conversa".to_string(),
            updated_at: chrono::Utc::now(),
            last_activity_at: chrono::Utc::now(),
            message_count: 0,
            provider_id: resolved.provider_name.clone(),
            model_id: resolved.model_id.clone(),
            workspace_root: workspace.display().to_string(),
            origin_kind: session_context
                .as_ref()
                .map(|context| context.origin_kind.clone())
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "local".to_string()),
            parent_session_id: session_context
                .as_ref()
                .and_then(|context| context.parent_session_id.clone()),
            status: "active".to_string(),
            created_at: chrono::Utc::now(),
            summary: String::new(),
            source_channel: session_context
                .as_ref()
                .map(|context| context.source_channel.clone())
                .unwrap_or_default(),
            thread_id: session_context
                .as_ref()
                .map(|context| context.thread_id.clone())
                .unwrap_or_default(),
            correlation_id: session_context
                .as_ref()
                .map(|context| context.correlation_id.clone())
                .unwrap_or_default(),
        })
        .await;

    let response = match runtime_variant {
        RuntimeVariant::Classic => {
            let mut agent = AgentLoop::new(
                loop_config,
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
                    &mlx_agent_core::session::SessionMessage::user(request.message.clone()),
                )
                .await;

            let _ = state
                .session_store
                .append(
                    &session_id,
                    &mlx_agent_core::session::SessionMessage::assistant(response.content.clone()),
                )
                .await;

            response
        }
        RuntimeVariant::HermesInspired => {
            let runtime = AgentRuntime::new(
                AgentRuntimeConfig {
                    variant: RuntimeVariant::HermesInspired,
                    persist_tool_events: request
                        .persist_tool_events
                        .unwrap_or(agent_cfg.persist_tool_events),
                    memory_profile: request
                        .memory_profile
                        .clone()
                        .unwrap_or_else(|| agent_cfg.memory_profile.clone()),
                    session_search_enabled: request
                        .session_search_enabled
                        .unwrap_or(agent_cfg.session_search_enabled),
                    delegate_depth: request.delegate_depth.unwrap_or(0),
                    session_context,
                    session_name: None,
                    toolset_id: toolset.id.clone(),
                    memory_snapshot_mode: parse_memory_snapshot_mode(
                        request
                            .memory_snapshot_mode
                            .as_deref()
                            .or(Some(agent_cfg.memory_snapshot_mode.as_str())),
                    ),
                },
                state.session_store.clone(),
                state.agent_state.memory.clone(),
                state.agent_state.event_bus.clone(),
            );

            runtime
                .run(
                    loop_config,
                    &resolved.provider_name,
                    resolved.provider,
                    tool_registry,
                    skill_runtime,
                    policy,
                    state.agent_state.approval.clone(),
                    state.agent_state.audit.clone(),
                    request.message.trim(),
                )
                .await
                .map(|result| result.response)
                .map_err(AgentApiError::from_agent_error)?
        }
    };

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

async fn execute_agent_request(
    state: &super::AppState,
    request: AgentRunRequest,
) -> Result<AgentRunResponse, AgentApiError> {
    let started_at = Instant::now();
    let message = request.message.trim();
    if message.is_empty() {
        return Err(AgentApiError::bad_request("message cannot be empty"));
    }

    let cfg = super::config::AppConfig::load_settings().apply_env();
    let agent_cfg = cfg.agent.clone();

    if request
        .provider_profile_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .is_some()
        && resolve_provider_profile(&agent_cfg, &request).is_none()
    {
        return Err(AgentApiError::bad_request("provider_profile_id not found"));
    }

    let provider_profile = resolve_provider_profile(&agent_cfg, &request).cloned();
    let provider = request
        .provider
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            provider_profile
                .as_ref()
                .map(|profile| profile.provider.clone())
        })
        .unwrap_or_else(|| agent_cfg.provider.clone());
    let model_id = request
        .model_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            provider_profile
                .as_ref()
                .map(|profile| profile.model_id.clone())
        })
        .unwrap_or_else(|| agent_cfg.model_id.clone());
    let base_url = request
        .base_url
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            provider_profile
                .as_ref()
                .map(|profile| profile.base_url.clone())
        })
        .unwrap_or_else(|| agent_cfg.base_url.clone());
    let api_key = resolve_agent_api_key(request.api_key.clone(), &agent_cfg)?;
    let streaming_enabled = request.streaming.unwrap_or(agent_cfg.streaming);
    let headers = request.custom_headers.clone().unwrap_or_else(|| {
        provider_profile
            .as_ref()
            .map(|profile| profile.custom_headers.clone())
            .unwrap_or_else(|| agent_cfg.custom_headers.clone())
    });

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

    if is_capability_inventory_request(message) {
        let session_id = request
            .session_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(mlx_agent_core::SessionStore::new_session_id);
        let snapshot = build_capability_inventory_snapshot(
            state,
            &cfg,
            &agent_cfg,
            &request,
            &provider,
            &model_id,
            &session_id,
        )
        .await?;
        let content = render_capability_inventory_markdown(&snapshot);
        persist_agent_exchange(state, &session_id, &request.message, &content).await;

        return Ok(AgentRunResponse {
            session_id: session_id.clone(),
            audit_id: Some(session_id),
            provider,
            model_id,
            content,
            iterations: 0,
            tool_calls_made: 0,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            latency_ms: started_at.elapsed().as_millis() as u64,
            context_budget: None,
        });
    }

    let registry = AgentProviderRegistry;
    let primary = registry.resolve(state, &provider, &model_id, &api_key, &base_url, &headers)?;

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
        run_agent_once(state, &agent_cfg, &request, primary, workspace.clone()).await;
    match primary_result {
        Ok(response) => Ok(response),
        Err(err) if !fallback_enabled || err.error != "provider_error" => Err(err),
        Err(err) => {
            warn!("primary provider failed, trying fallback: {}", err.error);
            let fallback = registry.resolve(
                state,
                &fallback_provider,
                &fallback_model,
                &api_key,
                &base_url,
                &headers,
            )?;

            let fallback_result =
                run_agent_once(state, &agent_cfg, &request, fallback, workspace).await;
            fallback_result.map_err(|fallback_err| {
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

// ── Handlers ─────────────────────────────────────────────────────

/// POST /agent/run — run the agent loop and return the final response.
pub async fn agent_run(
    State(state): State<super::AppState>,
    Json(request): Json<AgentRunRequest>,
) -> Result<Json<AgentRunResponse>, AgentApiError> {
    execute_agent_request(&state, request).await.map(Json)
}

/// POST /agent/stream — streaming agent run.
pub async fn agent_stream(
    State(state): State<super::AppState>,
    Json(mut request): Json<AgentRunRequest>,
) -> Result<Response, AgentApiError> {
    if request.message.trim().is_empty() {
        return Err(AgentApiError::bad_request("message cannot be empty"));
    }

    let session_id = request
        .session_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(mlx_agent_core::SessionStore::new_session_id);
    request.session_id = Some(session_id.clone());

    let mut subscription = state.agent_state.event_bus.subscribe();
    let (tx, rx) = mpsc::channel::<AgentStreamFrame>(128);
    let state_clone = state.clone();

    tokio::spawn(async move {
        let mut run_task =
            tokio::spawn(async move { execute_agent_request(&state_clone, request).await });
        let mut emitted_answer = false;

        loop {
            tokio::select! {
                result = &mut run_task => {
                    match result {
                        Ok(Ok(response)) => {
                            if !emitted_answer && !response.content.trim().is_empty() {
                                let _ = tx.send(AgentStreamFrame::answer(
                                    response.content.clone(),
                                    response.session_id.clone(),
                                )).await;
                            }
                            let _ = tx.send(AgentStreamFrame::done(&response)).await;
                        }
                        Ok(Err(error)) => {
                            let message = error.details.clone().unwrap_or(error.error.clone());
                            let _ = tx.send(AgentStreamFrame::error(message, Some(session_id.clone()))).await;
                        }
                        Err(error) => {
                            let _ = tx.send(AgentStreamFrame::error(error.to_string(), Some(session_id.clone()))).await;
                        }
                    }
                    break;
                }
                event = subscription.recv() => {
                    let Ok(event) = event else { continue; };
                    let frame = match event {
                        AgentEvent::RunStarted { session_id: event_session_id, .. } if event_session_id == session_id => {
                            Some(AgentStreamFrame::status("thinking", Some(event_session_id)))
                        }
                        AgentEvent::ThinkingDelta { session_id: event_session_id, delta } if event_session_id == session_id => {
                            Some(AgentStreamFrame::thinking(delta, event_session_id))
                        }
                        AgentEvent::TextDelta { session_id: event_session_id, delta } if event_session_id == session_id => {
                            emitted_answer = true;
                            Some(AgentStreamFrame::answer(delta, event_session_id))
                        }
                        AgentEvent::ToolCallStarted { session_id: event_session_id, tool, .. } if event_session_id == session_id => {
                            Some(AgentStreamFrame::tool("tool_call_started", event_session_id, tool, Some("Executando tool".to_string())))
                        }
                        AgentEvent::ToolCallCompleted { session_id: event_session_id, tool, result_preview, .. } if event_session_id == session_id => {
                            Some(AgentStreamFrame::tool("tool_call_completed", event_session_id, tool, Some(result_preview)))
                        }
                        AgentEvent::ToolCallDenied { session_id: event_session_id, tool, reason } if event_session_id == session_id => {
                            Some(AgentStreamFrame::tool("tool_call_denied", event_session_id, tool, Some(reason)))
                        }
                        AgentEvent::RunFailed { session_id: event_session_id, error } if event_session_id == session_id => {
                            Some(AgentStreamFrame::error(error, Some(event_session_id)))
                        }
                        _ => None,
                    };

                    if let Some(frame) = frame {
                        if tx.send(frame).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    });

    let stream = ReceiverStream::new(rx).map(|event| {
        let mut payload = serde_json::to_vec(&event).unwrap_or_else(|_| {
            b"{\"event\":\"error\",\"message\":\"serialization failed\"}".to_vec()
        });
        payload.push(b'\n');
        Ok::<Bytes, io::Error>(Bytes::from(payload))
    });

    let body = Body::from_stream(stream);
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/x-ndjson; charset=utf-8")
        .header("Cache-Control", "no-cache")
        .body(body)
        .map_err(|error| {
            AgentApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "stream_build_failed",
                Some(error.to_string()),
            )
        })
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
    if merged.provider_profiles.is_empty() {
        merged.provider_profiles = cfg.agent.provider_profiles.clone();
    }
    if merged.provider_profile_id.trim().is_empty() {
        merged.provider_profile_id = cfg.agent.provider_profile_id.clone();
    }
    if merged.default_toolset_id.trim().is_empty() {
        merged.default_toolset_id = cfg.agent.default_toolset_id.clone();
    }
    if merged.gateway_mode.trim().is_empty() {
        merged.gateway_mode = cfg.agent.gateway_mode.clone();
    }
    if merged.memory_snapshot_mode.trim().is_empty() {
        merged.memory_snapshot_mode = cfg.agent.memory_snapshot_mode.clone();
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
        "toolsets": mlx_agent_core::toolset_profiles(),
        "entries": mlx_agent_core::tool_catalog(),
    })))
}

/// GET /agent/toolsets
pub async fn agent_toolsets() -> Result<Json<Vec<mlx_agent_core::ToolsetProfile>>, AgentApiError> {
    Ok(Json(mlx_agent_core::toolset_profiles()))
}

/// GET /agent/provider-profiles
pub async fn agent_provider_profiles(
) -> Result<Json<Vec<super::config::AgentProviderProfileConfig>>, AgentApiError> {
    let cfg = super::config::AppConfig::load_settings().apply_env();
    Ok(Json(cfg.agent.provider_profiles))
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

/// GET /agent/compat/report
pub async fn agent_compat_report(
    State(state): State<super::AppState>,
) -> Result<Json<AgentCompatReport>, AgentApiError> {
    let cfg = super::config::AppConfig::load_settings().apply_env();
    let channels = state
        .channel_service
        .list_channels()
        .await
        .map_err(|error| {
            AgentApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "channels_failed",
                Some(error),
            )
        })?;
    let plugins = state.plugin_manager.list_plugins().await;
    let catalog = load_skill_catalog(&state, &cfg).await?;
    let node_manager = normalize_node_manager(None, &cfg.agent.node_package_manager);
    let skills_check = build_skills_check_response(catalog.items, &node_manager);

    let mut gaps = Vec::new();

    let channel_entries = channels
        .iter()
        .map(|channel| {
            let (state, local_testable, requires_external_activation, notes, activation_checklist) =
                classify_channel_support(channel);
            let healthy_accounts = channel
                .accounts
                .iter()
                .filter(|account| {
                    matches!(
                        account.health_state.status.as_str(),
                        "healthy" | "connected" | "logged_in"
                    )
                })
                .count();
            let configured_accounts = channel
                .accounts
                .iter()
                .filter(|account| account.configured)
                .count();
            AgentCompatChannelEntry {
                id: channel.id.clone(),
                name: channel.name.clone(),
                state,
                local_testable,
                requires_external_activation,
                protocol_family: channel.protocol_family.clone(),
                protocol_version: channel.protocol_version.clone(),
                account_count: channel.accounts.len(),
                configured_accounts,
                healthy_accounts,
                capabilities: channel.capabilities.clone(),
                notes,
                activation_checklist,
            }
        })
        .collect::<Vec<_>>();

    let external_channels = channel_entries
        .iter()
        .filter(|channel| channel.requires_external_activation)
        .count();
    if external_channels > 0 {
        gaps.push(AgentCompatGap {
            area: "channels".to_string(),
            id: "external_activation".to_string(),
            severity: "warning".to_string(),
            message: format!(
                "{external_channels} channel adapters depend on external bridge/webhook activation for production use."
            ),
            action:
                "Usar o mock E2E local para validar o adapter e seguir o checklist de ativacao real no ambiente alvo."
                    .to_string(),
        });
    }

    let plugin_entries = plugins
        .iter()
        .map(|plugin| AgentCompatPluginEntry {
            id: plugin.id.clone(),
            name: plugin.name.clone(),
            state: if plugin.configured || !plugin.config.is_null() {
                "managed".to_string()
            } else {
                "catalogued".to_string()
            },
            enabled: plugin.enabled,
            configured: plugin.configured,
            loaded: plugin.loaded,
            health: plugin_health_status(plugin),
            capabilities: plugin.capabilities.clone(),
            errors: plugin.errors.clone(),
        })
        .collect::<Vec<_>>();

    for plugin in &plugin_entries {
        if !plugin.errors.is_empty() {
            gaps.push(AgentCompatGap {
                area: "plugins".to_string(),
                id: plugin.id.clone(),
                severity: "warning".to_string(),
                message: format!("Plugin '{}' reported runtime errors.", plugin.id),
                action: "Revisar health/runtime logs e reaplicar a configuracao do plugin."
                    .to_string(),
            });
        }
    }

    let skill_entries = skills_check
        .skills
        .iter()
        .map(|skill| AgentCompatSkillEntry {
            name: skill.name.clone(),
            status: skill_status(skill),
            eligible: skill.eligible,
            enabled: skill.enabled,
            active: skill.active,
            integrity: skill.integrity.clone(),
            installable: !skill.install_options.is_empty(),
            missing: skill.missing.clone(),
            capabilities: skill.capabilities.clone(),
        })
        .collect::<Vec<_>>();

    for skill in &skill_entries {
        if matches!(
            skill.status.as_str(),
            "missing_dependencies" | "missing_configuration" | "blocked_integrity"
        ) {
            gaps.push(AgentCompatGap {
                area: "skills".to_string(),
                id: skill.name.clone(),
                severity: if skill.status == "blocked_integrity" {
                    "critical".to_string()
                } else {
                    "warning".to_string()
                },
                message: format!("Skill '{}' is currently {}.", skill.name, skill.status),
                action: if skill.status == "missing_dependencies" {
                    "Executar /agent/skills/install ou provisionar os bins exigidos.".to_string()
                } else if skill.status == "missing_configuration" {
                    "Configurar env/keys pela UI ou vault antes de habilitar a skill.".to_string()
                } else {
                    "Atualizar a skill ou revisar os pins/hash de integridade configurados."
                        .to_string()
                },
            });
        }
    }

    let tool_policy = build_tool_policy_state(&cfg.agent, None, None);
    let selected_profile = parse_tool_profile(Some(cfg.agent.tool_policy.profile.as_str()));
    let tool_profiles = [
        mlx_agent_core::ToolProfileName::Minimal,
        mlx_agent_core::ToolProfileName::Coding,
        mlx_agent_core::ToolProfileName::Messaging,
        mlx_agent_core::ToolProfileName::Full,
    ]
    .into_iter()
    .map(|profile| {
        let mut profile_policy = tool_policy.clone();
        profile_policy.profile = profile;
        build_tool_profile_entry(profile, &profile_policy)
    })
    .collect::<Vec<_>>();
    let effective_policy =
        mlx_agent_core::resolve_effective_tool_policy(&tool_policy, DEFAULT_AGENT_ID, None);
    let blocked_effective_tools = effective_policy
        .entries
        .iter()
        .filter(|entry| entry.implemented && !entry.allowed)
        .count();
    if blocked_effective_tools > 0 {
        gaps.push(AgentCompatGap {
            area: "tools".to_string(),
            id: selected_profile.as_str().to_string(),
            severity: "info".to_string(),
            message: format!(
                "{blocked_effective_tools} implemented tools are blocked by the current effective policy."
            ),
            action: "Revisar allow/deny apenas se o profile atual estiver mais restritivo do que o onboarding esperado.".to_string(),
        });
    }

    let context_benchmark = build_context_benchmark(&cfg.agent);
    if context_benchmark.critical {
        gaps.push(AgentCompatGap {
            area: "context".to_string(),
            id: "small_local_budget".to_string(),
            severity: "warning".to_string(),
            message: "Synthetic small-local benchmark hit critical context headroom.".to_string(),
            action: context_benchmark.recommendation.clone(),
        });
    }

    let endpoint_compatibility = compatibility_endpoints();
    let channels_supported = channel_entries.len();
    let plugins_supported = plugin_entries.len();
    let tools_supported = tool_profiles
        .iter()
        .filter(|profile| profile.status == "covered")
        .count();
    let endpoint_supported = endpoint_compatibility
        .iter()
        .filter(|endpoint| endpoint.backward_compatible)
        .count();
    let skill_subsystem_supported = 1usize;
    let context_supported = usize::from(!context_benchmark.critical);
    let total_checks = channel_entries.len()
        + plugin_entries.len()
        + tool_profiles.len()
        + endpoint_compatibility.len()
        + 2;
    let passed_checks = channels_supported
        + plugins_supported
        + tools_supported
        + endpoint_supported
        + skill_subsystem_supported
        + context_supported;

    let critical_gaps = gaps.iter().filter(|gap| gap.severity == "critical").count();
    let warning_gaps = gaps.iter().filter(|gap| gap.severity == "warning").count();

    Ok(Json(AgentCompatReport {
        mode: "hermes-agent".to_string(),
        target: "Hermes-based local agent".to_string(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        coverage_methodology:
            "Coverage counts implemented compatibility surfaces: registered channels, managed plugins, skill subsystem availability, tool-profile coverage, preserved endpoints, and synthetic small-local context benchmark."
                .to_string(),
        summary: AgentCompatSummary {
            total_checks,
            passed_checks,
            coverage_percent: percent_ratio(passed_checks, total_checks),
            critical_gaps,
            warning_gaps,
        },
        migration: AgentCompatMigrationStatus {
            schema_version: cfg.schema_version,
            current_schema_version: super::config::APP_CONFIG_SCHEMA_VERSION,
            migrated: cfg.schema_version == super::config::APP_CONFIG_SCHEMA_VERSION,
            backward_compatible: true,
            migration_flags: vec![
                "config_schema_v2".to_string(),
                "legacy_enabled_tools_sync".to_string(),
                "compatibility_state_roundtrip".to_string(),
            ],
        },
        channels: channel_entries,
        plugins: plugin_entries,
        skills: AgentCompatSkillsSection {
            subsystem_status: "operational".to_string(),
            summary: skills_check.summary,
            entries: skill_entries,
        },
        tools: AgentCompatToolsSection {
            selected_profile: selected_profile.as_str().to_string(),
            profiles: tool_profiles,
            effective_policy,
        },
        endpoint_compatibility,
        context_benchmark,
        gaps,
    }))
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
        .unwrap_or_else(|| {
            mlx_agent_core::session::SessionMeta::basic(session_id, "Nova conversa".to_string())
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
    fn normalize_agent_model_id_strips_local_prefixes_and_display_suffixes() {
        assert_eq!(
            normalize_agent_model_id("ollama", "ollama::deepseek-r1:8b"),
            "deepseek-r1:8b"
        );
        assert_eq!(
            normalize_agent_model_id("ollama", "deepseek-r1:8b [Ollama]"),
            "deepseek-r1:8b"
        );
        assert_eq!(
            normalize_agent_model_id("mlx", "mlx::mlx-community/Qwen3-4B-4bit"),
            "mlx-community/Qwen3-4B-4bit"
        );
        assert_eq!(
            normalize_agent_model_id("llamacpp", "llama::qwen3.gguf [llama.cpp]"),
            "qwen3.gguf"
        );
    }

    #[test]
    fn capability_inventory_detector_matches_agent_capability_questions() {
        assert!(is_capability_inventory_request(
            "Qual seu nome? Quais skills, plugins e tools voce tem?",
        ));
        assert!(is_capability_inventory_request(
            "Tem pesquisa na web e consegue rodar Python?",
        ));
        assert!(!is_capability_inventory_request(
            "Crie um script Python para ordenar uma lista.",
        ));
    }

    #[test]
    fn capability_inventory_markdown_reflects_exec_and_web_flags() {
        let content = render_capability_inventory_markdown(&CapabilityInventorySnapshot {
            provider: "ollama".to_string(),
            model_id: "qwen3.5:9b".to_string(),
            tools: vec![
                (
                    "exec".to_string(),
                    "Executar comando de shell no workspace.".to_string(),
                ),
                (
                    "read_file".to_string(),
                    "Ler o conteudo de um arquivo.".to_string(),
                ),
            ],
            skills: vec![(
                "code-reviewer".to_string(),
                "Review changed code.".to_string(),
            )],
            plugins: vec![(
                "memory".to_string(),
                "loaded".to_string(),
                "semantic-search".to_string(),
            )],
            supports_exec: true,
            supports_web: false,
        });

        assert!(content.contains("MLX-Pilot Agent"));
        assert!(content.contains("`exec`"));
        assert!(content.contains("code-reviewer"));
        assert!(content.contains("memory"));
        assert!(content.contains("Python/exec: Sim"));
        assert!(content.contains("Pesquisa na web: Nao"));
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
    fn resolve_provider_profile_prefers_request_then_config_default() {
        let mut cfg = crate::config::AgentUiConfig::default();
        cfg.provider_profiles
            .push(crate::config::AgentProviderProfileConfig {
                id: "mlx-local".to_string(),
                description: "Apple Silicon MLX".to_string(),
                provider: "mlx".to_string(),
                model_id: "mlx-community/Qwen3-4B-4bit".to_string(),
                base_url: String::new(),
                api_key_ref: None,
                custom_headers: BTreeMap::new(),
                runtime_variant: "hermes_inspired".to_string(),
            });
        cfg.provider_profile_id = "ollama-local".to_string();

        let default_request = AgentRunRequest {
            session_id: None,
            message: "hello".to_string(),
            provider: None,
            model_id: None,
            api_key: None,
            base_url: None,
            custom_headers: None,
            streaming: None,
            fallback_enabled: None,
            fallback_provider: None,
            fallback_model_id: None,
            execution_mode: None,
            approval_mode: None,
            system_prompt: None,
            max_iterations: None,
            max_prompt_tokens: None,
            max_history_messages: None,
            max_tools_in_prompt: None,
            temperature: None,
            aggressive_tool_filtering: None,
            enable_tool_call_fallback: None,
            runtime_variant: None,
            persist_tool_events: None,
            session_search_enabled: None,
            memory_profile: None,
            memory_snapshot_mode: None,
            session_context: None,
            gateway_context: None,
            delegate_depth: None,
            enabled_skills: None,
            enabled_tools: None,
            toolset_id: None,
            provider_profile_id: None,
            workspace_root: None,
        };
        let default_profile = resolve_provider_profile(&cfg, &default_request).unwrap();
        assert_eq!(default_profile.id, "ollama-local");

        let explicit_request = AgentRunRequest {
            session_id: None,
            message: "hello".to_string(),
            provider: None,
            model_id: None,
            api_key: None,
            base_url: None,
            custom_headers: None,
            streaming: None,
            fallback_enabled: None,
            fallback_provider: None,
            fallback_model_id: None,
            execution_mode: None,
            approval_mode: None,
            system_prompt: None,
            max_iterations: None,
            max_prompt_tokens: None,
            max_history_messages: None,
            max_tools_in_prompt: None,
            temperature: None,
            aggressive_tool_filtering: None,
            enable_tool_call_fallback: None,
            runtime_variant: None,
            persist_tool_events: None,
            session_search_enabled: None,
            memory_profile: None,
            memory_snapshot_mode: None,
            session_context: None,
            gateway_context: None,
            delegate_depth: None,
            enabled_skills: None,
            enabled_tools: None,
            toolset_id: None,
            provider_profile_id: Some("mlx-local".to_string()),
            workspace_root: None,
        };
        let explicit_profile = resolve_provider_profile(&cfg, &explicit_request).unwrap();
        assert_eq!(explicit_profile.id, "mlx-local");
        assert_eq!(explicit_profile.provider, "mlx");
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
        #[cfg(windows)]
        let blocked = dir.path().join("blocked-command.exe");
        #[cfg(not(windows))]
        let blocked = dir.path().join("blocked-command");

        #[cfg(windows)]
        std::fs::write(&blocked, b"not-a-real-executable").unwrap();
        #[cfg(not(windows))]
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
        let stderr = result.stderr.to_ascii_lowercase();
        assert!(
            stderr.contains("permission")
                || stderr.contains("access is denied")
                || stderr.contains("program not found")
                || stderr.contains("not a valid win32")
                || stderr.contains("os error 193")
                || stderr.contains("os error 216")
                || stderr.contains("não é compatível")
                || stderr.contains("not compatible"),
            "unexpected stderr: {}",
            result.stderr
        );
    }
}
