mod agent_api;
mod agent_runtime_tools;
mod catalog;
mod channels;
mod chat_stream;
mod config;
mod openclaw;
mod plugins;
mod secrets_vault;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path as FsPath, PathBuf as FsPathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use bytes::Bytes;
use catalog::{
    CatalogConfig, CatalogError, CatalogSearchQuery, CatalogService, CatalogSourceDescriptor,
    CreateDownloadRequest, DownloadJob, RemoteModelCard,
};
use chat_stream::{spawn_chat_stream, ChatRuntimeConfig, ChatStreamEvent};
use config::AppConfig;
use llamacpp_provider::{LlamaCppProvider, LlamaCppProviderConfig};
use mlx_ollama_core::{
    ChatMessage, ChatRequest, ChatResponse, GenerationOptions, MessageRole, ModelDescriptor,
    ModelProvider, ProviderError,
};
use mlx_provider::{MlxProvider, MlxProviderConfig};
use ollama_provider::{OllamaProvider, OllamaProviderConfig};
use openclaw::{
    OpenClawChatRequest, OpenClawChatResponse, OpenClawCloudModel, OpenClawCurrentModel,
    OpenClawError, OpenClawLogChunkResponse, OpenClawLogQuery, OpenClawModelsStateResponse,
    OpenClawObservabilityResponse, OpenClawRuntime, OpenClawRuntimeActionRequest,
    OpenClawRuntimeActionResponse, OpenClawRuntimeConfig, OpenClawRuntimeStateResponse,
    OpenClawSetModelRequest, OpenClawStatusResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::process::{Child as TokioChild, Command as TokioCommand};
use tokio::sync::{mpsc, Mutex};
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

#[derive(Clone)]
struct AppState {
    provider_mode: LocalProviderMode,
    mlx_provider: Arc<MlxProvider>,
    llamacpp_provider: Arc<LlamaCppProvider>,
    ollama_provider: Arc<OllamaProvider>,
    brave_api_key: Option<String>,
    openclaw_local_provider: Arc<MlxProvider>,
    catalog: Arc<CatalogService>,
    chat_runtime: ChatRuntimeConfig,
    openclaw_runtime: Arc<OpenClawRuntime>,
    pub nanobot_runtime: Arc<Mutex<NanoBotRuntimeManager>>,
    pub session_store: Arc<mlx_agent_core::SessionStore>,
    pub agent_state: agent_api::AgentState,
    pub plugin_manager: Arc<PluginManager>,
    pub channel_service: Arc<ChannelService>,
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

#[derive(Debug)]
struct RoutedModel {
    provider: RoutedProvider,
    normalized_model_id: String,
}

#[derive(Debug)]
enum AppError {
    Provider(ProviderError),
    Catalog(CatalogError),
    OpenClaw(OpenClawError),
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

impl From<OpenClawError> for AppError {
    fn from(value: OpenClawError) -> Self {
        Self::OpenClaw(value)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message, error_code) = match self {
            AppError::Provider(error) => map_provider_error(error),
            AppError::Catalog(error) => map_catalog_error(error),
            AppError::OpenClaw(error) => map_openclaw_error(error),
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

fn map_openclaw_error(error: OpenClawError) -> (StatusCode, String, Option<String>) {
    match error {
        OpenClawError::BadRequest(message) => (StatusCode::BAD_REQUEST, message, None),
        OpenClawError::Io { context, source } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{context}: {source}"),
            None,
        ),
        OpenClawError::CommandFailed { command, stderr } => (
            StatusCode::BAD_GATEWAY,
            format!("command failed: {command}; stderr: {stderr}"),
            None,
        ),
        OpenClawError::Parse { details } => (StatusCode::BAD_GATEWAY, details, None),
        OpenClawError::Timeout { seconds } => (
            StatusCode::GATEWAY_TIMEOUT,
            format!("timeout ao consultar openclaw ({seconds}s)"),
            None,
        ),
        OpenClawError::Unavailable(message) => (StatusCode::SERVICE_UNAVAILABLE, message, None),
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
    status: &'static str,
    provider: &'static str,
}

#[derive(Debug, Serialize)]
struct OpenClawLocalModel {
    id: String,
    name: String,
    path: String,
}

#[derive(Debug, Serialize)]
struct OpenClawModelsResponse {
    session_key: String,
    current: OpenClawCurrentModel,
    cloud_models: Vec<OpenClawCloudModel>,
    local_models: Vec<OpenClawLocalModel>,
}

#[derive(Debug, Deserialize)]
struct OpenClawModelRequest {
    source: String,
    model_reference: Option<String>,
    local_model_id: Option<String>,
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
struct OpenClawEnvironmentQuery {
    reveal: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct OpenClawEnvironmentUpdateRequest {
    values: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
struct OpenClawEnvironmentVariable {
    key: String,
    label: String,
    value: String,
    masked: String,
    source: String,
    present: bool,
    is_secret: bool,
}

#[derive(Debug, Serialize)]
struct OpenClawEnvironmentResponse {
    env_path: String,
    env_exists: bool,
    env_example_path: String,
    env_example_exists: bool,
    variables: Vec<OpenClawEnvironmentVariable>,
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

    let catalog = Arc::new(CatalogService::new(CatalogConfig {
        hf_api_base: cfg.hf_api_base.clone(),
        hf_token: cfg.hf_token.clone(),
        downloads_root: cfg.remote_downloads_dir.clone(),
        search_limit_default: cfg.catalog_search_limit,
        download_timeout: cfg.catalog_download_timeout,
    })?);

    let state = AppState {
        provider_mode,
        mlx_provider: mlx_provider.clone(),
        llamacpp_provider,
        ollama_provider,
        brave_api_key: cfg.brave_api_key.clone(),
        openclaw_local_provider: mlx_provider,
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
        openclaw_runtime: Arc::new(OpenClawRuntime::new(OpenClawRuntimeConfig {
            node_command: cfg.openclaw_node_command.clone(),
            cli_path: cfg.openclaw_cli_path.clone(),
            state_dir: cfg.openclaw_state_dir.clone(),
            gateway_token: cfg.openclaw_gateway_token.clone(),
            session_key: cfg.openclaw_session_key.clone(),
            timeout: cfg.openclaw_timeout,
            gateway_log: cfg.openclaw_gateway_log.clone(),
            error_log: cfg.openclaw_error_log.clone(),
            sync_log: cfg.openclaw_sync_log.clone(),
        })),
        nanobot_runtime: Arc::new(Mutex::new(NanoBotRuntimeManager::new())),
        agent_state: agent_api::AgentState {
            default_workspace: std::env::var("APP_AGENT_WORKSPACE")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default()),
            approval: Arc::new(mlx_agent_core::approval::DefaultApprovalService::new()),
            event_bus: Arc::new(mlx_agent_core::EventBus::default()),
            audit: Arc::new(mlx_agent_core::AuditLog::new(
                std::env::temp_dir().join("mlx-pilot-audit"),
            )),
            memory: Arc::new(mlx_agent_core::MemoryStore::new(
                AppConfig::get_settings_path()
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .join("memory"),
            )),
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
    };

    let app = Router::new()
        .route("/config", get(get_config).post(update_config))
        .route("/health", get(health))
        .route("/models", get(list_models))
        .route("/models/rename", post(rename_model))
        .route("/models/{model_id}", delete(delete_model))
        .route("/chat", post(chat))
        .route("/chat/stream", post(chat_stream))
        .route("/web/brave/search", post(brave_web_search))
        .route("/openclaw/status", get(openclaw_status))
        .route("/openclaw/observability", get(openclaw_observability))
        .route(
            "/openclaw/runtime",
            get(openclaw_runtime_status).post(openclaw_runtime_action),
        )
        .route("/openclaw/models", get(openclaw_models))
        .route("/openclaw/logs", get(openclaw_logs))
        .route("/openclaw/model", post(openclaw_set_model))
        .route("/openclaw/chat", post(openclaw_chat))
        .route("/openclaw/install", post(openclaw_install))
        .route(
            "/environment",
            get(openclaw_environment).post(openclaw_update_environment),
        )
        .route(
            "/openclaw/environment",
            get(openclaw_environment).post(openclaw_update_environment),
        )
        .route("/nanobot/status", get(nanobot_status))
        .route("/nanobot/onboard", post(nanobot_onboard))
        .route("/nanobot/install", post(nanobot_install))
        .route(
            "/nanobot/runtime",
            get(nanobot_runtime_status).post(nanobot_runtime_action),
        )
        .route("/nanobot/chat", post(nanobot_chat))
        .route("/nanobot/logs", get(nanobot_logs))
        .route("/nanobot/observability", get(nanobot_observability))
        .route("/nanobot/models", get(nanobot_models))
        .route(
            "/nanobot/model",
            get(nanobot_get_model).post(nanobot_set_model),
        )
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
        .route("/agent/run", post(agent_api::agent_run))
        .route("/agent/providers", get(agent_api::agent_providers))
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
    Json(HealthBody {
        status: "ok",
        provider: state.provider_mode.label(),
    })
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
    let models = list_chat_models(&state).await?;
    Ok(Json(models))
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
            spawn_provider_compat_stream(state.ollama_provider.clone(), normalized_request)
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
        let env_values = read_openclaw_environment_values()?;
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
            let mlx_models = match state.mlx_provider.list_models().await {
                Ok(models) => models,
                Err(error) => {
                    warn!("mlx unavailable while listing models in auto mode: {error}");
                    Vec::new()
                }
            };
            let llamacpp_models = match state.llamacpp_provider.list_models().await {
                Ok(models) => models,
                Err(error) => {
                    warn!("llamacpp unavailable while listing models in auto mode: {error}");
                    Vec::new()
                }
            };

            let ollama_models = match state.ollama_provider.list_models().await {
                Ok(models) => models,
                Err(error) => {
                    debug!("ollama unavailable while listing models in auto mode: {error}");
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
                });
            }

            for model in llamacpp_models {
                combined.push(ModelDescriptor {
                    id: format!("llama::{}", model.id),
                    name: format!("{} [llama.cpp]", model.name),
                    provider: model.provider,
                    path: model.path,
                    is_available: model.is_available,
                });
            }

            for model in ollama_models {
                combined.push(ModelDescriptor {
                    id: format!("ollama::{}", model.id),
                    name: format!("{} [Ollama]", model.name),
                    provider: model.provider,
                    path: model.path,
                    is_available: model.is_available,
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

async fn chat_with_routing(
    state: &AppState,
    request: ChatRequest,
) -> Result<ChatResponse, ProviderError> {
    let routed = route_model_request(state, &request.model_id).await?;
    let request = ChatRequest {
        model_id: routed.normalized_model_id,
        messages: request.messages,
        options: request.options,
    };

    match routed.provider {
        RoutedProvider::Mlx => state.mlx_provider.chat(request).await,
        RoutedProvider::Llamacpp => state.llamacpp_provider.chat(request).await,
        RoutedProvider::Ollama => state.ollama_provider.chat(request).await,
    }
}

async fn route_model_request(
    state: &AppState,
    model_id: &str,
) -> Result<RoutedModel, ProviderError> {
    let trimmed = model_id.trim();
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

    let mlx_models = state.mlx_provider.list_models().await?;
    if mlx_models
        .iter()
        .any(|entry| entry.id == trimmed || entry.path == trimmed)
    {
        return Ok(RoutedModel {
            provider: RoutedProvider::Mlx,
            normalized_model_id: trimmed.to_string(),
        });
    }

    match state.llamacpp_provider.list_models().await {
        Ok(llamacpp_models) => {
            if llamacpp_models
                .iter()
                .any(|entry| entry.id == trimmed || entry.path == trimmed)
            {
                return Ok(RoutedModel {
                    provider: RoutedProvider::Llamacpp,
                    normalized_model_id: trimmed.to_string(),
                });
            }
        }
        Err(error) => {
            debug!("llamacpp unavailable while routing model '{trimmed}': {error}");
        }
    }

    match state.ollama_provider.list_models().await {
        Ok(ollama_models) => {
            if ollama_models
                .iter()
                .any(|entry| entry.id == trimmed || entry.path == trimmed)
            {
                return Ok(RoutedModel {
                    provider: RoutedProvider::Ollama,
                    normalized_model_id: trimmed.to_string(),
                });
            }
        }
        Err(error) => {
            warn!("ollama unavailable while routing model '{trimmed}': {error}");
        }
    }

    Ok(RoutedModel {
        provider: RoutedProvider::Mlx,
        normalized_model_id: trimmed.to_string(),
    })
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

async fn openclaw_status(
    State(state): State<AppState>,
) -> Result<Json<OpenClawStatusResponse>, AppError> {
    Ok(Json(state.openclaw_runtime.status().await))
}

async fn openclaw_observability(
    State(state): State<AppState>,
) -> Result<Json<OpenClawObservabilityResponse>, AppError> {
    let response = state.openclaw_runtime.observability().await?;
    Ok(Json(response))
}

async fn openclaw_runtime_status(
    State(state): State<AppState>,
) -> Result<Json<OpenClawRuntimeStateResponse>, AppError> {
    let status = state.openclaw_runtime.runtime_status().await?;
    Ok(Json(status))
}

async fn openclaw_runtime_action(
    State(state): State<AppState>,
    Json(request): Json<OpenClawRuntimeActionRequest>,
) -> Result<Json<OpenClawRuntimeActionResponse>, AppError> {
    let response = state.openclaw_runtime.runtime_action(request).await?;
    Ok(Json(response))
}

async fn openclaw_logs(
    State(state): State<AppState>,
    Query(query): Query<OpenClawLogQuery>,
) -> Result<Json<OpenClawLogChunkResponse>, AppError> {
    let chunk = state.openclaw_runtime.read_logs(query).await?;
    Ok(Json(chunk))
}

async fn openclaw_models(
    State(state): State<AppState>,
) -> Result<Json<OpenClawModelsResponse>, AppError> {
    let model_state: OpenClawModelsStateResponse = state.openclaw_runtime.models_state().await?;
    let local_models = state.openclaw_local_provider.list_models().await?;

    let mapped_local = local_models
        .into_iter()
        .map(|entry| OpenClawLocalModel {
            id: entry.id,
            name: entry.name,
            path: entry.path,
        })
        .collect::<Vec<_>>();

    Ok(Json(OpenClawModelsResponse {
        session_key: model_state.session_key,
        current: model_state.current,
        cloud_models: model_state.cloud_models,
        local_models: mapped_local,
    }))
}

async fn openclaw_set_model(
    State(state): State<AppState>,
    Json(request): Json<OpenClawModelRequest>,
) -> Result<Json<OpenClawCurrentModel>, AppError> {
    let source = request.source.trim().to_lowercase();

    if source == "cloud" {
        let result = state
            .openclaw_runtime
            .set_model(OpenClawSetModelRequest {
                source,
                model_reference: request.model_reference,
                local_model_path: None,
                local_model_name: None,
            })
            .await?;
        return Ok(Json(result));
    }

    if source == "local" {
        let model_id = request
            .local_model_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                AppError::OpenClaw(OpenClawError::BadRequest(
                    "local_model_id e obrigatorio para source=local".to_string(),
                ))
            })?;

        let local_models = state.openclaw_local_provider.list_models().await?;
        let selected = local_models
            .into_iter()
            .find(|entry| entry.id == model_id)
            .ok_or_else(|| {
                AppError::NotFound(format!("modelo local '{model_id}' nao encontrado"))
            })?;

        let result = state
            .openclaw_runtime
            .set_model(OpenClawSetModelRequest {
                source,
                model_reference: None,
                local_model_path: Some(selected.path),
                local_model_name: Some(selected.name),
            })
            .await?;
        return Ok(Json(result));
    }

    Err(AppError::OpenClaw(OpenClawError::BadRequest(
        "source invalido: use cloud ou local".to_string(),
    )))
}

async fn openclaw_chat(
    State(state): State<AppState>,
    Json(request): Json<OpenClawChatRequest>,
) -> Result<Json<OpenClawChatResponse>, AppError> {
    let response = state.openclaw_runtime.chat(request).await?;
    Ok(Json(response))
}

#[derive(Serialize)]
struct InstallResponse {
    message: String,
}

#[derive(Serialize)]
struct NanoBotStatusResponse {
    installed: bool,
    command: String,
    version: Option<String>,
    config_path: String,
    config_exists: bool,
    workspace_path: String,
    workspace_exists: bool,
    message: String,
    raw_status: Option<String>,
}

#[derive(Debug, Serialize)]
struct NanoBotRuntimeStateResponse {
    service_status: String,
    service_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pid: Option<u64>,
    rpc_ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    port_status: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    issues: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    uptime_seconds: Option<u64>,
    log_path: String,
}

#[derive(Debug, Deserialize)]
struct NanoBotRuntimeActionRequest {
    action: String,
}

#[derive(Debug, Serialize)]
struct NanoBotRuntimeActionResponse {
    action: String,
    runtime: NanoBotRuntimeStateResponse,
}

#[derive(Debug, Deserialize)]
struct NanoBotChatRequest {
    message: String,
    session_key: Option<String>,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
struct NanoBotUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    input: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_read: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_write: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total: Option<u64>,
}

#[derive(Debug, Serialize)]
struct NanoBotChatResponse {
    run_id: Option<String>,
    status: Option<String>,
    summary: Option<String>,
    reply: String,
    payloads: Vec<String>,
    duration_ms: Option<u64>,
    provider: Option<String>,
    model: Option<String>,
    usage: Option<NanoBotUsage>,
    skills: Vec<String>,
    tools: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct NanoBotLogQuery {
    stream: Option<String>,
    cursor: Option<u64>,
    max_bytes: Option<usize>,
}

#[derive(Debug, Serialize)]
struct NanoBotLogChunkResponse {
    stream: String,
    path: String,
    exists: bool,
    cursor: u64,
    next_cursor: u64,
    file_size: u64,
    truncated: bool,
    content: String,
}

#[derive(Debug, Serialize)]
struct NanoBotObservabilityResponse {
    session_key: String,
    provider: Option<String>,
    model: Option<String>,
    usage: Option<NanoBotUsage>,
    skills: Vec<String>,
    tools: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    updated_at: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct NanoBotSetModelRequest {
    source: Option<String>,
    model_reference: Option<String>,
    local_model_id: Option<String>,
    model: Option<String>,
}

#[derive(Debug)]
struct NanoBotProcessHandle {
    child: TokioChild,
    pid: u32,
    started_at: Instant,
}

#[derive(Debug)]
struct NanoBotRuntimeManager {
    process: Option<NanoBotProcessHandle>,
    last_error: Option<String>,
}

impl NanoBotRuntimeManager {
    fn new() -> Self {
        Self {
            process: None,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone)]
struct NanoBotCommandSpec {
    program: String,
    args: Vec<String>,
}

impl NanoBotCommandSpec {
    fn display(&self) -> String {
        if self.args.is_empty() {
            return self.program.clone();
        }
        format!("{} {}", self.program, self.args.join(" "))
    }
}

async fn openclaw_install(
    State(state): State<AppState>,
) -> Result<Json<InstallResponse>, AppError> {
    let message = state.openclaw_runtime.install().await?;
    Ok(Json(InstallResponse { message }))
}

async fn openclaw_environment(
    Query(query): Query<OpenClawEnvironmentQuery>,
) -> Result<Json<OpenClawEnvironmentResponse>, AppError> {
    let reveal = query.reveal.unwrap_or(false);
    let response = build_openclaw_environment_response(reveal)?;
    Ok(Json(response))
}

async fn openclaw_update_environment(
    Json(request): Json<OpenClawEnvironmentUpdateRequest>,
) -> Result<Json<OpenClawEnvironmentResponse>, AppError> {
    update_openclaw_environment_file(request.values)?;
    sync_nanobot_model_provider_from_environment()?;
    let response = build_openclaw_environment_response(false)?;
    Ok(Json(response))
}

async fn nanobot_status() -> Result<Json<NanoBotStatusResponse>, AppError> {
    let cfg = AppConfig::load_settings().apply_env();
    let spec = resolve_nanobot_command(&cfg);
    let command_cwd = resolve_nanobot_command_cwd(&cfg);
    let config_path = nanobot_config_path();
    let workspace_path =
        resolve_nanobot_workspace_from_config().unwrap_or_else(nanobot_workspace_path);

    let version = run_nanobot_command(&spec, &["--version"], command_cwd.as_deref())
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            let text = decode_stdout(&output);
            text.lines().next().map(str::trim).unwrap_or("").to_string()
        })
        .filter(|value| !value.is_empty());

    let status_output = run_nanobot_command(&spec, &["status"], command_cwd.as_deref())
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            let stdout = decode_stdout(&output);
            if stdout.is_empty() {
                decode_command_output(&output)
            } else {
                stdout
            }
        })
        .filter(|value| !value.is_empty());

    let installed = version.is_some();
    let message = if installed {
        "NanoBot detectado. Se o config ainda nao existe, inicialize com o botao onboard."
            .to_string()
    } else {
        "NanoBot nao encontrado no ambiente atual. Verifique o caminho/comando e rode a instalacao."
            .to_string()
    };

    Ok(Json(NanoBotStatusResponse {
        installed,
        command: spec.display(),
        version,
        config_path: config_path.display().to_string(),
        config_exists: config_path.exists(),
        workspace_path: workspace_path.display().to_string(),
        workspace_exists: workspace_path.exists(),
        message,
        raw_status: status_output,
    }))
}

async fn nanobot_onboard() -> Result<Json<InstallResponse>, AppError> {
    let cfg = AppConfig::load_settings().apply_env();
    let spec = resolve_nanobot_command(&cfg);
    let command_cwd = resolve_nanobot_command_cwd(&cfg);
    let config_path = nanobot_config_path();

    if config_path.exists() {
        return Ok(Json(InstallResponse {
            message: format!(
                "Configuracao ja existe em {}. Para evitar prompt interativo, o onboard automatico foi ignorado.",
                config_path.display()
            ),
        }));
    }

    let output =
        run_nanobot_command(&spec, &["onboard"], command_cwd.as_deref()).map_err(|error| {
            AppError::Provider(ProviderError::Io {
                context: "Falha ao executar nanobot onboard".to_string(),
                source: error,
            })
        })?;

    if output.status.success() {
        return Ok(Json(InstallResponse {
            message: format!(
                "NanoBot inicializado com sucesso em {}.",
                config_path.display()
            ),
        }));
    }

    Err(AppError::Provider(ProviderError::CommandFailed {
        command: format!("{} onboard", spec.display()),
        stderr: decode_command_output(&output),
    }))
}

async fn nanobot_install(
    State(_state): State<AppState>,
) -> Result<Json<InstallResponse>, AppError> {
    let cfg = AppConfig::load_settings().apply_env();
    let repo_dir = resolve_nanobot_repo_dir(&cfg)?;
    let repo_parent = repo_dir
        .parent()
        .ok_or_else(|| {
            AppError::Provider(ProviderError::Unavailable {
                details: "Nao foi possivel determinar diretorio pai do NanoBot.".to_string(),
            })
        })?
        .to_path_buf();

    std::fs::create_dir_all(&repo_parent).map_err(|error| {
        AppError::Provider(ProviderError::Io {
            context: "Falha ao criar diretorio pai do NanoBot".to_string(),
            source: error,
        })
    })?;

    if repo_dir.join(".git").exists() {
        let output = Command::new("git")
            .arg("-C")
            .arg(&repo_dir)
            .arg("pull")
            .arg("--ff-only")
            .output()
            .map_err(|error| {
                AppError::Provider(ProviderError::Io {
                    context: "Falha ao atualizar repositorio NanoBot".to_string(),
                    source: error,
                })
            })?;

        if !output.status.success() {
            return Err(AppError::Provider(ProviderError::CommandFailed {
                command: format!("git -C {} pull --ff-only", repo_dir.display()),
                stderr: decode_command_output(&output),
            }));
        }
    } else {
        if repo_dir.exists() {
            let has_files = std::fs::read_dir(&repo_dir)
                .map_err(|error| {
                    AppError::Provider(ProviderError::Io {
                        context: "Falha ao ler diretorio de destino do NanoBot".to_string(),
                        source: error,
                    })
                })?
                .next()
                .is_some();
            if has_files {
                return Err(AppError::Provider(ProviderError::Unavailable {
                    details: format!(
                        "O diretorio {} ja existe e nao esta vazio. Escolha outro caminho NanoBot CLI ou limpe o diretorio.",
                        repo_dir.display()
                    ),
                }));
            }
        }

        let output = Command::new("git")
            .arg("clone")
            .arg("https://github.com/HKUDS/nanobot.git")
            .arg(&repo_dir)
            .output()
            .map_err(|error| {
                AppError::Provider(ProviderError::Io {
                    context: "Falha ao clonar repositorio NanoBot".to_string(),
                    source: error,
                })
            })?;

        if !output.status.success() {
            return Err(AppError::Provider(ProviderError::CommandFailed {
                command: format!(
                    "git clone https://github.com/HKUDS/nanobot.git {}",
                    repo_dir.display()
                ),
                stderr: decode_command_output(&output),
            }));
        }
    }

    let venv_dir = repo_dir.join(".venv");
    if !venv_dir.join("bin").join("python").exists() {
        let venv_output = Command::new("python3")
            .arg("-m")
            .arg("venv")
            .arg(&venv_dir)
            .output()
            .map_err(|error| {
                AppError::Provider(ProviderError::Io {
                    context: "Falha ao criar venv local do NanoBot".to_string(),
                    source: error,
                })
            })?;

        if !venv_output.status.success() {
            return Err(AppError::Provider(ProviderError::CommandFailed {
                command: format!("python3 -m venv {}", venv_dir.display()),
                stderr: decode_command_output(&venv_output),
            }));
        }
    }

    let venv_python = venv_dir.join("bin").join("python");
    let pip_upgrade = Command::new(&venv_python)
        .arg("-m")
        .arg("pip")
        .arg("install")
        .arg("--upgrade")
        .arg("pip")
        .output()
        .map_err(|error| {
            AppError::Provider(ProviderError::Io {
                context: "Falha ao atualizar pip no venv do NanoBot".to_string(),
                source: error,
            })
        })?;

    if !pip_upgrade.status.success() {
        return Err(AppError::Provider(ProviderError::CommandFailed {
            command: format!("{} -m pip install --upgrade pip", venv_python.display()),
            stderr: decode_command_output(&pip_upgrade),
        }));
    }

    let install_output = Command::new(&venv_python)
        .arg("-m")
        .arg("pip")
        .arg("install")
        .arg("-e")
        .arg(&repo_dir)
        .output()
        .map_err(|error| {
            AppError::Provider(ProviderError::Io {
                context: "Falha ao executar instalacao pip do NanoBot".to_string(),
                source: error,
            })
        })?;

    if !install_output.status.success() {
        return Err(AppError::Provider(ProviderError::CommandFailed {
            command: format!(
                "{} -m pip install -e {}",
                venv_python.display(),
                repo_dir.display()
            ),
            stderr: decode_command_output(&install_output),
        }));
    }

    Ok(Json(InstallResponse {
        message: format!(
            "NanoBot pronto. Repo: {}. CLI ativa: {}. Proximo passo: execute o onboard para criar ~/.nanobot/config.json.",
            repo_dir.display(),
            repo_dir.join(".venv/bin/nanobot").display()
        ),
    }))
}

async fn nanobot_runtime_status(
    State(state): State<AppState>,
) -> Result<Json<NanoBotRuntimeStateResponse>, AppError> {
    let cfg = AppConfig::load_settings().apply_env();
    let spec = resolve_nanobot_command(&cfg);
    let command_cwd = resolve_nanobot_command_cwd(&cfg);

    let mut runtime = state.nanobot_runtime.lock().await;
    refresh_nanobot_process_state(&mut runtime);
    let response = build_nanobot_runtime_snapshot(&spec, command_cwd.as_deref(), &runtime);
    Ok(Json(response))
}

async fn nanobot_runtime_action(
    State(state): State<AppState>,
    Json(request): Json<NanoBotRuntimeActionRequest>,
) -> Result<Json<NanoBotRuntimeActionResponse>, AppError> {
    let action = request.action.trim().to_lowercase();
    if !matches!(action.as_str(), "start" | "stop" | "restart") {
        return Err(AppError::Provider(ProviderError::InvalidRequest {
            details: "acao invalida para runtime: use start, stop ou restart".to_string(),
        }));
    }

    let cfg = AppConfig::load_settings().apply_env();
    let spec = resolve_nanobot_command(&cfg);
    let command_cwd = resolve_nanobot_command_cwd(&cfg);

    let mut runtime = state.nanobot_runtime.lock().await;
    refresh_nanobot_process_state(&mut runtime);

    match action.as_str() {
        "start" => {
            if runtime.process.is_none() {
                spawn_nanobot_gateway(&spec, command_cwd.as_deref(), &mut runtime).await?;
            }
        }
        "stop" => {
            stop_nanobot_gateway(&mut runtime).await?;
        }
        "restart" => {
            stop_nanobot_gateway(&mut runtime).await?;
            spawn_nanobot_gateway(&spec, command_cwd.as_deref(), &mut runtime).await?;
        }
        _ => {}
    }

    refresh_nanobot_process_state(&mut runtime);
    let snapshot = build_nanobot_runtime_snapshot(&spec, command_cwd.as_deref(), &runtime);
    Ok(Json(NanoBotRuntimeActionResponse {
        action,
        runtime: snapshot,
    }))
}

async fn nanobot_chat(
    State(state): State<AppState>,
    Json(request): Json<NanoBotChatRequest>,
) -> Result<Json<NanoBotChatResponse>, AppError> {
    let message = request.message.trim();
    if message.is_empty() {
        return Err(AppError::Provider(ProviderError::InvalidRequest {
            details: "message nao pode ser vazio".to_string(),
        }));
    }

    let cfg = AppConfig::load_settings().apply_env();
    let spec = resolve_nanobot_command(&cfg);
    let command_cwd = resolve_nanobot_command_cwd(&cfg);
    let session_key = request
        .session_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("mlx-pilot")
        .to_string();

    let mut nanobot_config = read_nanobot_config_json_optional()?;
    let current_model = build_nanobot_model_response(nanobot_config.as_ref());

    if current_model.source == "cloud" {
        if let Some(config) = nanobot_config.as_mut() {
            let changed = sync_nanobot_cloud_provider_from_environment(
                config,
                current_model.reference.as_str(),
            )?;
            if changed {
                write_nanobot_config_json(config)?;
            }
        }
    }

    if current_model.source == "local" {
        let local_model_path = nanobot_config
            .as_ref()
            .and_then(extract_nanobot_model_local_path_value)
            .or_else(|| {
                current_model
                    .reference
                    .strip_prefix("openai/")
                    .map(ToString::to_string)
            })
            .or_else(|| {
                current_model
                    .model
                    .strip_prefix("openai/")
                    .map(ToString::to_string)
            })
            .or_else(|| {
                let candidate = current_model.model.trim();
                if candidate.is_empty() {
                    None
                } else {
                    Some(candidate.to_string())
                }
            })
            .ok_or_else(|| {
                AppError::Provider(ProviderError::InvalidRequest {
                    details: "modelo local NanoBot nao configurado corretamente".to_string(),
                })
            })?;

        let started = Instant::now();
        let chat_result = state
            .openclaw_local_provider
            .chat(ChatRequest {
                model_id: local_model_path,
                messages: vec![
                    ChatMessage::text(MessageRole::System, "Responda somente com a resposta final para o usuario, sem tags <think>, sem logs e sem metricas."),
                    ChatMessage::text(MessageRole::User, message),
                ],
                options: GenerationOptions {
                    temperature: Some(0.15),
                    max_tokens: Some(1024),
                    top_p: None,
                    airllm_enabled: None,
                },
            })
            .await
            .map_err(AppError::Provider)?;

        let observability = build_nanobot_observability_snapshot()?;
        let raw_reply = chat_result.message.content.trim().to_string();
        let reply = {
            let cleaned = sanitize_nanobot_cli_text(&raw_reply);
            if cleaned.is_empty() {
                raw_reply
            } else {
                cleaned
            }
        };
        let mut payloads = Vec::new();
        if let Some(raw) = chat_result.raw_output {
            if !raw.trim().is_empty() {
                payloads.push(raw);
            }
        }

        return Ok(Json(NanoBotChatResponse {
            run_id: None,
            status: Some("completed".to_string()),
            summary: Some(format!("session {} • local shared model", session_key)),
            reply: if reply.is_empty() {
                "(sem resposta textual)".to_string()
            } else {
                reply
            },
            payloads,
            duration_ms: Some(started.elapsed().as_millis() as u64),
            provider: Some(chat_result.provider),
            model: Some(current_model.reference),
            usage: Some(NanoBotUsage {
                input: Some(chat_result.usage.prompt_tokens as u64),
                output: Some(chat_result.usage.completion_tokens as u64),
                cache_read: None,
                cache_write: None,
                total: Some(chat_result.usage.total_tokens as u64),
            }),
            skills: observability.skills,
            tools: observability.tools,
        }));
    }

    let mut args = vec![
        "agent".to_string(),
        "--message".to_string(),
        message.to_string(),
        "--session".to_string(),
        session_key.clone(),
        "--no-markdown".to_string(),
    ];

    if request.timeout_ms.is_some() {
        // Mantemos compatibilidade de payload sem efeito direto no CLI atual.
        args.push("--no-logs".to_string());
    }

    let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    let started = Instant::now();

    let output =
        run_nanobot_command(&spec, &arg_refs, command_cwd.as_deref()).map_err(|error| {
            AppError::Provider(ProviderError::Io {
                context: "Falha ao executar nanobot agent".to_string(),
                source: error,
            })
        })?;

    if !output.status.success() {
        return Err(AppError::Provider(ProviderError::CommandFailed {
            command: format!("{} {}", spec.display(), arg_refs.join(" ")),
            stderr: decode_command_output(&output),
        }));
    }

    let stdout = decode_stdout(&output);
    let stderr = decode_stderr(&output);
    let reply = extract_nanobot_reply(&stdout, &stderr);
    let observability = build_nanobot_observability_snapshot()?;
    let mut payloads = Vec::new();
    if !stdout.is_empty() {
        payloads.push(stdout);
    }
    if !stderr.is_empty() {
        payloads.push(stderr);
    }

    Ok(Json(NanoBotChatResponse {
        run_id: None,
        status: Some("completed".to_string()),
        summary: Some(format!("session {}", session_key)),
        reply: if reply.is_empty() {
            "(sem resposta textual)".to_string()
        } else {
            reply
        },
        payloads,
        duration_ms: Some(started.elapsed().as_millis() as u64),
        provider: observability.provider,
        model: observability.model,
        usage: None,
        skills: observability.skills,
        tools: observability.tools,
    }))
}

async fn nanobot_logs(
    Query(query): Query<NanoBotLogQuery>,
) -> Result<Json<NanoBotLogChunkResponse>, AppError> {
    let stream = normalize_nanobot_log_stream(query.stream.as_deref());
    let path = nanobot_log_path_for_stream(&stream);
    let cursor = query.cursor.unwrap_or(0);
    let max_bytes = query.max_bytes.unwrap_or(65536).clamp(1024, 262_144);
    let chunk = read_nanobot_log_chunk(&stream, &path, cursor, max_bytes)?;
    Ok(Json(chunk))
}

async fn nanobot_observability() -> Result<Json<NanoBotObservabilityResponse>, AppError> {
    let snapshot = build_nanobot_observability_snapshot()?;
    Ok(Json(snapshot))
}

async fn nanobot_models(
    State(state): State<AppState>,
) -> Result<Json<OpenClawModelsResponse>, AppError> {
    let default_cloud_models = shared_agent_default_cloud_models();
    let mut cloud_models = default_cloud_models.clone();

    if let Ok(model_state) = state.openclaw_runtime.models_state().await {
        let mut merged = model_state.cloud_models;
        for entry in default_cloud_models {
            if !merged
                .iter()
                .any(|candidate| candidate.reference == entry.reference)
            {
                merged.push(entry);
            }
        }
        cloud_models = merged;
    }

    let local_models = state.openclaw_local_provider.list_models().await?;
    let mapped_local = local_models
        .into_iter()
        .map(|entry| OpenClawLocalModel {
            id: entry.id,
            name: entry.name,
            path: entry.path,
        })
        .collect::<Vec<_>>();

    let config = read_nanobot_config_json_optional()?;
    let current = build_nanobot_model_response(config.as_ref());

    if current.source == "cloud"
        && !current.reference.trim().is_empty()
        && !cloud_models
            .iter()
            .any(|entry| entry.reference == current.reference)
    {
        cloud_models.insert(
            0,
            OpenClawCloudModel {
                reference: current.reference.clone(),
                provider: current.provider.clone(),
                model: current.model.clone(),
                label: current.label.clone(),
                alias: None,
            },
        );
    }

    Ok(Json(OpenClawModelsResponse {
        session_key: "nanobot:main".to_string(),
        current,
        cloud_models,
        local_models: mapped_local,
    }))
}

async fn nanobot_get_model() -> Result<Json<OpenClawCurrentModel>, AppError> {
    let config = read_nanobot_config_json_optional()?;
    Ok(Json(build_nanobot_model_response(config.as_ref())))
}

async fn nanobot_set_model(
    State(state): State<AppState>,
    Json(request): Json<NanoBotSetModelRequest>,
) -> Result<Json<OpenClawCurrentModel>, AppError> {
    let mut config = read_nanobot_config_json_optional()?.unwrap_or_else(default_nanobot_config);

    let source = request
        .source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase)
        .unwrap_or_else(|| {
            if request.local_model_id.is_some() {
                "local".to_string()
            } else {
                "cloud".to_string()
            }
        });

    if !matches!(source.as_str(), "cloud" | "local") {
        return Err(AppError::Provider(ProviderError::InvalidRequest {
            details: "source invalido: use cloud ou local".to_string(),
        }));
    }

    if source == "local" {
        let model_id = request
            .local_model_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                AppError::Provider(ProviderError::InvalidRequest {
                    details: "local_model_id e obrigatorio para source=local".to_string(),
                })
            })?;

        let local_models = state.openclaw_local_provider.list_models().await?;
        let selected = local_models
            .into_iter()
            .find(|entry| entry.id == model_id)
            .ok_or_else(|| {
                AppError::NotFound(format!("modelo local '{model_id}' nao encontrado"))
            })?;

        let model_reference = format!("openai/{}", selected.path);
        set_json_string_at_path(
            &mut config,
            &["agents", "defaults", "model"],
            model_reference.clone(),
        );
        set_json_string_at_path(
            &mut config,
            &["agents", "defaults", "model_source"],
            "local".to_string(),
        );
        set_json_string_at_path(
            &mut config,
            &["agents", "defaults", "model_reference"],
            model_reference,
        );
        set_json_string_at_path(
            &mut config,
            &["agents", "defaults", "model_local_id"],
            selected.id,
        );
        set_json_string_at_path(
            &mut config,
            &["agents", "defaults", "model_local_name"],
            selected.name,
        );
        set_json_string_at_path(
            &mut config,
            &["agents", "defaults", "model_local_path"],
            selected.path,
        );
    } else {
        let model_reference = request
            .model_reference
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                request
                    .model
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
            })
            .ok_or_else(|| {
                AppError::Provider(ProviderError::InvalidRequest {
                    details: "model_reference e obrigatorio para source=cloud".to_string(),
                })
            })?;

        set_json_string_at_path(
            &mut config,
            &["agents", "defaults", "model"],
            model_reference.clone(),
        );
        set_json_string_at_path(
            &mut config,
            &["agents", "defaults", "model_source"],
            "cloud".to_string(),
        );
        set_json_string_at_path(
            &mut config,
            &["agents", "defaults", "model_reference"],
            model_reference.clone(),
        );
        let _ = sync_nanobot_cloud_provider_from_environment(&mut config, &model_reference)?;
        remove_json_key_at_path(&mut config, &["agents", "defaults", "model_local_id"]);
        remove_json_key_at_path(&mut config, &["agents", "defaults", "model_local_name"]);
        remove_json_key_at_path(&mut config, &["agents", "defaults", "model_local_path"]);
    }

    if extract_nanobot_workspace_value(&config).is_none() {
        set_json_string_at_path(
            &mut config,
            &["agents", "defaults", "workspace"],
            nanobot_workspace_path().display().to_string(),
        );
    }

    write_nanobot_config_json(&config)?;
    Ok(Json(build_nanobot_model_response(Some(&config))))
}

fn resolve_nanobot_repo_dir(cfg: &AppConfig) -> Result<FsPathBuf, AppError> {
    let raw = cfg.nanobot_cli_path.to_string_lossy().trim().to_string();
    if raw.is_empty() {
        return Err(AppError::Provider(ProviderError::Unavailable {
            details: "Caminho do NanoBot nao definido.".to_string(),
        }));
    }

    let candidate = FsPathBuf::from(&raw);
    if candidate
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("py"))
    {
        return candidate
            .parent()
            .map(|parent| parent.to_path_buf())
            .ok_or_else(|| {
                AppError::Provider(ProviderError::Unavailable {
                    details: "Caminho NanoBot .py invalido (sem diretorio pai).".to_string(),
                })
            });
    }

    if candidate.exists() {
        if candidate.is_dir() {
            return Ok(candidate);
        }

        return candidate
            .parent()
            .map(|parent| parent.to_path_buf())
            .ok_or_else(|| {
                AppError::Provider(ProviderError::Unavailable {
                    details: "Caminho NanoBot invalido (sem diretorio pai).".to_string(),
                })
            });
    }

    if raw.contains('/') || raw.contains('\\') {
        if candidate.extension().is_none() {
            return Ok(candidate);
        }

        return candidate
            .parent()
            .map(|parent| parent.to_path_buf())
            .ok_or_else(|| {
                AppError::Provider(ProviderError::Unavailable {
                    details: "Caminho NanoBot invalido (sem diretorio pai).".to_string(),
                })
            });
    }

    Err(AppError::Provider(ProviderError::Unavailable {
        details: "Para instalar via clone, informe um caminho de pasta no campo NanoBot CLI (ex.: /Users/kaike/prod/nanobot).".to_string(),
    }))
}

fn resolve_nanobot_command(cfg: &AppConfig) -> NanoBotCommandSpec {
    let raw = cfg.nanobot_cli_path.to_string_lossy().trim().to_string();
    if raw.is_empty() {
        return NanoBotCommandSpec {
            program: "nanobot".to_string(),
            args: Vec::new(),
        };
    }

    let candidate = FsPathBuf::from(&raw);
    if candidate.is_dir() {
        let local_venv = candidate.join(".venv").join("bin").join("nanobot");
        if local_venv.exists() {
            return NanoBotCommandSpec {
                program: local_venv.display().to_string(),
                args: Vec::new(),
            };
        }

        return NanoBotCommandSpec {
            program: "nanobot".to_string(),
            args: Vec::new(),
        };
    }

    if candidate
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("py"))
    {
        if let Some(parent) = candidate.parent() {
            let local_venv = parent.join(".venv").join("bin").join("nanobot");
            if local_venv.exists() {
                return NanoBotCommandSpec {
                    program: local_venv.display().to_string(),
                    args: Vec::new(),
                };
            }
        }

        if !candidate.exists() {
            return NanoBotCommandSpec {
                program: "nanobot".to_string(),
                args: Vec::new(),
            };
        }

        return NanoBotCommandSpec {
            program: "python3".to_string(),
            args: vec![candidate.display().to_string()],
        };
    }

    NanoBotCommandSpec {
        program: raw,
        args: Vec::new(),
    }
}

fn resolve_nanobot_command_cwd(cfg: &AppConfig) -> Option<FsPathBuf> {
    resolve_nanobot_repo_dir(cfg)
        .ok()
        .filter(|path| path.exists() && path.is_dir())
}

fn run_nanobot_command(
    spec: &NanoBotCommandSpec,
    extra_args: &[&str],
    cwd: Option<&FsPath>,
) -> io::Result<Output> {
    let mut command = Command::new(&spec.program);
    command.args(&spec.args);
    command.args(extra_args);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    command.output()
}

fn decode_command_output(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }

    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn decode_stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn decode_stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_string()
}

fn normalize_nanobot_log_stream(stream: Option<&str>) -> String {
    let normalized = stream.map(str::trim).unwrap_or("gateway").to_lowercase();

    match normalized.as_str() {
        "gateway" | "error" | "sync" => normalized,
        _ => "gateway".to_string(),
    }
}

fn nanobot_data_path() -> FsPathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    FsPathBuf::from(home).join(".nanobot")
}

