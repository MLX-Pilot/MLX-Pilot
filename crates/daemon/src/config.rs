use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSecurityConfig {
    #[serde(default)]
    pub tool_allowlist: Vec<String>,
    #[serde(default)]
    pub tool_denylist: Vec<String>,
    #[serde(default)]
    pub exec_safe_bins: Vec<String>,
    #[serde(default)]
    pub exec_deny_patterns: Vec<String>,
    #[serde(default)]
    pub sensitive_paths: Vec<String>,
    #[serde(default)]
    pub egress_allow_domains: Vec<String>,
}

impl Default for AgentSecurityConfig {
    fn default() -> Self {
        Self {
            tool_allowlist: Vec::new(),
            tool_denylist: Vec::new(),
            exec_safe_bins: vec![
                "ls".to_string(),
                "cat".to_string(),
                "grep".to_string(),
                "git".to_string(),
                "find".to_string(),
                "rg".to_string(),
            ],
            exec_deny_patterns: vec![
                "rm -rf *".to_string(),
                "sudo *".to_string(),
                "chmod 777 *".to_string(),
                "mkfs*".to_string(),
            ],
            sensitive_paths: vec![
                "~/.ssh/*".to_string(),
                "~/.aws/*".to_string(),
                "~/.gnupg/*".to_string(),
                "**/.env".to_string(),
                "**/.env.*".to_string(),
            ],
            egress_allow_domains: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentUiConfig {
    #[serde(default = "default_agent_provider")]
    pub provider: String,
    #[serde(default = "default_agent_model")]
    pub model_id: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub custom_headers: BTreeMap<String, String>,
    #[serde(default = "default_agent_execution_mode")]
    pub execution_mode: String,
    #[serde(default = "default_agent_approval_mode")]
    pub approval_mode: String,
    #[serde(default)]
    pub streaming: bool,
    #[serde(default)]
    pub fallback_enabled: bool,
    #[serde(default = "default_agent_fallback_provider")]
    pub fallback_provider: String,
    #[serde(default)]
    pub fallback_model_id: String,
    #[serde(default)]
    pub max_prompt_tokens: Option<usize>,
    #[serde(default)]
    pub max_history_messages: Option<usize>,
    #[serde(default)]
    pub max_tools_in_prompt: Option<usize>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub aggressive_tool_filtering: bool,
    #[serde(default = "default_true")]
    pub enable_tool_call_fallback: bool,
    #[serde(default)]
    pub enabled_skills: Vec<String>,
    #[serde(default)]
    pub enabled_tools: Vec<String>,
    #[serde(default)]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub security: AgentSecurityConfig,
}

impl Default for AgentUiConfig {
    fn default() -> Self {
        Self {
            provider: default_agent_provider(),
            model_id: default_agent_model(),
            base_url: String::new(),
            api_key: String::new(),
            custom_headers: BTreeMap::new(),
            execution_mode: default_agent_execution_mode(),
            approval_mode: default_agent_approval_mode(),
            streaming: false,
            fallback_enabled: false,
            fallback_provider: default_agent_fallback_provider(),
            fallback_model_id: String::new(),
            max_prompt_tokens: Some(2200),
            max_history_messages: Some(14),
            max_tools_in_prompt: Some(6),
            temperature: Some(0.1),
            aggressive_tool_filtering: true,
            enable_tool_call_fallback: true,
            enabled_skills: Vec::new(),
            enabled_tools: vec![
                "read_file".to_string(),
                "write_file".to_string(),
                "edit_file".to_string(),
                "list_dir".to_string(),
                "exec".to_string(),
            ],
            workspace_root: None,
            security: AgentSecurityConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub bind_addr: SocketAddr,
    pub local_provider: String,
    pub models_dir: PathBuf,
    pub mlx_command: String,
    pub mlx_prefix_args: Vec<String>,
    pub mlx_suffix_args: Vec<String>,
    pub mlx_timeout: Duration,
    pub llamacpp_server_binary: String,
    pub llamacpp_base_url: String,
    pub llamacpp_timeout: Duration,
    pub llamacpp_startup_timeout: Duration,
    pub llamacpp_auto_start: bool,
    pub llamacpp_auto_install: bool,
    pub llamacpp_context_size: u32,
    pub llamacpp_gpu_layers: i32,
    pub llamacpp_extra_args: Vec<String>,
    pub ollama_base_url: String,
    pub ollama_timeout: Duration,
    pub ollama_startup_timeout: Duration,
    pub ollama_auto_start: bool,
    pub ollama_auto_install: bool,
    pub remote_downloads_dir: PathBuf,
    pub hf_api_base: String,
    pub hf_token: Option<String>,
    pub brave_api_key: Option<String>,
    pub catalog_search_limit: usize,
    pub catalog_download_timeout: Duration,
    pub openclaw_node_command: String,
    pub openclaw_cli_path: PathBuf,
    pub openclaw_state_dir: PathBuf,
    pub openclaw_gateway_token: String,
    pub openclaw_session_key: String,
    pub openclaw_timeout: Duration,
    pub openclaw_gateway_log: PathBuf,
    pub openclaw_error_log: PathBuf,
    pub openclaw_sync_log: PathBuf,
    pub nanobot_cli_path: PathBuf,
    pub active_agent_framework: String,
    #[serde(default)]
    pub agent: AgentUiConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 11435),
            local_provider: "auto".to_string(),
            models_dir: PathBuf::from("/Users/kaike/models"),
            mlx_command: default_mlx_command(),
            mlx_prefix_args: Vec::new(),
            mlx_suffix_args: Vec::new(),
            mlx_timeout: Duration::from_secs(900),
            llamacpp_server_binary: default_llamacpp_server_binary(),
            llamacpp_base_url: "http://127.0.0.1:11439".to_string(),
            llamacpp_timeout: Duration::from_secs(900),
            llamacpp_startup_timeout: Duration::from_secs(45),
            llamacpp_auto_start: true,
            llamacpp_auto_install: true,
            llamacpp_context_size: 16384,
            llamacpp_gpu_layers: 999,
            llamacpp_extra_args: Vec::new(),
            ollama_base_url: "http://127.0.0.1:11434".to_string(),
            ollama_timeout: Duration::from_secs(900),
            ollama_startup_timeout: Duration::from_secs(30),
            ollama_auto_start: true,
            ollama_auto_install: true,
            remote_downloads_dir: PathBuf::from("/Users/kaike/models"),
            hf_api_base: "https://huggingface.co".to_string(),
            hf_token: None,
            brave_api_key: None,
            catalog_search_limit: 18,
            catalog_download_timeout: Duration::from_secs(21600),
            openclaw_node_command: "node".to_string(),
            openclaw_cli_path: PathBuf::from("/Users/kaike/prod/openclaw/openclaw.mjs"),
            openclaw_state_dir: PathBuf::from("/Users/kaike/prod/openclaw/deploy/data"),
            openclaw_gateway_token: "openclaw-local-token".to_string(),
            openclaw_session_key: "agent:main:main".to_string(),
            openclaw_timeout: Duration::from_secs(120),
            openclaw_gateway_log: PathBuf::from(
                "/Users/kaike/prod/openclaw/deploy/data/logs/gateway.log",
            ),
            openclaw_error_log: PathBuf::from(
                "/Users/kaike/prod/openclaw/deploy/data/logs/gateway.err.log",
            ),
            openclaw_sync_log: PathBuf::from("/Users/kaike/openclaw-mlx-sync.log"),
            nanobot_cli_path: PathBuf::from("/Users/kaike/prod/nanobot"),
            active_agent_framework: "openclaw".to_string(),
            agent: AgentUiConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn get_settings_path() -> PathBuf {
        // Use home dir via env instead of the `dirs` crate
        let base = if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"))
        {
            PathBuf::from(home).join(".config")
        } else if let Ok(app_data) = std::env::var("APPDATA") {
            PathBuf::from(app_data)
        } else {
            PathBuf::from(".")
        };
        let mut path = base;
        path.push("mlx-ollama-pilot");
        let _ = fs::create_dir_all(&path);
        path.push("settings.json");
        path
    }

    pub fn load_settings() -> Self {
        let path = Self::get_settings_path();
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(config) = serde_json::from_str::<AppConfig>(&content) {
                    return config;
                }
            }
        }
        Self::default()
    }

    pub fn save_settings(&self) -> Result<(), std::io::Error> {
        let path = Self::get_settings_path();
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    pub fn apply_env(mut self) -> Self {
        if let Ok(value) = env::var("APP_BIND_ADDR") {
            if let Ok(addr) = value.parse() {
                self.bind_addr = addr;
            }
        }

        if let Ok(value) = env::var("APP_LOCAL_PROVIDER") {
            let normalized = value.trim().to_lowercase();
            let normalized = match normalized.as_str() {
                "llama" | "llama.cpp" => "llamacpp".to_string(),
                _ => normalized,
            };
            if matches!(normalized.as_str(), "auto" | "mlx" | "llamacpp" | "ollama") {
                self.local_provider = normalized;
            }
        }

        if let Ok(value) = env::var("APP_MODELS_DIR") {
            self.models_dir = PathBuf::from(value);
        }

        self.remote_downloads_dir = self.models_dir.clone();

        if let Ok(value) = env::var("APP_MLX_COMMAND") {
            if !value.trim().is_empty() {
                self.mlx_command = value;
            }
        }

        if let Ok(value) = env::var("APP_MLX_PREFIX_ARGS") {
            if !value.trim().is_empty() {
                self.mlx_prefix_args = parse_shell_args(&value).unwrap_or_else(|| {
                    value
                        .split_whitespace()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                });
            }
        }

        if let Ok(value) = env::var("APP_MLX_SUFFIX_ARGS") {
            if !value.trim().is_empty() {
                self.mlx_suffix_args = parse_shell_args(&value).unwrap_or_else(|| {
                    value
                        .split_whitespace()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                });
            }
        }

        if let Ok(value) = env::var("APP_MLX_TIMEOUT_SECS") {
            if let Ok(seconds) = value.parse::<u64>() {
                self.mlx_timeout = Duration::from_secs(seconds.max(1));
            }
        }

        if let Ok(value) = env::var("APP_LLAMACPP_SERVER_BINARY") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                self.llamacpp_server_binary = trimmed.to_string();
            }
        }

        if let Ok(value) = env::var("APP_LLAMACPP_BASE_URL") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                self.llamacpp_base_url = trimmed.to_string();
            }
        }

