//! Importacao de workflows exportados do n8n.
//!
//! E um conversor de arquivo: le o JSON que o n8n exporta e devolve um
//! [`Flow`] no formato `mlxflow.v1`. Nao fala com nenhuma instancia do n8n e
//! nao depende dele em tempo de execucao — existe apenas para que workflows ja
//! escritos nao precisem ser refeitos a mao.
//!
//! A conversao e best-effort e sempre acompanha um relatorio: tipos de no sem
//! equivalente viram `debug.log` com uma nota, para que o desenho do grafo
//! sobreviva e a lacuna fique visivel no editor.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::model::{Edge, Flow, Node, Position};
use crate::nodes::control::{FALSE_PORT, TRUE_PORT};

/// Resultado de uma importacao.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportReport {
    pub flow: Flow,
    /// Pontos que exigem revisao manual.
    pub warnings: Vec<String>,
    /// Tipos de no do n8n que nao tem equivalente nativo.
    pub unsupported_kinds: Vec<String>,
}

/// Converte um workflow do n8n para o formato nativo.
pub fn from_n8n(source: &Value) -> Result<ImportReport, String> {
    let object = source
        .as_object()
        .ok_or_else(|| "o JSON do workflow precisa ser um objeto".to_string())?;

    let raw_nodes = object
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| "o workflow precisa ter uma lista `nodes`".to_string())?;

    let mut warnings = Vec::new();
    let mut unsupported = HashSet::new();
    let mut flow = Flow::new(
        object
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("Workflow importado"),
    );
    flow.description = Some("Importado de um workflow do n8n.".to_string());

    // Nomes do n8n sao as chaves de `connections`, entao precisamos do mapa
    // nome -> id nativo para reconstruir as arestas.
    let mut id_by_name: HashMap<String, String> = HashMap::new();
    let mut kind_by_name: HashMap<String, String> = HashMap::new();
    let mut used_names: HashSet<String> = HashSet::new();

    for (index, raw) in raw_nodes.iter().enumerate() {
        let Some(node_object) = raw.as_object() else {
            warnings.push(format!("o no na posicao {index} nao e um objeto e foi ignorado."));
            continue;
        };

        let original_name = node_object
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("No {}", index + 1));

        let name = unique_name(&original_name, &mut used_names);
        if name != original_name {
            warnings.push(format!(
                "o nome \"{original_name}\" estava repetido e virou \"{name}\"."
            ));
        }

        let id = node_object
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        let n8n_type = node_object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let parameters = node_object
            .get("parameters")
            .cloned()
            .unwrap_or_else(|| json!({}));

        let converted = convert_node(&n8n_type, &parameters);
        if converted.unsupported {
            unsupported.insert(n8n_type.clone());
            warnings.push(format!(
                "o no \"{name}\" era do tipo `{n8n_type}`, que nao tem equivalente nativo: virou um `debug.log` com nota."
            ));
        }
        warnings.extend(converted.warnings.into_iter().map(|warning| format!("\"{name}\": {warning}")));

        let mut node = Node::new(id.clone(), name.clone(), converted.kind.clone());
        node.parameters = converted.parameters;
        node.position = read_position(node_object.get("position"), index);
        node.disabled = node_object
            .get("disabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        node.notes = converted.notes.or_else(|| {
            node_object
                .get("notes")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        });

        id_by_name.insert(name.clone(), id);
        kind_by_name.insert(name.clone(), converted.kind);
        flow.nodes.push(node);
    }

    // `connections` e um mapa nome -> { "main": [ [ {node, index}, ... ] ] }.
    if let Some(connections) = object.get("connections").and_then(Value::as_object) {
        for (source_name, outputs) in connections {
            let Some(source_id) = id_by_name.get(source_name) else {
                warnings.push(format!(
                    "a conexao partindo de \"{source_name}\" foi descartada: o no nao existe."
                ));
                continue;
            };
            let source_kind = kind_by_name
                .get(source_name)
                .cloned()
                .unwrap_or_default();

            let Some(main_outputs) = outputs.get("main").and_then(Value::as_array) else {
                continue;
            };

            for (output_index, branch) in main_outputs.iter().enumerate() {
                let Some(targets) = branch.as_array() else {
                    continue;
                };
                let port = port_for(&source_kind, output_index, &mut warnings, source_name);

                for target in targets {
                    let Some(target_name) = target.get("node").and_then(Value::as_str) else {
                        continue;
                    };
                    let Some(target_id) = id_by_name.get(target_name) else {
                        warnings.push(format!(
                            "a conexao de \"{source_name}\" para \"{target_name}\" foi descartada: o destino nao existe."
                        ));
                        continue;
                    };
                    flow.edges.push(Edge::from_port(
                        source_id.clone(),
                        port.clone(),
                        target_id.clone(),
                    ));
                }
            }
        }
        // `connections` e um mapa sem ordem garantida; ordena para o import ser
        // reproduzivel.
        flow.edges.sort_by(|a, b| {
            (&a.from, &a.from_port, &a.to).cmp(&(&b.from, &b.from_port, &b.to))
        });
    }

    let mut unsupported_kinds: Vec<String> = unsupported.into_iter().collect();
    unsupported_kinds.sort();

    Ok(ImportReport {
        flow,
        warnings,
        unsupported_kinds,
    })
}

