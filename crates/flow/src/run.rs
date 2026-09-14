//! Registro de execucao: o que aconteceu em cada no e no fluxo como um todo.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Quantos itens de saida sao guardados por no no historico.
const MAX_PREVIEW_ITEMS: usize = 20;
/// Teto de caracteres por item guardado, para o historico nao virar um dump.
const MAX_PREVIEW_CHARS: usize = 8_000;

/// Situacao final de uma execucao.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Success,
    Failed,
    Cancelled,
    TimedOut,
}

/// Situacao de um no dentro de uma execucao.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    /// Nunca chegou a ser avaliado (execucao abortou antes).
    Pending,
    Success,
    Failed,
    /// Nenhuma aresta de entrada ficou ativa: o ramo nao passou por aqui.
    Skipped,
    /// Desligado no editor; repassa a entrada para a saida.
    Disabled,
}

/// Origem da execucao.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerSource {
    Manual,
    Webhook,
    Schedule,
    /// Disparada por outro componente do MLX Pilot.
    Internal,
}

impl TriggerSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Webhook => "webhook",
            Self::Schedule => "schedule",
            Self::Internal => "internal",
        }
    }
}

/// Resultado de um no.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeRun {
    pub node_id: String,
    pub node_name: String,
    pub kind: String,
    pub status: NodeStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    pub duration_ms: u64,
    pub attempts: u32,
    pub input_count: usize,
    pub output_count: usize,
    /// Amostra da saida por porta, truncada.
    pub output: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub logs: Vec<String>,
}

impl NodeRun {
    /// Registro inicial de um no que ainda nao rodou.
    pub fn pending(node_id: &str, node_name: &str, kind: &str) -> Self {
        Self {
            node_id: node_id.to_string(),
            node_name: node_name.to_string(),
            kind: kind.to_string(),
            status: NodeStatus::Pending,
            started_at: None,
            finished_at: None,
            duration_ms: 0,
            attempts: 0,
            input_count: 0,
            output_count: 0,
            output: json!({}),
            error: None,
            logs: Vec::new(),
        }
    }
}

/// Execucao completa de um fluxo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: String,
    pub flow_id: String,
    pub flow_name: String,
    pub status: RunStatus,
    pub trigger: TriggerSource,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    pub duration_ms: u64,
    pub nodes: Vec<NodeRun>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Itens produzidos pelos nos finais do fluxo.
    pub output: Vec<Value>,
}

impl RunRecord {
    /// Verdadeiro quando a execucao terminou bem.
    pub fn succeeded(&self) -> bool {
        self.status == RunStatus::Success
    }

    /// Registro de um no pelo id.
    pub fn node(&self, node_id: &str) -> Option<&NodeRun> {
        self.nodes.iter().find(|node| node.node_id == node_id)
    }

    /// Projecao leve para listagens de historico.
    pub fn summary(&self) -> RunSummary {
        RunSummary {
            id: self.id.clone(),
            flow_id: self.flow_id.clone(),
            flow_name: self.flow_name.clone(),
            status: self.status,
            trigger: self.trigger,
            started_at: self.started_at,
            finished_at: self.finished_at,
            duration_ms: self.duration_ms,
            node_count: self.nodes.len(),
            failed_node: self
                .nodes
                .iter()
                .find(|node| node.status == NodeStatus::Failed)
                .map(|node| node.node_name.clone()),
            error: self.error.clone(),
        }
    }
}

/// Linha do historico de execucoes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunSummary {
    pub id: String,
    pub flow_id: String,
    pub flow_name: String,
    pub status: RunStatus,
    pub trigger: TriggerSource,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    pub duration_ms: u64,
    pub node_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_node: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Reduz uma lista de itens ao que cabe no historico.
pub fn preview_items(items: &[Value]) -> Value {
    let truncated: Vec<Value> = items
        .iter()
        .take(MAX_PREVIEW_ITEMS)
        .map(preview_value)
        .collect();

    if items.len() > MAX_PREVIEW_ITEMS {
        json!({
            "items": truncated,
            "truncated": true,
            "total": items.len(),
        })
    } else {
        json!({ "items": truncated, "truncated": false, "total": items.len() })
    }
}

/// Corta um valor grande, preservando o tipo quando ele ja e pequeno.
fn preview_value(value: &Value) -> Value {
    let serialized = value.to_string();
    if serialized.len() <= MAX_PREVIEW_CHARS {
        return value.clone();
    }
    let mut cut = MAX_PREVIEW_CHARS;
    while cut > 0 && !serialized.is_char_boundary(cut) {
        cut -= 1;
    }
    json!({
        "_truncated": true,
        "_bytes": serialized.len(),
        "preview": &serialized[..cut],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_keeps_small_payloads_intact() {
        let items = vec![json!({ "a": 1 }), json!({ "a": 2 })];
        let preview = preview_items(&items);
        assert_eq!(preview["truncated"], json!(false));
        assert_eq!(preview["total"], json!(2));
        assert_eq!(preview["items"][1]["a"], json!(2));
    }

    #[test]
    fn preview_caps_item_count() {
        let items: Vec<Value> = (0..50).map(|n| json!({ "n": n })).collect();
        let preview = preview_items(&items);
        assert_eq!(preview["truncated"], json!(true));
        assert_eq!(preview["total"], json!(50));
        assert_eq!(preview["items"].as_array().unwrap().len(), MAX_PREVIEW_ITEMS);
    }

    #[test]
    fn preview_caps_huge_single_item() {
        let big = json!({ "text": "x".repeat(MAX_PREVIEW_CHARS * 2) });
        let preview = preview_items(&[big]);
        assert_eq!(preview["items"][0]["_truncated"], json!(true));
        assert!(preview["items"][0]["preview"]
            .as_str()
            .unwrap()
            .len()
            <= MAX_PREVIEW_CHARS);
    }

    #[test]
    fn summary_reports_the_failed_node() {
        let mut failed = NodeRun::pending("n2", "Chamar API", "http.request");
        failed.status = NodeStatus::Failed;
        let record = RunRecord {
            id: "r1".to_string(),
            flow_id: "f1".to_string(),
            flow_name: "F".to_string(),
            status: RunStatus::Failed,
            trigger: TriggerSource::Manual,
            started_at: Utc::now(),
            finished_at: None,
            duration_ms: 0,
            nodes: vec![NodeRun::pending("n1", "Start", "trigger.manual"), failed],
            error: Some("boom".to_string()),
            output: Vec::new(),
        };

        let summary = record.summary();
        assert_eq!(summary.failed_node.as_deref(), Some("Chamar API"));
        assert_eq!(summary.node_count, 2);
        assert!(!record.succeeded());
    }
}
