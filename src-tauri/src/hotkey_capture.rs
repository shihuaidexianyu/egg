use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use once_cell::sync::Lazy;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

use crate::{hotkey::bind_hotkey, state::AppState};

pub const HOTKEY_CAPTURE_RESULT_EVENT: &str = "hotkey_capture_result";
pub const HOTKEY_CAPTURE_CANCELLED_EVENT: &str = "hotkey_capture_cancelled";
pub const HOTKEY_CAPTURE_INVALID_EVENT: &str = "hotkey_capture_invalid";

#[derive(Clone, Serialize)]
struct HotkeyCaptureResultPayload {
    shortcut: String,
}

struct CaptureContext {
    app_handle: AppHandle,
    app_state: AppState,
    suspension_flag: Arc<AtomicBool>,
    registered_shortcuts: Vec<String>,
    display_map: HashMap<String, String>,
    previous_hotkey: Option<String>,
}

static CAPTURE_CONTEXT: Lazy<Mutex<Option<CaptureContext>>> = Lazy::new(|| Mutex::new(None));

const MOD_CTRL: u8 = 0b0001;
const MOD_SHIFT: u8 = 0b0010;
const MOD_ALT: u8 = 0b0100;
const MOD_SUPER: u8 = 0b1000;
const ESCAPE_LITERAL: &str = "escape";

pub fn start(app_handle: AppHandle, state: AppState) -> Result<(), String> {
    {
        let mut guard = CAPTURE_CONTEXT
            .lock()
            .map_err(|_| "无法初始化快捷键捕捉上下文".to_string())?;
        if guard.is_some() {
            return Err("已有快捷键捕捉任务在进行".into());
        }

        let previous_hotkey = state
            .registered_hotkey
            .lock()
            .map_err(|_| "无法访问当前快捷键".to_string())?
            .clone();

        if let Some(previous) = previous_hotkey.as_deref() {
            if let Err(err) = app_handle.global_shortcut().unregister(previous) {
                log::warn!("解除现有快捷键 {previous} 失败: {err}");
            }
        }

        let (shortcuts, display_map) = build_shortcut_catalog();
        let registration_list = shortcuts.iter().map(|s| s.as_str()).collect::<Vec<_>>();
        let handler_app = app_handle.clone();
        if let Err(err) = app_handle.global_shortcut().on_shortcuts(
            registration_list,
            move |app, shortcut, event| {
                handle_shortcut_event(app, shortcut, event);
            },
        ) {
            log::error!("注册快捷键捕捉监听失败: {err}");
            if let Some(previous) = previous_hotkey.as_deref() {
                if let Err(rebind_err) = bind_hotkey(&handler_app, &state, previous, "main") {
                    log::error!("恢复快捷键 {previous} 失败: {rebind_err}");
                }
            }
            return Err("无法注册快捷键捕捉监听".into());
        }

        state.hotkey_capture_suspended.store(true, Ordering::SeqCst);

        *guard = Some(CaptureContext {
            app_handle: app_handle.clone(),
            app_state: state.clone(),
            suspension_flag: state.hotkey_capture_suspended.clone(),
            registered_shortcuts: shortcuts,
            display_map,
            previous_hotkey,
        });
    }

    Ok(())
}

pub fn stop() -> Result<(), String> {
    stop_internal(None)
}

fn stop_internal(handle_hint: Option<&AppHandle>) -> Result<(), String> {
    let mut guard = CAPTURE_CONTEXT
        .lock()
        .map_err(|_| "无法访问快捷键捕捉状态".to_string())?;

    if let Some(ctx) = guard.take() {
        let app_handle = handle_hint
            .cloned()
            .unwrap_or_else(|| ctx.app_handle.clone());

        if !ctx.registered_shortcuts.is_empty() {
            let unregister_list = ctx
                .registered_shortcuts
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>();
            if let Err(err) = app_handle
                .global_shortcut()
                .unregister_multiple(unregister_list)
            {
                log::warn!("注销捕捉快捷键失败: {err}");
            }
        }

        if let Some(previous) = ctx.previous_hotkey.as_deref() {
            if let Err(err) = bind_hotkey(&app_handle, &ctx.app_state, previous, "main") {
                log::error!("恢复默认快捷键 {previous} 失败: {err}");
            }
        }

        ctx.suspension_flag.store(false, Ordering::SeqCst);
    }

    Ok(())
}

