use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

use crate::{
    bookmarks::BookmarkEntry,
    config::AppConfig,
    models::{ApplicationInfo, SearchResult},
};

#[derive(Clone, Debug)]
pub enum PendingAction {
    Application(ApplicationInfo),
    Bookmark(BookmarkEntry),
    Url(String),
    Search(String),
}

#[derive(Clone)]
pub struct AppState {
    pub app_index: Arc<Mutex<Vec<ApplicationInfo>>>,
    pub bookmark_index: Arc<Mutex<Vec<BookmarkEntry>>>,
    pub config: Arc<Mutex<AppConfig>>,
    pub pending_actions: Arc<Mutex<HashMap<String, PendingAction>>>,
    pub search_cache: Arc<Mutex<SearchCache>>,
    pub recent_actions: Arc<Mutex<RecentList>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            app_index: Arc::new(Mutex::new(Vec::new())),
            bookmark_index: Arc::new(Mutex::new(Vec::new())),
            config: Arc::new(Mutex::new(AppConfig::default())),
            pending_actions: Arc::new(Mutex::new(HashMap::new())),
            search_cache: Arc::new(Mutex::new(SearchCache::new(8))),
            recent_actions: Arc::new(Mutex::new(RecentList::new(12))),
        }
    }
}

#[derive(Clone)]
pub struct CachedSearch {
    pub results: Vec<SearchResult>,
    pub pending_actions: HashMap<String, PendingAction>,
}

#[derive(Clone)]
pub struct RecentEntry {
    pub result: SearchResult,
    pub action: PendingAction,
}

pub struct RecentList {
    capacity: usize,
    entries: VecDeque<RecentEntry>,
}

impl RecentList {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: VecDeque::new(),
        }
    }

    pub fn insert(&mut self, entry: RecentEntry) {
        if let Some(pos) = self
            .entries
            .iter()
            .position(|item| item.result.id == entry.result.id)
        {
            self.entries.remove(pos);
        }
        self.entries.push_front(entry);
        self.evict_if_needed();
    }

    pub fn items(&self) -> impl Iterator<Item = &RecentEntry> {
        self.entries.iter()
    }

    fn evict_if_needed(&mut self) {
        while self.entries.len() > self.capacity {
            self.entries.pop_back();
        }
    }
}

pub struct SearchCache {
    capacity: usize,
    entries: HashMap<String, CachedSearch>,
    order: VecDeque<String>,
}

impl SearchCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn get(&mut self, key: &str) -> Option<CachedSearch> {
        let entry = self.entries.get(key).cloned();
        if entry.is_some() {
            self.promote(key);
        }
        entry
    }

    pub fn insert(&mut self, key: String, value: CachedSearch) {
        if self.entries.contains_key(&key) {
            self.entries.insert(key.clone(), value);
            self.promote(&key);
            return;
        }

        self.entries.insert(key.clone(), value);
        self.order.push_back(key);
        self.evict_if_needed();
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    fn promote(&mut self, key: &str) {
        if let Some(pos) = self.order.iter().position(|item| item == key) {
            self.order.remove(pos);
            self.order.push_back(key.to_string());
        }
    }

    fn evict_if_needed(&mut self) {
        while self.order.len() > self.capacity {
            if let Some(front) = self.order.pop_front() {
                self.entries.remove(&front);
            }
        }
    }
}
