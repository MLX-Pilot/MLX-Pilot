use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    /// Tool result message (contains the output of a tool call).
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
    /// Tool calls requested by the assistant (only set when role=Assistant).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCallRequest>,
    /// The tool_call_id this message is responding to (only set when role=Tool).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Raciocínio separado do conteúdo, quando o modelo produz os dois.
    ///
    /// O Ollama devolve isso no campo `thinking` para modelos como a família Qwen3.
    /// Guardar separado importa por dois motivos: um turno com raciocínio e `content`
    /// vazio **não** é um turno vazio, e o raciocínio pode ser transmitido à UI sem
    /// virar a resposta final do usuário.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
}

impl ChatMessage {
    /// Create a simple text message (no tool calls).
    pub fn text(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            reasoning: None,
        }
    }

    /// Create a tool result message.
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Tool,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
            reasoning: None,
        }
    }

    /// Se o turno não produziu conteúdo nem raciocínio nem chamada de ferramenta.
    ///
    /// Um turno só de raciocínio **não** conta como vazio: o modelo produziu trabalho,
    /// só não o escreveu como resposta.
    pub fn is_blank(&self) -> bool {
        self.content.trim().is_empty()
            && self.tool_calls.is_empty()
            && self
                .reasoning
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
    }

    /// Se o turno raciocinou mas não escreveu resposta nem chamou ferramenta.
    pub fn is_reasoning_only(&self) -> bool {
        self.content.trim().is_empty()
            && self.tool_calls.is_empty()
            && self
                .reasoning
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
    }
}

// ── Tool-calling types ─────────────────────────────────────────────

/// A tool call requested by the LLM.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallRequest {
    /// Unique ID for this call (used to correlate with ToolResult).
    pub id: String,
    /// Name of the tool/function to invoke.
    pub name: String,
    /// JSON-encoded arguments.
    pub arguments: String,
}

/// Function definition for LLM tool-calling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// A chat request that includes tool definitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatToolsRequest {
    pub model_id: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub tools: Vec<FunctionDef>,
    #[serde(default)]
    pub options: GenerationOptions,
}

/// Runtime overrides used by remote providers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeProviderConfig {
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

// ── Generation options ─────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GenerationOptions {
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub airllm_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model_id: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub options: GenerationOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub model_id: String,
    pub provider: String,
    pub message: ChatMessage,
    pub usage: TokenUsage,
    pub latency_ms: u64,
    pub raw_output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDescriptor {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub path: String,
    pub is_available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_tool_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_tool_reason: Option<String>,
    #[serde(default)]
    pub agent_recommended: bool,
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("invalid request: {details}")]
    InvalidRequest { details: String },
    #[error("model not found: {model_id}")]
    ModelNotFound { model_id: String },
    #[error("i/o error in {context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
    #[error("provider command failed ({command}): {stderr}")]
    CommandFailed { command: String, stderr: String },
    #[error("provider timed out after {seconds}s")]
    Timeout { seconds: u64 },
    #[error("provider unavailable: {details}")]
    Unavailable { details: String },
}

#[async_trait]
pub trait ModelProvider: Send + Sync {
    fn provider_id(&self) -> &'static str;
    async fn list_models(&self) -> Result<Vec<ModelDescriptor>, ProviderError>;
    async fn list_models_with_runtime(
        &self,
        runtime: Option<RuntimeProviderConfig>,
    ) -> Result<Vec<ModelDescriptor>, ProviderError> {
        let _ = runtime;
        self.list_models().await
    }
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError>;
    async fn chat_with_runtime(
        &self,
        request: ChatRequest,
        runtime: Option<RuntimeProviderConfig>,
    ) -> Result<ChatResponse, ProviderError> {
        let _ = runtime;
        self.chat(request).await
    }

    /// Chat with tool-calling support.
    ///
    /// Default implementation returns `Unavailable`. Providers that support
    /// tool-calling (e.g. Ollama with function calling) should override this.
    async fn chat_with_tools(
        &self,
        request: ChatToolsRequest,
    ) -> Result<ChatResponse, ProviderError> {
        let _ = request;
        Err(ProviderError::Unavailable {
            details: format!(
                "provider '{}' does not support tool-calling",
                self.provider_id()
            ),
        })
    }

    /// Chat with tool-calling support and runtime overrides.
    async fn chat_with_tools_with_runtime(
        &self,
        request: ChatToolsRequest,
        runtime: Option<RuntimeProviderConfig>,
    ) -> Result<ChatResponse, ProviderError> {
        let _ = runtime;
        self.chat_with_tools(request).await
    }
}

#[cfg(test)]
mod chat_message_tests {
    use super::*;

    #[test]
    fn reasoning_only_turn_is_not_blank() {
        // Regressao: um turno em que o modelo so raciocinou chegava ao agente como
        // conteudo vazio, e o usuario recebia "" como resposta.
        let message = ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            reasoning: Some("O usuario quer a soma de 17 e 25.".to_string()),
        };

        assert!(!message.is_blank(), "raciocinio e trabalho, nao vazio");
        assert!(message.is_reasoning_only());
    }

    #[test]
    fn truly_empty_turn_is_blank() {
        let message = ChatMessage::text(MessageRole::Assistant, "   ");
        assert!(message.is_blank());
        assert!(!message.is_reasoning_only());
    }

    #[test]
    fn turn_with_content_is_neither_blank_nor_reasoning_only() {
        let mut message = ChatMessage::text(MessageRole::Assistant, "42");
        message.reasoning = Some("pensei bastante".to_string());
        assert!(!message.is_blank());
        assert!(!message.is_reasoning_only());
    }

    #[test]
    fn whitespace_reasoning_does_not_count() {
        let mut message = ChatMessage::text(MessageRole::Assistant, "");
        message.reasoning = Some("   \n ".to_string());
        assert!(message.is_blank());
        assert!(!message.is_reasoning_only());
    }

    #[test]
    fn reasoning_is_omitted_from_the_wire_when_absent() {
        let message = ChatMessage::text(MessageRole::User, "oi");
        let json = serde_json::to_string(&message).unwrap();
        assert!(!json.contains("reasoning"), "json: {json}");
    }
}
