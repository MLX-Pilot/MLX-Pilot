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
    pub airllm_enabled: bool,
    pub airllm_threshold_percent: u8,
    pub airllm_python_command: String,
    pub airllm_runner: String,
    pub airllm_backend: String,
}

impl Default for MlxProviderConfig {
    fn default() -> Self {
        Self {
            models_dir: default_models_dir(),
            command: default_mlx_command(),
            command_prefix_args: Vec::new(),
            command_suffix_args: Vec::new(),
            timeout: Duration::from_secs(900),
            airllm_enabled: true,
            airllm_threshold_percent: 70,
            airllm_python_command: default_airllm_python_command(),
            airllm_runner: default_airllm_runner(),
            airllm_backend: "auto".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MlxProvider {
    cfg: MlxProviderConfig,
}

const RUNTIME_META_PREFIX: &str = "[[MLX-PILOT-META";

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
        Self::strip_runtime_meta(raw_output).trim().to_string()
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

    fn runtime_meta_line(airllm_required: bool, airllm_used: bool) -> Option<String> {
        if !airllm_required && !airllm_used {
            return None;
        }
        Some(format!(
            "[[MLX-PILOT-META airllm_required={} airllm_used={}]]",
            if airllm_required { 1 } else { 0 },
            if airllm_used { 1 } else { 0 }
        ))
    }

    fn inject_runtime_meta(raw: String, airllm_required: bool, airllm_used: bool) -> String {
        let Some(meta_line) = Self::runtime_meta_line(airllm_required, airllm_used) else {
            return raw;
        };

        if raw.trim().is_empty() {
            return meta_line;
        }

        format!("{meta_line}\n{raw}")
    }

    fn strip_runtime_meta(raw_output: &str) -> String {
        let mut lines = raw_output.lines();
        let Some(first_line) = lines.next() else {
            return String::new();
        };

        if first_line.trim_start().starts_with(RUNTIME_META_PREFIX) {
            return lines.collect::<Vec<_>>().join("\n");
        }

        raw_output.to_string()
    }

    fn has_arg(args: &[String], flag: &str) -> bool {
        args.iter().any(|value| value == flag)
    }

    fn append_large_model_guard_args(args: &mut Vec<String>) {
        if !Self::has_arg(args, "--max-kv-size") {
            args.push("--max-kv-size".to_string());
            args.push("1024".to_string());
        }
        if !Self::has_arg(args, "--kv-bits") {
            args.push("--kv-bits".to_string());
            args.push("4".to_string());
        }
        if !Self::has_arg(args, "--kv-group-size") {
            args.push("--kv-group-size".to_string());
            args.push("64".to_string());
        }
        if !Self::has_arg(args, "--quantized-kv-start") {
            args.push("--quantized-kv-start".to_string());
            args.push("0".to_string());
        }
    }

    fn memory_profile(model_bytes: u64) -> Option<MemoryProfile> {
        let system_memory_bytes = Self::detect_system_memory_bytes()?;
        if system_memory_bytes == 0 || model_bytes == 0 {
            return None;
        }

        Some(MemoryProfile {
            system_memory_bytes,
            model_bytes,
            usage_ratio: model_bytes as f64 / system_memory_bytes as f64,
        })
    }

    fn should_try_airllm(
        &self,
        profile: Option<MemoryProfile>,
        request_override: Option<bool>,
    ) -> bool {
        let enabled = request_override.unwrap_or(self.cfg.airllm_enabled);
        if !enabled {
            return false;
        }
        if request_override == Some(true) {
            return true;
        }
        let Some(profile) = profile else { return false };
        let threshold = (self.cfg.airllm_threshold_percent as f64 / 100.0).clamp(0.0, 1.0);
        profile.usage_ratio >= threshold
    }

    fn is_memory_pressure_error(stdout: &str, stderr: &str) -> bool {
        let text = format!("{stdout}\n{stderr}").to_ascii_lowercase();
        // Explicit memory pressure keywords
        if text.contains("insufficient memory")
            || text.contains("out of memory")
            || text.contains("outofmemory")
            || text.contains("kiogpucommandbuffercallbackerroroutofmemory")
            || text.contains("max_recommended_working_set_size")
        {
            return true;
        }

        // Signal termination with Metal/GPU crash indicators — stderr may not
        // have flushed the explicit memory error text before abort().
        let signal_terminated = text.contains("terminated by signal")
            || text.contains("libc++abi")
            || text.contains("uncaught exception");
        let gpu_crash = text.contains("[metal]")
            || text.contains("command buffer")
            || text.contains("iokit")
            || text.contains("iogpu");
        if signal_terminated && gpu_crash {
            return true;
        }

        false
    }

    fn failure_details(stdout: &str, stderr: &str, status: &ExitStatus) -> String {
        let status_detail = Self::command_status_details(status);
        if !stderr.trim().is_empty() {
            return format!("{status_detail}; {}", stderr.trim());
        }

        let stdout_tail = Self::tail_text(stdout, 700);
        if stdout_tail.is_empty() {
            status_detail
        } else {
            format!("{status_detail}; stdout: {stdout_tail}")
        }
    }

    fn build_airllm_args(
        &self,
        model_path: &Path,
        prompt: &str,
        request: &ChatRequest,
    ) -> Vec<String> {
        let backend = self.normalized_airllm_backend();
        let device_hint = Self::bridge_device_hint(backend);
        let mut args = vec![
            self.cfg.airllm_runner.clone(),
            "--model".to_string(),
            model_path.display().to_string(),
            "--prompt".to_string(),
            prompt.to_string(),
            "--backend".to_string(),
            backend.to_string(),
            "--device".to_string(),
            device_hint.to_string(),
        ];

        if let Some(temp) = request.options.temperature {
            args.push("--temp".to_string());
            args.push(temp.to_string());
        }

        if let Some(max_tokens) = request.options.max_tokens {
            let fallback_cap = max_tokens.min(256);
            args.push("--max-tokens".to_string());
            args.push(fallback_cap.to_string());
        }

        if let Some(top_p) = request.options.top_p {
            args.push("--top-p".to_string());
            args.push(top_p.to_string());
        }

        args
    }

    fn normalized_airllm_backend(&self) -> &'static str {
        match self.cfg.airllm_backend.trim().to_ascii_lowercase().as_str() {
            "original" | "airllm" | "airllm-original" => "original",
            "legacy" | "legacy-bridge" | "mlx-lm" => "legacy",
            _ => "auto",
        }
    }

    fn bridge_device_hint(backend: &str) -> &'static str {
        if backend == "legacy" {
            // Legacy fallback can trigger the same MLX memory pressure; keep CPU there.
            "cpu"
        } else {
            // Original AirLLM backend should pick the best available accelerator.
            "auto"
        }
    }

