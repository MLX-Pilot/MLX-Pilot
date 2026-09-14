//! Escalonador e executor do fluxo.
//!
//! A execucao percorre as camadas topologicas produzidas por [`FlowGraph`].
//! Dentro de uma camada os nos sao independentes entre si e rodam
//! concorrentemente, limitados por `settings.max_parallel`. Entre camadas ha
//! uma barreira: a camada seguinte so comeca quando a anterior terminou, o que
//! garante que todo no ve as saidas completas dos seus antecessores.
//!
//! Poda de ramo: um no so executa se pelo menos uma aresta de entrada estiver
//! ativa, ou seja, se o no de origem executou com sucesso e emitiu itens na
//! porta daquela aresta. E assim que `flow.if` corta o ramo nao escolhido.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tracing::{debug, warn};

use crate::expr::NodeView;
use crate::graph::{FlowGraph, ValidationReport};
use crate::host::FlowHost;
use crate::model::Flow;
use crate::registry::{NodeContext, NodeError, NodeOutput, NodeRegistry};
use crate::run::{
    preview_items, preview_main_port, NodeRun, NodeStatus, RunRecord, RunStatus, TriggerSource,
};

/// Falha que impede a execucao de comecar.
#[derive(Debug, Clone)]
pub enum EngineError {
    /// O fluxo nao passou na validacao estrutural.
    Invalid(ValidationReport),
    /// Nao foi possivel decidir por onde comecar.
    NoEntryNode(String),
    /// O `start_node` pedido nao existe no fluxo.
    UnknownNode(String),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(report) => write!(formatter, "fluxo invalido: {}", report.error_summary()),
            Self::NoEntryNode(message) => write!(formatter, "{message}"),
            Self::UnknownNode(id) => write!(formatter, "no inexistente: {id}"),
        }
    }
}

impl std::error::Error for EngineError {}

/// Parametros de uma execucao.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunOptions {
    /// Quem disparou.
    pub trigger: TriggerSource,
    /// Id do no inicial. Quando ausente, o motor escolhe pelo tipo de gatilho.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_node: Option<String>,
    /// Dados injetados no no inicial. Array vira varios itens.
    #[serde(default)]
    pub payload: Value,
    /// Variaveis visiveis em `$env`.
    #[serde(default)]
    pub env: Map<String, Value>,
    /// Id da execucao. Quem dispara pode reservar um id antes de executar para
    /// devolve-lo de imediato ao chamador — e o que o webhook assincrono faz.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            trigger: TriggerSource::Manual,
            start_node: None,
            payload: Value::Null,
            env: Map::new(),
            run_id: None,
        }
    }
}

impl RunOptions {
    /// Execucao manual com um payload.
    pub fn manual(payload: Value) -> Self {
        Self {
            trigger: TriggerSource::Manual,
            payload,
            ..Default::default()
        }
    }
}

/// Motor de execucao.
#[derive(Clone)]
pub struct FlowEngine {
    registry: Arc<NodeRegistry>,
    host: Arc<dyn FlowHost>,
}

impl std::fmt::Debug for FlowEngine {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FlowEngine")
            .field("registry", &self.registry)
            .finish_non_exhaustive()
    }
}

impl FlowEngine {
    pub fn new(registry: Arc<NodeRegistry>, host: Arc<dyn FlowHost>) -> Self {
        Self { registry, host }
    }

    pub fn registry(&self) -> &NodeRegistry {
        &self.registry
    }

    pub fn host(&self) -> &Arc<dyn FlowHost> {
        &self.host
    }

    /// Valida o fluxo contra os tipos de no registrados.
    pub fn validate(&self, flow: &Flow) -> ValidationReport {
        crate::graph::validate(flow, |kind| self.registry.contains(kind))
    }

