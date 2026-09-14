//! Nos que falam com o proprio MLX Pilot.
//!
//! Sao os dois nos que justificam o motor ser embutido: em vez de sair por HTTP
//! e voltar, `agent.run` chama o agente no mesmo processo e `tool.call` usa o
//! registro de ferramentas do agente, com o sandbox que ja existe.

use async_trait::async_trait;
use serde_json::{json, Map, Value};


use crate::host::{AgentNodeRequest, ToolNodeRequest};
use crate::model::MAIN_PORT;
use crate::registry::{
    FieldKind, FieldSpec, NodeContext, NodeDescriptor, NodeError, NodeExecutor, NodeOutput,
    OptionsSource,
};

use super::{param_array, param_bool, param_str, param_str_or, param_u64};

/// Chama o agente do MLX Pilot uma vez por item.
pub struct AgentNode;

#[async_trait]
impl NodeExecutor for AgentNode {
    fn descriptor(&self) -> NodeDescriptor {
        NodeDescriptor {
            kind: "agent.run".to_string(),
            label: "Agente MLX Pilot".to_string(),
            group: "MLX Pilot".to_string(),
            description: "Envia um prompt ao agente local e devolve a resposta.".to_string(),
            color: "#00d4ff".to_string(),
            glyph: "AI".to_string(),
            inputs: vec![MAIN_PORT.to_string()],
            outputs: vec![MAIN_PORT.to_string()],
            defaults: json!({
                "message": "Resuma em uma frase: {{ $json.texto }}",
                "system_prompt": "",
                // Preenchidos pela UI com o provedor/modelo ativos ao criar o
                // no; vazios significam "usar o que o agente estiver usando".
                "provider": "",
                "model_id": "",
                "provider_profile_id": "",
                "base_url": "",
                // `null` significa herdar a temperatura configurada no agente.
                "temperature": null,
                "max_iterations": 1,
                "tools": [],
                "output_key": "response",
                "parse_json": false,
                "keep_input": true
            }),
            fields: vec![
                FieldSpec::dynamic("provider", "Provedor", OptionsSource::AgentProviders)
                    .help("Comeca no provedor ativo do MLX Pilot."),
                FieldSpec::dynamic("model_id", "Modelo", OptionsSource::AgentModels)
                    .help("Modelos locais instalados ou os do provedor de nuvem escolhido."),
                FieldSpec::new("message", "Mensagem", FieldKind::Textarea)
                    .required()
                    .placeholder("Resuma em uma frase: {{ $json.texto }}"),
                FieldSpec::new("system_prompt", "Prompt de sistema", FieldKind::Textarea),
                FieldSpec::new("tools", "Ferramentas liberadas", FieldKind::Json)
                    .help("Lista de nomes. Vazia roda sem ferramenta nenhuma."),
                FieldSpec::new("max_iterations", "Iteracoes maximas", FieldKind::Number)
                    .help("1 desliga o laco de ferramentas e devolve a primeira resposta."),
                FieldSpec::new("parse_json", "Interpretar a resposta como JSON", FieldKind::Boolean)
                    .help("Falha se o modelo nao devolver JSON valido."),
                FieldSpec::new("output_key", "Campo de saida", FieldKind::Text)
                    .placeholder("response")
                    .advanced(),
                FieldSpec::new("keep_input", "Manter os campos de entrada", FieldKind::Boolean)
                    .advanced(),
                // Herdados do provedor escolhido; so aparecem para sobrescrever.
                FieldSpec::new("base_url", "Base URL", FieldKind::Text)
                    .placeholder("herdada do provedor selecionado")
                    .advanced(),
                FieldSpec::new("temperature", "Temperatura", FieldKind::Number)
                    .placeholder("herdada do agente")
                    .advanced(),
            ],
        }
    }

