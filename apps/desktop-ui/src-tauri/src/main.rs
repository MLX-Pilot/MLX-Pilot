#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::OnceLock;
use tauri::Manager;

static DAEMON_BOOTSTRAPPED: OnceLock<()> = OnceLock::new();
static DAEMON_BASE_URL: OnceLock<String> = OnceLock::new();
static DESKTOP_LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

const DEFAULT_DAEMON_HOST: &str = "127.0.0.1";
const DEFAULT_DAEMON_PORT: u16 = 11435;
const DAEMON_PORT_SCAN_LIMIT: u16 = 12;

#[derive(Serialize)]
struct DesktopRuntimeInfo {
    daemon_url: String,
    embedded_daemon_enabled: bool,
    log_path: String,
    pid: u32,
}

#[derive(Serialize)]
struct DesktopLogSnapshot {
    path: String,
    entries: Vec<String>,
}

fn default_desktop_log_path() -> PathBuf {
    if let Ok(path) = std::env::var("MLX_PILOT_DESKTOP_LOG") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(local_app_data)
            .join("MLX Pilot")
            .join("logs")
            .join("desktop.log");
    }

    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("mlx-pilot")
            .join("logs")
            .join("desktop.log");
    }

    std::env::temp_dir()
        .join("mlx-pilot")
        .join("logs")
        .join("desktop.log")
}

fn desktop_log_path() -> PathBuf {
    DESKTOP_LOG_PATH
        .get_or_init(default_desktop_log_path)
        .clone()
}

fn unix_timestamp_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn append_desktop_log_line(level: &str, message: &str) {
    let path = desktop_log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let sanitized = message.replace(['\r', '\n'], " ");
    let line = format!(
        "{} [{}] {}\n",
        unix_timestamp_millis(),
        level.trim().to_ascii_uppercase(),
        sanitized.trim()
    );

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

fn read_desktop_log_tail(limit: usize) -> Vec<String> {
    let path = desktop_log_path();
    let mut content = String::new();
    let Ok(mut file) = OpenOptions::new().read(true).open(path) else {
        return Vec::new();
    };
    if file.read_to_string(&mut content).is_err() {
        return Vec::new();
    }

    let limit = limit.clamp(1, 1000);
    let mut entries = content
        .lines()
        .rev()
        .take(limit)
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    entries.reverse();
    entries
}

#[tauri::command]
fn desktop_runtime_info() -> DesktopRuntimeInfo {
    DesktopRuntimeInfo {
        daemon_url: current_daemon_url(),
        embedded_daemon_enabled: should_bootstrap_embedded_daemon(),
        log_path: desktop_log_path().display().to_string(),
        pid: std::process::id(),
    }
}

#[tauri::command]
fn desktop_log_snapshot(limit: Option<usize>) -> DesktopLogSnapshot {
    DesktopLogSnapshot {
        path: desktop_log_path().display().to_string(),
        entries: read_desktop_log_tail(limit.unwrap_or(250)),
    }
}

#[tauri::command]
fn desktop_log_append(level: String, message: String) {
    append_desktop_log_line(&level, &message);
}

#[tauri::command]
fn desktop_log_clear() -> Result<(), String> {
    let path = desktop_log_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create log directory: {error}"))?;
    }
    fs::write(&path, "").map_err(|error| format!("failed to clear desktop log: {error}"))
}

fn should_bootstrap_embedded_daemon() -> bool {
    match std::env::var("MLX_PILOT_DISABLE_EMBEDDED_DAEMON") {
        Ok(value) => {
            let normalized = value.trim().to_ascii_lowercase();
            !(normalized == "1"
                || normalized == "true"
                || normalized == "yes"
                || normalized == "on")
        }
        Err(_) => true,
    }
}

