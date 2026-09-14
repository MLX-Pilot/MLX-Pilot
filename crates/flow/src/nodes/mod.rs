//! Nos embutidos do motor.
//!
//! O conjunto cobre a mesma superficie que a aba de workflows usava no n8n
//! (gatilho manual, webhook, agenda, HTTP, edicao de campos, condicional) e
//! adiciona dois nos que so existem aqui: `agent.run`, que chama o agente do
//! MLX Pilot no mesmo processo, e `tool.call`, que executa uma ferramenta do
//! registro do agente dentro do sandbox ja existente.

use serde_json::{Map, Value};

use crate::registry::NodeRegistry;

pub mod control;
pub mod data;
pub mod http;
pub mod mlx;
pub mod triggers;

/// Registro com todos os nos embutidos.
pub fn builtin_registry() -> NodeRegistry {
    let mut registry = NodeRegistry::new();
    registry.register(std::sync::Arc::new(triggers::ManualTrigger));
    registry.register(std::sync::Arc::new(triggers::WebhookTrigger));
    registry.register(std::sync::Arc::new(triggers::ScheduleTrigger));
    registry.register(std::sync::Arc::new(http::HttpRequestNode::default()));
    registry.register(std::sync::Arc::new(data::SetFieldsNode));
    registry.register(std::sync::Arc::new(data::LogNode));
    registry.register(std::sync::Arc::new(control::IfNode));
    registry.register(std::sync::Arc::new(control::MergeNode));
    registry.register(std::sync::Arc::new(mlx::AgentNode));
    registry.register(std::sync::Arc::new(mlx::ToolNode));
    registry
}

// ───────────────────────── leitura de parametros ─────────────────────────

/// String nao vazia de um parametro.
///
/// Tolera valores que nao sao string porque um template resolvido preserva o
/// tipo: `message: "{{ $json.id }}"` com `id` numerico chega aqui como numero,
/// e recusar isso quebraria o no por um detalhe de tipagem.
pub(crate) fn param_str(params: &Value, key: &str) -> Option<String> {
    let text = match params.get(key) {
        None | Some(Value::Null) => return None,
        Some(Value::String(text)) => text.trim().to_string(),
        Some(other) => crate::expr::stringify(other),
    };
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// String com valor padrao.
pub(crate) fn param_str_or(params: &Value, key: &str, fallback: &str) -> String {
    param_str(params, key).unwrap_or_else(|| fallback.to_string())
}

/// Booleano tolerante: aceita `true`, `"true"` e numeros.
pub(crate) fn param_bool(params: &Value, key: &str, fallback: bool) -> bool {
    match params.get(key) {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::String(text)) => matches!(text.trim().to_lowercase().as_str(), "true" | "1" | "sim"),
        Some(Value::Number(number)) => number.as_f64().map(|n| n != 0.0).unwrap_or(fallback),
        _ => fallback,
    }
}

/// Inteiro tolerante: aceita numero ou string numerica.
pub(crate) fn param_u64(params: &Value, key: &str, fallback: u64) -> u64 {
    match params.get(key) {
        Some(Value::Number(number)) => number.as_u64().unwrap_or(fallback),
        Some(Value::String(text)) => text.trim().parse().unwrap_or(fallback),
        _ => fallback,
    }
}