    async fn execute(&self, ctx: &NodeContext<'_>) -> Result<NodeOutput, NodeError> {
        let items = ctx.items_or_single_empty();
        let mut output = Vec::with_capacity(items.len());

        for (index, item) in items.iter().enumerate() {
            let params = ctx.params(index)?;
            let message = param_str(&params, "message")
                .ok_or_else(|| NodeError::new("informe a mensagem enviada ao agente"))?;

            let request = AgentNodeRequest {
                message,
                system_prompt: param_str(&params, "system_prompt"),
                provider: param_str(&params, "provider"),
                model_id: param_str(&params, "model_id"),
                base_url: param_str(&params, "base_url"),
                provider_profile_id: param_str(&params, "provider_profile_id"),
                session_id: param_str(&params, "session_id"),
                max_iterations: Some(param_u64(&params, "max_iterations", 1).clamp(1, 20) as usize),
                temperature: params
                    .get("temperature")
                    .and_then(Value::as_f64)
                    .map(|value| value as f32),
                enabled_tools: Some(
                    param_array(&params, "tools")
                        .iter()
                        .filter_map(|value| value.as_str().map(ToString::to_string))
                        .collect(),
                ),
            };

            let result = ctx
                .host
                .run_agent(request)
                .await
                .map_err(|error| NodeError::new(format!("o agente falhou: {error}")))?;

            let value = if param_bool(&params, "parse_json", false) {
                parse_json_payload(&result.content)?
            } else {
                Value::String(result.content.clone())
            };

            let mut target = if param_bool(&params, "keep_input", true) {
                item.as_object().cloned().unwrap_or_default()
            } else {
                Map::new()
            };
            target.insert(param_str_or(&params, "output_key", "response"), value);
            target.insert(
                "_agent".to_string(),
                json!({
                    "provider": result.provider,
                    "model_id": result.model_id,
                    "session_id": result.session_id,
                    "total_tokens": result.total_tokens,
                    "latency_ms": result.latency_ms,
                }),
            );
            output.push(Value::Object(target));
        }

        Ok(NodeOutput::main(output))
    }
}

/// Aceita JSON puro ou embrulhado em cerca de markdown.
fn parse_json_payload(text: &str) -> Result<Value, NodeError> {
    let trimmed = text.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Ok(value);
    }

    // Modelos costumam responder com ```json ... ```; recorta o maior bloco.
    let start = trimmed.find(['{', '[']);
    let end = trimmed.rfind(['}', ']']);
    if let (Some(start), Some(end)) = (start, end) {
        if start < end {
            if let Ok(value) = serde_json::from_str::<Value>(&trimmed[start..=end]) {
                return Ok(value);
            }
        }
    }

    Err(NodeError::with_details(
        "a resposta do agente nao era JSON valido",
        json!({ "content": trimmed.chars().take(500).collect::<String>() }),
    ))
}

/// Executa uma ferramenta registrada no agente.
pub struct ToolNode;

#[async_trait]
impl NodeExecutor for ToolNode {
    fn descriptor(&self) -> NodeDescriptor {
        NodeDescriptor {
            kind: "tool.call".to_string(),
            label: "Ferramenta MLX Pilot".to_string(),
            group: "MLX Pilot".to_string(),
            description: "Roda uma ferramenta do agente (arquivos, busca, shell) no sandbox."
                .to_string(),
            color: "#7bdff2".to_string(),
            glyph: "TL".to_string(),
            inputs: vec![MAIN_PORT.to_string()],
            outputs: vec![MAIN_PORT.to_string()],
            defaults: json!({
                "tool": "read_file",
                "params": {},
                "read_only": true,
                "workspace_root": "",
                "output_key": "tool_output",
                "parse_json": false,
                "keep_input": true
            }),
            fields: vec![
                FieldSpec::dynamic("tool", "Ferramenta", OptionsSource::FlowTools).required(),
                FieldSpec::new("params", "Parametros", FieldKind::Json)
                    .help("Objeto JSON com os argumentos da ferramenta. Aceita expressoes."),
                FieldSpec::new("read_only", "Somente leitura", FieldKind::Boolean)
                    .help("Bloqueia escrita e execucao de comandos."),
                FieldSpec::new("parse_json", "Interpretar a saida como JSON", FieldKind::Boolean),
                FieldSpec::new("output_key", "Campo de saida", FieldKind::Text)
                    .placeholder("tool_output")
                    .advanced(),
                FieldSpec::new("keep_input", "Manter os campos de entrada", FieldKind::Boolean)
                    .advanced(),
                // Herdado do workspace do agente.
                FieldSpec::new("workspace_root", "Raiz do workspace", FieldKind::Text)
                    .placeholder("herdada do workspace do agente")
                    .advanced(),
            ],
        }
    }