fn handle_shortcut_event(app: &AppHandle, shortcut: &Shortcut, event: ShortcutEvent) {
    if event.state != ShortcutState::Pressed {
        return;
    }

    let normalized = shortcut.into_string().to_lowercase();

    if normalized == ESCAPE_LITERAL {
        let _ = app.emit(HOTKEY_CAPTURE_CANCELLED_EVENT, ());
        if let Err(err) = stop_internal(Some(app)) {
            log::error!("停止快捷键捕捉失败: {err}");
        }
        return;
    }

    let display_value = CAPTURE_CONTEXT.lock().ok().and_then(|guard| {
        guard
            .as_ref()
            .and_then(|ctx| ctx.display_map.get(&normalized).cloned())
    });

    if let Some(shortcut) = display_value {
        let payload = HotkeyCaptureResultPayload { shortcut };
        let _ = app.emit(HOTKEY_CAPTURE_RESULT_EVENT, payload);
        if let Err(err) = stop_internal(Some(app)) {
            log::error!("停止快捷键捕捉失败: {err}");
        }
    } else {
        let _ = app.emit(HOTKEY_CAPTURE_INVALID_EVENT, ());
    }
}

fn build_shortcut_catalog() -> (Vec<String>, HashMap<String, String>) {
    let mut shortcuts = Vec::new();
    let mut display_map = HashMap::new();

    // 注册单独的 Esc 用于取消
    shortcuts.push("Escape".to_string());

    for entry in KEY_ENTRIES.iter() {
        let literal = entry.literal.to_string();
        let normalized_literal = literal.to_lowercase();
        if entry.allow_plain {
            shortcuts.push(literal.clone());
            display_map.insert(normalized_literal.clone(), entry.display.to_string());
        }

        for mask in 1u8..=15u8 {
            let (modifier_literal, display_literal) = modifier_literals(mask);
            if modifier_literal.is_empty() {
                continue;
            }

            let shortcut_literal = format!("{modifier_literal}+{}", entry.literal);
            let display_string = format!("{display_literal}+{}", entry.display);
            display_map.insert(shortcut_literal.to_lowercase(), display_string);
            shortcuts.push(shortcut_literal);
        }
    }

    (shortcuts, display_map)
}

fn modifier_literals(mask: u8) -> (String, String) {
    let mut shortcut_parts = Vec::new();
    let mut display_parts = Vec::new();

    if mask & MOD_SHIFT != 0 {
        shortcut_parts.push("shift");
        display_parts.push("Shift");
    }
    if mask & MOD_CTRL != 0 {
        shortcut_parts.push("control");
        display_parts.push("Ctrl");
    }
    if mask & MOD_ALT != 0 {
        shortcut_parts.push("alt");
        display_parts.push("Alt");
    }
    if mask & MOD_SUPER != 0 {
        shortcut_parts.push("super");
        display_parts.push("Win");
    }

    (shortcut_parts.join("+"), display_parts.join("+"))
}

struct KeyEntry {
    literal: &'static str,
    display: &'static str,
    allow_plain: bool,
}

