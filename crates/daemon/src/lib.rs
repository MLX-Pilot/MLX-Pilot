mod agent_api;
mod agent_runtime_tools;
mod catalog;
mod channels;
mod chat_stream;
mod config;
mod hwfit_routes;
mod jobs;
mod model_catalog;
mod n8n_integration;
mod plugins;
mod provider_embedder;
mod research_routes;
mod runtime_doctor;
mod search;
mod secrets_vault;
mod startup;
mod wave1;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path as FsPath, PathBuf as FsPathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use bytes::Bytes;
use catalog::{
    CatalogConfig, CatalogError, CatalogSearchQuery, CatalogService, CatalogSourceDescriptor,
    CreateDownloadRequest, DownloadJob, RemoteModelCard,
};
use chat_stream::{spawn_chat_stream, ChatRuntimeConfig, ChatStreamEvent};
use config::AppConfig;
use llamacpp_provider::{LlamaCppProvider, LlamaCppProviderConfig};
use mlx_agent_core::embeddings::Embedder;
use mlx_agent_skills::normalize_env_key;
use mlx_ollama_core::{ChatRequest, ChatResponse, ModelDescriptor, ModelProvider, ProviderError};
use mlx_provider::{MlxProvider, MlxProviderConfig};
use ollama_provider::{OllamaProvider, OllamaProviderConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

use crate::channels::{
    ChannelAuthRequest, ChannelLogsQuery, ChannelProbeRequest, ChannelRemoveAccountRequest,
    ChannelResolveRequest, ChannelService, ChannelUpsertAccountRequest, LegacyChannelRemoveRequest,
    LegacyChannelUpsertRequest, MessageSendRequest,
};
use crate::plugins::{PluginConfigRequest, PluginManager, PluginToggleRequest};
use crate::secrets_vault::SecretsVault;
use http_llm_provider::{HttpLlmProvider, HttpLlmProviderConfig};

#[derive(Clone)]
struct AppState {
    provider_mode: LocalProviderMode,
    mlx_provider: Arc<MlxProvider>,
    llamacpp_provider: Arc<LlamaCppProvider>,
    ollama_provider: Arc<OllamaProvider>,
    brave_api_key: Option<String>,
    catalog: Arc<CatalogService>,
    chat_runtime: ChatRuntimeConfig,
    pub session_store: Arc<mlx_agent_core::SessionStore>,
    pub agent_state: agent_api::AgentState,
    pub plugin_manager: Arc<PluginManager>,
    pub channel_service: Arc<ChannelService>,
    pub presets: Arc<mlx_agent_core::PresetStore>,
    pub compare: Arc<mlx_agent_core::CompareStore>,
    pub jobs: Arc<jobs::JobRegistry>,
    pub state_db_path: FsPathBuf,
    pub search_service: Arc<search::SearchService>,
    pub search_config: search::SearchConfig,
    #[allow(dead_code)]
    pub embedder: Option<Arc<dyn mlx_agent_core::embeddings::Embedder>>,
    #[allow(dead_code)]
    pub vault: Option<Arc<SecretsVault>>,
    startup: startup::StartupCoordinator,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LocalProviderMode {
    Auto,
    Mlx,
    Llamacpp,
    Ollama,
}

impl LocalProviderMode {
    fn from_env(value: &str) -> Self {
        match value.trim().to_lowercase().as_str() {
            "mlx" => Self::Mlx,
            "llamacpp" | "llama" | "llama.cpp" => Self::Llamacpp,
            "ollama" => Self::Ollama,
            _ => Self::Auto,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Mlx => "mlx",
            Self::Llamacpp => "llamacpp",
            Self::Ollama => "ollama",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RoutedProvider {
    Mlx,
    Llamacpp,
    Ollama,
}

const WORKSPACE_MARKERS: &[&str] = &["skills", ".claude/skills", ".codex/skills"];

fn has_workspace_marker(path: &FsPath) -> bool {
    path.join(".git").exists()
        || WORKSPACE_MARKERS
            .iter()
            .any(|marker| path.join(marker).is_dir())
}

fn find_workspace_root_from(start: &FsPath) -> Option<FsPathBuf> {
    start
        .ancestors()
        .find(|candidate| has_workspace_marker(candidate))
        .map(FsPath::to_path_buf)
}

fn resolve_state_db_path() -> FsPathBuf {
    AppConfig::get_settings_path()
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("agent")
        .join("state.sqlite")
}

fn resolve_default_agent_workspace() -> FsPathBuf {
    if let Ok(value) = std::env::var("APP_AGENT_WORKSPACE") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return FsPathBuf::from(trimmed);
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        if let Some(workspace) = find_workspace_root_from(&cwd) {
            return workspace;
        }
        return cwd;
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            if let Some(workspace) = find_workspace_root_from(parent) {
                return workspace;
            }
            return parent.to_path_buf();
        }
    }

    FsPathBuf::new()
}

#[derive(Debug)]
struct RoutedModel {
    provider: RoutedProvider,
    normalized_model_id: String,
}

#[derive(Debug)]
enum AppError {
    Provider(ProviderError),
    Catalog(CatalogError),
    NotFound(String),
    InvalidChannelRequest {
        status: StatusCode,
        message: String,
        error_code: String,
    },
}

impl From<ProviderError> for AppError {
    fn from(value: ProviderError) -> Self {
        Self::Provider(value)
    }
}

impl From<CatalogError> for AppError {
    fn from(value: CatalogError) -> Self {
        Self::Catalog(value)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message, error_code) = match self {
            AppError::Provider(error) => map_provider_error(error),
            AppError::Catalog(error) => map_catalog_error(error),
            AppError::NotFound(message) => (StatusCode::NOT_FOUND, message, None),
            AppError::InvalidChannelRequest {
                status,
                message,
                error_code,
            } => (status, message, Some(error_code)),
        };

        (
            status,
            Json(ErrorBody {
                error: message,
                error_code,
                protocol_version: Some(channels::protocol::CHANNEL_PROTOCOL_VERSION.to_string()),
            }),
        )
            .into_response()
    }
}

fn map_provider_error(error: ProviderError) -> (StatusCode, String, Option<String>) {
    match error {
        ProviderError::InvalidRequest { details } => (StatusCode::BAD_REQUEST, details, None),
        ProviderError::ModelNotFound { model_id } => (
            StatusCode::NOT_FOUND,
            format!("model '{model_id}' not found"),
            None,
        ),
        ProviderError::Timeout { seconds } => (
            StatusCode::GATEWAY_TIMEOUT,
            format!("provider timeout ({seconds}s)"),
            None,
        ),
        ProviderError::Unavailable { details } => (StatusCode::SERVICE_UNAVAILABLE, details, None),
        ProviderError::CommandFailed { command, stderr } => (
            StatusCode::BAD_GATEWAY,
            format!("command failed: {command}; stderr: {stderr}"),
            None,
        ),
        ProviderError::Io { context, source } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{context}: {source}"),
            None,
        ),
    }
}

fn map_catalog_error(error: CatalogError) -> (StatusCode, String, Option<String>) {
    match error {
        CatalogError::BadRequest(message) => (StatusCode::BAD_REQUEST, message, None),
        CatalogError::NotFound(message) => (StatusCode::NOT_FOUND, message, None),
        CatalogError::Network(message) => (StatusCode::BAD_GATEWAY, message, None),
        CatalogError::Cancelled { details } => (StatusCode::CONFLICT, details, None),
        CatalogError::Io { context, source } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{context}: {source}"),
            None,
        ),
        CatalogError::Unavailable(message) => (StatusCode::SERVICE_UNAVAILABLE, message, None),
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    protocol_version: Option<String>,
}

fn ensure_channel_protocol(headers: &HeaderMap) -> Result<(), AppError> {
    channels::protocol::ensure_supported_request_version(headers).map_err(map_channel_service_error)
}

fn map_channel_service_error(error: String) -> AppError {
    let (code, message) = if let Some((code, message)) = error.split_once(": ") {
        (code.to_string(), message.to_string())
    } else {
        ("provider_error".to_string(), error)
    };

    let status = match code.as_str() {
        "invalid_request" | "invalid_target" | "protocol_version_mismatch" => {
            StatusCode::BAD_REQUEST
        }
        "auth_error" => StatusCode::UNAUTHORIZED,
        "permission_error" => StatusCode::FORBIDDEN,
        "rate_limited" => StatusCode::TOO_MANY_REQUESTS,
        "network_error" => StatusCode::BAD_GATEWAY,
        _ => StatusCode::BAD_GATEWAY,
    };

    AppError::InvalidChannelRequest {
        status,
        message,
        error_code: code,
    }
}

#[derive(Serialize)]
struct HealthBody {
    status: String,
    provider: &'static str,
    provider_ready: bool,
    degraded: bool,
}

#[derive(Debug, Deserialize)]
struct BraveSearchRequest {
    query: String,
    api_key: Option<String>,
    max_results: Option<usize>,
}

#[derive(Debug, Serialize)]
struct BraveSearchResultItem {
    title: String,
    url: String,
    description: String,
}

#[derive(Debug, Serialize)]
struct BraveSearchResponse {
    query: String,
    key_source: String,
    results: Vec<BraveSearchResultItem>,
}

#[derive(Debug, Deserialize)]
struct EnvironmentQuery {
    reveal: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct EnvironmentUpdateRequest {
    values: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
struct EnvironmentVariableView {
    key: String,
    label: String,
    value: String,
    masked: String,
    source: String,
    present: bool,
    is_secret: bool,
}

#[derive(Debug, Serialize)]
struct EnvironmentResponse {
    env_path: String,
    env_exists: bool,
    env_example_path: String,
    env_example_exists: bool,
    variables: Vec<EnvironmentVariableView>,
}

/// On Windows, stop spawned console child processes (python/mlx, probes, installers, ...)
/// from flashing a black terminal window. No-op on other platforms.
pub(crate) fn silence_console(command: &mut tokio::process::Command) {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

pub async fn run() -> anyhow::Result<()> {
    init_tracing();

    let cfg = AppConfig::load_settings().apply_env();
    info!("starting daemon on {}", cfg.bind_addr);
    let provider_mode = LocalProviderMode::from_env(&cfg.local_provider);

    let mlx_provider = Arc::new(MlxProvider::new(MlxProviderConfig {
        models_dir: cfg.models_dir.clone(),
        command: cfg.mlx_command.clone(),
        command_prefix_args: cfg.mlx_prefix_args.clone(),
        command_suffix_args: cfg.mlx_suffix_args.clone(),
        timeout: cfg.mlx_timeout,
        airllm_enabled: cfg.mlx_airllm_enabled,
        airllm_threshold_percent: cfg.mlx_airllm_threshold_percent,
        airllm_safe_mode: cfg.mlx_airllm_safe_mode,
        airllm_python_command: cfg.mlx_airllm_python_command.clone(),
        airllm_runner: cfg.mlx_airllm_runner.clone(),
        airllm_backend: cfg.mlx_airllm_backend.clone(),
    }));

    let llamacpp_provider = Arc::new(LlamaCppProvider::new(LlamaCppProviderConfig {
        models_dir: cfg.models_dir.clone(),
        server_binary: cfg.llamacpp_server_binary.clone(),
        base_url: cfg.llamacpp_base_url.clone(),
        timeout: cfg.llamacpp_timeout,
        startup_timeout: cfg.llamacpp_startup_timeout,
        auto_start: cfg.llamacpp_auto_start,
        auto_install: cfg.llamacpp_auto_install,
        context_size: cfg.llamacpp_context_size,
        gpu_layers: cfg.llamacpp_gpu_layers,
        extra_args: cfg.llamacpp_extra_args.clone(),
    }));

    let ollama_provider = Arc::new(OllamaProvider::new(OllamaProviderConfig {
        base_url: cfg.ollama_base_url.clone(),
        timeout: cfg.ollama_timeout,
        startup_timeout: cfg.ollama_startup_timeout,
        auto_start: cfg.ollama_auto_start,
        auto_install: cfg.ollama_auto_install,
    }));

    info!("chat provider mode selected: {}", provider_mode.label());
    let startup = startup::StartupCoordinator::default();

    let catalog = Arc::new(CatalogService::new(CatalogConfig {
        hf_api_base: cfg.hf_api_base.clone(),
        hf_token: cfg.hf_token.clone(),
        downloads_root: cfg.remote_downloads_dir.clone(),
        search_limit_default: cfg.catalog_search_limit,
        download_timeout: cfg.catalog_download_timeout,
    })?);

    let state_db_path = resolve_state_db_path();

    let vault = SecretsVault::open(
        AppConfig::get_settings_path()
            .parent()
            .unwrap_or(std::path::Path::new(".")),
    )
    .ok()
    .map(Arc::new);

    let search_config = search::SearchConfig {
        default_provider: cfg
            .search_provider
            .clone()
            .unwrap_or_else(|| "duckduckgo".to_string()),
        searxng_instance: cfg.searxng_instance.clone(),
        brave_api_key: cfg.brave_api_key.clone(),
        safe_search: cfg.search_safe_search.unwrap_or(true),
        max_results: cfg.search_max_results.unwrap_or(5),
        ..Default::default()
    };

    let search_service = Arc::new(search::SearchService::new(
        &search_config,
        cfg.brave_api_key.clone(),
    ));

    // Try to set up the embedder from the Ollama provider.
    let memory_root = AppConfig::get_settings_path()
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("memory");
    let (embedder, memory): (
        Option<Arc<dyn mlx_agent_core::embeddings::Embedder>>,
        Arc<mlx_agent_core::MemoryStore>,
    ) = {
        let provider_emb = provider_embedder::ProviderEmbedder::new(
            &cfg.ollama_base_url,
            &Some(ollama_provider.clone()),
        )
        .await;
        if provider_emb.is_ready() {
            let emb: Arc<dyn mlx_agent_core::embeddings::Embedder> = Arc::new(provider_emb);
            let mem = mlx_agent_core::MemoryStore::with_embedder(memory_root, emb.clone())
                .await
                .expect("Failed to initialize memory store with embedder");
            (Some(emb), Arc::new(mem))
        } else {
            let mem = mlx_agent_core::MemoryStore::new(memory_root)
                .await
                .expect("Failed to initialize memory store");
            (None, Arc::new(mem))
        }
    };

    let state = AppState {
        provider_mode,
        mlx_provider: mlx_provider.clone(),
        llamacpp_provider,
        ollama_provider,
        brave_api_key: cfg.brave_api_key.clone(),
        catalog,
        chat_runtime: ChatRuntimeConfig {
            models_dir: cfg.models_dir.clone(),
            command: cfg.mlx_command.clone(),
            command_prefix_args: cfg.mlx_prefix_args.clone(),
            command_suffix_args: cfg.mlx_suffix_args.clone(),
            timeout: cfg.mlx_timeout,
            airllm_enabled: cfg.mlx_airllm_enabled,
            airllm_threshold_percent: cfg.mlx_airllm_threshold_percent,
            airllm_safe_mode: cfg.mlx_airllm_safe_mode,
            airllm_python_command: cfg.mlx_airllm_python_command.clone(),
            airllm_runner: cfg.mlx_airllm_runner.clone(),
            airllm_backend: cfg.mlx_airllm_backend.clone(),
        },
        agent_state: agent_api::AgentState {
            default_workspace: resolve_default_agent_workspace(),
            approval: Arc::new(mlx_agent_core::approval::DefaultApprovalService::new()),
            event_bus: Arc::new(mlx_agent_core::EventBus::default()),
            audit: Arc::new(mlx_agent_core::AuditLog::new(
                std::env::temp_dir().join("mlx-pilot-audit"),
            )),
            memory: memory.clone(),
            budget_tracker: Arc::new(tokio::sync::RwLock::new(BTreeMap::new())),
        },
        session_store: Arc::new(
            mlx_agent_core::SessionStore::new(
                AppConfig::get_settings_path()
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .join("sessions"),
            )
            .await
            .expect("Failed to initialize session store"),
        ),
        plugin_manager: Arc::new(PluginManager::new(AppConfig::get_settings_path())),
        channel_service: Arc::new(ChannelService::new(AppConfig::get_settings_path())),
        presets: Arc::new(
            mlx_agent_core::PresetStore::new(
                AppConfig::get_settings_path()
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .join("presets"),
            )
            .await
            .expect("Failed to initialize preset store"),
        ),
        compare: Arc::new(
            mlx_agent_core::CompareStore::new(
                AppConfig::get_settings_path()
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .join("compare"),
            )
            .await
            .expect("Failed to initialize compare store"),
        ),
        jobs: Arc::new(jobs::JobRegistry::new(4)),
        state_db_path: state_db_path.clone(),
        search_service: search_service.clone(),
        search_config: search_config.clone(),
        embedder: embedder.clone(),
        vault: vault.clone(),
        startup,
    };

    state.startup.start(
        state.mlx_provider.clone(),
        state.llamacpp_provider.clone(),
        state.ollama_provider.clone(),
        selected_startup_ollama_model(&cfg),
    );

    // Initialize scheduler tables and start the background scheduler.
    jobs::ensure_scheduler_tables(&state.state_db_path)
        .await
        .expect("Failed to create scheduler tables");
    let scheduler = jobs::Scheduler::new(state.state_db_path.clone(), state.jobs.clone());
    let scheduler_shutdown = tokio_util::sync::CancellationToken::new();
    scheduler.start(scheduler_shutdown);

    let app = Router::new()
        .route("/config", get(get_config).post(update_config))
        .route("/health", get(health))
        .route("/runtime/startup", get(runtime_startup_get))
        .route("/runtime/startup/retry", post(runtime_startup_retry))
        .route("/runtime/startup/cancel", post(runtime_startup_cancel))
        .route(
            "/runtime/doctor",
            get(runtime_doctor_get).post(runtime_doctor_run),
        )
        .route("/models", get(list_models))
        .route("/models/all", get(list_models_all))
        .route("/models/rename", post(rename_model))
        .route("/models/{model_id}", delete(delete_model))
        .route("/chat", post(chat))
        .route("/chat/stream", post(chat_stream))
        .route("/web/brave/search", post(brave_web_search))
        .route("/api/search", post(search::api_search))
        .route("/api/search/fetch", post(search::api_search_fetch))
        .route("/api/search/providers", get(search::api_search_providers))
        .route("/api/search/config", get(search::api_search_config))
        // ── Research (Wave 5) ──
        .route("/api/research/start", post(research_routes::research_start))
        .route(
            "/api/research/stream/{job_id}",
            get(research_routes::research_stream),
        )
        .route(
            "/api/research/cancel/{job_id}",
            post(research_routes::research_cancel),
        )
        .route(
            "/api/research/result/{job_id}",
            get(research_routes::research_result),
        )
        .route(
            "/api/research/report/{id}",
            get(research_routes::research_report),
        )
        .route(
            "/api/research/library",
            get(research_routes::research_library),
        )
        .route(
            "/api/research/spinoff/{id}",
            post(research_routes::research_spinoff),
        )
        .route(
            "/api/research/{id}/hide-image",
            post(research_routes::research_hide_image),
        )
        .route(
            "/api/research/{id}/unhide-images",
            post(research_routes::research_unhide_images),
        )
        .route(
            "/api/research/{id}",
            delete(research_routes::research_delete),
        )
        // ── Hardware Fit (Wave 5) ──
        .route("/api/hwfit/system", get(hwfit_routes::hwfit_system))
        .route("/api/hwfit/models", get(hwfit_routes::hwfit_models))
        .route("/api/hwfit/profiles", get(hwfit_routes::hwfit_profiles))
        .route("/api/hwfit/simulate", post(hwfit_routes::hwfit_simulate))
        .route("/environment", get(environment).post(update_environment))
        .route("/catalog/sources", get(catalog_sources))
        .route("/catalog/models", get(catalog_models))
        .route(
            "/catalog/downloads",
            get(catalog_downloads).post(catalog_create_download),
        )
        .route("/catalog/downloads/{job_id}", get(catalog_download))
        .route(
            "/catalog/downloads/{job_id}/cancel",
            post(catalog_cancel_download),
        )
        // ── Agent API ──
        .route("/integrations/n8n/status", get(n8n_integration::status))
        .route(
            "/integrations/n8n/workflows/list",
            post(n8n_integration::list_workflows),
        )
        .route(
            "/integrations/n8n/workflows/generate",
            post(n8n_integration::generate_workflow),
        )
        .route(
            "/agent/gateway/events",
            post(agent_api::agent_gateway_event),
        )
        .route("/agent/run", post(agent_api::agent_run))
        .route("/agent/providers", get(agent_api::agent_providers))
        .route(
            "/agent/provider-profiles",
            get(agent_api::agent_provider_profiles),
        )
        .route(
            "/agent/config",
            get(agent_api::agent_get_config).post(agent_api::agent_update_config),
        )
        .route("/agent/skills", get(agent_api::agent_list_skills))
        .route("/agent/skills/check", get(agent_api::agent_check_skills))
        .route(
            "/agent/skills/install",
            post(agent_api::agent_install_skills),
        )
        .route("/agent/skills/enable", post(agent_api::agent_enable_skills))
        .route(
            "/agent/skills/disable",
            post(agent_api::agent_disable_skills),
        )
        .route(
            "/agent/skills/config",
            post(agent_api::agent_configure_skill),
        )
        .route("/agent/skills/reload", post(agent_api::agent_reload_skills))
        .route("/agent/tools", get(agent_api::agent_list_tools))
        .route("/agent/tools/catalog", get(agent_api::agent_tools_catalog))
        .route("/agent/toolsets", get(agent_api::agent_toolsets))
        .route("/agent/compat/report", get(agent_api::agent_compat_report))
        .route(
            "/agent/tools/effective-policy",
            get(agent_api::agent_tools_effective_policy),
        )
        .route("/agent/tools/profile", post(agent_api::agent_tools_profile))
        .route(
            "/agent/tools/allow-deny",
            post(agent_api::agent_tools_allow_deny),
        )
        .route("/agent/plugins", get(agent_plugins))
        .route("/agent/plugins/enable", post(agent_enable_plugin))
        .route("/agent/plugins/disable", post(agent_disable_plugin))
        .route("/agent/plugins/config", post(agent_configure_plugin))
        .route("/agent/channels/catalog", get(agent_channels_catalog))
        .route("/agent/channels", get(agent_channels))
        .route("/agent/channels/upsert", post(agent_channels_upsert))
        .route("/agent/channels/remove", post(agent_channels_remove))
        .route(
            "/agent/channels/upsert-account",
            post(agent_channels_upsert_account),
        )
        .route(
            "/agent/channels/remove-account",
            post(agent_channels_remove_account),
        )
        .route("/agent/channels/login", post(agent_channels_login))
        .route("/agent/channels/logout", post(agent_channels_logout))
        .route("/agent/channels/probe", post(agent_channels_probe))
        .route("/agent/channels/resolve", post(agent_channels_resolve))
        .route("/agent/channels/status", get(agent_channels_status))
        .route(
            "/agent/channels/capabilities",
            get(agent_channels_capabilities),
        )
        .route("/agent/channels/logs", get(agent_channels_logs))
        .route("/agent/message/send", post(agent_message_send))
        .route("/agent/audit", get(agent_api::agent_audit))
        .route("/agent/audit/export", get(agent_api::agent_audit_export))
        .route("/agent/audit/{id}", get(agent_api::agent_audit_get_id))
        .route("/agent/approve", post(agent_api::agent_approve))
        .route("/agent/stream", post(agent_api::agent_stream))
        .route(
            "/agent/context/budget",
            get(agent_api::agent_context_budget),
        )
        .route(
            "/agent/sessions",
            get(agent_api::agent_list_sessions).post(agent_api::agent_create_session),
        )
        .route(
            "/agent/sessions/{id}",
            get(agent_api::agent_get_session)
                .patch(agent_api::agent_rename_session)
                .delete(agent_api::agent_delete_session),
        )
        .route(
            "/agent/sessions/{id}/export",
            get(agent_api::agent_export_session),
        )
        // ── Wave 1: Presets ──
        .route(
            "/agent/presets",
            get(wave1::list_presets).post(wave1::save_preset),
        )
        .route(
            "/agent/presets/{id}",
            get(wave1::get_preset).delete(wave1::delete_preset),
        )
        // ── Wave 1: Persistent memory ──
        .route(
            "/agent/memory",
            get(wave1::list_memory).post(wave1::add_memory),
        )
        .route("/agent/memory/search", get(wave1::search_memory))
        .route(
            "/agent/memory/{id}",
            get(wave1::get_memory)
                .patch(wave1::update_memory)
                .delete(wave1::delete_memory),
        )
        .route("/agent/memory/{id}/pin", post(wave1::pin_memory))
        .route("/agent/memory/reindex", post(wave1::reindex_memory))
        .route("/agent/memory/semantic", get(wave1::memory_semantic_status))
        // ── Wave 1: Session history organization + editing ──
        .route(
            "/agent/sessions/{id}/messages",
            get(wave1::session_messages),
        )
        .route("/agent/sessions/{id}/flags", post(wave1::session_set_flags))
        .route("/agent/sessions/{id}/fork", post(wave1::session_fork))
        .route(
            "/agent/sessions/{id}/truncate",
            post(wave1::session_truncate),
        )
        .route(
            "/agent/sessions/{id}/messages/{event_id}",
            patch(wave1::session_edit_message).delete(wave1::session_delete_message),
        )
        .route("/agent/sessions/{id}/download", get(wave1::session_export))
        // ── Wave 1: Compare ──
        .route("/compare/run", post(wave1::compare_run))
        .route("/compare/history", get(wave1::compare_history))
        .route(
            "/compare/{id}",
            get(wave1::compare_get).delete(wave1::compare_delete),
        )
        .route("/compare/{id}/vote", post(wave1::compare_vote))
        .route("/compare/{id}/synthesize", post(wave1::compare_synthesize))
        // ── Jobs / Scheduler ──
        .route("/jobs", get(jobs::job_list))
        .route("/jobs/test", post(jobs::job_test))
        .route("/jobs/{job_id}", get(jobs::job_get))
        .route("/jobs/{job_id}/stream", get(jobs::job_stream))
        .route("/jobs/{job_id}/cancel", post(jobs::job_cancel))
        .route(
            "/scheduler/tasks",
            get(jobs::scheduler_list_tasks).post(jobs::scheduler_create_task),
        )
        .route(
            "/scheduler/tasks/{task_id}",
            delete(jobs::scheduler_delete_task),
        )
        .route(
            "/scheduler/tasks/{task_id}/runs",
            get(jobs::scheduler_task_runs),
        )
        .with_state(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let listener = TcpListener::bind(cfg.bind_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("mlx_ollama_daemon=info,tower_http=info"));

    tracing_subscriber::fmt().with_env_filter(env_filter).init();
}

async fn health(State(state): State<AppState>) -> Json<HealthBody> {
    let startup = state.startup.snapshot(&state.ollama_provider).await;
    Json(HealthBody {
        status: if !startup.app_ready {
            "starting".to_string()
        } else if startup.degraded {
            "degraded".to_string()
        } else if startup.phase == "failed" {
            "failed".to_string()
        } else {
            "ok".to_string()
        },
        provider: state.provider_mode.label(),
        provider_ready: startup.providers.iter().any(|provider| provider.ready),
        degraded: startup.degraded,
    })
}

async fn runtime_startup_get(State(state): State<AppState>) -> Json<startup::StartupSnapshot> {
    Json(state.startup.snapshot(&state.ollama_provider).await)
}

async fn runtime_startup_retry(State(state): State<AppState>) -> Json<startup::StartupSnapshot> {
    let cfg = AppConfig::load_settings().apply_env();
    state.startup.start(
        state.mlx_provider.clone(),
        state.llamacpp_provider.clone(),
        state.ollama_provider.clone(),
        selected_startup_ollama_model(&cfg),
    );
    Json(state.startup.snapshot(&state.ollama_provider).await)
}

async fn runtime_startup_cancel(State(state): State<AppState>) -> Json<startup::StartupSnapshot> {
    state.ollama_provider.cancel_runtime_operation();
    Json(state.startup.snapshot(&state.ollama_provider).await)
}

fn selected_startup_ollama_model(cfg: &AppConfig) -> Option<String> {
    if !cfg.agent.provider.trim().eq_ignore_ascii_case("ollama") {
        return None;
    }
    let without_prefix = cfg
        .agent
        .model_id
        .trim()
        .strip_prefix("ollama::")
        .unwrap_or(cfg.agent.model_id.trim());
    let model = without_prefix
        .strip_suffix(" [Ollama]")
        .unwrap_or(without_prefix)
        .trim();
    if model.is_empty() {
        None
    } else {
        Some(model.to_string())
    }
}

async fn runtime_doctor_get(
    State(state): State<AppState>,
) -> Result<Json<runtime_doctor::RuntimeDoctorReport>, AppError> {
    Ok(Json(
        runtime_doctor::inspect_runtime(&state, runtime_doctor::RuntimeDoctorRequest::default())
            .await,
    ))
}

async fn runtime_doctor_run(
    State(state): State<AppState>,
    Json(request): Json<runtime_doctor::RuntimeDoctorRequest>,
) -> Result<Json<runtime_doctor::RuntimeDoctorReport>, AppError> {
    Ok(Json(runtime_doctor::inspect_runtime(&state, request).await))
}

async fn get_config() -> Result<Json<AppConfig>, AppError> {
    let cfg = AppConfig::load_settings().apply_env();
    Ok(Json(cfg))
}

async fn update_config(Json(new_config): Json<AppConfig>) -> Result<Json<AppConfig>, AppError> {
    new_config
        .save_settings()
        .map_err(|e| AppError::NotFound(format!("Falha ao salvar config: {}", e)))?;
    Ok(Json(new_config))
}

async fn list_models(
    State(state): State<AppState>,
) -> Result<Json<Vec<ModelDescriptor>>, AppError> {
    let models = list_chat_models(&state)
        .await?
        .into_iter()
        .map(annotate_agent_model_compatibility)
        .collect::<Vec<_>>();
    Ok(Json(models))
}

async fn list_models_all(
    State(state): State<AppState>,
) -> Result<Json<Vec<model_catalog::ModelGroup>>, AppError> {
    let local = list_chat_models(&state)
        .await?
        .into_iter()
        .map(annotate_agent_model_compatibility)
        .collect::<Vec<_>>();
    let airgap = false; // TODO: read from config/settings
    let groups = model_catalog::build_unified_models(local, state.vault.as_deref(), airgap).await;
    Ok(Json(groups))
}

async fn agent_plugins(
    State(state): State<AppState>,
) -> Result<Json<Vec<plugins::PluginView>>, AppError> {
    Ok(Json(state.plugin_manager.list_plugins().await))
}

async fn agent_enable_plugin(
    State(state): State<AppState>,
    Json(request): Json<PluginToggleRequest>,
) -> Result<Json<plugins::PluginView>, AppError> {
    state
        .plugin_manager
        .set_plugin_enabled(&request.plugin_id, true)
        .await
        .map(Json)
        .map_err(AppError::NotFound)
}

async fn agent_disable_plugin(
    State(state): State<AppState>,
    Json(request): Json<PluginToggleRequest>,
) -> Result<Json<plugins::PluginView>, AppError> {
    state
        .plugin_manager
        .set_plugin_enabled(&request.plugin_id, false)
        .await
        .map(Json)
        .map_err(AppError::NotFound)
}

async fn agent_configure_plugin(
    State(state): State<AppState>,
    Json(request): Json<PluginConfigRequest>,
) -> Result<Json<plugins::PluginView>, AppError> {
    state
        .plugin_manager
        .update_plugin_config(&request.plugin_id, request.config, request.enabled)
        .await
        .map(Json)
        .map_err(AppError::NotFound)
}

async fn agent_channels_catalog(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Json<Vec<channels::ChannelView>>, AppError> {
    ensure_channel_protocol(&headers)?;
    state
        .channel_service
        .list_channels()
        .await
        .map(Json)
        .map_err(map_channel_service_error)
}

async fn agent_channels(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Json<Vec<channels::ChannelView>>, AppError> {
    ensure_channel_protocol(&headers)?;
    state
        .channel_service
        .list_channels()
        .await
        .map(Json)
        .map_err(map_channel_service_error)
}

async fn agent_channels_upsert(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<LegacyChannelUpsertRequest>,
) -> Result<Json<channels::ChannelView>, AppError> {
    ensure_channel_protocol(&headers)?;
    state
        .channel_service
        .legacy_upsert(request)
        .await
        .map(Json)
        .map_err(map_channel_service_error)
}

async fn agent_channels_remove(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<LegacyChannelRemoveRequest>,
) -> Result<StatusCode, AppError> {
    ensure_channel_protocol(&headers)?;
    state
        .channel_service
        .legacy_remove(request)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_channel_service_error)
}

async fn agent_channels_status(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Json<Vec<channels::ChannelView>>, AppError> {
    ensure_channel_protocol(&headers)?;
    state
        .channel_service
        .list_channels()
        .await
        .map(Json)
        .map_err(map_channel_service_error)
}

async fn agent_channels_capabilities(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Json<Vec<channels::ChannelCapabilityView>>, AppError> {
    ensure_channel_protocol(&headers)?;
    state
        .channel_service
        .channel_capabilities()
        .await
        .map(Json)
        .map_err(map_channel_service_error)
}

async fn agent_channels_upsert_account(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<ChannelUpsertAccountRequest>,
) -> Result<Json<channels::ChannelView>, AppError> {
    ensure_channel_protocol(&headers)?;
    state
        .channel_service
        .upsert_account(request)
        .await
        .map(Json)
        .map_err(map_channel_service_error)
}

async fn agent_channels_remove_account(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<ChannelRemoveAccountRequest>,
) -> Result<StatusCode, AppError> {
    ensure_channel_protocol(&headers)?;
    state
        .channel_service
        .remove_account(request)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_channel_service_error)
}

async fn agent_channels_login(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<ChannelAuthRequest>,
) -> Result<Json<channels::ChannelActionResponse>, AppError> {
    ensure_channel_protocol(&headers)?;
    state
        .channel_service
        .login(request)
        .await
        .map(Json)
        .map_err(map_channel_service_error)
}

async fn agent_channels_logout(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<ChannelAuthRequest>,
) -> Result<Json<channels::ChannelActionResponse>, AppError> {
    ensure_channel_protocol(&headers)?;
    state
        .channel_service
        .logout(request)
        .await
        .map(Json)
        .map_err(map_channel_service_error)
}

async fn agent_channels_probe(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<ChannelProbeRequest>,
) -> Result<Json<Vec<channels::ChannelActionResponse>>, AppError> {
    ensure_channel_protocol(&headers)?;
    state
        .channel_service
        .probe(request)
        .await
        .map(Json)
        .map_err(map_channel_service_error)
}

async fn agent_channels_resolve(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<ChannelResolveRequest>,
) -> Result<Json<channels::ChannelResolveResponse>, AppError> {
    ensure_channel_protocol(&headers)?;
    state
        .channel_service
        .resolve(request)
        .await
        .map(Json)
        .map_err(map_channel_service_error)
}

async fn agent_message_send(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<MessageSendRequest>,
) -> Result<Json<channels::MessageSendResponse>, AppError> {
    ensure_channel_protocol(&headers)?;
    state
        .channel_service
        .send_message(request)
        .await
        .map(Json)
        .map_err(map_channel_service_error)
}

async fn agent_channels_logs(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<ChannelLogsQuery>,
) -> Result<Json<Vec<channels::ChannelAuditEntry>>, AppError> {
    ensure_channel_protocol(&headers)?;
    state
        .channel_service
        .logs(query)
        .await
        .map(Json)
        .map_err(map_channel_service_error)
}

#[derive(Debug, Deserialize)]
struct RenameModelRequest {
    current_id: String,
    new_id: String,
}

#[derive(Debug, Serialize)]
struct ModelMutationResponse {
    message: String,
    model_id: String,
}

async fn rename_model(
    Json(request): Json<RenameModelRequest>,
) -> Result<Json<ModelMutationResponse>, AppError> {
    let current_id_raw = request.current_id.trim();
    let new_id_raw = request.new_id.trim();
    let current_id = normalize_mlx_model_id(current_id_raw);
    let new_id = normalize_mlx_model_id(new_id_raw);

    validate_local_model_id(&current_id)?;
    validate_local_model_id(&new_id)?;
    if current_id == new_id {
        return Err(AppError::Provider(ProviderError::InvalidRequest {
            details: "novo nome deve ser diferente do atual".to_string(),
        }));
    }

    let models_dir = AppConfig::load_settings().apply_env().models_dir;
    let source = models_dir.join(&current_id);
    let destination = models_dir.join(&new_id);

    if !source.exists() || !source.is_dir() {
        return Err(AppError::NotFound(format!(
            "modelo local '{}' nao encontrado",
            current_id
        )));
    }
    if destination.exists() {
        return Err(AppError::Provider(ProviderError::InvalidRequest {
            details: format!("ja existe um modelo chamado '{}'", new_id),
        }));
    }

    fs::rename(&source, &destination).map_err(|source_err| {
        AppError::Provider(ProviderError::Io {
            context: format!(
                "falha ao renomear modelo local '{}' para '{}'",
                current_id, new_id
            ),
            source: source_err,
        })
    })?;

    Ok(Json(ModelMutationResponse {
        message: format!("modelo '{}' renomeado para '{}'", current_id, new_id),
        model_id: new_id,
    }))
}

async fn delete_model(
    AxumPath(model_id): AxumPath<String>,
) -> Result<Json<ModelMutationResponse>, AppError> {
    let model_id_raw = model_id.trim();
    let model_id = normalize_mlx_model_id(model_id_raw);
    validate_local_model_id(&model_id)?;

    let models_dir = AppConfig::load_settings().apply_env().models_dir;
    let target = models_dir.join(&model_id);
    if !target.exists() || !target.is_dir() {
        return Err(AppError::NotFound(format!(
            "modelo local '{}' nao encontrado",
            model_id
        )));
    }

    fs::remove_dir_all(&target).map_err(|source_err| {
        AppError::Provider(ProviderError::Io {
            context: format!("falha ao apagar modelo local '{}'", model_id),
            source: source_err,
        })
    })?;

    Ok(Json(ModelMutationResponse {
        message: format!("modelo '{}' removido", model_id),
        model_id,
    }))
}

fn validate_local_model_id(model_id: &str) -> Result<(), AppError> {
    let normalized = model_id.trim();
    if normalized.is_empty() {
        return Err(AppError::Provider(ProviderError::InvalidRequest {
            details: "nome do modelo nao pode ser vazio".to_string(),
        }));
    }
    if normalized == "." || normalized == ".." {
        return Err(AppError::Provider(ProviderError::InvalidRequest {
            details: "nome do modelo invalido".to_string(),
        }));
    }
    if normalized.contains('/') || normalized.contains('\\') {
        return Err(AppError::Provider(ProviderError::InvalidRequest {
            details: "nome do modelo invalido: nao use separadores de pasta".to_string(),
        }));
    }
    if !normalized
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
    {
        return Err(AppError::Provider(ProviderError::InvalidRequest {
            details: "use apenas letras, numeros, '-', '_' ou '.' no nome do modelo".to_string(),
        }));
    }
    Ok(())
}

fn normalize_mlx_model_id(model_id: &str) -> String {
    let trimmed = model_id.trim();
    if let Some(stripped) = trimmed.strip_prefix("mlx::") {
        return stripped.trim().to_string();
    }
    if let Some(stripped) = trimmed.strip_prefix("MLX::") {
        return stripped.trim().to_string();
    }
    trimmed.to_string()
}

async fn chat(
    State(state): State<AppState>,
    Json(request): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, AppError> {
    if request.model_id.trim().is_empty() {
        return Err(AppError::Provider(ProviderError::InvalidRequest {
            details: "model_id cannot be empty".to_string(),
        }));
    }

    let response = chat_with_routing(&state, request).await.map_err(|error| {
        error!("chat request failed: {error}");
        AppError::Provider(error)
    })?;

    Ok(Json(response))
}

async fn chat_stream(
    State(state): State<AppState>,
    Json(request): Json<ChatRequest>,
) -> Result<Response, AppError> {
    if request.model_id.trim().is_empty() {
        return Err(AppError::Provider(ProviderError::InvalidRequest {
            details: "model_id cannot be empty".to_string(),
        }));
    }

    let routed = route_model_request(&state, &request.model_id).await?;
    let normalized_request = ChatRequest {
        model_id: routed.normalized_model_id.clone(),
        messages: request.messages.clone(),
        options: request.options.clone(),
    };

    let receiver = match routed.provider {
        RoutedProvider::Mlx => spawn_chat_stream(state.chat_runtime.clone(), normalized_request),
        RoutedProvider::Llamacpp => {
            spawn_provider_compat_stream(state.llamacpp_provider.clone(), normalized_request)
        }
        RoutedProvider::Ollama => {
            spawn_ollama_stream(state.ollama_provider.clone(), normalized_request)
        }
    };

    let stream = ReceiverStream::new(receiver).map(|event| {
        let mut payload = serde_json::to_vec(&event).unwrap_or_else(|_| {
            b"{\"event\":\"error\",\"message\":\"serialization failed\"}".to_vec()
        });
        payload.push(b'\n');
        Ok::<Bytes, io::Error>(Bytes::from(payload))
    });

    let body = Body::from_stream(stream);
    let response = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/x-ndjson; charset=utf-8")
        .header("Cache-Control", "no-cache")
        .body(body)
        .map_err(|error| AppError::NotFound(format!("falha ao criar resposta: {error}")))?;

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::{
        annotate_agent_model_compatibility, find_workspace_root_from, has_workspace_marker,
    };
    use mlx_ollama_core::ModelDescriptor;
    use std::fs;

    #[test]
    fn workspace_marker_detects_git_and_skill_roots() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join(".git")).unwrap();
        assert!(has_workspace_marker(tmp.path()));

        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join(".claude").join("skills")).unwrap();
        assert!(has_workspace_marker(tmp.path()));
    }

    #[test]
    fn workspace_root_scans_ancestors_for_repo_markers() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        let nested = root
            .join("apps")
            .join("desktop-ui")
            .join("src-tauri")
            .join("target")
            .join("release");
        fs::create_dir_all(root.join(".claude").join("skills")).unwrap();
        fs::create_dir_all(&nested).unwrap();

        let resolved = find_workspace_root_from(&nested).unwrap();
        assert_eq!(resolved, root);
    }

    #[test]
    fn annotates_tool_ready_and_chat_only_models() {
        let ready = annotate_agent_model_compatibility(ModelDescriptor {
            id: "ollama::qwen3.5:9b".to_string(),
            name: "qwen3.5:9b [Ollama]".to_string(),
            provider: "ollama".to_string(),
            path: "qwen3.5:9b".to_string(),
            is_available: true,
            agent_tool_mode: None,
            agent_tool_reason: None,
            agent_recommended: false,
        });
        assert_eq!(ready.agent_tool_mode.as_deref(), Some("tool_ready"));
        assert!(ready.agent_recommended);

        let chat_only = annotate_agent_model_compatibility(ModelDescriptor {
            id: "ollama::deepseek-r1:8b".to_string(),
            name: "deepseek-r1:8b [Ollama]".to_string(),
            provider: "ollama".to_string(),
            path: "deepseek-r1:8b".to_string(),
            is_available: true,
            agent_tool_mode: None,
            agent_tool_reason: None,
            agent_recommended: false,
        });
        assert_eq!(chat_only.agent_tool_mode.as_deref(), Some("chat_only"));
    }
}

fn spawn_ollama_stream(
    provider: Arc<OllamaProvider>,
    request: ChatRequest,
) -> mpsc::Receiver<ChatStreamEvent> {
    let (tx, rx) = mpsc::channel(64);

    tokio::spawn(async move {
        let started = Instant::now();
        if tx.send(ChatStreamEvent::status("thinking")).await.is_err() {
            return;
        }

        let response = match provider
            .begin_chat_stream(&request.model_id, &request.messages, &request.options)
            .await
        {
            Ok(response) => response,
            Err(error) => {
                let _ = tx.send(ChatStreamEvent::error(error.to_string())).await;
                return;
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let message = response.text().await.unwrap_or_default();
            let _ = tx
                .send(ChatStreamEvent::error(format!(
                    "ollama stream falhou com HTTP {status}: {message}"
                )))
                .await;
            return;
        }

        let mut parser = OllamaThinkingParser::default();
        let mut buffer = Vec::<u8>::new();
        let mut prompt_tokens = 0_usize;
        let mut completion_tokens = 0_usize;
        let mut total_latency_ms = started.elapsed().as_millis() as u64;
        let mut answer_status_sent = false;
        let mut response = response;

        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    buffer.extend_from_slice(&chunk);

                    while let Some(newline_index) = buffer.iter().position(|byte| *byte == b'\n') {
                        let line_bytes: Vec<u8> = buffer.drain(..=newline_index).collect();
                        let line = String::from_utf8_lossy(&line_bytes);
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }

                        let payload: serde_json::Value = match serde_json::from_str(trimmed) {
                            Ok(payload) => payload,
                            Err(error) => {
                                let _ = tx
                                    .send(ChatStreamEvent::error(format!(
                                        "falha parseando stream do Ollama: {error}"
                                    )))
                                    .await;
                                return;
                            }
                        };

                        let content = payload
                            .pointer("/message/content")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        let thinking = payload
                            .pointer("/message/thinking")
                            .and_then(Value::as_str)
                            .unwrap_or_default();

                        if !thinking.is_empty()
                            && tx
                                .send(ChatStreamEvent::thinking_delta(thinking.to_string()))
                                .await
                                .is_err()
                        {
                            return;
                        }

                        for event in parser.push(content) {
                            let is_answer = event.event == "answer_delta";
                            if is_answer && !answer_status_sent {
                                answer_status_sent = true;
                                if tx.send(ChatStreamEvent::status("answering")).await.is_err() {
                                    return;
                                }
                            }
                            if tx.send(event).await.is_err() {
                                return;
                            }
                        }

                        if payload
                            .get("done")
                            .and_then(Value::as_bool)
                            .unwrap_or(false)
                        {
                            prompt_tokens = payload
                                .get("prompt_eval_count")
                                .and_then(Value::as_u64)
                                .unwrap_or(0) as usize;
                            completion_tokens = payload
                                .get("eval_count")
                                .and_then(Value::as_u64)
                                .unwrap_or(completion_tokens as u64)
                                as usize;
                            total_latency_ms = payload
                                .get("total_duration")
                                .and_then(Value::as_u64)
                                .map(|nanos| nanos / 1_000_000)
                                .unwrap_or_else(|| started.elapsed().as_millis() as u64);

                            for event in parser.finish() {
                                if tx.send(event).await.is_err() {
                                    return;
                                }
                            }

                            let _ = tx
                                .send(ChatStreamEvent {
                                    event: "done".to_string(),
                                    status: Some("completed".to_string()),
                                    delta: None,
                                    message: None,
                                    prompt_tokens: Some(prompt_tokens),
                                    completion_tokens: Some(completion_tokens),
                                    total_tokens: Some(prompt_tokens + completion_tokens),
                                    prompt_tps: None,
                                    generation_tps: None,
                                    peak_memory_gb: None,
                                    latency_ms: Some(total_latency_ms),
                                    raw_metrics: None,
                                    airllm_required: None,
                                    airllm_used: None,
                                })
                                .await;
                            return;
                        }
                    }
                }
                Ok(None) => {
                    if !buffer.is_empty() {
                        let line = String::from_utf8_lossy(&buffer);
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            if let Ok(payload) = serde_json::from_str::<Value>(trimmed) {
                                let content = payload
                                    .pointer("/message/content")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default();
                                let thinking = payload
                                    .pointer("/message/thinking")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default();

                                if !thinking.is_empty()
                                    && tx
                                        .send(ChatStreamEvent::thinking_delta(thinking.to_string()))
                                        .await
                                        .is_err()
                                {
                                    return;
                                }

                                for event in parser.push(content) {
                                    let is_answer = event.event == "answer_delta";
                                    if is_answer && !answer_status_sent {
                                        answer_status_sent = true;
                                        if tx
                                            .send(ChatStreamEvent::status("answering"))
                                            .await
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }
                                    if tx.send(event).await.is_err() {
                                        return;
                                    }
                                }
                            }
                        }
                    }

                    for event in parser.finish() {
                        if tx.send(event).await.is_err() {
                            return;
                        }
                    }
                    let _ = tx
                        .send(ChatStreamEvent {
                            event: "done".to_string(),
                            status: Some("completed".to_string()),
                            delta: None,
                            message: None,
                            prompt_tokens: Some(prompt_tokens),
                            completion_tokens: Some(completion_tokens),
                            total_tokens: Some(prompt_tokens + completion_tokens),
                            prompt_tps: None,
                            generation_tps: None,
                            peak_memory_gb: None,
                            latency_ms: Some(total_latency_ms),
                            raw_metrics: None,
                            airllm_required: None,
                            airllm_used: None,
                        })
                        .await;
                    return;
                }
                Err(error) => {
                    let _ = tx
                        .send(ChatStreamEvent::error(format!(
                            "falha lendo stream do Ollama: {error}"
                        )))
                        .await;
                    return;
                }
            }
        }
    });

    rx
}