fn resolve_embedded_daemon_bind_addr() -> String {
    if let Ok(bind_addr) = std::env::var("APP_BIND_ADDR") {
        let trimmed = bind_addr.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    for port in DEFAULT_DAEMON_PORT..=DEFAULT_DAEMON_PORT + DAEMON_PORT_SCAN_LIMIT {
        if let Ok(listener) = TcpListener::bind((DEFAULT_DAEMON_HOST, port)) {
            drop(listener);
            return format!("{DEFAULT_DAEMON_HOST}:{port}");
        }
    }

    if let Ok(listener) = TcpListener::bind((DEFAULT_DAEMON_HOST, 0)) {
        if let Ok(addr) = listener.local_addr() {
            let host = match addr.ip() {
                std::net::IpAddr::V4(ip) => ip.to_string(),
                std::net::IpAddr::V6(ip) => format!("[{ip}]"),
            };
            drop(listener);
            return format!("{host}:{}", addr.port());
        }
    }

    format!("{DEFAULT_DAEMON_HOST}:{DEFAULT_DAEMON_PORT}")
}

fn store_daemon_url(bind_addr: &str) -> String {
    let daemon_url = format!("http://{bind_addr}");
    let _ = DAEMON_BASE_URL.set(daemon_url.clone());
    std::env::set_var("APP_BIND_ADDR", bind_addr);
    std::env::set_var("MLX_PILOT_DAEMON_URL", &daemon_url);
    daemon_url
}

fn current_daemon_url() -> String {
    if let Some(url) = DAEMON_BASE_URL.get() {
        return url.clone();
    }

    if let Ok(url) = std::env::var("MLX_PILOT_DAEMON_URL") {
        let trimmed = url.trim();
        if !trimmed.is_empty() {
            let normalized = trimmed.to_string();
            let _ = DAEMON_BASE_URL.set(normalized.clone());
            return normalized;
        }
    }

    if let Ok(bind_addr) = std::env::var("APP_BIND_ADDR") {
        let trimmed = bind_addr.trim();
        if !trimmed.is_empty() {
            return store_daemon_url(trimmed);
        }
    }

    store_daemon_url(&format!("{DEFAULT_DAEMON_HOST}:{DEFAULT_DAEMON_PORT}"))
}

fn daemon_bootstrap_script() -> String {
    let daemon_url_json =
        serde_json::to_string(&current_daemon_url()).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"
        window.__MLX_PILOT_DAEMON_URL__ = {daemon_url_json};
        try {{
          localStorage.setItem("mlxPilotDaemonUrl", window.__MLX_PILOT_DAEMON_URL__);
        }} catch (_error) {{}}
        window.dispatchEvent(new CustomEvent("mlx-pilot-daemon-ready", {{
          detail: {{ url: window.__MLX_PILOT_DAEMON_URL__ }}
        }}));
        "#
    )
}

fn inject_daemon_bootstrap(window: &tauri::WebviewWindow) {
    let script = daemon_bootstrap_script();
    let _ = window.eval(&script);
}

fn inject_daemon_bootstrap_into_webview(webview: &tauri::Webview) {
    let script = daemon_bootstrap_script();
    let _ = webview.eval(&script);
}

fn bootstrap_embedded_daemon() {
    if !should_bootstrap_embedded_daemon() {
        let _ = current_daemon_url();
        append_desktop_log_line("info", "embedded daemon disabled by environment");
        return;
    }

    let bind_addr = resolve_embedded_daemon_bind_addr();
    let daemon_url = store_daemon_url(&bind_addr);
    append_desktop_log_line("info", &format!("embedded daemon binding to {daemon_url}"));

    if DAEMON_BOOTSTRAPPED.set(()).is_err() {
        append_desktop_log_line("info", "embedded daemon already bootstrapped");
        return;
    }

    tauri::async_runtime::spawn(async {
        if let Err(error) = mlx_ollama_daemon::run().await {
            append_desktop_log_line(
                "error",
                &format!("embedded daemon failed to start: {error}"),
            );
            eprintln!("embedded daemon failed to start: {error}");
        }
    });
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            desktop_runtime_info,
            desktop_log_snapshot,
            desktop_log_append,
            desktop_log_clear
        ])
        .setup(|app| {
            append_desktop_log_line("info", "desktop shell starting");
            bootstrap_embedded_daemon();

            // Get the main webview window
            if let Some(webview_window) = app.get_webview_window("main") {
                inject_daemon_bootstrap(&webview_window);

                // Enable devtools only in debug builds
                #[cfg(debug_assertions)]
                webview_window.open_devtools();

                // Inject JS to block browser-like behaviors in release mode
                #[cfg(not(debug_assertions))]
                {
                    let _ = webview_window.eval(
                        r#"
                        // Block context menu (right-click)
                        document.addEventListener('contextmenu', function(e) {
                            e.preventDefault();
                        }, true);

                        // Block browser keyboard shortcuts
                        document.addEventListener('keydown', function(e) {
                            // Ctrl+Shift+I (DevTools)
                            if (e.ctrlKey && e.shiftKey && e.key === 'I') { e.preventDefault(); }
                            // Ctrl+Shift+J (Console)
                            if (e.ctrlKey && e.shiftKey && e.key === 'J') { e.preventDefault(); }
                            // Ctrl+U (View Source)
                            if (e.ctrlKey && e.key === 'u') { e.preventDefault(); }
                            // F12
                            if (e.key === 'F12') { e.preventDefault(); }
                        }, true);
                        "#,
                    );
                }
            }
            Ok(())
        })
        .on_page_load(|window, _payload| {
            inject_daemon_bootstrap_into_webview(window);
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
