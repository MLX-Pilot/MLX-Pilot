mod catalog;
mod chat_stream;
mod config;
mod openclaw;

use std::io;
use std::path::{PathBuf as FsPathBuf};
use std::process::{Command, Output};
use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use bytes::Bytes;
use catalog::{
    CatalogConfig, CatalogError, CatalogSearchQuery, CatalogService, CatalogSourceDescriptor,
    CreateDownloadRequest, DownloadJob, RemoteModelCard,
};
use chat_stream::{spawn_chat_stream, ChatRuntimeConfig, ChatStreamEvent};
use config::AppConfig;
use llamacpp_provider::{LlamaCppProvider, LlamaCppProviderConfig};
use mlx_ollama_core::{ChatRequest, ChatResponse, ModelDescriptor, ModelProvider, ProviderError};
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
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

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
        let (status, message) = match self {
            AppError::Provider(error) => map_provider_error(error),
            AppError::Catalog(error) => map_catalog_error(error),
            AppError::OpenClaw(error) => map_openclaw_error(error),
            AppError::NotFound(message) => (StatusCode::NOT_FOUND, message),
        };

        (status, Json(ErrorBody { error: message })).into_response()
    }
}

fn map_provider_error(error: ProviderError) -> (StatusCode, String) {
    match error {
        ProviderError::InvalidRequest { details } => (StatusCode::BAD_REQUEST, details),
        ProviderError::ModelNotFound { model_id } => (
            StatusCode::NOT_FOUND,
            format!("model '{model_id}' not found"),
        ),
        ProviderError::Timeout { seconds } => (
            StatusCode::GATEWAY_TIMEOUT,
            format!("provider timeout ({seconds}s)"),
        ),
        ProviderError::Unavailable { details } => (StatusCode::SERVICE_UNAVAILABLE, details),
        ProviderError::CommandFailed { command, stderr } => (
            StatusCode::BAD_GATEWAY,
            format!("command failed: {command}; stderr: {stderr}"),
        ),
        ProviderError::Io { context, source } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{context}: {source}"),
        ),
    }
}

fn map_catalog_error(error: CatalogError) -> (StatusCode, String) {
    match error {
        CatalogError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
        CatalogError::NotFound(message) => (StatusCode::NOT_FOUND, message),
        CatalogError::Network(message) => (StatusCode::BAD_GATEWAY, message),
        CatalogError::Cancelled { details } => (StatusCode::CONFLICT, details),
        CatalogError::Io { context, source } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{context}: {source}"),
        ),
        CatalogError::Unavailable(message) => (StatusCode::SERVICE_UNAVAILABLE, message),
    }
}

fn map_openclaw_error(error: OpenClawError) -> (StatusCode, String) {
    match error {
        OpenClawError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
        OpenClawError::Io { context, source } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{context}: {source}"),
        ),
        OpenClawError::CommandFailed { command, stderr } => (
            StatusCode::BAD_GATEWAY,
            format!("command failed: {command}; stderr: {stderr}"),
        ),
        OpenClawError::Parse { details } => (StatusCode::BAD_GATEWAY, details),
        OpenClawError::Timeout { seconds } => (
            StatusCode::GATEWAY_TIMEOUT,
            format!("timeout ao consultar openclaw ({seconds}s)"),
        ),
        OpenClawError::Unavailable(message) => (StatusCode::SERVICE_UNAVAILABLE, message),
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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
    };

    let app = Router::new()
        .route("/config", get(get_config).post(update_config))
        .route("/health", get(health))
        .route("/models", get(list_models))
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
        .route("/nanobot/status", get(nanobot_status))
        .route("/nanobot/onboard", post(nanobot_onboard))
        .route("/nanobot/install", post(nanobot_install))
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
    new_config.save_settings().map_err(|e| AppError::NotFound(format!("Falha ao salvar config: {}", e)))?;
    Ok(Json(new_config))
}