#[derive(Default)]
struct OllamaThinkingParser {
    carry: String,
    in_thinking: bool,
}

impl OllamaThinkingParser {
    fn push(&mut self, chunk: &str) -> Vec<ChatStreamEvent> {
        if chunk.is_empty() {
            return Vec::new();
        }

        let mut events = Vec::new();
        let mut remaining = format!("{}{}", self.carry, chunk);
        self.carry.clear();

        loop {
            let tag = if self.in_thinking {
                "</think>"
            } else {
                "<think>"
            };

            if let Some(index) = remaining.find(tag) {
                let head = remaining[..index].to_string();
                self.emit_segment(&mut events, head);
                remaining = remaining[index + tag.len()..].to_string();
                self.in_thinking = !self.in_thinking;
                continue;
            }

            let split_index = split_for_partial_tag(&remaining, tag);
            let head = remaining[..split_index].to_string();
            self.emit_segment(&mut events, head);
            self.carry = remaining[split_index..].to_string();
            break;
        }

        events
    }

    fn finish(&mut self) -> Vec<ChatStreamEvent> {
        let mut events = Vec::new();
        if !self.carry.is_empty() {
            let tail = std::mem::take(&mut self.carry);
            self.emit_segment(&mut events, tail);
        }
        events
    }

    fn emit_segment(&self, events: &mut Vec<ChatStreamEvent>, segment: String) {
        if segment.is_empty() {
            return;
        }

        if self.in_thinking {
            events.push(ChatStreamEvent::thinking_delta(segment));
        } else {
            events.push(ChatStreamEvent::answer_delta(segment));
        }
    }
}

