use std::collections::BTreeMap;
use std::io;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use mlx_ollama_core::{
    ChatMessage, ChatRequest, ChatResponse, ChatToolsRequest, MessageRole, ModelDescriptor,
    ModelProvider, ProviderError, RuntimeProviderConfig, TokenUsage, ToolCallRequest,
};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HttpApiKind {
    OpenAiCompatible,
    Anthropic,
}

#[derive(Debug, Clone)]
pub struct HttpLlmProviderConfig {
    pub provider_name: String,
    pub api_kind: HttpApiKind,
    pub base_url: String,
    pub api_key: Option<String>,
    pub default_headers: BTreeMap<String, String>,
    pub timeout: Duration,
    pub default_models: Vec<String>,
}

impl Default for HttpLlmProviderConfig {
    fn default() -> Self {
        Self {
            provider_name: "http".to_string(),
            api_kind: HttpApiKind::OpenAiCompatible,
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: None,
            default_headers: BTreeMap::new(),
            timeout: Duration::from_secs(120),
            default_models: vec!["gpt-4o-mini".to_string()],
        }
    }
}

#[derive(Debug, Clone)]
pub struct HttpLlmProvider {
    cfg: HttpLlmProviderConfig,
    client: reqwest::Client,
}

impl HttpLlmProvider {
    pub fn new(cfg: HttpLlmProviderConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(cfg.timeout)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { cfg, client }
    }

    fn merge_runtime(&self, runtime: Option<RuntimeProviderConfig>) -> EffectiveRuntime {
        let mut base_url = self.cfg.base_url.clone();
        let mut api_key = self.cfg.api_key.clone();
        let mut headers = self.cfg.default_headers.clone();

        if let Some(runtime_cfg) = runtime {
            if let Some(v) = runtime_cfg.base_url {
                let trimmed = v.trim();
                if !trimmed.is_empty() {
                    base_url = trimmed.to_string();
                }
            }
            if let Some(v) = runtime_cfg.api_key {
                let trimmed = v.trim();
                if !trimmed.is_empty() {
                    api_key = Some(trimmed.to_string());
                }
            }
            for (k, v) in runtime_cfg.headers {
                if !k.trim().is_empty() {
                    headers.insert(k, v);
                }
            }
        }

        EffectiveRuntime {
            base_url,
            api_key,
            headers,
        }
    }

