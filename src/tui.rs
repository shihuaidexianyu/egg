use std::{collections::HashMap, io, sync::Arc, time::Duration};

use anyhow::Result;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    prelude::*,
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Padding, Paragraph, Wrap},
};

use crate::{
    config::AppConfig,
    models::SearchResult,
    search_core as core,
    state::{AppState, CachedSearch, PendingAction},
    windows_utils::configure_launch_on_startup,
};

struct TerminalRestore;

impl Drop for TerminalRestore {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, LeaveAlternateScreen, cursor::Show);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Search,
    Settings,
}

impl ViewMode {
    fn label(self) -> &'static str {
        match self {
            ViewMode::Search => "search",
            ViewMode::Settings => "settings",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingKind {
    Toggle,
    Number { min: u32, max: u32 },
    Text,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingId {
    GlobalHotkey,
    QueryDelayMs,
    MaxResults,
    EnableAppResults,
    EnableBookmarkResults,
    ForceEnglishInput,
    DebugMode,
    LaunchOnStartup,
}

#[derive(Clone, Copy)]
struct SettingItem {
    id: SettingId,
    label: &'static str,
    description: &'static str,
    kind: SettingKind,
}

const SETTINGS: &[SettingItem] = &[
    SettingItem {
        id: SettingId::GlobalHotkey,
        label: "Global Hotkey",
        description: "Hotkey to toggle the launcher.",
        kind: SettingKind::Text,
    },
    SettingItem {
        id: SettingId::QueryDelayMs,
        label: "Query Delay (ms)",
        description: "Delay before search triggers (ms).",
        kind: SettingKind::Number { min: 0, max: 2000 },
    },
    SettingItem {
        id: SettingId::MaxResults,
        label: "Max Results",
        description: "Maximum results returned per query.",
        kind: SettingKind::Number { min: 10, max: 60 },
    },
    SettingItem {
        id: SettingId::EnableAppResults,
        label: "App Results",
        description: "Include installed applications in search.",
        kind: SettingKind::Toggle,
    },
    SettingItem {
        id: SettingId::EnableBookmarkResults,
        label: "Bookmark Results",
        description: "Include browser bookmarks in search.",
        kind: SettingKind::Toggle,
    },
    SettingItem {
        id: SettingId::ForceEnglishInput,
        label: "Force English Input",
        description: "Try to switch IME to English when active.",
        kind: SettingKind::Toggle,
    },
    SettingItem {
        id: SettingId::DebugMode,
        label: "Debug Mode",
        description: "Enable verbose logging.",
        kind: SettingKind::Toggle,
    },
    SettingItem {
        id: SettingId::LaunchOnStartup,
        label: "Launch on Startup",
        description: "Start egg automatically on login.",
        kind: SettingKind::Toggle,
    },
];

struct EditState {
    id: SettingId,
    buffer: String,
}

struct SettingsState {
    selected: usize,
    list_state: ListState,
    editing: Option<EditState>,
    status: Option<String>,
}

impl SettingsState {
    fn new() -> Self {
        let mut list_state = ListState::default();
        if !SETTINGS.is_empty() {
            list_state.select(Some(0));
        }
        Self {
            selected: 0,
            list_state,
            editing: None,
            status: None,
        }
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
    view_mode: ViewMode,
    settings: SettingsState,
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
            view_mode: ViewMode::Search,
            settings: SettingsState::new(),
        }
    }
}

pub(crate) fn run_tui(state: Arc<AppState>) -> Result<Option<(SearchResult, PendingAction)>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, cursor::Hide)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let _restore = TerminalRestore;

    let mut ui_state = TuiState::new();
    refresh_results(&mut ui_state, &state);