fn split_for_partial_tag(input: &str, tag: &str) -> usize {
    let mut keep = 0_usize;
    let max = input.len().min(tag.len().saturating_sub(1));

    for length in 1..=max {
        if input.ends_with(&tag[..length]) {
            keep = length;
        }
    }

    input.len().saturating_sub(keep)
}

async fn brave_web_search(
    State(state): State<AppState>,
    Json(request): Json<BraveSearchRequest>,
) -> Result<Json<BraveSearchResponse>, AppError> {
    let query = request.query.trim();
    let request_api_key = request
        .api_key
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let shared_env_api_key = if request_api_key.is_empty() {
        let env_values = read_environment_values()?;
        resolve_environment_value(&env_values, "BRAVE_API_KEY").unwrap_or_default()
    } else {
        String::new()
    };
    let server_api_key = state
        .brave_api_key
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let api_key = if !request_api_key.is_empty() {
        request_api_key.clone()
    } else if !shared_env_api_key.is_empty() {
        shared_env_api_key.clone()
    } else {
        server_api_key
    };
    let max_results = request.max_results.unwrap_or(5).clamp(1, 10);

    if query.is_empty() {
        return Err(AppError::Provider(ProviderError::InvalidRequest {
            details: "query nao pode ser vazio".to_string(),
        }));
    }

    if api_key.is_empty() {
        return Err(AppError::Provider(ProviderError::InvalidRequest {
            details:
                "api_key Brave nao configurada (Configuracoes > Environment > BRAVE_API_KEY ou APP_BRAVE_API_KEY)"
                    .to_string(),
        }));
    }

    let key_source = if !request_api_key.is_empty() {
        "request".to_string()
    } else if !shared_env_api_key.is_empty() {
        "environment".to_string()
    } else {
        "server".to_string()
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(18))
        .build()
        .map_err(|source| {
            AppError::Provider(ProviderError::Io {
                context: "falha criando cliente Brave API".to_string(),
                source: io::Error::other(source.to_string()),
            })
        })?;

    let response = client
        .get("https://api.search.brave.com/res/v1/web/search")
        .query(&[
            ("q", query),
            ("count", &max_results.to_string()),
            ("safesearch", "moderate"),
        ])
        .header("Accept", "application/json")
        .header("X-Subscription-Token", api_key)
        .send()
        .await
        .map_err(|source| {
            AppError::Provider(ProviderError::Io {
                context: "falha consultando Brave API".to_string(),
                source: io::Error::other(source.to_string()),
            })
        })?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(AppError::Provider(ProviderError::Unavailable {
            details: format!("Brave API retornou HTTP {status}: {}", body.trim()),
        }));
    }

    let parsed = serde_json::from_str::<Value>(&body).map_err(|source| {
        AppError::Provider(ProviderError::Io {
            context: "falha parseando resposta Brave API".to_string(),
            source: io::Error::other(source.to_string()),
        })
    })?;

    let results = parsed
        .pointer("/web/results")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let title = entry
                        .get("title")
                        .and_then(Value::as_str)?
                        .trim()
                        .to_string();
                    let url = entry
                        .get("url")
                        .or_else(|| entry.get("profile").and_then(|value| value.get("url")))
                        .and_then(Value::as_str)?
                        .trim()
                        .to_string();
                    let description = entry
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or("")
                        .to_string();

                    if title.is_empty() || url.is_empty() {
                        return None;
                    }

                    Some(BraveSearchResultItem {
                        title,
                        url,
                        description,
                    })
                })
                .take(max_results)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(Json(BraveSearchResponse {
        query: query.to_string(),
        key_source,
        results,
    }))
}

