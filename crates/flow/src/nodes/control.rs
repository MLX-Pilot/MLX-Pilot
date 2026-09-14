//! Nos de controle de fluxo.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::expr::{self, truthy};
use crate::model::MAIN_PORT;
use crate::registry::{
    FieldKind, FieldSpec, NodeContext, NodeDescriptor, NodeError, NodeExecutor, NodeOutput,
    SelectOption,
};

use super::{param_str, param_str_or};

/// Porta de saida para itens que satisfazem a condicao.
pub const TRUE_PORT: &str = "true";
/// Porta de saida para os demais.
pub const FALSE_PORT: &str = "false";

/// Divide os itens entre duas saidas conforme uma condicao.
///
/// Uma porta so aparece na saida se recebeu pelo menos um item. E isso que faz
/// o `FlowEngine` podar o ramo nao escolhido.
pub struct IfNode;

#[async_trait]
impl NodeExecutor for IfNode {
    fn descriptor(&self) -> NodeDescriptor {
        NodeDescriptor {
            kind: "flow.if".to_string(),
            label: "Condicional".to_string(),
            group: "Fluxo".to_string(),
            description: "Envia cada item para a saida verdadeira ou falsa.".to_string(),
            color: "#b38cff".to_string(),
            glyph: "IF".to_string(),
            inputs: vec![MAIN_PORT.to_string()],
            outputs: vec![TRUE_PORT.to_string(), FALSE_PORT.to_string()],
            defaults: json!({ "condition": "{{ $json.status == 200 }}" }),
            fields: vec![FieldSpec::new("condition", "Condicao", FieldKind::Expression)
                .required()
                .placeholder("{{ $json.status == 200 }}")
                .help(
                    "Avaliada item a item. Vazio, zero, lista vazia e null contam como falso.",
                )],
        }
    }

    async fn execute(&self, ctx: &NodeContext<'_>) -> Result<NodeOutput, NodeError> {
        // Le o parametro cru: uma condicao pode ser escrita com ou sem `{{ }}`.
        let raw = param_str(&ctx.node.parameters, "condition")
            .ok_or_else(|| NodeError::new("informe a condicao do no"))?;

        let items = ctx.items_or_single_empty();
        let mut matched = Vec::new();
        let mut rejected = Vec::new();

        for (index, item) in items.iter().enumerate() {
            let scope = ctx.scope(index);
            let value = if raw.contains("{{") {
                expr::render_template(&raw, &scope)?
            } else {
                expr::eval(&raw, &scope)?
            };

            if truthy(&value) {
                matched.push(item.clone());
            } else {
                rejected.push(item.clone());
            }
        }

        let mut output = NodeOutput::default();
        if !matched.is_empty() {
            output.ports.insert(TRUE_PORT.to_string(), matched);
        }
        if !rejected.is_empty() {
            output.ports.insert(FALSE_PORT.to_string(), rejected);
        }
        Ok(output)
    }
}

/// Junta os itens de varias entradas em uma saida so.
pub struct MergeNode;

#[async_trait]
impl NodeExecutor for MergeNode {
    fn descriptor(&self) -> NodeDescriptor {
        NodeDescriptor {
            kind: "flow.merge".to_string(),
            label: "Juntar".to_string(),
            group: "Fluxo".to_string(),
            description: "Reune os itens de varios ramos em uma saida unica.".to_string(),
            color: "#4cc9f0".to_string(),
            glyph: "MRG".to_string(),
            inputs: vec![MAIN_PORT.to_string()],
            outputs: vec![MAIN_PORT.to_string()],
            defaults: json!({ "mode": "append" }),
            fields: vec![FieldSpec::new("mode", "Modo", FieldKind::Select).options(vec![
                SelectOption::new("append", "Todos os itens, na ordem de chegada"),
                SelectOption::new("first", "Somente o primeiro item"),
                SelectOption::new("last", "Somente o ultimo item"),
            ])],
        }
    }