    fn build_headers(
        &self,
        runtime: &EffectiveRuntime,
        include_json_ct: bool,
    ) -> Result<HeaderMap, ProviderError> {
        let mut headers = HeaderMap::new();
        if include_json_ct {
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        }

        for (name, value) in &runtime.headers {
            let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                ProviderError::InvalidRequest {
                    details: format!("invalid header name: {name}"),
                }
            })?;
            let value =
                HeaderValue::from_str(value).map_err(|_| ProviderError::InvalidRequest {
                    details: format!("invalid header value for {name}"),
                })?;
            headers.insert(name, value);
        }

        match self.cfg.api_kind {
            HttpApiKind::OpenAiCompatible => {
                if let Some(key) = runtime.api_key.as_deref() {
                    let value = format!("Bearer {key}");
                    let value = HeaderValue::from_str(&value).map_err(|_| {
                        ProviderError::InvalidRequest {
                            details: "invalid api key".to_string(),
                        }
                    })?;
                    headers.insert(AUTHORIZATION, value);
                }
            }
            HttpApiKind::Anthropic => {
                if let Some(key) = runtime.api_key.as_deref() {
                    let value =
                        HeaderValue::from_str(key).map_err(|_| ProviderError::InvalidRequest {
                            details: "invalid anthropic api key".to_string(),
                        })?;
                    headers.insert("x-api-key", value);
                }

                if !headers.contains_key("anthropic-version") {
                    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
                }
            }
        }

        Ok(headers)
    }

    fn endpoint(base_url: &str, path: &str) -> String {
        format!(
            "{}/{}",
            base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    fn parse_error(status: StatusCode, body: &str) -> ProviderError {
        let message = serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| {
                v.pointer("/error/message")
                    .and_then(Value::as_str)
                    .or_else(|| v.get("error").and_then(Value::as_str))
                    .map(ToString::to_string)
            })
            .unwrap_or_else(|| body.trim().to_string());

        if status == StatusCode::NOT_FOUND {
            return ProviderError::ModelNotFound { model_id: message };
        }

        ProviderError::Unavailable {
            details: format!("remote provider HTTP {status}: {message}"),
        }
    }

    fn map_network_error(error: reqwest::Error) -> ProviderError {
        if error.is_timeout() {
            return ProviderError::Timeout { seconds: 120 };
        }
        ProviderError::Io {
            context: "http llm network failure".to_string(),
            source: io::Error::other(error.to_string()),
        }
    }

    async fn list_openai_models(
        &self,
        runtime: &EffectiveRuntime,
    ) -> Result<Vec<ModelDescriptor>, ProviderError> {
        let endpoint = Self::endpoint(&runtime.base_url, "/models");
        let headers = self.build_headers(runtime, false)?;
        let response = self
            .client
            .get(endpoint)
            .headers(headers)
            .send()
            .await
            .map_err(Self::map_network_error)?;

        if !response.status().is_success() {
            let status = response.status();
            let _body = response.text().await.unwrap_or_default();
            debug!(
                provider = %self.cfg.provider_name,
                status = %status,
                "failed to list remote models"
            );
            return Ok(self.default_models());
        }

        let payload: OpenAiModelsResponse =
            response.json().await.map_err(|e| ProviderError::Io {
                context: "failed to parse /models".to_string(),
                source: io::Error::other(e.to_string()),
            })?;

        if payload.data.is_empty() {
            return Ok(self.default_models());
        }

        let mut models = payload
            .data
            .into_iter()
            .map(|item| ModelDescriptor {
                id: item.id.clone(),
                name: item.id,
                provider: self.cfg.provider_name.clone(),
                path: runtime.base_url.clone(),
                is_available: true,
                agent_tool_mode: None,
                agent_tool_reason: None,
                agent_recommended: false,
            })
            .collect::<Vec<_>>();
        models.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(models)
    }

    fn default_models(&self) -> Vec<ModelDescriptor> {
        self.cfg
            .default_models
            .iter()
            .map(|id| ModelDescriptor {
                id: id.clone(),
                name: id.clone(),
                provider: self.cfg.provider_name.clone(),
                path: self.cfg.base_url.clone(),
                is_available: true,
                agent_tool_mode: None,
                agent_tool_reason: None,
                agent_recommended: false,
            })
            .collect()
    }

    async fn do_openai_chat(
        &self,
        request: ChatToolsRequest,
        runtime: &EffectiveRuntime,
    ) -> Result<ChatResponse, ProviderError> {
        let endpoint = Self::endpoint(&runtime.base_url, "/chat/completions");
        let headers = self.build_headers(runtime, true)?;
        let started = Instant::now();
        let payload = map_openai_request(&request);

        let response = self
            .client
            .post(endpoint)
            .headers(headers)
            .json(&payload)
            .send()
            .await
            .map_err(Self::map_network_error)?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Self::parse_error(status, &body));
        }

        let body: OpenAiResponseBody = response.json().await.map_err(|e| ProviderError::Io {
            context: "failed to parse chat/completions".to_string(),
            source: io::Error::other(e.to_string()),
        })?;
        let raw_output = serde_json::to_string(&body).ok();

        let choice = body
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| ProviderError::Unavailable {
                details: "chat/completions returned no choices".to_string(),
            })?;

        let mut tool_calls = Vec::new();
        if let Some(calls) = choice.message.tool_calls {
            for call in calls {
                let arguments = if call.function.arguments.is_string() {
                    call.function.arguments.as_str().unwrap_or("{}").to_string()
                } else {
                    serde_json::to_string(&call.function.arguments).unwrap_or_else(|_| "{}".into())
                };
                tool_calls.push(ToolCallRequest {
                    id: call.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                    name: call.function.name,
                    arguments,
                });
            }
        }

        let usage = TokenUsage {
            prompt_tokens: body.usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0),
            completion_tokens: body
                .usage
                .as_ref()
                .map(|u| u.completion_tokens)
                .unwrap_or(0),
            total_tokens: body.usage.as_ref().map(|u| u.total_tokens).unwrap_or(0),
        };

        Ok(ChatResponse {
            model_id: request.model_id.clone(),
            provider: self.cfg.provider_name.clone(),
            message: ChatMessage {
                role: MessageRole::Assistant,
                content: choice.message.content.unwrap_or_default(),
                tool_calls,
                tool_call_id: None,
            },
            usage,
            latency_ms: started.elapsed().as_millis() as u64,
            raw_output,
        })
    }

    async fn do_anthropic_chat(
        &self,
        request: ChatToolsRequest,
        runtime: &EffectiveRuntime,
    ) -> Result<ChatResponse, ProviderError> {
        let endpoint = Self::endpoint(&runtime.base_url, "/messages");
        let headers = self.build_headers(runtime, true)?;
        let started = Instant::now();
        let payload = map_anthropic_request(&request);

        let response = self
            .client
            .post(endpoint)
            .headers(headers)
            .json(&payload)
            .send()
            .await
            .map_err(Self::map_network_error)?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Self::parse_error(status, &body));
        }

        let body: AnthropicResponseBody = response.json().await.map_err(|e| ProviderError::Io {
            context: "failed to parse anthropic /messages".to_string(),
            source: io::Error::other(e.to_string()),
        })?;

        let mut content_parts = Vec::new();
        let mut tool_calls = Vec::new();
        for block in &body.content {
            match block {
                AnthropicOutputContent::Text { text } => {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        content_parts.push(trimmed.to_string());
                    }
                }
                AnthropicOutputContent::ToolUse { id, name, input } => {
                    tool_calls.push(ToolCallRequest {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: serde_json::to_string(input).unwrap_or_else(|_| "{}".into()),
                    });
                }
                _ => {}
            }
        }

        let prompt_tokens = body.usage.as_ref().map(|u| u.input_tokens).unwrap_or(0);
        let completion_tokens = body.usage.as_ref().map(|u| u.output_tokens).unwrap_or(0);
        let usage = TokenUsage {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
        };

        Ok(ChatResponse {
            model_id: request.model_id.clone(),
            provider: self.cfg.provider_name.clone(),
            message: ChatMessage {
                role: MessageRole::Assistant,
                content: content_parts.join("\n"),
                tool_calls,
                tool_call_id: None,
            },
            usage,
            latency_ms: started.elapsed().as_millis() as u64,
            raw_output: serde_json::to_string(&body).ok(),
        })
    }
}