fn nanobot_log_path_for_stream(stream: &str) -> FsPathBuf {
    let log_dir = nanobot_data_path().join("logs");
    match stream {
        "error" => log_dir.join("gateway.err.log"),
        "sync" => log_dir.join("agent.log"),
        _ => log_dir.join("gateway.log"),
    }
}

fn read_nanobot_log_chunk(
    stream: &str,
    path: &FsPathBuf,
    cursor: u64,
    max_bytes: usize,
) -> Result<NanoBotLogChunkResponse, AppError> {
    if !path.exists() {
        return Ok(NanoBotLogChunkResponse {
            stream: stream.to_string(),
            path: path.display().to_string(),
            exists: false,
            cursor,
            next_cursor: cursor,
            file_size: 0,
            truncated: false,
            content: String::new(),
        });
    }

    let metadata = fs::metadata(path).map_err(|source| {
        AppError::Provider(ProviderError::Io {
            context: format!("falha lendo metadata do log {}", path.display()),
            source,
        })
    })?;
    let file_size = metadata.len();

    let mut effective_cursor = cursor.min(file_size);
    let truncated = cursor > file_size;
    if truncated {
        effective_cursor = 0;
    }

    let mut file = fs::File::open(path).map_err(|source| {
        AppError::Provider(ProviderError::Io {
            context: format!("falha abrindo log {}", path.display()),
            source,
        })
    })?;
    file.seek(SeekFrom::Start(effective_cursor))
        .map_err(|source| {
            AppError::Provider(ProviderError::Io {
                context: format!("falha posicionando cursor no log {}", path.display()),
                source,
            })
        })?;

    let mut buffer = vec![0_u8; max_bytes];
    let bytes_read = file.read(&mut buffer).map_err(|source| {
        AppError::Provider(ProviderError::Io {
            context: format!("falha lendo log {}", path.display()),
            source,
        })
    })?;
    buffer.truncate(bytes_read);

    let content = String::from_utf8_lossy(&buffer).to_string();
    let next_cursor = effective_cursor + bytes_read as u64;

    Ok(NanoBotLogChunkResponse {
        stream: stream.to_string(),
        path: path.display().to_string(),
        exists: true,
        cursor: effective_cursor,
        next_cursor,
        file_size,
        truncated,
        content,
    })
}