        if let Ok(value) = env::var("APP_LLAMACPP_TIMEOUT_SECS") {
            if let Ok(seconds) = value.parse::<u64>() {
                self.llamacpp_timeout = Duration::from_secs(seconds.max(1));
            }
        }

        if let Ok(value) = env::var("APP_LLAMACPP_STARTUP_TIMEOUT_SECS") {
            if let Ok(seconds) = value.parse::<u64>() {
                self.llamacpp_startup_timeout = Duration::from_secs(seconds.max(2));
            }
        }

        if let Ok(value) = env::var("APP_LLAMACPP_AUTO_START") {
            self.llamacpp_auto_start = parse_bool(&value, self.llamacpp_auto_start);
        }

        if let Ok(value) = env::var("APP_LLAMACPP_AUTO_INSTALL") {
            self.llamacpp_auto_install = parse_bool(&value, self.llamacpp_auto_install);
        }

        if let Ok(value) = env::var("APP_LLAMACPP_CONTEXT_SIZE") {
            if let Ok(parsed) = value.parse::<u32>() {
                self.llamacpp_context_size = parsed.max(1024);
            }
        }

        if let Ok(value) = env::var("APP_LLAMACPP_GPU_LAYERS") {
            if let Ok(parsed) = value.parse::<i32>() {
                self.llamacpp_gpu_layers = parsed.max(-1);
            }
        }