async fn list_models(
    State(state): State<AppState>,
) -> Result<Json<Vec<ModelDescriptor>>, AppError> {
    let models = list_chat_models(&state).await?;
    Ok(Json(models))
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
    let api_key = if !request_api_key.is_empty() {
        request_api_key.clone()
    } else {
        state
            .brave_api_key
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_string()
    };
    let max_results = request.max_results.unwrap_or(5).clamp(1, 10);

    if query.is_empty() {
        return Err(AppError::Provider(ProviderError::InvalidRequest {
            details: "query nao pode ser vazio".to_string(),
        }));
    }

    if api_key.is_empty() {
        return Err(AppError::Provider(ProviderError::InvalidRequest {
            details: "api_key Brave nao configurada (UI ou APP_BRAVE_API_KEY)".to_string(),
        }));
    }

    let key_source = if !request_api_key.is_empty() {
        "request".to_string()
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
            let mlx_models = state.mlx_provider.list_models().await?;
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
    Path(job_id): Path<String>,
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
    Path(job_id): Path<String>,
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

async fn nanobot_status() -> Result<Json<NanoBotStatusResponse>, AppError> {
    let cfg = AppConfig::load_settings().apply_env();
    let spec = resolve_nanobot_command(&cfg);
    let config_path = nanobot_config_path();
    let workspace_path = nanobot_workspace_path();

    let version = run_nanobot_command(&spec, &["--version"], None)
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            let text = decode_command_output(&output);
            text.lines()
                .next()
                .map(str::trim)
                .unwrap_or("")
                .to_string()
        })
        .filter(|value| !value.is_empty());

    let status_output = run_nanobot_command(&spec, &["status"], None)
        .ok()
        .filter(|output| output.status.success())
        .map(|output| decode_command_output(&output))
        .filter(|value| !value.is_empty());

    let installed = version.is_some();
    let message = if installed {
        "NanoBot detectado. Se o config ainda nao existe, inicialize com o botao onboard.".to_string()
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
    let config_path = nanobot_config_path();

    if config_path.exists() {
        return Ok(Json(InstallResponse {
            message: format!(
                "Configuracao ja existe em {}. Para evitar prompt interativo, o onboard automatico foi ignorado.",
                config_path.display()
            ),
        }));
    }

    let output = run_nanobot_command(&spec, &["onboard"], None).map_err(|error| {
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
        .ok_or_else(|| AppError::Provider(ProviderError::Unavailable {
            details: "Nao foi possivel determinar diretorio pai do NanoBot.".to_string(),
        }))?
        .to_path_buf();

    std::fs::create_dir_all(&repo_parent).map_err(|error| AppError::Provider(ProviderError::Io {
        context: "Falha ao criar diretorio pai do NanoBot".to_string(),
        source: error,
    }))?;

    if repo_dir.join(".git").exists() {
        let output = Command::new("git")
            .arg("-C")
            .arg(&repo_dir)
            .arg("pull")
            .arg("--ff-only")
            .output()
            .map_err(|error| AppError::Provider(ProviderError::Io {
                context: "Falha ao atualizar repositorio NanoBot".to_string(),
                source: error,
            }))?;

        if !output.status.success() {
            return Err(AppError::Provider(ProviderError::CommandFailed {
                command: format!("git -C {} pull --ff-only", repo_dir.display()),
                stderr: decode_command_output(&output),
            }));
        }
    } else {
        if repo_dir.exists() {
            let has_files = std::fs::read_dir(&repo_dir)
                .map_err(|error| AppError::Provider(ProviderError::Io {
                    context: "Falha ao ler diretorio de destino do NanoBot".to_string(),
                    source: error,
                }))?
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
            .map_err(|error| AppError::Provider(ProviderError::Io {
                context: "Falha ao clonar repositorio NanoBot".to_string(),
                source: error,
            }))?;

        if !output.status.success() {
            return Err(AppError::Provider(ProviderError::CommandFailed {
                command: format!("git clone https://github.com/HKUDS/nanobot.git {}", repo_dir.display()),
                stderr: decode_command_output(&output),
            }));
        }
    }

    let install_output = Command::new("python3")
        .arg("-m")
        .arg("pip")
        .arg("install")
        .arg("-e")
        .arg(&repo_dir)
        .output()
        .map_err(|error| AppError::Provider(ProviderError::Io {
            context: "Falha ao executar instalacao pip do NanoBot".to_string(),
            source: error,
        }))?;

    if !install_output.status.success() {
        return Err(AppError::Provider(ProviderError::CommandFailed {
            command: format!("python3 -m pip install -e {}", repo_dir.display()),
            stderr: decode_command_output(&install_output),
        }));
    }

    Ok(Json(InstallResponse {
        message: format!(
            "NanoBot pronto. Repo: {}. Proximo passo: execute o onboard para criar ~/.nanobot/config.json.",
            repo_dir.display()
        ),
    }))
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
            .ok_or_else(|| AppError::Provider(ProviderError::Unavailable {
                details: "Caminho NanoBot .py invalido (sem diretorio pai).".to_string(),
            }));
    }

    if candidate.exists() {
        if candidate.is_dir() {
            return Ok(candidate);
        }

        return candidate
            .parent()
            .map(|parent| parent.to_path_buf())
            .ok_or_else(|| AppError::Provider(ProviderError::Unavailable {
                details: "Caminho NanoBot invalido (sem diretorio pai).".to_string(),
            }));
    }

    if raw.contains('/') || raw.contains('\\') {
        if candidate.extension().is_none() {
            return Ok(candidate);
        }

        return candidate
            .parent()
            .map(|parent| parent.to_path_buf())
            .ok_or_else(|| AppError::Provider(ProviderError::Unavailable {
                details: "Caminho NanoBot invalido (sem diretorio pai).".to_string(),
            }));
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
        if !candidate.exists() {
            if let Some(parent) = candidate.parent() {
                let local_venv = parent.join(".venv").join("bin").join("nanobot");
                if local_venv.exists() {
                    return NanoBotCommandSpec {
                        program: local_venv.display().to_string(),
                        args: Vec::new(),
                    };
                }
            }

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

fn run_nanobot_command(
    spec: &NanoBotCommandSpec,
    extra_args: &[&str],
    cwd: Option<&std::path::Path>,
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

fn nanobot_config_path() -> FsPathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    FsPathBuf::from(home).join(".nanobot").join("config.json")
}

fn nanobot_workspace_path() -> FsPathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    FsPathBuf::from(home).join(".nanobot").join("workspace")
}