fn spawn_provider_compat_stream(
    provider: Arc<dyn ModelProvider>,
    request: ChatRequest,
) -> mpsc::Receiver<ChatStreamEvent> {
    let (tx, rx) = mpsc::channel(16);

    tokio::spawn(async move {
        let started = Instant::now();
        if tx.send(ChatStreamEvent::status("waiting")).await.is_err() {
            return;
        }

        match provider.chat(request).await {
            Ok(response) => {
                let answer = response.message.content.trim().to_string();
                if !answer.is_empty() {
                    let _ = tx.send(ChatStreamEvent::answer_delta(answer)).await;
                }

                let latency_ms = response
                    .latency_ms
                    .max(started.elapsed().as_millis() as u64);

                let done_event = ChatStreamEvent {
                    event: "done".to_string(),
                    status: Some("completed".to_string()),
                    delta: None,
                    message: None,
                    prompt_tokens: Some(response.usage.prompt_tokens),
                    completion_tokens: Some(response.usage.completion_tokens),
                    total_tokens: Some(response.usage.total_tokens),
                    prompt_tps: None,
                    generation_tps: None,
                    peak_memory_gb: None,
                    latency_ms: Some(latency_ms),
                    raw_metrics: None,
                    airllm_required: None,
                    airllm_used: None,
                };
                let _ = tx.send(done_event).await;
            }
            Err(error) => {
                let _ = tx.send(ChatStreamEvent::error(error.to_string())).await;
            }
        }
    });

    rx
}

