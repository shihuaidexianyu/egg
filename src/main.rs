mod bookmarks;
mod cache;
mod config;
mod execute;
mod indexer;
mod models;
mod search_core;
mod state;
mod text_utils;
mod windows_utils;

use std::{
    collections::HashMap,
    io,
    sync::Arc,
    time::Duration,
};

use anyhow::Result;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use log::{debug, info, warn};
use ratatui::{
    backend::CrosstermBackend,
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::{
    config::AppConfig,
    execute::execute_action,
    indexer::build_index,
    models::SearchResult,
    search_core as core,
    state::{AppState, CachedSearch, PendingAction, RecentEntry},
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

struct TerminalRestore;

impl Drop for TerminalRestore {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, LeaveAlternateScreen, cursor::Show);
    }
}

struct TuiState {
    input: String,
    cursor: usize,
    results: Vec<SearchResult>,
    pending_actions: HashMap<String, PendingAction>,
    list_state: ListState,
    should_quit: bool,
    pending_action: Option<PendingAction>,
    pending_result: Option<SearchResult>,
}

impl TuiState {
    fn new() -> Self {
        Self {
            input: String::new(),
            cursor: 0,
            results: Vec::new(),
            pending_actions: HashMap::new(),
            list_state: ListState::default(),
            should_quit: false,
            pending_action: None,
            pending_result: None,
        }
    }
}

fn run_tui(state: Arc<AppState>) -> Result<Option<(SearchResult, PendingAction)>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, cursor::Hide)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let _restore = TerminalRestore;

    let mut ui_state = TuiState::new();
    refresh_results(&mut ui_state, &state);

    loop {
        terminal.draw(|frame| render_ui(frame, &mut ui_state))?;

        if ui_state.should_quit {
            break;
        }

        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                handle_key_event(key, &mut ui_state, &state);
            }
        }
    }

    terminal.show_cursor()?;
    Ok(ui_state
        .pending_action
        .zip(ui_state.pending_result)
        .map(|(action, result)| (result, action)))
}

fn handle_key_event(key: KeyEvent, ui_state: &mut TuiState, app_state: &AppState) {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('c') => {
                ui_state.should_quit = true;
            }
            KeyCode::Char('n') => move_selection(ui_state, 1),
            KeyCode::Char('p') => move_selection(ui_state, -1),
            KeyCode::Char('w') => {
                delete_prev_word(ui_state);
                refresh_results(ui_state, app_state);
            }
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Esc => ui_state.should_quit = true,
        KeyCode::Enter => {
            if let Some(index) = ui_state.list_state.selected() {
                if let Some(result) = ui_state.results.get(index).cloned() {
                    if let Some(action) = ui_state.pending_actions.get(&result.id).cloned() {
                        ui_state.pending_action = Some(action);
                        ui_state.pending_result = Some(result);
                        ui_state.should_quit = true;
                    }
                }
            }
        }
        KeyCode::Up => move_selection(ui_state, -1),
        KeyCode::Down => move_selection(ui_state, 1),
        KeyCode::Left => move_cursor(ui_state, -1),
        KeyCode::Right => move_cursor(ui_state, 1),
        KeyCode::Home => ui_state.cursor = 0,
        KeyCode::End => ui_state.cursor = ui_state.input.chars().count(),
        KeyCode::Backspace => {
            if delete_char_before_cursor(ui_state) {
                refresh_results(ui_state, app_state);
            }
        }
        KeyCode::Delete => {
            if delete_char_at_cursor(ui_state) {
                refresh_results(ui_state, app_state);
            }
        }
        KeyCode::Char(ch) => {
            if !key.modifiers.contains(KeyModifiers::ALT) {
                insert_char(ui_state, ch);
                refresh_results(ui_state, app_state);
            }
        }
        _ => {}
    }
}

fn refresh_results(ui_state: &mut TuiState, app_state: &AppState) {
    let trimmed = ui_state.input.trim();
    if trimmed.is_empty() {
        let recent_guard = app_state.recent_actions.lock().unwrap();
        ui_state.results = recent_guard
            .items()
            .map(|entry| entry.result.clone())
            .collect();
        ui_state.pending_actions = recent_guard
            .items()
            .map(|entry| (entry.result.id.clone(), entry.action.clone()))
            .collect();
        reset_selection(ui_state);
        return;
    }

    let config_snapshot = app_state.config.lock().unwrap().clone();
    let app_index = app_state.app_index.lock().unwrap().clone();
    let bookmark_index = app_state.bookmark_index.lock().unwrap().clone();
    let cache_key = format!(
        "{}|{}|{}|{}",
        trimmed,
        config_snapshot.enable_app_results,
        config_snapshot.enable_bookmark_results,
        config_snapshot.max_results
    );

    if let Ok(mut cache_guard) = app_state.search_cache.lock() {
        if let Some(cached) = cache_guard.get(&cache_key) {
            ui_state.results = cached.results.clone();
            ui_state.pending_actions = cached.pending_actions.clone();
            reset_selection(ui_state);
            return;
        }
    }

    let (results, pending_actions) = core::search(
        trimmed.to_string(),
        None,
        &app_index,
        &bookmark_index,
        &config_snapshot,
    );

    if let Ok(mut cache_guard) = app_state.search_cache.lock() {
        cache_guard.insert(
            cache_key,
            CachedSearch {
                results: results.clone(),
                pending_actions: pending_actions.clone(),
            },
        );
    }

    ui_state.results = results;
    ui_state.pending_actions = pending_actions;
    reset_selection(ui_state);
}

