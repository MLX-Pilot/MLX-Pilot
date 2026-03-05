use std::path::{Path, PathBuf};
#[allow(unused_imports)]
use std::process::{Command as StdCommand, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::timeout;

use mlx_ollama_core::{ChatMessage, ChatRequest, MessageRole};

#[derive(Debug, Clone)]
pub struct ChatRuntimeConfig {
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

#[derive(Debug, Clone, Serialize)]
pub struct ChatStreamEvent {
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tps: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_tps: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_memory_gb: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_metrics: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub airllm_required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub airllm_used: Option<bool>,
}

impl ChatStreamEvent {
    pub fn status(value: &str) -> Self {
        Self {
            event: "status".to_string(),
            status: Some(value.to_string()),
            delta: None,
            message: None,
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            prompt_tps: None,
            generation_tps: None,
            peak_memory_gb: None,
            latency_ms: None,
            raw_metrics: None,
            airllm_required: None,
            airllm_used: None,
        }
    }

    pub fn thinking_delta(delta: String) -> Self {
        Self {
            event: "thinking_delta".to_string(),
            status: None,
            delta: Some(delta),
            message: None,
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            prompt_tps: None,
            generation_tps: None,
            peak_memory_gb: None,
            latency_ms: None,
            raw_metrics: None,
            airllm_required: None,
            airllm_used: None,
        }
    }

    pub fn answer_delta(delta: String) -> Self {
        Self {
            event: "answer_delta".to_string(),
            status: None,
            delta: Some(delta),
            message: None,
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            prompt_tps: None,
            generation_tps: None,
            peak_memory_gb: None,
            latency_ms: None,
            raw_metrics: None,
            airllm_required: None,
            airllm_used: None,
        }
    }

    fn metrics(metrics: &ParsedMetrics) -> Self {
        Self {
            event: "metrics".to_string(),
            status: None,
            delta: None,
            message: None,
            prompt_tokens: metrics.prompt_tokens,
            completion_tokens: metrics.generation_tokens,
            total_tokens: None,
            prompt_tps: metrics.prompt_tps,
            generation_tps: metrics.generation_tps,
            peak_memory_gb: metrics.peak_memory_gb,
            latency_ms: None,
            raw_metrics: metrics
                .as_lines()
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string()),
            airllm_required: None,
            airllm_used: None,
        }
    }

    fn done(
        latency_ms: u64,
        prompt_tokens: usize,
        completion_tokens: usize,
        metrics: &ParsedMetrics,
        airllm_required: bool,
        airllm_used: bool,
    ) -> Self {
        Self {
            event: "done".to_string(),
            status: Some("completed".to_string()),
            delta: None,
            message: None,
            prompt_tokens: Some(prompt_tokens),
            completion_tokens: Some(completion_tokens),
            total_tokens: Some(prompt_tokens + completion_tokens),
            prompt_tps: metrics.prompt_tps,
            generation_tps: metrics.generation_tps,
            peak_memory_gb: metrics.peak_memory_gb,
            latency_ms: Some(latency_ms),
            raw_metrics: metrics
                .as_lines()
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string()),
            airllm_required: if airllm_required { Some(true) } else { None },
            airllm_used: if airllm_used { Some(true) } else { None },
        }
    }

    pub fn error(message: String) -> Self {
        Self {
            event: "error".to_string(),
            status: Some("error".to_string()),
            delta: None,
            message: Some(message),
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            prompt_tps: None,
            generation_tps: None,
            peak_memory_gb: None,
            latency_ms: None,
            raw_metrics: None,
            airllm_required: None,
            airllm_used: None,
        }
    }

    pub fn airllm_log(message: String) -> Self {
        Self {
            event: "airllm_log".to_string(),
            status: None,
            delta: None,
            message: Some(message),
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            prompt_tps: None,
            generation_tps: None,
            peak_memory_gb: None,
            latency_ms: None,
            raw_metrics: None,
            airllm_required: None,
            airllm_used: None,
        }
    }
}

