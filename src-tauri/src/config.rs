use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const CONFIG_FILE: &str = "settings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub global_hotkey: String,
    #[serde(default = "default_query_delay")]
    // ensure backward compatibility when loading old config files
    pub query_delay_ms: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            global_hotkey: "Alt+Space".to_string(),
            query_delay_ms: default_query_delay(),
        }
    }
}

const fn default_query_delay() -> u64 {
    120
}

impl AppConfig {
    pub fn load(handle: &AppHandle) -> Self {
        let Some(path) = config_path(handle) else {
            return Self::default();
        };

        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        match fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, handle: &AppHandle) -> Result<(), String> {
        let Some(path) = config_path(handle) else {
            return Err("无法确定配置目录".into());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let data = serde_json::to_string_pretty(self).map_err(|err| err.to_string())?;
        fs::write(path, data).map_err(|err| err.to_string())
    }
}

fn config_path(handle: &AppHandle) -> Option<PathBuf> {
    handle
        .path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join(CONFIG_FILE))
}