    async fn execute(&self, ctx: &NodeContext<'_>) -> Result<NodeOutput, NodeError> {
        let items = ctx.items_or_single_empty();
        let mut output = Vec::with_capacity(items.len());

        for (index, item) in items.iter().enumerate() {
            let params = ctx.params(index)?;
            let name = param_str(&params, "tool")
                .ok_or_else(|| NodeError::new("informe o nome da ferramenta"))?;

            let request = ToolNodeRequest {
                name: name.clone(),
                params: params.get("params").cloned().unwrap_or_else(|| json!({})),
                workspace_root: param_str(&params, "workspace_root"),
                read_only: param_bool(&params, "read_only", true),
            };

            let result = ctx
                .host
                .call_tool(request)
                .await
                .map_err(|error| NodeError::new(format!("a ferramenta `{name}` falhou: {error}")))?;

            if result.is_error {
                return Err(NodeError::with_details(
                    format!("a ferramenta `{name}` retornou erro"),
                    json!({ "output": result.output }),
                ));
            }

            let value = if param_bool(&params, "parse_json", false) {
                serde_json::from_str::<Value>(result.output.trim()).map_err(|error| {
                    NodeError::new(format!(
                        "a saida da ferramenta `{name}` nao era JSON valido: {error}"
                    ))
                })?
            } else {
                Value::String(result.output.clone())
            };

            let mut target = if param_bool(&params, "keep_input", true) {
                item.as_object().cloned().unwrap_or_default()
            } else {
                Map::new()
            };
            target.insert(param_str_or(&params, "output_key", "tool_output"), value);
            target.insert(
                "_tool".to_string(),
                json!({
                    "name": name,
                    "metadata": Value::Object(result.metadata),
                }),
            );
            output.push(Value::Object(target));
        }

        Ok(NodeOutput::main(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::NodeView;
    use crate::host::{AgentNodeResult, FlowHost, ToolNodeResult};
    use crate::model::Node;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    /// Host de teste que devolve respostas fixas e registra o que recebeu.
    #[derive(Default)]
    struct SpyHost {
        agent_reply: String,
        tool_reply: String,
        tool_is_error: bool,
        fail: bool,
        agent_calls: Mutex<Vec<AgentNodeRequest>>,
        tool_calls: Mutex<Vec<ToolNodeRequest>>,
    }

    #[async_trait]
    impl FlowHost for SpyHost {
        async fn run_agent(&self, request: AgentNodeRequest) -> Result<AgentNodeResult, String> {
            if self.fail {
                return Err("provedor fora do ar".to_string());
            }
            self.agent_calls.lock().unwrap().push(request);
            Ok(AgentNodeResult {
                content: self.agent_reply.clone(),
                provider: "ollama".to_string(),
                model_id: "teste".to_string(),
                session_id: "s1".to_string(),
                total_tokens: 10,
                latency_ms: 5,
            })
        }

        async fn call_tool(&self, request: ToolNodeRequest) -> Result<ToolNodeResult, String> {
            if self.fail {
                return Err("registro indisponivel".to_string());
            }
            self.tool_calls.lock().unwrap().push(request);
            Ok(ToolNodeResult {
                output: self.tool_reply.clone(),
                is_error: self.tool_is_error,
                metadata: Map::new(),
            })
        }
    }

    async fn run_node(
        executor: &dyn NodeExecutor,
        parameters: Value,
        items: Vec<Value>,
        host: Arc<SpyHost>,
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
            host,
        };
        executor.execute(&ctx).await
    }

    #[tokio::test]
    async fn agent_node_merges_the_reply_into_the_item() {
        let host = Arc::new(SpyHost {
            agent_reply: "resumo".to_string(),
            ..Default::default()
        });

        let output = run_node(
            &AgentNode,
            json!({ "message": "Resuma: {{ $json.texto }}" }),
            vec![json!({ "texto": "um texto longo", "id": 3 })],
            host.clone(),
        )
        .await
        .unwrap();

        let item = &output.main_items()[0];
        assert_eq!(item["response"], json!("resumo"));
        assert_eq!(item["id"], json!(3));
        assert_eq!(item["_agent"]["model_id"], json!("teste"));

        // A expressao foi resolvida antes de chegar ao host.
        let calls = host.agent_calls.lock().unwrap();
        assert_eq!(calls[0].message, "Resuma: um texto longo");
    }

    #[tokio::test]
    async fn agent_node_runs_once_per_item() {
        let host = Arc::new(SpyHost {
            agent_reply: "ok".to_string(),
            ..Default::default()
        });

        let output = run_node(
            &AgentNode,
            json!({ "message": "{{ $json.n }}" }),
            vec![json!({ "n": 1 }), json!({ "n": 2 })],
            host.clone(),
        )
        .await
        .unwrap();

        assert_eq!(output.main_items().len(), 2);
        let calls = host.agent_calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].message, "2");
    }

