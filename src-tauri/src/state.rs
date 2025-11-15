use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::{bookmarks::BookmarkEntry, config::AppConfig, models::ApplicationInfo};

#[derive(Clone)]
pub enum PendingAction {
    Application(ApplicationInfo),
    Bookmark(BookmarkEntry),
    Url(String),
    Search(String),
}

#[derive(Default)]
pub struct AppState {
    pub app_index: Arc<Mutex<Vec<ApplicationInfo>>>,
    pub bookmark_index: Arc<Mutex<Vec<BookmarkEntry>>>,
    pub config: Arc<Mutex<AppConfig>>,
    pub registered_hotkey: Arc<Mutex<Option<String>>>,
    pub pending_actions: Arc<Mutex<HashMap<String, PendingAction>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            app_index: Arc::new(Mutex::new(Vec::new())),
            bookmark_index: Arc::new(Mutex::new(Vec::new())),
            config: Arc::new(Mutex::new(AppConfig::default())),
            registered_hotkey: Arc::new(Mutex::new(None)),
            pending_actions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}
