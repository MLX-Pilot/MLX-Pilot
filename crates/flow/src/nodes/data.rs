//! Nos de manipulacao de dados.

use async_trait::async_trait;
use serde_json::{json, Map, Value};

use crate::expr::stringify;
use crate::model::MAIN_PORT;
use crate::registry::{
    FieldKind, FieldSpec, NodeContext, NodeDescriptor, NodeError, NodeExecutor, NodeOutput,
};

use super::{param_array, param_bool, param_str};

/// Monta ou altera campos de cada item.
pub struct SetFieldsNode;

#[async_trait]
impl NodeExecutor for SetFieldsNode {
    fn descriptor(&self) -> NodeDescriptor {
        NodeDescriptor {
            kind: "data.set".to_string(),
            label: "Editar campos".to_string(),
            group: "Dados".to_string(),
            description: "Define campos em cada item, com expressoes {{ }}.".to_string(),
            color: "#2ec4b6".to_string(),
            glyph: "SET".to_string(),
            inputs: vec![MAIN_PORT.to_string()],
            outputs: vec![MAIN_PORT.to_string()],
            defaults: json!({
                "assignments": [{ "name": "campo", "value": "{{ $json.valor }}" }],
                "keep_only_set": false
            }),
            fields: vec![
                FieldSpec::new("assignments", "Campos", FieldKind::Json)
                    .required()
                    .help("Lista de { \"name\": \"campo\", \"value\": \"{{ $json.x }}\" }. Use ponto no nome para aninhar."),
                FieldSpec::new("keep_only_set", "Descartar os outros campos", FieldKind::Boolean)
                    .help("Quando ligado, o item de saida tem apenas os campos definidos aqui."),
            ],
        }
    }

    async fn execute(&self, ctx: &NodeContext<'_>) -> Result<NodeOutput, NodeError> {
        let source_items = ctx.items_or_single_empty();
        let mut output = Vec::with_capacity(source_items.len());

        for (index, item) in source_items.iter().enumerate() {
            // Resolve os parametros contra este item, para `{{ $json.x }}`
            // enxergar o item certo em cada volta.
            let params = ctx.params(index)?;
            let assignments = param_array(&params, "assignments");
            let keep_only_set = param_bool(&params, "keep_only_set", false);

            let mut target = if keep_only_set {
                Map::new()
            } else {
                item.as_object().cloned().unwrap_or_default()
            };

            for assignment in &assignments {
                let Some(name) = param_str(assignment, "name") else {
                    return Err(NodeError::new(
                        "cada campo precisa de um `name` nao vazio".to_string(),
                    ));
                };
                let value = assignment.get("value").cloned().unwrap_or(Value::Null);
                set_path(&mut target, &name, value);
            }

            output.push(Value::Object(target));
        }

        Ok(NodeOutput::main(output))
    }
}

