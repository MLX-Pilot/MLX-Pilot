use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GenerationOptions {
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub top_p: Option<f32>,
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
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError>;
}
