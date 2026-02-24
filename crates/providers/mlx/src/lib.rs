use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use mlx_ollama_core::{
    ChatMessage, ChatRequest, ChatResponse, MessageRole, ModelDescriptor, ModelProvider,
    ProviderError, TokenUsage,
};
use tokio::process::Command;
use tokio::time::timeout;
use tracing::debug;

#[derive(Debug, Clone)]
pub struct MlxProviderConfig {
    pub models_dir: PathBuf,
    pub command: String,
    pub command_prefix_args: Vec<String>,
    pub command_suffix_args: Vec<String>,
    pub timeout: Duration,
}

impl Default for MlxProviderConfig {
    fn default() -> Self {
        Self {
            models_dir: default_models_dir(),
            command: default_mlx_command(),
            command_prefix_args: Vec::new(),
            command_suffix_args: Vec::new(),
            timeout: Duration::from_secs(900),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MlxProvider {
    cfg: MlxProviderConfig,
}

impl MlxProvider {
    pub fn new(cfg: MlxProviderConfig) -> Self {
        Self { cfg }
    }

    fn resolve_model_path(&self, model_id: &str) -> PathBuf {
        let candidate = PathBuf::from(model_id);
        if candidate.is_absolute() {
            candidate
        } else {
            self.cfg.models_dir.join(model_id)
        }
    }

    fn build_prompt(messages: &[ChatMessage]) -> String {
        let mut prompt = String::new();

        for message in messages {
            match message.role {
                MessageRole::System => {
                    let _ = writeln!(prompt, "[SYSTEM]\n{}\n", message.content.trim());
                }
                MessageRole::User => {
                    let _ = writeln!(prompt, "[USER]\n{}\n", message.content.trim());
                }
                MessageRole::Assistant => {
                    let _ = writeln!(prompt, "[ASSISTANT]\n{}\n", message.content.trim());
                }
                MessageRole::Tool => {
                    let _ = writeln!(prompt, "[TOOL]\n{}\n", message.content.trim());
                }
            }
        }

        prompt.push_str("[ASSISTANT]\n");
        prompt
    }

    fn extract_text(raw_output: &str) -> String {
        raw_output.trim().to_string()
    }

    fn command_debug_string(command: &str, args: &[String]) -> String {
        let mut all = vec![command.to_string()];
        all.extend_from_slice(args);
        all.join(" ")
    }

    fn model_name_from_path(path: &Path) -> String {
        path.file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string())
    }

    async fn scan_model_dir(path: &Path) -> Result<ModelScan, std::io::Error> {
        let mut entries = tokio::fs::read_dir(path).await?;
        let mut has_config = false;
        let mut has_safetensors_index = false;
        let mut safetensors_files = 0_usize;
        let mut safetensors_bytes = 0_u64;

        loop {
            let entry = entries.next_entry().await?;
            let Some(entry) = entry else { break };
            let metadata = entry.metadata().await?;
            if metadata.is_dir() {
                continue;
            }

            let file_name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if file_name == "config.json" {
                has_config = true;
            }
            if file_name == "model.safetensors.index.json" {
                has_safetensors_index = true;
            }
            if file_name.ends_with(".safetensors") {
                safetensors_files += 1;
                safetensors_bytes = safetensors_bytes.saturating_add(metadata.len());
            }
        }

        Ok(ModelScan {
            has_config,
            has_safetensors_index,
            safetensors_files,
            safetensors_bytes,
        })
    }

    fn format_size(bytes: u64) -> String {
        const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
        format!("{:.1} GiB", bytes as f64 / GIB)
    }

    fn detect_system_memory_bytes() -> Option<u64> {
        #[cfg(target_os = "macos")]
        {
            let output = std::process::Command::new("sysctl")
                .arg("-n")
                .arg("hw.memsize")
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            let raw = String::from_utf8_lossy(&output.stdout);
            raw.trim().parse::<u64>().ok()
        }

        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    }

    fn command_status_details(status: &ExitStatus) -> String {
        status
            .code()
            .map(|code| format!("exit code {code}"))
            .unwrap_or_else(|| "terminated by signal".to_string())
    }

    fn tail_text(raw: &str, max_chars: usize) -> String {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return String::new();
        }

        let char_count = trimmed.chars().count();
        if char_count <= max_chars {
            return trimmed.to_string();
        }

        let tail = trimmed
            .chars()
            .skip(char_count.saturating_sub(max_chars))
            .collect::<String>();
        format!("...{tail}")
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct ModelScan {
    has_config: bool,
    has_safetensors_index: bool,
    safetensors_files: usize,
    safetensors_bytes: u64,
}

impl ModelScan {
    fn is_runnable(self) -> bool {
        self.has_config && (self.safetensors_files > 0 || self.has_safetensors_index)
    }
}

fn default_mlx_command() -> String {
    let preferred = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
        .map(|home| home.join("mlx-env").join("bin").join("mlx_lm.generate"))
        .unwrap_or_else(|| PathBuf::from("mlx_lm.generate"));
    if preferred.exists() {
        preferred.display().to_string()
    } else {
        "mlx_lm.generate".to_string()
    }
}

fn default_models_dir() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
        .map(|home| home.join("mlx-pilot-models"))
        .unwrap_or_else(|| PathBuf::from(".").join("models"))
}

#[async_trait]
impl ModelProvider for MlxProvider {
    fn provider_id(&self) -> &'static str {
        "mlx"
    }

    async fn list_models(&self) -> Result<Vec<ModelDescriptor>, ProviderError> {
        if !self.cfg.models_dir.exists() {
            tokio::fs::create_dir_all(&self.cfg.models_dir)
                .await
                .map_err(|source| ProviderError::Io {
                    context: format!(
                        "creating models directory {}",
                        self.cfg.models_dir.display()
                    ),
                    source,
                })?;
            return Ok(Vec::new());
        }

        let mut entries = tokio::fs::read_dir(&self.cfg.models_dir)
            .await
            .map_err(|source| ProviderError::Io {
                context: format!("reading models directory {}", self.cfg.models_dir.display()),
                source,
            })?;

        let mut models = Vec::new();

        loop {
            let entry = entries
                .next_entry()
                .await
                .map_err(|source| ProviderError::Io {
                    context: format!(
                        "iterating models directory {}",
                        self.cfg.models_dir.display()
                    ),
                    source,
                })?;

            let Some(entry) = entry else { break };
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }

            let scan = Self::scan_model_dir(&path)
                .await
                .map_err(|source| ProviderError::Io {
                    context: format!("reading model directory {}", path.display()),
                    source,
                })?;
            if !scan.is_runnable() {
                continue;
            }

            models.push(ModelDescriptor {
                id: name.clone(),
                name,
                provider: self.provider_id().to_string(),
                path: path.display().to_string(),
                is_available: true,
            });
        }

        models.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(models)
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        if request.messages.is_empty() {
            return Err(ProviderError::InvalidRequest {
                details: "messages cannot be empty".to_string(),
            });
        }

        let model_path = self.resolve_model_path(&request.model_id);
        if !model_path.exists() {
            return Err(ProviderError::ModelNotFound {
                model_id: request.model_id,
            });
        }

        let model_scan =
            Self::scan_model_dir(&model_path)
                .await
                .map_err(|source| ProviderError::Io {
                    context: format!("reading model directory {}", model_path.display()),
                    source,
                })?;
        if !model_scan.is_runnable() {
            return Err(ProviderError::InvalidRequest {
                details: format!(
                    "modelo '{}' nao possui pesos safetensors validos para mlx_lm.generate",
                    model_path.display()
                ),
            });
        }

        if model_scan.safetensors_bytes > 0 {
            if let Some(system_memory) = Self::detect_system_memory_bytes() {
                let safe_limit = system_memory.saturating_mul(85) / 100;
                if model_scan.safetensors_bytes > safe_limit {
                    return Err(ProviderError::Unavailable {
                        details: format!(
                            "modelo '{}' exige aproximadamente {} de pesos, acima do limite seguro da maquina (RAM fisica {}); escolha um modelo menor ou quantizacao mais agressiva",
                            model_path.display(),
                            Self::format_size(model_scan.safetensors_bytes),
                            Self::format_size(system_memory),
                        ),
                    });
                }
            }
        }

        let prompt = Self::build_prompt(&request.messages);
        let mut args = self.cfg.command_prefix_args.clone();

        args.push("--model".to_string());
        args.push(model_path.display().to_string());
        args.push("--prompt".to_string());
        args.push(prompt.clone());

        if let Some(temp) = request.options.temperature {
            args.push("--temp".to_string());
            args.push(temp.to_string());
        }

        if let Some(max_tokens) = request.options.max_tokens {
            args.push("--max-tokens".to_string());
            args.push(max_tokens.to_string());
        }

        if let Some(top_p) = request.options.top_p {
            args.push("--top-p".to_string());
            args.push(top_p.to_string());
        }

        args.extend(self.cfg.command_suffix_args.clone());

        let command_debug = Self::command_debug_string(&self.cfg.command, &args);
        debug!("running mlx command: {command_debug}");

        let mut command = Command::new(&self.cfg.command);
        command
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let started = Instant::now();
        let output = timeout(self.cfg.timeout, command.output())
            .await
            .map_err(|_| ProviderError::Timeout {
                seconds: self.cfg.timeout.as_secs(),
            })?
            .map_err(|source| ProviderError::Io {
                context: format!("spawning command {}", self.cfg.command),
                source,
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

        if !output.status.success() {
            let status_detail = Self::command_status_details(&output.status);
            let mut details = status_detail;
            if !stderr.is_empty() {
                details.push_str("; ");
                details.push_str(&stderr);
            } else {
                let stdout_tail = Self::tail_text(&stdout, 700);
                if !stdout_tail.is_empty() {
                    details.push_str("; stdout: ");
                    details.push_str(&stdout_tail);
                }
            }

            return Err(ProviderError::CommandFailed {
                command: command_debug,
                stderr: details,
            });
        }

        let raw = stdout;
        let text = Self::extract_text(&raw);

        let prompt_tokens = prompt.split_whitespace().count();
        let completion_tokens = text.split_whitespace().count();
        let usage = TokenUsage {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
        };

        Ok(ChatResponse {
            model_id: Self::model_name_from_path(&model_path),
            provider: self.provider_id().to_string(),
            message: ChatMessage::text(MessageRole::Assistant, text),
            usage,
            latency_ms: started.elapsed().as_millis() as u64,
            raw_output: Some(raw),
        })
    }
}

#[cfg(test)]
mod tests {
    use mlx_ollama_core::{ChatMessage, MessageRole};

    use super::MlxProvider;

    #[test]
    fn prompt_contains_all_roles_and_assistant_suffix() {
        let messages = vec![
            ChatMessage::text(MessageRole::System, "You are concise"),
            ChatMessage::text(MessageRole::User, "Explain Rust ownership"),
        ];

        let prompt = MlxProvider::build_prompt(&messages);
        assert!(prompt.contains("[SYSTEM]"));
        assert!(prompt.contains("[USER]"));
        assert!(prompt.ends_with("[ASSISTANT]\n"));
    }
}
