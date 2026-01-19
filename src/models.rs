use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AppType {
    Win32,
    Uwp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApplicationInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub source_path: Option<String>,
    pub app_type: AppType,
    pub description: Option<String>,
    pub keywords: Vec<String>,
    #[serde(default)]
    pub pinyin_index: Option<String>,
    pub working_directory: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub score: i64,
    pub action_id: String,
}