const KEY_ENTRIES: &[KeyEntry] = &[
    KeyEntry {
        literal: "KeyA",
        display: "A",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyB",
        display: "B",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyC",
        display: "C",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyD",
        display: "D",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyE",
        display: "E",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyF",
        display: "F",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyG",
        display: "G",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyH",
        display: "H",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyI",
        display: "I",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyJ",
        display: "J",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyK",
        display: "K",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyL",
        display: "L",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyM",
        display: "M",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyN",
        display: "N",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyO",
        display: "O",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyP",
        display: "P",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyQ",
        display: "Q",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyR",
        display: "R",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyS",
        display: "S",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyT",
        display: "T",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyU",
        display: "U",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyV",
        display: "V",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyW",
        display: "W",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyX",
        display: "X",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyY",
        display: "Y",
        allow_plain: false,
    },
    KeyEntry {
        literal: "KeyZ",
        display: "Z",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Digit0",
        display: "0",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Digit1",
        display: "1",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Digit2",
        display: "2",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Digit3",
        display: "3",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Digit4",
        display: "4",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Digit5",
        display: "5",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Digit6",
        display: "6",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Digit7",
        display: "7",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Digit8",
        display: "8",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Digit9",
        display: "9",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Minus",
        display: "-",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Equal",
        display: "=",
        allow_plain: false,
    },
    KeyEntry {
        literal: "BracketLeft",
        display: "[",
        allow_plain: false,
    },
    KeyEntry {
        literal: "BracketRight",
        display: "]",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Backslash",
        display: "\\",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Semicolon",
        display: ";",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Quote",
        display: "'",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Comma",
        display: ",",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Period",
        display: ".",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Slash",
        display: "/",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Backquote",
        display: "`",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Space",
        display: "Space",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Tab",
        display: "Tab",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Enter",
        display: "Enter",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Backspace",
        display: "Backspace",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Delete",
        display: "Delete",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Insert",
        display: "Insert",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Home",
        display: "Home",
        allow_plain: false,
    },
    KeyEntry {
        literal: "End",
        display: "End",
        allow_plain: false,
    },
    KeyEntry {
        literal: "PageUp",
        display: "PageUp",
        allow_plain: false,
    },
    KeyEntry {
        literal: "PageDown",
        display: "PageDown",
        allow_plain: false,
    },
    KeyEntry {
        literal: "ArrowUp",
        display: "Up",
        allow_plain: false,
    },
    KeyEntry {
        literal: "ArrowDown",
        display: "Down",
        allow_plain: false,
    },
    KeyEntry {
        literal: "ArrowLeft",
        display: "Left",
        allow_plain: false,
    },
    KeyEntry {
        literal: "ArrowRight",
        display: "Right",
        allow_plain: false,
    },
    KeyEntry {
        literal: "Escape",
        display: "Esc",
        allow_plain: false,
    },
    KeyEntry {
        literal: "F1",
        display: "F1",
        allow_plain: true,
    },
    KeyEntry {
        literal: "F2",
        display: "F2",
        allow_plain: true,
    },
    KeyEntry {
        literal: "F3",
        display: "F3",
        allow_plain: true,
    },
    KeyEntry {
        literal: "F4",
        display: "F4",
        allow_plain: true,
    },
    KeyEntry {
        literal: "F5",
        display: "F5",
        allow_plain: true,
    },
    KeyEntry {
        literal: "F6",
        display: "F6",
        allow_plain: true,
    },
    KeyEntry {
        literal: "F7",
        display: "F7",
        allow_plain: true,
    },
    KeyEntry {
        literal: "F8",
        display: "F8",
        allow_plain: true,
    },
    KeyEntry {
        literal: "F9",
        display: "F9",
        allow_plain: true,
    },
    KeyEntry {
        literal: "F10",
        display: "F10",
        allow_plain: true,
    },
    KeyEntry {
        literal: "F11",
        display: "F11",
        allow_plain: true,
    },
    KeyEntry {
        literal: "F12",
        display: "F12",
        allow_plain: true,
    },
    KeyEntry {
        literal: "F13",
        display: "F13",
        allow_plain: true,
    },
    KeyEntry {
        literal: "F14",
        display: "F14",
        allow_plain: true,
    },
    KeyEntry {
        literal: "F15",
        display: "F15",
        allow_plain: true,
    },
    KeyEntry {
        literal: "F16",
        display: "F16",
        allow_plain: true,
    },
    KeyEntry {
        literal: "F17",
        display: "F17",
        allow_plain: true,
    },
    KeyEntry {
        literal: "F18",
        display: "F18",
        allow_plain: true,
    },
    KeyEntry {
        literal: "F19",
        display: "F19",
        allow_plain: true,
    },
    KeyEntry {
        literal: "F20",
        display: "F20",
        allow_plain: true,
    },
    KeyEntry {
        literal: "F21",
        display: "F21",
        allow_plain: true,
    },
    KeyEntry {
        literal: "F22",
        display: "F22",
        allow_plain: true,
    },
    KeyEntry {
        literal: "F23",
        display: "F23",
        allow_plain: true,
    },
    KeyEntry {
        literal: "F24",
        display: "F24",
        allow_plain: true,
    },
];
