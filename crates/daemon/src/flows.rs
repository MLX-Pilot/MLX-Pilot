//! Rotas HTTP e integracao do motor de workflows nativo.
//!
//! Substitui o antigo `n8n_integration`: em vez de proxiar a Public API de uma
//! instancia externa do n8n, o daemon guarda os fluxos em disco e os executa
//! ele mesmo, via `mlx-flow`. Nao ha API key, nem porta 5678, nem processo
//! Node para subir.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use mlx_agent_core::ToolRegistry;
use mlx_agent_tools::{ExecutionMode, ToolContext};
use mlx_flow::engine::{EngineError, FlowEngine, RunOptions};
use mlx_flow::host::{
    AgentNodeRequest, AgentNodeResult, FlowHost, ToolNodeRequest, ToolNodeResult,
};
use mlx_flow::model::Flow;
use mlx_flow::nodes;
use mlx_flow::registry::NodeRegistry;
use mlx_flow::run::{RunRecord, TriggerSource};
use mlx_flow::store::FlowStore;
use mlx_flow::{NodeDescriptor, ValidationReport};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

/// Limite padrao de itens do historico devolvido pela API.
const DEFAULT_RUN_LIMIT: usize = 50;

/// Servicos do motor guardados no `AppState`.
///
/// Guarda apenas o que nao depende do `AppState`: o executor de nos precisa do
/// agente, e esse e construido por requisicao a partir do proprio `State`, o
/// que evita uma referencia circular entre `AppState` e `FlowEngine`.
#[derive(Clone)]
pub struct FlowService {
    pub store: Arc<FlowStore>,
    registry: Arc<NodeRegistry>,
    tools: Arc<ToolRegistry>,
    default_workspace: std::path::PathBuf,
}

impl std::fmt::Debug for FlowService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FlowService")
            .field("flows_dir", &self.store.flows_dir())
            .finish_non_exhaustive()
    }
}

impl FlowService {
    pub fn new(root: impl AsRef<std::path::Path>, default_workspace: std::path::PathBuf) -> Self {
        Self {
            store: Arc::new(FlowStore::new(root)),
            registry: Arc::new(nodes::builtin_registry()),
            tools: Arc::new(ToolRegistry::with_builtins()),
            default_workspace,
        }
    }

    /// Motor ligado ao estado do daemon desta requisicao.
    pub fn engine(&self, state: &crate::AppState) -> FlowEngine {
        FlowEngine::new(
            self.registry.clone(),
            Arc::new(DaemonFlowHost {
                state: state.clone(),
                tools: self.tools.clone(),
                default_workspace: self.default_workspace.clone(),
            }),
        )
    }

    /// Catalogo de tipos de no para a paleta da UI.
    pub fn catalog(&self) -> Vec<NodeDescriptor> {
        self.registry.catalog()
    }

    /// Nomes das ferramentas disponiveis para o no `tool.call`.
    pub fn tool_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .tools
            .definitions()
            .into_iter()
            .map(|definition| definition.name)
            .collect();
        names.sort();
        names
    }
}

/// Implementacao da ponte do motor sobre o daemon.
struct DaemonFlowHost {
    state: crate::AppState,
    tools: Arc<ToolRegistry>,
    default_workspace: std::path::PathBuf,
}

#[async_trait::async_trait]
impl FlowHost for DaemonFlowHost {
    async fn run_agent(&self, request: AgentNodeRequest) -> Result<AgentNodeResult, String> {
        let run_request = crate::agent_api::AgentRunRequest {
            session_id: request.session_id.clone(),
            message: request.message,
            provider: request.provider,
            model_id: request.model_id,
            api_key: None,
            base_url: request.base_url,
            custom_headers: None,
            streaming: Some(false),
            fallback_enabled: Some(false),
            fallback_provider: None,
            fallback_model_id: None,
            // Um no de fluxo nunca deve pedir aprovacao interativa: ou a
            // ferramenta esta liberada na configuracao do no, ou nao roda.
            execution_mode: Some("dry_run".to_string()),
            approval_mode: Some("deny".to_string()),
            system_prompt: request.system_prompt,
            max_iterations: request.max_iterations,
            max_prompt_tokens: None,
            max_history_messages: Some(0),
            max_tools_in_prompt: None,
            temperature: request.temperature,
            aggressive_tool_filtering: Some(true),
            enable_tool_call_fallback: Some(false),
            runtime_variant: Some("classic".to_string()),
            persist_tool_events: Some(false),
            session_search_enabled: Some(false),
            memory_profile: Some("minimal".to_string()),
            memory_snapshot_mode: None,
            session_context: None,
            gateway_context: None,
            delegate_depth: None,
            enabled_skills: Some(Vec::new()),
            enabled_tools: request.enabled_tools,
            toolset_id: None,
            provider_profile_id: request.provider_profile_id,
            workspace_root: None,
            provider_timeout_secs: None,
            ooda_max_cycles: None,
            ooda_max_steps: None,
            ooda_max_tool_calls: None,
            ooda_deadline_secs: None,
        };

        let response = crate::agent_api::execute_agent_request(&self.state, run_request)
            .await
            .map_err(|error| error.message())?;

        Ok(AgentNodeResult {
            content: response.content,
            session_id: response.session_id,
            provider: response.provider,
            model_id: response.model_id,
            total_tokens: response.total_tokens,
            latency_ms: response.latency_ms,
        })
    }