fn reset_selection(ui_state: &mut TuiState) {
    if ui_state.results.is_empty() {
        ui_state.list_state.select(None);
    } else {
        ui_state.list_state.select(Some(0));
    }
}

fn move_selection(ui_state: &mut TuiState, delta: isize) {
    let len = ui_state.results.len();
    if len == 0 {
        ui_state.list_state.select(None);
        return;
    }

    let current = ui_state.list_state.selected().unwrap_or(0);
    let next = if delta < 0 {
        if current == 0 {
            len - 1
        } else {
            current - 1
        }
    } else if current + 1 >= len {
        0
    } else {
        current + 1
    };

    ui_state.list_state.select(Some(next));
}

fn move_cursor(ui_state: &mut TuiState, delta: isize) {
    let len = ui_state.input.chars().count();
    if delta < 0 {
        ui_state.cursor = ui_state.cursor.saturating_sub(1);
    } else if ui_state.cursor < len {
        ui_state.cursor += 1;
    }
}

fn insert_char(ui_state: &mut TuiState, ch: char) {
    let byte_index = char_to_byte_index(&ui_state.input, ui_state.cursor);
    ui_state.input.insert(byte_index, ch);
    ui_state.cursor += 1;
}

fn delete_char_before_cursor(ui_state: &mut TuiState) -> bool {
    if ui_state.cursor == 0 {
        return false;
    }
    let start = char_to_byte_index(&ui_state.input, ui_state.cursor - 1);
    let end = char_to_byte_index(&ui_state.input, ui_state.cursor);
    ui_state.input.replace_range(start..end, "");
    ui_state.cursor -= 1;
    true
}

fn delete_char_at_cursor(ui_state: &mut TuiState) -> bool {
    let len = ui_state.input.chars().count();
    if ui_state.cursor >= len {
        return false;
    }
    let start = char_to_byte_index(&ui_state.input, ui_state.cursor);
    let end = char_to_byte_index(&ui_state.input, ui_state.cursor + 1);
    ui_state.input.replace_range(start..end, "");
    true
}

fn delete_prev_word(ui_state: &mut TuiState) {
    if ui_state.cursor == 0 {
        return;
    }
    let cutoff = char_to_byte_index(&ui_state.input, ui_state.cursor);
    let prefix = &ui_state.input[..cutoff];
    let mut chars: Vec<char> = prefix.chars().collect();

    while let Some(ch) = chars.last() {
        if !ch.is_whitespace() {
            break;
        }
        chars.pop();
    }

    while let Some(ch) = chars.last() {
        if ch.is_whitespace() {
            break;
        }
        chars.pop();
    }

    let new_len = chars.len();
    let start = char_to_byte_index(&ui_state.input, new_len);
    ui_state.input.replace_range(start..cutoff, "");
    ui_state.cursor = new_len;
}

fn char_to_byte_index(input: &str, char_index: usize) -> usize {
    input
        .char_indices()
        .nth(char_index)
        .map(|(idx, _)| idx)
        .unwrap_or_else(|| input.len())
}

fn render_ui(frame: &mut Frame, ui_state: &mut TuiState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(frame.size());

    let input_area = layout[0];
    let list_area = layout[1];

    let input_width = input_area.width.saturating_sub(2) as usize;
    let (visible_input, cursor_x) = slice_input(&ui_state.input, ui_state.cursor, input_width);
    let input = Paragraph::new(visible_input).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title("Search"),
    );
    frame.render_widget(input, input_area);

    let items: Vec<ListItem> = ui_state
        .results
        .iter()
        .map(|result| {
            let title = Line::from(Span::styled(
                result.title.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ));
            let subtitle = Line::from(Span::styled(
                result.subtitle.clone(),
                Style::default().fg(Color::DarkGray),
            ));
            ListItem::new(vec![title, subtitle])
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Results"))
        .highlight_style(Style::default().fg(Color::White).bg(Color::Blue))
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, list_area, &mut ui_state.list_state);

    let cursor_x = input_area.x + 1 + cursor_x as u16;
    let cursor_y = input_area.y + 1;
    if cursor_x < input_area.x + input_area.width.saturating_sub(1) && input_area.height > 2 {
        frame.set_cursor(cursor_x, cursor_y);
    }
}

fn slice_input(input: &str, cursor: usize, width: usize) -> (String, usize) {
    let len = input.chars().count();
    if width == 0 {
        return (String::new(), 0);
    }

    let start = if len <= width {
        0
    } else if cursor >= width {
        cursor - width + 1
    } else {
        0
    };
    let end = (start + width).min(len);
    let slice = slice_chars(input, start, end);
    (slice, cursor.saturating_sub(start))
}

fn slice_chars(input: &str, start: usize, end: usize) -> String {
    let mut output = String::new();
    for (index, ch) in input.chars().enumerate() {
        if index >= end {
            break;
        }
        if index >= start {
            output.push(ch);
        }
    }
    output
}
