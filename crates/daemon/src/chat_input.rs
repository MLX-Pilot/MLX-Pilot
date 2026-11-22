//! Validation shared by the HTTP chat and agent entry points, before work starts.
use crate::agent_api::AgentRunRequest;
use mlx_ollama_core::{ChatRequest, MessageRole};

pub(crate) const CLOUD_PROVIDERS: [&str; 5] =
    ["deepseek", "openai", "anthropic", "groq", "openrouter"];

pub(crate) fn validate_model_id(raw: &str) -> Result<(), &'static str> {
    if raw.chars().any(char::is_control) {
        return Err("model_id contains a control character");
    }
    let (display_name, _) = crate::normalize_display_model_id(raw);
    let id = match display_name.split_once("::") {
        Some(("mlx" | "llama" | "ollama", name)) => name,
        _ => match display_name.split_once(':') {
            Some((provider, name)) if CLOUD_PROVIDERS.contains(&provider) => name,
            _ => display_name.as_str(),
        },
    };
    if id.trim().is_empty() {
        return Err("model_id cannot be empty");
    }
    Ok(())
}

fn text(value: &str) -> Result<(), &'static str> {
    if value
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
    {
        return Err("message contains a control character");
    }
    Ok(())
}

fn identifier(value: &str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err("invalid tool identifier");
    }
    Ok(())
}

pub(crate) fn validate_chat(request: &ChatRequest) -> Result<(), &'static str> {
    validate_model_id(&request.model_id)?;
    if request.messages.is_empty() {
        return Err("messages cannot be empty");
    }
    for turn in &request.messages {
        text(&turn.content)?;
        if let Some(reasoning) = &turn.reasoning {
            text(reasoning)?;
        }
        if !turn.tool_calls.is_empty() && turn.role != MessageRole::Assistant {
            return Err("tool_calls require an assistant message");
        }
        for call in &turn.tool_calls {
            identifier(&call.id)?;
            identifier(&call.name)?;
            text(&call.arguments)?;
            serde_json::from_str::<serde::de::IgnoredAny>(&call.arguments)
                .map_err(|_| "tool arguments must be JSON")?;
        }
        if let Some(id) = &turn.tool_call_id {
            if turn.role != MessageRole::Tool {
                return Err("tool_call_id requires a tool message");
            }
            identifier(id)?;
        }
        // An assistant may return only calls or reasoning. A tool is allowed to
        // complete successfully with no output (for example a write operation).
        let has_content = match turn.role {
            MessageRole::Assistant => !turn.is_blank(),
            MessageRole::Tool => turn.tool_call_id.is_some() || !turn.content.trim().is_empty(),
            MessageRole::System | MessageRole::User => !turn.content.trim().is_empty(),
        };
        if !has_content {
            return Err("message cannot be empty");
        }
    }
    Ok(())
}

pub(crate) fn validate_agent(request: &AgentRunRequest) -> Result<(), &'static str> {
    text(&request.message)?;
    if request.message.trim().is_empty() {
        return Err("message cannot be empty");
    }
    if let Some(prompt) = &request.system_prompt {
        text(prompt)?;
    }
    for provider in [&request.provider, &request.fallback_provider]
        .into_iter()
        .flatten()
    {
        if provider.chars().any(char::is_control) {
            return Err("provider contains a control character");
        }
    }
    for model in [&request.model_id, &request.fallback_model_id]
        .into_iter()
        .flatten()
    {
        // A missing or space-only optional override selects the configured model.
        if !model.trim().is_empty() || model.chars().any(char::is_control) {
            validate_model_id(model)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlx_ollama_core::{ChatMessage, ToolCallRequest};
    use serde_json::json;

    fn chat(model: &str, messages: Vec<ChatMessage>) -> ChatRequest {
        ChatRequest {
            model_id: model.into(),
            messages,
            options: Default::default(),
        }
    }

    #[test]
    fn validates_unicode_markdown_and_complete_tool_turns_without_rewriting() {
        let content = "    título 🧪\r\n\tこんにちは\n\n```rust\nlet x = 1;\n```\n";
        let mut assistant = ChatMessage::text(MessageRole::Assistant, "");
        assistant.tool_calls.push(ToolCallRequest {
            id: "call_1".into(),
            name: "ler_nota".into(),
            arguments: "{\n  \"título\": \"ação\"\n}".into(),
        });
        let mut reasoning = ChatMessage::text(MessageRole::Assistant, "");
        reasoning.reasoning = Some("Preparando a resposta.".into());
        let request = chat(
            "mlx::coleção/modelo [MLX]",
            vec![
                ChatMessage::text(MessageRole::User, content),
                assistant,
                ChatMessage::tool_result("call_1", ""),
                reasoning,
            ],
        );
        assert_eq!(validate_chat(&request), Ok(()));
        assert_eq!(request.messages[0].content, content);
        for valid in [
            "ollama::qwen3:8b",
            "llama::Meu Modelo.gguf",
            "openrouter:org/model",
            "modelo-東京",
        ] {
            assert!(validate_model_id(valid).is_ok());
        }
    }

    #[test]
    fn rejects_invalid_models_and_conversation_fields() {
        for model in [
            "",
            "   ",
            "ollama::",
            "mlx:: [MLX]",
            "openai:",
            "id\nlinha",
            "id\0",
        ] {
            assert!(validate_model_id(model).is_err());
        }
        assert_eq!(
            validate_chat(&chat("local", vec![])),
            Err("messages cannot be empty")
        );
        for (content, expected) in [
            (" \n", "message cannot be empty"),
            ("linha\0final", "message contains a control character"),
        ] {
            assert_eq!(
                validate_chat(&chat(
                    "local",
                    vec![ChatMessage::text(MessageRole::User, content)]
                )),
                Err(expected)
            );
        }
        let mut invalid = ChatMessage::text(MessageRole::Assistant, "");
        invalid.tool_calls.push(ToolCallRequest {
            id: "call_1".into(),
            name: "ler_nota".into(),
            arguments: "incompleto".into(),
        });
        assert_eq!(
            validate_chat(&chat("local", vec![invalid])),
            Err("tool arguments must be JSON")
        );
    }

    #[test]
    fn agent_validates_overrides_and_preserves_optional_model_defaults() {
        let mut request: AgentRunRequest =
            serde_json::from_value(json!({"message":"    código\n\tárvore", "model_id":" "}))
                .unwrap();
        assert!(validate_agent(&request).is_ok());
        request.system_prompt = Some("nota\0".into());
        assert_eq!(
            validate_agent(&request),
            Err("message contains a control character")
        );
        request.system_prompt = None;
        request.fallback_model_id = Some("modelo\rseparado".into());
        assert_eq!(
            validate_agent(&request),
            Err("model_id contains a control character")
        );
    }
}
