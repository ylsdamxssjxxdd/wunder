use anyhow::{anyhow, Result};
use chrono::TimeZone;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use futures::StreamExt;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{self, error::TryRecvError, UnboundedReceiver};
use unicode_width::UnicodeWidthChar;
use wunder_server::approval::{
    new_channel as new_approval_channel, ApprovalRequest, ApprovalRequestRx, ApprovalResponse,
};
use wunder_server::schemas::StreamEvent;

use crate::args::GlobalArgs;
use crate::runtime::CliRuntime;
use crate::slash_command::{self, ParsedSlashCommand, SlashCommand};

const MAX_LOG_ENTRIES: usize = 1200;
const MAX_DRAIN_MESSAGES_PER_TICK: usize = 400;
const CTRL_C_EXIT_WINDOW: Duration = Duration::from_millis(1500);
const MAX_PERSISTED_HISTORY: usize = 200;
const MAX_HISTORY_ENTRY_CHARS: usize = 4000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    Info,
    User,
    Assistant,
    Reasoning,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusArea {
    Input,
    Transcript,
}

#[derive(Debug, Clone)]
struct ResumePickerState {
    sessions: Vec<crate::ResumeSessionSummary>,
    selected: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct InputHistoryStore {
    entries: Vec<String>,
}

#[derive(Debug, Clone)]
struct IndexedFile {
    path: String,
    lowered: String,
}

pub struct TuiApp {
    runtime: CliRuntime,
    global: GlobalArgs,
    display_language: String,
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
    pending_paste: VecDeque<String>,
    workspace_files: Vec<IndexedFile>,
    active_assistant: Option<usize>,
    active_reasoning: Option<usize>,
    stream_rx: Option<UnboundedReceiver<StreamMessage>>,
    approval_rx: Option<ApprovalRequestRx>,
    approval_queue: VecDeque<ApprovalRequest>,
    active_approval: Option<ApprovalRequest>,
    approval_mode: String,
    ctrl_c_hint_deadline: Option<Instant>,
    model_name: String,
    tool_call_mode: String,
    model_max_rounds: u32,
    model_max_context: Option<u32>,
    session_stats: crate::SessionStatsSnapshot,
    last_usage: Option<String>,
    config_wizard: Option<ConfigWizardState>,
    stream_saw_output: bool,
    stream_saw_final: bool,
    stream_received_content_delta: bool,
    stream_tool_markup_open: bool,
    transcript_offset_from_bottom: u16,
    session_stats_dirty: bool,
    shortcuts_visible: bool,
    mouse_mode: MouseMode,
    focus_area: FocusArea,
    transcript_selected: Option<usize>,
    resume_picker: Option<ResumePickerState>,
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
        let display_language = crate::locale::resolve_cli_language(&global);

        let mut app = Self {
            runtime,
            global,
            display_language,
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
            pending_paste: VecDeque::new(),
            workspace_files: Vec::new(),
            active_assistant: None,
            active_reasoning: None,
            stream_rx: None,
            approval_rx: None,
            approval_queue: VecDeque::new(),
            active_approval: None,
            approval_mode: "full_auto".to_string(),
            ctrl_c_hint_deadline: None,
            model_name: "<none>".to_string(),
            tool_call_mode: "tool_call".to_string(),
            model_max_rounds: crate::CLI_MIN_MAX_ROUNDS,
            model_max_context: None,
            session_stats: crate::SessionStatsSnapshot::default(),
            last_usage: None,
            config_wizard: None,
            stream_saw_output: false,
            stream_saw_final: false,
            stream_received_content_delta: false,
            stream_tool_markup_open: false,
            transcript_offset_from_bottom: 0,
            session_stats_dirty: false,
            shortcuts_visible: false,
            mouse_mode: MouseMode::Scroll,
            focus_area: FocusArea::Input,
            transcript_selected: None,
            resume_picker: None,
            tool_phase_notice_emitted: false,
        };
        app.load_persisted_history();
        app.workspace_files = tokio::task::spawn_blocking({
            let root = app.runtime.launch_dir.clone();
            move || build_workspace_file_index(&root)
        })
        .await
        .unwrap_or_default();
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

    pub fn is_zh_language(&self) -> bool {
        crate::locale::is_zh_language(self.display_language.as_str())
    }

    pub fn status_line(&self) -> String {
        let context_summary = if let Some(max_context) = self.model_max_context {
            let percent_left = crate::context_left_percent(
                self.session_stats.context_used_tokens,
                Some(max_context),
            )
            .unwrap_or(0);
            if self.is_zh_language() {
                format!("剩余上下文 {percent_left}%")
            } else {
                format!("{percent_left}% context left")
            }
        } else {
            let used = self.session_stats.context_used_tokens.max(0);
            if self.is_zh_language() {
                format!("已用上下文 {used}")
            } else {
                format!("{used} context used")
            }
        };
        let running_hint = if self.resume_picker.is_some() {
            if self.is_zh_language() {
                "会话恢复面板"
            } else {
                "resume picker"
            }
        } else if self.active_approval.is_some() {
            if self.is_zh_language() {
                "待审批"
            } else {
                "approval pending"
            }
        } else if self.busy {
            if self.is_zh_language() {
                "执行中..."
            } else {
                "working..."
            }
        } else if self.is_zh_language() {
            "快捷键"
        } else {
            "shortcuts"
        };
        let usage_hint = self
            .last_usage
            .as_deref()
            .map(|value| {
                if self.is_zh_language() {
                    format!(" | 最近 tokens {value}")
                } else {
                    format!(" | last tokens {value}")
                }
            })
            .unwrap_or_default();
        let scroll_hint = if self.transcript_offset_from_bottom > 0 {
            if self.is_zh_language() {
                format!(" | 滚动 -{}", self.transcript_offset_from_bottom)
            } else {
                format!(" | scroll -{}", self.transcript_offset_from_bottom)
            }
        } else {
            String::new()
        };
        let mouse_hint = match self.mouse_mode {
            MouseMode::Scroll => {
                if self.is_zh_language() {
                    " | 鼠标滚轮"
                } else {
                    " | mouse scroll"
                }
            }
            MouseMode::Select => {
                if self.is_zh_language() {
                    " | 鼠标选择"
                } else {
                    " | mouse select"
                }
            }
        };
        let focus_hint = match self.focus_area {
            FocusArea::Input => {
                if self.is_zh_language() {
                    " | 输入焦点"
                } else {
                    " | focus input"
                }
            }
            FocusArea::Transcript => {
                if self.is_zh_language() {
                    " | 输出焦点"
                } else {
                    " | focus output"
                }
            }
        };
        format!(
            "  {running_hint}{usage_hint}{scroll_hint}{mouse_hint}{focus_hint} (F2/F3)    {context_summary}"
        )
    }

    pub fn shortcuts_visible(&self) -> bool {
        self.shortcuts_visible
    }

    pub fn mouse_capture_enabled(&self) -> bool {
        self.mouse_mode == MouseMode::Scroll
    }

    fn input_history_file(&self) -> PathBuf {
        let mut name = self
            .runtime
            .user_id
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
            .collect::<String>();
        if name.trim_matches('_').is_empty() {
            name = "cli_user".to_string();
        }
        self.runtime
            .temp_root
            .join(format!("sessions/input_history_{name}.json"))
    }

    fn load_persisted_history(&mut self) {
        let path = self.input_history_file();
        let Ok(text) = fs::read_to_string(path) else {
            return;
        };
        let Ok(store) = serde_json::from_str::<InputHistoryStore>(&text) else {
            return;
        };
        self.history = store
            .entries
            .into_iter()
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect();
        if self.history.len() > MAX_PERSISTED_HISTORY {
            let keep_from = self.history.len().saturating_sub(MAX_PERSISTED_HISTORY);
            self.history = self.history.split_off(keep_from);
        }
    }

