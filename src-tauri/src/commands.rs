use std::{collections::HashMap, path::Path, process::Command, sync::Arc};

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
    bookmarks::{self, BookmarkEntry},
    config::AppConfig,
    hotkey::bind_hotkey,
    indexer,
    models::{AppType, ApplicationInfo, SearchResult},
    state::{AppState, PendingAction},
};

const DEFAULT_RESULT_LIMIT: usize = 8;
const MIN_QUERY_DELAY_MS: u64 = 50;
const MAX_QUERY_DELAY_MS: u64 = 2000;
pub const HIDE_WINDOW_EVENT: &str = "hide_window";
pub const OPEN_SETTINGS_EVENT: &str = "open_settings";

#[derive(Clone, Copy, PartialEq, Eq)]
enum QueryMode {
    All,
    Bookmark,
    Application,
}

impl QueryMode {
    fn from_option(mode: Option<String>) -> Self {
        match mode
            .as_deref()
            .map(|value| value.trim().to_lowercase())
            .as_deref()
        {
            Some("bookmark") | Some("bookmarks") | Some("b") => Self::Bookmark,
            Some("app") | Some("apps") | Some("application") | Some("r") => Self::Application,
            _ => Self::All,
        }
    }

    fn allows_bookmarks(&self) -> bool {
        matches!(self, Self::All | Self::Bookmark)
    }

    fn allows_applications(&self) -> bool {
        matches!(self, Self::All | Self::Application)
    }
}

#[tauri::command]
pub fn submit_query(
    query: String,
    mode: Option<String>,
    state: State<'_, AppState>,
) -> Vec<SearchResult> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let query_mode = QueryMode::from_option(mode);

    let mut results = Vec::new();
    let mut counter = 0usize;
    let mut pending_actions: HashMap<String, PendingAction> = HashMap::new();

    if is_url_like(trimmed) {
        let result_id = format!("url-{counter}");
        pending_actions.insert(result_id.clone(), PendingAction::Url(trimmed.to_string()));
        results.push(SearchResult {
            id: result_id,
            title: format!("打开网址: {trimmed}"),
            subtitle: trimmed.to_string(),
            icon: String::new(),
            score: 200,
            action_id: "url".to_string(),
        });
        counter += 1;
    }

    let matcher = SkimMatcherV2::default();
    let apps = if query_mode.allows_applications() {
        Some(
            state
                .app_index
                .lock()
                .expect("failed to lock app index")
                .clone(),
        )
    } else {
        None
    };
    let bookmarks = if query_mode.allows_bookmarks() {
        Some(
            state
                .bookmark_index
                .lock()
                .expect("failed to lock bookmark index")
                .clone(),
        )
    } else {
        None
    };

    if let Some(apps) = apps.as_ref() {
        for app in apps.iter() {
            if let Some(score) = match_application(&matcher, app, trimmed) {
                counter += 1;
                let result_id = format!("app-{}", app.id);
                pending_actions.insert(result_id.clone(), PendingAction::Application(app.clone()));
                results.push(SearchResult {
                    id: result_id,
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
                });
            }
        }
    }

    if let Some(bookmarks) = bookmarks.as_ref() {
        for bookmark in bookmarks.iter() {
            if let Some(score) = match_bookmark(&matcher, bookmark, trimmed) {
                counter += 1;
                let subtitle = match &bookmark.folder_path {
                    Some(path) => format!("收藏夹 · {path} · {}", bookmark.url),
                    None => format!("收藏夹 · {}", bookmark.url),
                };
                let result_id = format!("bookmark-{}", bookmark.id);
                pending_actions
                    .insert(result_id.clone(), PendingAction::Bookmark(bookmark.clone()));
                results.push(SearchResult {
                    id: result_id,
                    title: bookmark.title.clone(),
                    subtitle,
                    icon: String::new(),
                    score,
                    action_id: "bookmark".to_string(),
                });
            }
        }
    }

    results.sort_by(|a, b| b.score.cmp(&a.score));
    if DEFAULT_RESULT_LIMIT > 1 && results.len() >= DEFAULT_RESULT_LIMIT {
        results.truncate(DEFAULT_RESULT_LIMIT - 1);
    } else {
        results.truncate(DEFAULT_RESULT_LIMIT);
    }

    let search_id = format!("search-{counter}");
    let search_url = format!(
        "https://google.com/search?q={}",
        urlencoding::encode(trimmed)
    );
    pending_actions.insert(search_id.clone(), PendingAction::Search(search_url.clone()));
    results.push(SearchResult {
        id: search_id,
        title: format!("在 Google 上搜索: {trimmed}"),
        subtitle: String::from("Google 搜索"),
        icon: String::new(),
        score: i64::MIN,
        action_id: "search".to_string(),
    });

    if let Ok(mut guard) = state.pending_actions.lock() {
        guard.clear();
        guard.extend(pending_actions);
    } else {
        log::warn!("无法记录搜索结果缓存，可能导致执行失败");
    }

    results
}

