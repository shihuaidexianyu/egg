mod bookmarks;
mod cache;
mod config;
mod search_core;
mod execute;
mod indexer;
mod models;
mod state;
mod text_utils;
mod windows_utils;

use std::{
    io::{self, Write},
    sync::Arc,
    time::Duration,
};

use anyhow::Result;
use log::{debug, info, warn};

use crate::{
    config::AppConfig,
    search_core as core,
    execute::execute_action,
    indexer::build_index,
    state::{AppState, CachedSearch, PendingAction},
};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_secs()
        .init();

    println!("egg-cli v0.1.0 starting...");

    // Load configuration
    let config = AppConfig::load();
    debug!("Loaded configuration");

    // Initialize state
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
    println!("Type a query to search, or 'help' for commands.\n");

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

    // REPL loop
    run_repl(state).await?;

    Ok(())
}

async fn run_repl(state: Arc<AppState>) -> Result<()> {
    let mut current_results: Vec<(crate::models::SearchResult, PendingAction)> = Vec::new();

    loop {
        // Display prompt
        print!("> ");
        io::stdout().flush()?;

        // Read input
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        let trimmed = input.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Handle commands
        match trimmed.to_lowercase().as_str() {
            "quit" | "exit" | "q" => {
                println!("Goodbye!");
                break;
            }
            "help" | "h" => {
                print_help();
                continue;
            }
            "reindex" => {
                trigger_reindex(state.clone()).await?;
                println!("Reindex complete!");
                continue;
            }
            "clear" | "c" => {
                current_results.clear();
                println!("Cleared results");
                continue;
            }
            _ => {
                // Check if user is selecting by number
                if let Some(num_str) = trimmed.strip_prefix('!') {
                    if let Ok(index) = num_str.parse::<usize>() {
                        execute_by_index(state.clone(), &current_results, index)?;
                        continue;
                    }
                }

                // Otherwise, treat as search query
                let config_snapshot = state.config.lock().unwrap().clone();
                let app_index = state.app_index.lock().unwrap().clone();
                let bookmark_index = state.bookmark_index.lock().unwrap().clone();
                let cache_key = format!(
                    "{}|{}|{}|{}",
                    trimmed,
                    config_snapshot.enable_app_results,
                    config_snapshot.enable_bookmark_results,
                    config_snapshot.max_results
                );

                if let Ok(mut cache_guard) = state.search_cache.lock() {
                    if let Some(cached) = cache_guard.get(&cache_key) {
                        current_results.clear();
                        for result in &cached.results {
                            if let Some(action) = cached.pending_actions.get(&result.id) {
                                current_results.push((result.clone(), action.clone()));
                            }
                        }
                        display_results(&current_results);
                        continue;
                    }
                }

                let (results, pending_actions) = core::search(
                    trimmed.to_string(),
                    None, // mode
                    &app_index,
                    &bookmark_index,
                    &config_snapshot,
                );

                if let Ok(mut cache_guard) = state.search_cache.lock() {
                    cache_guard.insert(
                        cache_key,
                        CachedSearch {
                            results: results.clone(),
                            pending_actions: pending_actions.clone(),
                        },
                    );
                }

                // Store pending actions with their results
                current_results.clear();
                for result in &results {
                    if let Some(action) = pending_actions.get(&result.id) {
                        current_results.push((result.clone(), action.clone()));
                    }
                }

                // Display results
                display_results(&current_results);
            }
        }
    }

    Ok(())
}

fn display_results(results: &[(crate::models::SearchResult, PendingAction)]) {
    if results.is_empty() {
        println!("No results found.");
        return;
    }

    println!();
    for (index, (result, _action)) in results.iter().enumerate() {
        println!(
            "[{}] {} - {}",
            index + 1,
            result.title,
            result.subtitle
        );
    }
    println!();
    println!("Type !<number> to execute (e.g., !1), or another query to search again.");
}

fn execute_by_index(
    _state: Arc<AppState>,
    results: &[(crate::models::SearchResult, PendingAction)],
    index: usize,
) -> Result<()> {
    if index == 0 || index > results.len() {
        println!("Invalid index: {}", index);
        return Ok(());
    }

    let (_result, action) = &results[index - 1];

    println!("Executing: {:?}", action);

    match execute_action(action, false) {
        Ok(_) => {
            println!("Launched successfully!");
        }
        Err(e) => {
            println!("Error: {}", e);
        }
    }

    Ok(())
}

async fn trigger_reindex(state: Arc<AppState>) -> Result<()> {
    let app_index = Arc::clone(&state.app_index);
    let bookmark_index = Arc::clone(&state.bookmark_index);
    let config_arc = Arc::clone(&state.config);

    // Reindex apps
    let exclusion_paths = {
        let config = config_arc.lock().unwrap();
        config.system_tool_exclusions.clone()
    };

    let (apps_task, bookmarks_task) = tokio::join!(
        tokio::spawn(async move { build_index(exclusion_paths).await }),
        tokio::task::spawn_blocking(bookmarks::load_chrome_bookmarks),
    );
    let apps = apps_task.unwrap_or_default();
    if let Ok(mut guard) = app_index.lock() {
        *guard = apps.clone();
    }
    let _ = cache::save_app_index(&apps);

    let bookmarks = bookmarks_task.unwrap_or_default();
    if let Ok(mut guard) = bookmark_index.lock() {
        *guard = bookmarks;
    }

    if let Ok(mut cache_guard) = state.search_cache.lock() {
        cache_guard.clear();
    }

    Ok(())
}

fn print_help() {
    println!();
    println!("egg-cli Commands:");
    println!("  <query>       - Search for apps, bookmarks, or URLs");
    println!("  !<number>     - Execute search result by index (e.g., !1)");
    println!("  reindex       - Rebuild application and bookmark indexes");
    println!("  clear         - Clear current results");
    println!("  help, h       - Show this help message");
    println!("  quit, q       - Exit egg-cli");
    println!();
    println!("Examples:");
    println!("  chrome        - Search for Chrome");
    println!("  github.com    - Open URL directly");
    println!("  !1            - Launch first result");
    println!();
}