/// Nome da porta de saida correspondente ao indice do n8n.
fn port_for(
    source_kind: &str,
    output_index: usize,
    warnings: &mut Vec<String>,
    source_name: &str,
) -> String {
    if source_kind == "flow.if" {
        // No n8n, a saida 0 do IF e a verdadeira e a 1 e a falsa.
        return if output_index == 0 {
            TRUE_PORT.to_string()
        } else {
            FALSE_PORT.to_string()
        };
    }
    if output_index > 0 {
        warnings.push(format!(
            "\"{source_name}\" tinha mais de uma saida; todas foram ligadas na saida principal."
        ));
    }
    crate::model::MAIN_PORT.to_string()
}

/// Resultado da conversao de um unico no.
struct ConvertedNode {
    kind: String,
    parameters: Value,
    notes: Option<String>,
    warnings: Vec<String>,
    unsupported: bool,
}

impl ConvertedNode {
    fn new(kind: &str, parameters: Value) -> Self {
        Self {
            kind: kind.to_string(),
            parameters,
            notes: None,
            warnings: Vec::new(),
            unsupported: false,
        }
    }

    fn warn(mut self, message: &str) -> Self {
        self.warnings.push(message.to_string());
        self
    }
}

fn convert_node(n8n_type: &str, parameters: &Value) -> ConvertedNode {
    match n8n_type {
        "n8n-nodes-base.manualTrigger" | "n8n-nodes-base.executeWorkflowTrigger" => {
            ConvertedNode::new("trigger.manual", json!({ "sample": "" }))
        }
        "n8n-nodes-base.webhook" => ConvertedNode::new(
            "trigger.webhook",
            json!({
                "path": string_param(parameters, "path").unwrap_or_else(|| "importado".to_string()),
                "method": string_param(parameters, "httpMethod")
                    .unwrap_or_else(|| "POST".to_string())
                    .to_uppercase(),
                "response_mode": "last_node"
            }),
        ),
        "n8n-nodes-base.scheduleTrigger" | "n8n-nodes-base.cron" => convert_schedule(parameters),
        "n8n-nodes-base.httpRequest" => convert_http(parameters),
        "n8n-nodes-base.set" => convert_set(parameters),
        "n8n-nodes-base.if" | "n8n-nodes-base.filter" => convert_if(parameters),
        "n8n-nodes-base.merge" => ConvertedNode::new("flow.merge", json!({ "mode": "append" })),
        "n8n-nodes-base.noOp" => ConvertedNode::new("debug.log", json!({ "message": "{{ $json }}" })),
        "n8n-nodes-base.respondToWebhook" => ConvertedNode::new(
            "debug.log",
            json!({ "message": "{{ $json }}" }),
        )
        .warn(
            "o `Respond to Webhook` virou um log: no formato nativo a resposta e controlada pelo campo `response_mode` do gatilho de webhook.",
        ),
        other => {
            let mut converted = ConvertedNode::new(
                "debug.log",
                json!({ "message": "{{ $json }}" }),
            );
            converted.notes = Some(format!(
                "Importado de `{other}`. Parametros originais: {}",
                serde_json::to_string(parameters).unwrap_or_else(|_| "{}".to_string())
            ));
            converted.unsupported = true;
            converted
        }
    }
}