    /// Executa o fluxo do inicio ao fim.
    pub async fn run(&self, flow: &Flow, options: RunOptions) -> Result<RunRecord, EngineError> {
        let graph = FlowGraph::build(flow, |kind| self.registry.contains(kind))
            .map_err(EngineError::Invalid)?;
        let entry = self.resolve_entry(flow, &graph, &options)?;

        let run_id = options
            .run_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let started_at = Utc::now();
        let deadline = started_at
            + chrono::Duration::seconds(flow.settings.effective_timeout_secs() as i64);
        let max_parallel = flow.settings.effective_max_parallel();

        // Ramos nao alcancaveis a partir do no inicial nunca executam.
        let active_scope = graph.reachable_from(flow, entry);

        let mut runs: Vec<NodeRun> = flow
            .nodes
            .iter()
            .map(|node| NodeRun::pending(&node.id, &node.name, &node.kind))
            .collect();
        // Saida por (indice do no, porta). Chave para decidir arestas ativas.
        let mut outputs: HashMap<usize, NodeOutput> = HashMap::new();
        let mut succeeded: HashSet<usize> = HashSet::new();
        let mut node_views: HashMap<String, NodeView> = HashMap::new();

        let entry_items = payload_items(&options.payload);
        let mut run_status = RunStatus::Success;
        let mut run_error: Option<String> = None;

        'levels: for level in graph.levels() {
            if Utc::now() >= deadline {
                run_status = RunStatus::TimedOut;
                run_error = Some(format!(
                    "a execucao passou do limite de {}s",
                    flow.settings.effective_timeout_secs()
                ));
                break;
            }

            // Decide entrada e atividade de cada no da camada.
            let mut runnable: Vec<(usize, Vec<Value>)> = Vec::new();
            for &index in level {
                if !active_scope.contains(&index) {
                    runs[index].status = NodeStatus::Skipped;
                    continue;
                }

                let items = if index == entry {
                    entry_items.clone()
                } else {
                    match collect_inputs(flow, &graph, index, &outputs, &succeeded) {
                        Some(items) => items,
                        None => {
                            runs[index].status = NodeStatus::Skipped;
                            continue;
                        }
                    }
                };

                let node = &flow.nodes[index];
                if node.disabled {
                    // No desligado repassa a entrada, para poder ser desativado
                    // temporariamente sem quebrar o resto do fluxo.
                    runs[index].status = NodeStatus::Disabled;
                    runs[index].input_count = items.len();
                    runs[index].output_count = items.len();
                    runs[index].output = preview_main_port(&items);
                    node_views.insert(
                        node.name.clone(),
                        NodeView {
                            items: items.clone(),
                        },
                    );
                    succeeded.insert(index);
                    outputs.insert(index, NodeOutput::main(items));
                    continue;
                }

                runnable.push((index, items));
            }

            if runnable.is_empty() {
                continue;
            }

            let remaining = (deadline - Utc::now()).num_milliseconds().max(0) as u64;
            let results: Vec<NodeAttempt> = stream::iter(runnable.into_iter().map(
                |(index, items)| {
                    let node_views = &node_views;
                    let options_ref = &options;
                    let run_id = run_id.as_str();
                    async move {
                        self.execute_node(
                            flow,
                            index,
                            items,
                            node_views,
                            options_ref,
                            run_id,
                            Duration::from_millis(remaining),
                        )
                        .await
                    }
                },
            ))
            .buffer_unordered(max_parallel)
            .collect()
            .await;

            for attempt in results {
                let index = attempt.index;
                let node = &flow.nodes[index];
                let record = &mut runs[index];
                record.started_at = Some(attempt.started_at);
                record.finished_at = Some(attempt.finished_at);
                record.duration_ms = (attempt.finished_at - attempt.started_at)
                    .num_milliseconds()
                    .max(0) as u64;
                record.attempts = attempt.attempts;
                record.input_count = attempt.input_count;

                match attempt.result {
                    Ok(output) => {
                        let all = output.all_items();
                        record.status = NodeStatus::Success;
                        record.output_count = all.len();
                        record.output = preview_output(&output);
                        record.logs = output.logs.clone();
                        node_views.insert(node.name.clone(), NodeView { items: all });
                        succeeded.insert(index);
                        outputs.insert(index, output);
                    }
                    Err(error) => {
                        record.status = NodeStatus::Failed;
                        record.error = Some(error.message.clone());
                        record.error_details = error.details.clone();

                        match node.on_error {
                            crate::model::OnError::Continue => {
                                // Segue o fluxo com os itens que entraram.
                                warn!(
                                    node = %node.name,
                                    error = %error.message,
                                    "no falhou e o fluxo continuou por configuracao"
                                );
                                let passthrough = attempt.items.clone();
                                record.output_count = passthrough.len();
                                record.output = preview_main_port(&passthrough);
                                node_views.insert(
                                    node.name.clone(),
                                    NodeView {
                                        items: passthrough.clone(),
                                    },
                                );
                                succeeded.insert(index);
                                outputs.insert(index, NodeOutput::main(passthrough));
                            }
                            crate::model::OnError::Stop => {
                                run_status = RunStatus::Failed;
                                run_error = Some(format!(
                                    "no \"{}\" falhou: {}",
                                    node.name, error.message
                                ));
                                break 'levels;
                            }
                        }
                    }
                }
            }
        }

