use super::{
    channel_error, connected_session_state, hash_message_target, logged_out_session_state,
    truncate_text, AdapterContext, AdapterResponse, ChannelSessionState, ProbeResult, SendResult,
};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tokio::process::Command;
use tokio::sync::Mutex;

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

static WHATSAPP_HELPER_INSTALL_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

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

fn helper_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("whatsapp-bridge")
}

fn helper_entry() -> PathBuf {
    helper_dir().join("bridge.mjs")
}

fn helper_has_dependencies() -> bool {
    helper_dir()
        .join("node_modules")
        .join("@whiskeysockets")
        .join("baileys")
        .exists()
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

async fn ensure_whatsapp_helper_ready() -> Result<(), String> {
    if helper_has_dependencies() {
        return Ok(());
    }
    let lock = WHATSAPP_HELPER_INSTALL_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock.lock().await;

    if helper_has_dependencies() {
        return Ok(());
    }
    if !helper_entry().exists() {
        return Err(channel_error(
            "provider_error",
            "WhatsApp helper entrypoint is missing from the daemon package.",
        ));
    }

    let helper_dir = helper_dir();
    let mut command = Command::new("npm");
    if helper_dir.join("package-lock.json").exists() {
        command.arg("ci");
    } else {
        command.arg("install");
    }
    command
        .arg("--omit=dev")
        .arg("--no-audit")
        .arg("--no-fund")
        .current_dir(&helper_dir);

    let output = command.output().await.map_err(|error| {
        channel_error(
            "provider_error",
            &format!("failed to install WhatsApp helper dependencies: {error}"),
        )
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        return Err(channel_error(
            "provider_error",
            &format!(
                "failed to install WhatsApp helper dependencies: {}",
                truncate_text(&detail, 400)
            ),
        ));
    }
    if !helper_has_dependencies() {
        return Err(channel_error(
            "provider_error",
            "WhatsApp helper dependencies were not installed correctly.",
        ));
    }
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
    ensure_whatsapp_helper_ready().await?;

    let mut command = Command::new(helper_node_command(ctx));
    command
        .arg(helper_entry())
        .arg(command_name)
        .arg("--session-root")
        .arg(&ctx.session_root)
        .arg("--account-id")
        .arg(&ctx.account_id)
        .current_dir(helper_dir());
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