    async fn run_command_capture(
        &self,
        command_name: &str,
        args: &[String],
    ) -> Result<CommandRun, ProviderError> {
        let command_debug = Self::command_debug_string(command_name, args);
        debug!("running command: {command_debug}");

        let mut command = Command::new(command_name);
        command
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = timeout(self.cfg.timeout, command.output())
            .await
            .map_err(|_| ProviderError::Timeout {
                seconds: self.cfg.timeout.as_secs(),
            })?
            .map_err(|source| ProviderError::Io {
                context: format!("spawning command {command_name}"),
                source,
            })?;

        Ok(CommandRun {
            command_debug,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            status: output.status,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct MemoryProfile {
    system_memory_bytes: u64,
    model_bytes: u64,
    usage_ratio: f64,
}

#[derive(Debug)]
struct CommandRun {
    command_debug: String,
    stdout: String,
    stderr: String,
    status: ExitStatus,
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

fn default_airllm_python_command() -> String {
    let preferred = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
        .map(|home| {
            if cfg!(windows) {
                home.join("mlx-env").join("Scripts").join("python.exe")
            } else {
                home.join("mlx-env").join("bin").join("python")
            }
        })
        .unwrap_or_else(|| {
            if cfg!(windows) {
                PathBuf::from("python")
            } else {
                PathBuf::from("python3")
            }
        });
    if preferred.exists() {
        preferred.display().to_string()
    } else {
        if cfg!(windows) {
            "python".to_string()
        } else {
            "python3".to_string()
        }
    }
}

fn default_airllm_runner() -> String {
    resolve_airllm_runner("scripts/mlx_airllm_bridge.py")
}

fn resolve_airllm_runner(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "scripts/mlx_airllm_bridge.py".to_string();
    }

    let input = PathBuf::from(trimmed);
    if input.is_absolute() && input.exists() {
        return input.display().to_string();
    }

    let script_name = input
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "mlx_airllm_bridge.py".to_string());
    let relative = if input.is_absolute() {
        PathBuf::from("scripts").join(script_name.as_str())
    } else if input.components().count() > 1 {
        input.clone()
    } else {
        PathBuf::from("scripts").join(script_name.as_str())
    };

    let mut candidates: Vec<PathBuf> = Vec::new();

    if input.is_absolute() {
        if let Some(parent) = input.parent() {
            candidates.push(parent.join(script_name.as_str()));
            candidates.push(parent.join("scripts").join(script_name.as_str()));
        }
    }

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            candidates.push(exe_dir.join(&relative));
            candidates.push(exe_dir.join("../Resources").join(&relative));
            candidates.push(
                exe_dir
                    .join("../Resources")
                    .join("scripts")
                    .join(script_name.as_str()),
            );
            candidates.push(exe_dir.join("../Resources").join(script_name.as_str()));
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(&relative));
    }

    let workspace_hint = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(&relative);
    candidates.push(workspace_hint);

    if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        let trimmed_home = home.trim();
        if !trimmed_home.is_empty() {
            candidates.push(
                PathBuf::from(trimmed_home)
                    .join("mlx-ollama-pilot")
                    .join(&relative),
            );
        }
    }