        if let Ok(value) = env::var("APP_LLAMACPP_EXTRA_ARGS") {
            if !value.trim().is_empty() {
                self.llamacpp_extra_args = parse_shell_args(&value).unwrap_or_else(|| {
                    value
                        .split_whitespace()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                });
            }
        }

        if let Ok(value) = env::var("APP_OLLAMA_BASE_URL") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                self.ollama_base_url = trimmed.to_string();
            }
        }

        if let Ok(value) = env::var("APP_OLLAMA_TIMEOUT_SECS") {
            if let Ok(seconds) = value.parse::<u64>() {
                self.ollama_timeout = Duration::from_secs(seconds.max(1));
            }
        }

        if let Ok(value) = env::var("APP_OLLAMA_STARTUP_TIMEOUT_SECS") {
            if let Ok(seconds) = value.parse::<u64>() {
                self.ollama_startup_timeout = Duration::from_secs(seconds.max(2));
            }
        }

        if let Ok(value) = env::var("APP_OLLAMA_AUTO_START") {
            self.ollama_auto_start = parse_bool(&value, self.ollama_auto_start);
        }

        if let Ok(value) = env::var("APP_OLLAMA_AUTO_INSTALL") {
            self.ollama_auto_install = parse_bool(&value, self.ollama_auto_install);
        }

        if let Ok(value) = env::var("APP_REMOTE_DOWNLOADS_DIR") {
            if !value.trim().is_empty() {
                self.remote_downloads_dir = PathBuf::from(value);
            }
        }

        if let Ok(value) = env::var("APP_HF_API_BASE") {
            if !value.trim().is_empty() {
                self.hf_api_base = value;
            }
        }

        if let Ok(value) = env::var("APP_HF_TOKEN") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                self.hf_token = Some(trimmed.to_string());
            }
        }

        if let Ok(value) = env::var("APP_BRAVE_API_KEY") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                self.brave_api_key = Some(trimmed.to_string());
            }
        } else if let Ok(value) = env::var("BRAVE_API_KEY") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                self.brave_api_key = Some(trimmed.to_string());
            }
        }

        if let Ok(value) = env::var("APP_CATALOG_SEARCH_LIMIT") {
            if let Ok(parsed) = value.parse::<usize>() {
                self.catalog_search_limit = parsed.clamp(1, 40);
            }
        }

        if let Ok(value) = env::var("APP_CATALOG_DOWNLOAD_TIMEOUT_SECS") {
            if let Ok(seconds) = value.parse::<u64>() {
                self.catalog_download_timeout = Duration::from_secs(seconds.max(30));
            }
        }

        if let Ok(value) = env::var("APP_OPENCLAW_NODE_COMMAND") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                self.openclaw_node_command = trimmed.to_string();
            }
        }

        if let Ok(value) = env::var("APP_OPENCLAW_CLI_PATH") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                self.openclaw_cli_path = PathBuf::from(trimmed);
            }
        }

        if let Ok(value) = env::var("APP_OPENCLAW_STATE_DIR") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                self.openclaw_state_dir = PathBuf::from(trimmed);
            }
        }

        if let Ok(value) = env::var("APP_OPENCLAW_GATEWAY_TOKEN") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                self.openclaw_gateway_token = trimmed.to_string();
            }
        }

        if let Ok(value) = env::var("APP_OPENCLAW_SESSION_KEY") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                self.openclaw_session_key = trimmed.to_string();
            }
        }

        if let Ok(value) = env::var("APP_OPENCLAW_TIMEOUT_SECS") {
            if let Ok(seconds) = value.parse::<u64>() {
                self.openclaw_timeout = Duration::from_secs(seconds.max(5));
            }
        }

        if let Ok(value) = env::var("APP_OPENCLAW_GATEWAY_LOG") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                self.openclaw_gateway_log = PathBuf::from(trimmed);
            }
        }

        if let Ok(value) = env::var("APP_OPENCLAW_ERROR_LOG") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                self.openclaw_error_log = PathBuf::from(trimmed);
            }
        }

        if let Ok(value) = env::var("APP_OPENCLAW_SYNC_LOG") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                self.openclaw_sync_log = PathBuf::from(trimmed);
            }
        }

        if let Ok(value) = env::var("APP_NANOBOT_CLI_PATH") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                self.nanobot_cli_path = PathBuf::from(trimmed);
            }
        }

        if let Ok(value) = env::var("APP_ACTIVE_AGENT_FRAMEWORK") {
            let normalized = value.trim().to_ascii_lowercase();
            if matches!(normalized.as_str(), "openclaw" | "nanobot") {
                self.active_agent_framework = normalized;
            }
        }

        if let Ok(value) = env::var("APP_AGENT_PROVIDER") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                self.agent.provider = trimmed.to_string();
            }
        }

        if let Ok(value) = env::var("APP_AGENT_MODEL") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                self.agent.model_id = trimmed.to_string();
            }
        }

        if let Ok(value) = env::var("APP_AGENT_BASE_URL") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                self.agent.base_url = trimmed.to_string();
            }
        }

        if let Ok(value) = env::var("APP_AGENT_API_KEY") {
            self.agent.api_key = value.trim().to_string();
        }

        if let Ok(value) = env::var("APP_AGENT_APPROVAL_MODE") {
            let mode = value.trim().to_ascii_lowercase();
            if matches!(mode.as_str(), "auto" | "ask" | "deny") {
                self.agent.approval_mode = mode;
            }
        }

        normalize_mlx_command(&mut self);

        self
    }
}

