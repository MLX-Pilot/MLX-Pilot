use std::io;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use mlx_ollama_core::{
    ChatMessage, ChatRequest, ChatResponse, MessageRole, ModelDescriptor, ModelProvider,
    ProviderError, TokenUsage,
};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct OllamaProviderConfig {
    pub base_url: String,
    pub timeout: Duration,
}

impl Default for OllamaProviderConfig {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:11434".to_string(),
            timeout: Duration::from_secs(900),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OllamaProvider {
    cfg: OllamaProviderConfig,
    client: reqwest::Client,
}

impl OllamaProvider {
    pub fn new(cfg: OllamaProviderConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(cfg.timeout)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self { cfg, client }
    }

    fn endpoint(&self, path: &str) -> Result<String, ProviderError> {
        let base = self.cfg.base_url.trim();
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
    options: Option<OllamaOptions>,
}

#[derive(Debug, Serialize)]
struct OllamaChatMessage {
    role: String,
    content: String,
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
        let endpoint = self.endpoint("/api/tags")?;
        let response = self
            .client
            .get(&endpoint)
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
                })
            })
            .collect::<Vec<_>>();

        models.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(models)
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        if request.messages.is_empty() {
            return Err(ProviderError::InvalidRequest {
                details: "messages cannot be empty".to_string(),
            });
        }

        let endpoint = self.endpoint("/api/chat")?;
        let started = Instant::now();

        let body = OllamaChatRequestBody {
            model: request.model_id.clone(),
            messages: request
                .messages
                .iter()
                .map(|message| OllamaChatMessage {
                    role: match message.role {
                        MessageRole::System => "system".to_string(),
                        MessageRole::User => "user".to_string(),
                        MessageRole::Assistant => "assistant".to_string(),
                    },
                    content: message.content.clone(),
                })
                .collect::<Vec<_>>(),
            stream: false,
            options: Some(OllamaOptions {
                temperature: request.options.temperature,
                max_tokens: request.options.max_tokens,
                top_p: request.options.top_p,
            }),
        };

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

        Ok(ChatResponse {
            model_id: if payload.model.trim().is_empty() {
                request.model_id
            } else {
                payload.model
            },
            provider: self.provider_id().to_string(),
            message: ChatMessage {
                role: MessageRole::Assistant,
                content,
            },
            usage,
            latency_ms,
            raw_output,
        })
    }
}