async fn list_chat_models(state: &AppState) -> Result<Vec<ModelDescriptor>, ProviderError> {
    match state.provider_mode {
        LocalProviderMode::Mlx => state.mlx_provider.list_models().await,
        LocalProviderMode::Llamacpp => state.llamacpp_provider.list_models().await,
        LocalProviderMode::Ollama => state.ollama_provider.list_models().await,
        LocalProviderMode::Auto => {
            let mlx_future =
                tokio::time::timeout(Duration::from_secs(3), state.mlx_provider.list_models());
            let llamacpp_future = tokio::time::timeout(
                Duration::from_secs(3),
                state.llamacpp_provider.list_models(),
            );
            let ollama_future =
                tokio::time::timeout(Duration::from_secs(2), state.ollama_provider.list_models());

            let (mlx_result, llamacpp_result, ollama_result) =
                tokio::join!(mlx_future, llamacpp_future, ollama_future);

            let mlx_models = match mlx_result {
                Ok(Ok(models)) => models,
                Ok(Err(error)) => {
                    warn!("mlx unavailable while listing models in auto mode: {error}");
                    Vec::new()
                }
                Err(_) => {
                    warn!("mlx model listing timed out in auto mode");
                    Vec::new()
                }
            };

            let llamacpp_models = match llamacpp_result {
                Ok(Ok(models)) => models,
                Ok(Err(error)) => {
                    warn!("llamacpp unavailable while listing models in auto mode: {error}");
                    Vec::new()
                }
                Err(_) => {
                    warn!("llamacpp model listing timed out in auto mode");
                    Vec::new()
                }
            };

            let ollama_models = match ollama_result {
                Ok(Ok(models)) => models,
                Ok(Err(error)) => {
                    debug!("ollama unavailable while listing models in auto mode: {error}");
                    Vec::new()
                }
                Err(_) => {
                    debug!("ollama model listing timed out in auto mode");
                    Vec::new()
                }
            };

            let mut combined = Vec::new();
            for model in mlx_models {
                combined.push(ModelDescriptor {
                    id: format!("mlx::{}", model.id),
                    name: format!("{} [MLX]", model.name),
                    provider: model.provider,
                    path: model.path,
                    is_available: model.is_available,
                    agent_tool_mode: None,
                    agent_tool_reason: None,
                    agent_recommended: false,
                });
            }

            for model in llamacpp_models {
                combined.push(ModelDescriptor {
                    id: format!("llama::{}", model.id),
                    name: format!("{} [llama.cpp]", model.name),
                    provider: model.provider,
                    path: model.path,
                    is_available: model.is_available,
                    agent_tool_mode: None,
                    agent_tool_reason: None,
                    agent_recommended: false,
                });
            }

            for model in ollama_models {
                combined.push(ModelDescriptor {
                    id: format!("ollama::{}", model.id),
                    name: format!("{} [Ollama]", model.name),
                    provider: model.provider,
                    path: model.path,
                    is_available: model.is_available,
                    agent_tool_mode: None,
                    agent_tool_reason: None,
                    agent_recommended: false,
                });
            }

            combined.sort_by(|left, right| {
                let by_provider = left.provider.cmp(&right.provider);
                if by_provider.is_eq() {
                    return left.name.to_lowercase().cmp(&right.name.to_lowercase());
                }
                by_provider
            });

            Ok(combined)
        }
    }
}

