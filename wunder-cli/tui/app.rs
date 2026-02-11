use anyhow::{anyhow, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use futures::StreamExt;
use serde_json::{json, Value};
use tokio::sync::mpsc::{self, error::TryRecvError, UnboundedReceiver};
use wunder_server::schemas::StreamEvent;

use crate::args::GlobalArgs;
use crate::runtime::CliRuntime;
use crate::slash_command::{self, ParsedSlashCommand, SlashCommand};

const MAX_LOG_ENTRIES: usize = 1200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    Info,
    User,
    Assistant,
    Tool,
    Error,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub kind: LogKind,
    pub text: String,
}

enum StreamMessage {
    Event(StreamEvent),
    Error(String),
    Done,
}

#[derive(Debug, Clone, Default)]
struct ConfigWizardState {
    base_url: Option<String>,
    api_key: Option<String>,
    model_name: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct WrappedInputLine {
    start: usize,
    end: usize,
}

pub struct TuiApp {
    runtime: CliRuntime,
    global: GlobalArgs,
    session_id: String,
    input: String,
    input_cursor: usize,
    input_viewport_width: u16,
    logs: Vec<LogEntry>,
    busy: bool,
    should_quit: bool,
    history: Vec<String>,
    history_cursor: Option<usize>,
    history_draft: String,
    active_assistant: Option<usize>,
    stream_rx: Option<UnboundedReceiver<StreamMessage>>,
    model_name: String,
    tool_call_mode: String,
    model_max_rounds: u32,
    model_max_context: Option<u32>,
    session_stats: crate::SessionStatsSnapshot,
    last_usage: Option<String>,
    config_wizard: Option<ConfigWizardState>,
    stream_saw_output: bool,
    stream_saw_final: bool,
    transcript_offset_from_bottom: u16,
    session_stats_dirty: bool,
    shortcuts_visible: bool,
}

impl TuiApp {
    pub async fn new(
        runtime: CliRuntime,
        global: GlobalArgs,
        session_override: Option<String>,
    ) -> Result<Self> {
        let session_id =
            session_override.unwrap_or_else(|| runtime.resolve_session(global.session.as_deref()));
        runtime.save_session(&session_id).ok();

        let mut app = Self {
            runtime,
            global,
            session_id,
            input: String::new(),
            input_cursor: 0,
            input_viewport_width: 1,
            logs: Vec::new(),
            busy: false,
            should_quit: false,
            history: Vec::new(),
            history_cursor: None,
            history_draft: String::new(),
            active_assistant: None,
            stream_rx: None,
            model_name: "<none>".to_string(),
            tool_call_mode: "tool_call".to_string(),
            model_max_rounds: crate::CLI_MIN_MAX_ROUNDS,
            model_max_context: None,
            session_stats: crate::SessionStatsSnapshot::default(),
            last_usage: None,
            config_wizard: None,
            stream_saw_output: false,
            stream_saw_final: false,
            transcript_offset_from_bottom: 0,
            session_stats_dirty: false,
            shortcuts_visible: false,
        };
        app.sync_model_status().await;
        app.reload_session_stats().await;
        app.push_log(
            LogKind::Info,
            "wunder-cli tui mode. type /help for commands.".to_string(),
        );
        Ok(app)
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn status_line(&self) -> String {
        let context_summary = if let Some(max_context) = self.model_max_context {
            let percent_left = crate::context_left_percent(
                self.session_stats.context_used_tokens,
                Some(max_context),
            )
            .unwrap_or(0);
            format!("{percent_left}% context left")
        } else {
            format!(
                "{} context used",
                self.session_stats.context_used_tokens.max(0)
            )
        };
        let running_hint = if self.busy {
            "working..."
        } else {
            "? for shortcuts"
        };
        let usage_hint = self
            .last_usage
            .as_deref()
            .map(|value| format!(" | last tokens {value}"))
            .unwrap_or_default();
        let scroll_hint = if self.transcript_offset_from_bottom > 0 {
            format!(" | scroll -{}", self.transcript_offset_from_bottom)
        } else {
            String::new()
        };
        format!("  {running_hint}{usage_hint}{scroll_hint}    {context_summary}")
    }

    pub fn shortcuts_visible(&self) -> bool {
        self.shortcuts_visible
    }

    pub fn shortcuts_lines(&self) -> Vec<String> {
        vec![
            "Esc / ?               close shortcuts".to_string(),
            "Enter                 send message".to_string(),
            "Shift+Enter / Ctrl+J  insert newline".to_string(),
            "Left / Right          move cursor".to_string(),
            "Ctrl+B / Ctrl+F       move cursor".to_string(),
            "Alt+B / Alt+F         move by word".to_string(),
            "Alt+Left/Right        move by word".to_string(),
            "Ctrl+W / Alt+Backspace delete previous word".to_string(),
            "Alt+Delete            delete next word".to_string(),
            "Ctrl+U / Ctrl+K       delete to line start/end".to_string(),
            "Ctrl+A / Ctrl+E       move to line start/end".to_string(),
            "Up / Down             history (or move line in multiline)".to_string(),
            "Tab                   complete slash command".to_string(),
            "PgUp/PgDn             scroll transcript".to_string(),
            "Mouse Wheel           scroll transcript".to_string(),
            "Ctrl+N / Ctrl+L       new session / clear transcript".to_string(),
            "Ctrl+C                exit".to_string(),
        ]
    }

    pub fn set_input_viewport(&mut self, viewport_width: u16) {
        self.input_viewport_width = viewport_width.max(1);
    }

    pub fn input_view(&self, viewport_width: u16, viewport_height: u16) -> (String, u16, u16) {
        let width = viewport_width.max(1) as usize;
        let height = viewport_height.max(1) as usize;
        let lines = build_wrapped_input_lines(&self.input, width);

        let cursor = self.input_cursor.min(self.input.len());
        let (cursor_row, cursor_col) = cursor_visual_position(&self.input, &lines, cursor);

        let mut start_line = lines.len().saturating_sub(height);
        if cursor_row < start_line {
            start_line = cursor_row;
        }
        if cursor_row >= start_line.saturating_add(height) {
            start_line = cursor_row.saturating_sub(height.saturating_sub(1));
        }

        let end_line = (start_line + height).min(lines.len());
        let display = lines[start_line..end_line]
            .iter()
            .map(|line| self.input[line.start..line.end].to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let cursor_y = cursor_row.saturating_sub(start_line) as u16;
        let cursor_x = cursor_col.min(width.saturating_sub(1)) as u16;
        (display, cursor_x, cursor_y)
    }

    pub fn visible_logs(&self, max_entries: usize) -> Vec<LogEntry> {
        let len = self.logs.len();
        if len <= max_entries {
            return self.logs.clone();
        }
        self.logs[len - max_entries..].to_vec()
    }

    pub fn transcript_scroll(&self, viewport_height: u16) -> u16 {
        let max_scroll = self.max_transcript_scroll(viewport_height);
        let offset = self.transcript_offset_from_bottom.min(max_scroll);
        max_scroll.saturating_sub(offset)
    }

    fn scroll_transcript_up(&mut self, lines: u16) {
        self.transcript_offset_from_bottom =
            self.transcript_offset_from_bottom.saturating_add(lines);
    }

    fn scroll_transcript_down(&mut self, lines: u16) {
        self.transcript_offset_from_bottom =
            self.transcript_offset_from_bottom.saturating_sub(lines);
    }

    fn scroll_transcript_to_top(&mut self) {
        self.transcript_offset_from_bottom = u16::MAX;
    }

    fn scroll_transcript_to_bottom(&mut self) {
        self.transcript_offset_from_bottom = 0;
    }

    fn max_transcript_scroll(&self, viewport_height: u16) -> u16 {
        let viewport = viewport_height.max(1);
        self.total_transcript_lines().saturating_sub(viewport)
    }

    fn total_transcript_lines(&self) -> u16 {
        let total = self
            .logs
            .iter()
            .map(|entry| {
                let count = entry.text.lines().count();
                count.max(1)
            })
            .sum::<usize>();
        total.min(u16::MAX as usize) as u16
    }

    pub fn popup_lines(&self) -> Vec<String> {
        let trimmed = self.input.trim_start();
        if !trimmed.starts_with('/') {
            return Vec::new();
        }
        let body = trimmed.trim_start_matches('/');
        slash_command::popup_lines(body, 7)
    }

    pub async fn drain_stream_events(&mut self) {
        loop {
            let Some(receiver) = self.stream_rx.as_mut() else {
                break;
            };
            match receiver.try_recv() {
                Ok(message) => self.handle_stream_message(message),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.stream_rx = None;
                    self.busy = false;
                    self.active_assistant = None;
                    self.session_stats_dirty = true;
                    break;
                }
            }
        }

        if self.session_stats_dirty {
            self.reload_session_stats().await;
            self.session_stats_dirty = false;
        }
    }

    async fn reload_session_stats(&mut self) {
        self.session_stats = crate::load_session_stats(&self.runtime, &self.session_id).await;
    }

    pub async fn on_key(&mut self, key: KeyEvent) -> Result<()> {
        if key.modifiers == KeyModifiers::NONE {
            match key.code {
                KeyCode::Char('\u{0002}') => {
                    self.move_cursor_left();
                    return Ok(());
                }
                KeyCode::Char('\u{0006}') => {
                    self.move_cursor_right();
                    return Ok(());
                }
                KeyCode::Char('\u{0010}') => {
                    if self.should_use_multiline_navigation() {
                        self.move_cursor_up();
                    } else {
                        self.history_up();
                    }
                    return Ok(());
                }
                KeyCode::Char('\u{000e}') => {
                    if self.should_use_multiline_navigation() {
                        self.move_cursor_down();
                    } else {
                        self.history_down();
                    }
                    return Ok(());
                }
                _ => {}
            }
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') | KeyCode::Char('d') => {
                    self.should_quit = true;
                    return Ok(());
                }
                KeyCode::Char('l') => {
                    self.logs.clear();
                    self.active_assistant = None;
                    self.transcript_offset_from_bottom = 0;
                    return Ok(());
                }
                KeyCode::Char('n') => {
                    if self.busy {
                        self.push_log(
                            LogKind::Error,
                            "assistant is still running, wait for completion before creating a new session"
                                .to_string(),
                        );
                    } else {
                        self.switch_to_new_session().await;
                    }
                    return Ok(());
                }
                KeyCode::Char('j') => {
                    if !self.shortcuts_visible && self.config_wizard.is_none() {
                        self.insert_char_at_cursor('\n');
                        return Ok(());
                    }
                }
                KeyCode::Char('a') => {
                    self.move_cursor_to_line_start_with_wrap(true);
                    return Ok(());
                }
                KeyCode::Char('e') => {
                    self.move_cursor_to_line_end_with_wrap(true);
                    return Ok(());
                }
                KeyCode::Char('b') => {
                    self.move_cursor_left();
                    return Ok(());
                }
                KeyCode::Char('f') => {
                    self.move_cursor_right();
                    return Ok(());
                }
                KeyCode::Char('h') => {
                    self.backspace_at_cursor();
                    return Ok(());
                }
                KeyCode::Char('p') => {
                    if self.should_use_multiline_navigation() {
                        self.move_cursor_up();
                    } else {
                        self.history_up();
                    }
                    return Ok(());
                }
                KeyCode::Char('w') => {
                    self.delete_word_left();
                    return Ok(());
                }
                KeyCode::Char('u') => {
                    self.delete_to_line_start();
                    return Ok(());
                }
                KeyCode::Char('k') => {
                    self.delete_to_line_end();
                    return Ok(());
                }
                _ => {}
            }
        }

