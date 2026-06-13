use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use mlx_ollama_core::{
    ChatMessage, ChatRequest, ChatResponse, ChatToolsRequest, GenerationOptions, MessageRole,
    ModelDescriptor, ModelProvider, ProviderError, RuntimeProviderConfig, TokenUsage,
};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

mod runtime;
pub use runtime::{GpuStatus, OllamaRuntimeStatus, RuntimePhase, MANAGED_OLLAMA_VERSION};
use runtime::{OllamaRuntime, OllamaRuntimeConfig};

#[derive(Debug, Clone)]
pub struct OllamaProviderConfig {
    pub base_url: String,
    pub timeout: Duration,
    pub startup_timeout: Duration,
    pub auto_start: bool,
    pub auto_install: bool,
}

impl Default for OllamaProviderConfig {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:11434".to_string(),
            timeout: Duration::from_secs(900),
            startup_timeout: Duration::from_secs(30),
            auto_start: true,
            auto_install: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OllamaProvider {
    cfg: OllamaProviderConfig,
    client: reqwest::Client,
    runtime: Arc<OllamaRuntime>,
}

impl OllamaProvider {
    pub fn new(cfg: OllamaProviderConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(cfg.timeout)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let runtime = Arc::new(OllamaRuntime::new(OllamaRuntimeConfig::from_provider(
            cfg.base_url.clone(),
            cfg.startup_timeout,
            cfg.auto_install,
            cfg.auto_start,
        )));
        Self {
            cfg,
            client,
            runtime,
        }
    }

    pub fn config(&self) -> &OllamaProviderConfig {
        &self.cfg
    }

    pub async fn prepare_runtime(
        &self,
        selected_model: Option<String>,
    ) -> Result<(), ProviderError> {
        self.runtime.prepare(selected_model).await
    }

    pub async fn runtime_status(&self) -> OllamaRuntimeStatus {
        self.runtime.status().await
    }

    pub fn cancel_runtime_operation(&self) {
        self.runtime.cancel();
    }

    async fn runner_failure(&self, detail: &str) -> ProviderError {
        ProviderError::Unavailable {
            details: self.runtime.diagnostic_detail(detail).await,
        }
    }

    fn endpoint(&self, path: &str) -> Result<String, ProviderError> {
        self.endpoint_with_runtime(path, None)
    }

    fn endpoint_with_runtime(
        &self,
        path: &str,
        runtime: Option<&RuntimeProviderConfig>,
    ) -> Result<String, ProviderError> {
        let base = runtime
            .and_then(|cfg| cfg.base_url.as_deref())
            .unwrap_or(self.cfg.base_url.as_str())
            .trim();
        if base.is_empty() {
            return Err(ProviderError::InvalidRequest {
                details: "APP_OLLAMA_BASE_URL nao pode ser vazio".to_string(),
            });
        }

        Ok(format!(
            "{}/{}",
            base.trim_end_matches('/'),
            path.trim_start_matches('/')
        ))
    }

    fn http_error(status: StatusCode, body: String) -> ProviderError {
        let detail = body.trim();
        if status == StatusCode::NOT_FOUND {
            return ProviderError::ModelNotFound {
                model_id: if detail.is_empty() {
                    "modelo nao encontrado".to_string()
                } else {
                    detail.to_string()
                },
            };
        }

        ProviderError::Unavailable {
            details: format!("ollama respondeu HTTP {status}: {detail}"),
        }
    }

    fn map_network_error(error: reqwest::Error) -> ProviderError {
        if error.is_timeout() {
            return ProviderError::Timeout { seconds: 900 };
        }

        ProviderError::Io {
            context: "falha de rede com Ollama".to_string(),
            source: io::Error::other(error.to_string()),
        }
    }

    async fn ensure_ready(&self) -> Result<(), ProviderError> {
        self.runtime.ensure_ready_for_model(None).await
    }

    async fn ensure_ready_for_model(&self, model_id: &str) -> Result<(), ProviderError> {
        self.runtime.ensure_ready_for_model(Some(model_id)).await
    }

    async fn ensure_ready_with_runtime(
        &self,
        runtime: Option<&RuntimeProviderConfig>,
    ) -> Result<(), ProviderError> {
        if runtime
            .and_then(|cfg| cfg.base_url.as_deref())
            .filter(|value| !value.trim().is_empty())
            .is_some()
        {
            if self
                .ping_server_with_timeout(Duration::from_secs(2), runtime)
                .await
            {
                return Ok(());
            }
            return Err(ProviderError::Unavailable {
                details: "runtime Ollama base_url did not respond".to_string(),
            });
        }

        self.ensure_ready().await
    }

