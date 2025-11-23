use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    path::Path,
    ptr,
    sync::Arc,
};

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;
use windows::{
    core::{HSTRING, PCWSTR},
    Win32::{
        Foundation::HWND,
        System::Com::{CoCreateInstance, CLSCTX_LOCAL_SERVER},
        UI::{
            Shell::{
                ApplicationActivationManager, IApplicationActivationManager, ShellExecuteW,
                ACTIVATEOPTIONS,
            },
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    },
};

use crate::windows_utils::{os_str_to_wide, ComGuard};

use crate::{
    bookmarks::{self, BookmarkEntry},
    config::AppConfig,
    hotkey::bind_hotkey,
    hotkey_capture, indexer,
    models::{AppType, ApplicationInfo, SearchResult},
    state::{AppState, PendingAction},
};

const MIN_QUERY_DELAY_MS: u64 = 50;
const MAX_QUERY_DELAY_MS: u64 = 2000;
const MIN_RESULT_LIMIT: u32 = 10;
const MAX_RESULT_LIMIT: u32 = 60;
const MIN_WINDOW_OPACITY: f32 = 0.0;
const MAX_WINDOW_OPACITY: f32 = 1.0;
pub const HIDE_WINDOW_EVENT: &str = "hide_window";
pub const OPEN_SETTINGS_EVENT: &str = "open_settings";
pub const SETTINGS_UPDATED_EVENT: &str = "settings_updated";
pub const FOCUS_INPUT_EVENT: &str = "focus_input";
pub const WINDOW_OPACITY_PREVIEW_EVENT: &str = "window_opacity_preview";

#[derive(Debug, Default, Deserialize)]
pub struct SettingsUpdatePayload {
    pub global_hotkey: Option<String>,
    pub query_delay_ms: Option<u64>,
    pub max_results: Option<u32>,
    pub enable_app_results: Option<bool>,
    pub enable_bookmark_results: Option<bool>,
    // 新增：三种模式的可配置前缀
    pub prefix_app: Option<String>,
    pub prefix_bookmark: Option<String>,
    pub prefix_search: Option<String>,
    pub launch_on_startup: Option<bool>,
    pub force_english_input: Option<bool>,
    pub debug_mode: Option<bool>,
    pub window_opacity: Option<f32>,
    pub system_tool_exclusions: Option<Vec<String>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum QueryMode {
    All,
    Bookmark,
    Application,
    Search,
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
            Some("search") | Some("s") => Self::Search,
            _ => Self::All,
        }
    }

    fn allows_bookmarks(&self) -> bool {
        matches!(self, Self::All | Self::Bookmark)
    }

    fn allows_applications(&self) -> bool {
        matches!(self, Self::All | Self::Application)
    }

    fn allows_web_search(&self) -> bool {
        matches!(self, Self::All | Self::Search)
    }
}

