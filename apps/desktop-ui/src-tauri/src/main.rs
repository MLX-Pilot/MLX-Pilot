#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // Get the main webview window
            if let Some(webview_window) = app.get_webview_window("main") {
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