fn refresh_nanobot_process_state(runtime: &mut NanoBotRuntimeManager) {
    let Some(process) = runtime.process.as_mut() else {
        return;
    };

    match process.child.try_wait() {
        Ok(Some(status)) => {
            runtime.last_error = Some(format!(
                "gateway finalizou com status {}",
                status
                    .code()
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "desconhecido".to_string())
            ));
            runtime.process = None;
        }
        Ok(None) => {}
        Err(error) => {
            runtime.last_error = Some(format!("falha ao inspecionar processo gateway: {error}"));
            runtime.process = None;
        }
    }
}

fn build_nanobot_runtime_snapshot(
    spec: &NanoBotCommandSpec,
    command_cwd: Option<&FsPath>,
    runtime: &NanoBotRuntimeManager,
) -> NanoBotRuntimeStateResponse {
    let (service_status, service_state, pid, uptime_seconds) =
        if let Some(process) = runtime.process.as_ref() {
            (
                "running".to_string(),
                "active".to_string(),
                Some(process.pid as u64),
                Some(process.started_at.elapsed().as_secs()),
            )
        } else {
            ("stopped".to_string(), "inactive".to_string(), None, None)
        };

    let mut issues = Vec::new();
    if let Some(last_error) = runtime.last_error.as_deref() {
        issues.push(last_error.to_string());
    }

    if !nanobot_config_path().exists() {
        issues.push("config.json ausente (execute onboard)".to_string());
    }

    let status_probe = run_nanobot_command(spec, &["status"], command_cwd);
    let rpc_ok = status_probe
        .as_ref()
        .map(|output| output.status.success())
        .unwrap_or(false);

    if !rpc_ok {
        match status_probe {
            Ok(output) => {
                let summary = decode_command_output(&output);
                if !summary.is_empty() {
                    issues.push(summary.lines().next().unwrap_or_default().to_string());
                }
            }
            Err(error) => {
                issues.push(format!("status indisponivel: {error}"));
            }
        }
    }

    dedup_vec(&mut issues);

    NanoBotRuntimeStateResponse {
        service_status,
        service_state,
        pid,
        rpc_ok,
        port_status: None,
        issues,
        uptime_seconds,
        log_path: nanobot_log_path_for_stream("gateway").display().to_string(),
    }
}