#[tauri::command]
pub async fn submit_query(
    query: String,
    mode: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<SearchResult>, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let query_mode = QueryMode::from_option(mode);
    let config_snapshot = state
        .config
        .lock()
        .map(|cfg| cfg.clone())
        .unwrap_or_default();
    let include_apps = config_snapshot.enable_app_results;
    let include_bookmarks = config_snapshot.enable_bookmark_results;
    let mut result_limit = config_snapshot
        .max_results
        .clamp(MIN_RESULT_LIMIT, MAX_RESULT_LIMIT) as usize;
    if result_limit == 0 {
        result_limit = MIN_RESULT_LIMIT as usize;
    }

    let app_index = state.app_index.clone();
    let bookmark_index = state.bookmark_index.clone();
    let query_str = trimmed.to_string();

    let (results, pending_actions) = tauri::async_runtime::spawn_blocking(move || {
        let mut results = Vec::new();
        let mut counter = 0usize;
        let mut pending_actions: HashMap<String, PendingAction> = HashMap::new();

        if is_url_like(&query_str) {
            let result_id = format!("url-{counter}");
            pending_actions.insert(result_id.clone(), PendingAction::Url(query_str.clone()));
            results.push(SearchResult {
                id: result_id,
                title: format!("打开网址: {query_str}"),
                subtitle: query_str.clone(),
                icon: String::new(),
                score: 200,
                action_id: "url".to_string(),
            });
            counter += 1;
        }

        let matcher = SkimMatcherV2::default();
        let apps = if query_mode.allows_applications() && include_apps {
            Some(app_index.lock().expect("failed to lock app index").clone())
        } else {
            None
        };
        let bookmarks = if query_mode.allows_bookmarks() && include_bookmarks {
            Some(
                bookmark_index
                    .lock()
                    .expect("failed to lock bookmark index")
                    .clone(),
            )
        } else {
            None
        };

        if let Some(apps) = apps.as_ref() {
            for app in apps.iter() {
                if let Some(score) = match_application(&matcher, app, &query_str) {
                    counter += 1;
                    let result_id = format!("app-{}", app.id);
                    pending_actions
                        .insert(result_id.clone(), PendingAction::Application(app.clone()));
                    let subtitle = app
                        .description
                        .clone()
                        .filter(|d| !d.is_empty())
                        .or_else(|| app.source_path.clone())
                        .unwrap_or_else(|| app.path.clone());
                    results.push(SearchResult {
                        id: result_id,
                        title: app.name.clone(),
                        subtitle,
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
                if let Some(score) = match_bookmark(&matcher, bookmark, &query_str) {
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
        if result_limit > 1 && results.len() >= result_limit {
            results.truncate(result_limit - 1);
        } else {
            results.truncate(result_limit);
        }

        if query_mode.allows_web_search() {
            let search_id = format!("search-{counter}");
            let search_url = format!(
                "https://google.com/search?q={}",
                urlencoding::encode(&query_str)
            );
            pending_actions.insert(search_id.clone(), PendingAction::Search(search_url.clone()));
            results.push(SearchResult {
                id: search_id,
                title: format!("在 Google 上搜索: {query_str}"),
                subtitle: String::from("Google 搜索"),
                icon: String::new(),
                score: i64::MIN,
                action_id: "search".to_string(),
            });
        }

        (results, pending_actions)
    })
    .await
    .map_err(|e| e.to_string())?;

    if let Ok(mut guard) = state.pending_actions.lock() {
        guard.clear();
        guard.extend(pending_actions);
    } else {
        log::warn!("无法记录搜索结果缓存，可能导致执行失败");
    }

    Ok(results)
}

#[tauri::command]
pub async fn execute_action(
    id: String,
    run_as_admin: bool,
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
            AppType::Win32 => launch_win32_app(&app, run_as_admin)?,
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
    let config_arc = Arc::clone(&state.config);

    tauri::async_runtime::spawn(async move {
        let exclusion_paths = {
            let config = config_arc.lock().unwrap();
            config.system_tool_exclusions.clone()
        };
        
        let apps = indexer::build_index(exclusion_paths).await;
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
pub fn update_settings(
    updates: SettingsUpdatePayload,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppConfig, String> {
    let mut guard = state
        .config
        .lock()
        .map_err(|_| "无法获取配置".to_string())?;

    if let Some(hotkey) = updates.global_hotkey {
        let normalized = hotkey.trim();
        if normalized.is_empty() {
            return Err("快捷键不能为空".into());
        }
        if normalized != guard.global_hotkey {
            bind_hotkey(&app_handle, &state, normalized, "main")?;
            guard.global_hotkey = normalized.to_string();
        }
    }

    if updates.query_delay_ms.is_some() {
        guard.query_delay_ms = normalize_query_delay(updates.query_delay_ms, guard.query_delay_ms);
    }

    if updates.max_results.is_some() {
        guard.max_results = normalize_max_results(updates.max_results, guard.max_results);
    }

    if let Some(value) = updates.enable_app_results {
        guard.enable_app_results = value;
    }

    if let Some(value) = updates.enable_bookmark_results {
        guard.enable_bookmark_results = value;
    }

    if let Some(value) = updates.launch_on_startup {
        crate::windows_utils::configure_launch_on_startup(value)?;
        guard.launch_on_startup = value;
    }

    if let Some(value) = updates.force_english_input {
        guard.force_english_input = value;
    }

    if let Some(value) = updates.debug_mode {
        guard.debug_mode = value;
    }

    if let Some(value) = updates.window_opacity {
        guard.window_opacity = clamp_window_opacity(value);
        let _ = app_handle.emit(WINDOW_OPACITY_PREVIEW_EVENT, guard.window_opacity);
    }

    // 同步模式前缀设置（如果前端传入了非空值）
    if let Some(prefix) = updates.prefix_app {
        guard.prefix_app = normalize_prefix(&prefix)
            .ok_or_else(|| "应用模式前缀需为单个字母，可选跟随空格或冒号".to_string())?;
    }

    if let Some(prefix) = updates.prefix_bookmark {
        guard.prefix_bookmark = normalize_prefix(&prefix)
            .ok_or_else(|| "书签模式前缀需为单个字母，可选跟随空格或冒号".to_string())?;
    }

    if let Some(prefix) = updates.prefix_search {
        guard.prefix_search = normalize_prefix(&prefix)
            .ok_or_else(|| "搜索模式前缀需为单个字母，可选跟随空格或冒号".to_string())?;
    }

    if let Some(paths) = updates.system_tool_exclusions {
        guard.system_tool_exclusions = paths;
    }

    guard.save(&app_handle)?;
    let snapshot = guard.clone();
    let _ = app_handle.emit(SETTINGS_UPDATED_EVENT, snapshot.clone());
    Ok(snapshot)
}

#[tauri::command]
pub fn preview_window_opacity(value: f32, app_handle: AppHandle) -> Result<(), String> {
    let clamped = clamp_window_opacity(value);
    let _ = app_handle.emit(WINDOW_OPACITY_PREVIEW_EVENT, clamped);
    Ok(())
}

#[tauri::command]
pub fn update_hotkey(
    hotkey: String,
    query_delay_ms: Option<u64>,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppConfig, String> {
    update_settings(
        SettingsUpdatePayload {
            global_hotkey: Some(hotkey),
            query_delay_ms,
            max_results: None,
            enable_app_results: None,
            enable_bookmark_results: None,
            prefix_app: None,
            prefix_bookmark: None,
            prefix_search: None,
            launch_on_startup: None,
            force_english_input: None,
            debug_mode: None,
            window_opacity: None,
            system_tool_exclusions: None,
        },
        app_handle,
        state,
    )
}

#[tauri::command]
pub fn begin_hotkey_capture(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    hotkey_capture::start(app_handle, state.inner().clone())
}

#[tauri::command]
pub fn end_hotkey_capture() -> Result<(), String> {
    hotkey_capture::stop()
}

fn normalize_query_delay(candidate: Option<u64>, current: u64) -> u64 {
    let value = candidate.unwrap_or(current);
    value.clamp(MIN_QUERY_DELAY_MS, MAX_QUERY_DELAY_MS)
}

fn normalize_max_results(candidate: Option<u32>, current: u32) -> u32 {
    let value = candidate.unwrap_or(current);
    value.clamp(MIN_RESULT_LIMIT, MAX_RESULT_LIMIT)
}

fn clamp_window_opacity(value: f32) -> f32 {
    value.clamp(MIN_WINDOW_OPACITY, MAX_WINDOW_OPACITY)
}

fn normalize_prefix(value: &str) -> Option<String> {
    let trimmed_start = value.trim_start();
    if trimmed_start.is_empty() {
        return None;
    }

    let mut chars = trimmed_start.chars();
    let Some(first) = chars.next() else {
        return None;
    };

    if !first.is_ascii_alphabetic() {
        return None;
    }

    let mut normalized = String::new();
    normalized.push(first.to_ascii_uppercase());

    let remainder: String = chars.collect();
    let mut remainder_chars = remainder.chars().filter(|c| !c.is_control());
    if let Some(next) = remainder_chars.next() {
        match next {
            ' ' | ':' => normalized.push(next),
            _ => return None,
        }

        if remainder_chars.any(|c| !c.is_whitespace()) {
            return None;
        }
    }

    Some(normalized)
}

fn open_url(app_handle: &AppHandle, target: &str) -> Result<(), String> {
    app_handle
        .opener()
        .open_url(target.to_string(), Option::<&str>::None)
        .map_err(|err| err.to_string())
}

fn launch_win32_app(app: &ApplicationInfo, run_as_admin: bool) -> Result<(), String> {
    let primary = Path::new(&app.path);
    match shell_execute_path(primary, run_as_admin) {
        Ok(_) => Ok(()),
        Err(primary_err) => {
            if let Some(source) = &app.source_path {
                launch_from_source(
                    source,
                    app.arguments.as_deref(),
                    app.working_directory.as_deref(),
                    run_as_admin,
                )
                .or(Err(primary_err))
            } else {
                Err(primary_err)
            }
        }
    }
}

fn shell_execute_path(path: &Path, run_as_admin: bool) -> Result<(), String> {
    if !path.exists() {
        return Err("目标程序不存在或已被移动".into());
    }

    let verb = if run_as_admin {
        Some(OsStr::new("runas"))
    } else {
        None
    };
    shell_execute_internal(path.as_os_str(), None, None, verb)
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

fn launch_from_source(
    source: &str,
    arguments: Option<&str>,
    working_directory: Option<&str>,
    run_as_admin: bool,
) -> Result<(), String> {
    let normalized = source.trim().trim_matches(|c| c == '"' || c == '\'');
    if normalized.is_empty() {
        return Err("备用路径无效".into());
    }

    if normalized.contains("://") && !Path::new(normalized).exists() {
        return shell_execute_uri(normalized);
    }

    shell_execute_raw(normalized, arguments, working_directory, run_as_admin)
}

fn shell_execute_raw(
    target: &str,
    arguments: Option<&str>,
    working_directory: Option<&str>,
    run_as_admin: bool,
) -> Result<(), String> {
    let target_os = OsString::from(target);
    let argument_os = arguments
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(OsString::from);
    let working_dir_os = working_directory
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(OsString::from);

    let verb = if run_as_admin {
        Some(OsStr::new("runas"))
    } else {
        None
    };

    shell_execute_internal(
        target_os.as_os_str(),
        argument_os.as_deref(),
        working_dir_os.as_deref(),
        verb,
    )
}

fn shell_execute_uri(uri: &str) -> Result<(), String> {
    let uri_os = OsString::from(uri);
    shell_execute_internal(uri_os.as_os_str(), None, None, None)
}

fn shell_execute_internal(
    target: &OsStr,
    arguments: Option<&OsStr>,
    working_directory: Option<&OsStr>,
    verb: Option<&OsStr>,
) -> Result<(), String> {
    let file_buffer = os_str_to_wide(target);
    let arg_buffer = arguments.map(os_str_to_wide);
    let dir_buffer = working_directory.map(os_str_to_wide);
    let verb_buffer = verb.map(os_str_to_wide);

    let arg_ptr = arg_buffer
        .as_ref()
        .map(|value| PCWSTR(value.as_ptr()))
        .unwrap_or(PCWSTR::null());
    let dir_ptr = dir_buffer
        .as_ref()
        .map(|value| PCWSTR(value.as_ptr()))
        .unwrap_or(PCWSTR::null());
    let verb_ptr = verb_buffer
        .as_ref()
        .map(|value| PCWSTR(value.as_ptr()))
        .unwrap_or(PCWSTR::null());

    let result = unsafe {
        ShellExecuteW(
            HWND(ptr::null_mut()),
            verb_ptr,
            PCWSTR(file_buffer.as_ptr()),
            arg_ptr,
            dir_ptr,
            SW_SHOWNORMAL,
        )
    };

    if result.0 as isize <= 32 {
        Err(format!(
            "无法启动程序 (ShellExecute 错误码 {})",
            result.0 as isize
        ))
    } else {
        Ok(())
    }
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