    for candidate in candidates {
        if candidate.exists() {
            return candidate.display().to_string();
        }
    }

    relative.display().to_string()
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

        let memory_profile = Self::memory_profile(model_scan.safetensors_bytes);
        let should_try_airllm =
            self.should_try_airllm(memory_profile, request.options.airllm_enabled);
        let force_airllm = request.options.airllm_enabled == Some(true);
        if let Some(profile) = memory_profile {
            debug!(
                "model memory profile: model={} system={} ratio={:.2}%",
                Self::format_size(profile.model_bytes),
                Self::format_size(profile.system_memory_bytes),
                profile.usage_ratio * 100.0
            );
        }

        let prompt = Self::build_prompt(&request.messages);
        let mut primary_args = self.cfg.command_prefix_args.clone();

        primary_args.push("--model".to_string());
        primary_args.push(model_path.display().to_string());
        primary_args.push("--prompt".to_string());
        primary_args.push(prompt.clone());

        if let Some(temp) = request.options.temperature {
            primary_args.push("--temp".to_string());
            primary_args.push(temp.to_string());
        }

        if let Some(max_tokens) = request.options.max_tokens {
            primary_args.push("--max-tokens".to_string());
            primary_args.push(max_tokens.to_string());
        }

        if let Some(top_p) = request.options.top_p {
            primary_args.push("--top-p".to_string());
            primary_args.push(top_p.to_string());
        }

        primary_args.extend(self.cfg.command_suffix_args.clone());

        if should_try_airllm {
            Self::append_large_model_guard_args(&mut primary_args);
        }

        let started = Instant::now();
        let mut airllm_used = false;
        let raw = if should_try_airllm && force_airllm {
            let runner_path = PathBuf::from(&self.cfg.airllm_runner);
            if !runner_path.exists() {
                return Err(ProviderError::CommandFailed {
                    command: self.cfg.airllm_python_command.clone(),
                    stderr: format!(
                        "fallback airllm runner nao encontrado: {}",
                        runner_path.display()
                    ),
                });
            }

            let airllm_args = self.build_airllm_args(&model_path, &prompt, &request);
            let airllm = self
                .run_command_capture(&self.cfg.airllm_python_command, &airllm_args)
                .await?;

            if airllm.status.success() {
                airllm_used = true;
                airllm.stdout
            } else {
                let fallback_details =
                    Self::failure_details(&airllm.stdout, &airllm.stderr, &airllm.status);
                return Err(ProviderError::CommandFailed {
                    command: airllm.command_debug,
                    stderr: format!("fallback airllm falhou: {fallback_details}"),
                });
            }
        } else {
            let primary = self
                .run_command_capture(&self.cfg.command, &primary_args)
                .await?;

            if primary.status.success() {
                primary.stdout
            } else if should_try_airllm
                && Self::is_memory_pressure_error(&primary.stdout, &primary.stderr)
            {
                let runner_path = PathBuf::from(&self.cfg.airllm_runner);
                if !runner_path.exists() {
                    let primary_details =
                        Self::failure_details(&primary.stdout, &primary.stderr, &primary.status);
                    return Err(ProviderError::CommandFailed {
                        command: primary.command_debug,
                        stderr: format!(
                            "mlx falhou por memoria: {primary_details}; fallback airllm runner nao encontrado: {}",
                            runner_path.display()
                        ),
                    });
                }

                let airllm_args = self.build_airllm_args(&model_path, &prompt, &request);
                let airllm = self
                    .run_command_capture(&self.cfg.airllm_python_command, &airllm_args)
                    .await?;

                if airllm.status.success() {
                    airllm_used = true;
                    airllm.stdout
                } else {
                    let primary_details =
                        Self::failure_details(&primary.stdout, &primary.stderr, &primary.status);
                    let fallback_details =
                        Self::failure_details(&airllm.stdout, &airllm.stderr, &airllm.status);
                    return Err(ProviderError::CommandFailed {
                        command: format!("{} || {}", primary.command_debug, airllm.command_debug),
                        stderr: format!(
                            "mlx falhou por memoria: {primary_details}; fallback airllm falhou: {fallback_details}"
                        ),
                    });
                }
            } else {
                return Err(ProviderError::CommandFailed {
                    command: primary.command_debug,
                    stderr: Self::failure_details(
                        &primary.stdout,
                        &primary.stderr,
                        &primary.status,
                    ),
                });
            }
        };

        let raw = Self::inject_runtime_meta(raw, should_try_airllm, airllm_used);

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