#[tauri::command]
pub async fn execute_action(
    id: String,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let action = {
        let guard = state
            .pending_actions
            .lock()
            .map_err(|_| "无法访问待执行队列".to_string())?;
        guard
            .get(&id)
            .cloned()
            .ok_or_else(|| "结果已失效，请重新搜索".to_string())?
    };

    match action {
        PendingAction::Application(app) => match app.app_type {
            AppType::Win32 => launch_win32_app(&app)?,
            AppType::Uwp => launch_uwp_app(&app.path)?,
        },
        PendingAction::Bookmark(entry) => open_url(&app_handle, &entry.url)?,
        PendingAction::Url(url) | PendingAction::Search(url) => {
            open_url(&app_handle, &url)?;
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
    let bookmark_index = Arc::clone(&state.bookmark_index);

    tauri::async_runtime::spawn(async move {
        let apps = indexer::build_index().await;
        if let Ok(mut guard) = app_index.lock() {
            *guard = apps;
        }
        log::info!("应用索引刷新完成");
    });

    tauri::async_runtime::spawn_blocking(move || {
        let bookmarks = bookmarks::load_chrome_bookmarks();
        if let Ok(mut guard) = bookmark_index.lock() {
            *guard = bookmarks;
        }
        log::info!("Chrome 收藏夹索引刷新完成");
    });

    Ok(())
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppConfig {
    state
        .config
        .lock()
        .map(|cfg| cfg.clone())
        .unwrap_or_default()
}

#[tauri::command]
pub fn update_hotkey(
    hotkey: String,
    query_delay_ms: Option<u64>,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppConfig, String> {
    let normalized = hotkey.trim();
    if normalized.is_empty() {
        return Err("快捷键不能为空".into());
    }

    bind_hotkey(&app_handle, &state, normalized, "main")?;

    let mut guard = state
        .config
        .lock()
        .map_err(|_| "无法获取配置".to_string())?;
    guard.global_hotkey = normalized.to_string();
    guard.query_delay_ms = normalize_query_delay(query_delay_ms, guard.query_delay_ms);
    guard.save(&app_handle)?;
    Ok(guard.clone())
}

fn normalize_query_delay(candidate: Option<u64>, current: u64) -> u64 {
    let value = candidate.unwrap_or(current);
    value.clamp(MIN_QUERY_DELAY_MS, MAX_QUERY_DELAY_MS)
}

fn open_url(app_handle: &AppHandle, target: &str) -> Result<(), String> {
    app_handle
        .opener()
        .open_url(target.to_string(), Option::<&str>::None)
        .map_err(|err| err.to_string())
}

fn launch_win32_app(app: &ApplicationInfo) -> Result<(), String> {
    let path = Path::new(&app.path);
    if !path.exists() {
        return Err("目标程序不存在或已被移动".into());
    }

    let mut command = Command::new(path);
    if let Some(parent) = path.parent() {
        command.current_dir(parent);
    }

    command.spawn().map(|_| ()).map_err(|err| err.to_string())
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

fn match_bookmark(matcher: &SkimMatcherV2, bookmark: &BookmarkEntry, query: &str) -> Option<i64> {
    let mut best = matcher.fuzzy_match(&bookmark.title, query);

    if let Some(path) = &bookmark.folder_path {
        if let Some(score) = matcher.fuzzy_match(path, query) {
            let score = score - 5;
            if best.is_none_or(|current| score > current) {
                best = Some(score);
            }
        }
    }

    if let Some(score) = matcher
        .fuzzy_match(&bookmark.url, query)
        .map(|value| value - 8)
    {
        if best.is_none_or(|current| score > current) {
            best = Some(score);
        }
    }

    for keyword in &bookmark.keywords {
        if keyword.is_empty() {
            continue;
        }

        if let Some(score) = matcher.fuzzy_match(keyword, query) {
            let score = score - 8;
            if best.is_none_or(|current| score > current) {
                best = Some(score);
            }
        }
    }

    best
}