    async fn call_tool(&self, request: ToolNodeRequest) -> Result<ToolNodeResult, String> {
        let workspace_root = request
            .workspace_root
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| self.default_workspace.clone());

        let ctx = ToolContext {
            workspace_root,
            session_id: format!("flow-{}", uuid::Uuid::new_v4()),
            active_skill: None,
            mode: if request.read_only {
                ExecutionMode::ReadOnly
            } else {
                ExecutionMode::Full
            },
        };

        match self.tools.dispatch(&request.name, &request.params, &ctx).await {
            Ok(result) => Ok(ToolNodeResult {
                output: result.output,
                is_error: result.is_error,
                metadata: result.metadata.into_iter().collect(),
            }),
            Err(error) => Err(error.to_string()),
        }
    }

    fn available_tools(&self) -> Vec<String> {
        self.tools
            .definitions()
            .into_iter()
            .map(|definition| definition.name)
            .collect()
    }
}

// ─────────────────────────── Respostas ───────────────────────────

#[derive(Debug, Serialize)]
struct FlowErrorResponse {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    validation: Option<ValidationReport>,
}

fn flow_error(status: StatusCode, error: &str, details: Option<String>) -> Response {
    (
        status,
        Json(FlowErrorResponse {
            error: error.to_string(),
            details,
            validation: None,
        }),
    )
        .into_response()
}

fn validation_error(report: ValidationReport) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(FlowErrorResponse {
            error: "flow_invalid".to_string(),
            details: Some(report.error_summary()),
            validation: Some(report),
        }),
    )
        .into_response()
}

fn engine_error(error: EngineError) -> Response {
    match error {
        EngineError::Invalid(report) => validation_error(report),
        EngineError::NoEntryNode(message) => {
            flow_error(StatusCode::BAD_REQUEST, "flow_no_entry_node", Some(message))
        }
        EngineError::UnknownNode(id) => flow_error(
            StatusCode::BAD_REQUEST,
            "flow_unknown_node",
            Some(format!("o no `{id}` nao existe neste fluxo")),
        ),
    }
}

fn storage_error(error: std::io::Error) -> Response {
    flow_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "flow_storage_error",
        Some(error.to_string()),
    )
}

// ─────────────────────────── Handlers ───────────────────────────

/// `GET /flows` — lista os fluxos salvos.
pub async fn list_flows(State(state): State<crate::AppState>) -> Response {
    match state.flows.store.list().await {
        Ok(flows) => Json(json!({ "flows": flows })).into_response(),
        Err(error) => storage_error(error),
    }
}

/// `GET /flows/node-types` — catalogo de nos e ferramentas para a UI.
pub async fn node_types(State(state): State<crate::AppState>) -> Response {
    Json(json!({
        "nodes": state.flows.catalog(),
        "tools": state.flows.tool_names(),
    }))
    .into_response()
}

/// `GET /flows/{id}` — um fluxo completo.
pub async fn get_flow(
    State(state): State<crate::AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match state.flows.store.get(&id).await {
        Ok(Some(flow)) => Json(flow).into_response(),
        Ok(None) => flow_error(
            StatusCode::NOT_FOUND,
            "flow_not_found",
            Some(format!("nao existe fluxo com id `{id}`")),
        ),
        Err(error) => storage_error(error),
    }
}

/// `POST /flows` — cria ou atualiza um fluxo.
///
/// Valida antes de gravar: um fluxo invalido nunca chega ao disco.
pub async fn save_flow(State(state): State<crate::AppState>, Json(flow): Json<Flow>) -> Response {
    let engine = state.flows.engine(&state);
    let report = engine.validate(&flow);
    if !report.valid {
        return validation_error(report);
    }

    match state.flows.store.save(flow).await {
        Ok(saved) => Json(json!({ "flow": saved, "validation": report })).into_response(),
        Err(error) => storage_error(error),
    }
}

