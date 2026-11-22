use super::{
    channel_error, connected_session_state, hash_message_target, logged_out_session_state,
    truncate_text, AdapterContext, AdapterResponse, ChannelSessionState, ProbeResult, SendResult,
};
use anyhow::{ensure, Context};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::process::Command;

const DEFAULT_LOGIN_START_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_CONNECT_TIMEOUT_MS: u64 = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WhatsAppBackendMode {
    Mock,
    Embedded,
}

#[derive(Debug, Deserialize)]
struct WhatsAppHelperStateResponse {
    status: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    qr_code: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WhatsAppHelperSendResponse {
    #[serde(default)]
    message_id: Option<String>,
}

// Bundle source files, never native node_modules from the build machine.
const HELPER_FILES: &[(&str, &[u8])] = &[
    (
        "bridge.mjs",
        include_bytes!("../../whatsapp-bridge/bridge.mjs"),
    ),
    (
        "package.json",
        include_bytes!("../../whatsapp-bridge/package.json"),
    ),
    (
        "package-lock.json",
        include_bytes!("../../whatsapp-bridge/package-lock.json"),
    ),
];

pub(super) fn whatsapp_backend_mode(ctx: &AdapterContext) -> WhatsAppBackendMode {
    if cfg!(test) {
        return WhatsAppBackendMode::Mock;
    }
    let backend = ctx
        ._adapter_config
        .get("backend")
        .and_then(serde_json::Value::as_str)
        .map(|value| value.trim().to_ascii_lowercase());
    match backend.as_deref() {
        Some("mock") | Some("local") => WhatsAppBackendMode::Mock,
        _ => WhatsAppBackendMode::Embedded,
    }
}

pub(super) async fn whatsapp_login_via_embedded(
    ctx: &AdapterContext,
) -> Result<Option<AdapterResponse>, String> {
    if whatsapp_backend_mode(ctx) == WhatsAppBackendMode::Mock {
        return Ok(None);
    }
    let payload: WhatsAppHelperStateResponse = run_whatsapp_helper_json(
        ctx,
        "login-start",
        &[(
            "timeout-ms",
            helper_timeout_ms(ctx, "login_timeout_ms", DEFAULT_LOGIN_START_TIMEOUT_MS).to_string(),
        )],
    )
    .await?;

    let status = payload.status.trim().to_ascii_lowercase();
    let message = payload
        .message
        .unwrap_or_else(|| "WhatsApp helper response received.".to_string());

    match status.as_str() {
        "pending_qr" => {
            let qr_code = payload
                .qr_code
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            if qr_code.is_none() {
                return Err(channel_error(
                    "provider_error",
                    "WhatsApp helper returned pending_qr without qr_code.",
                ));
            }
            Ok(Some(AdapterResponse {
                status: "pending_qr".to_string(),
                message,
                details: json!({
                    "backend": "embedded",
                    "qr_code": qr_code,
                }),
                session_state: Some(ChannelSessionState {
                    status: "pending_qr".to_string(),
                    session_dir: Some(ctx.session_root.display().to_string()),
                    qr_code,
                    qr_image_data_url: None,
                    connected_at_epoch_ms: None,
                    disconnected_at_epoch_ms: None,
                }),
            }))
        }
        "connected" => Ok(Some(AdapterResponse {
            status: "connected".to_string(),
            message,
            details: json!({ "backend": "embedded" }),
            session_state: Some(connected_session_state(ctx)),
        })),
        "logged_out" | "not_logged_in" => Err(channel_error("provider_error", &message)),
        other => Err(channel_error(
            "provider_error",
            &format!("WhatsApp helper returned unexpected login status: {other}"),
        )),
    }
}

pub(super) async fn whatsapp_logout_via_embedded(
    ctx: &AdapterContext,
) -> Result<Option<AdapterResponse>, String> {
    if whatsapp_backend_mode(ctx) == WhatsAppBackendMode::Mock {
        return Ok(None);
    }
    let payload: WhatsAppHelperStateResponse = run_whatsapp_helper_json(ctx, "logout", &[]).await?;
    Ok(Some(AdapterResponse {
        status: "logged_out".to_string(),
        message: payload
            .message
            .unwrap_or_else(|| "WhatsApp account logged out.".to_string()),
        details: json!({ "backend": "embedded" }),
        session_state: Some(logged_out_session_state(ctx)),
    }))
}

pub(super) async fn whatsapp_probe_via_embedded(
    ctx: &AdapterContext,
) -> Result<Option<ProbeResult>, String> {
    if whatsapp_backend_mode(ctx) == WhatsAppBackendMode::Mock {
        return Ok(None);
    }
    let payload: WhatsAppHelperStateResponse = run_whatsapp_helper_json(ctx, "probe", &[]).await?;
    let status = match payload.status.trim().to_ascii_lowercase().as_str() {
        "healthy" | "connected" => "healthy",
        "pending_qr" => "pending_qr",
        _ => "not_logged_in",
    };
    Ok(Some(ProbeResult {
        status: status.to_string(),
        message: payload
            .message
            .unwrap_or_else(|| "WhatsApp probe completed.".to_string()),
    }))
}

pub(super) async fn whatsapp_send_via_embedded(
    ctx: &AdapterContext,
    target: &str,
    message: &str,
) -> Result<Option<SendResult>, String> {
    if whatsapp_backend_mode(ctx) == WhatsAppBackendMode::Mock {
        return Ok(None);
    }
    let payload: WhatsAppHelperSendResponse = run_whatsapp_helper_json(
        ctx,
        "send",
        &[
            ("target", target.to_string()),
            ("message", message.to_string()),
            (
                "timeout-ms",
                helper_timeout_ms(ctx, "connect_timeout_ms", DEFAULT_CONNECT_TIMEOUT_MS)
                    .to_string(),
            ),
        ],
    )
    .await?;
    Ok(Some(SendResult {
        message_id: payload
            .message_id
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("wa-{}", hash_message_target(target))),
    }))
}

