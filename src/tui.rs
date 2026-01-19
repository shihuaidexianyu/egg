use std::{
    collections::HashMap,
    io,
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};

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
    cache,
    config::config_path,
    indexer::build_index,
    models::SearchResult,
    search_core as core,
    state::{AppState, CachedSearch, PendingAction},
};

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
    status_message: Option<String>,
    status_deadline: Option<Instant>,
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
            status_message: None,
            status_deadline: None,
        }
    }
}

const STATUS_MESSAGE_TTL: Duration = Duration::from_secs(2);

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

    handle_search_key_event(key, ui_state, app_state);
}

fn handle_search_key_event(key: KeyEvent, ui_state: &mut TuiState, app_state: &AppState) {
    if key_matches_blacklist_hotkey(key, app_state) {
        add_selected_to_blacklist(ui_state, app_state);
        return;
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('c') => {
                ui_state.should_quit = true;
            }
            KeyCode::Char('o') => open_settings_in_editor(app_state),
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
        KeyCode::Left => move_cursor(ui_state, -1),
        KeyCode::Right => move_cursor(ui_state, 1),
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

fn set_status_message(ui_state: &mut TuiState, message: impl Into<String>) {
    ui_state.status_message = Some(message.into());
    ui_state.status_deadline = Some(Instant::now() + STATUS_MESSAGE_TTL);
}

fn update_status_message(ui_state: &mut TuiState) {
    if let Some(deadline) = ui_state.status_deadline {
        if Instant::now() >= deadline {
            ui_state.status_message = None;
            ui_state.status_deadline = None;
        }
    }
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

fn render_ui(frame: &mut Frame, ui_state: &mut TuiState, _app_state: &AppState) {
    let theme = Theme::new();
    update_status_message(ui_state);
    let area = frame.size();
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.background)),
        area,
    );

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
            "  search".to_string(),
            Style::default().fg(theme.dim),
        ),
    ]);
    let left_widget = Paragraph::new(left).style(Style::default().bg(theme.background));
    frame.render_widget(left_widget, layout[0]);

    let label = if ui_state.input.trim().is_empty() {
        "recent"
    } else {
        "results"
    };
    let right_text = format!("{label}: {}", ui_state.results.len());
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
            let (type_label, type_color) = result_type_info(&result.action_id, theme);
            let mut subtitle_spans = Vec::new();
            subtitle_spans.push(Span::styled(type_label, Style::default().fg(type_color)));
            if !result.subtitle.trim().is_empty() {
                subtitle_spans.push(Span::styled(
                    format!(" {}", result.subtitle),
                    Style::default().fg(theme.dim),
                ));
            }
            let subtitle = Line::from(subtitle_spans);
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

fn result_type_info(action_id: &str, theme: Theme) -> (&'static str, Color) {
    match action_id {
        "app" => ("app", theme.accent),
        "uwp" => ("uwp", Color::Rgb(126, 211, 158)),
        "bookmark" => ("bookmark", Color::Rgb(122, 199, 242)),
        "url" => ("url", Color::Rgb(238, 185, 110)),
        "search" => ("search", Color::Rgb(190, 168, 255)),
        _ => ("Other", theme.dim),
    }
}

fn render_footer(frame: &mut Frame, area: Rect, ui_state: &TuiState, theme: Theme) {
    if let Some(message) = ui_state.status_message.as_deref() {
        let footer_widget = Paragraph::new(Line::from(Span::styled(
            message,
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )))
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center)
        .style(Style::default().bg(theme.background));
        frame.render_widget(footer_widget, area);
        return;
    }

    let key_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);
    let hint_style = Style::default().fg(theme.dim);
    let footer = Line::from(vec![
        Span::styled("Enter", key_style),
        Span::styled(": run  ", hint_style),
        Span::styled("Esc", key_style),
        Span::styled(": quit  ", hint_style),
        Span::styled("Up/Down", key_style),
        Span::styled(": move  ", hint_style),
        Span::styled("Ctrl+W", key_style),
        Span::styled(": delete  ", hint_style),
        Span::styled("Ctrl+O", key_style),
        Span::styled(": settings", hint_style),
    ]);
    let footer_widget = Paragraph::new(footer)
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center)
        .style(Style::default().bg(theme.background));
    frame.render_widget(footer_widget, area);
}

fn open_settings_in_editor(app_state: &AppState) {
    let _ = app_state.config.lock().unwrap().save();
    let Some(path) = config_path() else {
        return;
    };

    if open::that(&path).is_err() {
        let _ = Command::new("notepad").arg(path).spawn();
    }
}

fn key_matches_blacklist_hotkey(key: KeyEvent, app_state: &AppState) -> bool {
    let hotkey = {
        let config = app_state.config.lock().unwrap();
        config.blacklist_hotkey.clone()
    };
    let Some(spec) = parse_hotkey(&hotkey) else {
        return false;
    };
    hotkey_matches(key, &spec)
}

