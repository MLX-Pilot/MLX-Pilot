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
use reqwest::Url;
use serde_json::{json, Value};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::sleep;
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub struct LlamaCppProviderConfig {
    pub models_dir: PathBuf,
    pub server_binary: String,
    pub base_url: String,
    pub timeout: Duration,
    pub startup_timeout: Duration,
    pub auto_start: bool,
    pub auto_install: bool,
    pub context_size: u32,
    pub gpu_layers: i32,
    pub extra_args: Vec<String>,
}

impl Default for LlamaCppProviderConfig {
    fn default() -> Self {
        Self {
            models_dir: PathBuf::from("/Users/kaike/models"),
            server_binary: default_llama_server_binary(),
            base_url: "http://127.0.0.1:11439".to_string(),
            timeout: Duration::from_secs(900),
            startup_timeout: Duration::from_secs(45),
            auto_start: true,
            auto_install: true,
            context_size: 16384,
            gpu_layers: 999,
            extra_args: Vec::new(),
        }
    }
}

#[derive(Debug, Default)]
struct ServerState {
    child: Option<Child>,
    model_path: Option<PathBuf>,
    model_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LlamaCppProvider {
    cfg: LlamaCppProviderConfig,
    client: reqwest::Client,
    state: Arc<Mutex<ServerState>>,
}

impl LlamaCppProvider {
    pub fn new(cfg: LlamaCppProviderConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(cfg.timeout)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            cfg,
            client,
            state: Arc::new(Mutex::new(ServerState::default())),
        }
    }

    async fn ensure_ready_for_model(&self, model_path: &Path) -> Result<String, ProviderError> {
        let mut state = self.state.lock().await;
        self.refresh_child_state_locked(&mut state).await?;

        let expected_model = model_path
            .file_name()
            .and_then(|value| value.to_str())
            .map(ToString::to_string)
            .unwrap_or_else(|| model_path.display().to_string());

        if state.model_path.as_deref() == Some(model_path) && self.ping_server().await {
            return Ok(state
                .model_name
                .clone()
                .unwrap_or_else(|| expected_model.clone()));
        }

        if state.child.is_some() {
            self.stop_child_locked(&mut state).await?;
        }

        let server_binary = self.ensure_server_binary().await?;
        if !self.cfg.auto_start {
            return Err(ProviderError::Unavailable {
                details: "llama.cpp server nao esta em execucao e auto_start=false".to_string(),
            });
        }

        self.spawn_server_locked(&mut state, &server_binary, model_path, &expected_model)
            .await?;
        drop(state);

        self.wait_for_server().await?;
        Ok(expected_model)
    }

    async fn ensure_server_binary(&self) -> Result<String, ProviderError> {
        let configured = self.cfg.server_binary.trim();
        if !configured.is_empty() && command_available(configured).await {
            return Ok(configured.to_string());
        }

        if !self.cfg.auto_install {
            return Err(ProviderError::Unavailable {
                details: format!(
                    "llama-server nao encontrado em '{}' e auto_install=false",
                    self.cfg.server_binary
                ),
            });
        }

        self.install_llamacpp().await?;

        if !configured.is_empty() && command_available(configured).await {
            return Ok(configured.to_string());
        }

        if command_available("llama-server").await {
            return Ok("llama-server".to_string());
        }

        Err(ProviderError::Unavailable {
            details: "instalacao automatica concluiu sem binario 'llama-server' disponivel"
                .to_string(),
        })
    }

    async fn install_llamacpp(&self) -> Result<(), ProviderError> {
        if cfg!(target_os = "macos") && command_available("brew").await {
            run_command("brew", &["install", "llama.cpp"], Duration::from_secs(1800)).await?;
            return Ok(());
        }

        if cfg!(target_os = "windows") && command_available("winget").await {
            let _ = run_command(
                "winget",
                &[
                    "install",
                    "--id",
                    "ggml.llama.cpp",
                    "-e",
                    "--accept-package-agreements",
                    "--accept-source-agreements",
                    "--silent",
                ],
                Duration::from_secs(1800),
            )
            .await;
        }

        Err(ProviderError::Unavailable {
            details:
                "nao foi possivel instalar llama.cpp automaticamente neste sistema. Configure APP_LLAMACPP_SERVER_BINARY com um binario existente."
                    .to_string(),
        })
    }

