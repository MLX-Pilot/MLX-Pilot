mod catalog;
mod chat_stream;
mod config;
mod openclaw;

use std::io;
use std::sync::Arc;

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
use chat_stream::{spawn_chat_stream, ChatRuntimeConfig};
use config::AppConfig;
use mlx_ollama_core::{ChatRequest, ChatResponse, ModelDescriptor, ModelProvider, ProviderError};
use mlx_provider::{MlxProvider, MlxProviderConfig};
use openclaw::{
    OpenClawChatRequest, OpenClawChatResponse, OpenClawCloudModel, OpenClawCurrentModel,
    OpenClawError, OpenClawLogChunkResponse, OpenClawLogQuery, OpenClawModelsStateResponse,
    OpenClawRuntime, OpenClawRuntimeConfig, OpenClawSetModelRequest, OpenClawStatusResponse,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[derive(Clone)]
struct AppState {
    provider: Arc<dyn ModelProvider>,
    catalog: Arc<CatalogService>,
    chat_runtime: ChatRuntimeConfig,
    openclaw_runtime: Arc<OpenClawRuntime>,
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let cfg = AppConfig::from_env();
    info!("starting daemon on {}", cfg.bind_addr);

    let provider = Arc::new(MlxProvider::new(MlxProviderConfig {
        models_dir: cfg.models_dir.clone(),
        command: cfg.mlx_command.clone(),
        command_prefix_args: cfg.mlx_prefix_args.clone(),
        command_suffix_args: cfg.mlx_suffix_args.clone(),
        timeout: cfg.mlx_timeout,
    }));

    let catalog = Arc::new(CatalogService::new(CatalogConfig {
        hf_api_base: cfg.hf_api_base.clone(),
        hf_token: cfg.hf_token.clone(),
        downloads_root: cfg.remote_downloads_dir.clone(),
        search_limit_default: cfg.catalog_search_limit,
        download_timeout: cfg.catalog_download_timeout,
    })?);

    let state = AppState {
        provider,
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
        .route("/openclaw/status", get(openclaw_status))
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
        provider: state.provider.provider_id(),
    })
}

async fn list_models(
    State(state): State<AppState>,
) -> Result<Json<Vec<ModelDescriptor>>, AppError> {
    let models = state.provider.list_models().await?;
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

    let response = state.provider.chat(request).await.map_err(|error| {
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

    let receiver = spawn_chat_stream(state.chat_runtime.clone(), request);

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
    let local_models = state.provider.list_models().await?;

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

        let local_models = state.provider.list_models().await?;
        let selected = local_models
            .into_iter()
            .find(|entry| entry.id == model_id)
            .ok_or_else(|| AppError::NotFound(format!("modelo local '{model_id}' nao encontrado")))?;

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