fn convert_schedule(parameters: &Value) -> ConvertedNode {
    // `cronExpression` aparece no no `cron`; o `scheduleTrigger` usa
    // `rule.interval[]` com campos como `minutesInterval`.
    if let Some(cron) = string_param(parameters, "cronExpression") {
        return ConvertedNode::new("trigger.schedule", json!({ "cron": cron }));
    }

    let interval = parameters
        .get("rule")
        .and_then(|rule| rule.get("interval"))
        .and_then(Value::as_array)
        .and_then(|items| items.first().cloned());

    if let Some(interval) = interval {
        if let Some(minutes) = interval.get("minutesInterval").and_then(Value::as_u64) {
            return ConvertedNode::new(
                "trigger.schedule",
                json!({ "cron": format!("0 */{} * * * *", minutes.max(1)) }),
            );
        }
        if let Some(hours) = interval.get("hoursInterval").and_then(Value::as_u64) {
            return ConvertedNode::new(
                "trigger.schedule",
                json!({ "cron": format!("0 0 */{} * * *", hours.max(1)) }),
            );
        }
    }

    ConvertedNode::new("trigger.schedule", json!({ "cron": "0 */5 * * * *" }))
        .warn("nao foi possivel ler a agenda original; ficou a cada 5 minutos.")
}

fn convert_http(parameters: &Value) -> ConvertedNode {
    let url = string_param(parameters, "url").unwrap_or_default();
    let method = string_param(parameters, "method")
        .unwrap_or_else(|| "GET".to_string())
        .to_uppercase();
    let raw_body = string_param(parameters, "jsonBody");

    // Chamadas ao proprio MLX Pilot viram o no nativo de agente.
    if url.trim_end_matches('/').ends_with("/agent/run") {
        if let Some(body) = raw_body
            .as_deref()
            .map(convert_expression)
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        {
            if let Some(message) = body.get("message").and_then(Value::as_str) {
                return ConvertedNode::new(
                    "agent.run",
                    json!({
                        "message": message,
                        "provider": body.get("provider").and_then(Value::as_str).unwrap_or_default(),
                        "model_id": body.get("model_id").and_then(Value::as_str).unwrap_or_default(),
                        "base_url": body.get("base_url").and_then(Value::as_str).unwrap_or_default(),
                        "max_iterations": body.get("max_iterations").and_then(Value::as_u64).unwrap_or(1),
                        "tools": [],
                        "output_key": "response",
                        "keep_input": true
                    }),
                )
                .warn("era um HTTP Request para /agent/run e virou o no nativo `agent.run`.");
            }
        }
    }

    let send_body = parameters
        .get("sendBody")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let body_value = raw_body.as_deref().map(convert_expression);

    let mut converted = ConvertedNode::new(
        "http.request",
        json!({
            "method": method,
            "url": convert_expression(&url),
            "headers": {},
            "query": {},
            "body_type": if send_body && body_value.is_some() { "json" } else { "none" },
            "body": body_value.clone().unwrap_or_default(),
            "timeout_secs": 30,
            "response_format": "auto",
            "fail_on_error_status": true
        }),
    );
    if send_body && body_value.is_none() {
        converted = converted.warn("o corpo da requisicao nao pode ser lido e ficou vazio.");
    }
    converted
}

fn convert_set(parameters: &Value) -> ConvertedNode {
    // Formato novo: `assignments.assignments[] = { name, value, type }`.
    let entries = parameters
        .get("assignments")
        .and_then(|value| value.get("assignments"))
        .and_then(Value::as_array)
        .cloned()
        .or_else(|| {
            // Formato antigo: `values.string[] = { name, value }`.
            parameters
                .get("values")
                .and_then(Value::as_object)
                .map(|groups| {
                    groups
                        .values()
                        .filter_map(Value::as_array)
                        .flatten()
                        .cloned()
                        .collect()
                })
        })
        .unwrap_or_default();

    let assignments: Vec<Value> = entries
        .iter()
        .filter_map(|entry| {
            let name = entry.get("name").and_then(Value::as_str)?;
            let value = match entry.get("value") {
                Some(Value::String(text)) => Value::String(convert_expression(text)),
                Some(other) => other.clone(),
                None => Value::Null,
            };
            Some(json!({ "name": name, "value": value }))
        })
        .collect();

    let keep_only_set = parameters
        .get("includeOtherFields")
        .and_then(Value::as_bool)
        .map(|include| !include)
        .or_else(|| {
            parameters
                .get("options")
                .and_then(|options| options.get("keepOnlySet"))
                .and_then(Value::as_bool)
        })
        .unwrap_or(false);

    let converted = ConvertedNode::new(
        "data.set",
        json!({ "assignments": assignments, "keep_only_set": keep_only_set }),
    );
    if entries.is_empty() {
        return converted.warn("nao havia campos legiveis no `Set` original.");
    }
    converted
}

