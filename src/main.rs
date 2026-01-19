mod bookmarks;
mod cache;
mod config;
mod execute;
mod indexer;
mod models;
mod search_core;
mod state;
mod text_utils;
mod tui;
mod windows_utils;

use std::{env, process::Command, sync::Arc, time::Duration};

use anyhow::Result;
use log::{debug, info, warn};
use windows::Win32::{
    Foundation::HWND,
    UI::{
        Input::KeyboardAndMouse::{
            RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_SHIFT,
            MOD_WIN, VIRTUAL_KEY, VK_0, VK_1, VK_2, VK_3, VK_4, VK_5, VK_6, VK_7, VK_8, VK_9, VK_A,
            VK_B, VK_BACK, VK_C, VK_D, VK_DOWN, VK_E, VK_ESCAPE, VK_F, VK_F1, VK_F10, VK_F11,
            VK_F12, VK_F2, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9, VK_G, VK_H, VK_I, VK_J,
            VK_K, VK_L, VK_LEFT, VK_M, VK_N, VK_O, VK_P, VK_Q, VK_R, VK_RETURN, VK_RIGHT, VK_S,
            VK_SPACE, VK_T, VK_TAB, VK_U, VK_UP, VK_V, VK_W, VK_X, VK_Y, VK_Z,
        },
        WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY},
    },
};

use crate::{
    config::AppConfig,
    execute::execute_action,
    indexer::build_index,
    state::{AppState, RecentEntry},
    tui::run_tui,
    windows_utils::focus_and_center_console_window,
};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_secs()
        .init();

    let args: Vec<String> = env::args().collect();
    if args.iter().any(|arg| arg == "--daemon") {
        return run_hotkey_daemon();
    }
    if args.iter().any(|arg| arg == "--hotkey") {
        focus_and_center_console_window();
    }

    println!("egg-cli v0.1.0 starting...");

    let config = AppConfig::load();
    debug!("Loaded configuration");

    let state = Arc::new(AppState::new());
    {
        let mut config_guard = state.config.lock().unwrap();
        *config_guard = config.clone();
    }

    if let Some(cached_apps) = cache::load_app_index() {
        if !cached_apps.is_empty() {
            info!("Loaded {} cached applications", cached_apps.len());
            let mut app_index = state.app_index.lock().unwrap();
            *app_index = cached_apps;
        }
    }

    println!("Building application index...");
    println!("Loading bookmarks...");
    let exclusion_paths = config.system_tool_exclusions.clone();
    let (apps_task, bookmarks_task) = tokio::join!(
        tokio::spawn(async move { build_index(exclusion_paths).await }),
        tokio::task::spawn_blocking(bookmarks::load_chrome_bookmarks),
    );
    let apps = match apps_task {
        Ok(apps) => apps,
        Err(err) => {
            warn!("app index task failed: {err}");
            Vec::new()
        }
    };
    let bookmarks = match bookmarks_task {
        Ok(bookmarks) => bookmarks,
        Err(err) => {
            warn!("bookmark index task failed: {err}");
            Vec::new()
        }
    };
    info!("Indexed {} applications", apps.len());
    info!("Loaded {} bookmarks", bookmarks.len());

    if !apps.is_empty() {
        let mut app_index = state.app_index.lock().unwrap();
        if *app_index != apps {
            *app_index = apps.clone();
            let _ = cache::save_app_index(&apps);
            if let Ok(mut cache_guard) = state.search_cache.lock() {
                cache_guard.clear();
            }
        }
    }
    {
        let mut bookmark_index = state.bookmark_index.lock().unwrap();
        *bookmark_index = bookmarks;
    }

    println!(
        "\nReady! Indexed {} apps and {} bookmarks.",
        state.app_index.lock().unwrap().len(),
        state.bookmark_index.lock().unwrap().len()
    );
    println!("Starting TUI...\n");

    let refresh_state = state.clone();
    let refresh_exclusions = config.system_tool_exclusions.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let refreshed = build_index(refresh_exclusions).await;
        if refreshed.is_empty() {
            return;
        }

        let mut updated = false;
        if let Ok(mut guard) = refresh_state.app_index.lock() {
            if *guard != refreshed {
                *guard = refreshed.clone();
                updated = true;
            }
        }

        if updated {
            let _ = cache::save_app_index(&refreshed);
            if let Ok(mut cache_guard) = refresh_state.search_cache.lock() {
                cache_guard.clear();
            }
        }
    });

    let pending = run_tui(state.clone())?;
    if let Some((result, action)) = pending {
        if let Ok(mut recent_guard) = state.recent_actions.lock() {
            recent_guard.insert(RecentEntry {
                result: result.clone(),
                action: action.clone(),
            });
        }
        if let Err(err) = execute_action(&action, false) {
            eprintln!("Error: {err}");
        }
    }

    Ok(())
}