fn annotate_agent_model_compatibility(mut model: ModelDescriptor) -> ModelDescriptor {
    let combined = format!("{} {}", model.id, model.name).to_ascii_lowercase();

    let (mode, reason, recommended) = if model.provider.trim().eq_ignore_ascii_case("ollama") {
        if is_embedding_or_vision_model(&combined) {
            (
                Some("chat_only".to_string()),
                Some("familia focada em embedding, visao ou uso nao agentico".to_string()),
                false,
            )
        } else if is_known_chat_only_ollama_family(&combined) {
            (
                Some("chat_only".to_string()),
                Some(
                    "familia local conhecida por rejeitar tool calling no runtime atual"
                        .to_string(),
                ),
                false,
            )
        } else if is_known_tool_ready_ollama_family(&combined) {
            (
                Some("tool_ready".to_string()),
                Some("familia validada para tool calling no agent local".to_string()),
                combined.contains("qwen3.5:9b"),
            )
        } else {
            (
                Some("unknown".to_string()),
                Some("compatibilidade de tools ainda nao validada neste runtime".to_string()),
                false,
            )
        }
    } else {
        (
            Some("chat_only".to_string()),
            Some("provider local atual nao expõe tool calling no agent".to_string()),
            false,
        )
    };

    model.agent_tool_mode = mode;
    model.agent_tool_reason = reason;
    model.agent_recommended = recommended;
    model
}

fn is_embedding_or_vision_model(model: &str) -> bool {
    [
        "embed",
        "embedding",
        "nomic-embed",
        "mxbai-embed",
        "qwen3-vl",
        "vision",
        "-vl",
    ]
    .iter()
    .any(|needle| model.contains(needle))
}

fn is_known_chat_only_ollama_family(model: &str) -> bool {
    ["deepseek-r1", "dolphin3", "dolphin-mixtral", "mythomax"]
        .iter()
        .any(|needle| model.contains(needle))
}

fn is_known_tool_ready_ollama_family(model: &str) -> bool {
    [
        "llama3.1",
        "qwen2.5",
        "qwen2.5-coder",
        "qwen3:8b",
        "qwen3:14b",
        "qwen3.5:9b",
    ]
    .iter()
    .any(|needle| model.contains(needle))
}

/// Try to route a chat request to a cloud provider (e.g. "deepseek:model-name").
/// Returns `None` if the model_id doesn't match any cloud provider.
async fn try_cloud_chat(
    state: &AppState,
    request: &ChatRequest,
) -> Option<Result<ChatResponse, ProviderError>> {
    let model_id = request.model_id.trim();
    // Check known cloud provider prefixes.
    let known_cloud_providers = ["deepseek", "openai", "anthropic", "groq", "openrouter"];
    let mut cloud_provider: Option<&str> = None;
    let mut cloud_model: Option<&str> = None;

    for prefix in &known_cloud_providers {
        if let Some(rest) = model_id.strip_prefix(&format!("{prefix}:")) {
            cloud_provider = Some(prefix);
            cloud_model = Some(rest.trim());
            break;
        }
    }

    let provider_key = cloud_provider?;
    let model = cloud_model?;

    let config = model_catalog::find_cloud_config(provider_key)?;
    let api_key = model_catalog::get_api_key(state.vault.as_deref(), &config)?;

    let http_config = HttpLlmProviderConfig {
        provider_name: provider_key.to_string(),
        base_url: config.default_base_url.clone(),
        api_key: Some(api_key),
        api_kind: config.api_kind,
        timeout: std::time::Duration::from_secs(120),
        default_headers: BTreeMap::new(),
        default_models: vec![model.to_string()],
    };

    let provider = HttpLlmProvider::new(http_config);
    let cloud_request = ChatRequest {
        model_id: model.to_string(),
        messages: request.messages.clone(),
        options: request.options.clone(),
    };

    match provider.chat(cloud_request).await {
        Ok(response) => Some(Ok(response)),
        Err(e) => Some(Err(ProviderError::Unavailable {
            details: format!("cloud provider '{provider_key}': {e}"),
        })),
    }
}

