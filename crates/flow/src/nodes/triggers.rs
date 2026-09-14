//! Nos de gatilho: entrada do fluxo.
//!
//! Gatilhos nao tem porta de entrada. Quem decide qual gatilho inicia a
//! execucao e o `FlowEngine`, a partir do `TriggerSource`; o no em si so molda
//! o payload recebido em itens.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::model::MAIN_PORT;
use crate::registry::{
    FieldKind, FieldSpec, NodeContext, NodeDescriptor, NodeError, NodeExecutor, NodeOutput,
    SelectOption,
};

use super::{param_str, param_str_or};

/// Gatilho manual: executado pelo botao da UI ou pela API.
pub struct ManualTrigger;

#[async_trait]
impl NodeExecutor for ManualTrigger {
    fn descriptor(&self) -> NodeDescriptor {
        NodeDescriptor {
            kind: "trigger.manual".to_string(),
            label: "Gatilho manual".to_string(),
            group: "Gatilhos".to_string(),
            description: "Inicia o fluxo sob demanda, a partir do botao Executar ou da API."
                .to_string(),
            color: "#f2b84b".to_string(),
            glyph: "GO".to_string(),
            inputs: Vec::new(),
            outputs: vec![MAIN_PORT.to_string()],
            defaults: json!({ "sample": "" }),
            fields: vec![FieldSpec::new("sample", "Payload de teste", FieldKind::Json).help(
                "JSON usado quando a execucao nao envia dados. Util para testar o fluxo no editor.",
            )],
        }
    }

    async fn execute(&self, ctx: &NodeContext<'_>) -> Result<NodeOutput, NodeError> {
        let items = ctx.items_or_single_empty();

        // O payload de teste so entra quando a execucao nao trouxe dados.
        let received_data = items.iter().any(|item| match item {
            Value::Object(map) => !map.is_empty(),
            Value::Null => false,
            _ => true,
        });
        if received_data {
            return Ok(NodeOutput::main(items));
        }

        let params = ctx.params_once()?;
        let Some(raw_sample) = param_str(&params, "sample") else {
            return Ok(NodeOutput::main(items));
        };

        let sample: Value = serde_json::from_str(&raw_sample).map_err(|error| {
            NodeError::new(format!("o payload de teste nao e um JSON valido: {error}"))
        })?;

        Ok(match sample {
            Value::Array(entries) if !entries.is_empty() => NodeOutput::main(entries),
            Value::Null => NodeOutput::main(items),
            other => NodeOutput::main(vec![other]),
        })
    }
}

/// Gatilho por webhook. O daemon roteia `POST /flows/webhook/{path}` para ca.
pub struct WebhookTrigger;

#[async_trait]
impl NodeExecutor for WebhookTrigger {
    fn descriptor(&self) -> NodeDescriptor {
        NodeDescriptor {
            kind: "trigger.webhook".to_string(),
            label: "Webhook".to_string(),
            group: "Gatilhos".to_string(),
            description: "Recebe uma chamada HTTP no proprio daemon do MLX Pilot.".to_string(),
            color: "#e76f51".to_string(),
            glyph: "WH".to_string(),
            inputs: Vec::new(),
            outputs: vec![MAIN_PORT.to_string()],
            defaults: json!({
                "path": "meu-fluxo",
                "method": "POST",
                "response_mode": "last_node"
            }),
            fields: vec![
                FieldSpec::new("path", "Caminho", FieldKind::Text)
                    .required()
                    .placeholder("meu-fluxo")
                    .help("A URL fica em /flows/webhook/<caminho>. O fluxo precisa estar ativo."),
                FieldSpec::new("method", "Metodo", FieldKind::Select).options(vec![
                    SelectOption::new("POST", "POST"),
                    SelectOption::new("GET", "GET"),
                    SelectOption::new("PUT", "PUT"),
                    SelectOption::new("DELETE", "DELETE"),
                ]),
                FieldSpec::new("response_mode", "Resposta", FieldKind::Select)
                    .options(vec![
                        SelectOption::new("last_node", "Devolver a saida do fluxo"),
                        SelectOption::new("immediate", "Responder na hora e executar em segundo plano"),
                    ])
                    .help("`immediate` responde 202 e nao espera o fluxo terminar."),
            ],
        }
    }

    async fn execute(&self, ctx: &NodeContext<'_>) -> Result<NodeOutput, NodeError> {
        // O payload da requisicao ja chega como itens de entrada.
        Ok(NodeOutput::main(ctx.items_or_single_empty()))
    }
}

/// Caminho normalizado de um webhook: minusculo, sem barras nas pontas.
pub fn normalize_webhook_path(path: &str) -> String {
    path.trim().trim_matches('/').to_lowercase()
}

/// Le `path` e `method` de um no de webhook, ja normalizados.
pub fn webhook_binding(parameters: &Value) -> Option<(String, String)> {
    let path = normalize_webhook_path(&param_str(parameters, "path")?);
    if path.is_empty() {
        return None;
    }
    let method = param_str_or(parameters, "method", "POST").to_uppercase();
    Some((path, method))
}

