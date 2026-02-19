use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use mlx_ollama_core::{
    ChatMessage, ChatRequest, ChatResponse, MessageRole, ModelDescriptor, ModelProvider,
    ProviderError, TokenUsage,
};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::time::sleep;

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
    ensure_lock: Arc<Mutex<()>>,
}

impl OllamaProvider {
    pub fn new(cfg: OllamaProviderConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(cfg.timeout)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            cfg,
            client,
            ensure_lock: Arc::new(Mutex::new(())),
        }
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

    async fn ensure_ready(&self) -> Result<(), ProviderError> {
        if self.ping_server().await {
            return Ok(());
        }

        let _guard = self.ensure_lock.lock().await;

        if self.ping_server().await {
            return Ok(());
        }

        let mut binary = self.find_ollama_binary().await;
        if binary.is_none() && self.cfg.auto_install {
            self.install_ollama().await?;
            binary = self.find_ollama_binary().await;
        }

        let Some(binary) = binary else {
            return Err(ProviderError::Unavailable {
                details: "ollama nao encontrado e instalacao automatica indisponivel".to_string(),
            });
        };

        if self.cfg.auto_start {
            self.start_server(&binary).await?;
        }

        self.wait_until_ready().await
    }

    async fn ping_server(&self) -> bool {
        let endpoint = match self.endpoint("/api/version") {
            Ok(value) => value,
            Err(_) => return false,
        };

        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
        {
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

    async fn wait_until_ready(&self) -> Result<(), ProviderError> {
        let started = Instant::now();
        let timeout = self.cfg.startup_timeout.max(Duration::from_secs(2));

        loop {
            if self.ping_server().await {
                return Ok(());
            }

            if started.elapsed() >= timeout {
                return Err(ProviderError::Unavailable {
                    details: format!(
                        "ollama nao respondeu em {}s apos bootstrap automatico",
                        timeout.as_secs()
                    ),
                });
            }

            sleep(Duration::from_millis(450)).await;
        }
    }

    async fn find_ollama_binary(&self) -> Option<String> {
        if command_available("ollama").await {
            return Some("ollama".to_string());
        }

        let mut candidates = vec![
            "/opt/homebrew/bin/ollama",
            "/usr/local/bin/ollama",
            "/usr/bin/ollama",
            "/Applications/Ollama.app/Contents/Resources/ollama",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();

        if cfg!(target_os = "windows") {
            if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
                candidates.push(
                    Path::new(&localappdata)
                        .join("Programs")
                        .join("Ollama")
                        .join("ollama.exe"),
                );
            }
        }

        for candidate in candidates {
            if !candidate.exists() {
                continue;
            }

            let text = candidate.display().to_string();
            if command_available(&text).await {
                return Some(text);
            }
        }

        None
    }

    async fn install_ollama(&self) -> Result<(), ProviderError> {
        if cfg!(target_os = "macos") {
            if command_available("brew").await {
                run_command("brew", &["install", "ollama"], Duration::from_secs(1800)).await?;
                return Ok(());
            }

            run_shell(
                "curl -fsSL https://ollama.com/install.sh | sh",
                Duration::from_secs(1800),
            )
            .await?;
            return Ok(());
        }

        if cfg!(target_os = "linux") {
            run_shell(
                "curl -fsSL https://ollama.com/install.sh | sh",
                Duration::from_secs(1800),
            )
            .await?;
            return Ok(());
        }

        if cfg!(target_os = "windows") {
            run_command(
                "winget",
                &["install", "--id", "Ollama.Ollama", "-e", "--silent"],
                Duration::from_secs(1800),
            )
            .await?;
            return Ok(());
        }

        Err(ProviderError::Unavailable {
            details: "instalacao automatica do ollama nao suportada neste sistema".to_string(),
        })
    }

    async fn start_server(&self, binary: &str) -> Result<(), ProviderError> {
        if self.ping_server().await {
            return Ok(());
        }

        let log_path = std::env::temp_dir().join("mlx-pilot-ollama.log");
        let stdout_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .ok();
        let stderr_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .ok();

        let mut command = Command::new(binary);
        command.arg("serve");

        match stdout_file {
            Some(file) => {
                command.stdout(Stdio::from(file));
            }
            None => {
                command.stdout(Stdio::null());
            }
        }

        match stderr_file {
            Some(file) => {
                command.stderr(Stdio::from(file));
            }
            None => {
                command.stderr(Stdio::null());
            }
        }

        command.spawn().map_err(|source| ProviderError::Io {
            context: format!("falha ao iniciar servidor Ollama com '{binary} serve'"),
            source,
        })?;

        Ok(())
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
        self.ensure_ready().await?;

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

        self.ensure_ready().await?;

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

async fn command_available(command: &str) -> bool {
    if command.trim().is_empty() {
        return false;
    }

    let output = Command::new(command)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .await;

    output
        .map(|result| result.status.success())
        .unwrap_or(false)
}

async fn run_command(program: &str, args: &[&str], timeout: Duration) -> Result<(), ProviderError> {
    let output = tokio::time::timeout(timeout, Command::new(program).args(args).output())
        .await
        .map_err(|_| ProviderError::Timeout {
            seconds: timeout.as_secs().max(1),
        })?
        .map_err(|source| ProviderError::Io {
            context: format!("falha executando '{program}'"),
            source,
        })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(ProviderError::CommandFailed {
        command: format!("{} {}", program, args.join(" ")),
        stderr: if stderr.is_empty() {
            "sem stderr".to_string()
        } else {
            stderr
        },
    })
}

async fn run_shell(script: &str, timeout: Duration) -> Result<(), ProviderError> {
    #[cfg(target_os = "windows")]
    {
        run_command("powershell", &["-NoProfile", "-Command", script], timeout).await
    }

    #[cfg(not(target_os = "windows"))]
    {
        run_command("sh", &["-lc", script], timeout).await
    }
}