#[derive(Debug)]
pub enum ChatStreamError {
    InvalidRequest(String),
    ModelNotFound(String),
    Io(String),
    CommandFailed(String),
    Timeout(u64),
    ClientDisconnected,
}

impl std::fmt::Display for ChatStreamError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChatStreamError::InvalidRequest(message) => write!(formatter, "{message}"),
            ChatStreamError::ModelNotFound(message) => write!(formatter, "{message}"),
            ChatStreamError::Io(message) => write!(formatter, "{message}"),
            ChatStreamError::CommandFailed(message) => write!(formatter, "{message}"),
            ChatStreamError::Timeout(seconds) => {
                write!(formatter, "geracao expirou apos {seconds}s")
            }
            ChatStreamError::ClientDisconnected => {
                write!(formatter, "cliente desconectado")
            }
        }
    }
}

impl std::error::Error for ChatStreamError {}

pub fn spawn_chat_stream(
    cfg: ChatRuntimeConfig,
    request: ChatRequest,
) -> mpsc::Receiver<ChatStreamEvent> {
    let (tx, rx) = mpsc::channel(256);

    tokio::spawn(async move {
        let result = run_chat_stream(cfg, request, tx.clone()).await;
        if let Err(error) = result {
            if matches!(error, ChatStreamError::ClientDisconnected) {
                return;
            }
            let _ = tx.send(ChatStreamEvent::error(error.to_string())).await;
        }
    });

    rx
}

