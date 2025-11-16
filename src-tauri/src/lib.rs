mod bookmarks;
mod commands;
mod config;
mod hotkey;
mod hotkey_capture;
mod indexer;
mod models;
mod state;
mod text_utils;
mod windows_utils;

use commands::{
    begin_hotkey_capture, end_hotkey_capture, execute_action, get_settings, submit_query,
    trigger_reindex, update_hotkey, update_settings, FOCUS_INPUT_EVENT, HIDE_WINDOW_EVENT,
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
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_window(app);
        }))
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            submit_query,
            execute_action,
            trigger_reindex,
            get_settings,
            begin_hotkey_capture,
            end_hotkey_capture,
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

            if let Err(err) = windows_utils::configure_launch_on_startup(config.launch_on_startup) {
                warn!("failed to sync launch-on-startup setting: {err}");
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
                        // 通过事件通知前端统一执行“重置搜索 + 隐藏窗口”逻辑
                        let _ = app_handle.emit(HIDE_WINDOW_EVENT, ());
                    }
                    MENU_SETTINGS => {
                        let _ = app_handle.emit(OPEN_SETTINGS_EVENT, ());
                    }
                    MENU_QUIT => {
                        app_handle.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .on_window_event(|window, event| {
            use tauri::WindowEvent;

            // 当主窗口失去焦点时，先通知前端重置搜索状态，再隐藏窗口
            if window.label() == MAIN_WINDOW_LABEL {
                if let WindowEvent::Focused(false) = event {
                    let app_handle = window.app_handle();

                    // 通知前端重置搜索状态
                    let _ = app_handle.emit(HIDE_WINDOW_EVENT, ());

                    // 隐藏主窗口
                    if let Some(main_window) = app_handle.get_webview_window(MAIN_WINDOW_LABEL) {
                        let _ = main_window.hide();
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

pub(crate) fn show_window(app_handle: &AppHandle) {
    if let Some(window) = app_handle.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.set_focus();
        if should_force_english_input(app_handle) {
            windows_utils::switch_to_english_input_method();
        }
        let _ = app_handle.emit(FOCUS_INPUT_EVENT, ());
    }
}

fn should_force_english_input(app_handle: &AppHandle) -> bool {
    app_handle
        .try_state::<AppState>()
        .and_then(|state| state.config.lock().ok().map(|cfg| cfg.force_english_input))
        .unwrap_or(true)
}