#[async_trait]
impl ModelProvider for HttpLlmProvider {
    fn provider_id(&self) -> &'static str {
        "http_llm"
    }

    async fn list_models(&self) -> Result<Vec<ModelDescriptor>, ProviderError> {
        self.list_models_with_runtime(None).await
    }

    async fn list_models_with_runtime(
        &self,
        runtime: Option<RuntimeProviderConfig>,
    ) -> Result<Vec<ModelDescriptor>, ProviderError> {
        let runtime = self.merge_runtime(runtime);
        match self.cfg.api_kind {
            HttpApiKind::OpenAiCompatible => self.list_openai_models(&runtime).await,
            HttpApiKind::Anthropic => Ok(self.default_models()),
        }
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        self.chat_with_runtime(request, None).await
    }

    async fn chat_with_runtime(
        &self,
        request: ChatRequest,
        runtime: Option<RuntimeProviderConfig>,
    ) -> Result<ChatResponse, ProviderError> {
        self.chat_with_tools_with_runtime(
            ChatToolsRequest {
                model_id: request.model_id,
                messages: request.messages,
                tools: Vec::new(),
                options: request.options,
            },
            runtime,
        )
        .await
    }

    async fn chat_with_tools(
        &self,
        request: ChatToolsRequest,
    ) -> Result<ChatResponse, ProviderError> {
        self.chat_with_tools_with_runtime(request, None).await
    }

    async fn chat_with_tools_with_runtime(
        &self,
        request: ChatToolsRequest,
        runtime: Option<RuntimeProviderConfig>,
    ) -> Result<ChatResponse, ProviderError> {
        let runtime = self.merge_runtime(runtime);
        match self.cfg.api_kind {
            HttpApiKind::OpenAiCompatible => self.do_openai_chat(request, &runtime).await,
            HttpApiKind::Anthropic => self.do_anthropic_chat(request, &runtime).await,
        }
    }
}

#[derive(Debug, Clone)]
struct EffectiveRuntime {
    base_url: String,
    api_key: Option<String>,
    headers: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelsResponse {
    #[serde(default)]
    data: Vec<OpenAiModelItem>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelItem {
    id: String,
}

#[derive(Debug, Serialize)]
struct OpenAiRequestBody {
    model: String,
    messages: Vec<OpenAiRequestMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<OpenAiToolDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct OpenAiRequestMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiRequestToolCall>>,
}

#[derive(Debug, Serialize)]
struct OpenAiRequestToolCall {
    #[serde(rename = "type")]
    call_type: String,
    id: String,
    function: OpenAiRequestToolCallFunction,
}

#[derive(Debug, Serialize)]
struct OpenAiRequestToolCallFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize)]
struct OpenAiToolDefinition {
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenAiToolFunction,
}

#[derive(Debug, Serialize)]
struct OpenAiToolFunction {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Deserialize, Serialize)]
struct OpenAiResponseBody {
    #[serde(default)]
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize, Serialize)]
struct OpenAiChoice {
    message: OpenAiResponseMessage,
}

#[derive(Debug, Deserialize, Serialize)]
struct OpenAiResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiResponseToolCall>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct OpenAiResponseToolCall {
    #[serde(default)]
    id: Option<String>,
    function: OpenAiResponseToolCallFunction,
}

#[derive(Debug, Deserialize, Serialize)]
struct OpenAiResponseToolCallFunction {
    name: String,
    arguments: Value,
}