async fn run_chat_stream(
    cfg: ChatRuntimeConfig,
    request: ChatRequest,
    tx: mpsc::Sender<ChatStreamEvent>,
) -> Result<(), ChatStreamError> {
    if request.model_id.trim().is_empty() {
        return Err(ChatStreamError::InvalidRequest(
            "model_id nao pode ser vazio".to_string(),
        ));
    }

    if request.messages.is_empty() {
        return Err(ChatStreamError::InvalidRequest(
            "messages nao pode ser vazio".to_string(),
        ));
    }

    let model_path = resolve_model_path(&cfg, &request.model_id);
    if !model_path.exists() {
        return Err(ChatStreamError::ModelNotFound(format!(
            "modelo '{}' nao encontrado",
            request.model_id
        )));
    }

    let model_scan = scan_model_dir(&model_path)
        .await
        .map_err(|error| ChatStreamError::Io(format!("falha lendo modelo: {error}")))?;
    if !model_scan.is_runnable() {
        return Err(ChatStreamError::InvalidRequest(format!(
            "modelo '{}' nao possui pesos safetensors validos para mlx_lm.generate",
            model_path.display()
        )));
    }

    let memory_profile = memory_profile(model_scan.safetensors_bytes);
    let should_try_airllm = should_try_airllm(&cfg, memory_profile, request.options.airllm_enabled);

    let prompt = build_prompt(&request.messages);
    let mut args = cfg.command_prefix_args.clone();
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

    args.extend(cfg.command_suffix_args.clone());
    if should_try_airllm {
        append_large_model_guard_args(&mut args);
    }
    let command_preview = format!("{} {}", cfg.command, args.join(" "));
    let prompt_for_command = prompt.clone();

    send_event(&tx, ChatStreamEvent::status("waiting")).await?;
    if should_try_airllm {
        send_event(&tx, ChatStreamEvent::status("airllm_required")).await?;
    }

    let started = Instant::now();

    let command_future = async {
        let mut command = Command::new(&cfg.command);
        command
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = command
            .spawn()
            .map_err(|error| ChatStreamError::Io(format!("falha ao executar comando: {error}")))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ChatStreamError::Io("stdout indisponivel".to_string()))?;

        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| ChatStreamError::Io("stderr indisponivel".to_string()))?;

        let stderr_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut collected = Vec::new();
            let _ = reader.read_to_end(&mut collected).await;
            String::from_utf8_lossy(&collected).to_string()
        });

        let mut reader = BufReader::new(stdout);
        let mut buffer = [0_u8; 4096];
        let mut collected = String::new();

        let mut sent_thinking = 0_usize;
        let mut sent_answer = 0_usize;
        let mut sent_metrics: Option<ParsedMetrics> = None;

        let mut announced_thinking = false;
        let mut announced_answering = false;

        loop {
            let read = tokio::select! {
                _ = tx.closed() => {
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                    return Err(ChatStreamError::ClientDisconnected);
                }
                read = reader.read(&mut buffer) => {
                    read.map_err(|error| ChatStreamError::Io(format!("falha lendo stdout: {error}")))?
                }
            };

            if read == 0 {
                break;
            }

            collected.push_str(&String::from_utf8_lossy(&buffer[..read]));
            let parsed = ParsedOutput::parse(&collected);

            if parsed.thinking.len() > sent_thinking {
                if !announced_thinking {
                    send_event(&tx, ChatStreamEvent::status("thinking")).await?;
                    announced_thinking = true;
                }

                if let Some(delta) = parsed.thinking.get(sent_thinking..) {
                    send_event(&tx, ChatStreamEvent::thinking_delta(delta.to_string())).await?;
                }
                sent_thinking = parsed.thinking.len();
            }

            if parsed.answer.len() > sent_answer {
                if !announced_answering {
                    send_event(&tx, ChatStreamEvent::status("answering")).await?;
                    announced_answering = true;
                }

                if let Some(delta) = parsed.answer.get(sent_answer..) {
                    send_event(&tx, ChatStreamEvent::answer_delta(delta.to_string())).await?;
                }
                sent_answer = parsed.answer.len();
            }

            if parsed.metrics.has_any() {
                let should_emit = sent_metrics
                    .as_ref()
                    .map(|previous| previous != &parsed.metrics)
                    .unwrap_or(true);

                if should_emit {
                    send_event(&tx, ChatStreamEvent::metrics(&parsed.metrics)).await?;
                    sent_metrics = Some(parsed.metrics.clone());
                }
            }
        }

        let status = child
            .wait()
            .await
            .map_err(|error| ChatStreamError::Io(format!("falha aguardando comando: {error}")))?;

        let stderr_output = stderr_task
            .await
            .map_err(|error| ChatStreamError::Io(format!("falha ao aguardar stderr: {error}")))?;

        if !status.success() {
            let mut failure_details = describe_exit_status(status);
            let trimmed_stderr = stderr_output.trim();
            if !trimmed_stderr.is_empty() {
                failure_details.push_str("; ");
                failure_details.push_str(trimmed_stderr);
            } else {
                let stdout_tail = tail_text(&collected, 700);
                if !stdout_tail.is_empty() {
                    failure_details.push_str("; stdout: ");
                    failure_details.push_str(&stdout_tail);
                }
            }

            return Err(ChatStreamError::CommandFailed(format!(
                "comando '{}' falhou: {}",
                command_preview, failure_details
            )));
        }

        Ok((
            collected,
            prompt_for_command,
            started.elapsed().as_millis() as u64,
        ))
    };

    let primary_result = timeout(cfg.timeout, command_future)
        .await
        .map_err(|_| ChatStreamError::Timeout(cfg.timeout.as_secs()))?;

    let airllm_required = should_try_airllm;
    let (raw_output, prompt_text, latency_ms, used_fallback) = match primary_result {
        Ok((raw_output, prompt_text, latency_ms)) => (raw_output, prompt_text, latency_ms, false),
        Err(ChatStreamError::CommandFailed(message))
            if should_try_airllm && is_memory_pressure_error(&message) =>
        {
            send_event(&tx, ChatStreamEvent::status("fallback_airllm")).await?;
            send_event(
                &tx,
                ChatStreamEvent::airllm_log(format!(
                    "Fallback AIRLLM iniciado com python '{}'",
                    cfg.airllm_python_command
                )),
            )
            .await?;
            send_event(
                &tx,
                ChatStreamEvent::airllm_log(format!("Runner AIRLLM: {}", cfg.airllm_runner)),
            )
            .await?;
            let started_fallback = Instant::now();
            let fallback_output =
                run_airllm_bridge(&cfg, &model_path, &request, &prompt, &tx).await?;
            (
                fallback_output,
                prompt.clone(),
                started_fallback.elapsed().as_millis() as u64,
                true,
            )
        }
        Err(error) => return Err(error),
    };

    let parsed = ParsedOutput::parse(&raw_output);

    if used_fallback {
        if !parsed.thinking.trim().is_empty() {
            send_event(
                &tx,
                ChatStreamEvent::thinking_delta(parsed.thinking.clone()),
            )
            .await?;
        }
        if !parsed.answer.trim().is_empty() {
            send_event(&tx, ChatStreamEvent::answer_delta(parsed.answer.clone())).await?;
        } else {
            let fallback_text = raw_output.trim().to_string();
            if !fallback_text.is_empty() {
                send_event(&tx, ChatStreamEvent::answer_delta(fallback_text)).await?;
            }
        }
    }

    let prompt_tokens = parsed
        .metrics
        .prompt_tokens
        .unwrap_or_else(|| prompt_text.split_whitespace().count());

    let completion_tokens = parsed
        .metrics
        .generation_tokens
        .unwrap_or_else(|| parsed.answer.split_whitespace().count());

    send_event(
        &tx,
        ChatStreamEvent::done(
            latency_ms,
            prompt_tokens,
            completion_tokens,
            &parsed.metrics,
            airllm_required,
            used_fallback,
        ),
    )
    .await?;

    Ok(())
}