fn parse_shell_args(value: &str) -> Option<Vec<String>> {
    shell_words::split(value).ok()
}

fn default_mlx_command() -> String {
    let preferred = PathBuf::from("/Users/kaike/mlx-env/bin/mlx_lm.generate");
    if preferred.exists() {
        preferred.display().to_string()
    } else {
        "mlx_lm.generate".to_string()
    }
}

fn default_llamacpp_server_binary() -> String {
    let bundled = PathBuf::from("/Users/kaike/mlx-ollama-pilot/bin/llama-server");
    if bundled.exists() {
        bundled.display().to_string()
    } else {
        "llama-server".to_string()
    }
}

fn normalize_mlx_command(cfg: &mut AppConfig) {
    let raw_command = cfg.mlx_command.trim();

    if raw_command.chars().any(char::is_whitespace) {
        if let Some(parts) = parse_shell_args(raw_command) {
            if let Some((command, rest)) = parts.split_first() {
                cfg.mlx_command = command.clone();
                if cfg.mlx_prefix_args.is_empty() && !rest.is_empty() {
                    cfg.mlx_prefix_args = rest.to_vec();
                }
            }
        }
    }

    let is_python = matches!(cfg.mlx_command.as_str(), "python" | "python3");
    if !is_python {
        return;
    }

    if starts_with_legacy_module(&cfg.mlx_prefix_args, &["-m", "mlx_lm.generate"]) {
        cfg.mlx_command = default_mlx_command();
        cfg.mlx_prefix_args.drain(0..2);
        return;
    }

    if starts_with_legacy_module(&cfg.mlx_prefix_args, &["-m", "mlx_lm", "generate"]) {
        cfg.mlx_command = default_mlx_command();
        cfg.mlx_prefix_args.drain(0..3);
    }
}

fn starts_with_legacy_module(values: &[String], expected: &[&str]) -> bool {
    if values.len() < expected.len() {
        return false;
    }

    values
        .iter()
        .zip(expected.iter())
        .all(|(left, right)| left == right)
}

fn parse_bool(value: &str, fallback: bool) -> bool {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => fallback,
    }
}

fn default_true() -> bool {
    true
}

fn default_agent_provider() -> String {
    "ollama".to_string()
}

fn default_agent_fallback_provider() -> String {
    "mlx".to_string()
}

fn default_agent_model() -> String {
    "qwen2.5:7b".to_string()
}

fn default_agent_execution_mode() -> String {
    "full".to_string()
}

fn default_agent_approval_mode() -> String {
    "ask".to_string()
}