    async fn execute(&self, ctx: &NodeContext<'_>) -> Result<NodeOutput, NodeError> {
        let params = ctx.params_once()?;
        let items = ctx.items.clone();

        let selected = match param_str_or(&params, "mode", "append").as_str() {
            "first" => items.into_iter().take(1).collect::<Vec<Value>>(),
            "last" => items.into_iter().next_back().into_iter().collect(),
            _ => items,
        };

        Ok(NodeOutput::main(selected))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::NodeView;
    use crate::host::UnavailableHost;
    use crate::model::Node;
    use serde_json::Map;
    use std::collections::HashMap;
    use std::sync::Arc;

    async fn run_node(
        executor: &dyn NodeExecutor,
        parameters: Value,
        items: Vec<Value>,
    ) -> Result<NodeOutput, NodeError> {
        let mut node = Node::new("n", "No", "x");
        node.parameters = parameters;
        let nodes: HashMap<String, NodeView> = HashMap::new();
        let env = Map::new();
        let payload = Value::Null;
        let ctx = NodeContext {
            node: &node,
            flow_id: "f",
            run_id: "r",
            items,
            nodes: &nodes,
            env: &env,
            trigger_payload: &payload,
            now: chrono::Utc::now(),
            host: Arc::new(UnavailableHost),
        };
        executor.execute(&ctx).await
    }

    #[tokio::test]
    async fn if_splits_items_between_both_ports() {
        let output = run_node(
            &IfNode,
            json!({ "condition": "{{ $json.n > 2 }}" }),
            vec![json!({ "n": 1 }), json!({ "n": 3 }), json!({ "n": 5 })],
        )
        .await
        .unwrap();

        assert_eq!(output.ports[TRUE_PORT].len(), 2);
        assert_eq!(output.ports[FALSE_PORT].len(), 1);
        assert_eq!(output.ports[FALSE_PORT][0]["n"], json!(1));
    }

    #[tokio::test]
    async fn if_omits_the_port_that_got_no_items() {
        let output = run_node(
            &IfNode,
            json!({ "condition": "{{ $json.n > 2 }}" }),
            vec![json!({ "n": 10 })],
        )
        .await
        .unwrap();

        assert!(output.ports.contains_key(TRUE_PORT));
        // Porta ausente e o sinal que o motor usa para podar o ramo falso.
        assert!(!output.ports.contains_key(FALSE_PORT));
    }

    #[tokio::test]
    async fn if_accepts_a_condition_without_braces() {
        let output = run_node(
            &IfNode,
            json!({ "condition": "$json.ativo" }),
            vec![json!({ "ativo": true }), json!({ "ativo": false })],
        )
        .await
        .unwrap();

        assert_eq!(output.ports[TRUE_PORT].len(), 1);
        assert_eq!(output.ports[FALSE_PORT].len(), 1);
    }

    #[tokio::test]
    async fn if_without_condition_is_an_error() {
        let error = run_node(&IfNode, json!({}), vec![json!({})])
            .await
            .unwrap_err();
        assert!(error.message.contains("condicao"));
    }

    #[tokio::test]
    async fn if_reports_a_broken_expression() {
        let error = run_node(
            &IfNode,
            json!({ "condition": "{{ $json.n > }}" }),
            vec![json!({ "n": 1 })],
        )
        .await
        .unwrap_err();
        assert!(!error.message.is_empty());
    }

    #[tokio::test]
    async fn merge_modes_select_the_right_items() {
        let items = vec![json!({ "i": 1 }), json!({ "i": 2 }), json!({ "i": 3 })];

        let all = run_node(&MergeNode, json!({ "mode": "append" }), items.clone())
            .await
            .unwrap();
        assert_eq!(all.main_items().len(), 3);

        let first = run_node(&MergeNode, json!({ "mode": "first" }), items.clone())
            .await
            .unwrap();
        assert_eq!(first.main_items()[0]["i"], json!(1));

        let last = run_node(&MergeNode, json!({ "mode": "last" }), items)
            .await
            .unwrap();
        assert_eq!(last.main_items()[0]["i"], json!(3));
    }
}
