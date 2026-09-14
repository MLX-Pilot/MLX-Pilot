//! `mlx-flow` — motor de orquestracao de workflows embutido do MLX Pilot.
//!
//! Substitui a dependencia de uma instancia externa do n8n por um motor nativo
//! que roda dentro do proprio daemon. As pecas:
//!
//! - [`model`]: o formato `mlxflow.v1` (nos e arestas).
//! - [`graph`]: validacao e ordenacao topologica em camadas.
//! - [`expr`]: a linguagem de `{{ ... }}`, sem motor JavaScript.
//! - [`registry`]: o contrato de um no e o catalogo de tipos.
//! - [`engine`]: o escalonador assincrono.
//! - [`nodes`]: os nos embutidos, incluindo `agent.run` e `tool.call`.
//! - [`host`]: a ponte para o agente e as ferramentas do MLX Pilot.
//! - [`store`]: persistencia em disco de fluxos e execucoes.
//! - [`import`]: conversor offline de workflows exportados do n8n.
//!
//! ```no_run
//! use std::sync::Arc;
//! use mlx_flow::{engine::{FlowEngine, RunOptions}, host::UnavailableHost, nodes, model::Flow};
//!
//! # async fn exemplo(flow: Flow) -> anyhow::Result<()> {
//! let engine = FlowEngine::new(
//!     Arc::new(nodes::builtin_registry()),
//!     Arc::new(UnavailableHost),
//! );
//! let record = engine.run(&flow, RunOptions::manual(serde_json::json!({}))).await?;
//! println!("{:?}", record.status);
//! # Ok(())
//! # }
//! ```

pub mod engine;
pub mod expr;
pub mod graph;
pub mod host;
pub mod import;
pub mod model;
pub mod nodes;
pub mod registry;
pub mod run;
pub mod store;

use serde::{Deserialize, Serialize};

pub use engine::{EngineError, FlowEngine, RunOptions};
pub use graph::{FlowGraph, Severity, ValidationIssue, ValidationReport};
pub use host::{AgentNodeRequest, AgentNodeResult, FlowHost, ToolNodeRequest, ToolNodeResult};
pub use model::{Edge, Flow, FlowSettings, FlowSummary, Node, OnError, Position, RetryPolicy};
pub use registry::{NodeDescriptor, NodeError, NodeExecutor, NodeOutput, NodeRegistry};
pub use run::{NodeRun, NodeStatus, RunRecord, RunStatus, RunSummary, TriggerSource};
pub use store::FlowStore;

/// Um webhook publicado por um fluxo ativo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebhookBinding {
    pub flow_id: String,
    pub flow_name: String,
    pub node_id: String,
    pub path: String,
    pub method: String,
    /// `last_node` espera o fluxo terminar; `immediate` responde na hora.
    pub response_mode: String,
}

/// Uma agenda publicada por um fluxo ativo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduleBinding {
    pub flow_id: String,
    pub flow_name: String,
    pub node_id: String,
    pub cron: String,
}

/// Webhooks de todos os fluxos ativos.
///
/// Fluxos inativos sao ignorados de proposito: e o interruptor que decide se um
/// gatilho automatico esta publicado ou nao.
pub fn webhook_bindings(flows: &[Flow]) -> Vec<WebhookBinding> {
    let mut bindings = Vec::new();
    for flow in flows.iter().filter(|flow| flow.active) {
        for node in flow.triggers() {
            if node.kind != "trigger.webhook" {
                continue;
            }
            let Some((path, method)) = nodes::triggers::webhook_binding(&node.parameters) else {
                continue;
            };
            bindings.push(WebhookBinding {
                flow_id: flow.id.clone(),
                flow_name: flow.name.clone(),
                node_id: node.id.clone(),
                path,
                method,
                response_mode: node
                    .parameters
                    .get("response_mode")
                    .and_then(|value| value.as_str())
                    .unwrap_or("last_node")
                    .to_string(),
            });
        }
    }
    bindings
}

/// Agendas de todos os fluxos ativos.
pub fn schedule_bindings(flows: &[Flow]) -> Vec<ScheduleBinding> {
    let mut bindings = Vec::new();
    for flow in flows.iter().filter(|flow| flow.active) {
        for node in flow.triggers() {
            if node.kind != "trigger.schedule" {
                continue;
            }
            let Some(cron) = nodes::triggers::schedule_binding(&node.parameters) else {
                continue;
            };
            bindings.push(ScheduleBinding {
                flow_id: flow.id.clone(),
                flow_name: flow.name.clone(),
                node_id: node.id.clone(),
                cron,
            });
        }
    }
    bindings
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn flow_with_trigger(active: bool, kind: &str, parameters: serde_json::Value) -> Flow {
        let mut flow = Flow::new("Com gatilho");
        flow.id = "f1".to_string();
        flow.active = active;
        let mut node = Node::new("n1", "Gatilho", kind);
        node.parameters = parameters;
        flow.nodes.push(node);
        flow
    }

    #[test]
    fn only_active_flows_publish_webhooks() {
        let active = flow_with_trigger(true, "trigger.webhook", json!({ "path": "entrada" }));
        let inactive = flow_with_trigger(false, "trigger.webhook", json!({ "path": "outra" }));

        let bindings = webhook_bindings(&[active, inactive]);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].path, "entrada");
        assert_eq!(bindings[0].method, "POST");
        assert_eq!(bindings[0].response_mode, "last_node");
    }

    #[test]
    fn webhook_without_path_is_not_published() {
        let flow = flow_with_trigger(true, "trigger.webhook", json!({ "path": "" }));
        assert!(webhook_bindings(&[flow]).is_empty());
    }

    #[test]
    fn disabled_trigger_node_is_not_published() {
        let mut flow = flow_with_trigger(true, "trigger.webhook", json!({ "path": "entrada" }));
        flow.nodes[0].disabled = true;
        assert!(webhook_bindings(&[flow]).is_empty());
    }

    #[test]
    fn schedule_bindings_read_the_cron_field() {
        let flow = flow_with_trigger(true, "trigger.schedule", json!({ "cron": "0 0 * * * *" }));
        let bindings = schedule_bindings(&[flow]);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].cron, "0 0 * * * *");
        assert_eq!(bindings[0].flow_id, "f1");
    }

    #[test]
    fn response_mode_is_carried_over() {
        let flow = flow_with_trigger(
            true,
            "trigger.webhook",
            json!({ "path": "x", "response_mode": "immediate" }),
        );
        assert_eq!(webhook_bindings(&[flow])[0].response_mode, "immediate");
    }
}
