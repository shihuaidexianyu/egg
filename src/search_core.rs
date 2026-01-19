use std::collections::HashMap;

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

use crate::{
    bookmarks::BookmarkEntry,
    config::AppConfig,
    models::{AppType, ApplicationInfo, SearchResult},
    state::PendingAction,
};

const MIN_RESULT_LIMIT: u32 = 10;
const MAX_RESULT_LIMIT: u32 = 60;

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

/// Core search function - extracted from submit_query command
/// Returns (results, pending_actions)
pub fn search(
    query: String,
    mode: Option<String>,
    app_index: &[ApplicationInfo],
    bookmark_index: &[BookmarkEntry],
    config: &AppConfig,
) -> (Vec<SearchResult>, HashMap<String, PendingAction>) {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return (Vec::new(), HashMap::new());
    }

    let query_mode = QueryMode::from_option(mode);
    let include_apps = config.enable_app_results;
    let include_bookmarks = config.enable_bookmark_results;
    let mut result_limit = config
        .max_results
        .clamp(MIN_RESULT_LIMIT, MAX_RESULT_LIMIT) as usize;
    if result_limit == 0 {
        result_limit = MIN_RESULT_LIMIT as usize;
    }

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
                    score: 200,
                    action_id: "url".to_string(),
                });
        counter += 1;
    }

    let matcher = SkimMatcherV2::default();

    if query_mode.allows_applications() && include_apps {
        for app in app_index.iter() {
            if let Some(score) = match_application(&matcher, app, trimmed) {
                counter += 1;
                let result_id = format!("app-{}", app.id);
                pending_actions.insert(result_id.clone(), PendingAction::Application(app.clone()));
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
                    score,
                    action_id: match app.app_type {
                        AppType::Win32 => "app".to_string(),
                        AppType::Uwp => "uwp".to_string(),
                    },
                });
            }
        }
    }

    if query_mode.allows_bookmarks() && include_bookmarks {
        for bookmark in bookmark_index.iter() {
            if let Some(score) = match_bookmark(&matcher, bookmark, trimmed) {
                counter += 1;
                let subtitle = match &bookmark.folder_path {
                    Some(path) => format!("收藏夹 · {path} · {}", bookmark.url),
                    None => format!("收藏夹 · {}", bookmark.url),
                };
                let result_id = format!("bookmark-{}", bookmark.id);
                pending_actions.insert(result_id.clone(), PendingAction::Bookmark(bookmark.clone()));
                results.push(SearchResult {
                    id: result_id,
                    title: bookmark.title.clone(),
                    subtitle,
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
            urlencoding::encode(trimmed)
        );
        pending_actions.insert(search_id.clone(), PendingAction::Search(search_url.clone()));
        results.push(SearchResult {
            id: search_id,
            title: format!("在 Google 上搜索: {trimmed}"),
            subtitle: String::from("Google 搜索"),
            score: i64::MIN,
            action_id: "search".to_string(),
        });
    }

    (results, pending_actions)
}

fn is_url_like(input: &str) -> bool {
    input.starts_with("http://")
        || input.starts_with("https://")
        || input.contains('.') && input.split_whitespace().count() == 1
}

fn match_application(matcher: &SkimMatcherV2, app: &ApplicationInfo, query: &str) -> Option<i64> {
    let mut best = None;
    let query_lower = query.to_ascii_lowercase();

    if let Some(score) = matcher.fuzzy_match(&app.name, query) {
        let mut weighted = 100 + score;
        if app.name.to_ascii_lowercase().starts_with(&query_lower) {
            weighted += 20;
        }
        update_best(&mut best, weighted);
    }

    if let Some(pinyin_index) = &app.pinyin_index {
        for entry in pinyin_index.split_whitespace() {
            let (full, initials) = split_pinyin_entry(entry);
            if let Some(full) = full {
                if let Some(score) = matcher.fuzzy_match(full, query) {
                    update_best(&mut best, 70 + score);
                }
            }
            if let Some(initials) = initials {
                if let Some(score) = matcher.fuzzy_match(initials, query) {
                    update_best(&mut best, 80 + score);
                }
            }
        }
    }

    for keyword in &app.keywords {
        if keyword.is_empty() {
            continue;
        }

        if let Some(score) = matcher.fuzzy_match(keyword, query) {
            update_best(&mut best, 50 + score);
        }
    }

    best
}

fn match_bookmark(matcher: &SkimMatcherV2, bookmark: &BookmarkEntry, query: &str) -> Option<i64> {
    let mut best = matcher.fuzzy_match(&bookmark.title, query);

    if let Some(pinyin_index) = &bookmark.pinyin_index {
        for entry in pinyin_index.split_whitespace() {
            let (full, initials) = split_pinyin_entry(entry);
            if let Some(full) = full {
                if let Some(score) = matcher.fuzzy_match(full, query) {
                    let weighted = 60 + score;
                    if best.is_none_or(|current| weighted > current) {
                        best = Some(weighted);
                    }
                }
            }
            if let Some(initials) = initials {
                if let Some(score) = matcher.fuzzy_match(initials, query) {
                    let weighted = 60 + score;
                    if best.is_none_or(|current| weighted > current) {
                        best = Some(weighted);
                    }
                }
            }
        }
    }

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

fn split_pinyin_entry(entry: &str) -> (Option<&str>, Option<&str>) {
    if let Some((full, initials)) = entry.split_once('|') {
        (
            if full.is_empty() { None } else { Some(full) },
            if initials.is_empty() { None } else { Some(initials) },
        )
    } else if entry.is_empty() {
        (None, None)
    } else {
        (Some(entry), None)
    }
}

fn update_best(best: &mut Option<i64>, candidate: i64) {
    if best.is_none_or(|current| candidate > current) {
        *best = Some(candidate);
    }
}