fn add_selected_to_blacklist(ui_state: &mut TuiState, app_state: &AppState) {
    let Some(index) = ui_state.list_state.selected() else {
        set_status_message(ui_state, "No selection to blacklist.");
        return;
    };
    let Some(result) = ui_state.results.get(index).cloned() else {
        set_status_message(ui_state, "No selection to blacklist.");
        return;
    };
    let Some(action) = ui_state.pending_actions.get(&result.id).cloned() else {
        set_status_message(ui_state, "Unable to resolve selection.");
        return;
    };
    let PendingAction::Application(app) = action else {
        set_status_message(ui_state, "Only apps can be blacklisted.");
        return;
    };
    let entry = app.path.trim();
    if entry.is_empty() {
        set_status_message(ui_state, "Selected app has no path.");
        return;
    }
    let entry = entry.to_string();
    let app_name = app.name.clone();
    let result_id = result.id.clone();

    let mut config = app_state.config.lock().unwrap();
    if config
        .system_tool_exclusions
        .iter()
        .any(|item| item.eq_ignore_ascii_case(&entry))
    {
        set_status_message(ui_state, format!("Already in blacklist: {app_name}"));
        return;
    }
    config.system_tool_exclusions.push(entry.clone());
    if config.save().is_err() {
        set_status_message(ui_state, "Failed to save settings.");
        return;
    }
    drop(config);

    if let Ok(mut guard) = app_state.app_index.lock() {
        guard.retain(|item| !item.path.eq_ignore_ascii_case(&entry));
    }

    if let Ok(mut recent_guard) = app_state.recent_actions.lock() {
        recent_guard.retain(|item| item.result.id != result_id);
    }

    if let Ok(mut cache_guard) = app_state.search_cache.lock() {
        cache_guard.clear();
    }
    refresh_app_index(app_state);
    refresh_results(ui_state, app_state);
    set_status_message(ui_state, format!("Added to blacklist: {app_name}"));
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct HotkeySpec {
    modifiers: KeyModifiers,
    code: KeyCode,
}

fn parse_hotkey(input: &str) -> Option<HotkeySpec> {
    let mut modifiers = KeyModifiers::empty();
    let mut code = None;

    for token in input.split('+').map(|value| value.trim()) {
        if token.is_empty() {
            continue;
        }
        let token_upper = token.to_ascii_uppercase();
        match token_upper.as_str() {
            "ALT" => modifiers.insert(KeyModifiers::ALT),
            "CTRL" | "CONTROL" => modifiers.insert(KeyModifiers::CONTROL),
            "SHIFT" => modifiers.insert(KeyModifiers::SHIFT),
            _ => {
                if code.is_some() {
                    return None;
                }
                code = parse_key_code(&token_upper);
            }
        }
    }

    code.map(|code| HotkeySpec { modifiers, code })
}

fn parse_key_code(token: &str) -> Option<KeyCode> {
    if token.len() == 1 {
        let ch = token.chars().next().unwrap();
        let normalized = ch.to_ascii_lowercase();
        if normalized.is_ascii_alphanumeric() || normalized == ' ' {
            return Some(KeyCode::Char(normalized));
        }
    }

    match token {
        "SPACE" => Some(KeyCode::Char(' ')),
        "ENTER" | "RETURN" => Some(KeyCode::Enter),
        "TAB" => Some(KeyCode::Tab),
        "ESC" | "ESCAPE" => Some(KeyCode::Esc),
        "BACKSPACE" => Some(KeyCode::Backspace),
        "LEFT" => Some(KeyCode::Left),
        "RIGHT" => Some(KeyCode::Right),
        "UP" => Some(KeyCode::Up),
        "DOWN" => Some(KeyCode::Down),
        _ => None,
    }
}

fn hotkey_matches(event: KeyEvent, spec: &HotkeySpec) -> bool {
    let mut event_mods = event.modifiers;
    let mut spec_mods = spec.modifiers;
    let mut event_code = event.code;
    let mut spec_code = spec.code;

    if let (KeyCode::Char(event_char), KeyCode::Char(spec_char)) = (event.code, spec.code) {
        event_mods.remove(KeyModifiers::SHIFT);
        spec_mods.remove(KeyModifiers::SHIFT);
        event_code = KeyCode::Char(event_char.to_ascii_lowercase());
        spec_code = KeyCode::Char(spec_char.to_ascii_lowercase());
    }

    event_mods == spec_mods && event_code == spec_code
}

fn refresh_app_index(app_state: &AppState) {
    let refresh_state = app_state.clone();
    tokio::spawn(async move {
        let exclusions = {
            let config = refresh_state.config.lock().unwrap();
            config.system_tool_exclusions.clone()
        };
        let refreshed = build_index(exclusions).await;
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