        let finished_at = Utc::now();
        let output = terminal_output(flow, &graph, &outputs, &succeeded);

        Ok(RunRecord {
            id: run_id,
            flow_id: flow.id.clone(),
            flow_name: flow.name.clone(),
            status: run_status,
            trigger: options.trigger,
            started_at,
            finished_at: Some(finished_at),
            duration_ms: (finished_at - started_at).num_milliseconds().max(0) as u64,
            nodes: runs,
            error: run_error,
            output,
        })
    }

    /// Executa um no com retentativas e timeout.
    #[allow(clippy::too_many_arguments)]
    async fn execute_node(
        &self,
        flow: &Flow,
        index: usize,
        items: Vec<Value>,
        node_views: &HashMap<String, NodeView>,
        options: &RunOptions,
        run_id: &str,
        budget: Duration,
    ) -> NodeAttempt {
        let node = &flow.nodes[index];
        let started_at = Utc::now();
        let input_count = items.len();
        let policy = node.retry_policy();
        let attempts_allowed = policy.attempts();

        // `validate` garantiu que o tipo existe antes de chegar aqui.
        let executor = self
            .registry
            .get(&node.kind)
            .expect("tipo de no validado antes da execucao")
            .clone();

        let mut attempts = 0u32;
        let mut last_error = NodeError::new("no nao executou");

        while attempts < attempts_allowed {
            attempts += 1;

            let ctx = NodeContext {
                node,
                flow_id: &flow.id,
                run_id,
                items: items.clone(),
                nodes: node_views,
                env: &options.env,
                trigger_payload: &options.payload,
                now: Utc::now(),
                host: self.host.clone(),
            };

            let outcome = if budget.is_zero() {
                Err(NodeError::new("tempo da execucao esgotado"))
            } else {
                match tokio::time::timeout(budget, executor.execute(&ctx)).await {
                    Ok(result) => result,
                    Err(_) => Err(NodeError::new(format!(
                        "o no passou do tempo limite de {}s da execucao",
                        budget.as_secs()
                    ))),
                }
            };

            match outcome {
                Ok(output) => {
                    return NodeAttempt {
                        index,
                        items,
                        input_count,
                        attempts,
                        started_at,
                        finished_at: Utc::now(),
                        result: Ok(output),
                    }
                }
                Err(error) => {
                    debug!(node = %node.name, attempt = attempts, error = %error.message, "tentativa falhou");
                    last_error = error;
                    if attempts < attempts_allowed && policy.delay_ms > 0 {
                        tokio::time::sleep(Duration::from_millis(policy.delay_ms)).await;
                    }
                }
            }
        }

        NodeAttempt {
            index,
            items,
            input_count,
            attempts,
            started_at,
            finished_at: Utc::now(),
            result: Err(last_error),
        }
    }

    /// Decide o no inicial da execucao.
    fn resolve_entry(
        &self,
        flow: &Flow,
        graph: &FlowGraph,
        options: &RunOptions,
    ) -> Result<usize, EngineError> {
        if let Some(requested) = options
            .start_node
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return graph
                .index_of(requested)
                .ok_or_else(|| EngineError::UnknownNode(requested.to_string()));
        }

        let wanted_kind = match options.trigger {
            TriggerSource::Webhook => Some("trigger.webhook"),
            TriggerSource::Schedule => Some("trigger.schedule"),
            TriggerSource::Manual | TriggerSource::Internal => None,
        };

        if let Some(kind) = wanted_kind {
            let found = flow
                .nodes
                .iter()
                .enumerate()
                .find(|(_, node)| !node.disabled && node.kind == kind)
                .map(|(index, _)| index);
            return found.ok_or_else(|| {
                EngineError::NoEntryNode(format!(
                    "o fluxo nao tem um no `{kind}` habilitado para receber este gatilho"
                ))
            });
        }

        // Manual: prefere um gatilho manual; senao, o primeiro no sem entradas.
        if let Some((index, _)) = flow
            .nodes
            .iter()
            .enumerate()
            .find(|(_, node)| !node.disabled && node.kind == "trigger.manual")
        {
            return Ok(index);
        }

        (0..graph.len())
            .find(|index| graph.incoming(*index).is_empty() && !flow.nodes[*index].disabled)
            .ok_or_else(|| {
                EngineError::NoEntryNode(
                    "o fluxo nao tem nenhum no inicial: todos os nos tem entradas".to_string(),
                )
            })
    }
}

