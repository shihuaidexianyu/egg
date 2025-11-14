mod commands;
mod indexer;
mod models;
mod state;
mod windows_utils;

use log::warn;
use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use commands::{execute_action, submit_query, trigger_reindex};
use state::AppState;

const MAIN_WINDOW_LABEL: &str = "main";
const GLOBAL_SHORTCUT: &str = "Alt+Space";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            submit_query,
            execute_action,
            trigger_reindex
        ])
        .setup(|app| {
            if let Err(err) = app.handle().global_shortcut().on_shortcut(
                GLOBAL_SHORTCUT,
                |app_handle, _, event| {
                    if event.state == ShortcutState::Pressed {
                        if let Some(window) = app_handle.get_webview_window(MAIN_WINDOW_LABEL) {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                },
            ) {
                warn!(
                    "failed to register global shortcut {}: {}",
                    GLOBAL_SHORTCUT, err
                );
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