    async fn spawn_server_locked(
        &self,
        state: &mut ServerState,
        binary: &str,
        model_path: &Path,
        model_name: &str,
    ) -> Result<(), ProviderError> {
        let (host, port) = extract_host_port(&self.cfg.base_url)?;
        let log_path = std::env::temp_dir().join("mlx-pilot-llamacpp.log");
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
        command
            .arg("--host")
            .arg(&host)
            .arg("--port")
            .arg(port.to_string())
            .arg("-m")
            .arg(model_path)
            .arg("-c")
            .arg(self.cfg.context_size.to_string())
            .arg("-ngl")
            .arg(self.cfg.gpu_layers.to_string())
            .kill_on_drop(true);

        for arg in &self.cfg.extra_args {
            command.arg(arg);
        }

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

        debug!(
            "starting llama.cpp server: binary={} model={} host={} port={}",
            binary,
            model_path.display(),
            host,
            port
        );

        let child = command.spawn().map_err(|source| ProviderError::Io {
            context: format!("falha iniciando '{binary}' para {}", model_path.display()),
            source,
        })?;

        state.child = Some(child);
        state.model_path = Some(model_path.to_path_buf());
        state.model_name = Some(model_name.to_string());

        Ok(())
    }

    async fn stop_child_locked(&self, state: &mut ServerState) -> Result<(), ProviderError> {
        if let Some(child) = state.child.as_mut() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }

        state.child = None;
        state.model_path = None;
        state.model_name = None;
        Ok(())
    }

    async fn refresh_child_state_locked(
        &self,
        state: &mut ServerState,
    ) -> Result<(), ProviderError> {
        let mut exited = false;
        if let Some(child) = state.child.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    warn!("llama.cpp server process exited with status: {status}");
                    exited = true;
                }
                Ok(None) => {}
                Err(source) => {
                    return Err(ProviderError::Io {
                        context: "falha verificando processo llama.cpp".to_string(),
                        source,
                    });
                }
            }
        }

        if exited {
            state.child = None;
            state.model_path = None;
            state.model_name = None;
        }

        Ok(())
    }

    async fn wait_for_server(&self) -> Result<(), ProviderError> {
        let started = Instant::now();
        let timeout = self.cfg.startup_timeout.max(Duration::from_secs(2));

        loop {
            if self.ping_server().await {
                return Ok(());
            }

            if started.elapsed() >= timeout {
                return Err(ProviderError::Unavailable {
                    details: format!("llama.cpp server nao respondeu em {}s", timeout.as_secs()),
                });
            }

            sleep(Duration::from_millis(450)).await;
        }
    }

    async fn ping_server(&self) -> bool {
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
        {
            Ok(client) => client,
            Err(_) => return false,
        };

        let health_url = format!("{}/health", self.cfg.base_url.trim_end_matches('/'));
        if let Ok(response) = client.get(&health_url).send().await {
            if response.status().is_success() {
                return true;
            }
        }

        let models_url = format!("{}/v1/models", self.cfg.base_url.trim_end_matches('/'));
        client
            .get(models_url)
            .send()
            .await
            .map(|response| response.status().is_success())
            .unwrap_or(false)
    }

    async fn resolve_model_path(&self, model_id: &str) -> Result<PathBuf, ProviderError> {
        let trimmed = model_id.trim();
        if trimmed.is_empty() {
            return Err(ProviderError::InvalidRequest {
                details: "model_id cannot be empty".to_string(),
            });
        }

        let candidate = PathBuf::from(trimmed);
        let resolved = if candidate.is_absolute() {
            candidate
        } else {
            self.cfg.models_dir.join(candidate)
        };

        if !resolved.exists() || !resolved.is_file() {
            return Err(ProviderError::ModelNotFound {
                model_id: trimmed.to_string(),
            });
        }

        if !resolved
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.eq_ignore_ascii_case("gguf"))
            .unwrap_or(false)
        {
            return Err(ProviderError::InvalidRequest {
                details: format!(
                    "modelo '{}' nao e GGUF (llama.cpp requer .gguf)",
                    resolved.display()
                ),
            });
        }

        Ok(resolved)
    }
}

