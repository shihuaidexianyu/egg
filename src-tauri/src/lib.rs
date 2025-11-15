mod bookmarks;
mod commands;
mod config;
mod hotkey;
mod indexer;
mod models;
mod state;
mod text_utils;
mod windows_utils;

use commands::{
    execute_action, get_settings, submit_query, trigger_reindex, update_hotkey, update_settings,
    OPEN_SETTINGS_EVENT,
};
use config::AppConfig;
use hotkey::bind_hotkey;
use log::warn;
use state::AppState;
use tauri::{menu::MenuBuilder, tray::TrayIconBuilder, AppHandle, Emitter, Manager};

const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_ID: &str = "main-tray";
const MENU_SHOW: &str = "tray-show";
const MENU_HIDE: &str = "tray-hide";
const MENU_SETTINGS: &str = "tray-settings";
const MENU_QUIT: &str = "tray-quit";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            submit_query,
            execute_action,
            trigger_reindex,
            get_settings,
            update_hotkey,
            update_settings
        ])
        .setup(|app| {
            let handle = app.handle();
            let state = app.state::<AppState>();

            let config = AppConfig::load(handle);
            if let Ok(mut guard) = state.config.lock() {
                *guard = config.clone();
            }

            if let Err(err) = bind_hotkey(handle, &state, &config.global_hotkey, MAIN_WINDOW_LABEL)
            {
                warn!(
                    "failed to register global shortcut {}: {}",
                    config.global_hotkey, err
                );
            }

            let tray_menu = MenuBuilder::new(app)
                .text(MENU_SHOW, "显示窗口")
                .text(MENU_HIDE, "隐藏窗口")
                .separator()
                .text(MENU_SETTINGS, "打开设置")
                .separator()
                .text(MENU_QUIT, "退出")
                .build()?;

            let tray_builder = if let Some(icon) = app.default_window_icon().cloned() {
                TrayIconBuilder::with_id(TRAY_ID).icon(icon)
            } else {
                TrayIconBuilder::with_id(TRAY_ID)
            };

            tray_builder
                .menu(&tray_menu)
                .tooltip("RustLauncher")
                .on_menu_event(|app_handle, event| match event.id().as_ref() {
                    MENU_SHOW => show_window(app_handle),
                    MENU_HIDE => {
                        if let Some(window) = app_handle.get_webview_window(MAIN_WINDOW_LABEL) {
                            let _ = window.hide();
                        }
                    }
                    MENU_SETTINGS => {
                        let _ = app_handle.emit(OPEN_SETTINGS_EVENT, ());
                        show_window(app_handle);
                    }
                    MENU_QUIT => {
                        app_handle.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn show_window(app_handle: &AppHandle) {
    if let Some(window) = app_handle.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.set_focus();
    }
}