fn helper_cache_dir(settings_path: &Path) -> PathBuf {
    let mut digest = Sha256::new();
    for (_, bytes) in HELPER_FILES {
        digest.update(bytes);
    }
    settings_path
        .with_file_name("runtime-cache")
        .join("whatsapp-bridge")
        .join(format!(
            "{}-{}-{:x}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            digest.finalize()
        ))
}

fn installed(dir: &Path) -> bool {
    dir.join(".installed").is_file()
        && dir.join("bridge.mjs").is_file()
        && dir
            .join("node_modules/@whiskeysockets/baileys/package.json")
            .is_file()
}

fn runtime_command(program: &str) -> Command {
    let mut command = Command::new(program);
    // macOS apps launched by Finder do not inherit the user's shell PATH.
    #[cfg(target_os = "macos")]
    {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let mut dirs: Vec<_> = std::env::split_paths(&path).collect();
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
        if let Ok(path) = std::env::join_paths(dirs) {
            command.env("PATH", path);
        }
    }
    crate::silence_console(&mut command);
    command
}

fn helper_timeout_ms(ctx: &AdapterContext, key: &str, fallback: u64) -> u64 {
    ctx._adapter_config
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .filter(|value| *value >= 5_000)
        .unwrap_or(fallback)
}

fn helper_node_command(ctx: &AdapterContext) -> String {
    ctx._adapter_config
        .get("node_command")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("node")
        .to_string()
}

async fn install_helper(dir: PathBuf) -> anyhow::Result<()> {
    static INSTALL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _local_guard = INSTALL.lock().await;
    tokio::fs::create_dir_all(&dir).await?;
    let lock_path = dir.join(".install.lock");
    // Different desktop and standalone daemon processes can share this directory.
    let _guard = tokio::task::spawn_blocking(move || -> std::io::Result<_> {
        let lock = std::fs::File::options()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(lock_path)?;
        lock.lock()?;
        Ok(lock)
    })
    .await??;
    if installed(&dir) {
        return Ok(());
    }
    // An interrupted installation must run npm ci again, even if npm created its directories.
    let marker = dir.join(".installed");
    if marker.exists() {
        tokio::fs::remove_file(&marker).await?;
    }
    for (name, bytes) in HELPER_FILES {
        tokio::fs::write(dir.join(name), bytes).await?;
    }
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let output = runtime_command(npm)
        .args(["ci", "--omit=dev", "--no-audit", "--no-fund"])
        .arg("--cache")
        .arg(dir.join(".npm-cache"))
        .current_dir(&dir)
        .kill_on_drop(true)
        .output()
        .await
        .context("npm ci requires Node.js 22+ and npm")?;
    let detail = if output.stderr.is_empty() {
        &output.stdout
    } else {
        &output.stderr
    };
    ensure!(
        output.status.success(),
        "npm ci failed: {}",
        truncate_text(&String::from_utf8_lossy(detail), 400)
    );
    ensure!(
        dir.join("node_modules/@whiskeysockets/baileys/package.json")
            .is_file(),
        "npm ci did not install Baileys"
    );
    tokio::fs::write(marker, b"").await?;
    Ok(())
}

async fn run_whatsapp_helper_json<T>(
    ctx: &AdapterContext,
    command_name: &str,
    args: &[(&str, String)],
) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let dir = std::path::absolute(helper_cache_dir(
        &crate::config::AppConfig::get_settings_path(),
    ))
    .map_err(|error| channel_error("provider_error", &error.to_string()))?;
    if !installed(&dir) {
        // A channel operation can time out while npm is downloading; allow the installation
        // to complete so its next attempt reuses the same verified dependency tree.
        tokio::spawn(install_helper(dir.clone()))
            .await
            .map_err(|error| channel_error("provider_error", &error.to_string()))?
            .map_err(|error| {
                channel_error(
                    "provider_error",
                    &format!("WhatsApp helper setup failed: {error:#}"),
                )
            })?;
    }
    let session = std::path::absolute(&ctx.session_root)
        .map_err(|error| channel_error("provider_error", &error.to_string()))?;
    let mut command = runtime_command(&helper_node_command(ctx));
    command
        .arg(dir.join("bridge.mjs"))
        .arg(command_name)
        .arg("--session-root")
        .arg(session)
        .arg("--account-id")
        .arg(&ctx.account_id)
        .current_dir(&dir);
    for (key, value) in args {
        command.arg(format!("--{key}")).arg(value);
    }

    let output = command.output().await.map_err(|error| {
        channel_error(
            "provider_error",
            &format!("failed to execute WhatsApp helper: {error}"),
        )
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        return Err(channel_error(
            "provider_error",
            &format!(
                "WhatsApp helper command failed ({}): {}",
                command_name,
                truncate_text(&detail, 400)
            ),
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    serde_json::from_str(&stdout).map_err(|error| {
        channel_error(
            "provider_error",
            &format!(
                "WhatsApp helper returned invalid JSON for {}: {}",
                command_name, error
            ),
        )
    })
}
