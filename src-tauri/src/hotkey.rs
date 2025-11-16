use std::sync::atomic::Ordering;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::{show_window, state::AppState};

pub fn bind_hotkey(
    app_handle: &AppHandle,
    state: &AppState,
    hotkey: &str,
    window_label: &str,
) -> Result<(), String> {
    if hotkey.trim().is_empty() {
        return Err("快捷键不能为空".into());
    }

    let mut current_hotkey = state
        .registered_hotkey
        .lock()
        .map_err(|_| "无法获取快捷键状态".to_string())?;

    if let Some(previous) = current_hotkey.as_deref() {
        if let Err(err) = app_handle.global_shortcut().unregister(previous) {
            log::warn!("failed to unregister previous hotkey {previous}: {err}");
        }
    }

    let hotkey_string = hotkey.trim().to_string();
    let shortcut_literal = hotkey_string.clone();
    let window_label_string = window_label.to_string();
    let capture_guard = state.hotkey_capture_suspended.clone();
    app_handle
        .global_shortcut()
        .on_shortcut(shortcut_literal.as_str(), {
            let window_label = window_label_string;
            let capture_guard = capture_guard.clone();
            move |app_handle, _, event| {
                if event.state == ShortcutState::Pressed {
                    if capture_guard.load(Ordering::SeqCst) {
                        return;
                    }
                    if let Some(window) = app_handle.get_webview_window(&window_label) {
                        if window.is_visible().unwrap_or(false) {
                            let _ = window.hide();
                        } else {
                            show_window(app_handle);
                        }
                    }
                }
            }
        })
        .map_err(|err| err.to_string())?;

    *current_hotkey = Some(hotkey_string);
    Ok(())
}