fn convert_if(parameters: &Value) -> ConvertedNode {
    // As condicoes do n8n sao uma arvore de operadores; traduzir fielmente
    // exigiria reimplementar essa semantica. Converte a primeira comparacao e
    // deixa o resto explicito como aviso.
    let first = parameters
        .get("conditions")
        .and_then(|conditions| conditions.get("conditions"))
        .and_then(Value::as_array)
        .and_then(|items| items.first().cloned());

    let Some(condition) = first else {
        return ConvertedNode::new("flow.if", json!({ "condition": "{{ $json }}" }))
            .warn("a condicao original nao pode ser lida; revise o no antes de executar.");
    };

    let left = condition
        .get("leftValue")
        .map(|value| match value {
            Value::String(text) => convert_expression(text),
            other => other.to_string(),
        })
        .unwrap_or_else(|| "$json".to_string());
    let right = condition
        .get("rightValue")
        .map(|value| match value {
            Value::String(text) => format!("'{}'", convert_expression(text).replace('\'', "\\'")),
            other => other.to_string(),
        })
        .unwrap_or_else(|| "''".to_string());
    let operator = condition
        .get("operator")
        .and_then(|operator| operator.get("operation"))
        .and_then(Value::as_str)
        .unwrap_or("equals");

    let left_expression = strip_braces(&left);
    let expression = match operator {
        "notEquals" => format!("{left_expression} != {right}"),
        "gt" | "larger" => format!("{left_expression} > {right}"),
        "gte" | "largerEqual" => format!("{left_expression} >= {right}"),
        "lt" | "smaller" => format!("{left_expression} < {right}"),
        "lte" | "smallerEqual" => format!("{left_expression} <= {right}"),
        "contains" => format!("contains({left_expression}, {right})"),
        "isEmpty" => format!("!{left_expression}"),
        "isNotEmpty" => format!("!!{left_expression}"),
        _ => format!("{left_expression} == {right}"),
    };

    ConvertedNode::new(
        "flow.if",
        json!({ "condition": format!("{{{{ {expression} }}}}") }),
    )
    .warn("so a primeira condicao foi convertida; confira o no se o original tinha varias.")
}

/// Remove o `=` inicial que o n8n usa para marcar expressoes.
fn convert_expression(text: &str) -> String {
    text.strip_prefix('=').unwrap_or(text).to_string()
}

/// Tira `{{ }}` de uma expressao para poder compo-la com operadores.
fn strip_braces(text: &str) -> String {
    let trimmed = text.trim();
    trimmed
        .strip_prefix("{{")
        .and_then(|rest| rest.strip_suffix("}}"))
        .map(str::trim)
        .unwrap_or(trimmed)
        .to_string()
}