/// `POST /flows/validate` — valida sem gravar.
pub async fn validate_flow(
    State(state): State<crate::AppState>,
    Json(flow): Json<Flow>,
) -> Response {
    Json(state.flows.engine(&state).validate(&flow)).into_response()
}

/// `DELETE /flows/{id}` — remove o fluxo e seu historico.
pub async fn delete_flow(
    State(state): State<crate::AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match state.flows.store.delete(&id).await {
        Ok(true) => {
            if let Err(error) = state.flows.store.delete_runs_of(&id).await {
                warn!(%error, "nao foi possivel apagar o historico do fluxo");
            }
            Json(json!({ "deleted": true, "id": id })).into_response()
        }
        Ok(false) => flow_error(
            StatusCode::NOT_FOUND,
            "flow_not_found",
            Some(format!("nao existe fluxo com id `{id}`")),
        ),
        Err(error) => storage_error(error),
    }
}

/// Corpo de `POST /flows/{id}/run`.
#[derive(Debug, Default, Deserialize)]
pub struct RunFlowRequest {
    /// Dados injetados no no inicial.
    #[serde(default)]
    pub payload: Value,
    /// Variaveis visiveis em `$env`.
    #[serde(default)]
    pub env: Map<String, Value>,
    /// Comecar de um no especifico, para testar so um trecho.
    #[serde(default)]
    pub start_node: Option<String>,
}

/// `POST /flows/{id}/run` — executa e devolve o registro completo.
pub async fn run_flow(
    State(state): State<crate::AppState>,
    AxumPath(id): AxumPath<String>,
    body: Option<Json<RunFlowRequest>>,
) -> Response {
    let request = body.map(|Json(value)| value).unwrap_or_default();

    let flow = match state.flows.store.get(&id).await {
        Ok(Some(flow)) => flow,
        Ok(None) => {
            return flow_error(
                StatusCode::NOT_FOUND,
                "flow_not_found",
                Some(format!("nao existe fluxo com id `{id}`")),
            )
        }
        Err(error) => return storage_error(error),
    };

    let options = RunOptions {
        trigger: TriggerSource::Manual,
        start_node: request.start_node,
        payload: request.payload,
        env: request.env,
    };

    match execute_and_store(&state, &flow, options).await {
        Ok(record) => Json(record).into_response(),
        Err(error) => engine_error(error),
    }
}

/// Filtros de `GET /flows/runs`.
#[derive(Debug, Deserialize)]
pub struct RunHistoryQuery {
    #[serde(default)]
    limit: Option<usize>,
}

/// `GET /flows/runs` — historico de todas as execucoes.
pub async fn list_all_runs(
    State(state): State<crate::AppState>,
    Query(query): Query<RunHistoryQuery>,
) -> Response {
    let limit = query.limit.unwrap_or(DEFAULT_RUN_LIMIT).clamp(1, 500);
    match state.flows.store.list_runs(None, limit).await {
        Ok(runs) => Json(json!({ "runs": runs })).into_response(),
        Err(error) => storage_error(error),
    }
}

/// `GET /flows/{id}/runs` — historico de um fluxo.
pub async fn list_flow_runs(
    State(state): State<crate::AppState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<RunHistoryQuery>,
) -> Response {
    let limit = query.limit.unwrap_or(DEFAULT_RUN_LIMIT).clamp(1, 500);
    match state.flows.store.list_runs(Some(&id), limit).await {
        Ok(runs) => Json(json!({ "runs": runs })).into_response(),
        Err(error) => storage_error(error),
    }
}

/// `GET /flows/runs/{run_id}` — uma execucao completa.
pub async fn get_run(
    State(state): State<crate::AppState>,
    AxumPath(run_id): AxumPath<String>,
) -> Response {
    match state.flows.store.get_run(&run_id).await {
        Ok(Some(run)) => Json(run).into_response(),
        Ok(None) => flow_error(
            StatusCode::NOT_FOUND,
            "run_not_found",
            Some(format!("nao existe execucao com id `{run_id}`")),
        ),
        Err(error) => storage_error(error),
    }
}

