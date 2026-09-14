//! Contrato de um no executavel e o registro de tipos.
//!
//! Cada tipo de no expoe um `NodeDescriptor` que descreve rotulo, portas,
//! parametros padrao e os campos do formulario. A UI monta a paleta e o
//! inspetor a partir desse catalogo, entao adicionar um no novo nao exige
//! mexer no JavaScript.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::expr::{self, ExprScope, NodeView};
use crate::host::FlowHost;
use crate::model::{Node, MAIN_PORT};

/// Tipo de campo do inspetor, para a UI escolher o controle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    Text,
    Textarea,
    Number,
    Boolean,
    Select,
    Json,
    Expression,
}

/// Opcao de um campo `select`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

impl SelectOption {
    pub fn new(value: &str, label: &str) -> Self {
        Self {
            value: value.to_string(),
            label: label.to_string(),
        }
    }
}

/// Origem das opcoes de um campo `select` que o daemon nao consegue preencher
/// sozinho, porque dependem do que o usuario configurou no resto do app.
///
/// O catalogo publica so o nome da fonte; quem resolve e a UI, que ja mantem a
/// lista de provedores, modelos e ferramentas para as outras abas. E isso que
/// faz um no `agent.run` nascer apontando para o mesmo provedor e modelo que
/// estao selecionados no MLX Pilot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptionsSource {
    /// Provedores de modelo configurados e disponiveis.
    AgentProviders,
    /// Modelos do provedor escolhido no proprio no.
    AgentModels,
    /// Ferramentas registradas para o no `tool.call`.
    FlowTools,
}

/// Descricao de um parametro editavel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldSpec {
    pub key: String,
    pub label: String,
    pub kind: FieldKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<SelectOption>,
    /// Preenche `options` em tempo de renderizacao.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options_source: Option<OptionsSource>,
    #[serde(default)]
    pub required: bool,
    /// Campo que fica recolhido em "Avancado": tem um padrao herdado do app e
    /// so precisa aparecer quando alguem quiser sobrescrever.
    #[serde(default)]
    pub advanced: bool,
}

impl FieldSpec {
    pub fn new(key: &str, label: &str, kind: FieldKind) -> Self {
        Self {
            key: key.to_string(),
            label: label.to_string(),
            kind,
            placeholder: None,
            help: None,
            options: Vec::new(),
            options_source: None,
            required: false,
            advanced: false,
        }
    }

    /// Campo `select` cujas opcoes a UI resolve.
    pub fn dynamic(key: &str, label: &str, source: OptionsSource) -> Self {
        let mut field = Self::new(key, label, FieldKind::Select);
        field.options_source = Some(source);
        field
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Move o campo para a secao recolhida de avancado.
    pub fn advanced(mut self) -> Self {
        self.advanced = true;
        self
    }

    pub fn placeholder(mut self, text: &str) -> Self {
        self.placeholder = Some(text.to_string());
        self
    }

    pub fn help(mut self, text: &str) -> Self {
        self.help = Some(text.to_string());
        self
    }

    pub fn options(mut self, options: Vec<SelectOption>) -> Self {
        self.options = options;
        self
    }
}

/// Metadados de um tipo de no.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeDescriptor {
    pub kind: String,
    pub label: String,
    pub group: String,
    pub description: String,
    pub color: String,
    pub glyph: String,
    /// Portas de entrada. Vazio para gatilhos.
    pub inputs: Vec<String>,
    /// Portas de saida.
    pub outputs: Vec<String>,
    pub defaults: Value,
    pub fields: Vec<FieldSpec>,
}

impl NodeDescriptor {
    /// Verdadeiro para nos sem entrada, ou seja, gatilhos.
    pub fn is_trigger(&self) -> bool {
        self.inputs.is_empty()
    }
}

/// Falha na execucao de um no.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeError {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl NodeError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(message: impl Into<String>, details: Value) -> Self {
        Self {
            message: message.into(),
            details: Some(details),
        }
    }
}

impl std::fmt::Display for NodeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl From<expr::ExprError> for NodeError {
    fn from(error: expr::ExprError) -> Self {
        Self {
            message: error.to_string(),
            details: Some(json!({ "expression": error.expression })),
        }
    }
}

/// Itens produzidos por um no, separados por porta de saida.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NodeOutput {
    pub ports: BTreeMap<String, Vec<Value>>,
    /// Linhas de log exibidas no painel de execucao.
    pub logs: Vec<String>,
}

impl NodeOutput {
    /// Saida na porta padrao.
    pub fn main(items: Vec<Value>) -> Self {
        let mut ports = BTreeMap::new();
        ports.insert(MAIN_PORT.to_string(), items);
        Self {
            ports,
            logs: Vec::new(),
        }
    }

    /// Saida em uma porta nomeada. Portas ausentes interrompem aquele ramo.
    pub fn port(name: &str, items: Vec<Value>) -> Self {
        let mut ports = BTreeMap::new();
        ports.insert(name.to_string(), items);
        Self {
            ports,
            logs: Vec::new(),
        }
    }

    /// Saida sem nenhuma porta: o ramo para aqui.
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn with_log(mut self, line: impl Into<String>) -> Self {
        self.logs.push(line.into());
        self
    }