#[derive(Debug, Deserialize, Serialize)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: usize,
    #[serde(default)]
    completion_tokens: usize,
    #[serde(default)]
    total_tokens: usize,
}

fn map_openai_request(request: &ChatToolsRequest) -> OpenAiRequestBody {
    let messages = request
        .messages
        .iter()
        .map(|m| {
            let role = match m.role {
                MessageRole::System => "system",
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::Tool => "tool",
            }
            .to_string();

            let tool_calls = if m.tool_calls.is_empty() {
                None
            } else {
                Some(
                    m.tool_calls
                        .iter()
                        .map(|tc| OpenAiRequestToolCall {
                            call_type: "function".to_string(),
                            id: tc.id.clone(),
                            function: OpenAiRequestToolCallFunction {
                                name: tc.name.clone(),
                                arguments: tc.arguments.clone(),
                            },
                        })
                        .collect(),
                )
            };

            OpenAiRequestMessage {
                role,
                content: Some(m.content.clone()),
                tool_call_id: m.tool_call_id.clone(),
                tool_calls,
            }
        })
        .collect();

    let tools = request
        .tools
        .iter()
        .map(|tool| OpenAiToolDefinition {
            tool_type: "function".to_string(),
            function: OpenAiToolFunction {
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool.parameters.clone(),
            },
        })
        .collect::<Vec<_>>();

    OpenAiRequestBody {
        model: request.model_id.clone(),
        messages,
        temperature: request.options.temperature,
        max_tokens: request.options.max_tokens,
        top_p: request.options.top_p,
        tool_choice: if tools.is_empty() {
            None
        } else {
            Some("auto".to_string())
        },
        tools,
        stream: false,
    }
}

#[derive(Debug, Serialize)]
struct AnthropicRequestBody {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<AnthropicInputMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<AnthropicToolDefinition>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct AnthropicInputMessage {
    role: String,
    content: Vec<AnthropicInputContent>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicInputContent {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Debug, Serialize)]
struct AnthropicToolDefinition {
    name: String,
    description: String,
    input_schema: Value,
}

#[derive(Debug, Deserialize, Serialize)]
struct AnthropicResponseBody {
    #[serde(default)]
    content: Vec<AnthropicOutputContent>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicOutputContent {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    Thinking {
        text: String,
    },
}

#[derive(Debug, Deserialize, Serialize)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: usize,
    #[serde(default)]
    output_tokens: usize,
}

fn map_anthropic_request(request: &ChatToolsRequest) -> AnthropicRequestBody {
    let mut system_parts = Vec::new();
    let mut messages = Vec::new();

    for message in &request.messages {
        match message.role {
            MessageRole::System => {
                let text = message.content.trim();
                if !text.is_empty() {
                    system_parts.push(text.to_string());
                }
            }
            MessageRole::User => {
                messages.push(AnthropicInputMessage {
                    role: "user".to_string(),
                    content: vec![AnthropicInputContent::Text {
                        text: message.content.clone(),
                    }],
                });
            }
            MessageRole::Assistant => {
                let mut content = Vec::new();
                let text = message.content.trim();
                if !text.is_empty() {
                    content.push(AnthropicInputContent::Text {
                        text: text.to_string(),
                    });
                }
                for call in &message.tool_calls {
                    let input = serde_json::from_str::<Value>(&call.arguments)
                        .unwrap_or_else(|_| json!({}));
                    content.push(AnthropicInputContent::ToolUse {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        input,
                    });
                }
                if content.is_empty() {
                    content.push(AnthropicInputContent::Text {
                        text: String::new(),
                    });
                }
                messages.push(AnthropicInputMessage {
                    role: "assistant".to_string(),
                    content,
                });
            }
            MessageRole::Tool => {
                let tool_id = message
                    .tool_call_id
                    .clone()
                    .unwrap_or_else(|| "unknown_tool_call".to_string());
                messages.push(AnthropicInputMessage {
                    role: "user".to_string(),
                    content: vec![AnthropicInputContent::ToolResult {
                        tool_use_id: tool_id,
                        content: message.content.clone(),
                    }],
                });
            }
        }
    }

    let tools = request
        .tools
        .iter()
        .map(|tool| AnthropicToolDefinition {
            name: tool.name.clone(),
            description: tool.description.clone(),
            input_schema: tool.parameters.clone(),
        })
        .collect::<Vec<_>>();

    AnthropicRequestBody {
        model: request.model_id.clone(),
        max_tokens: request.options.max_tokens.unwrap_or(1024),
        temperature: request.options.temperature,
        top_p: request.options.top_p,
        system: if system_parts.is_empty() {
            None
        } else {
            Some(system_parts.join("\n\n"))
        },
        messages,
        tools,
        stream: false,
    }
}
