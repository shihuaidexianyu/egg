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
    let tokens = tokenize_query(trimmed);
    if tokens.is_empty() {
        return (Vec::new(), HashMap::new());
    }

    let query_mode = QueryMode::from_option(mode);
    let include_apps = config.enable_app_results;
    let include_bookmarks = config.enable_bookmark_results;
    let mut result_limit = config.max_results.clamp(MIN_RESULT_LIMIT, MAX_RESULT_LIMIT) as usize;
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
            if let Some(score) = match_application(&matcher, app, trimmed, &tokens) {
                counter += 1;
                let result_id = format!("app-{}", app.id);
                pending_actions.insert(result_id.clone(), PendingAction::Application(app.clone()));
                let subtitle = app
                    .path
                    .clone();
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
            if let Some(score) = match_bookmark(&matcher, bookmark, trimmed, &tokens) {
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

fn match_application(
    matcher: &SkimMatcherV2,
    app: &ApplicationInfo,
    query: &str,
    tokens: &[&str],
) -> Option<i64> {
    let mut fields = Vec::new();
    fields.push(Field::new(&app.name, 120, true));
    for keyword in &app.keywords {
        if keyword.is_empty() {
            continue;
        }
        fields.push(Field::new(keyword.as_str(), 70, false));
    }
    if let Some(pinyin_index) = &app.pinyin_index {
        for entry in pinyin_index.split_whitespace() {
            let (full, initials) = split_pinyin_entry(entry);
            if let Some(full) = full {
                fields.push(Field::new(full, 85, false));
            }
            if let Some(initials) = initials {
                fields.push(Field::new(initials, 95, false));
            }
        }
    }

    score_fields(matcher, query, tokens, &fields)
}

fn match_bookmark(
    matcher: &SkimMatcherV2,
    bookmark: &BookmarkEntry,
    query: &str,
    tokens: &[&str],
) -> Option<i64> {
    let mut fields = Vec::new();
    fields.push(Field::new(&bookmark.title, 110, true));
    if let Some(path) = &bookmark.folder_path {
        fields.push(Field::new(path.as_str(), 65, false));
    }
    fields.push(Field::new(&bookmark.url, 45, false));
    for keyword in &bookmark.keywords {
        if keyword.is_empty() {
            continue;
        }
        fields.push(Field::new(keyword.as_str(), 55, false));
    }
    if let Some(pinyin_index) = &bookmark.pinyin_index {
        for entry in pinyin_index.split_whitespace() {
            let (full, initials) = split_pinyin_entry(entry);
            if let Some(full) = full {
                fields.push(Field::new(full, 80, false));
            }
            if let Some(initials) = initials {
                fields.push(Field::new(initials, 90, false));
            }
        }
    }

    score_fields(matcher, query, tokens, &fields)
}

fn split_pinyin_entry(entry: &str) -> (Option<&str>, Option<&str>) {
    if let Some((full, initials)) = entry.split_once('|') {
        (
            if full.is_empty() { None } else { Some(full) },
            if initials.is_empty() {
                None
            } else {
                Some(initials)
            },
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

#[derive(Clone, Copy)]
struct Field<'a> {
    text: &'a str,
    weight: i64,
    full_query_boost: bool,
}

impl<'a> Field<'a> {
    fn new(text: &'a str, weight: i64, full_query_boost: bool) -> Self {
        Self {
            text,
            weight,
            full_query_boost,
        }
    }
}

fn tokenize_query(query: &str) -> Vec<&str> {
    query
        .split_whitespace()
        .filter(|value| !value.is_empty())
        .collect()
}

fn score_fields(
    matcher: &SkimMatcherV2,
    query: &str,
    tokens: &[&str],
    fields: &[Field<'_>],
) -> Option<i64> {
    let mut total = 0i64;
    for token in tokens {
        let mut best: Option<i64> = None;
        for field in fields {
            if let Some(score) = score_token(matcher, field, token) {
                best = Some(best.map_or(score, |current| current.max(score)));
            }
        }
        let Some(best_score) = best else {
            return None;
        };
        total += best_score;
    }

    let query_lower = query.to_ascii_lowercase();
    let mut bonus = None;
    for field in fields.iter().filter(|field| field.full_query_boost) {
        let field_lower = field.text.to_ascii_lowercase();
        let score = if field_lower == query_lower {
            140
        } else if field_lower.starts_with(&query_lower) {
            70
        } else if field_lower.contains(&query_lower) {
            30
        } else {
            0
        };
        if score > 0 {
            update_best(&mut bonus, score + field.weight);
        }
    }
    if let Some(extra) = bonus {
        total += extra;
    }

    Some(total)
}

fn score_token(matcher: &SkimMatcherV2, field: &Field<'_>, token: &str) -> Option<i64> {
    let fuzzy = matcher.fuzzy_match(field.text, token)?;
    let token_lower = token.to_ascii_lowercase();
    let field_lower = field.text.to_ascii_lowercase();
    let mut score = fuzzy + field.weight;

    if field_lower == token_lower {
        score += 30;
    } else if field_lower.starts_with(&token_lower) {
        score += 18;
    } else if field_lower.contains(&token_lower) {
        score += 8;
    }

    let field_len = field.text.chars().count();
    let token_len = token.chars().count();
    let length_penalty = field_len.saturating_sub(token_len) as i64 / 6;
    Some(score - length_penalty)
}