async fn chat_with_routing(
    state: &AppState,
    request: ChatRequest,
) -> Result<ChatResponse, ProviderError> {
    // Check for cloud provider prefix (e.g. "deepseek:model-name").
    if let Some(cloud_result) = try_cloud_chat(state, &request).await {
        return cloud_result;
    }

    let provider_pinned = request_has_explicit_provider(&request.model_id);
    let routed = route_model_request(state, &request.model_id).await?;
    let request = ChatRequest {
        model_id: routed.normalized_model_id,
        messages: request.messages,
        options: request.options,
    };

    match send_chat_with_provider(state, routed.provider, request.clone()).await {
        Ok(response) => Ok(response),
        Err(error)
            if provider_pinned || !matches!(state.provider_mode, LocalProviderMode::Auto) =>
        {
            Err(error)
        }
        Err(mut last_error) => {
            warn!(
                "primary routed provider {:?} failed for '{}': {}. trying fallbacks",
                routed.provider, request.model_id, last_error
            );
            for provider in auto_provider_priority() {
                if provider == routed.provider {
                    continue;
                }
                if !provider_has_model(state, provider, &request.model_id).await {
                    continue;
                }

                match send_chat_with_provider(state, provider, request.clone()).await {
                    Ok(response) => {
                        warn!(
                            "fallback provider {:?} succeeded for '{}'",
                            provider, request.model_id
                        );
                        return Ok(response);
                    }
                    Err(error) => {
                        warn!(
                            "fallback provider {:?} also failed for '{}': {}",
                            provider, request.model_id, error
                        );
                        last_error = error;
                    }
                }
            }
            Err(last_error)
        }
    }
}

async fn route_model_request(
    state: &AppState,
    model_id: &str,
) -> Result<RoutedModel, ProviderError> {
    let (normalized_input, display_provider) = normalize_display_model_id(model_id);
    let trimmed = normalized_input.as_str();
    if trimmed.is_empty() {
        return Err(ProviderError::InvalidRequest {
            details: "model_id cannot be empty".to_string(),
        });
    }

    if let Some(normalized) = trimmed.strip_prefix("mlx::") {
        return Ok(RoutedModel {
            provider: RoutedProvider::Mlx,
            normalized_model_id: normalized.trim().to_string(),
        });
    }

    if let Some(normalized) = trimmed.strip_prefix("ollama::") {
        return Ok(RoutedModel {
            provider: RoutedProvider::Ollama,
            normalized_model_id: normalized.trim().to_string(),
        });
    }

    if let Some(normalized) = trimmed.strip_prefix("llama::") {
        return Ok(RoutedModel {
            provider: RoutedProvider::Llamacpp,
            normalized_model_id: normalized.trim().to_string(),
        });
    }

    if let Some(provider) = display_provider {
        return Ok(RoutedModel {
            provider,
            normalized_model_id: trimmed.to_string(),
        });
    }

    match state.provider_mode {
        LocalProviderMode::Mlx => {
            return Ok(RoutedModel {
                provider: RoutedProvider::Mlx,
                normalized_model_id: trimmed.to_string(),
            });
        }
        LocalProviderMode::Llamacpp => {
            return Ok(RoutedModel {
                provider: RoutedProvider::Llamacpp,
                normalized_model_id: trimmed.to_string(),
            });
        }
        LocalProviderMode::Ollama => {
            return Ok(RoutedModel {
                provider: RoutedProvider::Ollama,
                normalized_model_id: trimmed.to_string(),
            });
        }
        LocalProviderMode::Auto => {}
    }

    if looks_like_llamacpp_model_id(trimmed) {
        return Ok(RoutedModel {
            provider: RoutedProvider::Llamacpp,
            normalized_model_id: trimmed.to_string(),
        });
    }

    if looks_like_mlx_model_id(trimmed) {
        return Ok(RoutedModel {
            provider: RoutedProvider::Mlx,
            normalized_model_id: trimmed.to_string(),
        });
    }

    if looks_like_ollama_model_id(trimmed) {
        return Ok(RoutedModel {
            provider: RoutedProvider::Ollama,
            normalized_model_id: trimmed.to_string(),
        });
    }

    let mut fallback_provider = None;
    for provider in auto_provider_priority() {
        match list_models_for_provider(state, provider).await {
            Ok(models) => {
                if models
                    .iter()
                    .any(|entry| entry.id == trimmed || entry.path == trimmed)
                {
                    return Ok(RoutedModel {
                        provider,
                        normalized_model_id: trimmed.to_string(),
                    });
                }
                if fallback_provider.is_none() && !models.is_empty() {
                    fallback_provider = Some(provider);
                }
            }
            Err(error) => match provider {
                RoutedProvider::Ollama => {
                    warn!("ollama unavailable while routing model '{trimmed}': {error}");
                }
                RoutedProvider::Llamacpp => {
                    debug!("llamacpp unavailable while routing model '{trimmed}': {error}");
                }
                RoutedProvider::Mlx => {
                    debug!("mlx unavailable while routing model '{trimmed}': {error}");
                }
            },
        }
    }

    if let Some(provider) = fallback_provider {
        return Ok(RoutedModel {
            provider,
            normalized_model_id: trimmed.to_string(),
        });
    }

    Err(ProviderError::ModelNotFound {
        model_id: trimmed.to_string(),
    })
}

async fn send_chat_with_provider(
    state: &AppState,
    provider: RoutedProvider,
    request: ChatRequest,
) -> Result<ChatResponse, ProviderError> {
    match provider {
        RoutedProvider::Mlx => state.mlx_provider.chat(request).await,
        RoutedProvider::Llamacpp => state.llamacpp_provider.chat(request).await,
        RoutedProvider::Ollama => state.ollama_provider.chat(request).await,
    }
}

async fn list_models_for_provider(
    state: &AppState,
    provider: RoutedProvider,
) -> Result<Vec<ModelDescriptor>, ProviderError> {
    match provider {
        RoutedProvider::Mlx => state.mlx_provider.list_models().await,
        RoutedProvider::Llamacpp => state.llamacpp_provider.list_models().await,
        RoutedProvider::Ollama => state.ollama_provider.list_models().await,
    }
}

async fn provider_has_model(state: &AppState, provider: RoutedProvider, model_id: &str) -> bool {
    list_models_for_provider(state, provider)
        .await
        .map(|models| {
            models
                .iter()
                .any(|entry| entry.id == model_id || entry.path == model_id)
        })
        .unwrap_or(false)
}

fn auto_provider_priority() -> Vec<RoutedProvider> {
    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        vec![
            RoutedProvider::Mlx,
            RoutedProvider::Llamacpp,
            RoutedProvider::Ollama,
        ]
    } else {
        vec![
            RoutedProvider::Llamacpp,
            RoutedProvider::Ollama,
            RoutedProvider::Mlx,
        ]
    }
}

fn request_has_explicit_provider(model_id: &str) -> bool {
    let trimmed = model_id.trim();
    trimmed.starts_with("mlx::")
        || trimmed.starts_with("ollama::")
        || trimmed.starts_with("llama::")
        || trimmed.ends_with(" [MLX]")
        || trimmed.ends_with(" [Ollama]")
        || trimmed.ends_with(" [llama.cpp]")
}

fn normalize_display_model_id(model_id: &str) -> (String, Option<RoutedProvider>) {
    let trimmed = model_id.trim();

    if let Some(value) = trimmed.strip_suffix(" [Ollama]") {
        return (value.trim().to_string(), Some(RoutedProvider::Ollama));
    }

    if let Some(value) = trimmed.strip_suffix(" [MLX]") {
        return (value.trim().to_string(), Some(RoutedProvider::Mlx));
    }

    if let Some(value) = trimmed.strip_suffix(" [llama.cpp]") {
        return (value.trim().to_string(), Some(RoutedProvider::Llamacpp));
    }

    (trimmed.to_string(), None)
}

fn looks_like_mlx_model_id(model_id: &str) -> bool {
    let value = model_id.trim();
    value.starts_with('/')
        || value.contains('\\')
        || value.contains("/Users/")
        || value.starts_with("huggingface--")
}

fn looks_like_ollama_model_id(model_id: &str) -> bool {
    let value = model_id.trim();
    value.starts_with("ollama/")
        || (value.contains(':') && !value.contains('/') && !value.contains('\\'))
}

fn looks_like_llamacpp_model_id(model_id: &str) -> bool {
    let value = model_id.trim().to_lowercase();
    value.ends_with(".gguf")
        || value.contains(".gguf/")
        || value.contains("/gguf/")
        || value.contains("\\gguf\\")
}

async fn catalog_sources(
    State(state): State<AppState>,
) -> Result<Json<Vec<CatalogSourceDescriptor>>, AppError> {
    Ok(Json(state.catalog.list_sources()))
}

async fn catalog_models(
    State(state): State<AppState>,
    Query(query): Query<CatalogSearchQuery>,
) -> Result<Json<Vec<RemoteModelCard>>, AppError> {
    let models = state.catalog.search_models(query).await?;
    Ok(Json(models))
}

async fn catalog_create_download(
    State(state): State<AppState>,
    Json(request): Json<CreateDownloadRequest>,
) -> Result<Json<DownloadJob>, AppError> {
    let job = state.catalog.create_download(request).await?;
    Ok(Json(job))
}

async fn catalog_downloads(
    State(state): State<AppState>,
) -> Result<Json<Vec<DownloadJob>>, AppError> {
    let jobs = state.catalog.list_downloads().await;
    Ok(Json(jobs))
}