/// Resultado bruto de uma tentativa de execucao de no.
struct NodeAttempt {
    index: usize,
    /// Itens de entrada, preservados para o modo `continue`.
    items: Vec<Value>,
    input_count: usize,
    attempts: u32,
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
    result: Result<NodeOutput, NodeError>,
}

/// Converte o payload do gatilho em itens.
fn payload_items(payload: &Value) -> Vec<Value> {
    match payload {
        Value::Null => vec![json!({})],
        Value::Array(items) if items.is_empty() => vec![json!({})],
        Value::Array(items) => items.clone(),
        other => vec![other.clone()],
    }
}

/// Junta os itens das arestas ativas que chegam em `index`.
/// `None` quando nenhuma aresta esta ativa: o no deve ser pulado.
fn collect_inputs(
    flow: &Flow,
    graph: &FlowGraph,
    index: usize,
    outputs: &HashMap<usize, NodeOutput>,
    succeeded: &HashSet<usize>,
) -> Option<Vec<Value>> {
    let mut items = Vec::new();
    let mut any_active = false;

    for &edge_index in graph.incoming(index) {
        let edge = &flow.edges[edge_index];
        let Some(source) = graph.index_of(&edge.from) else {
            continue;
        };
        if !succeeded.contains(&source) {
            continue;
        }
        let Some(output) = outputs.get(&source) else {
            continue;
        };
        // A porta precisa existir na saida: e o que corta o ramo do `if`.
        let Some(port_items) = output.ports.get(&edge.from_port) else {
            continue;
        };
        any_active = true;
        items.extend(port_items.iter().cloned());
    }

    if any_active {
        Some(items)
    } else {
        None
    }
}

/// Itens produzidos pelas folhas do fluxo.
fn terminal_output(
    flow: &Flow,
    graph: &FlowGraph,
    outputs: &HashMap<usize, NodeOutput>,
    succeeded: &HashSet<usize>,
) -> Vec<Value> {
    let mut leaves: Vec<Value> = Vec::new();
    for index in 0..graph.len() {
        if !succeeded.contains(&index) || !graph.outgoing(index).is_empty() {
            continue;
        }
        if let Some(output) = outputs.get(&index) {
            leaves.extend(output.all_items());
        }
    }
    if !leaves.is_empty() {
        return leaves;
    }

    // Sem folhas bem-sucedidas (ex.: o fluxo parou no meio): devolve a saida do
    // ultimo no que rodou, seguindo a ordem topologica.
    graph
        .levels()
        .iter()
        .flatten()
        .rev()
        .find(|index| succeeded.contains(index))
        .and_then(|index| outputs.get(index))
        .map(NodeOutput::all_items)
        .unwrap_or_default()
        .into_iter()
        .inspect(|_| debug!(flow = %flow.name, "saida final veio de um no intermediario"))
        .collect()
}