async fn send_event(
    tx: &mpsc::Sender<ChatStreamEvent>,
    event: ChatStreamEvent,
) -> Result<(), ChatStreamError> {
    tx.send(event)
        .await
        .map_err(|_| ChatStreamError::ClientDisconnected)
}

#[derive(Debug, Clone, Copy)]
struct MemoryProfile {
    usage_ratio: f64,
}

fn has_arg(args: &[String], flag: &str) -> bool {
    args.iter().any(|value| value == flag)
}

fn append_large_model_guard_args(args: &mut Vec<String>) {
    if !has_arg(args, "--max-kv-size") {
        args.push("--max-kv-size".to_string());
        args.push("1024".to_string());
    }
    if !has_arg(args, "--kv-bits") {
        args.push("--kv-bits".to_string());
        args.push("4".to_string());
    }
    if !has_arg(args, "--kv-group-size") {
        args.push("--kv-group-size".to_string());
        args.push("64".to_string());
    }
    if !has_arg(args, "--quantized-kv-start") {
        args.push("--quantized-kv-start".to_string());
        args.push("0".to_string());
    }
}

fn memory_profile(model_bytes: u64) -> Option<MemoryProfile> {
    let system_memory_bytes = detect_system_memory_bytes()?;
    if model_bytes == 0 || system_memory_bytes == 0 {
        return None;
    }
    Some(MemoryProfile {
        usage_ratio: model_bytes as f64 / system_memory_bytes as f64,
    })
}

fn should_try_airllm(
    cfg: &ChatRuntimeConfig,
    profile: Option<MemoryProfile>,
    request_override: Option<bool>,
) -> bool {
    let enabled = request_override.unwrap_or(cfg.airllm_enabled);
    if !enabled {
        return false;
    }
    if request_override == Some(true) {
        return true;
    }
    let Some(profile) = profile else { return false };
    let threshold = (cfg.airllm_threshold_percent as f64 / 100.0).clamp(0.0, 1.0);
    profile.usage_ratio >= threshold
}