/// Gatilho por agenda. O daemon registra a expressao cron no agendador.
pub struct ScheduleTrigger;

#[async_trait]
impl NodeExecutor for ScheduleTrigger {
    fn descriptor(&self) -> NodeDescriptor {
        NodeDescriptor {
            kind: "trigger.schedule".to_string(),
            label: "Agenda".to_string(),
            group: "Gatilhos".to_string(),
            description: "Executa o fluxo em uma expressao cron.".to_string(),
            color: "#ff8fab".to_string(),
            glyph: "CR".to_string(),
            inputs: Vec::new(),
            outputs: vec![MAIN_PORT.to_string()],
            defaults: json!({ "cron": "0 */5 * * * *" }),
            fields: vec![FieldSpec::new("cron", "Expressao cron", FieldKind::Text)
                .required()
                .placeholder("0 */5 * * * *")
                .help("Seis campos: segundo minuto hora dia mes dia-da-semana. O fluxo precisa estar ativo.")],
        }
    }

    async fn execute(&self, ctx: &NodeContext<'_>) -> Result<NodeOutput, NodeError> {
        let params = ctx.params_once()?;
        Ok(NodeOutput::main(vec![json!({
            "triggered_at": ctx.now.to_rfc3339(),
            "cron": param_str_or(&params, "cron", ""),
        })]))
    }
}

/// Le a expressao cron de um no de agenda.
pub fn schedule_binding(parameters: &Value) -> Option<String> {
    param_str(parameters, "cron")
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

    fn context<'a>(
        node: &'a Node,
        items: Vec<Value>,
        nodes: &'a HashMap<String, NodeView>,
        env: &'a Map<String, Value>,
        payload: &'a Value,
    ) -> NodeContext<'a> {
        NodeContext {
            node,
            flow_id: "f",
            run_id: "r",
            items,
            nodes,
            env,
            trigger_payload: payload,
            now: chrono::Utc::now(),
            host: Arc::new(UnavailableHost),
        }
    }

    #[tokio::test]
    async fn manual_trigger_uses_sample_only_when_no_data_arrives() {
        let mut node = Node::new("n", "Start", "trigger.manual");
        node.parameters = json!({ "sample": "{\"origem\":\"teste\"}" });
        let nodes = HashMap::new();
        let env = Map::new();
        let payload = Value::Null;

        let empty = ManualTrigger
            .execute(&context(&node, vec![json!({})], &nodes, &env, &payload))
            .await
            .unwrap();
        assert_eq!(empty.main_items()[0]["origem"], json!("teste"));

        let with_data = ManualTrigger
            .execute(&context(
                &node,
                vec![json!({ "real": 1 })],
                &nodes,
                &env,
                &payload,
            ))
            .await
            .unwrap();
        assert_eq!(with_data.main_items()[0]["real"], json!(1));
    }

    #[tokio::test]
    async fn manual_trigger_rejects_invalid_sample_json() {
        let mut node = Node::new("n", "Start", "trigger.manual");
        node.parameters = json!({ "sample": "{nao e json}" });
        let nodes = HashMap::new();
        let env = Map::new();
        let payload = Value::Null;

        let error = ManualTrigger
            .execute(&context(&node, vec![json!({})], &nodes, &env, &payload))
            .await
            .unwrap_err();
        assert!(error.message.contains("JSON valido"));
    }

    #[tokio::test]
    async fn manual_trigger_sample_array_becomes_multiple_items() {
        let mut node = Node::new("n", "Start", "trigger.manual");
        node.parameters = json!({ "sample": "[{\"i\":1},{\"i\":2}]" });
        let nodes = HashMap::new();
        let env = Map::new();
        let payload = Value::Null;

        let output = ManualTrigger
            .execute(&context(&node, vec![json!({})], &nodes, &env, &payload))
            .await
            .unwrap();
        assert_eq!(output.main_items().len(), 2);
    }

    #[test]
    fn webhook_binding_normalizes_path_and_method() {
        let binding = webhook_binding(&json!({ "path": "/Meu-Fluxo/", "method": "post" }));
        assert_eq!(binding, Some(("meu-fluxo".to_string(), "POST".to_string())));
    }

    #[test]
    fn webhook_binding_rejects_empty_path() {
        assert_eq!(webhook_binding(&json!({ "path": "  /  " })), None);
        assert_eq!(webhook_binding(&json!({})), None);
    }

    #[test]
    fn webhook_binding_defaults_to_post() {
        let binding = webhook_binding(&json!({ "path": "x" })).unwrap();
        assert_eq!(binding.1, "POST");
    }

    #[test]
    fn schedule_binding_reads_the_cron_expression() {
        assert_eq!(
            schedule_binding(&json!({ "cron": "0 0 * * * *" })),
            Some("0 0 * * * *".to_string())
        );
        assert_eq!(schedule_binding(&json!({ "cron": "  " })), None);
    }
}
