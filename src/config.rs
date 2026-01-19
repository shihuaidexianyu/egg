use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

const CONFIG_FILE: &str = "settings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub global_hotkey: String,
    #[serde(default = "default_query_delay")]
    // ensure backward compatibility when loading old config files
    pub query_delay_ms: u64,
    #[serde(default = "default_max_results")]
    pub max_results: u32,
    #[serde(default = "default_enable_app_results")]
    pub enable_app_results: bool,
    #[serde(default = "default_enable_bookmark_results")]
    pub enable_bookmark_results: bool,
    #[serde(default = "default_prefix_app")]
    pub prefix_app: String,
    #[serde(default = "default_prefix_bookmark")]
    pub prefix_bookmark: String,
    #[serde(default = "default_prefix_search")]
    pub prefix_search: String,
    #[serde(default = "default_launch_on_startup")]
    pub launch_on_startup: bool,
    #[serde(default = "default_force_english_input")]
    pub force_english_input: bool,
    #[serde(default = "default_debug_mode")]
    pub debug_mode: bool,
    #[serde(default = "default_system_tool_exclusions")]
    pub system_tool_exclusions: Vec<String>,
}

fn default_system_tool_exclusions() -> Vec<String> {
    vec![
        r"c:\windows\system32".to_string(),
        r"c:\windows\syswow64".to_string(),
        r"c:\windows\winsxs".to_string(),
        r"c:\windows\immersivecontrolpanel".to_string(),
        r"c:\program files\windows".to_string(),
        r"c:\program files (x86)\windows".to_string(),
    ]
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            global_hotkey: "Alt+Space".to_string(),
            query_delay_ms: default_query_delay(),
            max_results: default_max_results(),
            enable_app_results: default_enable_app_results(),
            enable_bookmark_results: default_enable_bookmark_results(),
            prefix_app: default_prefix_app(),
            prefix_bookmark: default_prefix_bookmark(),
            prefix_search: default_prefix_search(),
            launch_on_startup: default_launch_on_startup(),
            force_english_input: default_force_english_input(),
            debug_mode: default_debug_mode(),
            system_tool_exclusions: default_system_tool_exclusions(),
        }
    }
}

const fn default_query_delay() -> u64 {
    120
}

const fn default_max_results() -> u32 {
    40
}

const fn default_enable_app_results() -> bool {
    true
}

const fn default_enable_bookmark_results() -> bool {
    true
}

fn default_prefix_app() -> String {
    "R".to_string()
}

fn default_prefix_bookmark() -> String {
    "B".to_string()
}

fn default_prefix_search() -> String {
    "S".to_string()
}

const fn default_launch_on_startup() -> bool {
    false
}

const fn default_force_english_input() -> bool {
    true
}

const fn default_debug_mode() -> bool {
    false
}


impl AppConfig {
    pub fn load() -> Self {
        let Some(path) = config_path() else {
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

    #[allow(dead_code)]
    pub fn save(&self) -> Result<(), String> {
        let Some(path) = config_path() else {
            return Err("无法确定配置目录".into());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let data = serde_json::to_string_pretty(self).map_err(|err| err.to_string())?;
        fs::write(path, data).map_err(|err| err.to_string())
    }
}

fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("egg-cli").join(CONFIG_FILE))
}