fn is_memory_pressure_error(text: &str) -> bool {
    let normalized = text.to_ascii_lowercase();
    // Explicit memory pressure keywords
    if normalized.contains("insufficient memory")
        || normalized.contains("out of memory")
        || normalized.contains("outofmemory")
        || normalized.contains("kiogpucommandbuffercallbackerroroutofmemory")
        || normalized.contains("max_recommended_working_set_size")
    {
        return true;
    }

    // Signal termination with Metal/GPU crash indicators — stderr may not have
    // flushed the explicit memory error text before the process was killed.
    let signal_terminated = normalized.contains("terminated by signal")
        || normalized.contains("libc++abi")
        || normalized.contains("uncaught exception");
    let gpu_crash = normalized.contains("[metal]")
        || normalized.contains("command buffer")
        || normalized.contains("iokit")
        || normalized.contains("iogpu");
    if signal_terminated && gpu_crash {
        return true;
    }

    false
}

fn normalize_airllm_backend(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "original" | "airllm" | "airllm-original" => "original",
        "legacy" | "legacy-bridge" | "mlx-lm" => "legacy",
        _ => "auto",
    }
}

fn bridge_device_hint(backend: &str) -> &'static str {
    if backend == "legacy" {
        // Legacy bridge path can hit the same MLX memory pressure; keep CPU fallback there.
        "cpu"
    } else {
        // Original AirLLM should keep automatic device selection (GPU/MLX when available).
        "auto"
    }
}

async fn run_airllm_bridge(
    cfg: &ChatRuntimeConfig,
    model_path: &Path,
    request: &ChatRequest,
    prompt: &str,
    tx: &mpsc::Sender<ChatStreamEvent>,
) -> Result<String, ChatStreamError> {
    let runner_path = PathBuf::from(&cfg.airllm_runner);
    if !runner_path.exists() {
        let cwd = std::env::current_dir()
            .ok()
            .map(|dir| dir.display().to_string())
            .unwrap_or_else(|| "<unknown>".to_string());
        return Err(ChatStreamError::CommandFailed(format!(
            "fallback airllm runner nao encontrado: {} (cwd: {})",
            runner_path.display(),
            cwd
        )));
    }

    let fallback_timeout = cfg.timeout.min(Duration::from_secs(300));
    let backend = normalize_airllm_backend(&cfg.airllm_backend);
    let device_hint = bridge_device_hint(backend);

    send_event(
        tx,
        ChatStreamEvent::airllm_log(format!(
            "Bridge AIRLLM backend='{}' device='{}'",
            backend, device_hint
        )),
    )
    .await?;

    let mut args = vec![
        cfg.airllm_runner.clone(),
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

    let mut command = Command::new(&cfg.airllm_python_command);
    command
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = command.spawn().map_err(|error| {
        ChatStreamError::Io(format!(
            "falha ao iniciar fallback airllm '{}': {error}",
            cfg.airllm_python_command
        ))
    })?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ChatStreamError::Io("stdout indisponivel no fallback airllm".to_string()))?;

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ChatStreamError::Io("stderr indisponivel no fallback airllm".to_string()))?;

    let stdout_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.map_err(|error| {
            ChatStreamError::Io(format!("falha lendo stdout do fallback airllm: {error}"))
        })?;
        Ok::<String, ChatStreamError>(String::from_utf8_lossy(&bytes).to_string())
    });

    let tx_for_stderr = tx.clone();
    let stderr_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        let mut collected = String::new();
        loop {
            line.clear();
            let read = reader.read_line(&mut line).await.map_err(|error| {
                ChatStreamError::Io(format!("falha lendo stderr do fallback airllm: {error}"))
            })?;
            if read == 0 {
                break;
            }
            collected.push_str(&line);
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                let _ = tx_for_stderr
                    .send(ChatStreamEvent::airllm_log(trimmed.to_string()))
                    .await;
            }
        }

        Ok::<String, ChatStreamError>(collected)
    });

    let status = match timeout(fallback_timeout, child.wait()).await {
        Ok(wait_result) => wait_result.map_err(|error| {
            ChatStreamError::Io(format!("falha aguardando fallback airllm: {error}"))
        })?,
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            let _ = send_event(
                tx,
                ChatStreamEvent::airllm_log(format!(
                    "Fallback AIRLLM expirou apos {}s e foi encerrado.",
                    fallback_timeout.as_secs()
                )),
            )
            .await;
            return Err(ChatStreamError::CommandFailed(format!(
                "fallback airllm expirou apos {}s",
                fallback_timeout.as_secs()
            )));
        }
    };

    let stdout = stdout_task
        .await
        .map_err(|error| ChatStreamError::Io(format!("falha no join de stdout: {error}")))??;
    let stderr = stderr_task
        .await
        .map_err(|error| ChatStreamError::Io(format!("falha no join de stderr: {error}")))??;
    let stderr = stderr.trim().to_string();

    if !status.success() {
        let details = if stderr.is_empty() {
            describe_exit_status(status)
        } else {
            format!("{}; {}", describe_exit_status(status), stderr)
        };
        return Err(ChatStreamError::CommandFailed(format!(
            "fallback airllm falhou: {details}"
        )));
    }

    send_event(
        tx,
        ChatStreamEvent::airllm_log("Fallback AIRLLM concluiu com sucesso.".to_string()),
    )
    .await?;

    Ok(stdout)
}