/// Objeto de um parametro, vazio quando ausente ou de outro tipo.
pub(crate) fn param_object(params: &Value, key: &str) -> Map<String, Value> {
    params
        .get(key)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

/// Lista de um parametro, vazia quando ausente ou de outro tipo.
pub(crate) fn param_array(params: &Value, key: &str) -> Vec<Value> {
    params
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builtin_registry_exposes_the_documented_kinds() {
        let registry = builtin_registry();
        for kind in [
            "trigger.manual",
            "trigger.webhook",
            "trigger.schedule",
            "http.request",
            "data.set",
            "debug.log",
            "flow.if",
            "flow.merge",
            "agent.run",
            "tool.call",
        ] {
            assert!(registry.contains(kind), "faltou o no {kind}");
        }
    }

    #[test]
    fn every_descriptor_declares_its_default_parameters_as_an_object() {
        for descriptor in builtin_registry().catalog() {
            assert!(
                descriptor.defaults.is_object(),
                "{} tem defaults que nao sao objeto",
                descriptor.kind
            );
            assert!(
                !descriptor.outputs.is_empty(),
                "{} nao declara saida",
                descriptor.kind
            );
        }
    }

    #[test]
    fn trigger_descriptors_have_no_inputs() {
        for descriptor in builtin_registry().catalog() {
            let is_trigger_kind = descriptor.kind.starts_with("trigger.");
            assert_eq!(
                is_trigger_kind,
                descriptor.is_trigger(),
                "{} tem entradas incoerentes com o tipo",
                descriptor.kind
            );
        }
    }

    #[test]
    fn provider_model_and_tool_fields_are_resolved_by_the_ui() {
        use crate::registry::OptionsSource;
        let registry = builtin_registry();

        let agent = registry.get("agent.run").unwrap().descriptor();
        let source_of = |kind: &crate::registry::NodeDescriptor, key: &str| {
            kind.fields
                .iter()
                .find(|field| field.key == key)
                .unwrap_or_else(|| panic!("campo {key} ausente"))
                .options_source
        };
        assert_eq!(
            source_of(&agent, "provider"),
            Some(OptionsSource::AgentProviders)
        );
        assert_eq!(
            source_of(&agent, "model_id"),
            Some(OptionsSource::AgentModels)
        );

        let tool = registry.get("tool.call").unwrap().descriptor();
        assert_eq!(source_of(&tool, "tool"), Some(OptionsSource::FlowTools));
    }

    #[test]
    fn inherited_settings_are_hidden_under_advanced() {
        let registry = builtin_registry();
        let advanced_keys = |kind: &str| {
            registry
                .get(kind)
                .unwrap()
                .descriptor()
                .fields
                .into_iter()
                .filter(|field| field.advanced)
                .map(|field| field.key)
                .collect::<Vec<_>>()
        };

        // Base URL vem do provedor selecionado; workspace vem do agente.
        assert!(advanced_keys("agent.run").contains(&"base_url".to_string()));
        assert!(advanced_keys("tool.call").contains(&"workspace_root".to_string()));
    }

    #[test]
    fn every_field_key_exists_in_the_node_defaults() {
        // Um campo sem chave correspondente nos defaults apareceria vazio na UI
        // e nunca seria enviado ao motor.
        for descriptor in builtin_registry().catalog() {
            let defaults = descriptor.defaults.as_object().expect("defaults objeto");
            for field in &descriptor.fields {
                assert!(
                    defaults.contains_key(&field.key),
                    "{} declara o campo `{}` sem default",
                    descriptor.kind,
                    field.key
                );
            }
        }
    }

    #[test]
    fn param_helpers_are_tolerant_with_types() {
        let params = json!({
            "flag_text": "true",
            "flag_num": 1,
            "count_text": "7",
            "blank": "   "
        });
        assert!(param_bool(&params, "flag_text", false));
        assert!(param_bool(&params, "flag_num", false));
        assert!(param_bool(&params, "ausente", true));
        assert_eq!(param_u64(&params, "count_text", 0), 7);
        assert_eq!(param_u64(&params, "ausente", 5), 5);
        assert_eq!(param_str(&params, "blank"), None);
        assert_eq!(param_str_or(&params, "blank", "x"), "x");
    }

    #[test]
    fn param_str_accepts_values_produced_by_a_template() {
        // `"{{ $json.id }}"` com id numerico resolve para numero, nao string.
        let params = json!({ "message": 42, "flag": true, "nada": null });
        assert_eq!(param_str(&params, "message"), Some("42".to_string()));
        assert_eq!(param_str(&params, "flag"), Some("true".to_string()));
        assert_eq!(param_str(&params, "nada"), None);
        assert_eq!(param_str(&params, "ausente"), None);
    }
}