    async fn ping_server_with_timeout(
        &self,
        timeout: Duration,
        runtime: Option<&RuntimeProviderConfig>,
    ) -> bool {
        let endpoint = match self.endpoint_with_runtime("/api/version", runtime) {
            Ok(value) => value,
            Err(_) => return false,
        };

        let client = match reqwest::Client::builder().timeout(timeout).build() {
            Ok(client) => client,
            Err(_) => return false,
        };

        client
            .get(endpoint)
            .send()
            .await
            .map(|response| response.status().is_success())
            .unwrap_or(false)
    }

    fn apply_runtime_headers(
        &self,
        builder: reqwest::RequestBuilder,
        runtime: Option<&RuntimeProviderConfig>,
    ) -> reqwest::RequestBuilder {
        let mut out = builder;
        if let Some(runtime) = runtime {
            for (key, value) in &runtime.headers {
                out = out.header(key, value);
            }
            if let Some(api_key) = runtime
                .api_key
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                out = out.bearer_auth(api_key);
            }
        }
        out
    }
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    #[serde(default)]
    models: Vec<OllamaTagEntry>,
}

#[derive(Debug, Deserialize)]
struct OllamaTagEntry {
    #[serde(default)]
    name: String,
    #[serde(default)]
    model: String,
}

#[derive(Debug, Serialize)]
struct OllamaChatRequestBody {
    model: String,
    messages: Vec<OllamaChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    think: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    tools: Vec<OllamaTool>,
}

#[derive(Debug, Serialize)]
struct OllamaTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: OllamaFunction,
}

#[derive(Debug, Serialize)]
struct OllamaFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct OllamaChatMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    tool_calls: Vec<OllamaToolCall>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaToolCall {
    function: OllamaToolCallFunction,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaToolCallFunction {
    name: String,
    arguments: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "num_predict")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
}

#[derive(Debug, Deserialize, Serialize)]
struct OllamaChatResponseBody {
    #[serde(default)]
    model: String,
    #[serde(default)]
    message: Option<OllamaChatResponseMessage>,
    #[serde(default)]
    prompt_eval_count: Option<usize>,
    #[serde(default)]
    eval_count: Option<usize>,
    #[serde(default)]
    total_duration: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
struct OllamaChatResponseMessage {
    #[serde(default)]
    content: String,
    #[serde(default)]
    tool_calls: Option<Vec<OllamaToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OllamaErrorBody {
    #[serde(default)]
    error: String,
}

#[async_trait]
impl ModelProvider for OllamaProvider {
    fn provider_id(&self) -> &'static str {
        "ollama"
    }

    async fn list_models(&self) -> Result<Vec<ModelDescriptor>, ProviderError> {
        self.list_models_with_runtime(None).await
    }

    async fn list_models_with_runtime(
        &self,
        runtime: Option<RuntimeProviderConfig>,
    ) -> Result<Vec<ModelDescriptor>, ProviderError> {
        if !self
            .ping_server_with_timeout(Duration::from_millis(400), runtime.as_ref())
            .await
        {
            return Ok(Vec::new());
        }

        let endpoint = self.endpoint_with_runtime("/api/tags", runtime.as_ref())?;
        let response = self
            .apply_runtime_headers(self.client.get(&endpoint), runtime.as_ref())
            .send()
            .await
            .map_err(Self::map_network_error)?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Self::http_error(status, body));
        }

        let payload: OllamaTagsResponse =
            response.json().await.map_err(|error| ProviderError::Io {
                context: "falha parseando /api/tags".to_string(),
                source: io::Error::other(error.to_string()),
            })?;

        let mut models = payload
            .models
            .into_iter()
            .filter_map(|entry| {
                let id = if !entry.model.trim().is_empty() {
                    entry.model.trim().to_string()
                } else {
                    entry.name.trim().to_string()
                };

                if id.is_empty() {
                    return None;
                }

                let name = if !entry.name.trim().is_empty() {
                    entry.name.trim().to_string()
                } else {
                    id.clone()
                };

                Some(ModelDescriptor {
                    id: id.clone(),
                    name,
                    provider: self.provider_id().to_string(),
                    path: id,
                    is_available: true,
                    agent_tool_mode: None,
                    agent_tool_reason: None,
                    agent_recommended: false,
                })
            })
            .collect::<Vec<_>>();