fn resolve_model_path(cfg: &ChatRuntimeConfig, model_id: &str) -> PathBuf {
    let candidate = PathBuf::from(model_id);
    if candidate.is_absolute() {
        candidate
    } else {
        cfg.models_dir.join(model_id)
    }
}

fn build_prompt(messages: &[ChatMessage]) -> String {
    let mut prompt = String::new();

    for message in messages {
        match message.role {
            MessageRole::System => {
                prompt.push_str("[SYSTEM]\n");
                prompt.push_str(message.content.trim());
                prompt.push_str("\n\n");
            }
            MessageRole::User => {
                prompt.push_str("[USER]\n");
                prompt.push_str(message.content.trim());
                prompt.push_str("\n\n");
            }
            MessageRole::Assistant => {
                prompt.push_str("[ASSISTANT]\n");
                prompt.push_str(message.content.trim());
                prompt.push_str("\n\n");
            }
            MessageRole::Tool => {
                prompt.push_str("[TOOL]\n");
                prompt.push_str(message.content.trim());
                prompt.push_str("\n\n");
            }
        }
    }

    prompt.push_str("[ASSISTANT]\n");
    prompt
}

#[derive(Debug, Clone, PartialEq, Default)]
struct ParsedMetrics {
    prompt_tokens: Option<usize>,
    prompt_tps: Option<f32>,
    generation_tokens: Option<usize>,
    generation_tps: Option<f32>,
    peak_memory_gb: Option<f32>,
}

impl ParsedMetrics {
    fn has_any(&self) -> bool {
        self.prompt_tokens.is_some()
            || self.prompt_tps.is_some()
            || self.generation_tokens.is_some()
            || self.generation_tps.is_some()
            || self.peak_memory_gb.is_some()
    }

    fn as_lines(&self) -> Option<String> {
        if !self.has_any() {
            return None;
        }

        let mut lines = Vec::new();

        if let (Some(tokens), Some(tps)) = (self.prompt_tokens, self.prompt_tps) {
            lines.push(format!("Prompt: {tokens} tokens, {tps:.3} tokens-per-sec"));
        } else if let Some(tokens) = self.prompt_tokens {
            lines.push(format!("Prompt: {tokens} tokens"));
        }

        if let (Some(tokens), Some(tps)) = (self.generation_tokens, self.generation_tps) {
            lines.push(format!(
                "Generation: {tokens} tokens, {tps:.3} tokens-per-sec"
            ));
        } else if let Some(tokens) = self.generation_tokens {
            lines.push(format!("Generation: {tokens} tokens"));
        }

        if let Some(memory) = self.peak_memory_gb {
            lines.push(format!("Peak memory: {memory:.3} GB"));
        }

        Some(lines.join("\n"))
    }
}

#[derive(Debug, Clone)]
struct ParsedOutput {
    thinking: String,
    answer: String,
    metrics: ParsedMetrics,
}