    fn persist_history(&self) {
        let path = self.input_history_file();
        let parent = path.parent().map(PathBuf::from);
        if let Some(parent) = parent {
            if fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        let store = InputHistoryStore {
            entries: self.history.clone(),
        };
        let Ok(payload) = serde_json::to_vec_pretty(&store) else {
            return;
        };
        let _ = fs::write(path, payload);
    }

    pub fn selected_transcript_index(&self) -> Option<usize> {
        self.transcript_selected
    }

    pub fn transcript_focus_active(&self) -> bool {
        self.focus_area == FocusArea::Transcript
    }

    pub fn resume_picker_rows(&self) -> Option<(Vec<String>, usize)> {
        let picker = self.resume_picker.as_ref()?;
        let rows = picker
            .sessions
            .iter()
            .map(|session| {
                let current = if session.session_id == self.session_id {
                    "*"
                } else {
                    " "
                };
                let when =
                    format_session_timestamp(session.updated_at.max(session.last_message_at));
                format!(
                    "{current} {}  {}  {}",
                    session.session_id, when, session.title
                )
            })
            .collect::<Vec<_>>();
        Some((rows, picker.selected))
    }

    pub fn approval_modal_lines(&self) -> Option<Vec<String>> {
        let request = self.active_approval.as_ref()?;
        let mut lines = if self.is_zh_language() {
            vec![
                format!("编号: {}", request.id),
                format!("工具: {}", request.tool),
                format!("摘要: {}", request.summary),
                format!("类型: {:?}", request.kind),
            ]
        } else {
            vec![
                format!("id: {}", request.id),
                format!("tool: {}", request.tool),
                format!("summary: {}", request.summary),
                format!("kind: {:?}", request.kind),
            ]
        };

        let detail = compact_json(&request.detail);
        if !detail.trim().is_empty() {
            if self.is_zh_language() {
                lines.push(format!("详情: {detail}"));
            } else {
                lines.push(format!("detail: {detail}"));
            }
        }
        let args = compact_json(&request.args);
        if !args.trim().is_empty() {
            if self.is_zh_language() {
                lines.push(format!("参数: {args}"));
            } else {
                lines.push(format!("args: {args}"));
            }
        }
        lines.push(String::new());
        if self.is_zh_language() {
            lines.push("1/Enter/Y: 仅本次批准".to_string());
            lines.push("2/A: 本会话批准".to_string());
            lines.push("3/N/Esc: 拒绝".to_string());
        } else {
            lines.push("1/Enter/Y: approve once".to_string());
            lines.push("2/A: approve for session".to_string());
            lines.push("3/N/Esc: deny".to_string());
        }
        Some(lines)
    }

    pub fn shortcuts_lines(&self) -> Vec<String> {
        let mouse_mode = match self.mouse_mode {
            MouseMode::Scroll => "scroll",
            MouseMode::Select => "select/copy",
        };
        if self.is_zh_language() {
            let mouse_mode = if self.mouse_mode == MouseMode::Scroll {
                "滚轮"
            } else {
                "选择/复制"
            };
            return vec![
                "Esc / ?               关闭快捷键面板".to_string(),
                "Enter                 发送消息".to_string(),
                "Shift+Enter / Ctrl+J  插入换行".to_string(),
                "Left / Right          光标左右移动".to_string(),
                "Ctrl+B / Ctrl+F       光标左右移动".to_string(),
                "Alt+B / Alt+F         按词移动".to_string(),
                "Alt+Left/Right        按词移动".to_string(),
                "Ctrl+W / Alt+Backspace 删除上一个词".to_string(),
                "Alt+Delete            删除下一个词".to_string(),
                "Ctrl+U / Ctrl+K       删除到行首/行尾".to_string(),
                "Ctrl+A / Ctrl+E       移动到行首/行尾".to_string(),
                "Up / Down             历史消息（多行时为上下移动）".to_string(),
                "F3                   切换输入/输出焦点".to_string(),
                "(输出焦点) arrows     选择会话日志条目".to_string(),
                "Tab                   补全 slash 命令".to_string(),
                "PgUp/PgDn             滚动输出区".to_string(),
                "Mouse Wheel           滚动输出区".to_string(),
                "Shift+Drag            选择/复制（取决于终端）".to_string(),
                format!("F2                   切换鼠标模式 ({mouse_mode})"),
                "Ctrl+N / Ctrl+L       新会话 / 清空输出".to_string(),
                "Ctrl+C                中断 / 双击退出".to_string(),
            ];
        }
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
            "F3                   toggle input/output focus".to_string(),
            "(output focus) arrows select transcript entries".to_string(),
            "Tab                   complete slash command".to_string(),
            "PgUp/PgDn             scroll transcript".to_string(),
            "Mouse Wheel           scroll transcript".to_string(),
            "Shift+Drag            select/copy (terminal bypass, if supported)".to_string(),
            format!("F2                   toggle mouse mode ({mouse_mode})"),
            "Ctrl+N / Ctrl+L       new session / clear transcript".to_string(),
            "Ctrl+C                interrupt / double-tap exit".to_string(),
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

    fn set_focus_area(&mut self, focus: FocusArea) {
        if self.focus_area == focus {
            return;
        }
        self.focus_area = focus;
        match self.focus_area {
            FocusArea::Input => {
                self.push_log(LogKind::Info, "focus switched to input".to_string());
            }
            FocusArea::Transcript => {
                if self.transcript_selected.is_none() && !self.logs.is_empty() {
                    self.transcript_selected = Some(self.logs.len().saturating_sub(1));
                }
                self.push_log(
                    LogKind::Info,
                    "focus switched to output (arrows now select transcript)".to_string(),
                );
            }
        }
    }

    fn toggle_focus_area(&mut self) {
        let next = if self.focus_area == FocusArea::Input {
            FocusArea::Transcript
        } else {
            FocusArea::Input
        };
        self.set_focus_area(next);
    }

    fn has_resume_picker(&self) -> bool {
        self.resume_picker.is_some()
    }

    async fn open_resume_picker(&mut self) -> Result<()> {
        let sessions = crate::list_recent_sessions(&self.runtime, 40).await?;
        if sessions.is_empty() {
            self.push_log(LogKind::Info, "no historical sessions found".to_string());
            self.push_log(
                LogKind::Info,
                "tip: send a few messages first, then /resume to switch".to_string(),
            );
            return Ok(());
        }
        let selected = sessions
            .iter()
            .position(|session| session.session_id == self.session_id)
            .unwrap_or(0);
        self.shortcuts_visible = false;
        self.resume_picker = Some(ResumePickerState { sessions, selected });
        Ok(())
    }

    fn close_resume_picker(&mut self) {
        self.resume_picker = None;
    }

    fn move_resume_picker_selection(&mut self, step: isize) {
        let Some(picker) = self.resume_picker.as_mut() else {
            return;
        };
        if picker.sessions.is_empty() {
            picker.selected = 0;
            return;
        }
        let max_index = picker.sessions.len().saturating_sub(1);
        let next = if step < 0 {
            picker.selected.saturating_sub(step.unsigned_abs())
        } else {
            picker.selected.saturating_add(step as usize).min(max_index)
        };
        picker.selected = next;
    }

    fn selected_resume_session_id(&self) -> Option<String> {
        let picker = self.resume_picker.as_ref()?;
        picker
            .sessions
            .get(picker.selected)
            .map(|session| session.session_id.clone())
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

    fn move_transcript_selection(&mut self, step: isize) {
        if self.logs.is_empty() {
            self.transcript_selected = None;
            return;
        }
        let max_index = self.logs.len().saturating_sub(1);
        let current = self.transcript_selected.unwrap_or(max_index).min(max_index);
        let next = if step < 0 {
            current.saturating_sub(step.unsigned_abs())
        } else {
            current.saturating_add(step as usize).min(max_index)
        };
        self.transcript_selected = Some(next);
        self.ensure_transcript_selection_visible(next);
    }

    fn select_transcript_boundary(&mut self, to_top: bool) {
        if self.logs.is_empty() {
            self.transcript_selected = None;
            return;
        }
        let index = if to_top {
            0
        } else {
            self.logs.len().saturating_sub(1)
        };
        self.transcript_selected = Some(index);
        self.ensure_transcript_selection_visible(index);
    }

    fn ensure_transcript_selection_visible(&mut self, selected_index: usize) {
        let viewport = self.transcript_viewport_height.max(1);
        let max_scroll = self.max_transcript_scroll(viewport);
        let current_scroll =
            max_scroll.saturating_sub(self.transcript_offset_from_bottom.min(max_scroll));

        let width = usize::from(self.transcript_viewport_width.max(1));
        let mut start_line = 0u16;
        for (index, entry) in self.logs.iter().enumerate() {
            let line_count =
                wrapped_visual_line_count(visual_log_text(entry.kind, &entry.text).as_str(), width)
                    .max(1)
                    .min(u16::MAX as usize) as u16;
            let end_line = start_line.saturating_add(line_count.saturating_sub(1));
            if index == selected_index {
                let mut target_scroll = current_scroll;
                if start_line < current_scroll {
                    target_scroll = start_line;
                } else {
                    let viewport_end = current_scroll.saturating_add(viewport.saturating_sub(1));
                    if end_line > viewport_end {
                        target_scroll = end_line.saturating_sub(viewport.saturating_sub(1));
                    }
                }
                let clamped = target_scroll.min(max_scroll);
                self.transcript_offset_from_bottom = max_scroll.saturating_sub(clamped);
                return;
            }
            start_line = end_line.saturating_add(1);
        }
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
        if trimmed.starts_with('/') {
            let body = trimmed.trim_start_matches('/');
            return slash_command::popup_lines_with_language(
                body,
                7,
                self.display_language.as_str(),
            );
        }

        let cursor = self.input_cursor.min(self.input.len());
        let head = &self.input[..cursor];
        let token_start = head
            .rfind(char::is_whitespace)
            .map(|index| index.saturating_add(1))
            .unwrap_or(0);
        let token = &head[token_start..];
        if let Some(query) = token.strip_prefix('@') {
            return self.mention_popup_lines(query, 7);
        }

        Vec::new()
    }

    pub fn popup_title(&self) -> &'static str {
        let trimmed = self.input.trim_start();
        if trimmed.starts_with('/') {
            return if self.is_zh_language() {
                " 命令 "
            } else {
                " Commands "
            };
        }
        let cursor = self.input_cursor.min(self.input.len());
        let head = &self.input[..cursor];
        let token_start = head
            .rfind(char::is_whitespace)
            .map(|index| index.saturating_add(1))
            .unwrap_or(0);
        let token = &head[token_start..];
        if token.starts_with('@') {
            return if self.is_zh_language() {
                " 文件 "
            } else {
                " Files "
            };
        }
        if self.is_zh_language() {
            " 命令 "
        } else {
            " Commands "
        }
    }

    fn mention_popup_lines(&self, query: &str, limit: usize) -> Vec<String> {
        let query = query.trim();
        if query.is_empty() {
            return self
                .workspace_files
                .iter()
                .take(limit)
                .map(|item| format!("@{}", item.path))
                .collect();
        }
        let lowered = query.to_ascii_lowercase();
        self.workspace_files
            .iter()
            .filter(|item| item.lowered.contains(&lowered))
            .take(limit)
            .map(|item| format!("@{}", item.path))
            .collect()
    }

    pub async fn drain_stream_events(&mut self) {
        self.flush_pending_paste();
        self.drain_approval_requests();

        let mut drained = 0usize;
        loop {
            let Some(receiver) = self.stream_rx.as_mut() else {
                break;
            };
            if drained >= MAX_DRAIN_MESSAGES_PER_TICK {
                break;
            }
            match receiver.try_recv() {
                Ok(message) => self.handle_stream_message(message),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.stream_rx = None;
                    self.busy = false;
                    self.active_assistant = None;
                    self.active_reasoning = None;
                    self.stream_received_content_delta = false;
                    self.stream_tool_markup_open = false;
                    self.session_stats_dirty = true;
                    break;
                }
            }
            drained = drained.saturating_add(1);
        }

        if self.session_stats_dirty {
            self.reload_session_stats().await;
            self.session_stats_dirty = false;
        }
    }

    async fn reload_session_stats(&mut self) {
        self.session_stats = crate::load_session_stats(&self.runtime, &self.session_id).await;
    }

    fn drain_approval_requests(&mut self) {
        loop {
            let Some(receiver) = self.approval_rx.as_mut() else {
                break;
            };
            match receiver.try_recv() {
                Ok(request) => {
                    self.enqueue_approval_request(request);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.approval_rx = None;
                    break;
                }
            }
        }
    }

    fn enqueue_approval_request(&mut self, request: ApprovalRequest) {
        // Ensure the approval overlay is not obscured by other modal states.
        self.shortcuts_visible = false;
        self.resume_picker = None;

        if self.active_approval.is_none() {
            self.active_approval = Some(request);
            return;
        }
        self.approval_queue.push_back(request);
    }

    fn flush_pending_paste(&mut self) {
        while let Some(chunk) = self.pending_paste.pop_front() {
            self.insert_text_at_cursor(chunk.as_str());
        }
    }

    pub fn on_paste(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        self.pending_paste.push_back(text);
    }

    pub async fn on_key(&mut self, key: KeyEvent) -> Result<()> {
        if self
            .ctrl_c_hint_deadline
            .map(|deadline| Instant::now() > deadline)
            .unwrap_or(false)
        {
            self.ctrl_c_hint_deadline = None;
        }

        if self.active_approval.is_some() {
            self.handle_approval_key(key);
            return Ok(());
        }

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
                    self.handle_ctrl_c();
                    return Ok(());
                }
                KeyCode::Char('l') => {
                    self.logs.clear();
                    self.active_assistant = None;
                    self.active_reasoning = None;
                    self.transcript_offset_from_bottom = 0;
                    self.transcript_selected = None;
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

        if self.has_resume_picker() {
            self.handle_resume_picker_key(key).await?;
            return Ok(());
        }

        if self.focus_area == FocusArea::Transcript {
            if self.handle_transcript_focus_key(key) {
                return Ok(());
            }
            self.focus_area = FocusArea::Input;
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
            KeyCode::F(3) => {
                self.toggle_focus_area();
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

    fn handle_ctrl_c(&mut self) {
        let now = Instant::now();
        if self.busy {
            if self
                .ctrl_c_hint_deadline
                .map(|deadline| deadline >= now)
                .unwrap_or(false)
            {
                self.should_quit = true;
                return;
            }
            if self.runtime.state.monitor.cancel(&self.session_id) {
                self.push_log(
                    LogKind::Info,
                    "interrupt requested, waiting for running round to stop...".to_string(),
                );
            } else {
                self.push_log(
                    LogKind::Info,
                    "no cancellable round found, press Ctrl+C again to exit".to_string(),
                );
            }
            self.ctrl_c_hint_deadline = Some(now + CTRL_C_EXIT_WINDOW);
            return;
        }

        if self
            .ctrl_c_hint_deadline
            .map(|deadline| deadline >= now)
            .unwrap_or(false)
        {
            self.should_quit = true;
            return;
        }

        self.ctrl_c_hint_deadline = Some(now + CTRL_C_EXIT_WINDOW);
        self.push_log(
            LogKind::Info,
            "press Ctrl+C again to exit (or wait to continue)".to_string(),
        );
    }

    fn handle_approval_key(&mut self, key: KeyEvent) {
        let Some(request) = self.active_approval.take() else {
            return;
        };

        let response = match key.code {
            KeyCode::Esc => Some(ApprovalResponse::Deny),
            KeyCode::Char('1') | KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                Some(ApprovalResponse::ApproveOnce)
            }
            KeyCode::Char('2') | KeyCode::Char('a') | KeyCode::Char('A') => {
                Some(ApprovalResponse::ApproveSession)
            }
            KeyCode::Char('3') | KeyCode::Char('n') | KeyCode::Char('N') => {
                Some(ApprovalResponse::Deny)
            }
            _ => None,
        };

        let Some(response) = response else {
            self.active_approval = Some(request);
            return;
        };

        let _ = request.respond_to.send(response);
        match response {
            ApprovalResponse::ApproveOnce => {
                self.push_log(LogKind::Info, format!("approved once: {}", request.summary))
            }
            ApprovalResponse::ApproveSession => self.push_log(
                LogKind::Info,
                format!("approved for session: {}", request.summary),
            ),
            ApprovalResponse::Deny => {
                self.push_log(LogKind::Info, format!("denied: {}", request.summary))
            }
        };

        self.active_approval = self.approval_queue.pop_front();
    }

    async fn handle_resume_picker_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.close_resume_picker();
            }
            KeyCode::Up => self.move_resume_picker_selection(-1),
            KeyCode::Down => self.move_resume_picker_selection(1),
            KeyCode::PageUp => self.move_resume_picker_selection(-8),
            KeyCode::PageDown => self.move_resume_picker_selection(8),
            KeyCode::Home => {
                if let Some(picker) = self.resume_picker.as_mut() {
                    picker.selected = 0;
                }
            }
            KeyCode::End => {
                if let Some(picker) = self.resume_picker.as_mut() {
                    picker.selected = picker.sessions.len().saturating_sub(1);
                }
            }
            KeyCode::Enter => {
                if let Some(target) = self.selected_resume_session_id() {
                    self.close_resume_picker();
                    self.resume_to_session(target.as_str()).await?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_transcript_focus_key(&mut self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
            return false;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.focus_area = FocusArea::Input;
                true
            }
            KeyCode::Up => {
                self.move_transcript_selection(-1);
                true
            }
            KeyCode::Down => {
                self.move_transcript_selection(1);
                true
            }
            KeyCode::Left => {
                self.scroll_transcript_up(1);
                true
            }
            KeyCode::Right => {
                self.scroll_transcript_down(1);
                true
            }
            KeyCode::PageUp => {
                self.move_transcript_selection(-8);
                true
            }
            KeyCode::PageDown => {
                self.move_transcript_selection(8);
                true
            }
            KeyCode::Home => {
                self.select_transcript_boundary(true);
                true
            }
            KeyCode::End => {
                self.select_transcript_boundary(false);
                true
            }
            KeyCode::F(3) => {
                self.toggle_focus_area();
                true
            }
            _ => false,
        }
    }

    async fn resume_to_session(&mut self, target: &str) -> Result<()> {
        if self.busy {
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "助手仍在运行，请等待完成后再恢复其他会话",
                    "assistant is still running, wait for completion before resuming another session",
                ),
            );
            return Ok(());
        }

        let cleaned = target.trim();
        if cleaned.is_empty() {
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "会话 ID 不能为空",
                    "session id is empty",
                ),
            );
            return Ok(());
        }
        if cleaned == self.session_id {
            if self.is_zh_language() {
                self.push_log(LogKind::Info, format!("当前已在会话: {cleaned}"));
            } else {
                self.push_log(LogKind::Info, format!("already using session: {cleaned}"));
            }
            return Ok(());
        }