fn string_param(parameters: &Value, key: &str) -> Option<String> {
    parameters
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn read_position(value: Option<&Value>, index: usize) -> Position {
    let fallback = Position {
        x: 240.0 + (index as f64 * 260.0),
        y: 200.0,
    };
    let Some(items) = value.and_then(Value::as_array) else {
        return fallback;
    };
    let x = items.first().and_then(Value::as_f64);
    let y = items.get(1).and_then(Value::as_f64);
    match (x, y) {
        (Some(x), Some(y)) => Position { x, y },
        _ => fallback,
    }
}

/// Garante nome unico, ja que `$node["..."]` depende disso.
fn unique_name(name: &str, used: &mut HashSet<String>) -> String {
    if used.insert(name.to_string()) {
        return name.to_string();
    }
    let mut suffix = 2;
    loop {
        let candidate = format!("{name} ({suffix})");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        suffix += 1;
    }
}

/// Converte `Map` do serde para o `Flow`, util para chamadas a partir de rotas.
pub fn from_n8n_object(object: &Map<String, Value>) -> Result<ImportReport, String> {
    from_n8n(&Value::Object(object.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::builtin_registry;

    fn sample_workflow() -> Value {
        json!({
            "name": "Resumo via MLX Pilot",
            "nodes": [
                {
                    "id": "a",
                    "name": "When clicking Test",
                    "type": "n8n-nodes-base.manualTrigger",
                    "typeVersion": 1,
                    "position": [260, 300],
                    "parameters": {}
                },
                {
                    "id": "b",
                    "name": "Ask MLX Pilot",
                    "type": "n8n-nodes-base.httpRequest",
                    "typeVersion": 4.5,
                    "position": [520, 300],
                    "parameters": {
                        "method": "POST",
                        "url": "http://127.0.0.1:11435/agent/run",
                        "sendBody": true,
                        "jsonBody": "={\"message\":\"Resuma o texto\",\"provider\":\"ollama\",\"model_id\":\"qwen3.5:9b\",\"max_iterations\":1}"
                    }
                },
                {
                    "id": "c",
                    "name": "Formatar",
                    "type": "n8n-nodes-base.set",
                    "typeVersion": 3.4,
                    "position": [780, 300],
                    "parameters": {
                        "assignments": {
                            "assignments": [
                                { "name": "resumo", "value": "={{ $json.final_response }}" }
                            ]
                        }
                    }
                }
            ],
            "connections": {
                "When clicking Test": { "main": [[{ "node": "Ask MLX Pilot", "type": "main", "index": 0 }]] },
                "Ask MLX Pilot": { "main": [[{ "node": "Formatar", "type": "main", "index": 0 }]] }
            }
        })
    }

    #[test]
    fn imports_nodes_edges_and_positions() {
        let report = from_n8n(&sample_workflow()).unwrap();
        let flow = &report.flow;

        assert_eq!(flow.name, "Resumo via MLX Pilot");
        assert_eq!(flow.nodes.len(), 3);
        assert_eq!(flow.edges.len(), 2);
        assert_eq!(flow.nodes[0].kind, "trigger.manual");
        assert_eq!(flow.nodes[0].position.x, 260.0);
        assert_eq!(flow.nodes[1].position.y, 300.0);
    }

    #[test]
    fn agent_http_request_becomes_the_native_agent_node() {
        let report = from_n8n(&sample_workflow()).unwrap();
        let agent = report.flow.node_by_name("Ask MLX Pilot").unwrap();

        assert_eq!(agent.kind, "agent.run");
        assert_eq!(agent.parameters["message"], json!("Resuma o texto"));
        assert_eq!(agent.parameters["model_id"], json!("qwen3.5:9b"));
    }

    #[test]
    fn set_node_assignments_lose_the_n8n_equals_prefix() {
        let report = from_n8n(&sample_workflow()).unwrap();
        let set = report.flow.node_by_name("Formatar").unwrap();

        assert_eq!(set.kind, "data.set");
        assert_eq!(
            set.parameters["assignments"][0]["value"],
            json!("{{ $json.final_response }}")
        );
    }

    #[test]
    fn imported_flow_passes_native_validation() {
        let report = from_n8n(&sample_workflow()).unwrap();
        let registry = builtin_registry();
        let validation = crate::graph::validate(&report.flow, |kind| registry.contains(kind));
        assert!(validation.valid, "{:?}", validation.issues);
    }

    #[test]
    fn if_node_output_indexes_become_true_and_false_ports() {
        let workflow = json!({
            "name": "Com IF",
            "nodes": [
                { "id": "a", "name": "IF", "type": "n8n-nodes-base.if", "parameters": {
                    "conditions": { "conditions": [
                        { "leftValue": "={{ $json.status }}", "rightValue": 200, "operator": { "operation": "equals" } }
                    ]}
                }},
                { "id": "b", "name": "Sim", "type": "n8n-nodes-base.noOp", "parameters": {} },
                { "id": "c", "name": "Nao", "type": "n8n-nodes-base.noOp", "parameters": {} }
            ],
            "connections": {
                "IF": { "main": [
                    [{ "node": "Sim", "index": 0 }],
                    [{ "node": "Nao", "index": 0 }]
                ]}
            }
        });

        let report = from_n8n(&workflow).unwrap();
        let ports: Vec<&str> = report
            .flow
            .edges
            .iter()
            .map(|edge| edge.from_port.as_str())
            .collect();
        assert!(ports.contains(&"true"));
        assert!(ports.contains(&"false"));

        let if_node = report.flow.node_by_name("IF").unwrap();
        assert_eq!(if_node.kind, "flow.if");
        assert_eq!(
            if_node.parameters["condition"],
            json!("{{ $json.status == 200 }}")
        );
    }

    #[test]
    fn unsupported_node_becomes_a_log_with_a_note() {
        let workflow = json!({
            "name": "Com Code",
            "nodes": [
                { "id": "a", "name": "Code", "type": "n8n-nodes-base.code", "parameters": { "jsCode": "return items;" } }
            ],
            "connections": {}
        });

        let report = from_n8n(&workflow).unwrap();
        let node = &report.flow.nodes[0];
        assert_eq!(node.kind, "debug.log");
        assert!(node.notes.as_ref().unwrap().contains("n8n-nodes-base.code"));
        assert_eq!(report.unsupported_kinds, vec!["n8n-nodes-base.code"]);
        assert!(report.warnings.iter().any(|w| w.contains("Code")));
    }

    #[test]
    fn schedule_interval_becomes_a_cron_expression() {
        let workflow = json!({
            "name": "Agenda",
            "nodes": [
                { "id": "a", "name": "Cada 10 min", "type": "n8n-nodes-base.scheduleTrigger",
                  "parameters": { "rule": { "interval": [{ "field": "minutes", "minutesInterval": 10 }] } } }
            ],
            "connections": {}
        });

        let report = from_n8n(&workflow).unwrap();
        assert_eq!(
            report.flow.nodes[0].parameters["cron"],
            json!("0 */10 * * * *")
        );
    }

    #[test]
    fn duplicated_names_are_made_unique() {
        let workflow = json!({
            "name": "Repetido",
            "nodes": [
                { "id": "a", "name": "Igual", "type": "n8n-nodes-base.noOp", "parameters": {} },
                { "id": "b", "name": "Igual", "type": "n8n-nodes-base.noOp", "parameters": {} }
            ],
            "connections": {}
        });

        let report = from_n8n(&workflow).unwrap();
        assert_eq!(report.flow.nodes[0].name, "Igual");
        assert_eq!(report.flow.nodes[1].name, "Igual (2)");
        assert!(report.warnings.iter().any(|w| w.contains("repetido")));
    }

    #[test]
    fn dangling_connection_is_dropped_with_a_warning() {
        let workflow = json!({
            "name": "Solto",
            "nodes": [{ "id": "a", "name": "Um", "type": "n8n-nodes-base.noOp", "parameters": {} }],
            "connections": { "Um": { "main": [[{ "node": "Fantasma", "index": 0 }]] } }
        });

        let report = from_n8n(&workflow).unwrap();
        assert!(report.flow.edges.is_empty());
        assert!(report.warnings.iter().any(|w| w.contains("Fantasma")));
    }

    #[test]
    fn webhook_parameters_are_carried_over() {
        let workflow = json!({
            "name": "Hook",
            "nodes": [{
                "id": "a", "name": "Webhook", "type": "n8n-nodes-base.webhook",
                "parameters": { "path": "entrada", "httpMethod": "post" }
            }],
            "connections": {}
        });

        let report = from_n8n(&workflow).unwrap();
        assert_eq!(report.flow.nodes[0].kind, "trigger.webhook");
        assert_eq!(report.flow.nodes[0].parameters["path"], json!("entrada"));
        assert_eq!(report.flow.nodes[0].parameters["method"], json!("POST"));
    }

    #[test]
    fn rejects_json_without_a_node_list() {
        assert!(from_n8n(&json!({ "name": "vazio" })).is_err());
        assert!(from_n8n(&json!([])).is_err());
    }

    #[test]
    fn import_is_deterministic() {
        let first = from_n8n(&sample_workflow()).unwrap().flow;
        let second = from_n8n(&sample_workflow()).unwrap().flow;
        let names: Vec<&String> = first.nodes.iter().map(|node| &node.name).collect();
        let other_names: Vec<&String> = second.nodes.iter().map(|node| &node.name).collect();
        assert_eq!(names, other_names);
        assert_eq!(first.edges, second.edges);
    }
}
