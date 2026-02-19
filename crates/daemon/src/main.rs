mod catalog;
mod chat_stream;
mod config;
mod openclaw;

use std::io;
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
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Clone)]
struct AppState {
    provider_mode: LocalProviderMode,
    mlx_provider: Arc<MlxProvider>,
    ollama_provider: Arc<OllamaProvider>,
    openclaw_local_provider: Arc<MlxProvider>,
    catalog: Arc<CatalogService>,
    chat_runtime: ChatRuntimeConfig,
    openclaw_runtime: Arc<OpenClawRuntime>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LocalProviderMode {
    Auto,
    Mlx,
    Ollama,
}

impl LocalProviderMode {
    fn from_env(value: &str) -> Self {
        match value.trim().to_lowercase().as_str() {
            "mlx" => Self::Mlx,
            "ollama" => Self::Ollama,
            _ => Self::Auto,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Mlx => "mlx",
            Self::Ollama => "ollama",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RoutedProvider {
    Mlx,
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
    api_key: String,
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
    results: Vec<BraveSearchResultItem>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let cfg = AppConfig::from_env();
    info!("starting daemon on {}", cfg.bind_addr);
    let provider_mode = LocalProviderMode::from_env(&cfg.local_provider);

    let mlx_provider = Arc::new(MlxProvider::new(MlxProviderConfig {
        models_dir: cfg.models_dir.clone(),
        command: cfg.mlx_command.clone(),
        command_prefix_args: cfg.mlx_prefix_args.clone(),
        command_suffix_args: cfg.mlx_suffix_args.clone(),
        timeout: cfg.mlx_timeout,
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
        ollama_provider,
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
    Json(request): Json<BraveSearchRequest>,
) -> Result<Json<BraveSearchResponse>, AppError> {
    let query = request.query.trim();
    let api_key = request.api_key.trim();
    let max_results = request.max_results.unwrap_or(5).clamp(1, 10);

    if query.is_empty() {
        return Err(AppError::Provider(ProviderError::InvalidRequest {
            details: "query nao pode ser vazio".to_string(),
        }));
    }

    if api_key.is_empty() {
        return Err(AppError::Provider(ProviderError::InvalidRequest {
            details: "api_key nao pode ser vazio".to_string(),
        }));
    }

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
        LocalProviderMode::Ollama => state.ollama_provider.list_models().await,
        LocalProviderMode::Auto => {
            let mlx_models = state.mlx_provider.list_models().await?;
            let ollama_models = match state.ollama_provider.list_models().await {
                Ok(models) => models,
                Err(error) => {
                    warn!("ollama unavailable while listing models in auto mode: {error}");
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

    match state.provider_mode {
        LocalProviderMode::Mlx => {
            return Ok(RoutedModel {
                provider: RoutedProvider::Mlx,
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