    loop {
        terminal.draw(|frame| render_ui(frame, &mut ui_state, &state))?;

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
    if key.kind == KeyEventKind::Release {
        return;
    }

    match ui_state.view_mode {
        ViewMode::Search => handle_search_key_event(key, ui_state, app_state),
        ViewMode::Settings => handle_settings_key_event(key, ui_state, app_state),
    }
}

fn handle_search_key_event(key: KeyEvent, ui_state: &mut TuiState, app_state: &AppState) {
    if matches!(key.code, KeyCode::Left | KeyCode::Right) {
        if ui_state.input.trim().is_empty() {
            ui_state.view_mode = ViewMode::Settings;
            ui_state.settings.status = None;
        } else if key.code == KeyCode::Left {
            move_cursor(ui_state, -1);
        } else {
            move_cursor(ui_state, 1);
        }
        return;
    }

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
            KeyCode::Left => move_cursor(ui_state, -1),
            KeyCode::Right => move_cursor(ui_state, 1),
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

fn handle_settings_key_event(key: KeyEvent, ui_state: &mut TuiState, app_state: &AppState) {
    if matches!(key.code, KeyCode::Left | KeyCode::Right) {
        ui_state.view_mode = ViewMode::Search;
        ui_state.settings.status = None;
        return;
    }

    if ui_state.settings.editing.is_some() {
        let mut editing = ui_state.settings.editing.take().unwrap();
        let mut keep_editing = true;
        match key.code {
            KeyCode::Esc => {
                keep_editing = false;
            }
            KeyCode::Enter => {
                commit_setting_edit(&editing, ui_state, app_state);
                keep_editing = false;
            }
            KeyCode::Backspace => {
                editing.buffer.pop();
            }
            KeyCode::Char(ch) => {
                if is_input_allowed(editing.id, ch) {
                    editing.buffer.push(ch);
                }
            }
            _ => {}
        }
        if keep_editing {
            ui_state.settings.editing = Some(editing);
        }
        return;
    }

    match key.code {
        KeyCode::Up => move_settings_selection(&mut ui_state.settings, -1),
        KeyCode::Down => move_settings_selection(&mut ui_state.settings, 1),
        KeyCode::Char(' ') => toggle_setting(ui_state, app_state),
        KeyCode::Enter => start_setting_edit(ui_state, app_state),
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

#[derive(Clone, Copy)]
struct Theme {
    background: Color,
    surface: Color,
    border: Color,
    accent: Color,
    text: Color,
    dim: Color,
    highlight_bg: Color,
    highlight_fg: Color,
}

impl Theme {
    fn new() -> Self {
        Self {
            background: Color::Rgb(18, 20, 23),
            surface: Color::Rgb(28, 31, 36),
            border: Color::Rgb(58, 62, 70),
            accent: Color::Rgb(242, 193, 78),
            text: Color::Rgb(232, 230, 227),
            dim: Color::Rgb(148, 153, 160),
            highlight_bg: Color::Rgb(45, 93, 124),
            highlight_fg: Color::Rgb(250, 250, 250),
        }
    }
}

fn render_ui(frame: &mut Frame, ui_state: &mut TuiState, app_state: &AppState) {
    let theme = Theme::new();
    let area = frame.size();
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.background)),
        area,
    );

    match ui_state.view_mode {
        ViewMode::Search => {
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Length(3),
                    Constraint::Min(1),
                    Constraint::Length(1),
                ])
                .split(area);

            let header_area = layout[0];
            let input_area = layout[1];
            let list_area = layout[2];
            let footer_area = layout[3];

            render_header(frame, header_area, ui_state, theme);
            render_input(frame, input_area, ui_state, theme);
            render_results(frame, list_area, ui_state, theme);
            render_footer(frame, footer_area, ui_state, theme);
        }
        ViewMode::Settings => {
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(1),
                    Constraint::Length(1),
                ])
                .split(area);

            let header_area = layout[0];
            let body_area = layout[1];
            let footer_area = layout[2];

            render_header(frame, header_area, ui_state, theme);
            render_settings(frame, body_area, ui_state, app_state, theme);
            render_footer(frame, footer_area, ui_state, theme);
        }
    }
}

