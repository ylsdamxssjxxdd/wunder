use anyhow::{anyhow, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use futures::StreamExt;
use serde_json::{json, Value};
use tokio::sync::mpsc::{self, error::TryRecvError, UnboundedReceiver};
use unicode_width::UnicodeWidthChar;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MouseMode {
    Scroll,
    Select,
}

pub struct TuiApp {
    runtime: CliRuntime,
    global: GlobalArgs,
    session_id: String,
    input: String,
    input_cursor: usize,
    input_viewport_width: u16,
    transcript_viewport_width: u16,
    transcript_viewport_height: u16,
    transcript_rendered_lines: u16,
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
    mouse_mode: MouseMode,
    tool_phase_notice_emitted: bool,
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
            transcript_viewport_width: 1,
            transcript_viewport_height: 1,
            transcript_rendered_lines: 0,
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
            mouse_mode: MouseMode::Scroll,
            tool_phase_notice_emitted: false,
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
        let running_hint = if self.busy { "working..." } else { "shortcuts" };
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
        let mouse_hint = match self.mouse_mode {
            MouseMode::Scroll => " | mouse scroll",
            MouseMode::Select => " | mouse select",
        };
        format!("  {running_hint}{usage_hint}{scroll_hint}{mouse_hint} (F2)    {context_summary}")
    }

    pub fn shortcuts_visible(&self) -> bool {
        self.shortcuts_visible
    }

    pub fn mouse_capture_enabled(&self) -> bool {
        self.mouse_mode == MouseMode::Scroll
    }

    pub fn shortcuts_lines(&self) -> Vec<String> {
        let mouse_mode = match self.mouse_mode {
            MouseMode::Scroll => "scroll",
            MouseMode::Select => "select/copy",
        };
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
            "Shift+Drag            select/copy (terminal bypass, if supported)".to_string(),
            format!("F2                   toggle mouse mode ({mouse_mode})"),
            "Ctrl+N / Ctrl+L       new session / clear transcript".to_string(),
            "Ctrl+C                exit".to_string(),
        ]
    }

    pub fn set_input_viewport(&mut self, viewport_width: u16) {
        self.input_viewport_width = viewport_width.max(1);
    }

    pub fn set_transcript_viewport(&mut self, viewport_width: u16, viewport_height: u16) {
        self.transcript_viewport_width = viewport_width.max(1);
        self.transcript_viewport_height = viewport_height.max(1);
    }

    pub fn set_transcript_rendered_lines(&mut self, rendered_lines: usize) {
        self.transcript_rendered_lines = rendered_lines.min(u16::MAX as usize) as u16;
    }

    fn set_mouse_mode(&mut self, mode: MouseMode) {
        if self.mouse_mode == mode {
            return;
        }
        self.mouse_mode = mode;
        let notice = match mode {
            MouseMode::Scroll => "mouse mode: scroll (wheel enabled)",
            MouseMode::Select => "mouse mode: select/copy (wheel disabled)",
        };
        self.push_log(LogKind::Info, notice.to_string());
    }

    fn toggle_mouse_mode(&mut self) {
        let next = if self.mouse_mode == MouseMode::Scroll {
            MouseMode::Select
        } else {
            MouseMode::Scroll
        };
        self.set_mouse_mode(next);
    }

    pub fn input_view(&self, viewport_width: u16, viewport_height: u16) -> (String, u16, u16) {
        let width = viewport_width.max(1) as usize;
        let height = viewport_height.max(1) as usize;
        let lines = build_wrapped_input_lines(&self.input, width);

        let cursor = self.input_cursor.min(self.input.len());
        let (cursor_row, cursor_col) = normalize_wrapped_cursor_position(
            cursor_visual_position(&self.input, &lines, cursor),
            width,
        );

        let visual_line_count = lines.len().max(cursor_row.saturating_add(1));
        let mut start_line = visual_line_count.saturating_sub(height);
        if cursor_row < start_line {
            start_line = cursor_row;
        }
        if cursor_row >= start_line.saturating_add(height) {
            start_line = cursor_row.saturating_sub(height.saturating_sub(1));
        }

        let end_line = (start_line + height).min(visual_line_count);
        let mut display_lines = Vec::with_capacity(end_line.saturating_sub(start_line));
        for line_index in start_line..end_line {
            if let Some(line) = lines.get(line_index) {
                display_lines.push(self.input[line.start..line.end].to_string());
            } else {
                display_lines.push(String::new());
            }
        }

        let display = display_lines.join("\n");
        let cursor_y = cursor_row.saturating_sub(start_line) as u16;
        let cursor_x = cursor_col as u16;
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
        let max_scroll = self.max_transcript_scroll(self.transcript_viewport_height);
        let current = self.transcript_offset_from_bottom.min(max_scroll);
        self.transcript_offset_from_bottom = current.saturating_add(lines).min(max_scroll);
    }

    fn scroll_transcript_down(&mut self, lines: u16) {
        let max_scroll = self.max_transcript_scroll(self.transcript_viewport_height);
        let current = self.transcript_offset_from_bottom.min(max_scroll);
        self.transcript_offset_from_bottom = current.saturating_sub(lines);
    }

    fn scroll_transcript_to_top(&mut self) {
        self.transcript_offset_from_bottom =
            self.max_transcript_scroll(self.transcript_viewport_height);
    }

    fn scroll_transcript_to_bottom(&mut self) {
        self.transcript_offset_from_bottom = 0;
    }

    fn max_transcript_scroll(&self, viewport_height: u16) -> u16 {
        let viewport = viewport_height.max(1);
        self.total_transcript_lines().saturating_sub(viewport)
    }

    fn total_transcript_lines(&self) -> u16 {
        if self.transcript_rendered_lines > 0 {
            return self.transcript_rendered_lines;
        }

        let width = usize::from(self.transcript_viewport_width.max(1));
        let total = self
            .logs
            .iter()
            .map(|entry| {
                wrapped_visual_line_count(visual_log_text(entry.kind, &entry.text).as_str(), width)
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
            KeyCode::F(2) => {
                self.toggle_mouse_mode();
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
        if self.mouse_mode != MouseMode::Scroll {
            return;
        }
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

        self.scroll_transcript_to_bottom();
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
        self.tool_phase_notice_emitted = false;
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
                self.tool_phase_notice_emitted = false;
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
                self.tool_phase_notice_emitted = false;
                self.session_stats_dirty = true;
            }
        }
    }

    fn apply_stream_event(&mut self, event: StreamEvent) {
        let payload = event_payload(&event.data);
        match event.event.as_str() {
            "llm_output_delta" => {
                if let Some(delta) = payload.get("delta").and_then(Value::as_str) {
                    let cleaned_delta = sanitize_assistant_delta(delta);
                    if !cleaned_delta.is_empty() {
                        self.stream_saw_output = true;
                        let index = self.ensure_assistant_entry();
                        if let Some(entry) = self.logs.get_mut(index) {
                            entry.text.push_str(cleaned_delta.as_str());
                        }
                    }
                }
            }
            "llm_output" => {
                if payload_has_tool_calls(payload) {
                    self.emit_tool_phase_notice();
                    self.active_assistant = None;
                    return;
                }
                if let Some(content) = payload.get("content").and_then(Value::as_str) {
                    let cleaned = sanitize_assistant_text(content);
                    if !cleaned.is_empty() {
                        self.stream_saw_output = true;
                        self.merge_assistant_content(cleaned.as_str());
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
                self.active_assistant = None;
                self.tool_phase_notice_emitted = false;
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
                if !self.tool_phase_notice_emitted {
                    self.emit_tool_phase_notice();
                }
                self.active_assistant = None;
                let tool = payload
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let args = payload.get("args").unwrap_or(&Value::Null);
                self.push_log(LogKind::Tool, format_tool_call_line(tool, args));
            }
            "tool_result" => {
                self.session_stats.tool_results = self.session_stats.tool_results.saturating_add(1);
                let tool = payload
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                for line in format_tool_result_lines(tool, payload) {
                    self.push_log(LogKind::Tool, line);
                }
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
                if !answer.trim().is_empty() {
                    self.stream_saw_output = true;
                    self.merge_final_answer(answer);
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
            SlashCommand::Mouse => {
                self.handle_mouse_slash(command.args);
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

    fn handle_mouse_slash(&mut self, args: &str) {
        let cleaned = args.trim();
        if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("show") {
            let mode = if self.mouse_mode == MouseMode::Scroll {
                "scroll"
            } else {
                "select"
            };
            self.push_log(LogKind::Info, format!("mouse mode: {mode}"));
            self.push_log(
                LogKind::Info,
                "usage: /mouse [scroll|select]  (F2 to toggle)".to_string(),
            );
            return;
        }

        if cleaned.eq_ignore_ascii_case("scroll") {
            self.set_mouse_mode(MouseMode::Scroll);
            return;
        }
        if cleaned.eq_ignore_ascii_case("select")
            || cleaned.eq_ignore_ascii_case("copy")
            || cleaned.eq_ignore_ascii_case("selection")
        {
            self.set_mouse_mode(MouseMode::Select);
            return;
        }

        self.push_log(LogKind::Error, format!("invalid /mouse args: {cleaned}"));
        self.push_log(
            LogKind::Info,
            "usage: /mouse [scroll|select]  (F2 to toggle)".to_string(),
        );
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
            format!(
                "- mouse_mode: {}",
                if self.mouse_mode == MouseMode::Scroll {
                    "scroll"
                } else {
                    "select"
                }
            ),
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

    fn remove_log_entry(&mut self, index: usize) {
        if index >= self.logs.len() {
            return;
        }
        self.logs.remove(index);
        if let Some(active) = self.active_assistant {
            self.active_assistant = if active == index {
                None
            } else if active > index {
                Some(active.saturating_sub(1))
            } else {
                Some(active)
            };
        }
    }

    fn emit_tool_phase_notice(&mut self) {
        if self.tool_phase_notice_emitted {
            return;
        }

        let mut has_meaningful_assistant = false;
        if let Some(index) = self.active_assistant {
            if let Some(entry) = self.logs.get_mut(index) {
                let cleaned = sanitize_assistant_text(entry.text.as_str());
                if !cleaned.is_empty() && !looks_like_tool_payload(cleaned.as_str()) {
                    entry.text = cleaned;
                    has_meaningful_assistant = true;
                }
            }
            if !has_meaningful_assistant {
                self.remove_log_entry(index);
            }
        }

        if !has_meaningful_assistant {
            self.push_log(LogKind::Assistant, "正在调用工具...".to_string());
        }
        self.tool_phase_notice_emitted = true;
    }

    fn merge_assistant_content(&mut self, content: &str) {
        let cleaned = sanitize_assistant_text(content);
        if cleaned.is_empty() {
            return;
        }

        let index = self.ensure_assistant_entry();
        if let Some(entry) = self.logs.get_mut(index) {
            if entry.text.trim().is_empty() {
                entry.text = cleaned.clone();
                return;
            }

            if entry.text == cleaned || entry.text.ends_with(cleaned.as_str()) {
                return;
            }

            if cleaned.starts_with(entry.text.as_str()) {
                entry.text = cleaned;
                return;
            }

            if !entry.text.contains(cleaned.as_str()) {
                if !entry.text.ends_with('\n') {
                    entry.text.push('\n');
                }
                entry.text.push_str(cleaned.as_str());
            }
        }
    }

    fn merge_final_answer(&mut self, answer: &str) {
        let cleaned = sanitize_assistant_text(answer);
        if cleaned.is_empty() {
            return;
        }

        let index = self.ensure_assistant_entry();
        if let Some(entry) = self.logs.get_mut(index) {
            if entry.text.trim().is_empty() {
                entry.text = cleaned.clone();
                return;
            }

            if entry.text.trim() == cleaned || entry.text.ends_with(cleaned.as_str()) {
                return;
            }

            if cleaned.starts_with(entry.text.trim()) {
                entry.text = cleaned;
                return;
            }

            if !entry.text.ends_with("\n\n") {
                if !entry.text.ends_with('\n') {
                    entry.text.push('\n');
                }
                entry.text.push('\n');
            }
            entry.text.push_str(cleaned.as_str());
        }
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

fn visual_log_text(kind: LogKind, text: &str) -> String {
    format!("{}{text}", log_prefix(kind))
}

pub(super) fn log_prefix(kind: LogKind) -> &'static str {
    match kind {
        LogKind::Info => "- ",
        LogKind::User => "you> ",
        LogKind::Assistant => "assistant> ",
        LogKind::Tool => "tool> ",
        LogKind::Error => "error> ",
    }
}

fn wrapped_visual_line_count(text: &str, width: usize) -> usize {
    let width = width.max(1);
    if text.is_empty() {
        return 1;
    }

    let mut line_count = 1usize;
    let mut line_columns = 0usize;
    for ch in text.chars() {
        if ch == '\n' {
            line_count = line_count.saturating_add(1);
            line_columns = 0;
            continue;
        }

        let char_width = display_char_width(ch);
        if line_columns > 0 && line_columns.saturating_add(char_width) > width {
            line_count = line_count.saturating_add(1);
            line_columns = 0;
        }
        line_columns = line_columns.saturating_add(char_width).min(width);
    }

    line_count
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
    byte_index_for_display_column(text, target.start, target.end, col)
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

        let char_width = display_char_width(ch);
        if line_columns > 0 && line_columns.saturating_add(char_width) > width {
            lines.push(WrappedInputLine {
                start: line_start,
                end: index,
            });
            line_start = index;
            line_columns = 0;
        }
        line_columns = line_columns.saturating_add(char_width);
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
            let col = display_width(&text[line.start..cursor]);
            return (row, col);
        }
    }

    let fallback = lines
        .last()
        .copied()
        .unwrap_or(WrappedInputLine { start: 0, end: 0 });
    let col = display_width(&text[fallback.start..cursor.min(fallback.end)]);
    (lines.len().saturating_sub(1), col)
}

fn display_char_width(ch: char) -> usize {
    UnicodeWidthChar::width_cjk(ch)
        .or_else(|| UnicodeWidthChar::width(ch))
        .unwrap_or(0)
        .max(1)
}

fn display_width(text: &str) -> usize {
    text.chars().map(display_char_width).sum()
}

fn normalize_wrapped_cursor_position((row, col): (usize, usize), width: usize) -> (usize, usize) {
    if width == 0 {
        return (row, 0);
    }
    if col < width {
        return (row, col);
    }

    (row.saturating_add(col / width), col % width)
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

fn byte_index_for_display_column(text: &str, start: usize, end: usize, column: usize) -> usize {
    let mut consumed = 0usize;
    let mut cursor = start;

    for (offset, ch) in text[start..end].char_indices() {
        if consumed >= column {
            return start + offset;
        }
        let width = display_char_width(ch);
        consumed = consumed.saturating_add(width);
        cursor = start + offset + ch.len_utf8();
        if consumed >= column {
            return cursor;
        }
    }

    cursor.min(end)
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn payload_has_tool_calls(payload: &Value) -> bool {
    match payload.get("tool_calls") {
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
        Some(Value::String(value)) => !value.trim().is_empty(),
        Some(Value::Null) | None => false,
        Some(_) => true,
    }
}

fn sanitize_assistant_delta(delta: &str) -> String {
    if delta.trim().is_empty() {
        return String::new();
    }
    let cleaned = strip_tool_block_tags(delta);
    let trimmed = cleaned.trim();
    if trimmed.is_empty() || looks_like_tool_payload(trimmed) {
        return String::new();
    }
    cleaned
}

fn sanitize_assistant_text(text: &str) -> String {
    strip_tool_block_tags(text).trim().to_string()
}

fn looks_like_tool_payload(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    lowered.contains("<tool_call")
        || lowered.contains("</tool_call>")
        || lowered.contains("\"name\"") && lowered.contains("\"arguments\"")
}

fn strip_tool_block_tags(text: &str) -> String {
    let without_tool_call = strip_tag_block(text.to_string(), "<tool_call", "</tool_call>");
    strip_tag_block(without_tool_call, "<tool", "</tool>")
}

fn strip_tag_block(mut text: String, start_tag: &str, end_tag: &str) -> String {
    loop {
        let lowered = text.to_ascii_lowercase();
        let Some(start) = lowered.find(start_tag) else {
            break;
        };

        let after_start = start + start_tag.len();
        let Some(close_offset) = lowered[after_start..].find('>') else {
            text.truncate(start);
            break;
        };
        let body_start = after_start + close_offset + 1;

        if let Some(end_offset) = lowered[body_start..].find(end_tag) {
            let end = body_start + end_offset + end_tag.len();
            text.replace_range(start..end, "");
        } else {
            text.truncate(start);
            break;
        }
    }
    text
}

fn format_tool_call_line(tool: &str, args: &Value) -> String {
    if tool == "执行命令" {
        if let Some(command) = args
            .get("content")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return format!("[tool_call] {tool} `{command}`");
        }
    }

    format!("[tool_call] {tool} {}", compact_json(args))
}

fn format_tool_result_lines(tool: &str, payload: &Value) -> Vec<String> {
    let result = payload.get("result").unwrap_or(payload);
    if tool == "执行命令" {
        let lines = format_execute_command_result_lines(tool, result);
        if !lines.is_empty() {
            return lines;
        }
    }

    let ok = result.get("ok").and_then(Value::as_bool);
    let mut headline = format!("[tool_result] {tool}");
    if let Some(ok) = ok {
        headline.push_str(if ok { " ok" } else { " failed" });
    }

    let mut lines = vec![headline];
    if let Some(error) = result
        .get("error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("  error: {error}"));
    }

    let data = result.get("data").unwrap_or(result);
    lines.push(format!("  data: {}", compact_json(data)));
    lines
}

fn format_execute_command_result_lines(tool: &str, result: &Value) -> Vec<String> {
    let data = result.get("data").unwrap_or(result);
    let Some(first) = data
        .get("results")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };

    let command = first
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let returncode = value_as_i64(first.get("returncode"))
        .or_else(|| value_as_i64(result.get("meta").and_then(|meta| meta.get("exit_code"))));
    let duration_ms = value_as_i64(result.get("meta").and_then(|meta| meta.get("duration_ms")));

    let mut header = format!("[tool_result] {tool}");
    if let Some(returncode) = returncode {
        header.push_str(&format!(" exit={returncode}"));
    }
    if let Some(duration_ms) = duration_ms {
        header.push_str(&format!(" {duration_ms}ms"));
    }

    let mut lines = vec![header];
    if !command.is_empty() {
        lines.push(format!("  cmd: {command}"));
    }

    if let Some(stdout) = first.get("stdout").and_then(Value::as_str) {
        append_text_preview(&mut lines, "stdout", stdout, 8, 1200);
    }
    if let Some(stderr) = first.get("stderr").and_then(Value::as_str) {
        append_text_preview(&mut lines, "stderr", stderr, 8, 1200);
    }

    if lines.len() <= 2 {
        lines.push("  output: <empty>".to_string());
    }

    lines
}

fn append_text_preview(
    lines: &mut Vec<String>,
    label: &str,
    text: &str,
    max_lines: usize,
    max_chars: usize,
) {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized.trim_end_matches('\n').trim();
    if trimmed.is_empty() {
        return;
    }

    let (preview, chars_truncated) = truncate_by_chars(trimmed, max_chars);
    let parts = preview.lines().collect::<Vec<_>>();
    if parts.is_empty() {
        return;
    }

    lines.push(format!("  {label}: {}", parts[0]));
    for line in parts.iter().skip(1).take(max_lines.saturating_sub(1)) {
        lines.push(format!("    {line}"));
    }

    let hidden_lines = parts.len().saturating_sub(max_lines);
    if hidden_lines > 0 || chars_truncated {
        let mut suffix = String::new();
        if hidden_lines > 0 {
            suffix.push_str(&format!("{hidden_lines} more lines"));
        }
        if chars_truncated {
            if !suffix.is_empty() {
                suffix.push_str(", ");
            }
            suffix.push_str("truncated");
        }
        lines.push(format!("    ... ({suffix})"));
    }
}

fn truncate_by_chars(text: &str, max_chars: usize) -> (String, bool) {
    if max_chars == 0 {
        return (String::new(), !text.is_empty());
    }

    if text.chars().count() <= max_chars {
        return (text.to_string(), false);
    }

    let mut output = String::new();
    for ch in text.chars().take(max_chars) {
        output.push(ch);
    }
    (output, true)
}

fn value_as_i64(value: Option<&Value>) -> Option<i64> {
    value.and_then(|item| {
        item.as_i64()
            .or_else(|| item.as_u64().map(|num| num.min(i64::MAX as u64) as i64))
            .or_else(|| {
                item.as_str()
                    .and_then(|text| text.trim().parse::<i64>().ok())
            })
    })
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
        let mut safe_boundary = MAX_INLINE_JSON_CHARS;
        while safe_boundary > 0 && !text.is_char_boundary(safe_boundary) {
            safe_boundary = safe_boundary.saturating_sub(1);
        }
        text.truncate(safe_boundary);
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

    #[test]
    fn cursor_visual_position_handles_cjk_width() {
        let text = "\u{4f60}\u{597d}a";
        let lines = build_wrapped_input_lines(text, 8);
        let cursor_after_nihao = "\u{4f60}\u{597d}".len();
        assert_eq!(
            cursor_visual_position(text, &lines, cursor_after_nihao),
            (0, 4)
        );
        assert_eq!(cursor_visual_position(text, &lines, text.len()), (0, 5));
    }

    #[test]
    fn wrapped_input_lines_wrap_cjk_without_splitting_char() {
        let text = "\u{4f60}\u{597d}ab";
        let lines = build_wrapped_input_lines(text, 4);
        assert_eq!(lines.len(), 2);
        assert_eq!(&text[lines[0].start..lines[0].end], "\u{4f60}\u{597d}");
        assert_eq!(&text[lines[1].start..lines[1].end], "ab");
    }

    #[test]
    fn normalize_wrapped_cursor_position_wraps_boundary_columns() {
        assert_eq!(normalize_wrapped_cursor_position((2, 3), 4), (2, 3));
        assert_eq!(normalize_wrapped_cursor_position((2, 4), 4), (3, 0));
        assert_eq!(normalize_wrapped_cursor_position((2, 9), 4), (4, 1));
    }

    #[test]
    fn wrapped_visual_line_count_tracks_wrap_and_newlines() {
        assert_eq!(wrapped_visual_line_count("", 8), 1);
        assert_eq!(wrapped_visual_line_count("abcdef", 3), 2);
        assert_eq!(wrapped_visual_line_count("ab\ncd", 8), 2);
        assert_eq!(wrapped_visual_line_count("\u{4f60}\u{597d}\u{5417}", 4), 2);
    }

    #[test]
    fn sanitize_assistant_text_strips_tool_markup_blocks() {
        let raw = "before <tool_call>{\"name\":\"读取文件\"}</tool_call> after";
        assert_eq!(sanitize_assistant_text(raw), "before  after");
    }

    #[test]
    fn sanitize_assistant_delta_filters_tool_payload_fragments() {
        assert!(sanitize_assistant_delta("<tool_call>{").is_empty());
        assert!(sanitize_assistant_delta("{\"name\":\"读取文件\",\"arguments\":{}}").is_empty());
    }

    #[test]
    fn payload_has_tool_calls_accepts_non_empty_array() {
        let payload = serde_json::json!({ "tool_calls": [{ "name": "读取文件" }] });
        assert!(payload_has_tool_calls(&payload));
    }

    #[test]
    fn compact_json_handles_multibyte_truncation() {
        let value = serde_json::json!({ "message": "你".repeat(400) });
        let output = compact_json(&value);
        assert!(output.ends_with("..."));
    }
}