        models.sort_by_key(|a| a.name.to_lowercase());
        Ok(models)
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        self.do_chat(
            &request.model_id,
            &request.messages,
            &request.options,
            None,
            None,
        )
        .await
    }

    async fn chat_with_runtime(
        &self,
        request: ChatRequest,
        runtime: Option<RuntimeProviderConfig>,
    ) -> Result<ChatResponse, ProviderError> {
        self.do_chat(
            &request.model_id,
            &request.messages,
            &request.options,
            None,
            runtime.as_ref(),
        )
        .await
    }

    async fn chat_with_tools(
        &self,
        request: ChatToolsRequest,
    ) -> Result<ChatResponse, ProviderError> {
        self.do_chat(
            &request.model_id,
            &request.messages,
            &request.options,
            Some(request.tools),
            None,
        )
        .await
    }

    async fn chat_with_tools_with_runtime(
        &self,
        request: ChatToolsRequest,
        runtime: Option<RuntimeProviderConfig>,
    ) -> Result<ChatResponse, ProviderError> {
        self.do_chat(
            &request.model_id,
            &request.messages,
            &request.options,
            Some(request.tools),
            runtime.as_ref(),
        )
        .await
    }
}

impl OllamaProvider {
    pub async fn begin_chat_stream(
        &self,
        model_id: &str,
        messages: &[ChatMessage],
        options: &GenerationOptions,
    ) -> Result<reqwest::Response, ProviderError> {
        let mut body = self.build_chat_request(model_id, messages, options, None)?;
        body.stream = true;
        body.think = Some(true);
        self.ensure_ready_for_model(model_id).await?;

        let endpoint = self.endpoint("/api/chat")?;
        let response = self
            .client
            .post(&endpoint)
            .json(&body)
            .send()
            .await
            .map_err(Self::map_network_error)?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            let detail = serde_json::from_str::<OllamaErrorBody>(&text)
                .ok()
                .map(|value| value.error)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(text);
            if status.is_server_error() {
                return Err(self.runner_failure(&detail).await);
            }
            return Err(Self::http_error(status, detail));
        }
        Ok(response)
    }

    async fn do_chat(
        &self,
        model_id: &str,
        messages: &[ChatMessage],
        options: &GenerationOptions,
        tools: Option<Vec<mlx_ollama_core::FunctionDef>>,
        runtime: Option<&RuntimeProviderConfig>,
    ) -> Result<ChatResponse, ProviderError> {
        if messages.is_empty() {
            return Err(ProviderError::InvalidRequest {
                details: "messages cannot be empty".to_string(),
            });
        }

        let body = self.build_chat_request(model_id, messages, options, tools)?;
        if runtime.is_some() {
            self.ensure_ready_with_runtime(runtime).await?;
        } else {
            self.ensure_ready_for_model(model_id).await?;
        }

        let endpoint = self.endpoint_with_runtime("/api/chat", runtime)?;
        let started = Instant::now();

        let response = self
            .apply_runtime_headers(self.client.post(&endpoint), runtime)
            .json(&body)
            .send()
            .await
            .map_err(Self::map_network_error)?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            let detail = serde_json::from_str::<OllamaErrorBody>(&text)
                .ok()
                .map(|value| value.error)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(text);
            if status.is_server_error() {
                return Err(self.runner_failure(&detail).await);
            }
            return Err(Self::http_error(status, detail));
        }

        let payload: OllamaChatResponseBody =
            response.json().await.map_err(|error| ProviderError::Io {
                context: "falha parseando /api/chat".to_string(),
                source: io::Error::other(error.to_string()),
            })?;

        let content = payload
            .message
            .as_ref()
            .map(|message| message.content.trim().to_string())
            .unwrap_or_default();

        let mut tool_calls = Vec::new();
        if let Some(msg) = &payload.message {
            if let Some(calls) = &msg.tool_calls {
                for call in calls {
                    tool_calls.push(mlx_ollama_core::ToolCallRequest {
                        id: uuid::Uuid::new_v4().to_string(),
                        name: call.function.name.clone(),
                        arguments: serde_json::to_string(&call.function.arguments)
                            .unwrap_or_default(),
                    });
                }
            }
        }

        let prompt_tokens = payload.prompt_eval_count.unwrap_or(0);
        let completion_tokens = payload
            .eval_count
            .unwrap_or_else(|| content.split_whitespace().count());
        let usage = TokenUsage {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
        };

        let latency_ms = payload
            .total_duration
            .map(|nanos| nanos / 1_000_000)
            .unwrap_or_else(|| started.elapsed().as_millis() as u64);

        let raw_output = serde_json::to_string(&payload).ok();

        let chat_message = ChatMessage {
            role: MessageRole::Assistant,
            content,
            tool_calls,
            tool_call_id: None,
        };

        Ok(ChatResponse {
            model_id: if payload.model.trim().is_empty() {
                model_id.to_string()
            } else {
                payload.model
            },
            provider: self.provider_id().to_string(),
            message: chat_message,
            usage,
            latency_ms,
            raw_output,
        })
    }

    fn build_chat_request(
        &self,
        model_id: &str,
        messages: &[ChatMessage],
        options: &GenerationOptions,
        tools: Option<Vec<mlx_ollama_core::FunctionDef>>,
    ) -> Result<OllamaChatRequestBody, ProviderError> {
        if messages.is_empty() {
            return Err(ProviderError::InvalidRequest {
                details: "messages cannot be empty".to_string(),
            });
        }

        let mut mapped_messages = Vec::with_capacity(messages.len());
        for message in messages {
            let role = match message.role {
                MessageRole::System => "system".to_string(),
                MessageRole::User => "user".to_string(),
                MessageRole::Assistant => "assistant".to_string(),
                MessageRole::Tool => "tool".to_string(),
            };

            let tool_calls = message
                .tool_calls
                .iter()
                .map(|tc| OllamaToolCall {
                    function: OllamaToolCallFunction {
                        name: tc.name.clone(),
                        arguments: serde_json::from_str(&tc.arguments).unwrap_or_default(),
                    },
                })
                .collect();

            mapped_messages.push(OllamaChatMessage {
                role,
                content: message.content.clone(),
                tool_calls,
            });
        }

        let mapped_tools = tools
            .unwrap_or_default()
            .into_iter()
            .map(|f| OllamaTool {
                tool_type: "function".to_string(),
                function: OllamaFunction {
                    name: f.name,
                    description: f.description,
                    parameters: f.parameters,
                },
            })
            .collect();

        Ok(OllamaChatRequestBody {
            model: model_id.to_string(),
            messages: mapped_messages,
            stream: false,
            think: None,
            options: Some(OllamaOptions {
                temperature: options.temperature,
                max_tokens: options.max_tokens,
                top_p: options.top_p,
            }),
            tools: mapped_tools,
        })
    }
}