    /// Itens da porta padrao.
    pub fn main_items(&self) -> &[Value] {
        self.ports
            .get(MAIN_PORT)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Todos os itens de todas as portas, para o registro da execucao.
    pub fn all_items(&self) -> Vec<Value> {
        self.ports.values().flatten().cloned().collect()
    }
}

/// Tudo que um no enxerga durante a execucao.
pub struct NodeContext<'a> {
    pub node: &'a Node,
    pub flow_id: &'a str,
    pub run_id: &'a str,
    /// Itens que chegaram das arestas de entrada, ja concatenados.
    pub items: Vec<Value>,
    /// Saidas dos nos ja executados, por nome.
    pub nodes: &'a HashMap<String, NodeView>,
    /// Variaveis da execucao.
    pub env: &'a Map<String, Value>,
    /// Payload que iniciou a execucao.
    pub trigger_payload: &'a Value,
    pub now: DateTime<Utc>,
    pub host: Arc<dyn FlowHost>,
}

impl<'a> NodeContext<'a> {
    /// Escopo de expressao para o item em `index`.
    pub fn scope(&self, index: usize) -> ExprScope<'_> {
        ExprScope {
            json: self.items.get(index).unwrap_or(&Value::Null),
            items: &self.items,
            index,
            nodes: self.nodes,
            env: self.env,
            now: self.now,
            run_id: self.run_id,
            flow_id: self.flow_id,
            node_name: &self.node.name,
        }
    }

    /// Parametros do no resolvidos contra o item em `index`.
    pub fn params(&self, index: usize) -> Result<Value, NodeError> {
        let scope = self.scope(index);
        expr::render_value(&self.node.parameters, &scope).map_err(NodeError::from)
    }

    /// Parametros resolvidos contra o primeiro item. Para nos que nao iteram.
    pub fn params_once(&self) -> Result<Value, NodeError> {
        self.params(0)
    }

    /// Itens de entrada, garantindo pelo menos um item vazio para que nos sem
    /// entrada ainda executem uma vez.
    pub fn items_or_single_empty(&self) -> Vec<Value> {
        if self.items.is_empty() {
            vec![json!({})]
        } else {
            self.items.clone()
        }
    }
}

/// Um tipo de no executavel.
#[async_trait]
pub trait NodeExecutor: Send + Sync {
    fn descriptor(&self) -> NodeDescriptor;
    async fn execute(&self, ctx: &NodeContext<'_>) -> Result<NodeOutput, NodeError>;
}

/// Registro de tipos de no disponiveis para o motor.
#[derive(Clone, Default)]
pub struct NodeRegistry {
    executors: BTreeMap<String, Arc<dyn NodeExecutor>>,
}

impl std::fmt::Debug for NodeRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NodeRegistry")
            .field("kinds", &self.kinds())
            .finish()
    }
}

impl NodeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra (ou substitui) um tipo de no.
    pub fn register(&mut self, executor: Arc<dyn NodeExecutor>) {
        let kind = executor.descriptor().kind;
        self.executors.insert(kind, executor);
    }

    pub fn get(&self, kind: &str) -> Option<&Arc<dyn NodeExecutor>> {
        self.executors.get(kind)
    }

    pub fn contains(&self, kind: &str) -> bool {
        self.executors.contains_key(kind)
    }

    pub fn kinds(&self) -> Vec<String> {
        self.executors.keys().cloned().collect()
    }

    /// Catalogo completo, ordenado por tipo, para a paleta da UI.
    pub fn catalog(&self) -> Vec<NodeDescriptor> {
        self.executors
            .values()
            .map(|executor| executor.descriptor())
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.executors.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dummy;

    #[async_trait]
    impl NodeExecutor for Dummy {
        fn descriptor(&self) -> NodeDescriptor {
            NodeDescriptor {
                kind: "test.dummy".to_string(),
                label: "Dummy".to_string(),
                group: "Testes".to_string(),
                description: String::new(),
                color: "#fff".to_string(),
                glyph: "D".to_string(),
                inputs: vec![MAIN_PORT.to_string()],
                outputs: vec![MAIN_PORT.to_string()],
                defaults: json!({}),
                fields: Vec::new(),
            }
        }

        async fn execute(&self, _ctx: &NodeContext<'_>) -> Result<NodeOutput, NodeError> {
            Ok(NodeOutput::main(vec![json!({})]))
        }
    }

    #[test]
    fn registry_indexes_by_descriptor_kind() {
        let mut registry = NodeRegistry::new();
        registry.register(Arc::new(Dummy));
        assert!(registry.contains("test.dummy"));
        assert_eq!(registry.kinds(), vec!["test.dummy".to_string()]);
        assert_eq!(registry.catalog().len(), 1);
    }

    #[test]
    fn output_without_port_stops_the_branch() {
        let output = NodeOutput::empty();
        assert!(output.main_items().is_empty());
        assert!(output.ports.is_empty());
    }

    #[test]
    fn named_port_output_does_not_populate_main() {
        let output = NodeOutput::port("false", vec![json!({ "a": 1 })]);
        assert!(output.main_items().is_empty());
        assert_eq!(output.ports["false"].len(), 1);
        assert_eq!(output.all_items().len(), 1);
    }

    #[test]
    fn descriptor_without_inputs_is_a_trigger() {
        let mut descriptor = Dummy.descriptor();
        assert!(!descriptor.is_trigger());
        descriptor.inputs.clear();
        assert!(descriptor.is_trigger());
    }
}