/// Amostra da saida por porta para o historico.
fn preview_output(output: &NodeOutput) -> Value {
    let mut ports = Map::new();
    for (port, items) in &output.ports {
        ports.insert(port.clone(), preview_items(items));
    }
    Value::Object(ports)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::UnavailableHost;
    use crate::model::{Edge, Node, OnError, RetryPolicy};
    use crate::nodes;
    use crate::registry::{NodeDescriptor, NodeExecutor};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// No de teste que conta execucoes e pode falhar as N primeiras vezes.
    struct Flaky {
        fail_times: usize,
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl NodeExecutor for Flaky {
        fn descriptor(&self) -> NodeDescriptor {
            NodeDescriptor {
                kind: "test.flaky".to_string(),
                label: "Flaky".to_string(),
                group: "Testes".to_string(),
                description: String::new(),
                color: "#000".to_string(),
                glyph: "F".to_string(),
                inputs: vec!["main".to_string()],
                outputs: vec!["main".to_string()],
                defaults: json!({}),
                fields: Vec::new(),
            }
        }

        async fn execute(&self, ctx: &NodeContext<'_>) -> Result<NodeOutput, NodeError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if call < self.fail_times {
                return Err(NodeError::new("falha proposital"));
            }
            Ok(NodeOutput::main(ctx.items_or_single_empty()))
        }
    }

    fn engine_with(extra: Option<Arc<dyn NodeExecutor>>) -> FlowEngine {
        let mut registry = nodes::builtin_registry();
        if let Some(executor) = extra {
            registry.register(executor);
        }
        FlowEngine::new(Arc::new(registry), Arc::new(UnavailableHost))
    }

    fn node(id: &str, kind: &str, params: Value) -> Node {
        let mut node = Node::new(id, id, kind);
        node.parameters = params;
        node
    }

    #[tokio::test]
    async fn linear_flow_passes_data_forward() {
        let mut flow = Flow::new("Linear");
        flow.id = "f1".to_string();
        flow.nodes.push(node("start", "trigger.manual", json!({})));
        flow.nodes.push(node(
            "set",
            "data.set",
            json!({
                "keep_only_set": true,
                "assignments": [{ "name": "dobro", "value": "{{ $json.n * 2 }}" }]
            }),
        ));
        flow.edges.push(Edge::new("start", "set"));

        let record = engine_with(None)
            .run(&flow, RunOptions::manual(json!({ "n": 21 })))
            .await
            .unwrap();

        assert_eq!(record.status, RunStatus::Success);
        assert_eq!(record.output.len(), 1);
        assert_eq!(record.output[0]["dobro"], json!(42));
    }

    #[tokio::test]
    async fn if_node_prunes_the_branch_not_taken() {
        let mut flow = Flow::new("Condicional");
        flow.nodes.push(node("start", "trigger.manual", json!({})));
        flow.nodes.push(node(
            "check",
            "flow.if",
            json!({ "condition": "{{ $json.n > 10 }}" }),
        ));
        flow.nodes.push(node(
            "alto",
            "data.set",
            json!({ "keep_only_set": true, "assignments": [{ "name": "via", "value": "alto" }] }),
        ));
        flow.nodes.push(node(
            "baixo",
            "data.set",
            json!({ "keep_only_set": true, "assignments": [{ "name": "via", "value": "baixo" }] }),
        ));
        flow.edges.push(Edge::new("start", "check"));
        flow.edges.push(Edge::from_port("check", "true", "alto"));
        flow.edges.push(Edge::from_port("check", "false", "baixo"));

        let engine = engine_with(None);

        let high = engine
            .run(&flow, RunOptions::manual(json!({ "n": 50 })))
            .await
            .unwrap();
        assert_eq!(high.node("alto").unwrap().status, NodeStatus::Success);
        assert_eq!(high.node("baixo").unwrap().status, NodeStatus::Skipped);
        assert_eq!(high.output[0]["via"], json!("alto"));

        let low = engine
            .run(&flow, RunOptions::manual(json!({ "n": 1 })))
            .await
            .unwrap();
        assert_eq!(low.node("alto").unwrap().status, NodeStatus::Skipped);
        assert_eq!(low.node("baixo").unwrap().status, NodeStatus::Success);
        assert_eq!(low.output[0]["via"], json!("baixo"));
    }

    #[tokio::test]
    async fn parallel_branches_merge_into_one_node() {
        let mut flow = Flow::new("Paralelo");
        flow.nodes.push(node("start", "trigger.manual", json!({})));
        flow.nodes.push(node(
            "a",
            "data.set",
            json!({ "keep_only_set": true, "assignments": [{ "name": "via", "value": "a" }] }),
        ));
        flow.nodes.push(node(
            "b",
            "data.set",
            json!({ "keep_only_set": true, "assignments": [{ "name": "via", "value": "b" }] }),
        ));
        flow.nodes.push(node("merge", "flow.merge", json!({})));
        flow.edges.push(Edge::new("start", "a"));
        flow.edges.push(Edge::new("start", "b"));
        flow.edges.push(Edge::new("a", "merge"));
        flow.edges.push(Edge::new("b", "merge"));

        let record = engine_with(None)
            .run(&flow, RunOptions::manual(json!({})))
            .await
            .unwrap();

        assert_eq!(record.status, RunStatus::Success);
        assert_eq!(record.output.len(), 2);
        let vias: HashSet<String> = record
            .output
            .iter()
            .map(|item| item["via"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(vias, HashSet::from(["a".to_string(), "b".to_string()]));
    }

    #[tokio::test]
    async fn node_output_is_visible_by_name_downstream() {
        let mut flow = Flow::new("Referencia");
        flow.nodes.push(node("start", "trigger.manual", json!({})));
        let mut origem = node(
            "origem",
            "data.set",
            json!({ "keep_only_set": true, "assignments": [{ "name": "valor", "value": 7 }] }),
        );
        origem.name = "Origem".to_string();
        flow.nodes.push(origem);
        flow.nodes.push(node(
            "destino",
            "data.set",
            json!({
                "keep_only_set": true,
                "assignments": [{ "name": "copiado", "value": "{{ $node[\"Origem\"].json.valor }}" }]
            }),
        ));
        flow.edges.push(Edge::new("start", "origem"));
        flow.edges.push(Edge::new("origem", "destino"));

        let record = engine_with(None)
            .run(&flow, RunOptions::manual(json!({})))
            .await
            .unwrap();

        assert_eq!(record.output[0]["copiado"], json!(7));
    }

    #[tokio::test]
    async fn failure_stops_the_run_by_default() {
        let calls = Arc::new(AtomicUsize::new(0));
        let engine = engine_with(Some(Arc::new(Flaky {
            fail_times: 99,
            calls: calls.clone(),
        })));

        let mut flow = Flow::new("Falha");
        flow.nodes.push(node("start", "trigger.manual", json!({})));
        flow.nodes.push(node("boom", "test.flaky", json!({})));
        flow.nodes.push(node("depois", "debug.log", json!({})));
        flow.edges.push(Edge::new("start", "boom"));
        flow.edges.push(Edge::new("boom", "depois"));

        let record = engine.run(&flow, RunOptions::default()).await.unwrap();

        assert_eq!(record.status, RunStatus::Failed);
        assert_eq!(record.node("boom").unwrap().status, NodeStatus::Failed);
        assert_eq!(record.node("depois").unwrap().status, NodeStatus::Pending);
        assert!(record.error.unwrap().contains("boom"));
    }

    #[tokio::test]
    async fn on_error_continue_keeps_the_flow_going() {
        let calls = Arc::new(AtomicUsize::new(0));
        let engine = engine_with(Some(Arc::new(Flaky {
            fail_times: 99,
            calls: calls.clone(),
        })));

        let mut flow = Flow::new("Continua");
        flow.nodes.push(node("start", "trigger.manual", json!({})));
        let mut boom = node("boom", "test.flaky", json!({}));
        boom.on_error = OnError::Continue;
        flow.nodes.push(boom);
        flow.nodes.push(node("depois", "debug.log", json!({})));
        flow.edges.push(Edge::new("start", "boom"));
        flow.edges.push(Edge::new("boom", "depois"));

        let record = engine.run(&flow, RunOptions::default()).await.unwrap();

        assert_eq!(record.status, RunStatus::Success);
        assert_eq!(record.node("boom").unwrap().status, NodeStatus::Failed);
        assert_eq!(record.node("depois").unwrap().status, NodeStatus::Success);
    }

    #[tokio::test]
    async fn retry_policy_reexecutes_until_success() {
        let calls = Arc::new(AtomicUsize::new(0));
        let engine = engine_with(Some(Arc::new(Flaky {
            fail_times: 2,
            calls: calls.clone(),
        })));

        let mut flow = Flow::new("Retry");
        flow.nodes.push(node("start", "trigger.manual", json!({})));
        let mut flaky = node("flaky", "test.flaky", json!({}));
        flaky.retry = Some(RetryPolicy {
            max_attempts: 3,
            delay_ms: 0,
        });
        flow.nodes.push(flaky);
        flow.edges.push(Edge::new("start", "flaky"));

        let record = engine.run(&flow, RunOptions::default()).await.unwrap();

        assert_eq!(record.status, RunStatus::Success);
        assert_eq!(record.node("flaky").unwrap().attempts, 3);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn disabled_node_passes_input_through() {
        let mut flow = Flow::new("Desligado");
        flow.nodes.push(node("start", "trigger.manual", json!({})));
        let mut disabled = node(
            "pulado",
            "data.set",
            json!({ "keep_only_set": true, "assignments": [{ "name": "x", "value": 1 }] }),
        );
        disabled.disabled = true;
        flow.nodes.push(disabled);
        flow.nodes.push(node("fim", "debug.log", json!({})));
        flow.edges.push(Edge::new("start", "pulado"));
        flow.edges.push(Edge::new("pulado", "fim"));

        let record = engine_with(None)
            .run(&flow, RunOptions::manual(json!({ "original": true })))
            .await
            .unwrap();

        assert_eq!(record.node("pulado").unwrap().status, NodeStatus::Disabled);
        // O `data.set` desligado nao aplicou nada: o item original passou.
        assert_eq!(record.output[0]["original"], json!(true));
    }

    #[tokio::test]
    async fn every_node_reports_its_output_indexed_by_port() {
        // O painel de execucao e o contador de itens nas arestas leem sempre
        // `output[porta]`; um no desativado ou que falhou e seguiu tem de usar
        // a mesma forma, senao a UI mostra a aresta sem itens.
        let calls = Arc::new(AtomicUsize::new(0));
        let engine = engine_with(Some(Arc::new(Flaky {
            fail_times: 99,
            calls,
        })));

        let mut flow = Flow::new("Formas de saida");
        flow.nodes.push(node("start", "trigger.manual", json!({})));
        let mut disabled = node("off", "debug.log", json!({}));
        disabled.disabled = true;
        flow.nodes.push(disabled);
        let mut tolerant = node("boom", "test.flaky", json!({}));
        tolerant.on_error = OnError::Continue;
        flow.nodes.push(tolerant);
        flow.edges.push(Edge::new("start", "off"));
        flow.edges.push(Edge::new("off", "boom"));

        let record = engine
            .run(&flow, RunOptions::manual(json!({ "n": 1 })))
            .await
            .unwrap();

        for id in ["start", "off", "boom"] {
            let node_run = record.node(id).unwrap();
            assert_eq!(
                node_run.output["main"]["total"],
                json!(1),
                "no `{id}` ({:?}) nao reportou itens na porta main: {}",
                node_run.status,
                node_run.output
            );
        }

        // O detalhe do erro sai de `output` e vai para o campo proprio.
        let failed = record.node("boom").unwrap();
        assert_eq!(failed.status, NodeStatus::Failed);
        assert!(failed.error.is_some());
        assert!(failed.output.get("error").is_none());
    }

    #[tokio::test]
    async fn a_reserved_run_id_is_used_instead_of_a_new_one() {
        let mut flow = Flow::new("Id reservado");
        flow.nodes.push(node("start", "trigger.manual", json!({})));

        let record = engine_with(None)
            .run(
                &flow,
                RunOptions {
                    run_id: Some("reservado-123".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        assert_eq!(record.id, "reservado-123");
    }

    #[tokio::test]
    async fn a_blank_reserved_run_id_falls_back_to_a_generated_one() {
        let mut flow = Flow::new("Id vazio");
        flow.nodes.push(node("start", "trigger.manual", json!({})));

        let record = engine_with(None)
            .run(
                &flow,
                RunOptions {
                    run_id: Some("   ".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        assert!(!record.id.trim().is_empty());
    }

    #[tokio::test]
    async fn invalid_flow_is_rejected_before_running() {
        let mut flow = Flow::new("Ciclico");
        flow.nodes.push(node("a", "trigger.manual", json!({})));
        flow.nodes.push(node("b", "debug.log", json!({})));
        flow.edges.push(Edge::new("a", "b"));
        flow.edges.push(Edge::new("b", "a"));

        let error = engine_with(None)
            .run(&flow, RunOptions::default())
            .await
            .unwrap_err();
        assert!(matches!(error, EngineError::Invalid(_)));
    }

    #[tokio::test]
    async fn webhook_trigger_requires_a_webhook_node() {
        let mut flow = Flow::new("Sem webhook");
        flow.nodes.push(node("start", "trigger.manual", json!({})));

        let error = engine_with(None)
            .run(
                &flow,
                RunOptions {
                    trigger: TriggerSource::Webhook,
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(error, EngineError::NoEntryNode(_)));
    }

    #[tokio::test]
    async fn array_payload_becomes_multiple_items() {
        let mut flow = Flow::new("Itens");
        flow.nodes.push(node("start", "trigger.manual", json!({})));
        flow.nodes.push(node(
            "set",
            "data.set",
            json!({ "keep_only_set": true, "assignments": [{ "name": "id", "value": "{{ $json.id }}" }] }),
        ));
        flow.edges.push(Edge::new("start", "set"));

        let record = engine_with(None)
            .run(
                &flow,
                RunOptions::manual(json!([{ "id": 1 }, { "id": 2 }, { "id": 3 }])),
            )
            .await
            .unwrap();

        assert_eq!(record.output.len(), 3);
        assert_eq!(record.output[2]["id"], json!(3));
    }

    #[tokio::test]
    async fn start_node_override_runs_a_subgraph() {
        let mut flow = Flow::new("Subgrafo");
        flow.nodes.push(node("start", "trigger.manual", json!({})));
        flow.nodes.push(node("meio", "debug.log", json!({})));
        flow.nodes.push(node("fim", "debug.log", json!({})));
        flow.edges.push(Edge::new("start", "meio"));
        flow.edges.push(Edge::new("meio", "fim"));

        let record = engine_with(None)
            .run(
                &flow,
                RunOptions {
                    start_node: Some("meio".to_string()),
                    payload: json!({ "direto": true }),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        assert_eq!(record.node("start").unwrap().status, NodeStatus::Skipped);
        assert_eq!(record.node("meio").unwrap().status, NodeStatus::Success);
        assert_eq!(record.node("fim").unwrap().status, NodeStatus::Success);
    }
}
