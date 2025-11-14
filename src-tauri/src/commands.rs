use std::sync::Arc;

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;
use windows::{
    core::{HSTRING, PCWSTR},
    Win32::{
        System::Com::{CoCreateInstance, CLSCTX_LOCAL_SERVER},
        UI::Shell::{ApplicationActivationManager, IApplicationActivationManager, ACTIVATEOPTIONS},
    },
};

use crate::windows_utils::ComGuard;

use crate::{
    indexer,
    models::{AppType, ApplicationInfo, SearchResult},
    state::AppState,
};

const DEFAULT_RESULT_LIMIT: usize = 8;
const HIDE_WINDOW_EVENT: &str = "hide_window";

#[tauri::command]
pub fn submit_query(query: String, state: State<'_, AppState>) -> Vec<SearchResult> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();
    let mut counter = 0usize;

    if is_url_like(trimmed) {
        results.push(SearchResult {
            id: format!("url-{counter}"),
            title: format!("打开网址: {trimmed}"),
            subtitle: trimmed.to_string(),
            icon: String::new(),
            score: 200,
            action_id: "url".to_string(),
            action_payload: trimmed.to_string(),
        });
        counter += 1;
    }

    let matcher = SkimMatcherV2::default();
    let apps = {
        let guard = state.app_index.lock().expect("failed to lock app index");
        guard.clone()
    };

    for app in apps.iter() {
        if let Some(score) = match_application(&matcher, app, trimmed) {
            counter += 1;
            results.push(SearchResult {
                id: format!("app-{}", app.id),
                title: app.name.clone(),
                subtitle: app
                    .description
                    .clone()
                    .filter(|d| !d.is_empty())
                    .unwrap_or_else(|| app.path.clone()),
                icon: app.icon_b64.clone(),
                score,
                action_id: match app.app_type {
                    AppType::Win32 => "app".to_string(),
                    AppType::Uwp => "uwp".to_string(),
                },
                action_payload: app.path.clone(),
            });
        }
    }

    results.sort_by(|a, b| b.score.cmp(&a.score));
    if DEFAULT_RESULT_LIMIT > 1 && results.len() >= DEFAULT_RESULT_LIMIT {
        results.truncate(DEFAULT_RESULT_LIMIT - 1);
    } else {
        results.truncate(DEFAULT_RESULT_LIMIT);
    }

    results.push(SearchResult {
        id: format!("search-{counter}"),
        title: format!("在 Google 上搜索: {trimmed}"),
        subtitle: String::from("Google 搜索"),
        icon: String::new(),
        score: i64::MIN,
        action_id: "search".to_string(),
        action_payload: format!(
            "https://google.com/search?q={}",
            urlencoding::encode(trimmed)
        ),
    });

    results
}

#[tauri::command]
pub async fn execute_action(
    id: String,
    payload: String,
    app_handle: AppHandle,
) -> Result<(), String> {
    match id.as_str() {
        "app" => launch_win32_app(&payload)?,
        "uwp" => launch_uwp_app(&payload)?,
        "url" | "search" => {
            app_handle
                .opener()
                .open_url(payload.clone(), Option::<&str>::None)
                .map_err(|err| err.to_string())?;
        }
        _ => {
            log::warn!("未知的 action id: {id}");
        }
    }

    if let Some(window) = app_handle.get_webview_window("main") {
        let _ = window.hide();
    }

    let _ = app_handle.emit(HIDE_WINDOW_EVENT, ());

    Ok(())
}

#[tauri::command]
pub async fn trigger_reindex(state: State<'_, AppState>) -> Result<(), String> {
    let app_index = Arc::clone(&state.app_index);

    tauri::async_runtime::spawn(async move {
        let apps = indexer::build_index().await;
        if let Ok(mut guard) = app_index.lock() {
            *guard = apps;
        }
        log::info!("应用索引刷新完成");
    });

    Ok(())
}

fn launch_win32_app(path: &str) -> Result<(), String> {
    std::process::Command::new("cmd")
        .args(["/C", "start", "", path])
        .spawn()
        .map(|_| ())
        .map_err(|err| err.to_string())
}

fn launch_uwp_app(app_id: &str) -> Result<(), String> {
    unsafe {
        let _guard = ComGuard::new().map_err(|err| err.to_string())?;

        let manager: IApplicationActivationManager =
            CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_LOCAL_SERVER)
                .map_err(|err| err.to_string())?;

        let app_id = HSTRING::from(app_id);
        let _process_id = manager
            .ActivateApplication(&app_id, PCWSTR::null(), ACTIVATEOPTIONS::default())
            .map_err(|err| err.to_string())?;
        Ok(())
    }
}

fn is_url_like(input: &str) -> bool {
    input.starts_with("http://")
        || input.starts_with("https://")
        || input.contains('.') && input.split_whitespace().count() == 1
}

fn match_application(matcher: &SkimMatcherV2, app: &ApplicationInfo, query: &str) -> Option<i64> {
    let mut best = matcher.fuzzy_match(&app.name, query);

    for keyword in &app.keywords {
        if keyword.is_empty() {
            continue;
        }

        if let Some(score) = matcher.fuzzy_match(keyword, query) {
            let score = score - 5; // prefer primary name by adding small penalty to keyword matches
            if best.is_none_or(|current| score > current) {
                best = Some(score);
            }
        }
    }

    best
}