/// `POST /flows/import/n8n` — converte um workflow exportado do n8n.
///
/// E uma conversao de arquivo: nao contata nenhuma instancia do n8n. O fluxo
/// convertido volta na resposta e so e gravado se o usuario salvar.
pub async fn import_n8n(
    State(state): State<crate::AppState>,
    Json(workflow): Json<Value>,
) -> Response {
    // Aceita tanto o JSON do workflow quanto `{ "workflow": {...} }`.
    let source = workflow
        .get("workflow")
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or(workflow);

    match mlx_flow::import::from_n8n(&source) {
        Ok(report) => {
            let validation = state.flows.engine(&state).validate(&report.flow);
            Json(json!({
                "flow": report.flow,
                "warnings": report.warnings,
                "unsupported_kinds": report.unsupported_kinds,
                "validation": validation,
            }))
            .into_response()
        }
        Err(message) => flow_error(StatusCode::BAD_REQUEST, "n8n_import_failed", Some(message)),
    }
}

/// `ANY /flows/webhook/{*path}` — ponto de entrada dos gatilhos de webhook.
pub async fn webhook(
    State(state): State<crate::AppState>,
    AxumPath(path): AxumPath<String>,
    method: Method,
    Query(query): Query<HashMap<String, String>>,
    body: Option<Json<Value>>,
) -> Response {
    let normalized = nodes::triggers::normalize_webhook_path(&path);

    let flows = match state.flows.store.read_all().await {
        Ok(flows) => flows,
        Err(error) => return storage_error(error),
    };

    let bindings = mlx_flow::webhook_bindings(&flows);
    let Some(binding) = bindings
        .iter()
        .find(|binding| binding.path == normalized && binding.method == method.as_str())
    else {
        // Distingue "caminho errado" de "metodo errado": ajuda muito a depurar.
        let path_exists = bindings.iter().any(|binding| binding.path == normalized);
        return if path_exists {
            flow_error(
                StatusCode::METHOD_NOT_ALLOWED,
                "webhook_method_not_allowed",
                Some(format!(
                    "o webhook `{normalized}` nao aceita {}",
                    method.as_str()
                )),
            )
        } else {
            flow_error(
                StatusCode::NOT_FOUND,
                "webhook_not_found",
                Some(format!(
                    "nenhum fluxo ativo publica o webhook `{normalized}`"
                )),
            )
        };
    };

    let Some(flow) = flows.iter().find(|flow| flow.id == binding.flow_id).cloned() else {
        return flow_error(
            StatusCode::NOT_FOUND,
            "flow_not_found",
            Some("o fluxo do webhook desapareceu entre a busca e a execucao".to_string()),
        );
    };

    let payload = json!({
        "body": body.map(|Json(value)| value).unwrap_or(Value::Null),
        "query": query,
        "method": method.as_str(),
        "path": normalized,
        "received_at": Utc::now().to_rfc3339(),
    });

    let options = RunOptions {
        trigger: TriggerSource::Webhook,
        start_node: Some(binding.node_id.clone()),
        payload,
        env: Map::new(),
    };

    if binding.response_mode == "immediate" {
        // Responde na hora e executa em segundo plano.
        let background_state = state.clone();
        let run_id = uuid::Uuid::new_v4().to_string();
        tokio::spawn(async move {
            if let Err(error) = execute_and_store(&background_state, &flow, options).await {
                warn!(%error, flow = %flow.name, "execucao de webhook falhou");
            }
        });
        return (
            StatusCode::ACCEPTED,
            Json(json!({ "accepted": true, "reference": run_id })),
        )
            .into_response();
    }

    match execute_and_store(&state, &flow, options).await {
        Ok(record) => {
            let status = if record.succeeded() {
                StatusCode::OK
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (
                status,
                Json(json!({
                    "run_id": record.id,
                    "status": record.status,
                    "output": record.output,
                    "error": record.error,
                })),
            )
                .into_response()
        }
        Err(error) => engine_error(error),
    }
}

/// Executa o fluxo e grava o registro, quando o fluxo pede historico.
async fn execute_and_store(
    state: &crate::AppState,
    flow: &Flow,
    options: RunOptions,
) -> Result<RunRecord, EngineError> {
    let record = state.flows.engine(state).run(flow, options).await?;

    if flow.settings.save_runs {
        if let Err(error) = state.flows.store.save_run(&record).await {
            // Falhar ao gravar o historico nao invalida a execucao em si.
            warn!(%error, "nao foi possivel gravar o historico da execucao");
        }
    }

    Ok(record)
}

// ─────────────────────────── Agendador ───────────────────────────

/// Dispara fluxos com gatilho `trigger.schedule`.
///
/// Guarda em memoria o ultimo disparo de cada gatilho para nao repetir a mesma
/// ocorrencia entre dois ticks nem disparar tudo de uma vez ao subir.
pub struct FlowScheduler {
    state: crate::AppState,
    last_fired: HashMap<(String, String), DateTime<Utc>>,
}

impl FlowScheduler {
    pub fn new(state: crate::AppState) -> Self {
        Self {
            state,
            last_fired: HashMap::new(),
        }
    }

    /// Sobe o laco de agendamento ate o `CancellationToken` ser acionado.
    pub fn start(mut self, shutdown: CancellationToken) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            info!("agendador de fluxos iniciado (tick de 30s)");

            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => {
                        info!("agendador de fluxos encerrando");
                        break;
                    }
                    _ = interval.tick() => self.tick().await,
                }
            }
        })
    }

    async fn tick(&mut self) {
        let flows = match self.state.flows.store.read_all().await {
            Ok(flows) => flows,
            Err(error) => {
                warn!(%error, "agendador nao conseguiu ler os fluxos");
                return;
            }
        };

        let bindings = mlx_flow::schedule_bindings(&flows);
        let now = Utc::now();

        for binding in bindings {
            let key = (binding.flow_id.clone(), binding.node_id.clone());
            // Na primeira vez que vemos um gatilho, so marcamos o relogio: subir
            // o daemon nao deve disparar execucoes atrasadas em lote.
            let reference = *self.last_fired.entry(key.clone()).or_insert(now);

            let Some(next) = next_occurrence(&binding.cron, reference) else {
                warn!(cron = %binding.cron, flow = %binding.flow_name, "expressao cron invalida");
                continue;
            };
            if next > now {
                continue;
            }
            self.last_fired.insert(key, now);

            let Some(flow) = flows.iter().find(|flow| flow.id == binding.flow_id).cloned() else {
                continue;
            };

            debug!(flow = %flow.name, cron = %binding.cron, "disparando fluxo agendado");
            let state = self.state.clone();
            let options = RunOptions {
                trigger: TriggerSource::Schedule,
                start_node: Some(binding.node_id.clone()),
                payload: json!({ "triggered_at": now.to_rfc3339(), "cron": binding.cron }),
                env: Map::new(),
            };
            tokio::spawn(async move {
                if let Err(error) = execute_and_store(&state, &flow, options).await {
                    warn!(%error, flow = %flow.name, "execucao agendada falhou");
                }
            });
        }
    }
}

