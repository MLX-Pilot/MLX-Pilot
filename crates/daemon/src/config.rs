use std::env;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub bind_addr: SocketAddr,
    pub models_dir: PathBuf,
    pub mlx_command: String,
    pub mlx_prefix_args: Vec<String>,
    pub mlx_suffix_args: Vec<String>,
    pub mlx_timeout: Duration,
    pub remote_downloads_dir: PathBuf,
    pub hf_api_base: String,
    pub hf_token: Option<String>,
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
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 11435),
            models_dir: PathBuf::from("/Users/kaike/models"),
            mlx_command: default_mlx_command(),
            mlx_prefix_args: Vec::new(),
            mlx_suffix_args: Vec::new(),
            mlx_timeout: Duration::from_secs(900),
            remote_downloads_dir: PathBuf::from("/Users/kaike/models"),
            hf_api_base: "https://huggingface.co".to_string(),
            hf_token: None,
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
        }
    }
}

impl AppConfig {
    pub fn from_env() -> Self {
        let mut cfg = Self::default();

        if let Ok(value) = env::var("APP_BIND_ADDR") {
            if let Ok(addr) = value.parse() {
                cfg.bind_addr = addr;
            }
        }

        if let Ok(value) = env::var("APP_MODELS_DIR") {
            cfg.models_dir = PathBuf::from(value);
        }

        cfg.remote_downloads_dir = cfg.models_dir.clone();

        if let Ok(value) = env::var("APP_MLX_COMMAND") {
            if !value.trim().is_empty() {
                cfg.mlx_command = value;
            }
        }

        if let Ok(value) = env::var("APP_MLX_PREFIX_ARGS") {
            if !value.trim().is_empty() {
                cfg.mlx_prefix_args = parse_shell_args(&value).unwrap_or_else(|| {
                    value
                        .split_whitespace()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                });
            }
        }

        if let Ok(value) = env::var("APP_MLX_SUFFIX_ARGS") {
            if !value.trim().is_empty() {
                cfg.mlx_suffix_args = parse_shell_args(&value).unwrap_or_else(|| {
                    value
                        .split_whitespace()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                });
            }
        }

        if let Ok(value) = env::var("APP_MLX_TIMEOUT_SECS") {
            if let Ok(seconds) = value.parse::<u64>() {
                cfg.mlx_timeout = Duration::from_secs(seconds.max(1));
            }
        }

        if let Ok(value) = env::var("APP_REMOTE_DOWNLOADS_DIR") {
            if !value.trim().is_empty() {
                cfg.remote_downloads_dir = PathBuf::from(value);
            }
        }

        if let Ok(value) = env::var("APP_HF_API_BASE") {
            if !value.trim().is_empty() {
                cfg.hf_api_base = value;
            }
        }

        if let Ok(value) = env::var("APP_HF_TOKEN") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                cfg.hf_token = Some(trimmed.to_string());
            }
        }

        if let Ok(value) = env::var("APP_CATALOG_SEARCH_LIMIT") {
            if let Ok(parsed) = value.parse::<usize>() {
                cfg.catalog_search_limit = parsed.clamp(1, 40);
            }
        }

        if let Ok(value) = env::var("APP_CATALOG_DOWNLOAD_TIMEOUT_SECS") {
            if let Ok(seconds) = value.parse::<u64>() {
                cfg.catalog_download_timeout = Duration::from_secs(seconds.max(30));
            }
        }

        if let Ok(value) = env::var("APP_OPENCLAW_NODE_COMMAND") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                cfg.openclaw_node_command = trimmed.to_string();
            }
        }

        if let Ok(value) = env::var("APP_OPENCLAW_CLI_PATH") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                cfg.openclaw_cli_path = PathBuf::from(trimmed);
            }
        }

        if let Ok(value) = env::var("APP_OPENCLAW_STATE_DIR") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                cfg.openclaw_state_dir = PathBuf::from(trimmed);
            }
        }

        if let Ok(value) = env::var("APP_OPENCLAW_GATEWAY_TOKEN") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                cfg.openclaw_gateway_token = trimmed.to_string();
            }
        }

        if let Ok(value) = env::var("APP_OPENCLAW_SESSION_KEY") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                cfg.openclaw_session_key = trimmed.to_string();
            }
        }

        if let Ok(value) = env::var("APP_OPENCLAW_TIMEOUT_SECS") {
            if let Ok(seconds) = value.parse::<u64>() {
                cfg.openclaw_timeout = Duration::from_secs(seconds.max(5));
            }
        }

        if let Ok(value) = env::var("APP_OPENCLAW_GATEWAY_LOG") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                cfg.openclaw_gateway_log = PathBuf::from(trimmed);
            }
        }

        if let Ok(value) = env::var("APP_OPENCLAW_ERROR_LOG") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                cfg.openclaw_error_log = PathBuf::from(trimmed);
            }
        }

        if let Ok(value) = env::var("APP_OPENCLAW_SYNC_LOG") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                cfg.openclaw_sync_log = PathBuf::from(trimmed);
            }
        }

        normalize_mlx_command(&mut cfg);

        cfg
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