async fn stop_nanobot_gateway(runtime: &mut NanoBotRuntimeManager) -> Result<(), AppError> {
    let Some(mut process) = runtime.process.take() else {
        return Ok(());
    };

    process.child.kill().await.map_err(|source| {
        AppError::Provider(ProviderError::Io {
            context: "Falha ao finalizar processo gateway do NanoBot".to_string(),
            source,
        })
    })?;
    let _ = process.child.wait().await;
    Ok(())
}

async fn spawn_nanobot_gateway(
    spec: &NanoBotCommandSpec,
    command_cwd: Option<&FsPath>,
    runtime: &mut NanoBotRuntimeManager,
) -> Result<(), AppError> {
    let log_dir = nanobot_data_path().join("logs");
    fs::create_dir_all(&log_dir).map_err(|source| {
        AppError::Provider(ProviderError::Io {
            context: format!(
                "Falha ao criar diretorio de logs do NanoBot ({})",
                log_dir.display()
            ),
            source,
        })
    })?;

    let gateway_log = nanobot_log_path_for_stream("gateway");
    let gateway_err_log = nanobot_log_path_for_stream("error");

    let stdout_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&gateway_log)
        .map_err(|source| {
            AppError::Provider(ProviderError::Io {
                context: format!("Falha ao abrir log {}", gateway_log.display()),
                source,
            })
        })?;
    let stderr_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&gateway_err_log)
        .map_err(|source| {
            AppError::Provider(ProviderError::Io {
                context: format!("Falha ao abrir log {}", gateway_err_log.display()),
                source,
            })
        })?;

    let mut command = TokioCommand::new(&spec.program);
    command.args(&spec.args);
    command.arg("gateway");
    if let Some(cwd) = command_cwd {
        command.current_dir(cwd);
    }
    command.stdin(Stdio::null());
    command.stdout(Stdio::from(stdout_file));
    command.stderr(Stdio::from(stderr_file));

    let mut child = command.spawn().map_err(|source| {
        AppError::Provider(ProviderError::Io {
            context: format!(
                "Falha ao iniciar gateway do NanoBot via '{} gateway'",
                spec.display()
            ),
            source,
        })
    })?;

    tokio::time::sleep(std::time::Duration::from_millis(350)).await;
    if let Some(status) = child.try_wait().map_err(|source| {
        AppError::Provider(ProviderError::Io {
            context: "Falha ao checar processo inicial do NanoBot".to_string(),
            source,
        })
    })? {
        let tail =
            read_log_tail(&gateway_err_log, 4096).or_else(|| read_log_tail(&gateway_log, 4096));
        let details =
            tail.unwrap_or_else(|| "processo encerrou imediatamente sem detalhes".to_string());
        return Err(AppError::Provider(ProviderError::Unavailable {
            details: format!(
                "gateway do NanoBot encerrou logo apos start (status {status}): {details}"
            ),
        }));
    }

    let pid = child.id().unwrap_or(0);
    runtime.process = Some(NanoBotProcessHandle {
        child,
        pid,
        started_at: Instant::now(),
    });
    runtime.last_error = None;
    Ok(())
}