#[async_trait]
impl ModelProvider for LlamaCppProvider {
    fn provider_id(&self) -> &'static str {
        "llamacpp"
    }

    async fn list_models(&self) -> Result<Vec<ModelDescriptor>, ProviderError> {
        let files =
            discover_gguf_models(&self.cfg.models_dir).map_err(|source| ProviderError::Io {
                context: format!("reading models directory {}", self.cfg.models_dir.display()),
                source,
            })?;

        let mut models = files
            .into_iter()
            .map(|path| {
                let id = path
                    .strip_prefix(&self.cfg.models_dir)
                    .map(|value| value.display().to_string())
                    .unwrap_or_else(|_| path.display().to_string());

                let name = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| id.clone());

                ModelDescriptor {
                    id,
                    name,
                    provider: self.provider_id().to_string(),
                    path: path.display().to_string(),
                    is_available: true,
                }
            })
            .collect::<Vec<_>>();

        models.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
        Ok(models)
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        if request.messages.is_empty() {
            return Err(ProviderError::InvalidRequest {
                details: "messages cannot be empty".to_string(),
            });
        }

        let model_path = self.resolve_model_path(&request.model_id).await?;
        let model_name = self.ensure_ready_for_model(&model_path).await?;
        let started = Instant::now();

        let endpoint = format!(
            "{}/v1/chat/completions",
            self.cfg.base_url.trim_end_matches('/')
        );

        let payload = json!({
            "model": model_name,
            "messages": request.messages.iter().map(|entry| json!({
                "role": match entry.role {
                    MessageRole::System => "system",
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                    MessageRole::Tool => "tool",
                },
                "content": entry.content,
            })).collect::<Vec<_>>(),
            "temperature": request.options.temperature,
            "max_tokens": request.options.max_tokens,
            "top_p": request.options.top_p,
            "stream": false,
        });

        let response = self
            .client
            .post(&endpoint)
            .json(&payload)
            .send()
            .await
            .map_err(|source| ProviderError::Io {
                context: "falha consultando llama.cpp /v1/chat/completions".to_string(),
                source: io::Error::other(source.to_string()),
            })?;

        let status = response.status();
        let raw = response.text().await.map_err(|source| ProviderError::Io {
            context: "falha lendo resposta llama.cpp".to_string(),
            source: io::Error::other(source.to_string()),
        })?;

        if !status.is_success() {
            return Err(ProviderError::Unavailable {
                details: format!("llama.cpp retornou HTTP {status}: {}", raw.trim()),
            });
        }

        let body = serde_json::from_str::<Value>(&raw).map_err(|source| ProviderError::Io {
            context: "falha parseando JSON de resposta llama.cpp".to_string(),
            source: io::Error::other(source.to_string()),
        })?;

        let answer = extract_chat_content(&body);
        let prompt_tokens = body
            .pointer("/usage/prompt_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        let completion_tokens =
            body.pointer("/usage/completion_tokens")
                .and_then(Value::as_u64)
                .unwrap_or_else(|| answer.split_whitespace().count() as u64) as usize;
        let total_tokens =
            body.pointer("/usage/total_tokens")
                .and_then(Value::as_u64)
                .unwrap_or((prompt_tokens + completion_tokens) as u64) as usize;

        Ok(ChatResponse {
            model_id: model_path.display().to_string(),
            provider: self.provider_id().to_string(),
            message: ChatMessage::text(MessageRole::Assistant, answer),
            usage: TokenUsage {
                prompt_tokens,
                completion_tokens,
                total_tokens,
            },
            latency_ms: started.elapsed().as_millis() as u64,
            raw_output: Some(raw),
        })
    }
}

fn extract_chat_content(body: &Value) -> String {
    if let Some(content) = body
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return content.to_string();
    }

    if let Some(items) = body
        .pointer("/choices/0/message/content")
        .and_then(Value::as_array)
    {
        let mut combined = Vec::new();
        for item in items {
            if let Some(text) = item.get("text").and_then(Value::as_str) {
                let normalized = text.trim();
                if !normalized.is_empty() {
                    combined.push(normalized.to_string());
                }
            }
        }
        if !combined.is_empty() {
            return combined.join("\n");
        }
    }

    String::new()
}

fn discover_gguf_models(root: &Path) -> Result<Vec<PathBuf>, io::Error> {
    let mut out = Vec::new();
    collect_gguf_files(root, 0, &mut out)?;
    Ok(out)
}

fn collect_gguf_files(path: &Path, depth: usize, out: &mut Vec<PathBuf>) -> Result<(), io::Error> {
    if depth > 8 || !path.exists() {
        return Ok(());
    }

    if path.is_file() {
        if path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.eq_ignore_ascii_case("gguf"))
            .unwrap_or(false)
        {
            out.push(path.to_path_buf());
        }
        return Ok(());
    }

    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let candidate = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();

        if name.starts_with('.') {
            continue;
        }

        collect_gguf_files(&candidate, depth + 1, out)?;
    }

    Ok(())
}

fn extract_host_port(base_url: &str) -> Result<(String, u16), ProviderError> {
    let parsed = Url::parse(base_url.trim()).map_err(|source| ProviderError::InvalidRequest {
        details: format!("APP_LLAMACPP_BASE_URL invalido: {source}"),
    })?;

    let host = parsed
        .host_str()
        .map(ToString::to_string)
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let port = parsed.port_or_known_default().unwrap_or(11439);
    Ok((host, port))
}

fn default_llama_server_binary() -> String {
    let bundled = PathBuf::from("/Users/kaike/mlx-ollama-pilot/bin/llama-server");
    if bundled.exists() {
        bundled.display().to_string()
    } else {
        "llama-server".to_string()
    }
}

async fn command_available(command: &str) -> bool {
    if command.trim().is_empty() {
        return false;
    }

    Command::new(command)
        .arg("--help")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .await
        .map(|output| output.status.success())
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