fn render_header(frame: &mut Frame, area: Rect, ui_state: &TuiState, theme: Theme) {
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(area);

    let left = Line::from(vec![
        Span::styled(
            "egg",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {}", ui_state.view_mode.label()),
            Style::default().fg(theme.dim),
        ),
    ]);
    let left_widget = Paragraph::new(left).style(Style::default().bg(theme.background));
    frame.render_widget(left_widget, layout[0]);

    let right_text = if ui_state.view_mode == ViewMode::Settings {
        "settings".to_string()
    } else {
        let label = if ui_state.input.trim().is_empty() {
            "recent"
        } else {
            "results"
        };
        format!("{label}: {}", ui_state.results.len())
    };
    let right = Paragraph::new(Line::from(Span::styled(
        right_text,
        Style::default().fg(theme.dim),
    )))
    .alignment(Alignment::Right)
    .style(Style::default().bg(theme.background));
    frame.render_widget(right, layout[1]);
}

fn render_input(frame: &mut Frame, area: Rect, ui_state: &mut TuiState, theme: Theme) {
    let input_padding = 1u16;
    let input_width = area
        .width
        .saturating_sub(2 + input_padding.saturating_mul(2)) as usize;
    let (visible_input, cursor_x) = slice_input(&ui_state.input, ui_state.cursor, input_width);
    let input_span = if ui_state.input.is_empty() {
        Span::styled("Type to search...", Style::default().fg(theme.dim))
    } else {
        Span::styled(visible_input, Style::default().fg(theme.text))
    };

    let input = Paragraph::new(Line::from(input_span))
        .style(Style::default().bg(theme.surface))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme.accent))
                .style(Style::default().bg(theme.surface))
                .padding(Padding::horizontal(input_padding))
                .title(Span::styled(
                    " Search ",
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                )),
        );
    frame.render_widget(input, area);

    let cursor_x = area.x + 1 + input_padding + cursor_x as u16;
    let cursor_y = area.y + 1;
    let max_cursor_x = area.x + area.width.saturating_sub(1 + input_padding);
    if cursor_x < max_cursor_x && area.height > 2 {
        frame.set_cursor(cursor_x, cursor_y);
    }
}

fn render_results(frame: &mut Frame, area: Rect, ui_state: &mut TuiState, theme: Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(theme.surface))
        .title(Span::styled(
            " Results ",
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ));

    if ui_state.results.is_empty() {
        let message = if ui_state.input.trim().is_empty() {
            "No recent items. Type to search."
        } else {
            "No results. Try another query."
        };
        let empty = Paragraph::new(message)
            .style(Style::default().fg(theme.dim).bg(theme.surface))
            .alignment(Alignment::Center)
            .block(block);
        frame.render_widget(empty, area);
        return;
    }

    let items: Vec<ListItem> = ui_state
        .results
        .iter()
        .map(|result| {
            let title = Line::from(Span::styled(
                result.title.clone(),
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            ));
            let type_label = result_type_label(&result.action_id);
            let subtitle = Line::from(Span::styled(
                type_label,
                Style::default().fg(theme.dim),
            ));
            ListItem::new(vec![title, subtitle])
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .fg(theme.highlight_fg)
                .bg(theme.highlight_bg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, area, &mut ui_state.list_state);
}

fn result_type_label(action_id: &str) -> &'static str {
    match action_id {
        "app" => "Application",
        "uwp" => "Microsoft Store Application",
        "bookmark" => "Bookmark",
        "url" => "Web Address",
        "search" => "Web Search",
        _ => "Other",
    }
}

