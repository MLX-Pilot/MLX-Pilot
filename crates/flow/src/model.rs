//! Modelo de dados do formato `mlxflow.v1`.
//!
//! Um fluxo e um grafo dirigido: `nodes` sao as unidades de execucao e `edges`
//! descrevem o caminho dos dados. Diferente do formato do n8n, as conexoes sao
//! uma lista plana de arestas (e nao um mapa aninhado indexado por nome), o que
//! torna a validacao do DAG direta e evita que renomear um no quebre o grafo.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// Identificador da versao do schema persistido.
pub const FLOW_SCHEMA: &str = "mlxflow.v1";

/// Porta de saida padrao de um no.
pub const MAIN_PORT: &str = "main";

fn default_schema() -> String {
    FLOW_SCHEMA.to_string()
}

fn default_port() -> String {
    MAIN_PORT.to_string()
}

fn is_main_port(port: &str) -> bool {
    port == MAIN_PORT
}

/// Posicao do no no canvas do editor. Puramente visual.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

impl Default for Position {
    fn default() -> Self {
        Self { x: 240.0, y: 200.0 }
    }
}

/// O que fazer quando um no falha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnError {
    /// Aborta a execucao inteira (padrao).
    #[default]
    Stop,
    /// Marca o no como falho e segue o fluxo com os itens de entrada.
    Continue,
}

/// Politica de retentativa por no.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Total de tentativas, incluindo a primeira. Minimo efetivo 1.
    pub max_attempts: u32,
    /// Espera entre tentativas.
    #[serde(default)]
    pub delay_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 1,
            delay_ms: 0,
        }
    }
}

impl RetryPolicy {
    /// Numero de tentativas saneado (sempre >= 1, teto defensivo em 10).
    pub fn attempts(&self) -> u32 {
        self.max_attempts.clamp(1, 10)
    }
}

/// Unidade de execucao do fluxo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    /// Nome exibido no editor. Unico dentro do fluxo: e a chave usada por
    /// expressoes `$node["Nome"]`.
    pub name: String,
    /// Tipo registrado no `NodeRegistry`, ex.: `http.request`.
    pub kind: String,
    #[serde(default = "empty_object")]
    pub parameters: Value,
    #[serde(default)]
    pub position: Position,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub on_error: OnError,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<RetryPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

fn empty_object() -> Value {
    json!({})
}

impl Node {
    /// Cria um no minimo com parametros vazios.
    pub fn new(id: impl Into<String>, name: impl Into<String>, kind: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: kind.into(),
            parameters: empty_object(),
            position: Position::default(),
            disabled: false,
            on_error: OnError::default(),
            retry: None,
            notes: None,
        }
    }

    /// Parametros como objeto. Retorna um mapa vazio se o valor nao for objeto.
    pub fn parameters_object(&self) -> Map<String, Value> {
        self.parameters
            .as_object()
            .cloned()
            .unwrap_or_else(Map::new)
    }

    /// Politica de retentativa efetiva.
    pub fn retry_policy(&self) -> RetryPolicy {
        self.retry.clone().unwrap_or_default()
    }
}

/// Aresta dirigida entre duas portas de nos.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    #[serde(default)]
    pub id: String,
    /// Id do no de origem.
    pub from: String,
    /// Porta de saida da origem. `main` quando omitido.
    #[serde(default = "default_port", skip_serializing_if = "is_main_port")]
    pub from_port: String,
    /// Id do no de destino.
    pub to: String,
    /// Porta de entrada do destino. `main` quando omitido.
    #[serde(default = "default_port", skip_serializing_if = "is_main_port")]
    pub to_port: String,
}

impl Edge {
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            id: String::new(),
            from: from.into(),
            from_port: default_port(),
            to: to.into(),
            to_port: default_port(),
        }
    }

    /// Aresta a partir de uma porta nomeada (ex.: saida `true` de um `flow.if`).
    pub fn from_port(
        from: impl Into<String>,
        from_port: impl Into<String>,
        to: impl Into<String>,
    ) -> Self {
        Self {
            id: String::new(),
            from: from.into(),
            from_port: from_port.into(),
            to: to.into(),
            to_port: default_port(),
        }
    }
}

/// Configuracoes de execucao do fluxo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowSettings {
    /// Timeout total da execucao.
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    /// Quantos nos podem rodar em paralelo dentro de um mesmo nivel topologico.
    #[serde(default = "default_max_parallel")]
    pub max_parallel: usize,
    /// Fuso usado por `$now` em expressoes. Informativo; `$now` e sempre UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    /// Persistir o historico de execucoes em disco.
    #[serde(default = "default_true")]
    pub save_runs: bool,
}

fn default_timeout_secs() -> u64 {
    300
}

fn default_max_parallel() -> usize {
    8
}

fn default_true() -> bool {
    true
}