    #[tokio::test]
    async fn agent_node_can_drop_the_input_fields() {
        let host = Arc::new(SpyHost {
            agent_reply: "r".to_string(),
            ..Default::default()
        });

        let output = run_node(
            &AgentNode,
            json!({ "message": "x", "keep_input": false }),
            vec![json!({ "secreto": 1 })],
            host,
        )
        .await
        .unwrap();

        assert!(output.main_items()[0].get("secreto").is_none());
    }

    #[tokio::test]
    async fn agent_node_parses_json_wrapped_in_markdown() {
        let host = Arc::new(SpyHost {
            agent_reply: "Claro!\n```json\n{\"nota\": 9}\n```".to_string(),
            ..Default::default()
        });

        let output = run_node(
            &AgentNode,
            json!({ "message": "x", "parse_json": true }),
            vec![json!({})],
            host,
        )
        .await
        .unwrap();

        assert_eq!(output.main_items()[0]["response"]["nota"], json!(9));
    }

    #[tokio::test]
    async fn agent_node_reports_unparseable_json() {
        let host = Arc::new(SpyHost {
            agent_reply: "nao vou responder em json".to_string(),
            ..Default::default()
        });

        let error = run_node(
            &AgentNode,
            json!({ "message": "x", "parse_json": true }),
            vec![json!({})],
            host,
        )
        .await
        .unwrap_err();
        assert!(error.message.contains("JSON valido"));
    }

    #[tokio::test]
    async fn agent_node_surfaces_host_failures() {
        let host = Arc::new(SpyHost {
            fail: true,
            ..Default::default()
        });

        let error = run_node(&AgentNode, json!({ "message": "x" }), vec![json!({})], host)
            .await
            .unwrap_err();
        assert!(error.message.contains("provedor fora do ar"));
    }

    #[tokio::test]
    async fn agent_node_requires_a_message() {
        let host = Arc::new(SpyHost::default());
        let error = run_node(&AgentNode, json!({ "message": "  " }), vec![json!({})], host)
            .await
            .unwrap_err();
        assert!(error.message.contains("mensagem"));
    }

    #[tokio::test]
    async fn tool_node_forwards_resolved_parameters() {
        let host = Arc::new(SpyHost {
            tool_reply: "conteudo".to_string(),
            ..Default::default()
        });

        let output = run_node(
            &ToolNode,
            json!({
                "tool": "read_file",
                "params": { "path": "docs/{{ $json.nome }}.md" }
            }),
            vec![json!({ "nome": "guia" })],
            host.clone(),
        )
        .await
        .unwrap();

        assert_eq!(output.main_items()[0]["tool_output"], json!("conteudo"));
        let calls = host.tool_calls.lock().unwrap();
        assert_eq!(calls[0].params["path"], json!("docs/guia.md"));
        assert!(calls[0].read_only, "read_only deve ser o padrao");
    }

    #[tokio::test]
    async fn tool_node_fails_when_the_tool_reports_an_error() {
        let host = Arc::new(SpyHost {
            tool_reply: "arquivo nao encontrado".to_string(),
            tool_is_error: true,
            ..Default::default()
        });

        let error = run_node(
            &ToolNode,
            json!({ "tool": "read_file", "params": {} }),
            vec![json!({})],
            host,
        )
        .await
        .unwrap_err();
        assert!(error.message.contains("retornou erro"));
    }

    #[test]
    fn json_payload_parser_handles_bare_and_fenced_json() {
        assert_eq!(parse_json_payload("{\"a\":1}").unwrap()["a"], json!(1));
        assert_eq!(
            parse_json_payload("texto antes ```json\n[1,2]\n``` texto depois").unwrap(),
            json!([1, 2])
        );
        assert!(parse_json_payload("sem json aqui").is_err());
    }

}
