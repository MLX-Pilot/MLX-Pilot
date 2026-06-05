#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::OnceLock;
use tauri::Manager;

static DAEMON_BOOTSTRAPPED: OnceLock<()> = OnceLock::new();

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

fn bootstrap_embedded_daemon() {
    if !should_bootstrap_embedded_daemon() {
        return;
    }

    if DAEMON_BOOTSTRAPPED.set(()).is_err() {
        return;
    }

    tauri::async_runtime::spawn(async {
        if let Err(error) = mlx_ollama_daemon::run().await {
            eprintln!("embedded daemon failed to start: {error}");
        }
    });
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            bootstrap_embedded_daemon();

            if let Some(webview_window) = app.get_webview_window("main") {
                #[cfg(debug_assertions)]
                webview_window.open_devtools();

                #[cfg(not(debug_assertions))]
                {
                    let _ = webview_window.eval(
                        r#"
                        document.addEventListener('contextmenu', function(e) {
                            e.preventDefault();
                        }, true);
                        document.addEventListener('keydown', function(e) {
                            if (e.ctrlKey && e.shiftKey && e.key === 'I') { e.preventDefault(); }
                            if (e.ctrlKey && e.shiftKey && e.key === 'J') { e.preventDefault(); }
                            if (e.ctrlKey && e.key === 'u') { e.preventDefault(); }
                            if (e.key === 'F12') { e.preventDefault(); }
                        }, true);
                        "#,
                    );
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