fn read_log_tail(path: &FsPath, max_bytes: usize) -> Option<String> {
    if !path.exists() {
        return None;
    }

    let mut file = fs::File::open(path).ok()?;
    let file_size = file.metadata().ok()?.len();
    let start = file_size.saturating_sub(max_bytes as u64);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return None;
    }

    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return None;
    }

    let text = String::from_utf8_lossy(&bytes).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn default_nanobot_config() -> Value {
    json!({
        "agents": {
            "defaults": {
                "workspace": nanobot_workspace_path().display().to_string(),
                "model": "anthropic/claude-opus-4-5"
            }
        }
    })
}

const OPENCLAW_ENV_CATALOG: &[(&str, &str)] = &[
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
    ("OPENCLAW_GATEWAY_TOKEN", "OpenClaw gateway token"),
];

fn build_openclaw_environment_response(
    reveal: bool,
) -> Result<OpenClawEnvironmentResponse, AppError> {
    let cfg = AppConfig::load_settings().apply_env();
    let env_path = resolve_openclaw_env_path(&cfg);
    let env_example_path = resolve_openclaw_env_example_path(&env_path);

    let env_values = read_env_file_assignments_optional(&env_path)?;
    let env_example_values = read_env_file_assignments_optional(&env_example_path)?;

    let mut labels = BTreeMap::new();
    let mut keys = BTreeSet::new();
    for (key, label) in OPENCLAW_ENV_CATALOG {
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

        variables.push(OpenClawEnvironmentVariable {
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

    Ok(OpenClawEnvironmentResponse {
        env_path: env_path.display().to_string(),
        env_exists: env_path.exists(),
        env_example_path: env_example_path.display().to_string(),
        env_example_exists: env_example_path.exists(),
        variables,
    })
}

fn update_openclaw_environment_file(updates: BTreeMap<String, String>) -> Result<(), AppError> {
    let mut normalized_updates = BTreeMap::new();
    for (raw_key, raw_value) in updates {
        let Some(key) = normalize_env_key(&raw_key) else {
            continue;
        };
        normalized_updates.insert(key, raw_value.trim().to_string());
    }

    if normalized_updates.is_empty() {
        return Err(AppError::Provider(ProviderError::InvalidRequest {
            details: "nenhuma variavel valida recebida para atualizar environment".to_string(),
        }));
    }

    let cfg = AppConfig::load_settings().apply_env();
    let env_path = resolve_openclaw_env_path(&cfg);

    let mut lines = if env_path.exists() {
        fs::read_to_string(&env_path)
            .map_err(|source| {
                AppError::Provider(ProviderError::Io {
                    context: format!("Falha ao ler environment OpenClaw ({})", env_path.display()),
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
                    "Falha ao criar diretorio do environment OpenClaw ({})",
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
                "Falha ao salvar environment OpenClaw ({})",
                env_path.display()
            ),
            source,
        })
    })?;

    Ok(())
}

fn sync_nanobot_cloud_provider_from_environment(
    config: &mut Value,
    model_reference: &str,
) -> Result<bool, AppError> {
    let Some(provider) = infer_model_provider_name(model_reference) else {
        return Ok(false);
    };

    let (provider_key, api_key_candidates, base_url_candidates) =
        nanobot_provider_env_binding(&provider);
    if api_key_candidates.is_empty() {
        return Ok(false);
    }

    let openclaw_env_values = read_openclaw_environment_values()?;
    let Some(api_key) = api_key_candidates
        .iter()
        .find_map(|key| resolve_environment_value(&openclaw_env_values, key))
    else {
        return Ok(false);
    };

    let mut changed = false;
    let api_key_pointer = format!("/providers/{provider_key}/apiKey");
    let current_api_key = config
        .pointer(&api_key_pointer)
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if current_api_key != api_key {
        set_json_string_at_path(
            config,
            &["providers", provider_key.as_str(), "apiKey"],
            api_key.clone(),
        );
        changed = true;
    }

    if let Some(base_url) = base_url_candidates
        .iter()
        .find_map(|key| resolve_environment_value(&openclaw_env_values, key))
    {
        let base_url_pointer = format!("/providers/{provider_key}/apiBase");
        let current_base_url = config
            .pointer(&base_url_pointer)
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        if current_base_url != base_url {
            set_json_string_at_path(
                config,
                &["providers", provider_key.as_str(), "apiBase"],
                base_url,
            );
            changed = true;
        }
    }

    Ok(changed)
}

fn sync_nanobot_model_provider_from_environment() -> Result<(), AppError> {
    let Some(mut config) = read_nanobot_config_json_optional()? else {
        return Ok(());
    };

    let current = build_nanobot_model_response(Some(&config));
    if current.source != "cloud" {
        return Ok(());
    }

    if sync_nanobot_cloud_provider_from_environment(&mut config, &current.reference)? {
        write_nanobot_config_json(&config)?;
    }

    Ok(())
}

fn infer_model_provider_name(model_reference: &str) -> Option<String> {
    let provider = model_reference
        .split('/')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_lowercase();

    let normalized = match provider.as_str() {
        "zai" | "zhipuai" => "zhipu".to_string(),
        "kimi" => "moonshot".to_string(),
        other => other.to_string(),
    };
    Some(normalized)
}

fn nanobot_provider_env_binding(provider: &str) -> (String, Vec<String>, Vec<String>) {
    match provider {
        "openrouter" => (
            "openrouter".to_string(),
            vec!["OPENROUTER_API_KEY".to_string()],
            vec!["OPENROUTER_BASE_URL".to_string()],
        ),
        "deepseek" => (
            "deepseek".to_string(),
            vec!["DEEPSEEK_API_KEY".to_string()],
            vec!["DEEPSEEK_BASE_URL".to_string()],
        ),
        "openai" | "aihubmix" | "siliconflow" | "volcengine" => (
            "openai".to_string(),
            vec!["OPENAI_API_KEY".to_string()],
            vec!["OPENAI_BASE_URL".to_string()],
        ),
        "anthropic" => (
            "anthropic".to_string(),
            vec!["ANTHROPIC_API_KEY".to_string()],
            Vec::new(),
        ),
        "gemini" => (
            "gemini".to_string(),
            vec!["GEMINI_API_KEY".to_string(), "GOOGLE_API_KEY".to_string()],
            vec!["GEMINI_BASE_URL".to_string()],
        ),
        "groq" => (
            "groq".to_string(),
            vec!["GROQ_API_KEY".to_string()],
            vec!["GROQ_BASE_URL".to_string()],
        ),
        "dashscope" => (
            "dashscope".to_string(),
            vec!["DASHSCOPE_API_KEY".to_string()],
            Vec::new(),
        ),
        "moonshot" => (
            "moonshot".to_string(),
            vec!["MOONSHOT_API_KEY".to_string(), "KIMI_API_KEY".to_string()],
            vec!["MOONSHOT_API_BASE".to_string()],
        ),
        "minimax" => (
            "minimax".to_string(),
            vec!["MINIMAX_API_KEY".to_string()],
            vec!["MINIMAX_BASE_URL".to_string()],
        ),
        "zhipu" => (
            "zhipu".to_string(),
            vec!["ZAI_API_KEY".to_string(), "ZHIPUAI_API_KEY".to_string()],
            Vec::new(),
        ),
        "vllm" | "hosted_vllm" => (
            "vllm".to_string(),
            vec![
                "HOSTED_VLLM_API_KEY".to_string(),
                "OPENAI_API_KEY".to_string(),
            ],
            vec!["OPENAI_BASE_URL".to_string()],
        ),
        other => (
            other.to_string(),
            vec![format!(
                "{}_API_KEY",
                other.to_ascii_uppercase().replace('-', "_")
            )],
            vec![format!(
                "{}_BASE_URL",
                other.to_ascii_uppercase().replace('-', "_")
            )],
        ),
    }
}

fn resolve_openclaw_env_path(cfg: &AppConfig) -> FsPathBuf {
    let mut candidates = Vec::new();
    if let Some(parent) = cfg.openclaw_state_dir.parent() {
        candidates.push(parent.join(".env"));
    }
    candidates.push(cfg.openclaw_state_dir.join(".env"));
    if let Some(parent) = cfg.openclaw_cli_path.parent() {
        candidates.push(parent.join("deploy").join(".env"));
        candidates.push(parent.join(".env"));
    }

    let mut seen = BTreeSet::new();
    for candidate in candidates {
        if seen.insert(candidate.clone()) && candidate.exists() {
            return candidate;
        }
    }

    if let Some(first) = seen.into_iter().next() {
        first
    } else {
        FsPathBuf::from(".env")
    }
}

fn resolve_openclaw_env_example_path(env_path: &FsPath) -> FsPathBuf {
    env_path
        .parent()
        .map(|parent| parent.join(".env.example"))
        .unwrap_or_else(|| FsPathBuf::from(".env.example"))
}

fn read_openclaw_environment_values() -> Result<BTreeMap<String, String>, AppError> {
    let cfg = AppConfig::load_settings().apply_env();
    let env_path = resolve_openclaw_env_path(&cfg);
    read_env_file_assignments_optional(&env_path)
}

fn read_env_file_assignments_optional(path: &FsPath) -> Result<BTreeMap<String, String>, AppError> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    read_env_file_assignments(path)
}

fn read_env_file_assignments(path: &FsPath) -> Result<BTreeMap<String, String>, AppError> {
    let content = fs::read_to_string(path).map_err(|source| {
        AppError::Provider(ProviderError::Io {
            context: format!("Falha ao ler arquivo de environment ({})", path.display()),
            source,
        })
    })?;
    let mut values = BTreeMap::new();
    for line in content.lines() {
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

    let without_export = trimmed.strip_prefix("export ").unwrap_or(trimmed);
    let (raw_key, raw_value) = without_export.split_once('=')?;
    let key = normalize_env_key(raw_key)?;
    let value = decode_env_value(raw_value.trim());
    Some((key, value))
}

fn normalize_env_key(raw: &str) -> Option<String> {
    let key = raw.trim().to_uppercase();
    if key.is_empty() || !is_valid_env_key(&key) {
        return None;
    }
    Some(key)
}

fn is_valid_env_key(key: &str) -> bool {
    key.chars()
        .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

fn decode_env_value(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        let inner = &trimmed[1..trimmed.len() - 1];
        if trimmed.starts_with('"') {
            return inner.replace("\\\"", "\"").replace("\\\\", "\\");
        }
        return inner.to_string();
    }

    trimmed.to_string()
}

fn encode_env_value(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }

    let requires_quotes = value
        .chars()
        .any(|ch| ch.is_whitespace() || matches!(ch, '#' | '"' | '\''));
    if !requires_quotes {
        return value.to_string();
    }

    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn should_expose_environment_key(key: &str) -> bool {
    let normalized = key.trim().to_uppercase();
    if normalized.is_empty() {
        return false;
    }
    normalized.ends_with("_API_KEY")
        || normalized.ends_with("_TOKEN")
        || normalized.ends_with("_SECRET")
        || normalized.ends_with("_BASE_URL")
        || normalized == "TELEGRAM_CHAT_ID"
}

fn is_secret_environment_key(key: &str) -> bool {
    let normalized = key.trim().to_uppercase();
    normalized.contains("KEY")
        || normalized.contains("TOKEN")
        || normalized.contains("SECRET")
        || normalized.contains("PASSWORD")
}

fn mask_environment_value(value: &str) -> String {
    if value.trim().is_empty() {
        return "-".to_string();
    }

    let visible_prefix = value.chars().take(4).collect::<String>();
    let visible_suffix = value
        .chars()
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    let hidden_count = value
        .chars()
        .count()
        .saturating_sub(visible_prefix.len() + visible_suffix.len());
    if hidden_count == 0 {
        return value.to_string();
    }
    format!(
        "{visible_prefix}{}{}",
        "*".repeat(hidden_count),
        visible_suffix
    )
}

fn resolve_environment_value(values: &BTreeMap<String, String>, key: &str) -> Option<String> {
    let normalized = normalize_env_key(key)?;
    if let Some(value) = values
        .get(&normalized)
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(value.to_string());
    }

    std::env::var(&normalized)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn read_nanobot_config_json_optional() -> Result<Option<Value>, AppError> {
    let path = nanobot_config_path();
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&path).map_err(|source| {
        AppError::Provider(ProviderError::Io {
            context: format!("Falha ao ler config NanoBot ({})", path.display()),
            source,
        })
    })?;

    serde_json::from_str::<Value>(&raw)
        .map(Some)
        .map_err(|source| {
            AppError::Provider(ProviderError::Io {
                context: format!("Falha ao parsear config NanoBot ({})", path.display()),
                source: io::Error::other(source.to_string()),
            })
        })
}

fn write_nanobot_config_json(config: &Value) -> Result<(), AppError> {
    let path = nanobot_config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| {
            AppError::Provider(ProviderError::Io {
                context: format!(
                    "Falha ao criar diretorio de config NanoBot ({})",
                    parent.display()
                ),
                source,
            })
        })?;
    }

    let body = serde_json::to_string_pretty(config).map_err(|source| {
        AppError::Provider(ProviderError::Io {
            context: "Falha ao serializar config NanoBot".to_string(),
            source: io::Error::other(source.to_string()),
        })
    })?;

    fs::write(&path, body).map_err(|source| {
        AppError::Provider(ProviderError::Io {
            context: format!("Falha ao salvar config NanoBot ({})", path.display()),
            source,
        })
    })?;

    Ok(())
}

