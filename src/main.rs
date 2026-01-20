mod bookmarks;
mod cache;
mod config;
mod execute;
mod indexer;
mod models;
mod search_core;
mod state;
mod text_utils;
mod tui;
mod windows_utils;

use std::{sync::Arc, time::Duration};

use anyhow::Result;
use log::{debug, info, warn};

use crate::{
    config::AppConfig,
    execute::execute_action,
    indexer::build_index,
    state::{AppState, RecentEntry},
    tui::run_tui,
};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_secs()
        .init();

    println!("egg-cli v0.1.0 starting...");

    let config = AppConfig::load();
    debug!("Loaded configuration");

    let state = Arc::new(AppState::new());
    {
        let mut config_guard = state.config.lock().unwrap();
        *config_guard = config.clone();
    }

    if let Some(cached_apps) = cache::load_app_index() {
        if !cached_apps.is_empty() {
            info!("Loaded {} cached applications", cached_apps.len());
            let mut app_index = state.app_index.lock().unwrap();
            *app_index = cached_apps;
        }
    }

    println!("Building application index...");
    println!("Loading bookmarks...");
    let exclusion_paths = config.system_tool_exclusions.clone();
    let (apps_task, bookmarks_task) = tokio::join!(
        tokio::spawn(async move { build_index(exclusion_paths).await }),
        tokio::task::spawn_blocking(bookmarks::load_chrome_bookmarks),
    );
    let apps = match apps_task {
        Ok(apps) => apps,
        Err(err) => {
            warn!("app index task failed: {err}");
            Vec::new()
        }
    };
    let bookmarks = match bookmarks_task {
        Ok(bookmarks) => bookmarks,
        Err(err) => {
            warn!("bookmark index task failed: {err}");
            Vec::new()
        }
    };
    info!("Indexed {} applications", apps.len());
    info!("Loaded {} bookmarks", bookmarks.len());

    if !apps.is_empty() {
        let mut app_index = state.app_index.lock().unwrap();
        if *app_index != apps {
            *app_index = apps.clone();
            let _ = cache::save_app_index(&apps);
            if let Ok(mut cache_guard) = state.search_cache.lock() {
                cache_guard.clear();
            }
        }
    }
    {
        let mut bookmark_index = state.bookmark_index.lock().unwrap();
        *bookmark_index = bookmarks;
    }

    println!(
        "\nReady! Indexed {} apps and {} bookmarks.",
        state.app_index.lock().unwrap().len(),
        state.bookmark_index.lock().unwrap().len()
    );
    println!("Starting TUI...\n");

    let refresh_state = state.clone();
    let refresh_exclusions = config.system_tool_exclusions.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let refreshed = build_index(refresh_exclusions).await;
        if refreshed.is_empty() {
            return;
        }

        let mut updated = false;
        if let Ok(mut guard) = refresh_state.app_index.lock() {
            if *guard != refreshed {
                *guard = refreshed.clone();
                updated = true;
            }
        }

        if updated {
            let _ = cache::save_app_index(&refreshed);
            if let Ok(mut cache_guard) = refresh_state.search_cache.lock() {
                cache_guard.clear();
            }
        }
    });

    let pending = run_tui(state.clone())?;
    if let Some((result, action)) = pending {
        if let Ok(mut recent_guard) = state.recent_actions.lock() {
            recent_guard.insert(RecentEntry {
                result: result.clone(),
                action: action.clone(),
            });
        }
        if let Err(err) = execute_action(&action, false) {
            eprintln!("Error: {err}");
        }
    }

    Ok(())
}
