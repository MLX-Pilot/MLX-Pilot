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
}

impl ChatMessage {
    /// Create a simple text message (no tool calls).
    pub fn text(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    /// Create a tool result message.
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Tool,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
        }
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