fn set_json_string_at_path(root: &mut Value, path: &[&str], value: String) {
    if path.is_empty() {
        return;
    }

    let mut cursor = root;
    for (index, key) in path.iter().enumerate() {
        let is_last = index + 1 == path.len();
        if !cursor.is_object() {
            *cursor = Value::Object(serde_json::Map::new());
        }

        let object = cursor.as_object_mut().expect("cursor must be object");
        if is_last {
            object.insert((*key).to_string(), Value::String(value.clone()));
            return;
        }

        cursor = object
            .entry((*key).to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
    }
}

fn remove_json_key_at_path(root: &mut Value, path: &[&str]) {
    if path.is_empty() {
        return;
    }

    let mut cursor = root;
    for key in &path[..path.len().saturating_sub(1)] {
        let Some(next) = cursor.get_mut(*key) else {
            return;
        };
        cursor = next;
    }

    if let Some(object) = cursor.as_object_mut() {
        object.remove(path[path.len() - 1]);
    }
}

fn extract_nanobot_model_value(config: &Value) -> Option<String> {
    config
        .pointer("/agents/defaults/model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn extract_nanobot_model_source_value(config: &Value) -> Option<String> {
    config
        .pointer("/agents/defaults/model_source")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase)
}

fn extract_nanobot_model_reference_value(config: &Value) -> Option<String> {
    config
        .pointer("/agents/defaults/model_reference")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn extract_nanobot_model_local_name_value(config: &Value) -> Option<String> {
    config
        .pointer("/agents/defaults/model_local_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn extract_nanobot_model_local_path_value(config: &Value) -> Option<String> {
    config
        .pointer("/agents/defaults/model_local_path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn extract_nanobot_workspace_value(config: &Value) -> Option<FsPathBuf> {
    config
        .pointer("/agents/defaults/workspace")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            if let Some(rest) = value.strip_prefix("~/") {
                return nanobot_home_dir().join(rest);
            }
            FsPathBuf::from(value)
        })
}

fn resolve_nanobot_workspace_from_config() -> Option<FsPathBuf> {
    read_nanobot_config_json_optional()
        .ok()
        .flatten()
        .and_then(|config| extract_nanobot_workspace_value(&config))
}

fn build_nanobot_model_response(config: Option<&Value>) -> OpenClawCurrentModel {
    let model = config
        .and_then(extract_nanobot_model_value)
        .unwrap_or_default();
    let source = config
        .and_then(extract_nanobot_model_source_value)
        .filter(|value| matches!(value.as_str(), "cloud" | "local"))
        .unwrap_or_else(|| "cloud".to_string());
    let reference = config
        .and_then(extract_nanobot_model_reference_value)
        .unwrap_or_else(|| model.clone());
    let provider = reference
        .split('/')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("-")
        .to_string();
    let label = if source == "local" {
        config
            .and_then(extract_nanobot_model_local_name_value)
            .map(|name| format!("{name} (local)"))
            .unwrap_or_else(|| {
                if reference.is_empty() {
                    "-".to_string()
                } else {
                    format!("{reference} (local)")
                }
            })
    } else if reference.is_empty() && model.is_empty() {
        "-".to_string()
    } else if reference.is_empty() {
        model.clone()
    } else {
        reference.clone()
    };

    OpenClawCurrentModel {
        source,
        reference,
        model,
        provider,
        label,
    }
}

fn shared_agent_default_cloud_models() -> Vec<OpenClawCloudModel> {
    vec![
        OpenClawCloudModel {
            reference: "deepseek/deepseek-chat".to_string(),
            provider: "deepseek".to_string(),
            model: "deepseek-chat".to_string(),
            label: "DeepSeek Chat".to_string(),
            alias: None,
        },
        OpenClawCloudModel {
            reference: "deepseek/deepseek-reasoner".to_string(),
            provider: "deepseek".to_string(),
            model: "deepseek-reasoner".to_string(),
            label: "DeepSeek Reasoner".to_string(),
            alias: None,
        },
    ]
}

fn build_nanobot_observability_snapshot() -> Result<NanoBotObservabilityResponse, AppError> {
    let config = read_nanobot_config_json_optional()?;
    let model = config.as_ref().and_then(extract_nanobot_model_value);
    let provider = model.as_deref().and_then(|value| {
        value
            .split('/')
            .next()
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .map(ToString::to_string)
    });
    let workspace = config
        .as_ref()
        .and_then(extract_nanobot_workspace_value)
        .unwrap_or_else(nanobot_workspace_path);
    let skills = list_nanobot_skills(&workspace);
    let tools = list_nanobot_tools(config.as_ref());

    Ok(NanoBotObservabilityResponse {
        session_key: "nanobot:main".to_string(),
        provider,
        model,
        usage: None,
        skills,
        tools,
        updated_at: now_unix_ms(),
    })
}

fn list_nanobot_skills(workspace: &FsPath) -> Vec<String> {
    let mut skills = Vec::new();
    let skills_dir = workspace.join("skills");
    let entries = match fs::read_dir(&skills_dir) {
        Ok(entries) => entries,
        Err(_) => return skills,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = if path.is_dir() {
            path.file_name()
                .and_then(|value| value.to_str())
                .map(str::to_string)
        } else {
            path.file_stem()
                .and_then(|value| value.to_str())
                .map(str::to_string)
        };

        if let Some(name) = name {
            let normalized = name.trim();
            if !normalized.is_empty() {
                skills.push(normalized.to_string());
            }
        }
    }

    skills.sort();
    skills.dedup();
    skills
}

fn list_nanobot_tools(config: Option<&Value>) -> Vec<String> {
    let mut tools = vec![
        "exec".to_string(),
        "files".to_string(),
        "memory".to_string(),
    ];

    if let Some(config) = config {
        if let Some(web) = config
            .pointer("/tools/web/search")
            .and_then(Value::as_object)
        {
            let has_api_key = web
                .get("apiKey")
                .or_else(|| web.get("api_key"))
                .and_then(Value::as_str)
                .map(str::trim)
                .is_some_and(|value| !value.is_empty());
            if has_api_key {
                tools.push("web.search".to_string());
            }
        }

        let mcp_servers = config
            .pointer("/tools/mcpServers")
            .and_then(Value::as_object)
            .or_else(|| {
                config
                    .pointer("/tools/mcp_servers")
                    .and_then(Value::as_object)
            });
        if let Some(mcp_servers) = mcp_servers {
            for server_name in mcp_servers.keys() {
                let trimmed = server_name.trim();
                if !trimmed.is_empty() {
                    tools.push(format!("mcp:{trimmed}"));
                }
            }
        }

        let restricted = config
            .pointer("/tools/restrictToWorkspace")
            .and_then(Value::as_bool)
            .or_else(|| {
                config
                    .pointer("/tools/restrict_to_workspace")
                    .and_then(Value::as_bool)
            })
            .unwrap_or(false);
        if restricted {
            tools.push("workspace.restricted".to_string());
        }
    }

    tools.sort();
    tools.dedup();
    tools
}

fn extract_nanobot_reply(stdout: &str, stderr: &str) -> String {
    let stdout_reply = sanitize_nanobot_cli_text(stdout);
    if !stdout_reply.is_empty() {
        return stdout_reply;
    }

    sanitize_nanobot_cli_text(stderr)
}

fn sanitize_nanobot_cli_text(raw: &str) -> String {
    let lines = raw
        .replace('\r', "")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('✓'))
        .filter(|line| !line.starts_with("You:"))
        .filter(|line| !line.starts_with("Goodbye"))
        .filter(|line| !line.contains("nanobot is thinking"))
        .filter(|line| !line.contains("Interactive mode"))
        .filter(|line| !line.chars().all(|ch| ch == '='))
        .filter(|line| !is_nanobot_telemetry_line(line))
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    lines.join("\n").trim().to_string()
}

fn is_nanobot_telemetry_line(line: &str) -> bool {
    let normalized = line.trim().to_lowercase();
    normalized.starts_with("prompt:")
        || normalized.starts_with("generation:")
        || normalized.starts_with("peak memory:")
        || normalized.starts_with("completed •")
        || normalized.ends_with("ms")
            && (normalized.contains("session ") || normalized.contains("local shared model"))
        || normalized.contains("tokens-per-sec")
}

fn dedup_vec(values: &mut Vec<String>) {
    values.retain(|value| !value.trim().is_empty());
    let mut deduped = Vec::new();
    for value in values.iter() {
        if !deduped.iter().any(|entry: &String| entry == value) {
            deduped.push(value.clone());
        }
    }
    *values = deduped;
}

fn now_unix_ms() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis() as u64)
}

fn nanobot_home_dir() -> FsPathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    FsPathBuf::from(home)
}

fn nanobot_config_path() -> FsPathBuf {
    nanobot_data_path().join("config.json")
}

fn nanobot_workspace_path() -> FsPathBuf {
    nanobot_data_path().join("workspace")
}