fn render_footer(frame: &mut Frame, area: Rect, ui_state: &TuiState, theme: Theme) {
    let key_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);
    let hint_style = Style::default().fg(theme.dim);
    let footer = if ui_state.view_mode == ViewMode::Search {
        Line::from(vec![
            Span::styled("Enter", key_style),
            Span::styled(": run  ", hint_style),
            Span::styled("Esc", key_style),
            Span::styled(": quit  ", hint_style),
            Span::styled("Up/Down", key_style),
            Span::styled(": move  ", hint_style),
            Span::styled("Ctrl+W", key_style),
            Span::styled(": delete  ", hint_style),
            Span::styled("Left/Right", key_style),
            Span::styled(": settings", hint_style),
        ])
    } else if ui_state.settings.editing.is_some() {
        Line::from(vec![
            Span::styled("Enter", key_style),
            Span::styled(": apply  ", hint_style),
            Span::styled("Esc", key_style),
            Span::styled(": cancel  ", hint_style),
            Span::styled("Left/Right", key_style),
            Span::styled(": search", hint_style),
        ])
    } else {
        Line::from(vec![
            Span::styled("Enter", key_style),
            Span::styled(": edit  ", hint_style),
            Span::styled("Space", key_style),
            Span::styled(": toggle  ", hint_style),
            Span::styled("Up/Down", key_style),
            Span::styled(": move  ", hint_style),
            Span::styled("Left/Right", key_style),
            Span::styled(": search", hint_style),
        ])
    };
    let footer_widget = Paragraph::new(footer)
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center)
        .style(Style::default().bg(theme.background));
    frame.render_widget(footer_widget, area);
}

fn render_settings(
    frame: &mut Frame,
    area: Rect,
    ui_state: &mut TuiState,
    app_state: &AppState,
    theme: Theme,
) {
    let config = app_state.config.lock().unwrap().clone();
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    let list_items: Vec<ListItem> = SETTINGS
        .iter()
        .map(|item| {
            let value = setting_value(&config, item.id);
            let is_editing = ui_state
                .settings
                .editing
                .as_ref()
                .is_some_and(|edit| edit.id == item.id);
            let value_style = if is_editing {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.dim)
            };
            let title = Line::from(Span::styled(
                item.label,
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            ));
            let value = Line::from(Span::styled(value, value_style));
            ListItem::new(vec![title, value])
        })
        .collect();

    let list_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(theme.surface))
        .title(Span::styled(
            " Settings ",
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ));
    let list = List::new(list_items)
        .block(list_block)
        .highlight_style(
            Style::default()
                .fg(theme.highlight_fg)
                .bg(theme.highlight_bg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, layout[0], &mut ui_state.settings.list_state);

    let current = SETTINGS
        .get(ui_state.settings.selected)
        .unwrap_or(&SETTINGS[0]);
    let mut detail_lines = vec![
        Line::from(Span::styled(
            current.label,
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            current.description,
            Style::default().fg(theme.dim),
        )),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            format!("Current: {}", setting_value(&config, current.id)),
            Style::default().fg(theme.text),
        )),
    ];

    if let Some(editing) = ui_state.settings.editing.as_ref() {
        if editing.id == current.id {
            detail_lines.push(Line::from(Span::raw("")));
            detail_lines.push(Line::from(Span::styled(
                format!("Editing: {}", editing.buffer),
                Style::default().fg(theme.accent),
            )));
        }
    }

    if let Some(status) = ui_state.settings.status.as_ref() {
        detail_lines.push(Line::from(Span::raw("")));
        detail_lines.push(Line::from(Span::styled(
            status.clone(),
            Style::default().fg(theme.dim),
        )));
    }

    let detail = Paragraph::new(detail_lines)
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme.border))
                .style(Style::default().bg(theme.surface))
                .title(Span::styled(
                    " Details ",
                    Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
                )),
        )
        .style(Style::default().bg(theme.surface));
    frame.render_widget(detail, layout[1]);
}

fn move_settings_selection(settings: &mut SettingsState, delta: isize) {
    let len = SETTINGS.len();
    if len == 0 {
        settings.selected = 0;
        settings.list_state.select(None);
        return;
    }
    let current = settings.selected;
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
    settings.selected = next;
    settings.list_state.select(Some(next));
}

fn toggle_setting(ui_state: &mut TuiState, app_state: &AppState) {
    let Some(item) = SETTINGS.get(ui_state.settings.selected) else {
        return;
    };
    if item.kind != SettingKind::Toggle {
        return;
    }

    let mut new_launch_setting = None;
    update_config(app_state, &mut ui_state.settings, |config| match item.id {
        SettingId::EnableAppResults => config.enable_app_results = !config.enable_app_results,
        SettingId::EnableBookmarkResults => {
            config.enable_bookmark_results = !config.enable_bookmark_results
        }
        SettingId::ForceEnglishInput => config.force_english_input = !config.force_english_input,
        SettingId::DebugMode => config.debug_mode = !config.debug_mode,
        SettingId::LaunchOnStartup => {
            config.launch_on_startup = !config.launch_on_startup;
            new_launch_setting = Some(config.launch_on_startup);
        }
        _ => {}
    });

    if let Some(value) = new_launch_setting {
        if let Err(err) = configure_launch_on_startup(value) {
            ui_state.settings.status = Some(format!("Startup update failed: {err}"));
        }
    }
}