async fn catalog_download(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Json<DownloadJob>, AppError> {
    match state.catalog.get_download(&job_id).await {
        Some(job) => Ok(Json(job)),
        None => Err(AppError::NotFound(format!(
            "download job '{job_id}' nao encontrado"
        ))),
    }
}

async fn catalog_cancel_download(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Json<DownloadJob>, AppError> {
    let cancelled = state.catalog.cancel_download(&job_id).await?;
    Ok(Json(cancelled))
}

async fn environment(
    Query(query): Query<EnvironmentQuery>,
) -> Result<Json<EnvironmentResponse>, AppError> {
    let reveal = query.reveal.unwrap_or(false);
    let response = build_environment_response(reveal)?;
    Ok(Json(response))
}

async fn update_environment(
    Json(request): Json<EnvironmentUpdateRequest>,
) -> Result<Json<EnvironmentResponse>, AppError> {
    update_environment_file(request.values)?;
    let response = build_environment_response(false)?;
    Ok(Json(response))
}

const ENVIRONMENT_CATALOG: &[(&str, &str)] = &[
    ("OPENROUTER_API_KEY", "OpenRouter API key"),
    ("DEEPSEEK_API_KEY", "DeepSeek API key"),
    ("DEEPSEEK_BASE_URL", "DeepSeek base URL"),
    ("OPENAI_API_KEY", "OpenAI API key"),
    ("OPENAI_BASE_URL", "OpenAI-compatible base URL"),
    ("ANTHROPIC_API_KEY", "Anthropic API key"),
    ("GEMINI_API_KEY", "Gemini API key"),
    ("GROQ_API_KEY", "Groq API key"),
    ("ZAI_API_KEY", "Zhipu/ZAI API key"),
    ("ZHIPUAI_API_KEY", "Zhipu compatibility key"),
    ("DASHSCOPE_API_KEY", "DashScope API key"),
    ("MOONSHOT_API_KEY", "Moonshot API key"),
    ("MOONSHOT_API_BASE", "Moonshot base URL"),
    ("MINIMAX_API_KEY", "MiniMax API key"),
    ("MINIMAX_BASE_URL", "MiniMax base URL"),
    ("HOSTED_VLLM_API_KEY", "vLLM/OpenAI-compatible local key"),
    ("PERPLEXITY_API_KEY", "Perplexity API key"),
    ("QIANFAN_API_KEY", "Qianfan API key"),
    ("BRAVE_API_KEY", "Brave Search API key"),
    ("FIRECRAWL_API_KEY", "Firecrawl API key"),
    ("DEEPGRAM_API_KEY", "Deepgram API key"),
    ("ELEVENLABS_API_KEY", "ElevenLabs API key"),
    ("VOYAGE_API_KEY", "Voyage API key"),
    ("TELEGRAM_BOT_TOKEN", "Telegram bot token"),
    ("TELEGRAM_CHAT_ID", "Telegram chat id"),
    ("DISCORD_BOT_TOKEN", "Discord bot token"),
];

fn build_environment_response(reveal: bool) -> Result<EnvironmentResponse, AppError> {
    let cfg = AppConfig::load_settings().apply_env();
    let env_path = resolve_environment_path(&cfg);
    let env_example_path = resolve_environment_example_path(&env_path);

    let env_values = read_env_file_assignments_optional(&env_path)?;
    let env_example_values = read_env_file_assignments_optional(&env_example_path)?;

    let mut labels = BTreeMap::new();
    let mut keys = BTreeSet::new();
    for (key, label) in ENVIRONMENT_CATALOG {
        labels.insert((*key).to_string(), (*label).to_string());
        keys.insert((*key).to_string());
    }

    for key in env_values.keys().chain(env_example_values.keys()) {
        if should_expose_environment_key(key) {
            keys.insert(key.clone());
        }
    }

    let mut variables = Vec::with_capacity(keys.len());
    for key in keys {
        let file_value = env_values.get(&key).cloned().unwrap_or_default();
        let process_value = std::env::var(&key)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_default();
        let resolved_value = if !file_value.is_empty() {
            file_value.clone()
        } else {
            process_value.clone()
        };
        let source = if !file_value.is_empty() {
            "env_file"
        } else if !process_value.is_empty() {
            "process_env"
        } else if env_example_values.contains_key(&key) {
            "env_example"
        } else {
            "catalog"
        };

        variables.push(EnvironmentVariableView {
            label: labels
                .get(&key)
                .cloned()
                .unwrap_or_else(|| key.replace('_', " ")),
            key: key.clone(),
            value: if reveal {
                resolved_value.clone()
            } else {
                String::new()
            },
            masked: mask_environment_value(&resolved_value),
            source: source.to_string(),
            present: !resolved_value.is_empty(),
            is_secret: is_secret_environment_key(&key),
        });
    }

    Ok(EnvironmentResponse {
        env_path: env_path.display().to_string(),
        env_exists: env_path.exists(),
        env_example_path: env_example_path.display().to_string(),
        env_example_exists: env_example_path.exists(),
        variables,
    })
}

fn update_environment_file(updates: BTreeMap<String, String>) -> Result<(), AppError> {
    let mut normalized_updates = BTreeMap::new();
    for (raw_key, raw_value) in updates {
        let key = normalize_env_key(&raw_key);
        if key.is_empty() {
            continue;
        }
        normalized_updates.insert(key, raw_value.trim().to_string());
    }

    if normalized_updates.is_empty() {
        return Err(AppError::Provider(ProviderError::InvalidRequest {
            details: "nenhuma variavel valida recebida para atualizar environment".to_string(),
        }));
    }

    let cfg = AppConfig::load_settings().apply_env();
    let env_path = resolve_environment_path(&cfg);

    let mut lines = if env_path.exists() {
        fs::read_to_string(&env_path)
            .map_err(|source| {
                AppError::Provider(ProviderError::Io {
                    context: format!(
                        "Falha ao ler arquivo de environment ({})",
                        env_path.display()
                    ),
                    source,
                })
            })?
            .lines()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let mut touched = BTreeSet::new();
    for line in &mut lines {
        if let Some((key, _)) = parse_env_assignment_line(line) {
            if let Some(value) = normalized_updates.get(&key) {
                *line = format!("{key}={}", encode_env_value(value));
                touched.insert(key);
            }
        }
    }

    for (key, value) in normalized_updates {
        if !touched.contains(&key) {
            lines.push(format!("{key}={}", encode_env_value(&value)));
        }
    }

    if let Some(parent) = env_path.parent() {
        fs::create_dir_all(parent).map_err(|source| {
            AppError::Provider(ProviderError::Io {
                context: format!(
                    "Falha ao criar diretorio do environment ({})",
                    parent.display()
                ),
                source,
            })
        })?;
    }

    let mut next_content = lines.join("\n");
    if !next_content.ends_with('\n') {
        next_content.push('\n');
    }
    fs::write(&env_path, next_content).map_err(|source| {
        AppError::Provider(ProviderError::Io {
            context: format!(
                "Falha ao salvar arquivo de environment ({})",
                env_path.display()
            ),
            source,
        })
    })?;

    Ok(())
}

fn read_environment_values() -> Result<BTreeMap<String, String>, AppError> {
    let cfg = AppConfig::load_settings().apply_env();
    let env_path = resolve_environment_path(&cfg);
    read_env_file_assignments_optional(&env_path)
}

fn resolve_environment_value(values: &BTreeMap<String, String>, key: &str) -> Option<String> {
    let key = normalize_env_key(key);
    if key.is_empty() {
        return None;
    }
    values.get(&key).cloned().or_else(|| {
        std::env::var(&key)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn resolve_environment_path(cfg: &AppConfig) -> FsPathBuf {
    AppConfig::get_settings_path()
        .parent()
        .map(|parent| parent.join(".env"))
        .unwrap_or_else(|| default_app_environment_dir(cfg).join(".env"))
}

fn resolve_environment_example_path(env_path: &FsPath) -> FsPathBuf {
    env_path
        .parent()
        .map(|parent| parent.join(".env.example"))
        .unwrap_or_else(|| FsPathBuf::from(".env.example"))
}

fn default_app_environment_dir(cfg: &AppConfig) -> FsPathBuf {
    AppConfig::get_settings_path()
        .parent()
        .map(FsPathBuf::from)
        .or_else(|| cfg.models_dir.parent().map(FsPathBuf::from))
        .unwrap_or_else(|| FsPathBuf::from("."))
}

fn read_env_file_assignments_optional(path: &FsPath) -> Result<BTreeMap<String, String>, AppError> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }

    let body = fs::read_to_string(path).map_err(|source| {
        AppError::Provider(ProviderError::Io {
            context: format!("Falha ao ler arquivo de environment ({})", path.display()),
            source,
        })
    })?;

    let mut values = BTreeMap::new();
    for line in body.lines() {
        if let Some((key, value)) = parse_env_assignment_line(line) {
            values.insert(key, value);
        }
    }
    Ok(values)
}

fn parse_env_assignment_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let (raw_key, raw_value) = trimmed.split_once('=')?;
    let key = normalize_env_key(raw_key);
    if key.is_empty() {
        return None;
    }
    let value = decode_env_value(raw_value.trim());
    Some((key, value))
}

fn decode_env_value(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 2 {
        let quoted = (trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\''));
        if quoted {
            let inner = &trimmed[1..trimmed.len() - 1];
            return inner
                .replace("\\n", "\n")
                .replace("\\r", "\r")
                .replace("\\t", "\t")
                .replace("\\\"", "\"")
                .replace("\\\\", "\\");
        }
    }
    trimmed.to_string()
}

fn encode_env_value(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_string();
    }

    let requires_quotes = value
        .chars()
        .any(|ch| ch.is_whitespace() || matches!(ch, '#' | '"' | '\'' | '\\'));
    if !requires_quotes {
        return value.to_string();
    }

    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn should_expose_environment_key(key: &str) -> bool {
    let normalized = normalize_env_key(key);
    if normalized.is_empty() {
        return false;
    }
    normalized.contains('_')
        && normalized
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

fn is_secret_environment_key(key: &str) -> bool {
    let upper = key.trim().to_ascii_uppercase();
    upper.contains("KEY")
        || upper.contains("TOKEN")
        || upper.contains("SECRET")
        || upper.contains("PASSWORD")
}

fn mask_environment_value(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.len() <= 8 {
        return "*".repeat(trimmed.len());
    }

    let prefix = trimmed.chars().take(4).collect::<String>();
    let suffix = trimmed
        .chars()
        .rev()
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    let hidden = "*".repeat(trimmed.chars().count().saturating_sub(6));
    format!("{prefix}{hidden}{suffix}")
}