/// Proxima ocorrencia de uma expressao cron depois de `after`.
fn next_occurrence(expression: &str, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    use std::str::FromStr;
    cron::Schedule::from_str(expression)
        .ok()?
        .after(&after)
        .next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;

    #[test]
    fn next_occurrence_reads_six_field_cron() {
        let base = DateTime::parse_from_rfc3339("2026-01-01T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        // Todo minuto no segundo zero.
        let next = next_occurrence("0 * * * * *", base).unwrap();
        assert_eq!(next.to_rfc3339(), "2026-01-01T10:01:00+00:00");
    }

    #[test]
    fn next_occurrence_rejects_a_broken_expression() {
        let base = Utc::now();
        assert!(next_occurrence("nao e cron", base).is_none());
        assert!(next_occurrence("", base).is_none());
    }

    #[test]
    fn a_five_minute_cron_does_not_fire_within_the_same_minute() {
        let base = DateTime::parse_from_rfc3339("2026-01-01T10:00:10Z")
            .unwrap()
            .with_timezone(&Utc);
        let next = next_occurrence("0 */5 * * * *", base).unwrap();
        // A proxima ocorrencia esta no futuro, entao o tick seguinte nao dispara.
        assert!(next > base);
        assert!(next <= base + ChronoDuration::minutes(5));
    }

    #[test]
    fn run_request_defaults_are_permissive() {
        let request: RunFlowRequest = serde_json::from_value(json!({})).unwrap();
        assert_eq!(request.payload, Value::Null);
        assert!(request.env.is_empty());
        assert!(request.start_node.is_none());
    }

    #[test]
    fn run_request_accepts_payload_and_env() {
        let request: RunFlowRequest = serde_json::from_value(json!({
            "payload": { "a": 1 },
            "env": { "TOKEN": "x" },
            "start_node": "n2"
        }))
        .unwrap();
        assert_eq!(request.payload["a"], json!(1));
        assert_eq!(request.env["TOKEN"], json!("x"));
        assert_eq!(request.start_node.as_deref(), Some("n2"));
    }
}