/// Grava um valor em `map`, criando objetos intermediarios para nomes com ponto.
fn set_path(map: &mut Map<String, Value>, path: &str, value: Value) {
    let parts: Vec<&str> = path.split('.').filter(|part| !part.is_empty()).collect();
    if parts.is_empty() {
        return;
    }
    if parts.len() == 1 {
        map.insert(parts[0].to_string(), value);
        return;
    }

    let mut current = map;
    for part in &parts[..parts.len() - 1] {
        // Se o caminho atravessa algo que nao e objeto, sobrescreve.
        let entry = current
            .entry(part.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        current = entry.as_object_mut().expect("acabou de virar objeto");
    }
    current.insert(parts[parts.len() - 1].to_string(), value);
}

/// Repassa os itens registrando uma mensagem no log da execucao.
pub struct LogNode;

#[async_trait]
impl NodeExecutor for LogNode {
    fn descriptor(&self) -> NodeDescriptor {
        NodeDescriptor {
            kind: "debug.log".to_string(),
            label: "Log".to_string(),
            group: "Dados".to_string(),
            description: "Registra uma mensagem no historico e repassa os itens.".to_string(),
            color: "#8d99ae".to_string(),
            glyph: "LOG".to_string(),
            inputs: vec![MAIN_PORT.to_string()],
            outputs: vec![MAIN_PORT.to_string()],
            defaults: json!({ "message": "{{ $json }}" }),
            fields: vec![FieldSpec::new("message", "Mensagem", FieldKind::Expression)
                .placeholder("{{ $json }}")
                .help("Avaliada uma vez por item.")],
        }
    }

    async fn execute(&self, ctx: &NodeContext<'_>) -> Result<NodeOutput, NodeError> {
        let items = ctx.items_or_single_empty();
        let mut output = NodeOutput::main(items.clone());

        for (index, item) in items.iter().enumerate() {
            let params = ctx.params(index)?;
            let line = params
                .get("message")
                .map(stringify)
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| item.to_string());
            output.logs.push(line);
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::NodeView;
    use crate::host::UnavailableHost;
    use crate::model::Node;
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
    async fn set_merges_into_the_existing_item_by_default() {
        let output = run_node(
            &SetFieldsNode,
            json!({ "assignments": [{ "name": "novo", "value": "x" }] }),
            vec![json!({ "antigo": 1 })],
        )
        .await
        .unwrap();

        assert_eq!(output.main_items()[0]["antigo"], json!(1));
        assert_eq!(output.main_items()[0]["novo"], json!("x"));
    }

    #[tokio::test]
    async fn keep_only_set_drops_the_original_fields() {
        let output = run_node(
            &SetFieldsNode,
            json!({
                "keep_only_set": true,
                "assignments": [{ "name": "novo", "value": "x" }]
            }),
            vec![json!({ "antigo": 1 })],
        )
        .await
        .unwrap();

        assert!(output.main_items()[0].get("antigo").is_none());
        assert_eq!(output.main_items()[0]["novo"], json!("x"));
    }

    #[tokio::test]
    async fn expressions_are_resolved_per_item() {
        let output = run_node(
            &SetFieldsNode,
            json!({
                "keep_only_set": true,
                "assignments": [{ "name": "dobro", "value": "{{ $json.n * 2 }}" }]
            }),
            vec![json!({ "n": 2 }), json!({ "n": 5 })],
        )
        .await
        .unwrap();

        assert_eq!(output.main_items()[0]["dobro"], json!(4));
        assert_eq!(output.main_items()[1]["dobro"], json!(10));
    }

    #[tokio::test]
    async fn dotted_names_create_nested_objects() {
        let output = run_node(
            &SetFieldsNode,
            json!({
                "keep_only_set": true,
                "assignments": [
                    { "name": "user.profile.name", "value": "Ana" },
                    { "name": "user.id", "value": 1 }
                ]
            }),
            vec![json!({})],
        )
        .await
        .unwrap();

        assert_eq!(output.main_items()[0]["user"]["profile"]["name"], json!("Ana"));
        assert_eq!(output.main_items()[0]["user"]["id"], json!(1));
    }

    #[tokio::test]
    async fn assignment_without_name_is_an_error() {
        let error = run_node(
            &SetFieldsNode,
            json!({ "assignments": [{ "value": "x" }] }),
            vec![json!({})],
        )
        .await
        .unwrap_err();
        assert!(error.message.contains("name"));
    }

    #[tokio::test]
    async fn log_passes_items_through_and_records_lines() {
        let output = run_node(
            &LogNode,
            json!({ "message": "n={{ $json.n }}" }),
            vec![json!({ "n": 1 }), json!({ "n": 2 })],
        )
        .await
        .unwrap();

        assert_eq!(output.main_items().len(), 2);
        assert_eq!(output.logs, vec!["n=1".to_string(), "n=2".to_string()]);
    }

    #[test]
    fn set_path_overwrites_non_object_segments() {
        let mut map = Map::new();
        map.insert("a".to_string(), json!("texto"));
        set_path(&mut map, "a.b", json!(1));
        assert_eq!(map["a"]["b"], json!(1));
    }
}
