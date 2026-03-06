use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSecurityConfig {
    #[serde(default = "default_security_mode")]
    pub security_mode: String,
    #[serde(default)]
    pub require_capabilities: bool,
    #[serde(default)]
    pub airgapped: bool,
    #[serde(default)]
    pub owner_only: bool,
    #[serde(default = "default_true")]
    pub block_direct_ip_egress: bool,
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
    #[serde(default)]
    pub skill_sha256_pins: BTreeMap<String, String>,
    #[serde(default = "default_true")]
    pub use_secrets_vault: bool,
}

impl Default for AgentSecurityConfig {
    fn default() -> Self {
        Self {
            security_mode: default_security_mode(),
            require_capabilities: false,
            airgapped: false,
            owner_only: false,
            block_direct_ip_egress: true,
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
            skill_sha256_pins: BTreeMap::new(),
            use_secrets_vault: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSkillOverride {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub env_refs: BTreeMap<String, String>,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
}

impl Default for AgentSkillOverride {
    fn default() -> Self {
        Self {
            enabled: None,
            env: BTreeMap::new(),
            env_refs: BTreeMap::new(),
            config: BTreeMap::new(),
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
    pub api_key_ref: Option<String>,
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
    #[serde(default = "default_agent_node_manager")]
    pub node_package_manager: String,
    #[serde(default)]
    pub skill_overrides: BTreeMap<String, AgentSkillOverride>,
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
            api_key_ref: None,
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
            node_package_manager: default_agent_node_manager(),
            skill_overrides: BTreeMap::new(),
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
pub struct PluginPersistedState {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_config_object")]
    pub config: Value,
}

impl PluginPersistedState {
    pub fn is_configured(&self) -> bool {
        json_value_is_configured(&self.config)
    }
}

impl Default for PluginPersistedState {
    fn default() -> Self {
        Self {
            enabled: false,
            config: default_config_object(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelPersistedState {
    #[serde(default)]
    pub default_account_id: Option<String>,
    #[serde(default)]
    pub accounts: BTreeMap<String, ChannelAccountPersistedState>,
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default = "default_config_object")]
    pub config: Value,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

impl ChannelPersistedState {
    #[allow(dead_code)]
    pub fn is_configured(&self) -> bool {
        !self.accounts.is_empty()
            || self
                .default_account_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_some()
            || self
                .accounts
                .values()
                .any(ChannelAccountPersistedState::is_configured)
            || self.accounts.values().any(|account| account.enabled)
            || json_value_is_configured(&self.config)
            || self.metadata.values().any(|value| !value.trim().is_empty())
            || self
                .alias
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_some()
    }
}

impl Default for ChannelPersistedState {
    fn default() -> Self {
        Self {
            default_account_id: None,
            accounts: BTreeMap::new(),
            alias: None,
            config: default_config_object(),
            metadata: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelAccountPersistedState {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub credentials_ref: Option<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    #[serde(default)]
    pub routing_defaults: BTreeMap<String, String>,
    #[serde(default)]
    pub health_state: ChannelAccountHealthState,
    #[serde(default)]
    pub limits: ChannelAccountPolicy,
    #[serde(default = "default_config_object")]
    pub adapter_config: Value,
}

impl ChannelAccountPersistedState {
    pub fn is_configured(&self) -> bool {
        self.credentials_ref
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_some()
            || !self.metadata.is_empty()
            || !self.routing_defaults.is_empty()
            || json_value_is_configured(&self.adapter_config)
    }
}

impl Default for ChannelAccountPersistedState {
    fn default() -> Self {
        Self {
            enabled: true,
            credentials_ref: None,
            metadata: BTreeMap::new(),
            routing_defaults: BTreeMap::new(),
            health_state: ChannelAccountHealthState::default(),
            limits: ChannelAccountPolicy::default(),
            adapter_config: default_config_object(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelAccountHealthState {
    #[serde(default = "default_channel_health_status")]
    pub status: String,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub failure_count: u32,
    #[serde(default)]
    pub circuit_open_until_epoch_ms: Option<u128>,
    #[serde(default)]
    pub last_checked_epoch_ms: Option<u128>,
    #[serde(default)]
    pub last_connected_epoch_ms: Option<u128>,
}

impl Default for ChannelAccountHealthState {
    fn default() -> Self {
        Self {
            status: default_channel_health_status(),
            last_error: None,
            failure_count: 0,
            circuit_open_until_epoch_ms: None,
            last_checked_epoch_ms: None,
            last_connected_epoch_ms: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelAccountPolicy {
    #[serde(default = "default_rate_limit_per_minute")]
    pub rate_limit_per_minute: u32,
    #[serde(default = "default_operation_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_retry_count")]
    pub max_retries: u32,
    #[serde(default = "default_backoff_base_ms")]
    pub backoff_base_ms: u64,
    #[serde(default = "default_circuit_breaker_threshold")]
    pub circuit_breaker_threshold: u32,
    #[serde(default = "default_circuit_breaker_open_ms")]
    pub circuit_breaker_open_ms: u64,
}

impl Default for ChannelAccountPolicy {
    fn default() -> Self {
        Self {
            rate_limit_per_minute: default_rate_limit_per_minute(),
            timeout_ms: default_operation_timeout_ms(),
            max_retries: default_retry_count(),
            backoff_base_ms: default_backoff_base_ms(),
            circuit_breaker_threshold: default_circuit_breaker_threshold(),
            circuit_breaker_open_ms: default_circuit_breaker_open_ms(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompatibilityConfig {
    #[serde(default)]
    pub plugins: BTreeMap<String, PluginPersistedState>,
    #[serde(default)]
    pub channels: BTreeMap<String, ChannelPersistedState>,
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
    #[serde(default = "default_true")]
    pub mlx_airllm_enabled: bool,
    #[serde(default = "default_mlx_airllm_threshold_percent")]
    pub mlx_airllm_threshold_percent: u8,
    #[serde(default = "default_true")]
    pub mlx_airllm_safe_mode: bool,
    #[serde(default = "default_mlx_airllm_python_command")]
    pub mlx_airllm_python_command: String,
    #[serde(default = "default_mlx_airllm_runner")]
    pub mlx_airllm_runner: String,
    #[serde(default = "default_mlx_airllm_backend")]
    pub mlx_airllm_backend: String,
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
    #[serde(default)]
    pub compatibility: CompatibilityConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        let models_dir = default_models_dir();
        let app_data_dir = default_app_data_dir();
        let openclaw_state_dir = default_openclaw_state_dir();
        let openclaw_logs_dir = openclaw_state_dir.join("logs");

        Self {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 11435),
            local_provider: "auto".to_string(),
            models_dir: models_dir.clone(),
            mlx_command: default_mlx_command(),
            mlx_prefix_args: Vec::new(),
            mlx_suffix_args: Vec::new(),
            mlx_timeout: Duration::from_secs(900),
            mlx_airllm_enabled: true,
            mlx_airllm_threshold_percent: default_mlx_airllm_threshold_percent(),
            mlx_airllm_safe_mode: true,
            mlx_airllm_python_command: default_mlx_airllm_python_command(),
            mlx_airllm_runner: default_mlx_airllm_runner(),
            mlx_airllm_backend: default_mlx_airllm_backend(),
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
            remote_downloads_dir: models_dir,
            hf_api_base: "https://huggingface.co".to_string(),
            hf_token: None,
            brave_api_key: None,
            catalog_search_limit: 18,
            catalog_download_timeout: Duration::from_secs(21600),
            openclaw_node_command: "node".to_string(),
            openclaw_cli_path: app_data_dir.join("openclaw").join("openclaw.mjs"),
            openclaw_state_dir: openclaw_state_dir.clone(),
            openclaw_gateway_token: "openclaw-local-token".to_string(),
            openclaw_session_key: "agent:main:main".to_string(),
            openclaw_timeout: Duration::from_secs(120),
            openclaw_gateway_log: openclaw_logs_dir.join("gateway.log"),
            openclaw_error_log: openclaw_logs_dir.join("gateway.err.log"),
            openclaw_sync_log: app_data_dir.join("logs").join("openclaw-mlx-sync.log"),
            nanobot_cli_path: app_data_dir.join("nanobot"),
            active_agent_framework: "openclaw".to_string(),
            agent: AgentUiConfig::default(),
            compatibility: CompatibilityConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn get_settings_path() -> PathBuf {
        if let Ok(path) = env::var("APP_SETTINGS_PATH") {
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                let path = PathBuf::from(trimmed);
                if let Some(parent) = path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                return path;
            }
        }
        let base = if let Some(home) = home_dir() {
            home.join(".config")
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
        Self::load_settings_from(&Self::get_settings_path())
    }

    pub fn save_settings(&self) -> Result<(), std::io::Error> {
        self.save_settings_to(&Self::get_settings_path())
    }

    pub fn load_settings_from(path: &std::path::Path) -> Self {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(path) {
                if let Ok(config) = serde_json::from_str::<AppConfig>(&content) {
                    return config;
                }
            }
        }
        Self::default()
    }

    pub fn save_settings_to(&self, path: &std::path::Path) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
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

        if let Ok(value) = env::var("APP_MLX_AIRLLM_ENABLED") {
            self.mlx_airllm_enabled = parse_bool(&value, self.mlx_airllm_enabled);
        }

        if let Ok(value) = env::var("APP_MLX_AIRLLM_THRESHOLD_PERCENT") {
            if let Ok(parsed) = value.parse::<u8>() {
                self.mlx_airllm_threshold_percent = parsed.clamp(1, 100);
            }
        }

        if let Ok(value) = env::var("APP_MLX_AIRLLM_SAFE_MODE") {
            self.mlx_airllm_safe_mode = parse_bool(&value, self.mlx_airllm_safe_mode);
        }

        if let Ok(value) = env::var("APP_MLX_AIRLLM_PYTHON_COMMAND") {
            if !value.trim().is_empty() {
                self.mlx_airllm_python_command = value;
            }
        }

        if let Ok(value) = env::var("APP_MLX_AIRLLM_RUNNER") {
            if !value.trim().is_empty() {
                self.mlx_airllm_runner = value;
            }
        }

        if let Ok(value) = env::var("APP_MLX_AIRLLM_BACKEND") {
            if !value.trim().is_empty() {
                self.mlx_airllm_backend = value;
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
            self.agent.api_key_ref = None;
        }

        if let Ok(value) = env::var("APP_AGENT_APPROVAL_MODE") {
            let mode = value.trim().to_ascii_lowercase();
            if matches!(mode.as_str(), "auto" | "ask" | "deny") {
                self.agent.approval_mode = mode;
            }
        }

        if let Ok(value) = env::var("APP_AGENT_SECURITY_MODE") {
            let mode = value.trim().to_ascii_lowercase();
            if matches!(mode.as_str(), "standard" | "enterprise" | "paranoid") {
                self.agent.security.security_mode = mode;
            }
        }

        if let Ok(value) = env::var("APP_AGENT_AIRGAPPED") {
            self.agent.security.airgapped = parse_bool(&value, self.agent.security.airgapped);
        }

        if let Ok(value) = env::var("APP_AGENT_OWNER_ONLY") {
            self.agent.security.owner_only = parse_bool(&value, self.agent.security.owner_only);
        }

        self.mlx_airllm_runner = resolve_mlx_airllm_runner(&self.mlx_airllm_runner);
        self.mlx_airllm_backend = normalize_mlx_airllm_backend(&self.mlx_airllm_backend);
        if cfg!(target_os = "windows") {
            // AIRLLM safe mode exists to mitigate MLX/Metal pressure on macOS.
            // On Windows the AIRLLM path should keep its native behavior.
            self.mlx_airllm_safe_mode = false;
        }

        normalize_mlx_command(&mut self);

        self
    }
}

fn default_config_object() -> Value {
    Value::Object(Map::new())
}

fn json_value_is_configured(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Object(map) => !map.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::String(text) => !text.trim().is_empty(),
        Value::Bool(flag) => *flag,
        Value::Number(_) => true,
    }
}

fn parse_shell_args(value: &str) -> Option<Vec<String>> {
    shell_words::split(value).ok()
}

fn home_dir() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }

    #[cfg(unix)]
    {
        if let Ok(user) = std::env::var("USER").or_else(|_| std::env::var("LOGNAME")) {
            let trimmed = user.trim();
            if !trimmed.is_empty() {
                let mac_home = PathBuf::from("/Users").join(trimmed);
                if mac_home.exists() {
                    return Some(mac_home);
                }

                let linux_home = PathBuf::from("/home").join(trimmed);
                if linux_home.exists() {
                    return Some(linux_home);
                }
            }
        }
    }

    None
}

fn default_app_data_dir() -> PathBuf {
    if let Some(home) = home_dir() {
        return home.join(".mlx-pilot");
    }
    PathBuf::from(".").join(".mlx-pilot")
}

fn default_models_dir() -> PathBuf {
    if let Some(home) = home_dir() {
        return home.join("mlx-pilot-models");
    }
    PathBuf::from(".").join("models")
}

fn default_openclaw_state_dir() -> PathBuf {
    default_app_data_dir().join("openclaw").join("state")
}

fn default_mlx_command() -> String {
    let preferred = home_dir()
        .map(|home| home.join("mlx-env").join("bin").join("mlx_lm.generate"))
        .unwrap_or_else(|| PathBuf::from("mlx_lm.generate"));
    if preferred.exists() {
        preferred.display().to_string()
    } else {
        "mlx_lm.generate".to_string()
    }
}

fn default_mlx_airllm_python_command() -> String {
    let preferred = home_dir()
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

fn default_mlx_airllm_runner() -> String {
    resolve_mlx_airllm_runner("scripts/mlx_airllm_bridge.py")
}

fn default_mlx_airllm_backend() -> String {
    "auto".to_string()
}

fn default_mlx_airllm_threshold_percent() -> u8 {
    70
}

fn default_llamacpp_server_binary() -> String {
    let mut candidates = Vec::new();

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            candidates.push(exe_dir.join("llama-server"));
            candidates.push(exe_dir.join("llama-server.exe"));
            candidates.push(exe_dir.join("bin").join("llama-server"));
            candidates.push(exe_dir.join("bin").join("llama-server.exe"));
            candidates.push(exe_dir.join("../Resources").join("llama-server"));
            candidates.push(exe_dir.join("../Resources").join("llama-server.exe"));
        }
    }

    candidates.push(PathBuf::from("bin").join("llama-server"));
    candidates.push(PathBuf::from("bin").join("llama-server.exe"));

    for candidate in candidates {
        if candidate.exists() {
            return candidate.display().to_string();
        }
    }

    "llama-server".to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_state_roundtrips_via_custom_settings_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        let mut cfg = AppConfig::default();
        cfg.compatibility.plugins.insert(
            "memory".to_string(),
            PluginPersistedState {
                enabled: true,
                config: serde_json::json!({"backend": "local"}),
            },
        );
        cfg.compatibility.channels.insert(
            "telegram".to_string(),
            ChannelPersistedState {
                default_account_id: Some("tg".to_string()),
                accounts: BTreeMap::from([(
                    "tg".to_string(),
                    ChannelAccountPersistedState {
                        enabled: true,
                        credentials_ref: Some("channels.telegram.tg.credentials".to_string()),
                        metadata: BTreeMap::from([("owner".to_string(), "local".to_string())]),
                        routing_defaults: BTreeMap::new(),
                        health_state: ChannelAccountHealthState::default(),
                        limits: ChannelAccountPolicy::default(),
                        adapter_config: serde_json::json!({"token": "secret"}),
                    },
                )]),
                alias: Some("tg".to_string()),
                config: serde_json::json!({"token": "secret"}),
                metadata: BTreeMap::new(),
            },
        );

        cfg.save_settings_to(&path).expect("save settings");
        let loaded = AppConfig::load_settings_from(&path);

        assert!(loaded
            .compatibility
            .plugins
            .get("memory")
            .expect("plugin state")
            .is_configured());
        assert!(loaded
            .compatibility
            .channels
            .get("telegram")
            .expect("channel state")
            .is_configured());
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

fn resolve_mlx_airllm_runner(raw: &str) -> String {
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
            candidates.push(exe_dir.join("scripts").join(script_name.as_str()));
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

    if let Some(home) = home_dir() {
        candidates.push(home.join("mlx-ollama-pilot").join(&relative));
    }

    for candidate in candidates {
        if candidate.exists() {
            return candidate.display().to_string();
        }
    }

    relative.display().to_string()
}

fn normalize_mlx_airllm_backend(raw: &str) -> String {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "airllm" | "original" | "airllm-original" => "original".to_string(),
        "legacy" | "legacy-bridge" | "mlx-lm" => "legacy".to_string(),
        "auto" => "auto".to_string(),
        _ => "auto".to_string(),
    }
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

fn default_channel_health_status() -> String {
    "not_configured".to_string()
}

fn default_rate_limit_per_minute() -> u32 {
    60
}

fn default_operation_timeout_ms() -> u64 {
    5_000
}

fn default_retry_count() -> u32 {
    2
}

fn default_backoff_base_ms() -> u64 {
    250
}

fn default_circuit_breaker_threshold() -> u32 {
    3
}

fn default_circuit_breaker_open_ms() -> u64 {
    30_000
}

fn default_security_mode() -> String {
    "standard".to_string()
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

fn default_agent_node_manager() -> String {
    "npm".to_string()
}