        if key.modifiers.contains(KeyModifiers::ALT) {
            match key.code {
                KeyCode::Char('b') => {
                    self.move_cursor_word_left();
                    return Ok(());
                }
                KeyCode::Char('f') => {
                    self.move_cursor_word_right();
                    return Ok(());
                }
                KeyCode::Left => {
                    self.move_cursor_word_left();
                    return Ok(());
                }
                KeyCode::Right => {
                    self.move_cursor_word_right();
                    return Ok(());
                }
                KeyCode::Backspace => {
                    self.delete_word_left();
                    return Ok(());
                }
                KeyCode::Delete => {
                    self.delete_word_right();
                    return Ok(());
                }
                _ => {}
            }
        }

        if self.shortcuts_visible {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('?')) {
                self.shortcuts_visible = false;
            }
            return Ok(());
        }

        match key.code {
            KeyCode::Esc => {
                self.input.clear();
                self.input_cursor = 0;
                self.history_cursor = None;
            }
            KeyCode::Enter => {
                if self.config_wizard.is_none()
                    && key
                        .modifiers
                        .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT)
                {
                    self.insert_char_at_cursor('\n');
                    return Ok(());
                }

                let raw_line = std::mem::take(&mut self.input);
                self.input_cursor = 0;
                self.history_cursor = None;
                if self.config_wizard.is_some() || !raw_line.trim().is_empty() {
                    self.submit_line(raw_line).await?;
                }
            }
            KeyCode::Backspace => {
                self.backspace_at_cursor();
            }
            KeyCode::Delete => {
                self.delete_at_cursor();
            }
            KeyCode::Left => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.move_cursor_word_left();
                } else {
                    self.move_cursor_left();
                }
            }
            KeyCode::Right => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.move_cursor_word_right();
                } else {
                    self.move_cursor_right();
                }
            }
            KeyCode::Tab => {
                self.apply_first_suggestion();
            }
            KeyCode::PageUp => {
                self.scroll_transcript_up(8);
            }
            KeyCode::PageDown => {
                self.scroll_transcript_down(8);
            }
            KeyCode::Home => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.scroll_transcript_to_top();
                } else {
                    self.move_cursor_to_line_start_with_wrap(false);
                }
            }
            KeyCode::End => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.scroll_transcript_to_bottom();
                } else {
                    self.move_cursor_to_line_end_with_wrap(false);
                }
            }
            KeyCode::Up => {
                if self.should_use_multiline_navigation() {
                    self.move_cursor_up();
                } else {
                    self.history_up();
                }
            }
            KeyCode::Down => {
                if self.should_use_multiline_navigation() {
                    self.move_cursor_down();
                } else {
                    self.history_down();
                }
            }
            KeyCode::Char('?') => {
                if self.input.trim().is_empty() && self.config_wizard.is_none() {
                    self.shortcuts_visible = true;
                } else {
                    self.insert_char_at_cursor('?');
                }
            }
            KeyCode::Char(ch) => {
                if matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT)
                    || is_altgr(key.modifiers)
                {
                    self.insert_char_at_cursor(ch);
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub fn on_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp => self.scroll_transcript_up(3),
            MouseEventKind::ScrollDown => self.scroll_transcript_down(3),
            _ => {}
        }
    }

    pub async fn submit_line(&mut self, line: String) -> Result<()> {
        if self.config_wizard.is_some() {
            return self.handle_config_wizard_input(line.trim()).await;
        }
        self.shortcuts_visible = false;

        let prompt = line.trim_end().to_string();
        if prompt.trim().is_empty() {
            return Ok(());
        }

        self.push_history(prompt.trim());

        if prompt.trim_start().starts_with('/') {
            return self.handle_slash_command(prompt.trim().to_string()).await;
        }

        if self.busy {
            self.push_log(
                LogKind::Error,
                "assistant is still running, wait for completion before sending a new prompt"
                    .to_string(),
            );
            return Ok(());
        }

        self.last_usage = None;
        self.stream_saw_output = false;
        self.stream_saw_final = false;
        self.push_log(LogKind::User, prompt.clone());
        self.busy = true;
        self.active_assistant = None;

        let request =
            crate::build_wunder_request(&self.runtime, &self.global, &prompt, &self.session_id)
                .await;
        let orchestrator = self.runtime.state.orchestrator.clone();
        let (tx, rx) = mpsc::unbounded_channel::<StreamMessage>();
        self.stream_rx = Some(rx);

        tokio::spawn(async move {
            match orchestrator.stream(request).await {
                Ok(mut stream) => {
                    while let Some(item) = stream.next().await {
                        let event = item.expect("infallible stream event");
                        if tx.send(StreamMessage::Event(event)).is_err() {
                            return;
                        }
                    }
                }
                Err(err) => {
                    let _ = tx.send(StreamMessage::Error(err.to_string()));
                }
            }
            let _ = tx.send(StreamMessage::Done);
        });

        Ok(())
    }

    fn apply_first_suggestion(&mut self) {
        let trimmed = self.input.trim_start();
        if !trimmed.starts_with('/') {
            return;
        }
        let body = trimmed.trim_start_matches('/');
        if body.contains(char::is_whitespace) {
            return;
        }
        if let Some(suggestion) = slash_command::first_command_completion(body) {
            self.input = format!("/{suggestion} ");
            self.input_cursor = self.input.len();
        }
    }

    fn should_use_multiline_navigation(&self) -> bool {
        if self.config_wizard.is_some() {
            return false;
        }
        let lines =
            build_wrapped_input_lines(&self.input, usize::from(self.input_viewport_width.max(1)));
        lines.len() > 1
    }

    fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        if self.history_cursor.is_none() {
            self.history_draft = self.input.clone();
            self.history_cursor = Some(self.history.len().saturating_sub(1));
        } else if let Some(cursor) = self.history_cursor {
            self.history_cursor = Some(cursor.saturating_sub(1));
        }
        if let Some(cursor) = self.history_cursor {
            self.input = self.history.get(cursor).cloned().unwrap_or_default();
            self.input_cursor = self.input.len();
        }
    }

    fn history_down(&mut self) {
        let Some(cursor) = self.history_cursor else {
            return;
        };
        let next = cursor + 1;
        if next >= self.history.len() {
            self.history_cursor = None;
            self.input = self.history_draft.clone();
            self.input_cursor = self.input.len();
            return;
        }
        self.history_cursor = Some(next);
        self.input = self.history.get(next).cloned().unwrap_or_default();
        self.input_cursor = self.input.len();
    }

    fn push_history(&mut self, value: &str) {
        if self
            .history
            .last()
            .map(|existing| existing == value)
            .unwrap_or(false)
        {
            return;
        }
        self.history.push(value.to_string());
    }

    fn insert_char_at_cursor(&mut self, ch: char) {
        if self.input_cursor > self.input.len() {
            self.input_cursor = self.input.len();
        }
        if !self.input.is_char_boundary(self.input_cursor) {
            self.input_cursor = prev_char_boundary(&self.input, self.input_cursor);
        }
        self.input.insert(self.input_cursor, ch);
        self.input_cursor += ch.len_utf8();
    }

    fn backspace_at_cursor(&mut self) {
        if self.input_cursor == 0 {
            return;
        }
        let prev = prev_char_boundary(&self.input, self.input_cursor);
        self.input.replace_range(prev..self.input_cursor, "");
        self.input_cursor = prev;
    }

    fn delete_at_cursor(&mut self) {
        if self.input_cursor >= self.input.len() {
            return;
        }
        let next = next_char_boundary(&self.input, self.input_cursor);
        self.input.replace_range(self.input_cursor..next, "");
    }

    fn move_cursor_left(&mut self) {
        if self.input_cursor == 0 {
            return;
        }
        self.input_cursor = prev_char_boundary(&self.input, self.input_cursor);
    }

    fn move_cursor_right(&mut self) {
        if self.input_cursor >= self.input.len() {
            return;
        }
        self.input_cursor = next_char_boundary(&self.input, self.input_cursor);
    }

    fn line_start_index(&self) -> usize {
        self.input
            .get(..self.input_cursor)
            .and_then(|value| value.rfind('\n').map(|idx| idx + 1))
            .unwrap_or(0)
    }

    fn line_end_index(&self) -> usize {
        self.input
            .get(self.input_cursor..)
            .and_then(|value| value.find('\n').map(|offset| self.input_cursor + offset))
            .unwrap_or(self.input.len())
    }

    fn move_cursor_to_line_start_with_wrap(&mut self, move_up_at_bol: bool) {
        let line_start = self.line_start_index();
        if move_up_at_bol && self.input_cursor == line_start && line_start > 0 {
            let prev_end = line_start.saturating_sub(1);
            let prev_start = self
                .input
                .get(..prev_end)
                .and_then(|value| value.rfind('\n').map(|idx| idx + 1))
                .unwrap_or(0);
            self.input_cursor = prev_start;
        } else {
            self.input_cursor = line_start;
        }
    }

    fn move_cursor_to_line_end_with_wrap(&mut self, move_down_at_eol: bool) {
        let line_end = self.line_end_index();
        if move_down_at_eol && self.input_cursor == line_end && line_end < self.input.len() {
            let next_start = line_end + 1;
            let next_end = self
                .input
                .get(next_start..)
                .and_then(|value| value.find('\n').map(|offset| next_start + offset))
                .unwrap_or(self.input.len());
            self.input_cursor = next_end;
        } else {
            self.input_cursor = line_end;
        }
    }

    fn move_cursor_word_left(&mut self) {
        let mut cursor = self.input_cursor.min(self.input.len());
        if cursor == 0 {
            return;
        }

        while cursor > 0 {
            let prev = prev_char_boundary(&self.input, cursor);
            let ch = self.input[prev..cursor].chars().next().unwrap_or(' ');
            if !ch.is_whitespace() {
                break;
            }
            cursor = prev;
        }
        if cursor == 0 {
            self.input_cursor = 0;
            return;
        }

        let prev = prev_char_boundary(&self.input, cursor);
        let ch = self.input[prev..cursor].chars().next().unwrap_or(' ');
        let word = is_word_char(ch);
        cursor = prev;

        while cursor > 0 {
            let before = prev_char_boundary(&self.input, cursor);
            let current = self.input[before..cursor].chars().next().unwrap_or(' ');
            if current.is_whitespace() || is_word_char(current) != word {
                break;
            }
            cursor = before;
        }

        self.input_cursor = cursor;
    }

    fn move_cursor_word_right(&mut self) {
        let mut cursor = self.input_cursor.min(self.input.len());
        let len = self.input.len();
        if cursor >= len {
            return;
        }

        while cursor < len {
            let next = next_char_boundary(&self.input, cursor);
            let ch = self.input[cursor..next].chars().next().unwrap_or(' ');
            if !ch.is_whitespace() {
                break;
            }
            cursor = next;
        }
        if cursor >= len {
            self.input_cursor = len;
            return;
        }

        let next = next_char_boundary(&self.input, cursor);
        let ch = self.input[cursor..next].chars().next().unwrap_or(' ');
        let word = is_word_char(ch);
        cursor = next;

        while cursor < len {
            let next = next_char_boundary(&self.input, cursor);
            let current = self.input[cursor..next].chars().next().unwrap_or(' ');
            if current.is_whitespace() || is_word_char(current) != word {
                break;
            }
            cursor = next;
        }

        self.input_cursor = cursor;
    }

    fn delete_word_left(&mut self) {
        let end = self.input_cursor.min(self.input.len());
        if end == 0 {
            return;
        }

        let mut start = end;
        while start > 0 {
            let prev = prev_char_boundary(&self.input, start);
            let ch = self.input[prev..start].chars().next().unwrap_or(' ');
            if !ch.is_whitespace() {
                break;
            }
            start = prev;
        }

        if start == 0 {
            self.input.replace_range(0..end, "");
            self.input_cursor = 0;
            return;
        }

        let prev = prev_char_boundary(&self.input, start);
        let ch = self.input[prev..start].chars().next().unwrap_or(' ');
        let word = is_word_char(ch);
        start = prev;

        while start > 0 {
            let before = prev_char_boundary(&self.input, start);
            let current = self.input[before..start].chars().next().unwrap_or(' ');
            if current.is_whitespace() || is_word_char(current) != word {
                break;
            }
            start = before;
        }

        self.input.replace_range(start..end, "");
        self.input_cursor = start;
    }

    fn delete_word_right(&mut self) {
        let start = self.input_cursor.min(self.input.len());
        let len = self.input.len();
        if start >= len {
            return;
        }

        let mut end = start;
        while end < len {
            let next = next_char_boundary(&self.input, end);
            let ch = self.input[end..next].chars().next().unwrap_or(' ');
            if !ch.is_whitespace() {
                break;
            }
            end = next;
        }

        if end >= len {
            self.input.replace_range(start..len, "");
            self.input_cursor = start;
            return;
        }

        let next = next_char_boundary(&self.input, end);
        let ch = self.input[end..next].chars().next().unwrap_or(' ');
        let word = is_word_char(ch);
        end = next;

        while end < len {
            let next = next_char_boundary(&self.input, end);
            let current = self.input[end..next].chars().next().unwrap_or(' ');
            if current.is_whitespace() || is_word_char(current) != word {
                break;
            }
            end = next;
        }

        self.input.replace_range(start..end, "");
        self.input_cursor = start;
    }

    fn delete_to_line_start(&mut self) {
        let end = self.input_cursor.min(self.input.len());
        let start = self.line_start_index();
        if start < end {
            self.input.replace_range(start..end, "");
            self.input_cursor = start;
        }
    }

    fn delete_to_line_end(&mut self) {
        let start = self.input_cursor.min(self.input.len());
        let end = self.line_end_index();
        if start < end {
            self.input.replace_range(start..end, "");
        }
    }

    fn move_cursor_up(&mut self) {
        self.input_cursor = move_cursor_vertical(
            &self.input,
            usize::from(self.input_viewport_width.max(1)),
            self.input_cursor,
            -1,
        );
    }

    fn move_cursor_down(&mut self) {
        self.input_cursor = move_cursor_vertical(
            &self.input,
            usize::from(self.input_viewport_width.max(1)),
            self.input_cursor,
            1,
        );
    }

    fn handle_stream_message(&mut self, message: StreamMessage) {
        match message {
            StreamMessage::Event(event) => self.apply_stream_event(event),
            StreamMessage::Error(err) => {
                self.push_log(LogKind::Error, err);
                self.busy = false;
                self.active_assistant = None;
                self.stream_rx = None;
                self.stream_saw_output = false;
                self.stream_saw_final = false;
                self.session_stats_dirty = true;
            }
            StreamMessage::Done => {
                if !self.stream_saw_output && !self.stream_saw_final {
                    self.push_log(
                        LogKind::Error,
                        "stream ended without model output or final answer".to_string(),
                    );
                }
                self.busy = false;
                self.active_assistant = None;
                self.stream_rx = None;
                self.stream_saw_output = false;
                self.stream_saw_final = false;
                self.session_stats_dirty = true;
            }
        }
    }

    fn apply_stream_event(&mut self, event: StreamEvent) {
        let payload = event_payload(&event.data);
        match event.event.as_str() {
            "llm_output_delta" => {
                if let Some(delta) = payload.get("delta").and_then(Value::as_str) {
                    if !delta.is_empty() {
                        self.stream_saw_output = true;
                        let index = self.ensure_assistant_entry();
                        if let Some(entry) = self.logs.get_mut(index) {
                            entry.text.push_str(delta);
                        }
                    }
                }
            }
            "llm_output" => {
                if let Some(content) = payload.get("content").and_then(Value::as_str) {
                    if !content.is_empty() {
                        self.stream_saw_output = true;
                        let index = self.ensure_assistant_entry();
                        if let Some(entry) = self.logs.get_mut(index) {
                            if entry.text.is_empty() {
                                entry.text = content.to_string();
                            }
                        }
                    }
                }
            }
            "progress" => {
                let stage = payload
                    .get("stage")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let summary = payload
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let message = format!("[progress] {stage} {summary}").trim().to_string();
                if !message.is_empty() {
                    self.push_log(LogKind::Info, message);
                }
            }
            "llm_request" => {
                self.session_stats.model_calls = self.session_stats.model_calls.saturating_add(1);
            }
            "context_usage" => {
                if let Some(tokens) = payload.get("context_tokens").and_then(Value::as_i64) {
                    let normalized = tokens.max(0);
                    self.session_stats.context_used_tokens = normalized;
                    self.session_stats.context_peak_tokens =
                        self.session_stats.context_peak_tokens.max(normalized);
                }
            }
            "token_usage" => {
                let input_tokens = payload
                    .get("input_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let output_tokens = payload
                    .get("output_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let total_tokens = payload
                    .get("total_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(input_tokens.saturating_add(output_tokens));
                self.session_stats.total_input_tokens = self
                    .session_stats
                    .total_input_tokens
                    .saturating_add(input_tokens);
                self.session_stats.total_output_tokens = self
                    .session_stats
                    .total_output_tokens
                    .saturating_add(output_tokens);
                self.session_stats.total_tokens =
                    self.session_stats.total_tokens.saturating_add(total_tokens);
                if total_tokens > 0 {
                    self.last_usage = Some(total_tokens.to_string());
                }
            }
            "tool_call" => {
                self.session_stats.tool_calls = self.session_stats.tool_calls.saturating_add(1);
                let tool = payload
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let args = payload
                    .get("args")
                    .map(compact_json)
                    .unwrap_or_else(|| "{}".to_string());
                self.push_log(LogKind::Tool, format!("[tool_call] {tool} {args}"));
            }
            "tool_result" => {
                self.session_stats.tool_results = self.session_stats.tool_results.saturating_add(1);
                let tool = payload
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let result = payload
                    .get("result")
                    .map(compact_json)
                    .unwrap_or_else(|| compact_json(payload));
                self.push_log(LogKind::Tool, format!("[tool_result] {tool} {result}"));
            }
            "error" => {
                self.push_log(
                    LogKind::Error,
                    format!("[error] {}", parse_error_message(payload)),
                );
            }
            "final" => {
                self.stream_saw_final = true;
                let answer = payload
                    .get("answer")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !answer.is_empty() {
                    self.stream_saw_output = true;
                    let index = self.ensure_assistant_entry();
                    if let Some(entry) = self.logs.get_mut(index) {
                        if entry.text.trim().is_empty() {
                            entry.text = answer.to_string();
                        }
                    }
                }
                self.last_usage = payload
                    .get("usage")
                    .and_then(|value| value.get("total_tokens"))
                    .and_then(Value::as_u64)
                    .map(|total| total.to_string());
            }
            _ => {}
        }
    }

    async fn handle_slash_command(&mut self, line: String) -> Result<()> {
        let Some(command) = slash_command::parse_slash_command(&line) else {
            self.push_log(LogKind::Error, format!("unknown command: {line}"));
            self.push_log(
                LogKind::Info,
                "type /help to list available slash commands".to_string(),
            );
            return Ok(());
        };

        match command.command {
            SlashCommand::Help => {
                for help in slash_command::help_lines() {
                    self.push_log(LogKind::Info, help);
                }
            }
            SlashCommand::Status => {
                for line in self.status_lines() {
                    self.push_log(LogKind::Info, line);
                }
            }
            SlashCommand::Session => {
                self.reload_session_stats().await;
                for line in self.session_stats_lines() {
                    self.push_log(LogKind::Info, line);
                }
            }
            SlashCommand::System => {
                self.handle_system_slash(command.args).await?;
            }
            SlashCommand::New => {
                if self.busy {
                    self.push_log(
                        LogKind::Error,
                        "assistant is still running, wait for completion before creating a new session"
                            .to_string(),
                    );
                } else {
                    self.switch_to_new_session().await;
                }
            }
            SlashCommand::Config => {
                self.apply_config_from_slash(command).await?;
            }
            SlashCommand::ConfigShow => {
                self.show_config_snapshot().await?;
            }
            SlashCommand::Model => {
                self.handle_model_slash(command.args).await?;
            }
            SlashCommand::ToolCallMode => {
                self.handle_tool_call_mode_slash(command.args).await?;
            }
            SlashCommand::Exit | SlashCommand::Quit => {
                self.should_quit = true;
            }
        }

        Ok(())
    }

    async fn show_config_snapshot(&mut self) -> Result<()> {
        let config = self.runtime.state.config_store.get().await;
        let model = self
            .runtime
            .resolve_model_name(self.global.model.as_deref())
            .await;
        let model_entry = model.as_ref().and_then(|name| config.llm.models.get(name));
        let tool_call_mode = model_entry
            .and_then(|model| model.tool_call_mode.clone())
            .unwrap_or_else(|| "tool_call".to_string());
        let max_context = model_entry
            .and_then(|model| model.max_context)
            .filter(|value| *value > 0);

        self.reload_session_stats().await;

        let payload = json!({
            "launch_dir": self.runtime.launch_dir,
            "temp_root": self.runtime.temp_root,
            "user_id": self.runtime.user_id,
            "workspace_root": config.workspace.root,
            "storage_backend": config.storage.backend,
            "db_path": config.storage.db_path,
            "model": model,
            "tool_call_mode": tool_call_mode,
            "max_rounds": self.model_max_rounds,
            "max_context": max_context,
            "context_used": self.session_stats.context_used_tokens.max(0),
            "context_left_percent": crate::context_left_percent(
                self.session_stats.context_used_tokens,
                max_context,
            ),
            "override_path": self.runtime.temp_root.join("config/wunder.override.yaml"),
        });

        for line in serde_json::to_string_pretty(&payload)?.lines() {
            self.push_log(LogKind::Info, line.to_string());
        }
        Ok(())
    }

    async fn apply_config_from_slash(&mut self, command: ParsedSlashCommand<'_>) -> Result<()> {
        let args = command.args.trim();
        if args.is_empty() {
            self.start_config_wizard();
            return Ok(());
        }

        let values =
            shell_words::split(args).map_err(|err| anyhow!("parse /config args failed: {err}"))?;
        if values.len() < 3 || values.len() > 4 {
            self.push_log(
                LogKind::Error,
                "invalid /config args, expected: /config <base_url> <api_key> <model> [max_context]"
                    .to_string(),
            );
            return Ok(());
        }

        let base_url = values[0].trim().to_string();
        let api_key = values[1].trim().to_string();
        let model_name = values[2].trim().to_string();
        if base_url.is_empty() || api_key.is_empty() || model_name.is_empty() {
            self.push_log(LogKind::Error, "config values cannot be empty".to_string());
            return Ok(());
        }

        let manual_max_context = if values.len() == 4 {
            match crate::parse_optional_max_context_value(values[3].as_str()) {
                Ok(value) => value,
                Err(err) => {
                    self.push_log(LogKind::Error, err.to_string());
                    return Ok(());
                }
            }
        } else {
            None
        };

        self.apply_model_config(base_url, api_key, model_name, manual_max_context)
            .await
    }

    fn start_config_wizard(&mut self) {
        self.config_wizard = Some(ConfigWizardState::default());
        self.push_log(LogKind::Info, "configure llm model (step 1/4)".to_string());
        self.push_log(
            LogKind::Info,
            "input base_url (empty line to cancel)".to_string(),
        );
    }

    async fn handle_config_wizard_input(&mut self, input: &str) -> Result<()> {
        let cleaned = input.trim();
        if cleaned.eq_ignore_ascii_case("/cancel") || cleaned.eq_ignore_ascii_case("/exit") {
            self.config_wizard = None;
            self.push_log(LogKind::Info, "config cancelled".to_string());
            return Ok(());
        }

        let Some(mut wizard) = self.config_wizard.take() else {
            return Ok(());
        };

        if wizard.base_url.is_none() {
            if cleaned.is_empty() {
                self.push_log(LogKind::Info, "config cancelled".to_string());
                return Ok(());
            }
            wizard.base_url = Some(cleaned.to_string());
            self.config_wizard = Some(wizard);
            self.push_log(LogKind::Info, "input api_key (step 2/4)".to_string());
            return Ok(());
        }

        if wizard.api_key.is_none() {
            if cleaned.is_empty() {
                self.push_log(LogKind::Info, "config cancelled".to_string());
                return Ok(());
            }
            wizard.api_key = Some(cleaned.to_string());
            self.config_wizard = Some(wizard);
            self.push_log(LogKind::Info, "input model name (step 3/4)".to_string());
            return Ok(());
        }

        if wizard.model_name.is_none() {
            if cleaned.is_empty() {
                self.push_log(LogKind::Info, "config cancelled".to_string());
                return Ok(());
            }
            wizard.model_name = Some(cleaned.to_string());
            self.config_wizard = Some(wizard);
            self.push_log(
                LogKind::Info,
                "input max_context (step 4/4, optional; Enter for auto probe)".to_string(),
            );
            return Ok(());
        }

        let manual_max_context = match crate::parse_optional_max_context_value(cleaned) {
            Ok(value) => value,
            Err(err) => {
                self.push_log(LogKind::Error, err.to_string());
                self.config_wizard = Some(wizard);
                self.push_log(
                    LogKind::Info,
                    "input max_context (step 4/4, optional; Enter for auto probe)".to_string(),
                );
                return Ok(());
            }
        };

        let base_url = wizard.base_url.unwrap_or_default();
        let api_key = wizard.api_key.unwrap_or_default();
        let model_name = wizard.model_name.unwrap_or_default();
        self.apply_model_config(base_url, api_key, model_name, manual_max_context)
            .await
    }

    async fn apply_model_config(
        &mut self,
        base_url: String,
        api_key: String,
        model_name: String,
        manual_max_context: Option<u32>,
    ) -> Result<()> {
        self.config_wizard = None;
        let (provider, resolved_max_context) = crate::apply_cli_model_config(
            &self.runtime,
            &base_url,
            &api_key,
            &model_name,
            manual_max_context,
        )
        .await?;

        self.sync_model_status().await;
        self.reload_session_stats().await;
        self.push_log(LogKind::Info, "model configured".to_string());
        self.push_log(LogKind::Info, format!("- provider: {provider}"));
        self.push_log(LogKind::Info, format!("- base_url: {base_url}"));
        self.push_log(LogKind::Info, format!("- model: {model_name}"));
        if let Some(value) = resolved_max_context {
            self.push_log(LogKind::Info, format!("- max_context: {value}"));
        } else {
            self.push_log(
                LogKind::Info,
                "- max_context: auto probe unavailable (or keep existing)".to_string(),
            );
        }
        self.push_log(LogKind::Info, "- tool_call_mode: tool_call".to_string());
        Ok(())
    }

    async fn handle_model_slash(&mut self, args: &str) -> Result<()> {
        let target = args.trim();
        if target.is_empty() {
            self.show_model_status().await;
            return Ok(());
        }

        let config = self.runtime.state.config_store.get().await;
        if !config.llm.models.contains_key(target) {
            self.push_log(LogKind::Error, format!("model not found: {target}"));
            let models = crate::sorted_model_names(&config);
            if models.is_empty() {
                self.push_log(
                    LogKind::Info,
                    "no models configured. run /config first.".to_string(),
                );
            } else {
                self.push_log(
                    LogKind::Info,
                    format!("available models: {}", models.join(", ")),
                );
            }
            return Ok(());
        }

        let target_name = target.to_string();
        self.runtime
            .state
            .config_store
            .update(move |config| {
                config.llm.default = target_name.clone();
            })
            .await?;

        self.sync_model_status().await;
        self.push_log(LogKind::Info, format!("model set: {target}"));
        self.show_model_status().await;
        Ok(())
    }

    async fn show_model_status(&mut self) {
        let config = self.runtime.state.config_store.get().await;
        let active_model = self
            .runtime
            .resolve_model_name(self.global.model.as_deref())
            .await
            .unwrap_or_else(|| "<none>".to_string());
        self.push_log(LogKind::Info, format!("current model: {active_model}"));

        let models = crate::sorted_model_names(&config);
        if models.is_empty() {
            self.push_log(
                LogKind::Info,
                "no models configured. run /config first.".to_string(),
            );
            return;
        }

        self.push_log(LogKind::Info, "available models:".to_string());
        for name in models {
            let marker = if name == active_model { "*" } else { " " };
            let mode = config
                .llm
                .models
                .get(&name)
                .and_then(|model| model.tool_call_mode.as_deref())
                .unwrap_or("tool_call");
            self.push_log(LogKind::Info, format!("{marker} {name} ({mode})"));
        }
    }

    async fn handle_tool_call_mode_slash(&mut self, args: &str) -> Result<()> {
        let cleaned = args.trim();
        if cleaned.is_empty() {
            self.push_log(
                LogKind::Info,
                format!(
                    "tool_call_mode: model={} mode={}",
                    self.model_name, self.tool_call_mode
                ),
            );
            self.push_log(
                LogKind::Info,
                "usage: /tool-call-mode <tool_call|function_call> [model]".to_string(),
            );
            return Ok(());
        }

        let mut parts = cleaned.split_whitespace();
        let Some(mode_token) = parts.next() else {
            return Ok(());
        };
        let Some(mode) = crate::parse_tool_call_mode(mode_token) else {
            self.push_log(LogKind::Error, format!("invalid mode: {mode_token}"));
            self.push_log(
                LogKind::Info,
                "valid modes: tool_call, function_call".to_string(),
            );
            return Ok(());
        };

        let model = parts.next().map(str::to_string);
        if parts.next().is_some() {
            self.push_log(LogKind::Error, "too many arguments".to_string());
            self.push_log(
                LogKind::Info,
                "usage: /tool-call-mode <tool_call|function_call> [model]".to_string(),
            );
            return Ok(());
        }

        let config = self.runtime.state.config_store.get().await;
        let target_model = if let Some(model_name) = model {
            if !config.llm.models.contains_key(&model_name) {
                self.push_log(
                    LogKind::Error,
                    format!("model not found in config: {model_name}"),
                );
                return Ok(());
            }
            model_name
        } else {
            self.runtime
                .resolve_model_name(self.global.model.as_deref())
                .await
                .ok_or_else(|| anyhow!("no llm model configured"))?
        };

        let mode_text = mode.as_str().to_string();
        let target_model_for_update = target_model.clone();
        self.runtime
            .state
            .config_store
            .update(move |config| {
                if config.llm.default.trim().is_empty() {
                    config.llm.default = target_model_for_update.clone();
                }
                if let Some(entry) = config.llm.models.get_mut(&target_model_for_update) {
                    entry.tool_call_mode = Some(mode_text.clone());
                }
            })
            .await?;

        self.sync_model_status().await;
        self.push_log(
            LogKind::Info,
            format!(
                "tool_call_mode set: model={target_model} mode={}",
                mode.as_str()
            ),
        );
        Ok(())
    }

    async fn sync_model_status(&mut self) {
        let config = self.runtime.state.config_store.get().await;
        self.model_name = self
            .runtime
            .resolve_model_name(self.global.model.as_deref())
            .await
            .unwrap_or_else(|| "<none>".to_string());
        let model_entry = config.llm.models.get(&self.model_name);
        self.tool_call_mode = model_entry
            .and_then(|model| model.tool_call_mode.clone())
            .unwrap_or_else(|| "tool_call".to_string());
        self.model_max_context = model_entry
            .and_then(|model| model.max_context)
            .filter(|value| *value > 0);
        self.model_max_rounds = model_entry
            .and_then(|model| model.max_rounds)
            .unwrap_or(crate::CLI_MIN_MAX_ROUNDS)
            .max(crate::CLI_MIN_MAX_ROUNDS);
    }

    fn session_stats_lines(&self) -> Vec<String> {
        let mut lines = vec![
            "session".to_string(),
            format!("- id: {}", self.session_id),
            format!("- model: {}", self.model_name),
        ];

        if let Some(total) = self.model_max_context {
            let used = self.session_stats.context_used_tokens.max(0) as u64;
            let left =
                crate::context_left_percent(self.session_stats.context_used_tokens, Some(total))
                    .unwrap_or(0);
            lines.push(format!("- context: {used}/{total} ({left}% left)"));
        } else {
            lines.push(format!(
                "- context: {}/unknown",
                self.session_stats.context_used_tokens.max(0)
            ));
        }

        lines.push(format!("- model_calls: {}", self.session_stats.model_calls));
        lines.push(format!("- tool_calls: {}", self.session_stats.tool_calls));
        lines.push(format!(
            "- tool_results: {}",
            self.session_stats.tool_results
        ));
        lines.push(format!(
            "- token_usage: input={} output={} total={}",
            self.session_stats.total_input_tokens,
            self.session_stats.total_output_tokens,
            self.session_stats.total_tokens
        ));
        lines
    }

    async fn handle_system_slash(&mut self, args: &str) -> Result<()> {
        let cleaned = args.trim();
        if cleaned.eq_ignore_ascii_case("clear") {
            self.runtime.clear_extra_prompt()?;
            self.push_log(LogKind::Info, "extra prompt cleared".to_string());
        } else if let Some(rest) = cleaned.strip_prefix("set ") {
            let prompt = rest.trim();
            if prompt.is_empty() {
                self.push_log(LogKind::Error, "extra prompt is empty".to_string());
                self.push_log(
                    LogKind::Info,
                    "usage: /system [set <extra_prompt>|clear]".to_string(),
                );
                return Ok(());
            }
            self.runtime.save_extra_prompt(prompt)?;
            self.push_log(
                LogKind::Info,
                format!("extra prompt saved ({} chars)", prompt.chars().count()),
            );
        } else if !cleaned.is_empty() && !cleaned.eq_ignore_ascii_case("show") {
            self.push_log(LogKind::Error, "invalid /system args".to_string());
            self.push_log(
                LogKind::Info,
                "usage: /system [set <extra_prompt>|clear]".to_string(),
            );
            return Ok(());
        }

        let prompt = crate::build_current_system_prompt(&self.runtime, &self.global).await?;
        let extra_prompt = self.runtime.load_extra_prompt();
        self.push_log(LogKind::Info, "system".to_string());
        self.push_log(LogKind::Info, format!("- session: {}", self.session_id));
        self.push_log(
            LogKind::Info,
            format!(
                "- extra_prompt: {}",
                extra_prompt
                    .as_ref()
                    .map(|value| format!("enabled ({} chars)", value.chars().count()))
                    .unwrap_or_else(|| "none".to_string())
            ),
        );
        self.push_log(LogKind::Info, "--- system prompt ---".to_string());
        for line in prompt.lines() {
            self.push_log(LogKind::Info, line.to_string());
        }
        self.push_log(LogKind::Info, "--- end system prompt ---".to_string());
        Ok(())
    }

    async fn switch_to_new_session(&mut self) {
        self.session_id = uuid::Uuid::new_v4().simple().to_string();
        self.runtime.save_session(&self.session_id).ok();
        self.input.clear();
        self.input_cursor = 0;
        self.history_cursor = None;
        self.config_wizard = None;
        self.last_usage = None;
        self.session_stats = crate::SessionStatsSnapshot::default();
        self.reload_session_stats().await;
        self.push_log(
            LogKind::Info,
            format!("switched to session: {}", self.session_id),
        );
    }

    fn status_lines(&self) -> Vec<String> {
        vec![
            "status".to_string(),
            format!("- session: {}", self.session_id),
            format!("- model: {}", self.model_name),
            format!("- tool_call_mode: {}", self.tool_call_mode),
            format!("- max_rounds: {}", self.model_max_rounds),
            format!("- workspace: {}", self.runtime.launch_dir.to_string_lossy()),
            format!("- temp_root: {}", self.runtime.temp_root.to_string_lossy()),
        ]
    }

    fn ensure_assistant_entry(&mut self) -> usize {
        if let Some(index) = self.active_assistant {
            return index;
        }
        let index = self.push_log(LogKind::Assistant, String::new());
        self.active_assistant = Some(index);
        index
    }

    fn push_log(&mut self, kind: LogKind, text: String) -> usize {
        self.logs.push(LogEntry { kind, text });
        if self.logs.len() > MAX_LOG_ENTRIES {
            self.logs.remove(0);
            if let Some(index) = self.active_assistant.as_mut() {
                *index = index.saturating_sub(1);
            }
        }
        self.logs.len().saturating_sub(1)
    }
}

fn move_cursor_vertical(text: &str, width: usize, cursor: usize, delta: i8) -> usize {
    let lines = build_wrapped_input_lines(text, width);
    if lines.len() <= 1 {
        return cursor.min(text.len());
    }

    let (row, col) = cursor_visual_position(text, &lines, cursor.min(text.len()));
    let target_row = if delta < 0 {
        row.saturating_sub(1)
    } else {
        (row + 1).min(lines.len().saturating_sub(1))
    };
    if target_row == row {
        return cursor.min(text.len());
    }

    let target = lines[target_row];
    byte_index_for_char_column(text, target.start, target.end, col)
}

fn build_wrapped_input_lines(text: &str, width: usize) -> Vec<WrappedInputLine> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut line_start = 0usize;
    let mut line_columns = 0usize;

    for (index, ch) in text.char_indices() {
        if ch == '\n' {
            lines.push(WrappedInputLine {
                start: line_start,
                end: index,
            });
            line_start = index + ch.len_utf8();
            line_columns = 0;
            continue;
        }

        if line_columns == width {
            lines.push(WrappedInputLine {
                start: line_start,
                end: index,
            });
            line_start = index;
            line_columns = 0;
        }
        line_columns = line_columns.saturating_add(1);
    }

    lines.push(WrappedInputLine {
        start: line_start,
        end: text.len(),
    });
    lines
}

fn cursor_visual_position(
    text: &str,
    lines: &[WrappedInputLine],
    cursor_index: usize,
) -> (usize, usize) {
    let cursor = cursor_index.min(text.len());
    for (row, line) in lines.iter().enumerate() {
        if cursor < line.start {
            continue;
        }
        if cursor <= line.end {
            if cursor == line.end && row + 1 < lines.len() && lines[row + 1].start == cursor {
                continue;
            }
            let col = text[line.start..cursor].chars().count();
            return (row, col);
        }
    }

    let fallback = lines
        .last()
        .copied()
        .unwrap_or(WrappedInputLine { start: 0, end: 0 });
    let col = text[fallback.start..cursor.min(fallback.end)]
        .chars()
        .count();
    (lines.len().saturating_sub(1), col)
}

#[cfg(windows)]
fn is_altgr(modifiers: KeyModifiers) -> bool {
    modifiers.contains(KeyModifiers::ALT) && modifiers.contains(KeyModifiers::CONTROL)
}

#[cfg(not(windows))]
fn is_altgr(_modifiers: KeyModifiers) -> bool {
    false
}

fn prev_char_boundary(text: &str, index: usize) -> usize {
    if index == 0 {
        return 0;
    }
    let mut cursor = index.saturating_sub(1).min(text.len().saturating_sub(1));
    while cursor > 0 && !text.is_char_boundary(cursor) {
        cursor = cursor.saturating_sub(1);
    }
    if text.is_char_boundary(cursor) {
        cursor
    } else {
        0
    }
}

fn next_char_boundary(text: &str, index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    let mut cursor = index.saturating_add(1);
    while cursor < text.len() && !text.is_char_boundary(cursor) {
        cursor += 1;
    }
    cursor.min(text.len())
}

fn byte_index_for_char_column(text: &str, start: usize, end: usize, column: usize) -> usize {
    let mut remaining = column;
    let mut cursor = start;
    for (offset, ch) in text[start..end].char_indices() {
        if remaining == 0 {
            return start + offset;
        }
        remaining = remaining.saturating_sub(1);
        cursor = start + offset + ch.len_utf8();
    }
    if remaining == 0 {
        cursor
    } else {
        end
    }
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn parse_error_message(data: &Value) -> String {
    let payload = event_payload(data);
    let nested_message = payload
        .get("data")
        .and_then(Value::as_object)
        .and_then(|inner| inner.get("message"))
        .and_then(Value::as_str);
    payload
        .as_str()
        .or_else(|| payload.get("message").and_then(Value::as_str))
        .or_else(|| payload.get("detail").and_then(Value::as_str))
        .or_else(|| payload.get("error").and_then(Value::as_str))
        .or(nested_message)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| compact_json(payload))
}

fn event_payload(data: &Value) -> &Value {
    data.get("data").unwrap_or(data)
}

fn compact_json(value: &Value) -> String {
    const MAX_INLINE_JSON_CHARS: usize = 200;
    let mut text = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
    if text.len() > MAX_INLINE_JSON_CHARS {
        text.truncate(MAX_INLINE_JSON_CHARS);
        text.push_str("...");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapped_input_lines_wrap_by_viewport_width() {
        let lines = build_wrapped_input_lines("abcdef", 3);
        assert_eq!(lines.len(), 2);
        assert_eq!((lines[0].start, lines[0].end), (0, 3));
        assert_eq!((lines[1].start, lines[1].end), (3, 6));
    }

    #[test]
    fn cursor_visual_position_prefers_next_wrapped_line_boundary() {
        let text = "abcdef";
        let lines = build_wrapped_input_lines(text, 3);
        assert_eq!(cursor_visual_position(text, &lines, 2), (0, 2));
        assert_eq!(cursor_visual_position(text, &lines, 3), (1, 0));
    }

    #[test]
    fn wrapped_input_lines_keep_explicit_newlines() {
        let text = "a

b";
        let lines = build_wrapped_input_lines(text, 8);
        assert_eq!(lines.len(), 3);
        assert_eq!((lines[0].start, lines[0].end), (0, 1));
        assert_eq!((lines[1].start, lines[1].end), (2, 2));
        assert_eq!((lines[2].start, lines[2].end), (3, 4));
        assert_eq!(cursor_visual_position(text, &lines, 2), (1, 0));
    }

    #[test]
    fn move_cursor_vertical_uses_wrapped_lines_without_newline() {
        let text = "abcdef";
        assert_eq!(move_cursor_vertical(text, 3, 4, -1), 1);
        assert_eq!(move_cursor_vertical(text, 3, 1, 1), 4);
    }

    #[test]
    fn move_cursor_vertical_clamps_to_line_end() {
        let text = "ab
cdef";
        assert_eq!(move_cursor_vertical(text, 16, 5, -1), 2);
        assert_eq!(move_cursor_vertical(text, 16, 1, 1), 4);
    }
}