        if !crate::session_exists(&self.runtime, cleaned).await? {
            if self.is_zh_language() {
                self.push_log(LogKind::Error, format!("会话不存在: {cleaned}"));
                self.push_log(LogKind::Info, "提示: 用 /resume 列出可用会话".to_string());
            } else {
                self.push_log(LogKind::Error, format!("session not found: {cleaned}"));
                self.push_log(
                    LogKind::Info,
                    "tip: /resume to list available sessions".to_string(),
                );
            }
            return Ok(());
        }

        self.switch_to_existing_session(cleaned).await?;
        Ok(())
    }

    async fn switch_to_existing_session(&mut self, session_id: &str) -> Result<()> {
        let history = crate::load_session_history_entries(&self.runtime, session_id, 0).await?;
        self.session_id = session_id.to_string();
        self.runtime.save_session(&self.session_id).ok();
        self.input.clear();
        self.input_cursor = 0;
        self.history_cursor = None;
        self.config_wizard = None;
        self.last_usage = None;
        self.active_assistant = None;
        self.active_reasoning = None;
        self.stream_saw_output = false;
        self.stream_saw_final = false;
        self.stream_received_content_delta = false;
        self.stream_tool_markup_open = false;
        self.tool_phase_notice_emitted = false;
        self.approval_rx = None;
        self.active_approval = None;
        self.approval_queue.clear();
        self.ctrl_c_hint_deadline = None;
        self.transcript_offset_from_bottom = 0;
        self.transcript_selected = None;
        self.focus_area = FocusArea::Input;
        self.resume_picker = None;
        self.logs.clear();

        let restored = self.restore_transcript_from_history(history);
        self.session_stats = crate::SessionStatsSnapshot::default();
        self.reload_session_stats().await;
        if self.is_zh_language() {
            self.push_log(
                LogKind::Info,
                format!(
                    "已恢复会话: {}（已恢复 {restored} 条消息）",
                    self.session_id
                ),
            );
        } else {
            self.push_log(
                LogKind::Info,
                format!("resumed session: {} ({restored} messages)", self.session_id),
            );
        }
        Ok(())
    }

    fn restore_transcript_from_history(&mut self, history: Vec<Value>) -> usize {
        let mut restored = 0usize;
        for record in history {
            let Some(role) = record.get("role").and_then(Value::as_str) else {
                continue;
            };

            let content = history_content_to_text(record.get("content"));
            let reasoning = record
                .get("reasoning_content")
                .or_else(|| record.get("reasoning"))
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default()
                .to_string();

            match role {
                "user" => {
                    if !content.trim().is_empty() {
                        self.push_log(LogKind::User, content.trim().to_string());
                        restored = restored.saturating_add(1);
                    }
                }
                "assistant" => {
                    if !reasoning.is_empty() {
                        self.push_log(LogKind::Reasoning, reasoning);
                        restored = restored.saturating_add(1);
                    }
                    let cleaned = sanitize_assistant_text(content.as_str());
                    if !cleaned.is_empty() {
                        self.push_log(LogKind::Assistant, cleaned);
                        restored = restored.saturating_add(1);
                    }
                }
                "tool" => {
                    let cleaned = content.trim();
                    if !cleaned.is_empty() {
                        let (preview, truncated) = truncate_by_chars(cleaned, 500);
                        let mut line = format!("[history] {preview}");
                        if truncated {
                            line.push_str(" ...");
                        }
                        self.push_log(LogKind::Tool, line);
                        restored = restored.saturating_add(1);
                    }
                }
                _ => {
                    let cleaned = content.trim();
                    if !cleaned.is_empty() {
                        self.push_log(LogKind::Info, format!("[{role}] {cleaned}"));
                        restored = restored.saturating_add(1);
                    }
                }
            }
        }

        if restored == 0 {
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "当前会话暂无历史消息",
                    "history is empty for this session",
                ),
            );
        }
        restored
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
        self.resume_picker = None;
        self.focus_area = FocusArea::Input;

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
        self.stream_received_content_delta = false;
        self.stream_tool_markup_open = false;
        self.tool_phase_notice_emitted = false;
        let user_echo = prompt.clone();
        self.start_stream_request(prompt, user_echo).await
    }

    async fn start_stream_request(&mut self, prompt: String, user_echo: String) -> Result<()> {
        if self.busy {
            self.push_log(
                LogKind::Error,
                "assistant is still running, wait for completion before sending a new prompt"
                    .to_string(),
            );
            return Ok(());
        }

        self.ctrl_c_hint_deadline = None;
        self.push_log(LogKind::User, user_echo);
        self.busy = true;
        self.active_assistant = None;
        self.active_reasoning = None;

        let (approval_tx, approval_rx) = new_approval_channel();
        self.approval_rx = Some(approval_rx);
        self.approval_queue.clear();
        self.active_approval = None;

        let mut request =
            crate::build_wunder_request(&self.runtime, &self.global, &prompt, &self.session_id)
                .await?;
        request.approval_tx = Some(approval_tx);
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
        if trimmed.starts_with('/') {
            let body = trimmed.trim_start_matches('/');
            if body.contains(char::is_whitespace) {
                return;
            }
            if let Some(suggestion) = slash_command::first_command_completion(body) {
                self.input = format!("/{suggestion} ");
                self.input_cursor = self.input.len();
            }
            return;
        }

        let cursor = self.input_cursor.min(self.input.len());
        let token_start = self
            .input
            .get(..cursor)
            .and_then(|text| text.rfind(char::is_whitespace))
            .map(|index| index.saturating_add(1))
            .unwrap_or(0);
        let token = &self.input[token_start..cursor];
        let Some(query) = token.strip_prefix('@') else {
            return;
        };

        let suggestions = self.mention_popup_lines(query, 1);
        let Some(first) = suggestions.first() else {
            return;
        };
        let replacement = format!("{first} ");
        self.input.replace_range(token_start..cursor, &replacement);
        self.input_cursor = token_start.saturating_add(replacement.len());
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
        let cleaned = value.trim();
        if cleaned.is_empty() {
            return;
        }
        let cleaned = if cleaned.chars().count() > MAX_HISTORY_ENTRY_CHARS {
            let mut shortened = cleaned
                .chars()
                .take(MAX_HISTORY_ENTRY_CHARS)
                .collect::<String>();
            shortened.push_str("...(truncated)");
            shortened
        } else {
            cleaned.to_string()
        };
        if self
            .history
            .last()
            .map(|existing| existing == &cleaned)
            .unwrap_or(false)
        {
            return;
        }
        self.history.push(cleaned);
        if self.history.len() > MAX_PERSISTED_HISTORY {
            let keep_from = self.history.len().saturating_sub(MAX_PERSISTED_HISTORY);
            self.history = self.history.split_off(keep_from);
        }
        self.persist_history();
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

    fn insert_text_at_cursor(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.input_cursor > self.input.len() {
            self.input_cursor = self.input.len();
        }
        if !self.input.is_char_boundary(self.input_cursor) {
            self.input_cursor = prev_char_boundary(&self.input, self.input_cursor);
        }
        self.input.insert_str(self.input_cursor, text);
        self.input_cursor = self.input_cursor.saturating_add(text.len());
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
                self.active_reasoning = None;
                self.stream_rx = None;
                self.approval_rx = None;
                self.active_approval = None;
                self.approval_queue.clear();
                self.stream_saw_output = false;
                self.stream_saw_final = false;
                self.stream_received_content_delta = false;
                self.stream_tool_markup_open = false;
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
                self.active_reasoning = None;
                self.stream_rx = None;
                self.approval_rx = None;
                self.active_approval = None;
                self.approval_queue.clear();
                self.stream_saw_output = false;
                self.stream_saw_final = false;
                self.stream_received_content_delta = false;
                self.stream_tool_markup_open = false;
                self.tool_phase_notice_emitted = false;
                self.session_stats_dirty = true;
            }
        }
    }

    fn apply_stream_event(&mut self, event: StreamEvent) {
        let payload = event_payload(&event.data);
        match event.event.as_str() {
            "llm_output_delta" => {
                if let Some(reasoning_delta) =
                    payload.get("reasoning_delta").and_then(Value::as_str)
                {
                    let cleaned_reasoning = sanitize_reasoning_text(reasoning_delta);
                    if !cleaned_reasoning.is_empty() {
                        self.stream_saw_output = true;
                        self.merge_reasoning_delta(cleaned_reasoning.as_str());
                    }
                }

                if let Some(delta) = payload.get("delta").and_then(Value::as_str) {
                    let cleaned_delta = sanitize_assistant_delta_streaming(
                        delta,
                        &mut self.stream_tool_markup_open,
                    );
                    if !cleaned_delta.is_empty() {
                        self.stream_saw_output = true;
                        self.stream_received_content_delta = true;
                        self.merge_assistant_delta(cleaned_delta.as_str());
                    }
                }
            }
            "llm_output" => {
                if payload_has_tool_calls(payload) {
                    self.emit_tool_phase_notice();
                    self.active_assistant = None;
                    self.active_reasoning = None;
                    self.stream_tool_markup_open = false;
                    return;
                }

                if let Some(reasoning) = payload.get("reasoning").and_then(Value::as_str) {
                    let cleaned_reasoning = sanitize_reasoning_text(reasoning);
                    if !cleaned_reasoning.is_empty() {
                        self.stream_saw_output = true;
                        self.merge_reasoning_content(cleaned_reasoning.as_str());
                    }
                }

                if let Some(content) = payload.get("content").and_then(Value::as_str) {
                    let cleaned = sanitize_assistant_text(content);
                    if !cleaned.is_empty() {
                        self.stream_saw_output = true;
                        if self.stream_received_content_delta {
                            self.replace_assistant_content(cleaned.as_str());
                        } else {
                            self.merge_assistant_content(cleaned.as_str());
                        }
                    }
                }

                self.stream_tool_markup_open = false;
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
                self.active_reasoning = None;
                self.stream_received_content_delta = false;
                self.stream_tool_markup_open = false;
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
                self.active_reasoning = None;
                self.stream_tool_markup_open = false;
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
                if let Some(reasoning) = payload.get("reasoning").and_then(Value::as_str) {
                    let cleaned_reasoning = sanitize_reasoning_text(reasoning);
                    if !cleaned_reasoning.is_empty() {
                        self.stream_saw_output = true;
                        self.merge_reasoning_content(cleaned_reasoning.as_str());
                    }
                }
                let answer = payload
                    .get("answer")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !answer.trim().is_empty() {
                    self.stream_saw_output = true;
                    self.merge_final_answer(answer);
                }
                self.stream_tool_markup_open = false;
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
                for help in slash_command::help_lines_with_language(self.display_language.as_str())
                {
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
            SlashCommand::Resume => {
                self.handle_resume_slash(command.args).await?;
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
            SlashCommand::Approvals => {
                self.handle_approvals_slash(command.args).await?;
            }
            SlashCommand::Diff => {
                self.handle_diff_slash().await?;
            }
            SlashCommand::Review => {
                self.handle_review_slash(command.args).await?;
            }
            SlashCommand::Mention => {
                self.handle_mention_slash(command.args).await?;
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
            "approval_mode": self.approval_mode,
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
            self.display_language.as_str(),
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

    async fn handle_resume_slash(&mut self, args: &str) -> Result<()> {
        let cleaned = args.trim();
        if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("list") {
            self.open_resume_picker().await?;
            if self.resume_picker.is_some() {
                self.push_log(
                    LogKind::Info,
                    "resume picker opened (Up/Down to choose, Enter to resume, Esc to cancel)"
                        .to_string(),
                );
            }
            return Ok(());
        }

        let target = if cleaned.eq_ignore_ascii_case("last") {
            self.runtime.load_saved_session().ok_or_else(|| {
                anyhow!(crate::locale::tr(
                    self.display_language.as_str(),
                    "未找到保存的会话",
                    "no saved session found",
                ))
            })?
        } else if let Ok(index) = cleaned.parse::<usize>() {
            let sessions = crate::list_recent_sessions(&self.runtime, 40).await?;
            let Some(item) = sessions.get(index.saturating_sub(1)) else {
                self.push_log(
                    LogKind::Error,
                    format!("session index out of range: {index}"),
                );
                return Ok(());
            };
            item.session_id.clone()
        } else {
            cleaned.to_string()
        };

        self.resume_to_session(target.as_str()).await
    }

    async fn handle_model_slash(&mut self, args: &str) -> Result<()> {
        let target = args.trim();
        if target.is_empty() {
            self.show_model_status().await;
            return Ok(());
        }

        let config = self.runtime.state.config_store.get().await;
        if !config.llm.models.contains_key(target) {
            if self.is_zh_language() {
                self.push_log(LogKind::Error, format!("模型不存在: {target}"));
            } else {
                self.push_log(LogKind::Error, format!("model not found: {target}"));
            }
            let models = crate::sorted_model_names(&config);
            if models.is_empty() {
                self.push_log(
                    LogKind::Info,
                    crate::locale::tr(
                        self.display_language.as_str(),
                        "尚未配置模型，请先运行 /config",
                        "no models configured. run /config first.",
                    ),
                );
            } else if self.is_zh_language() {
                self.push_log(LogKind::Info, format!("可用模型: {}", models.join(", ")));
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
        if self.is_zh_language() {
            self.push_log(LogKind::Info, format!("模型已切换: {target}"));
        } else {
            self.push_log(LogKind::Info, format!("model set: {target}"));
        }
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
        if self.is_zh_language() {
            self.push_log(LogKind::Info, format!("当前模型: {active_model}"));
        } else {
            self.push_log(LogKind::Info, format!("current model: {active_model}"));
        }

        let models = crate::sorted_model_names(&config);
        if models.is_empty() {
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "尚未配置模型，请先运行 /config",
                    "no models configured. run /config first.",
                ),
            );
            return;
        }

        self.push_log(
            LogKind::Info,
            crate::locale::tr(
                self.display_language.as_str(),
                "可用模型：",
                "available models:",
            ),
        );
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
            if self.is_zh_language() {
                self.push_log(
                    LogKind::Info,
                    format!(
                        "工具调用模式: 模型={} 模式={}",
                        self.model_name, self.tool_call_mode
                    ),
                );
            } else {
                self.push_log(
                    LogKind::Info,
                    format!(
                        "tool_call_mode: model={} mode={}",
                        self.model_name, self.tool_call_mode
                    ),
                );
            }
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "用法: /tool-call-mode <tool_call|function_call> [model]",
                    "usage: /tool-call-mode <tool_call|function_call> [model]",
                ),
            );
            return Ok(());
        }

        let mut parts = cleaned.split_whitespace();
        let Some(mode_token) = parts.next() else {
            return Ok(());
        };
        let Some(mode) = crate::parse_tool_call_mode(mode_token) else {
            if self.is_zh_language() {
                self.push_log(LogKind::Error, format!("非法模式: {mode_token}"));
            } else {
                self.push_log(LogKind::Error, format!("invalid mode: {mode_token}"));
            }
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "可选模式: tool_call, function_call",
                    "valid modes: tool_call, function_call",
                ),
            );
            return Ok(());
        };

        let model = parts.next().map(str::to_string);
        if parts.next().is_some() {
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "参数过多",
                    "too many arguments",
                ),
            );
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "用法: /tool-call-mode <tool_call|function_call> [model]",
                    "usage: /tool-call-mode <tool_call|function_call> [model]",
                ),
            );
            return Ok(());
        }

        let config = self.runtime.state.config_store.get().await;
        let target_model = if let Some(model_name) = model {
            if !config.llm.models.contains_key(&model_name) {
                if self.is_zh_language() {
                    self.push_log(LogKind::Error, format!("配置中不存在模型: {model_name}"));
                } else {
                    self.push_log(
                        LogKind::Error,
                        format!("model not found in config: {model_name}"),
                    );
                }
                return Ok(());
            }
            model_name
        } else {
            self.runtime
                .resolve_model_name(self.global.model.as_deref())
                .await
                .ok_or_else(|| {
                    anyhow!(crate::locale::tr(
                        self.display_language.as_str(),
                        "尚未配置 LLM 模型",
                        "no llm model configured",
                    ))
                })?
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
        if self.is_zh_language() {
            self.push_log(
                LogKind::Info,
                format!(
                    "工具调用模式已设置: 模型={target_model} 模式={}",
                    mode.as_str()
                ),
            );
        } else {
            self.push_log(
                LogKind::Info,
                format!(
                    "tool_call_mode set: model={target_model} mode={}",
                    mode.as_str()
                ),
            );
        }
        Ok(())
    }

    async fn handle_approvals_slash(&mut self, args: &str) -> Result<()> {
        let cleaned = args.trim();
        if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("show") {
            if self.is_zh_language() {
                self.push_log(LogKind::Info, format!("审批模式: {}", self.approval_mode));
            } else {
                self.push_log(
                    LogKind::Info,
                    format!("approval_mode: {}", self.approval_mode),
                );
            }
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "用法: /approvals [show|suggest|auto_edit|full_auto]",
                    "usage: /approvals [show|suggest|auto_edit|full_auto]",
                ),
            );
            return Ok(());
        }

        let Some(mode) = crate::parse_approval_mode(cleaned) else {
            if self.is_zh_language() {
                self.push_log(LogKind::Error, format!("非法审批模式: {cleaned}"));
            } else {
                self.push_log(LogKind::Error, format!("invalid approval mode: {cleaned}"));
            }
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "可选模式: suggest, auto_edit, full_auto",
                    "valid modes: suggest, auto_edit, full_auto",
                ),
            );
            return Ok(());
        };

        let mode_text = mode.as_str().to_string();
        self.runtime
            .state
            .config_store
            .update(move |config| {
                config.security.approval_mode = Some(mode_text.clone());
            })
            .await?;
        self.sync_model_status().await;
        if self.is_zh_language() {
            self.push_log(LogKind::Info, format!("审批模式已设置: {}", mode.as_str()));
        } else {
            self.push_log(
                LogKind::Info,
                format!("approval_mode set: {}", mode.as_str()),
            );
        }
        Ok(())
    }

    async fn handle_diff_slash(&mut self) -> Result<()> {
        let root = self.runtime.launch_dir.clone();
        let language = self.display_language.clone();
        let lines = tokio::task::spawn_blocking(move || {
            crate::git_diff_summary_lines_with_language(root.as_path(), language.as_str())
        })
        .await
        .unwrap_or_else(|_| {
            Ok(vec![
                crate::locale::tr(self.display_language.as_str(), "变更摘要", "diff"),
                crate::locale::tr(
                    self.display_language.as_str(),
                    "[错误] diff 任务已取消",
                    "[error] diff task cancelled",
                ),
            ])
        })?;
        for line in lines {
            self.push_log(LogKind::Info, line);
        }
        Ok(())
    }

    async fn handle_review_slash(&mut self, args: &str) -> Result<()> {
        if self.busy {
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "助手仍在运行，请等待完成后再执行 /review",
                    "assistant is still running, wait for completion before running /review",
                ),
            );
            return Ok(());
        }

        let root = self.runtime.launch_dir.clone();
        let focus = args.trim().to_string();
        let focus_for_prompt = focus.clone();
        let language = self.display_language.clone();
        let prompt = match tokio::task::spawn_blocking(move || {
            crate::build_review_prompt_with_language(
                root.as_path(),
                &focus_for_prompt,
                language.as_str(),
            )
        })
        .await
        {
            Ok(Ok(prompt)) => prompt,
            Ok(Err(err)) => {
                self.push_log(LogKind::Error, err.to_string());
                return Ok(());
            }
            Err(err) => {
                if self.is_zh_language() {
                    self.push_log(LogKind::Error, format!("review 任务已取消: {err}"));
                } else {
                    self.push_log(LogKind::Error, format!("review task cancelled: {err}"));
                }
                return Ok(());
            }
        };

        let user_echo = if focus.is_empty() {
            "/review".to_string()
        } else {
            format!("/review {focus}")
        };
        self.start_stream_request(prompt, user_echo).await
    }

    async fn handle_mention_slash(&mut self, args: &str) -> Result<()> {
        let query = args.trim();
        if query.is_empty() {
            self.push_log(LogKind::Info, "usage: /mention <query>".to_string());
            return Ok(());
        }
        let results = crate::search_workspace_files(self.runtime.launch_dir.as_path(), query, 30);
        if results.is_empty() {
            self.push_log(LogKind::Info, format!("no files found for: {query}"));
            return Ok(());
        }
        self.push_log(
            LogKind::Info,
            format!("mention results ({})", results.len()),
        );
        for path in results {
            self.push_log(LogKind::Info, path);
        }
        Ok(())
    }

    async fn sync_model_status(&mut self) {
        self.display_language = crate::locale::resolve_cli_language(&self.global);
        let config = self.runtime.state.config_store.get().await;
        self.approval_mode = self
            .global
            .approval_mode
            .map(|mode| mode.as_str().to_string())
            .or_else(|| {
                config
                    .security
                    .approval_mode
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "full_auto".to_string());
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
        let is_zh = self.is_zh_language();
        let mut lines = vec![
            if is_zh {
                "会话".to_string()
            } else {
                "session".to_string()
            },
            if is_zh {
                format!("- 会话 ID: {}", self.session_id)
            } else {
                format!("- id: {}", self.session_id)
            },
            if is_zh {
                format!("- 模型: {}", self.model_name)
            } else {
                format!("- model: {}", self.model_name)
            },
        ];

        if let Some(total) = self.model_max_context {
            let used = self.session_stats.context_used_tokens.max(0) as u64;
            let left =
                crate::context_left_percent(self.session_stats.context_used_tokens, Some(total))
                    .unwrap_or(0);
            if is_zh {
                lines.push(format!("- 上下文: {used}/{total} (剩余 {left}%)"));
            } else {
                lines.push(format!("- context: {used}/{total} ({left}% left)"));
            }
        } else if is_zh {
            lines.push(format!(
                "- 上下文: {}/未知",
                self.session_stats.context_used_tokens.max(0)
            ));
        } else {
            lines.push(format!(
                "- context: {}/unknown",
                self.session_stats.context_used_tokens.max(0)
            ));
        }

        if is_zh {
            lines.push(format!("- 模型调用: {}", self.session_stats.model_calls));
            lines.push(format!("- 工具调用: {}", self.session_stats.tool_calls));
            lines.push(format!("- 工具结果: {}", self.session_stats.tool_results));
            lines.push(format!(
                "- token 占用: input={} output={} total={}",
                self.session_stats.total_input_tokens,
                self.session_stats.total_output_tokens,
                self.session_stats.total_tokens
            ));
        } else {
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
        }
        lines
    }

    async fn handle_system_slash(&mut self, args: &str) -> Result<()> {
        let cleaned = args.trim();
        if cleaned.eq_ignore_ascii_case("clear") {
            self.runtime.clear_extra_prompt()?;
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "额外提示词已清除",
                    "extra prompt cleared",
                ),
            );
        } else if let Some(rest) = cleaned.strip_prefix("set ") {
            let prompt = rest.trim();
            if prompt.is_empty() {
                self.push_log(
                    LogKind::Error,
                    crate::locale::tr(
                        self.display_language.as_str(),
                        "额外提示词为空",
                        "extra prompt is empty",
                    ),
                );
                self.push_log(
                    LogKind::Info,
                    crate::locale::tr(
                        self.display_language.as_str(),
                        "用法: /system [set <extra_prompt>|clear]",
                        "usage: /system [set <extra_prompt>|clear]",
                    ),
                );
                return Ok(());
            }
            self.runtime.save_extra_prompt(prompt)?;
            if self.is_zh_language() {
                self.push_log(
                    LogKind::Info,
                    format!("额外提示词已保存（{} 字符）", prompt.chars().count()),
                );
            } else {
                self.push_log(
                    LogKind::Info,
                    format!("extra prompt saved ({} chars)", prompt.chars().count()),
                );
            }
        } else if !cleaned.is_empty() && !cleaned.eq_ignore_ascii_case("show") {
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "无效的 /system 参数",
                    "invalid /system args",
                ),
            );
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "用法: /system [set <extra_prompt>|clear]",
                    "usage: /system [set <extra_prompt>|clear]",
                ),
            );
            return Ok(());
        }

        let prompt = crate::build_current_system_prompt(&self.runtime, &self.global).await?;
        let extra_prompt = self.runtime.load_extra_prompt();
        self.push_log(
            LogKind::Info,
            crate::locale::tr(self.display_language.as_str(), "系统提示词", "system"),
        );
        if self.is_zh_language() {
            self.push_log(LogKind::Info, format!("- 会话: {}", self.session_id));
        } else {
            self.push_log(LogKind::Info, format!("- session: {}", self.session_id));
        }
        self.push_log(
            LogKind::Info,
            if self.is_zh_language() {
                format!(
                    "- 额外提示词: {}",
                    extra_prompt
                        .as_ref()
                        .map(|value| format!("已启用（{} 字符）", value.chars().count()))
                        .unwrap_or_else(|| "无".to_string())
                )
            } else {
                format!(
                    "- extra_prompt: {}",
                    extra_prompt
                        .as_ref()
                        .map(|value| format!("enabled ({} chars)", value.chars().count()))
                        .unwrap_or_else(|| "none".to_string())
                )
            },
        );
        self.push_log(
            LogKind::Info,
            crate::locale::tr(
                self.display_language.as_str(),
                "--- 系统提示词开始 ---",
                "--- system prompt ---",
            ),
        );
        for line in prompt.lines() {
            self.push_log(LogKind::Info, line.to_string());
        }
        self.push_log(
            LogKind::Info,
            crate::locale::tr(
                self.display_language.as_str(),
                "--- 系统提示词结束 ---",
                "--- end system prompt ---",
            ),
        );
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
        self.active_assistant = None;
        self.active_reasoning = None;
        self.stream_saw_output = false;
        self.stream_saw_final = false;
        self.stream_received_content_delta = false;
        self.stream_tool_markup_open = false;
        self.tool_phase_notice_emitted = false;
        self.approval_rx = None;
        self.active_approval = None;
        self.approval_queue.clear();
        self.ctrl_c_hint_deadline = None;
        self.focus_area = FocusArea::Input;
        self.transcript_selected = None;
        self.resume_picker = None;
        self.session_stats = crate::SessionStatsSnapshot::default();
        self.reload_session_stats().await;
        self.push_log(
            LogKind::Info,
            format!("switched to session: {}", self.session_id),
        );
    }

    fn status_lines(&self) -> Vec<String> {
        let is_zh = self.is_zh_language();
        vec![
            if is_zh {
                "状态".to_string()
            } else {
                "status".to_string()
            },
            if is_zh {
                format!("- 会话: {}", self.session_id)
            } else {
                format!("- session: {}", self.session_id)
            },
            if is_zh {
                format!("- 模型: {}", self.model_name)
            } else {
                format!("- model: {}", self.model_name)
            },
            if is_zh {
                format!("- 工具调用模式: {}", self.tool_call_mode)
            } else {
                format!("- tool_call_mode: {}", self.tool_call_mode)
            },
            if is_zh {
                format!("- 审批模式: {}", self.approval_mode)
            } else {
                format!("- approval_mode: {}", self.approval_mode)
            },
            if is_zh {
                format!("- 最大轮次: {}", self.model_max_rounds)
            } else {
                format!("- max_rounds: {}", self.model_max_rounds)
            },
            format!(
                "{} {}",
                if is_zh {
                    "- 鼠标模式:"
                } else {
                    "- mouse_mode:"
                },
                if self.mouse_mode == MouseMode::Scroll {
                    if is_zh {
                        "scroll(滚轮)"
                    } else {
                        "scroll"
                    }
                } else if is_zh {
                    "select(选择)"
                } else {
                    "select"
                }
            ),
            if is_zh {
                format!("- 工作目录: {}", self.runtime.launch_dir.to_string_lossy())
            } else {
                format!("- workspace: {}", self.runtime.launch_dir.to_string_lossy())
            },
            if is_zh {
                format!("- 临时目录: {}", self.runtime.temp_root.to_string_lossy())
            } else {
                format!("- temp_root: {}", self.runtime.temp_root.to_string_lossy())
            },
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

    fn ensure_reasoning_entry(&mut self) -> usize {
        if let Some(index) = self.active_reasoning {
            return index;
        }
        let index = self.push_log(LogKind::Reasoning, String::new());
        self.active_reasoning = Some(index);
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
        if let Some(active) = self.active_reasoning {
            self.active_reasoning = if active == index {
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

    fn merge_assistant_delta(&mut self, delta: &str) {
        if delta.trim().is_empty() {
            return;
        }

        let index = self.ensure_assistant_entry();
        if let Some(entry) = self.logs.get_mut(index) {
            merge_stream_text(&mut entry.text, delta);
        }
    }

    fn replace_assistant_content(&mut self, content: &str) {
        let cleaned = sanitize_assistant_text(content);
        if cleaned.is_empty() {
            return;
        }

        let index = self.ensure_assistant_entry();
        if let Some(entry) = self.logs.get_mut(index) {
            if is_equivalent_text(entry.text.as_str(), cleaned.as_str())
                && entry.text.chars().count() >= cleaned.chars().count()
            {
                return;
            }
            entry.text = cleaned;
        }
    }

    fn merge_reasoning_delta(&mut self, delta: &str) {
        let cleaned = sanitize_reasoning_text(delta);
        if cleaned.is_empty() {
            return;
        }

        let index = self.ensure_reasoning_entry();
        if let Some(entry) = self.logs.get_mut(index) {
            merge_stream_text(&mut entry.text, cleaned.as_str());
        }
    }

    fn merge_reasoning_content(&mut self, reasoning: &str) {
        let cleaned = sanitize_reasoning_text(reasoning);
        if cleaned.is_empty() {
            return;
        }

        let index = self.ensure_reasoning_entry();
        if let Some(entry) = self.logs.get_mut(index) {
            if is_equivalent_text(entry.text.as_str(), cleaned.as_str()) {
                if cleaned.chars().count() >= entry.text.chars().count() {
                    entry.text = cleaned;
                }
                return;
            }

            if compact_text_for_compare(cleaned.as_str())
                .starts_with(compact_text_for_compare(entry.text.as_str()).as_str())
            {
                entry.text = cleaned;
                return;
            }

            merge_stream_text(&mut entry.text, cleaned.as_str());
        }
    }

    fn merge_assistant_content(&mut self, content: &str) {
        let cleaned = sanitize_assistant_text(content);
        if cleaned.is_empty() {
            return;
        }

        let index = self.ensure_assistant_entry();
        if let Some(entry) = self.logs.get_mut(index) {
            let current = entry.text.trim();
            if current.is_empty() {
                entry.text = cleaned;
                return;
            }

            if is_equivalent_text(current, cleaned.as_str())
                || compact_text_for_compare(current)
                    .ends_with(compact_text_for_compare(cleaned.as_str()).as_str())
            {
                return;
            }

            if compact_text_for_compare(cleaned.as_str())
                .starts_with(compact_text_for_compare(current).as_str())
            {
                entry.text = cleaned;
                return;
            }

            if !entry.text.ends_with('\n') {
                entry.text.push('\n');
            }
            entry.text.push_str(cleaned.as_str());
        }
    }

    fn merge_final_answer(&mut self, answer: &str) {
        let cleaned = sanitize_assistant_text(answer);
        if cleaned.is_empty() {
            return;
        }

        let index = self.ensure_assistant_entry();
        if let Some(entry) = self.logs.get_mut(index) {
            let current = entry.text.trim();
            if current.is_empty() {
                entry.text = cleaned;
                return;
            }

            if is_equivalent_text(current, cleaned.as_str())
                || compact_text_for_compare(current)
                    .ends_with(compact_text_for_compare(cleaned.as_str()).as_str())
            {
                if cleaned.chars().count() > current.chars().count() {
                    entry.text = cleaned;
                }
                return;
            }

            if compact_text_for_compare(cleaned.as_str())
                .starts_with(compact_text_for_compare(current).as_str())
            {
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
        let text = if matches!(kind, LogKind::Info | LogKind::Error) {
            localize_cli_notice(self.display_language.as_str(), text.as_str())
        } else {
            text
        };
        self.logs.push(LogEntry { kind, text });
        if self.logs.len() > MAX_LOG_ENTRIES {
            self.logs.remove(0);
            if let Some(index) = self.active_assistant.as_mut() {
                *index = index.saturating_sub(1);
            }
            if let Some(index) = self.active_reasoning.as_mut() {
                *index = index.saturating_sub(1);
            }
            if let Some(index) = self.transcript_selected.as_mut() {
                *index = index.saturating_sub(1);
            }
        }

        if self.logs.is_empty() {
            self.transcript_selected = None;
        } else if self.focus_area == FocusArea::Transcript {
            self.transcript_selected = Some(self.logs.len().saturating_sub(1));
        } else if let Some(index) = self.transcript_selected {
            let max_index = self.logs.len().saturating_sub(1);
            self.transcript_selected = Some(index.min(max_index));
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
        LogKind::Reasoning => "think> ",
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

fn history_content_to_text(value: Option<&Value>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    match value {
        Value::String(text) => text.to_string(),
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                if let Some(text) = item
                    .as_object()
                    .and_then(|obj| obj.get("text"))
                    .and_then(Value::as_str)
                {
                    if !text.trim().is_empty() {
                        parts.push(text.trim().to_string());
                    }
                }
            }
            if parts.is_empty() {
                serde_json::to_string(value).unwrap_or_default()
            } else {
                parts.join(
                    "
",
                )
            }
        }
        Value::Object(map) => map
            .get("text")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .unwrap_or_else(|| serde_json::to_string(value).unwrap_or_default()),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn format_session_timestamp(ts: f64) -> String {
    if !ts.is_finite() || ts <= 0.0 {
        return "-".to_string();
    }
    let secs = ts.floor() as i64;
    let nanos = ((ts - secs as f64).max(0.0) * 1_000_000_000.0).round() as u32;
    chrono::Local
        .timestamp_opt(secs, nanos.min(999_999_999))
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "-".to_string())
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

#[cfg(test)]
fn sanitize_assistant_delta(delta: &str) -> String {
    let mut in_tool_markup = false;
    sanitize_assistant_delta_streaming(delta, &mut in_tool_markup)
}

fn sanitize_assistant_delta_streaming(delta: &str, in_tool_markup: &mut bool) -> String {
    if delta.trim().is_empty() {
        return String::new();
    }

    let stripped = strip_streaming_tool_markup(delta, in_tool_markup);
    if stripped.trim().is_empty() {
        return String::new();
    }

    let cleaned = strip_tool_block_tags(stripped.as_str());
    let trimmed = cleaned.trim();
    if trimmed.is_empty() || looks_like_tool_payload(trimmed) {
        return String::new();
    }

    cleaned
}

fn sanitize_reasoning_text(text: &str) -> String {
    text.trim().to_string()
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

fn strip_streaming_tool_markup(text: &str, in_tool_markup: &mut bool) -> String {
    let mut output = String::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if *in_tool_markup {
            let lowered = remaining.to_ascii_lowercase();
            let end_call = lowered.find("</tool_call>");
            let end_tool = lowered.find("</tool>");
            let Some((end_index, end_tag)) = (match (end_call, end_tool) {
                (Some(left), Some(right)) if left <= right => Some((left, "</tool_call>")),
                (Some(_), Some(right)) => Some((right, "</tool>")),
                (Some(left), None) => Some((left, "</tool_call>")),
                (None, Some(right)) => Some((right, "</tool>")),
                (None, None) => None,
            }) else {
                return output;
            };

            let after_end = end_index + end_tag.len();
            remaining = &remaining[after_end..];
            *in_tool_markup = false;
            continue;
        }

        let lowered = remaining.to_ascii_lowercase();
        let start_call = lowered.find("<tool_call");
        let start_tool = lowered.find("<tool");
        let Some(start_index) = (match (start_call, start_tool) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        }) else {
            output.push_str(remaining);
            break;
        };

        output.push_str(&remaining[..start_index]);
        let after_start = &remaining[start_index..];
        let lowered_after = after_start.to_ascii_lowercase();
        let Some(close_offset) = lowered_after.find('>') else {
            *in_tool_markup = true;
            return output;
        };

        let body_start = start_index + close_offset + 1;
        remaining = &remaining[body_start..];
        *in_tool_markup = true;
    }

    output
}

fn merge_stream_text(existing: &mut String, incoming: &str) {
    if incoming.is_empty() {
        return;
    }
    if existing.is_empty() {
        existing.push_str(incoming);
        return;
    }
    if existing == incoming {
        return;
    }
    if incoming.starts_with(existing.as_str()) {
        *existing = incoming.to_string();
        return;
    }
    if existing.ends_with(incoming) {
        return;
    }

    let overlap = longest_suffix_prefix_overlap(existing.as_str(), incoming);
    if overlap > 0 {
        existing.push_str(&incoming[overlap..]);
        return;
    }

    if !existing.ends_with('\n') {
        existing.push('\n');
    }
    existing.push_str(incoming);
}

fn longest_suffix_prefix_overlap(left: &str, right: &str) -> usize {
    let mut len = left.len().min(right.len());
    while len > 0 {
        if !left.is_char_boundary(left.len() - len) || !right.is_char_boundary(len) {
            len = len.saturating_sub(1);
            continue;
        }
        if left[left.len() - len..] == right[..len] {
            return len;
        }
        len = len.saturating_sub(1);
    }
    0
}

fn compact_text_for_compare(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn is_equivalent_text(left: &str, right: &str) -> bool {
    if left.trim().is_empty() || right.trim().is_empty() {
        return false;
    }
    compact_text_for_compare(left) == compact_text_for_compare(right)
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

    let status = match returncode {
        Some(0) => "ok",
        Some(_) => "failed",
        None => "done",
    };

    let mut header = format!("[tool_result] {tool} {status}");
    let mut metrics = Vec::new();
    if let Some(returncode) = returncode {
        metrics.push(format!("exit={returncode}"));
    }
    if let Some(duration_ms) = duration_ms {
        metrics.push(format!("{duration_ms}ms"));
    }
    if !metrics.is_empty() {
        header.push_str(&format!(" ({})", metrics.join(", ")));
    }

    let mut lines = vec![header];
    if !command.is_empty() {
        lines.push(format!("  cmd: {command}"));
    }

    let stdout = first
        .get("stdout")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let stderr = first
        .get("stderr")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let mut has_output = false;
    if returncode.unwrap_or(0) == 0 {
        has_output |= append_text_preview(&mut lines, "stdout", stdout, 6, 900);
        has_output |= append_text_preview(&mut lines, "stderr", stderr, 4, 300);
    } else {
        has_output |= append_text_preview(&mut lines, "stderr", stderr, 6, 900);
        has_output |= append_text_preview(&mut lines, "stdout", stdout, 4, 300);
    }

    if !has_output {
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
) -> bool {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized.trim_end_matches('\n').trim();
    if trimmed.is_empty() {
        return false;
    }

    let (preview, chars_truncated) = truncate_by_chars(trimmed, max_chars);
    let parts = preview.lines().collect::<Vec<_>>();
    if parts.is_empty() {
        return false;
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

    true
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

fn localize_cli_notice(language: &str, text: &str) -> String {
    if !crate::locale::is_zh_language(language) {
        return text.to_string();
    }

    if let Some(value) = text.strip_prefix("approved once: ") {
        return format!("已单次批准: {value}");
    }
    if let Some(value) = text.strip_prefix("approved for session: ") {
        return format!("已批准本会话: {value}");
    }
    if let Some(value) = text.strip_prefix("denied: ") {
        return format!("已拒绝: {value}");
    }
    if let Some(value) = text.strip_prefix("already using session: ") {
        return format!("当前已在会话: {value}");
    }
    if let Some(value) = text.strip_prefix("session not found: ") {
        return format!("会话不存在: {value}");
    }
    if let Some(value) = text.strip_prefix("switched to session: ") {
        return format!("已切换到会话: {value}");
    }
    if let Some(value) = text.strip_prefix("resumed session: ") {
        return format!("已恢复会话: {value}");
    }
    if let Some(value) = text.strip_prefix("model set: ") {
        return format!("模型已切换: {value}");
    }
    if let Some(value) = text.strip_prefix("current model: ") {
        return format!("当前模型: {value}");
    }
    if let Some(value) = text.strip_prefix("available models: ") {
        return format!("可用模型: {value}");
    }
    if let Some(value) = text.strip_prefix("invalid /mouse args: ") {
        return format!("无效的 /mouse 参数: {value}");
    }
    if let Some(value) = text.strip_prefix("invalid mode: ") {
        return format!("非法模式: {value}");
    }
    if let Some(value) = text.strip_prefix("invalid approval mode: ") {
        return format!("非法审批模式: {value}");
    }
    if let Some(value) = text.strip_prefix("no files found for: ") {
        return format!("未找到文件: {value}");
    }
    if let Some(value) = text.strip_prefix("mention results (") {
        return format!("搜索结果 ({value}");
    }
    if let Some(value) = text.strip_prefix("extra prompt saved (") {
        return format!("额外提示词已保存 ({value}");
    }
    if let Some(value) = text.strip_prefix("tool_call_mode set: ") {
        return format!("工具调用模式已设置: {value}");
    }
    if let Some(value) = text.strip_prefix("approval_mode set: ") {
        return format!("审批模式已设置: {value}");
    }
    if let Some(value) = text.strip_prefix("model not found: ") {
        return format!("模型不存在: {value}");
    }
    if let Some(value) = text.strip_prefix("session index out of range: ") {
        return format!("会话索引越界: {value}");
    }
    if let Some(value) = text.strip_prefix("unknown command: ") {
        return format!("未知命令: {value}");
    }
    if let Some(value) = text.strip_prefix("model not found in config: ") {
        return format!("配置中不存在模型: {value}");
    }
    if let Some(value) = text.strip_prefix("- provider: ") {
        return format!("- 提供商: {value}");
    }
    if let Some(value) = text.strip_prefix("- base_url: ") {
        return format!("- base_url: {value}");
    }
    if let Some(value) = text.strip_prefix("- model: ") {
        return format!("- 模型: {value}");
    }
    if let Some(value) = text.strip_prefix("- tool_call_mode: ") {
        return format!("- 工具调用模式: {value}");
    }
    if let Some(value) = text.strip_prefix("- session: ") {
        return format!("- 会话: {value}");
    }
    if let Some(value) = text.strip_prefix("- id: ") {
        return format!("- 会话 ID: {value}");
    }
    if let Some(value) = text.strip_prefix("review task cancelled: ") {
        return format!("review 任务已取消: {value}");
    }
    if let Some(value) = text.strip_prefix("- extra_prompt: enabled (") {
        return format!("- 额外提示词: 已启用（{value}");
    }

    match text {
        "wunder-cli tui mode. type /help for commands." => {
            "wunder-cli TUI 模式。输入 /help 查看命令。".to_string()
        }
        "no historical sessions found" => "未找到历史会话".to_string(),
        "tip: start chatting first, then use /resume to switch" => {
            "提示：先发起对话，再用 /resume 切换。".to_string()
        }
        "tip: send a few messages first, then /resume to switch" => {
            "提示：先发送几条消息，再用 /resume 切换。".to_string()
        }
        "focus switched to input" => "焦点已切换到输入区".to_string(),
        "focus switched to output (arrows now select transcript)" => {
            "焦点已切换到输出区（方向键可选择日志）".to_string()
        }
        "assistant is still running, wait for completion before creating a new session" => {
            "助手仍在运行，请等待完成后再新建会话".to_string()
        }
        "assistant is still running, wait for completion before sending a new prompt" => {
            "助手仍在运行，请等待完成后再发送新消息".to_string()
        }
        "assistant is still running, wait for completion before running /review" => {
            "助手仍在运行，请等待完成后再执行 /review".to_string()
        }
        "assistant is still running, wait for completion before resuming another session" => {
            "助手仍在运行，请等待完成后再恢复其他会话".to_string()
        }
        "interrupt requested, waiting for running round to stop..." => {
            "已请求中断，等待当前轮次停止...".to_string()
        }
        "no cancellable round found, press Ctrl+C again to exit" => {
            "未找到可中断轮次，再按一次 Ctrl+C 退出".to_string()
        }
        "press Ctrl+C again to exit (or wait to continue)" => {
            "再按一次 Ctrl+C 退出（或等待继续）".to_string()
        }
        "usage: /mention <query>" => "用法: /mention <query>".to_string(),
        "usage: /approvals [show|suggest|auto_edit|full_auto]" => {
            "用法: /approvals [show|suggest|auto_edit|full_auto]".to_string()
        }
        "valid modes: suggest, auto_edit, full_auto" => {
            "可选模式: suggest, auto_edit, full_auto".to_string()
        }
        "usage: /tool-call-mode <tool_call|function_call> [model]" => {
            "用法: /tool-call-mode <tool_call|function_call> [model]".to_string()
        }
        "valid modes: tool_call, function_call" => "可选模式: tool_call, function_call".to_string(),
        "too many arguments" => "参数过多".to_string(),
        "config values cannot be empty" => "配置值不能为空".to_string(),
        "configure llm model (step 1/4)" => "配置 LLM 模型（步骤 1/4）".to_string(),
        "config cancelled" => "配置已取消".to_string(),
        "input base_url (empty line to cancel)" => "请输入 base_url（空行取消）".to_string(),
        "input api_key (step 2/4)" => "请输入 api_key（步骤 2/4）".to_string(),
        "input model name (step 3/4)" => "请输入模型名称（步骤 3/4）".to_string(),
        "input max_context (step 4/4, optional; Enter for auto probe)" => {
            "请输入 max_context（步骤 4/4，可选；回车自动探测）".to_string()
        }
        "model configured" => "模型配置完成".to_string(),
        "- max_context: auto probe unavailable (or keep existing)" => {
            "- max_context: 自动探测不可用（或保留现有值）".to_string()
        }
        "mouse mode: scroll (wheel enabled)" => "鼠标模式：scroll（启用滚轮）".to_string(),
        "mouse mode: select/copy (wheel disabled)" => {
            "鼠标模式：select/copy（禁用滚轮）".to_string()
        }
        "usage: /mouse [scroll|select]  (F2 to toggle)" => {
            "用法: /mouse [scroll|select]  （F2 切换）".to_string()
        }
        "resume picker opened (Up/Down to choose, Enter to resume, Esc to cancel)" => {
            "已打开会话恢复面板（上下选择，Enter 恢复，Esc 取消）".to_string()
        }
        "tip: /resume to list available sessions" => "提示: 用 /resume 列出可用会话".to_string(),
        "tip: run /resume list to inspect available sessions" => {
            "提示: 运行 /resume list 查看可用会话".to_string()
        }
        "type /help to list available slash commands" => {
            "输入 /help 查看可用 slash 命令".to_string()
        }
        "available models:" => "可用模型：".to_string(),
        "no models configured. run /config first." => "尚未配置模型，请先运行 /config".to_string(),
        "no saved session found" => "未找到保存的会话".to_string(),
        "no llm model configured" => "尚未配置 LLM 模型".to_string(),
        "session id is empty" => "会话 ID 不能为空".to_string(),
        "extra prompt cleared" => "额外提示词已清除".to_string(),
        "extra prompt is empty" => "额外提示词为空".to_string(),
        "- extra_prompt: none" => "- 额外提示词: 无".to_string(),
        "usage: /system [set <extra_prompt>|clear]" => {
            "用法: /system [set <extra_prompt>|clear]".to_string()
        }
        "invalid /system args" => "无效的 /system 参数".to_string(),
        "system" => "系统提示词".to_string(),
        "--- system prompt ---" => "--- 系统提示词开始 ---".to_string(),
        "--- end system prompt ---" => "--- 系统提示词结束 ---".to_string(),
        "stream ended without model output or final answer" => {
            "流式输出结束，但未收到模型输出或最终答案".to_string()
        }
        _ => text.to_string(),
    }
}

fn build_workspace_file_index(root: &std::path::Path) -> Vec<IndexedFile> {
    const MAX_INDEX_FILES: usize = 50_000;
    let excluded_dirs = [
        ".git",
        "target",
        "WUNDER_TEMP",
        "data",
        "frontend",
        "web",
        "node_modules",
        "参考项目",
        "backups",
    ];
    let mut items = Vec::new();
    let walker = walkdir::WalkDir::new(root).follow_links(false);
    for entry in walker
        .into_iter()
        .filter_entry(|entry| {
            let path = entry.path();
            if path == root {
                return true;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                return true;
            };
            !excluded_dirs
                .iter()
                .any(|excluded| name.eq_ignore_ascii_case(excluded))
        })
        .filter_map(|entry| entry.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let rel = relative.to_string_lossy().replace('\\', "/");
        if rel.is_empty() {
            continue;
        }
        items.push(IndexedFile {
            lowered: rel.to_ascii_lowercase(),
            path: rel,
        });
        if items.len() >= MAX_INDEX_FILES {
            break;
        }
    }
    items.sort_by(|left, right| left.path.cmp(&right.path));
    items
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
    fn sanitize_assistant_delta_streaming_strips_split_tool_call_block() {
        let mut in_tool_markup = false;
        let first = sanitize_assistant_delta_streaming(
            "<tool_call>{\"name\":\"final_reply\",\"arguments\":{\"content\":\"",
            &mut in_tool_markup,
        );
        assert!(first.is_empty());
        assert!(in_tool_markup);

        let second = sanitize_assistant_delta_streaming(
            "hello\"}}</tool_call>hello world",
            &mut in_tool_markup,
        );
        assert_eq!(second, "hello world");
        assert!(!in_tool_markup);
    }

    #[test]
    fn merge_stream_text_reuses_snapshot_without_duplicate_append() {
        let mut output = "hello".to_string();
        merge_stream_text(&mut output, "hello world");
        assert_eq!(output, "hello world");

        merge_stream_text(&mut output, "world");
        assert_eq!(output, "hello world");
    }

    #[test]
    fn equivalent_text_ignores_whitespace_differences() {
        assert!(is_equivalent_text("hello  world", "hello world"));
        assert!(is_equivalent_text("- run command", "-run command"));
    }

    #[test]
    fn payload_has_tool_calls_accepts_non_empty_array() {
        let payload = serde_json::json!({ "tool_calls": [{ "name": "读取文件" }] });
        assert!(payload_has_tool_calls(&payload));
    }

    #[test]
    fn format_execute_command_result_lines_prioritizes_failure_output() {
        let payload = serde_json::json!({
            "data": {
                "results": [{
                    "command": "pip list",
                    "returncode": 1,
                    "stdout": "",
                    "stderr": "pip is not recognized as a cmdlet
        at line:1 char:1"
                }]
            },
            "meta": {
                "duration_ms": 15,
                "exit_code": 1
            }
        });

        let lines = format_execute_command_result_lines("exec", &payload);
        assert!(!lines.is_empty());
        assert!(lines[0].contains("failed"));
        assert!(lines.iter().any(|line| line.starts_with("  stderr:")));
        assert!(!lines
            .iter()
            .any(|line| line.starts_with("  output: <empty>")));
    }

    #[test]
    fn append_text_preview_truncates_long_output() {
        let mut lines = Vec::new();
        let value = "line1\nline2\nline3\nline4\nline5\nline6\nline7\n";
        let has_output = append_text_preview(&mut lines, "stdout", value, 4, 64);
        assert!(has_output);
        assert!(lines.iter().any(|line| line.contains("more lines")));
    }

    #[test]
    fn compact_json_handles_multibyte_truncation() {
        let value = serde_json::json!({ "message": "你".repeat(400) });
        let output = compact_json(&value);
        assert!(output.ends_with("..."));
    }
}