fn start_setting_edit(ui_state: &mut TuiState, app_state: &AppState) {
    let Some(item) = SETTINGS.get(ui_state.settings.selected) else {
        return;
    };
    match item.kind {
        SettingKind::Number { .. } | SettingKind::Text => {
            let config = app_state.config.lock().unwrap().clone();
            let buffer = setting_value(&config, item.id);
            ui_state.settings.editing = Some(EditState {
                id: item.id,
                buffer,
            });
            ui_state.settings.status = None;
        }
        SettingKind::Toggle => toggle_setting(ui_state, app_state),
    }
}

fn commit_setting_edit(editing: &EditState, ui_state: &mut TuiState, app_state: &AppState) {
    match setting_kind(editing.id) {
        SettingKind::Number { min, max } => {
            let value = editing.buffer.trim().parse::<u32>();
            let Ok(value) = value else {
                ui_state.settings.status = Some("Invalid number".to_string());
                return;
            };
            let value = value.clamp(min, max);
            update_config(app_state, &mut ui_state.settings, |config| {
                match editing.id {
                    SettingId::QueryDelayMs => config.query_delay_ms = value as u64,
                    SettingId::MaxResults => config.max_results = value,
                    _ => {}
                }
            });
        }
        SettingKind::Text => {
            let value = editing.buffer.trim().to_string();
            if value.is_empty() {
                ui_state.settings.status = Some("Value cannot be empty".to_string());
                return;
            }
            update_config(app_state, &mut ui_state.settings, |config| {
                match editing.id {
                    SettingId::GlobalHotkey => config.global_hotkey = value.clone(),
                    _ => {}
                }
            });
        }
        SettingKind::Toggle => {}
    }
}

fn is_input_allowed(id: SettingId, ch: char) -> bool {
    match setting_kind(id) {
        SettingKind::Number { .. } => ch.is_ascii_digit(),
        SettingKind::Text => !ch.is_control(),
        SettingKind::Toggle => false,
    }
}

fn setting_kind(id: SettingId) -> SettingKind {
    SETTINGS
        .iter()
        .find(|item| item.id == id)
        .map(|item| item.kind)
        .unwrap_or(SettingKind::Text)
}

fn setting_value(config: &AppConfig, id: SettingId) -> String {
    match id {
        SettingId::GlobalHotkey => config.global_hotkey.clone(),
        SettingId::QueryDelayMs => config.query_delay_ms.to_string(),
        SettingId::MaxResults => config.max_results.to_string(),
        SettingId::EnableAppResults => bool_label(config.enable_app_results),
        SettingId::EnableBookmarkResults => bool_label(config.enable_bookmark_results),
        SettingId::ForceEnglishInput => bool_label(config.force_english_input),
        SettingId::DebugMode => bool_label(config.debug_mode),
        SettingId::LaunchOnStartup => bool_label(config.launch_on_startup),
    }
}

fn bool_label(value: bool) -> String {
    if value {
        "On".to_string()
    } else {
        "Off".to_string()
    }
}

fn update_config(
    app_state: &AppState,
    settings: &mut SettingsState,
    updater: impl FnOnce(&mut AppConfig),
) {
    let mut config = app_state.config.lock().unwrap();
    updater(&mut config);
    let save_result = config.save();
    drop(config);

    if let Ok(mut cache_guard) = app_state.search_cache.lock() {
        cache_guard.clear();
    }

    match save_result {
        Ok(_) => settings.status = Some("Saved".to_string()),
        Err(err) => settings.status = Some(format!("Save failed: {err}")),
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