fn run_hotkey_daemon() -> Result<()> {
    let config = AppConfig::load();
    let (modifiers, key) = parse_hotkey_or_default(&config.global_hotkey);
    unsafe {
        RegisterHotKey(HWND::default(), 1, modifiers, key.0 as u32)
            .map_err(|err| anyhow::anyhow!("RegisterHotKey failed: {err}"))?;
    }

    let _guard = HotkeyGuard;
    loop {
        let mut msg = MSG::default();
        let status = unsafe { GetMessageW(&mut msg, HWND::default(), 0, 0) };
        if status.0 == 0 {
            break;
        }
        if msg.message == WM_HOTKEY {
            spawn_tui_instance();
        }
    }
    Ok(())
}

struct HotkeyGuard;

impl Drop for HotkeyGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = UnregisterHotKey(HWND::default(), 1);
        }
    }
}

fn spawn_tui_instance() {
    let Ok(exe_path) = env::current_exe() else {
        return;
    };
    let _ = Command::new(exe_path).arg("--tui").arg("--hotkey").spawn();
}

fn parse_hotkey_or_default(input: &str) -> (HOT_KEY_MODIFIERS, VIRTUAL_KEY) {
    parse_hotkey(input).unwrap_or((MOD_ALT, VK_SPACE))
}

fn parse_hotkey(input: &str) -> Option<(HOT_KEY_MODIFIERS, VIRTUAL_KEY)> {
    let mut modifiers = HOT_KEY_MODIFIERS(0);
    let mut key = None;

    for token in input.split('+').map(|value| value.trim()) {
        if token.is_empty() {
            continue;
        }
        let token_upper = token.to_ascii_uppercase();
        match token_upper.as_str() {
            "ALT" => modifiers |= MOD_ALT,
            "CTRL" | "CONTROL" => modifiers |= MOD_CONTROL,
            "SHIFT" => modifiers |= MOD_SHIFT,
            "WIN" | "WINDOWS" | "SUPER" => modifiers |= MOD_WIN,
            _ => {
                if key.is_some() {
                    return None;
                }
                key = parse_virtual_key(&token_upper);
            }
        }
    }

    key.map(|vk| (modifiers, vk))
}

fn parse_virtual_key(token: &str) -> Option<VIRTUAL_KEY> {
    if token.len() == 1 {
        let ch = token.chars().next().unwrap();
        return match ch {
            'A' => Some(VK_A),
            'B' => Some(VK_B),
            'C' => Some(VK_C),
            'D' => Some(VK_D),
            'E' => Some(VK_E),
            'F' => Some(VK_F),
            'G' => Some(VK_G),
            'H' => Some(VK_H),
            'I' => Some(VK_I),
            'J' => Some(VK_J),
            'K' => Some(VK_K),
            'L' => Some(VK_L),
            'M' => Some(VK_M),
            'N' => Some(VK_N),
            'O' => Some(VK_O),
            'P' => Some(VK_P),
            'Q' => Some(VK_Q),
            'R' => Some(VK_R),
            'S' => Some(VK_S),
            'T' => Some(VK_T),
            'U' => Some(VK_U),
            'V' => Some(VK_V),
            'W' => Some(VK_W),
            'X' => Some(VK_X),
            'Y' => Some(VK_Y),
            'Z' => Some(VK_Z),
            '0' => Some(VK_0),
            '1' => Some(VK_1),
            '2' => Some(VK_2),
            '3' => Some(VK_3),
            '4' => Some(VK_4),
            '5' => Some(VK_5),
            '6' => Some(VK_6),
            '7' => Some(VK_7),
            '8' => Some(VK_8),
            '9' => Some(VK_9),
            _ => None,
        };
    }

    match token {
        "SPACE" => Some(VK_SPACE),
        "ENTER" | "RETURN" => Some(VK_RETURN),
        "TAB" => Some(VK_TAB),
        "ESC" | "ESCAPE" => Some(VK_ESCAPE),
        "BACKSPACE" => Some(VK_BACK),
        "LEFT" => Some(VK_LEFT),
        "RIGHT" => Some(VK_RIGHT),
        "UP" => Some(VK_UP),
        "DOWN" => Some(VK_DOWN),
        "F1" => Some(VK_F1),
        "F2" => Some(VK_F2),
        "F3" => Some(VK_F3),
        "F4" => Some(VK_F4),
        "F5" => Some(VK_F5),
        "F6" => Some(VK_F6),
        "F7" => Some(VK_F7),
        "F8" => Some(VK_F8),
        "F9" => Some(VK_F9),
        "F10" => Some(VK_F10),
        "F11" => Some(VK_F11),
        "F12" => Some(VK_F12),
        _ => None,
    }
}
