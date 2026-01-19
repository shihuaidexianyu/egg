use std::{env, fs, path::PathBuf};

use log::{debug, warn};

use crate::models::ApplicationInfo;

const INDEX_CACHE_FILE: &str = "index.json";

pub fn load_app_index() -> Option<Vec<ApplicationInfo>> {
    let path = cache_path()?;
    let content = fs::read_to_string(&path).ok()?;
    match serde_json::from_str(&content) {
        Ok(apps) => Some(apps),
        Err(err) => {
            warn!("failed to parse app cache {:?}: {err}", path);
            None
        }
    }
}

pub fn save_app_index(apps: &[ApplicationInfo]) -> Result<(), String> {
    let Some(path) = cache_path() else {
        return Err("无法确定缓存目录".into());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let payload = serde_json::to_string(apps).map_err(|err| err.to_string())?;
    fs::write(&path, payload).map_err(|err| err.to_string())?;
    debug!("wrote app cache {:?}", path);
    Ok(())
}

fn cache_path() -> Option<PathBuf> {
    let base = env::var("LOCALAPPDATA").ok()?;
    Some(
        PathBuf::from(base)
            .join("egg")
            .join("cache")
            .join(INDEX_CACHE_FILE),
    )
}