impl Default for FlowSettings {
    fn default() -> Self {
        Self {
            timeout_secs: default_timeout_secs(),
            max_parallel: default_max_parallel(),
            timezone: None,
            save_runs: true,
        }
    }
}

impl FlowSettings {
    /// Timeout saneado: nunca zero, teto de 1 hora.
    pub fn effective_timeout_secs(&self) -> u64 {
        self.timeout_secs.clamp(1, 3600)
    }

    /// Paralelismo saneado: nunca zero, teto de 32.
    pub fn effective_max_parallel(&self) -> usize {
        self.max_parallel.clamp(1, 32)
    }
}

/// Definicao completa de um fluxo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Flow {
    #[serde(default = "default_schema")]
    pub schema: String,
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Habilita gatilhos automaticos (webhook e agenda). Execucao manual
    /// funciona mesmo com o fluxo inativo.
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    #[serde(default)]
    pub settings: FlowSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

impl Flow {
    /// Fluxo vazio com um nome.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            schema: default_schema(),
            id: String::new(),
            name: name.into(),
            description: None,
            active: false,
            nodes: Vec::new(),
            edges: Vec::new(),
            settings: FlowSettings::default(),
            created_at: None,
            updated_at: None,
        }
    }

    /// Busca um no pelo id.
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|node| node.id == id)
    }

    /// Busca um no pelo nome exibido.
    pub fn node_by_name(&self, name: &str) -> Option<&Node> {
        self.nodes.iter().find(|node| node.name == name)
    }

    /// Nos de gatilho (`trigger.*`) ainda habilitados.
    pub fn triggers(&self) -> Vec<&Node> {
        self.nodes
            .iter()
            .filter(|node| !node.disabled && node.kind.starts_with("trigger."))
            .collect()
    }

    /// Resumo usado nas listagens da UI.
    pub fn summary(&self) -> FlowSummary {
        FlowSummary {
            id: self.id.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            active: self.active,
            node_count: self.nodes.len(),
            edge_count: self.edges.len(),
            trigger_kinds: self
                .triggers()
                .iter()
                .map(|node| node.kind.clone())
                .collect(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// Projecao leve de um fluxo para listagens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowSummary {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub active: bool,
    pub node_count: usize,
    pub edge_count: usize,
    pub trigger_kinds: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_defaults_to_main_port_and_omits_it_when_serializing() {
        let edge: Edge = serde_json::from_value(json!({ "from": "a", "to": "b" })).unwrap();
        assert_eq!(edge.from_port, MAIN_PORT);
        assert_eq!(edge.to_port, MAIN_PORT);

        let serialized = serde_json::to_value(&edge).unwrap();
        assert!(serialized.get("from_port").is_none());
        assert!(serialized.get("to_port").is_none());
    }

    #[test]
    fn named_port_survives_round_trip() {
        let edge = Edge::from_port("if-node", "false", "log-node");
        let round_trip: Edge = serde_json::from_value(serde_json::to_value(&edge).unwrap()).unwrap();
        assert_eq!(round_trip.from_port, "false");
        assert_eq!(round_trip.to_port, MAIN_PORT);
    }

    #[test]
    fn flow_deserializes_from_minimal_json() {
        let flow: Flow = serde_json::from_value(json!({
            "name": "Minimo",
            "nodes": [{ "id": "n1", "name": "Start", "kind": "trigger.manual" }]
        }))
        .unwrap();

        assert_eq!(flow.schema, FLOW_SCHEMA);
        assert_eq!(flow.settings.timeout_secs, 300);
        assert!(!flow.nodes[0].disabled);
        assert_eq!(flow.triggers().len(), 1);
    }

    #[test]
    fn disabled_trigger_is_not_reported() {
        let mut flow = Flow::new("F");
        let mut trigger = Node::new("n1", "Start", "trigger.manual");
        trigger.disabled = true;
        flow.nodes.push(trigger);
        assert!(flow.triggers().is_empty());
    }

    #[test]
    fn settings_are_clamped_to_safe_bounds() {
        let settings = FlowSettings {
            timeout_secs: 0,
            max_parallel: 0,
            timezone: None,
            save_runs: true,
        };
        assert_eq!(settings.effective_timeout_secs(), 1);
        assert_eq!(settings.effective_max_parallel(), 1);

        let huge = FlowSettings {
            timeout_secs: 99_999,
            max_parallel: 9_999,
            timezone: None,
            save_runs: true,
        };
        assert_eq!(huge.effective_timeout_secs(), 3600);
        assert_eq!(huge.effective_max_parallel(), 32);
    }

    #[test]
    fn retry_attempts_are_clamped() {
        assert_eq!(RetryPolicy::default().attempts(), 1);
        assert_eq!(
            RetryPolicy {
                max_attempts: 0,
                delay_ms: 0
            }
            .attempts(),
            1
        );
        assert_eq!(
            RetryPolicy {
                max_attempts: 500,
                delay_ms: 0
            }
            .attempts(),
            10
        );
    }
}