/// On Windows, stop spawned console child processes (ollama, winget, ...) from flashing
/// a black terminal window. No-op on other platforms.
pub(crate) fn silence_console(command: &mut Command) {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_serialization() {
        let tools = vec![OllamaTool {
            tool_type: "function".to_string(),
            function: OllamaFunction {
                name: "get_weather".to_string(),
                description: "Get the weather".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "location": { "type": "string" }
                    }
                }),
            },
        }];

        let json = serde_json::to_string(&tools).unwrap();
        assert!(json.contains("get_weather"));
        assert!(json.contains("Get the weather"));
        assert!(json.contains("location"));
    }

    #[test]
    fn test_tool_call_deserialization() {
        let response_json = r#"{
            "model": "llama3.1",
            "message": {
                "role": "assistant",
                "content": "",
                "tool_calls": [
                    {
                        "function": {
                            "name": "get_weather",
                            "arguments": {
                                "location": "San Francisco"
                            }
                        }
                    }
                ]
            },
            "done": true
        }"#;

        let payload: OllamaChatResponseBody = serde_json::from_str(response_json).unwrap();
        let message = payload.message.unwrap();
        let tool_calls = message.tool_calls.unwrap();

        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].function.name, "get_weather");
        assert_eq!(
            tool_calls[0].function.arguments["location"]
                .as_str()
                .unwrap(),
            "San Francisco"
        );
    }

    #[test]
    fn runtime_endpoint_override_uses_runtime_base_url() {
        let provider = OllamaProvider::new(OllamaProviderConfig::default());
        let runtime = RuntimeProviderConfig {
            base_url: Some("http://127.0.0.1:22445".to_string()),
            api_key: None,
            headers: Default::default(),
        };
        let endpoint = provider
            .endpoint_with_runtime("/api/chat", Some(&runtime))
            .unwrap();
        assert_eq!(endpoint, "http://127.0.0.1:22445/api/chat");
    }
}