impl ParsedOutput {
    fn parse(raw: &str) -> Self {
        let normalized = raw.replace("\r\n", "\n");

        let (content_raw, metrics_raw) = split_sections(&normalized);
        let (thinking, answer) = split_thinking_and_answer(content_raw);

        let mut metrics = parse_metrics(metrics_raw);
        if !metrics.has_any() {
            metrics = parse_metrics(&normalized);
        }

        Self {
            thinking: trim_leading_newlines(&thinking),
            answer: trim_leading_newlines(&answer),
            metrics,
        }
    }
}

fn split_sections(normalized: &str) -> (&str, &str) {
    let marker = "==========";

    if let Some(first_index) = normalized.find(marker) {
        let after_first = trim_leading_newline_ref(&normalized[first_index + marker.len()..]);

        if let Some(second_index) = after_first.find(marker) {
            let content = trim_leading_newline_ref(&after_first[..second_index]);
            let metrics = trim_leading_newline_ref(&after_first[second_index + marker.len()..]);
            return (content, metrics);
        }

        return (after_first, "");
    }

    (trim_leading_newline_ref(normalized), "")
}

fn split_thinking_and_answer(content: &str) -> (String, String) {
    let think_open = "<think>";
    let think_close = "</think>";

    if let Some(open_index) = content.find(think_open) {
        let after_open = &content[open_index + think_open.len()..];

        if let Some(close_index) = after_open.find(think_close) {
            let thinking = &after_open[..close_index];
            let answer = &after_open[close_index + think_close.len()..];
            return (thinking.to_string(), answer.to_string());
        }

        return (after_open.to_string(), String::new());
    }

    (String::new(), content.to_string())
}

fn trim_leading_newline_ref(value: &str) -> &str {
    value.trim_start_matches('\n')
}

fn trim_leading_newlines(value: &str) -> String {
    value.trim_start_matches('\n').to_string()
}

fn parse_metrics(raw: &str) -> ParsedMetrics {
    let mut metrics = ParsedMetrics::default();

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("Prompt:") {
            let (tokens, tps) = parse_tokens_and_rate(rest);
            metrics.prompt_tokens = tokens;
            metrics.prompt_tps = tps;
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("Generation:") {
            let (tokens, tps) = parse_tokens_and_rate(rest);
            metrics.generation_tokens = tokens;
            metrics.generation_tps = tps;
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("Peak memory:") {
            metrics.peak_memory_gb = rest.split_whitespace().next().and_then(parse_f32_flexible);
        }
    }

    metrics
}

fn parse_tokens_and_rate(rest: &str) -> (Option<usize>, Option<f32>) {
    let mut tokens = None;
    let mut tps = None;

    let mut parts = rest.split(',');

    if let Some(first) = parts.next() {
        tokens = first
            .split_whitespace()
            .next()
            .and_then(|value| value.parse::<usize>().ok());
    }

    if let Some(second) = parts.next() {
        tps = second
            .split_whitespace()
            .next()
            .and_then(parse_f32_flexible);
    }

    (tokens, tps)
}

fn parse_f32_flexible(value: &str) -> Option<f32> {
    let normalized = value.trim().replace(',', ".");
    normalized.parse::<f32>().ok()
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

        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if name == "config.json" {
            has_config = true;
        }
        if name == "model.safetensors.index.json" {
            has_safetensors_index = true;
        }
        if name.ends_with(".safetensors") {
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

fn detect_system_memory_bytes() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        let output = StdCommand::new("sysctl")
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

fn describe_exit_status(status: ExitStatus) -> String {
    status
        .code()
        .map(|value| format!("exit code {value}"))
        .unwrap_or_else(|| "terminated by signal".to_string())
}

fn tail_text(raw: &str, max_chars: usize) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let total_chars = trimmed.chars().count();
    if total_chars <= max_chars {
        return trimmed.to_string();
    }

    let tail = trimmed
        .chars()
        .skip(total_chars.saturating_sub(max_chars))
        .collect::<String>();
    format!("...{tail}")
}
