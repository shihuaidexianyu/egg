use std::{
    env, fs,
    path::{Path, PathBuf},
};

use log::{debug, warn};
use serde_json::Value;
use sha1::{Digest, Sha1};
use winreg::{enums::*, RegKey};

use crate::text_utils::build_pinyin_index;

#[derive(Debug, Clone)]
pub struct BookmarkEntry {
    pub id: String,
    pub title: String,
    pub url: String,
    pub folder_path: Option<String>,
    pub keywords: Vec<String>,
    pub pinyin_index: Option<String>,
}

/// Loads Chrome bookmark entries from all detected profiles under LOCALAPPDATA.
pub fn load_chrome_bookmarks() -> Vec<BookmarkEntry> {
    let mut all_entries = Vec::new();

    for profile_dir in chrome_profile_dirs() {
        let Some(profile_name) = profile_dir
            .file_name()
            .and_then(|os| os.to_str())
            .map(|s| s.to_string())
        else {
            continue;
        };
        let display_name = profile_display_label(&profile_name);
        let bookmarks_path = profile_dir.join("Bookmarks");
        if !bookmarks_path.is_file() {
            continue;
        }

        match fs::read_to_string(&bookmarks_path) {
            Ok(content) => match serde_json::from_str::<Value>(&content) {
                Ok(json) => {
                    collect_entries_from_file(&json, &display_name, &mut all_entries);
                }
                Err(err) => warn!(
                    "failed to parse Chrome bookmarks {:?}: {err}",
                    bookmarks_path
                ),
            },
            Err(err) => warn!(
                "failed to read Chrome bookmarks {:?}: {err}",
                bookmarks_path
            ),
        }
    }

    debug!("loaded {} Chrome bookmark entries", all_entries.len());
    all_entries
}

fn chrome_profile_dirs() -> Vec<PathBuf> {
    let mut results = Vec::new();
    for root in chrome_user_data_roots() {
        if let Ok(entries) = fs::read_dir(&root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() && path.join("Bookmarks").is_file() {
                    results.push(path);
                }
            }
        }
    }
    results.sort();
    results.dedup();
    results
}

fn chrome_user_data_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
        let default_root = Path::new(&local_app_data)
            .join("Google")
            .join("Chrome")
            .join("User Data");
        if default_root.is_dir() {
            roots.push(default_root);
        }
    }

    if let Some(exe_path) = chrome_executable_path() {
        if let Some(exe_dir) = exe_path.parent() {
            let mut candidates = Vec::new();
            candidates.push(exe_dir.join("User Data"));
            candidates.push(exe_dir.join("Data"));
            if let Some(parent) = exe_dir.parent() {
                candidates.push(parent.join("User Data"));
                candidates.push(parent.join("Data"));
            }
            for candidate in candidates {
                if candidate.is_dir() {
                    roots.push(candidate);
                }
            }
        }
    }

    roots.sort();
    roots.dedup();
    roots
}

fn chrome_executable_path() -> Option<PathBuf> {
    let roots = [
        RegKey::predef(HKEY_LOCAL_MACHINE),
        RegKey::predef(HKEY_CURRENT_USER),
    ];
    for root in roots {
        if let Ok(key) = root
            .open_subkey(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\chrome.exe")
        {
            if let Ok(path) = key.get_value::<String, _>("") {
                let trimmed = path.trim().trim_matches('"');
                if !trimmed.is_empty() {
                    return Some(PathBuf::from(trimmed));
                }
            }
        }
    }
    None
}

fn collect_entries_from_file(json: &Value, profile_label: &str, acc: &mut Vec<BookmarkEntry>) {
    let Some(roots) = json.get("roots").and_then(|value| value.as_object()) else {
        return;
    };

    for (key, node) in roots.iter() {
        let mut path_stack = vec![profile_label.to_string()];
        if let Some(label) = root_display_label(key) {
            path_stack.push(label.to_string());
        }

        if let Some(children) = node.get("children").and_then(|value| value.as_array()) {
            for child in children {
                collect_node(child, profile_label, &mut path_stack, acc);
            }
        } else {
            collect_node(node, profile_label, &mut path_stack, acc);
        }
    }
}

fn collect_node(
    node: &Value,
    profile_label: &str,
    path_stack: &mut Vec<String>,
    acc: &mut Vec<BookmarkEntry>,
) {
    let node_type = node
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or_default();

    match node_type {
        "folder" => {
            let mut pushed = false;
            if let Some(name) = node.get("name").and_then(|value| value.as_str()) {
                let trimmed = name.trim();
                if !trimmed.is_empty() {
                    path_stack.push(trimmed.to_string());
                    pushed = true;
                }
            }

            if let Some(children) = node.get("children").and_then(|value| value.as_array()) {
                for child in children {
                    collect_node(child, profile_label, path_stack, acc);
                }
            }

            if pushed {
                path_stack.pop();
            }
        }
        "url" => {
            let Some(title) = node.get("name").and_then(|value| value.as_str()) else {
                return;
            };
            let Some(url) = node.get("url").and_then(|value| value.as_str()) else {
                return;
            };

            let title = title.trim();
            let url = url.trim();
            if title.is_empty() || url.is_empty() {
                return;
            }
            if !is_supported_url(url) {
                return;
            }

            let folder_path = if path_stack.is_empty() {
                None
            } else {
                Some(path_stack.join(" / "))
            };

            let mut keywords = Vec::new();
            keywords.push(title.to_string());
            keywords.push(url.to_string());
            if let Some(folder) = &folder_path {
                keywords.push(folder.clone());
                keywords.extend(folder.split('/').map(|segment| segment.trim().to_string()));
            }
            keywords.push(profile_label.to_string());
            keywords.retain(|value| !value.trim().is_empty());
            keywords.sort();
            keywords.dedup();
            let pinyin_index = build_pinyin_index(
                [Some(title), folder_path.as_deref(), Some(profile_label)]
                    .into_iter()
                    .flatten(),
            );

            let id = derive_bookmark_id(profile_label, node, url);
            acc.push(BookmarkEntry {
                id,
                title: title.to_string(),
                url: url.to_string(),
                folder_path,
                keywords,
                pinyin_index,
            });
        }
        _ => {}
    }
}

fn root_display_label(key: &str) -> Option<&'static str> {
    match key {
        "bookmark_bar" => Some("书签栏"),
        "other" => Some("其他书签"),
        "synced" => Some("已同步"),
        _ => None,
    }
}

fn profile_display_label(raw: &str) -> String {
    match raw {
        "Default" => "默认".to_string(),
        other => other.to_string(),
    }
}

fn is_supported_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

fn derive_bookmark_id(profile_label: &str, node: &Value, url: &str) -> String {
    if let Some(guid) = node.get("guid").and_then(|value| value.as_str()) {
        return format!("{profile_label}:{guid}");
    }
    if let Some(node_id) = node.get("id").and_then(|value| value.as_str()) {
        return format!("{profile_label}:{node_id}");
    }

    let mut hasher = Sha1::new();
    hasher.update(profile_label.as_bytes());
    hasher.update(url.as_bytes());
    format!("{profile_label}:{}", hex::encode(hasher.finalize()))
}

mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes
            .as_ref()
            .iter()
            .map(|byte| format!("{:02x}", byte))
            .collect()
    }
}
