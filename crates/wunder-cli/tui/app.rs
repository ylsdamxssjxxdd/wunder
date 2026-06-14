use super::frame_scheduler::FrameRequester;
use anyhow::{anyhow, Result};
use chrono::TimeZone;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use futures::StreamExt;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{self, error::TryRecvError, UnboundedReceiver};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use wunder_server::approval::{
    new_channel as new_approval_channel, ApprovalRequest, ApprovalRequestKind, ApprovalRequestRx,
    ApprovalResponse,
};
use wunder_server::schemas::StreamEvent;
use wunder_server::user_tools::UserMcpServer;

use crate::args::GlobalArgs;
use crate::render::FinalEvent;
use crate::runtime::CliRuntime;
use crate::slash_command::{self, ParsedSlashCommand, SlashCommand};

const MAX_LOG_ENTRIES: usize = 1200;
const MAX_LOG_TOTAL_CHARS: usize = 320_000;
const MAX_DRAIN_MESSAGES_PER_TICK_BASE: usize = 400;
const MAX_DRAIN_MESSAGES_PER_TICK_CATCHUP: usize = 1400;
const STREAM_CATCHUP_ENTER_DEPTH: usize = 120;
const STREAM_CATCHUP_EXIT_DEPTH: usize = 48;
const STREAM_CATCHUP_ENTER_HOLD: Duration = Duration::from_millis(120);
const STREAM_CATCHUP_EXIT_HOLD: Duration = Duration::from_millis(260);
const CTRL_C_EXIT_WINDOW: Duration = Duration::from_millis(1500);
const MAX_PERSISTED_HISTORY: usize = 200;
const MAX_HISTORY_ENTRY_CHARS: usize = 4000;
const MAX_PERSISTED_POPUP_RECENTS: usize = 120;
const POPUP_VISIBLE_LIMIT: usize = 7;
const POPUP_MAX_CANDIDATES: usize = 120;
const LARGE_PASTE_CHAR_THRESHOLD: usize = 1000;
#[cfg(not(windows))]
const PASTE_BURST_CHAR_GAP: Duration = Duration::from_millis(8);
#[cfg(windows)]
const PASTE_BURST_CHAR_GAP: Duration = Duration::from_millis(30);
#[cfg(not(windows))]
const PASTE_BURST_ACTIVE_IDLE_TIMEOUT: Duration = Duration::from_millis(8);
#[cfg(windows)]
const PASTE_BURST_ACTIVE_IDLE_TIMEOUT: Duration = Duration::from_millis(60);
const PASTE_BURST_ENTER_GAP: Duration = Duration::from_millis(120);
const PASTE_BURST_ENTER_CHAR_THRESHOLD: usize = 3;
const PASTE_BURST_CAPTURE_CHAR_THRESHOLD: usize = 3;
const CLIPBOARD_PASTE_PROBE_CACHE_TTL: Duration = Duration::from_millis(400);
const SUPPRESSED_CLIPBOARD_PASTE_TIMEOUT: Duration = Duration::from_millis(1200);
mod commands;
mod input_placeholders;

pub(super) mod helpers;
mod patch_log;

use helpers::*;
use input_placeholders::*;
use patch_log::*;

const STATUSLINE_ITEM_KEYS: &[&str] = &[
    "running", "usage", "scroll", "mouse", "focus", "context", "cwd", "project", "branch",
    "session", "model", "mode", "approval", "agent", "attach", "elapsed", "speed", "tools",
];

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
    special: Option<SpecialLogEntry>,
    markdown_cache: Option<MarkdownCache>,
}

#[derive(Debug, Clone)]
struct MarkdownCache {
    width: u16,
    lines: Vec<Line<'static>>,
}

#[derive(Debug)]
struct StreamedMarkdownState {
    index: usize,
    wrap_width: usize,
    collector: super::markdown_stream::MarkdownStreamCollector,
}

#[derive(Debug, Clone, Copy)]
pub struct TranscriptRenderEntry {
    pub global_index: usize,
}

#[derive(Debug, Clone)]
pub struct TranscriptRenderWindow {
    pub entries: Vec<TranscriptRenderEntry>,
    pub local_scroll: u16,
    pub total_lines: usize,
}

#[derive(Debug, Clone)]
pub struct PopupView {
    pub lines: Vec<String>,
    pub selected_index: Option<usize>,
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
    Auto,
    Scroll,
    Select,
}

impl MouseMode {
    fn captures_mouse(self) -> bool {
        matches!(self, Self::Scroll)
    }
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

#[derive(Debug, Clone)]
struct InquiryRoute {
    label: String,
    description: Option<String>,
    recommended: bool,
}

#[derive(Debug, Clone)]
struct InquiryPanelState {
    question: String,
    routes: Vec<InquiryRoute>,
    multiple: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct InputHistoryStore {
    entries: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct PopupRecentStore {
    items: Vec<String>,
}

#[derive(Debug, Clone)]
struct IndexedFile {
    path: String,
    lowered: String,
}

pub struct TuiApp {
    runtime: CliRuntime,
    global: GlobalArgs,
    frame_requester: FrameRequester,
    display_language: String,
    session_id: String,
    input: String,
    input_cursor: usize,
    input_viewport_width: u16,
    transcript_viewport_width: u16,
    transcript_viewport_height: u16,
    transcript_rendered_lines: usize,
    history_archived_entries: usize,
    pending_scrollback_lines: Vec<Line<'static>>,
    logs: Vec<LogEntry>,
    busy: bool,
    should_quit: bool,
    history: Vec<String>,
    history_cursor: Option<usize>,
    history_draft: String,
    pending_attachments: Vec<crate::attachments::PreparedAttachment>,
    pending_attachment_paths: VecDeque<String>,
    pending_paste: VecDeque<String>,
    pending_large_pastes: Vec<(String, String)>,
    large_paste_counters: HashMap<usize, usize>,
    workspace_files: Vec<IndexedFile>,
    active_assistant: Option<usize>,
    active_reasoning: Option<usize>,
    stream_rx: Option<UnboundedReceiver<StreamMessage>>,
    approval_rx: Option<ApprovalRequestRx>,
    approval_queue: VecDeque<ApprovalRequest>,
    active_approval: Option<ApprovalRequest>,
    approval_selected_index: usize,
    approval_mode: String,
    agent_id_override: Option<String>,
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
    stream_goal_continue_ready: bool,
    stream_received_content_delta: bool,
    stream_tool_markup_open: bool,
    turn_final_answer: String,
    turn_final_stop_reason: Option<String>,
    transcript_offset_from_bottom: usize,
    session_stats_dirty: bool,
    shortcuts_visible: bool,
    mouse_mode: MouseMode,
    focus_area: FocusArea,
    transcript_selected: Option<usize>,
    resume_picker: Option<ResumePickerState>,
    active_inquiry_panel: Option<InquiryPanelState>,
    inquiry_selected_index: usize,
    tool_phase_notice_emitted: bool,
    stream_catchup_mode: bool,
    stream_catchup_enter_since: Option<Instant>,
    stream_catchup_exit_since: Option<Instant>,
    terminal_focused: bool,
    key_char_burst_len: usize,
    key_char_burst_last_at: Option<Instant>,
    key_paste_burst_buffer: String,
    key_paste_burst_last_at: Option<Instant>,
    clipboard_probe_cache: Option<(Instant, Option<String>)>,
    suppressed_clipboard_paste: String,
    suppressed_clipboard_paste_last_at: Option<Instant>,
    statusline_items: Vec<String>,
    workspace_project_name: Option<String>,
    workspace_git_branch: Option<String>,
    mouse_passthrough_until: Option<Instant>,
    transcript_mouse_region: Rect,
    input_mouse_region: Rect,
    app_hints: Vec<String>,
    skill_hints: Vec<String>,
    enabled_skill_names: HashSet<String>,
    popup_recents: Vec<String>,
    popup_selected_index: usize,
    popup_signature: String,
    turn_llm_started_at: Option<Instant>,
    turn_llm_active_secs: f64,
    turn_output_tokens: u64,
    turn_tool_calls: u64,
    last_turn_elapsed_secs: Option<f64>,
    last_turn_speed_tps: Option<f64>,
    last_turn_tool_calls: u64,
    assistant_markdown_stream: Option<StreamedMarkdownState>,
    reasoning_markdown_stream: Option<StreamedMarkdownState>,
}

impl TuiApp {
    pub async fn new(
        runtime: CliRuntime,
        global: GlobalArgs,
        session_override: Option<String>,
        frame_requester: FrameRequester,
    ) -> Result<Self> {
        let session_id =
            session_override.unwrap_or_else(|| runtime.resolve_session(global.session.as_deref()));
        runtime.save_session(&session_id).ok();
        let display_language = crate::locale::resolve_cli_language(&global);
        let initial_agent_id_override = global
            .agent
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);

        let mut app = Self {
            runtime,
            global,
            frame_requester,
            display_language,
            session_id,
            input: String::new(),
            input_cursor: 0,
            input_viewport_width: 1,
            transcript_viewport_width: 1,
            transcript_viewport_height: 1,
            transcript_rendered_lines: 0,
            history_archived_entries: 0,
            pending_scrollback_lines: Vec::new(),
            logs: Vec::new(),
            busy: false,
            should_quit: false,
            history: Vec::new(),
            history_cursor: None,
            history_draft: String::new(),
            pending_attachments: Vec::new(),
            pending_attachment_paths: VecDeque::new(),
            pending_paste: VecDeque::new(),
            pending_large_pastes: Vec::new(),
            large_paste_counters: HashMap::new(),
            workspace_files: Vec::new(),
            active_assistant: None,
            active_reasoning: None,
            stream_rx: None,
            approval_rx: None,
            approval_queue: VecDeque::new(),
            active_approval: None,
            approval_selected_index: 0,
            approval_mode: "full_auto".to_string(),
            agent_id_override: initial_agent_id_override,
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
            stream_goal_continue_ready: false,
            stream_received_content_delta: false,
            stream_tool_markup_open: false,
            turn_final_answer: String::new(),
            turn_final_stop_reason: None,
            transcript_offset_from_bottom: 0,
            session_stats_dirty: false,
            shortcuts_visible: false,
            mouse_mode: MouseMode::Auto,
            focus_area: FocusArea::Input,
            transcript_selected: None,
            resume_picker: None,
            active_inquiry_panel: None,
            inquiry_selected_index: 0,
            tool_phase_notice_emitted: false,
            stream_catchup_mode: false,
            stream_catchup_enter_since: None,
            stream_catchup_exit_since: None,
            terminal_focused: true,
            key_char_burst_len: 0,
            key_char_burst_last_at: None,
            key_paste_burst_buffer: String::new(),
            key_paste_burst_last_at: None,
            clipboard_probe_cache: None,
            suppressed_clipboard_paste: String::new(),
            suppressed_clipboard_paste_last_at: None,
            statusline_items: Vec::new(),
            workspace_project_name: None,
            workspace_git_branch: None,
            mouse_passthrough_until: None,
            transcript_mouse_region: Rect::default(),
            input_mouse_region: Rect::default(),
            app_hints: Vec::new(),
            skill_hints: Vec::new(),
            enabled_skill_names: HashSet::new(),
            popup_recents: Vec::new(),
            popup_selected_index: 0,
            popup_signature: String::new(),
            turn_llm_started_at: None,
            turn_llm_active_secs: 0.0,
            turn_output_tokens: 0,
            turn_tool_calls: 0,
            last_turn_elapsed_secs: None,
            last_turn_speed_tps: None,
            last_turn_tool_calls: 0,
            assistant_markdown_stream: None,
            reasoning_markdown_stream: None,
        };
        app.load_persisted_history();
        app.load_popup_recents();
        app.load_statusline_items();
        app.refresh_workspace_context();
        if !app.global.attachments.is_empty() {
            for raw in app.global.attachments.clone() {
                match crate::attachments::prepare_attachment_from_path(&app.runtime, raw.as_str())
                    .await
                {
                    Ok(prepared) => app.queue_prepared_attachment(prepared, false),
                    Err(err) => {
                        if app.is_zh_language() {
                            app.push_log(LogKind::Error, format!("预加载附件失败: {raw} ({err})"));
                        } else {
                            app.push_log(
                                LogKind::Error,
                                format!("failed to preload attachment: {raw} ({err})"),
                            );
                        }
                    }
                }
            }
        }
        app.workspace_files = tokio::task::spawn_blocking({
            let root = app.runtime.launch_dir.clone();
            move || build_workspace_file_index(&root)
        })
        .await
        .unwrap_or_default();
        app.reload_popup_catalogs().await;
        app.sync_model_status().await;
        app.reload_session_stats().await;
        let logo = crate::welcome_logo::render_for_terminal();
        app.push_special_log(
            LogKind::Tool,
            logo.plain_text().to_string(),
            build_static_art_log(logo.plain_text().to_string(), logo.tui_lines()),
        );
        app.push_log(
            LogKind::Info,
            "wunder-cli tui mode. type /help for commands.".to_string(),
        );
        if !app.pending_attachments.is_empty() {
            app.push_log(
                LogKind::Info,
                crate::locale::tr(
                    app.display_language.as_str(),
                    "已预加载附件队列（下一轮自动发送）",
                    "preloaded attachment queue (auto-sent on next turn)",
                ),
            );
            let lines = app
                .pending_attachments
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    crate::attachments::summarize_attachment(
                        item,
                        index,
                        app.display_language.as_str(),
                    )
                })
                .collect::<Vec<_>>();
            for line in lines {
                app.push_log(LogKind::Info, line);
            }
        }
        Ok(app)
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn is_zh_language(&self) -> bool {
        crate::locale::is_zh_language(self.display_language.as_str())
    }

    pub fn status_line(&self) -> String {
        let parts = self.status_line_parts();
        if self.statusline_items.is_empty() {
            let mut items = Vec::new();
            for key in ["cwd", "branch", "elapsed", "speed", "tools", "context"] {
                if let Some(value) = parts.get(key) {
                    let value = value.trim();
                    if !value.is_empty() {
                        items.push(value.to_string());
                    }
                }
            }
            if items.is_empty() {
                return "  -".to_string();
            }
            return format!("  {}", items.join(" | "));
        }

        let mut items = Vec::new();
        for key in &self.statusline_items {
            if let Some(value) = parts.get(key.as_str()) {
                let value = value.trim();
                if !value.is_empty() {
                    items.push(value.to_string());
                }
            }
        }
        if items.is_empty() {
            if self.is_zh_language() {
                return "  状态栏：当前没有启用条目，可输入 /statusline reset 恢复默认值"
                    .to_string();
            }
            return "  status line: empty selection, run /statusline reset".to_string();
        }
        format!("  {}", items.join(" | "))
    }

    pub fn request_redraw(&self) {
        self.frame_requester.schedule_frame();
    }

    pub fn schedule_periodic_redraw_if_needed(&self) {
        let ctrl_c_pending = self
            .ctrl_c_hint_deadline
            .is_some_and(|deadline| Instant::now() <= deadline);
        if self.busy || self.stream_rx.is_some() {
            self.frame_requester
                .schedule_frame_in(super::activity_indicator::RUNNING_ANIMATION_FRAME);
            return;
        }
        if self.key_paste_burst_active() {
            self.frame_requester.schedule_frame_in(PASTE_BURST_CHAR_GAP);
            return;
        }
        if self.approval_rx.is_some() || self.active_approval.is_some() || ctrl_c_pending {
            self.frame_requester
                .schedule_frame_in(Duration::from_millis(90));
        }
    }

    pub fn input_focus_active(&self) -> bool {
        self.focus_area == FocusArea::Input
    }

    pub fn input_is_empty(&self) -> bool {
        self.input.is_empty()
    }

    pub fn activity_highlighted(&self) -> bool {
        self.active_approval.is_some()
            || self.active_inquiry_panel.is_some()
            || self.resume_picker.is_some()
            || self
                .ctrl_c_hint_deadline
                .is_some_and(|deadline| Instant::now() <= deadline)
    }

    pub fn activity_line(&self) -> String {
        let is_zh = self.is_zh_language();
        if self
            .ctrl_c_hint_deadline
            .is_some_and(|deadline| Instant::now() <= deadline)
        {
            return if is_zh {
                "再按一次 Ctrl+C 可退出当前终端会话".to_string()
            } else {
                "Press Ctrl+C again to exit this terminal session".to_string()
            };
        }

        if self.resume_picker.is_some() {
            return if is_zh {
                "会话恢复面板已打开 · Enter 恢复 · Esc 取消".to_string()
            } else {
                "Resume picker open · Enter resumes · Esc cancels".to_string()
            };
        }

        if self.active_approval.is_some() {
            let queued = self.approval_queue.len();
            return if is_zh {
                if queued > 0 {
                    format!("审批待确认 · Enter 确认 · Y/A/N 快速选择 · 待处理 {queued}")
                } else {
                    "审批待确认 · Enter 确认 · Y/A/N 快速选择".to_string()
                }
            } else if queued > 0 {
                format!("Approval pending · Enter confirms · Y/A/N quick choose · queued={queued}")
            } else {
                "Approval pending · Enter confirms · Y/A/N quick choose".to_string()
            };
        }

        if self.active_inquiry_panel.is_some() {
            return if is_zh {
                "路由面板已打开 · ↑↓ 选择 · Enter 发送 · Esc 关闭".to_string()
            } else {
                "Inquiry panel open · Up/Down select · Enter send · Esc close".to_string()
            };
        }

        if self.busy {
            let elapsed_secs =
                current_activity_elapsed_secs(self.turn_llm_started_at, self.turn_llm_active_secs);
            return format_busy_activity_line(
                is_zh,
                elapsed_secs,
                self.stream_catchup_mode,
                self.terminal_focused,
            );
        }

        String::new()
    }

    pub fn composer_hint_line(&self) -> String {
        let elapsed_label = self
            .status_elapsed_secs()
            .map(|value| format!("{value:.2} s"))
            .unwrap_or_else(|| "-".to_string());
        let speed_label = self
            .status_speed_tps()
            .map(|value| format!("{value:.2} token/s"))
            .unwrap_or_else(|| "-".to_string());
        let tool_calls = self.status_tool_calls();
        if self.is_zh_language() {
            format!("耗时={elapsed_label} | 速度={speed_label} | 工具={tool_calls}")
        } else {
            format!("elapsed={elapsed_label} | speed={speed_label} | tools={tool_calls}")
        }
    }

    pub fn composer_footer_items(&self) -> Vec<(String, String)> {
        let mut items = Vec::new();

        if !self.model_name.trim().is_empty() && self.model_name != "<none>" {
            items.push((String::new(), self.model_name.clone()));
        }

        let cwd_display = crate::path_display::format_directory_display(
            self.runtime.launch_dir.as_path(),
            Some(self.runtime.repo_root.as_path()),
            Some(28),
        );
        if !cwd_display.trim().is_empty() {
            items.push((String::new(), cwd_display));
        }

        items
    }

    pub fn composer_footer_context(&self) -> Option<String> {
        Some(format_footer_context_summary(
            self.is_zh_language(),
            self.session_stats.context_used_tokens,
            self.model_max_context,
        ))
    }

    pub fn inline_input_placeholders(&self) -> Vec<String> {
        let mut placeholders = attachment_placeholders(self.pending_attachments.as_slice());
        placeholders.extend(self.large_paste_placeholders());
        placeholders
    }

    pub fn shortcuts_visible(&self) -> bool {
        self.shortcuts_visible
    }

    pub fn set_terminal_focus(&mut self, focused: bool) {
        self.terminal_focused = focused;
    }

    pub fn mouse_capture_enabled(&self) -> bool {
        self.mouse_mode.captures_mouse()
    }

    fn refresh_workspace_context(&mut self) {
        self.workspace_project_name =
            crate::workspace_context::project_root_name(self.runtime.repo_root.as_path());
        self.workspace_git_branch =
            crate::workspace_context::read_git_branch(self.runtime.repo_root.as_path());
    }

    fn status_line_parts(&self) -> std::collections::HashMap<&'static str, String> {
        let mut parts = std::collections::HashMap::new();
        let is_zh = self.is_zh_language();
        let cwd_display = crate::path_display::format_directory_display(
            self.runtime.launch_dir.as_path(),
            Some(self.runtime.repo_root.as_path()),
            Some(32),
        );
        let project_display = self.workspace_project_name.as_deref();
        let branch_display = self
            .workspace_git_branch
            .as_deref()
            .map(|value| crate::workspace_context::format_branch_display(value, 18));
        let context_summary = if let Some(max_context) = self.model_max_context {
            let percent_left = crate::context_left_percent(
                self.session_stats.context_used_tokens,
                Some(max_context),
            )
            .unwrap_or(0);
            if is_zh {
                format!("上下文余量={percent_left}%")
            } else {
                format!("context_left={percent_left}%")
            }
        } else {
            let used = self.session_stats.context_used_tokens.max(0);
            if is_zh {
                format!("上下文占用={used}")
            } else {
                format!("context_used={used}")
            }
        };
        let running_hint = if self.resume_picker.is_some() {
            if is_zh {
                "状态=恢复会话".to_string()
            } else {
                "state=resume_picker".to_string()
            }
        } else if self.active_approval.is_some() {
            if is_zh {
                "状态=等待审批".to_string()
            } else {
                "state=approval_pending".to_string()
            }
        } else if self.busy {
            if is_zh {
                "状态=执行中".to_string()
            } else {
                "state=working".to_string()
            }
        } else if is_zh {
            "状态=空闲".to_string()
        } else {
            "state=idle".to_string()
        };
        let usage_hint = self
            .last_usage
            .as_deref()
            .map(|value| format!("tokens={value}"))
            .unwrap_or_else(|| "tokens=-".to_string());
        let scroll_hint = if self.transcript_offset_from_bottom > 0 {
            if is_zh {
                format!("滚动=-{}", self.transcript_offset_from_bottom)
            } else {
                format!("scroll=-{}", self.transcript_offset_from_bottom)
            }
        } else if is_zh {
            "滚动=0".to_string()
        } else {
            "scroll=0".to_string()
        };
        let mouse_hint = match self.mouse_mode {
            MouseMode::Auto => {
                if is_zh {
                    "鼠标=自动(终端原生)".to_string()
                } else {
                    "mouse=auto(native)".to_string()
                }
            }
            MouseMode::Scroll => {
                if is_zh {
                    "鼠标=滚动(接管)".to_string()
                } else {
                    "mouse=scroll(captured)".to_string()
                }
            }
            MouseMode::Select => {
                if is_zh {
                    "鼠标=选择复制".to_string()
                } else {
                    "mouse=select/copy".to_string()
                }
            }
        };
        let focus_hint = match self.focus_area {
            FocusArea::Input => {
                if is_zh {
                    "焦点=输入".to_string()
                } else {
                    "focus=input".to_string()
                }
            }
            FocusArea::Transcript => {
                if is_zh {
                    "焦点=输出".to_string()
                } else {
                    "focus=output".to_string()
                }
            }
        };
        let elapsed_label = self
            .status_elapsed_secs()
            .map(|value| format!("{value:.2} s"))
            .unwrap_or_else(|| "-".to_string());
        let speed_label = self
            .status_speed_tps()
            .map(|value| format!("{value:.2} token/s"))
            .unwrap_or_else(|| "-".to_string());
        let tool_calls = self.status_tool_calls();

        parts.insert("running", running_hint);
        parts.insert("usage", usage_hint);
        parts.insert("scroll", scroll_hint);
        parts.insert("mouse", mouse_hint);
        parts.insert("focus", focus_hint);
        parts.insert("context", context_summary);
        parts.insert(
            "cwd",
            if is_zh {
                format!("目录={cwd_display}")
            } else {
                format!("cwd={cwd_display}")
            },
        );
        if let Some(project_display) = project_display.filter(|value| !value.trim().is_empty()) {
            parts.insert(
                "project",
                if is_zh {
                    format!("项目={project_display}")
                } else {
                    format!("project={project_display}")
                },
            );
        }
        if let Some(branch_display) = branch_display.filter(|value| !value.trim().is_empty()) {
            parts.insert(
                "branch",
                if is_zh {
                    format!("分支={branch_display}")
                } else {
                    format!("branch={branch_display}")
                },
            );
        }
        parts.insert(
            "attach",
            if is_zh {
                format!("附件={}", self.pending_attachments.len())
            } else {
                format!("attach={}", self.pending_attachments.len())
            },
        );
        parts.insert(
            "session",
            if is_zh {
                format!("会话={}", self.session_id)
            } else {
                format!("session={}", self.session_id)
            },
        );
        parts.insert(
            "agent",
            if is_zh {
                format!("Agent={}", self.agent_id_override.as_deref().unwrap_or("-"))
            } else {
                format!("agent={}", self.agent_id_override.as_deref().unwrap_or("-"))
            },
        );
        parts.insert(
            "model",
            if is_zh {
                format!("模型={}", self.model_name)
            } else {
                format!("model={}", self.model_name)
            },
        );
        parts.insert(
            "mode",
            if is_zh {
                format!("模式={}", self.tool_call_mode)
            } else {
                format!("mode={}", self.tool_call_mode)
            },
        );
        parts.insert(
            "approval",
            if is_zh {
                format!("审批={}", self.approval_mode)
            } else {
                format!("approval={}", self.approval_mode)
            },
        );
        parts.insert(
            "elapsed",
            if is_zh {
                format!("耗时={elapsed_label}")
            } else {
                format!("elapsed={elapsed_label}")
            },
        );
        parts.insert(
            "speed",
            if is_zh {
                format!("速度={speed_label}")
            } else {
                format!("speed={speed_label}")
            },
        );
        parts.insert(
            "tools",
            if is_zh {
                format!("工具={tool_calls}")
            } else {
                format!("tools={tool_calls}")
            },
        );
        parts
    }

    fn status_elapsed_secs(&self) -> Option<f64> {
        self.last_turn_elapsed_secs
    }

    fn status_speed_tps(&self) -> Option<f64> {
        self.last_turn_speed_tps
    }

    fn status_tool_calls(&self) -> u64 {
        self.last_turn_tool_calls
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
            .filter(|item| should_store_history_entry(item))
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

    fn popup_recent_file(&self) -> PathBuf {
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
            .join(format!("sessions/popup_recent_{name}.json"))
    }

    fn load_popup_recents(&mut self) {
        let path = self.popup_recent_file();
        let Ok(text) = fs::read_to_string(path) else {
            return;
        };
        let Ok(store) = serde_json::from_str::<PopupRecentStore>(&text) else {
            return;
        };
        self.popup_recents = dedupe_case_insensitive(store.items)
            .into_iter()
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .take(MAX_PERSISTED_POPUP_RECENTS)
            .collect();
    }

    fn persist_popup_recents(&self) {
        let path = self.popup_recent_file();
        if let Some(parent) = path.parent() {
            if fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        let store = PopupRecentStore {
            items: self.popup_recents.clone(),
        };
        let Ok(payload) = serde_json::to_vec_pretty(&store) else {
            return;
        };
        let _ = fs::write(path, payload);
    }

    fn mark_popup_recent(&mut self, token: &str) {
        let cleaned = token.trim();
        if cleaned.is_empty() {
            return;
        }
        self.popup_recents
            .retain(|item| !item.eq_ignore_ascii_case(cleaned));
        self.popup_recents.insert(0, cleaned.to_string());
        if self.popup_recents.len() > MAX_PERSISTED_POPUP_RECENTS {
            self.popup_recents.truncate(MAX_PERSISTED_POPUP_RECENTS);
        }
        self.persist_popup_recents();
    }

    fn statusline_file(&self) -> PathBuf {
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
            .join(format!("config/statusline_{name}.json"))
    }

    fn load_statusline_items(&mut self) {
        let path = self.statusline_file();
        let Ok(text) = fs::read_to_string(path) else {
            return;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            return;
        };
        let Some(items) = value.get("items").and_then(Value::as_array) else {
            return;
        };
        let mut seen = std::collections::HashSet::new();
        self.statusline_items = items
            .iter()
            .filter_map(Value::as_str)
            .filter_map(normalize_statusline_item)
            .filter(|item| seen.insert(item.clone()))
            .collect();
    }

    fn persist_statusline_items(&self) {
        let path = self.statusline_file();
        if let Some(parent) = path.parent() {
            if fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        let payload = json!({ "items": self.statusline_items });
        let Ok(text) = serde_json::to_vec_pretty(&payload) else {
            return;
        };
        let _ = fs::write(path, text);
    }

    async fn reload_popup_catalogs(&mut self) {
        let config = self.runtime.state.config_store.get().await;
        let payload = self
            .runtime
            .state
            .user_tool_store
            .load_user_tools(&self.runtime.user_id);
        let enabled_skill_names = payload
            .skills
            .enabled
            .into_iter()
            .map(|name| name.trim().to_ascii_lowercase())
            .filter(|name| !name.is_empty())
            .collect::<HashSet<_>>();
        let mut app_hints = Vec::new();
        for service in config.a2a.services {
            let name = service.name.trim();
            if !name.is_empty() {
                app_hints.push(format!("${name}"));
            }
        }
        for server in payload.mcp_servers {
            let name = server.name.trim();
            if !name.is_empty() {
                app_hints.push(format!("${name}"));
            }
        }
        app_hints.sort_by_key(|value| value.to_ascii_lowercase());
        app_hints.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
        self.app_hints = app_hints;

        let (_, specs) = crate::load_user_skill_specs(&self.runtime).await;
        let mut skill_hints = specs
            .into_iter()
            .map(|spec| format!("#{}", spec.name))
            .collect::<Vec<_>>();
        skill_hints.sort_by_key(|value| value.to_ascii_lowercase());
        skill_hints.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
        self.skill_hints = skill_hints;
        self.enabled_skill_names = enabled_skill_names;
    }

    pub fn selected_transcript_index(&self) -> Option<usize> {
        self.transcript_selected
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
        let is_zh = self.is_zh_language();
        let selected = self.approval_selected_index.min(2);
        let summary = summarize_modal_text(request.summary.as_str(), 110);
        let detail = summarize_modal_text(compact_json(&request.detail).as_str(), 120);
        let args = summarize_modal_text(compact_json(&request.args).as_str(), 120);
        let prompt = approval_prompt_text(request, is_zh);
        let option_labels = approval_option_labels(request, is_zh);
        let patch_preview = if is_apply_patch_tool_name(request.tool.as_str()) {
            format_apply_patch_approval_lines(&request.args, is_zh)
        } else {
            None
        };
        let mut lines = vec![prompt];
        if !summary.trim().is_empty() {
            lines.push(summary);
        }
        if !self.approval_queue.is_empty() {
            lines.push(if is_zh {
                format!("还有 {} 个待处理请求", self.approval_queue.len())
            } else {
                format!("{} more pending request(s)", self.approval_queue.len())
            });
        }
        lines.push(String::new());
        if let Some(preview) = patch_preview {
            lines.extend(preview);
        } else {
            lines.push(request.tool.clone());
            if !detail.trim().is_empty() && detail != "{}" && detail != "null" {
                lines.push(detail);
            }
            if !args.trim().is_empty() && args != "{}" && args != "null" {
                lines.push(args);
            }
        }
        lines.push(String::new());
        for (index, label) in option_labels.iter().enumerate() {
            lines.push(format!(
                "{} {}. {}",
                if index == selected { "›" } else { " " },
                index + 1,
                label
            ));
        }
        Some(lines)
    }

    pub fn inquiry_modal_lines(&self) -> Option<Vec<String>> {
        let panel = self.active_inquiry_panel.as_ref()?;
        if panel.routes.is_empty() {
            return None;
        }
        let selected = self
            .inquiry_selected_index
            .min(panel.routes.len().saturating_sub(1));
        let mut lines = vec![panel.question.clone(), String::new()];
        for (index, route) in panel.routes.iter().enumerate() {
            let marker = if index == selected { "›" } else { " " };
            let mut title = route.label.clone();
            if route.recommended {
                if self.is_zh_language() {
                    title.push_str("（推荐）");
                } else {
                    title.push_str(" (recommended)");
                }
            }
            let body = match route.description.as_deref() {
                Some(description) if !description.trim().is_empty() => {
                    format!("{title}  {description}")
                }
                _ => title,
            };
            lines.push(format!("{marker} {}. {body}", index + 1));
        }
        Some(lines)
    }

    fn is_same_inquiry_panel(&self, left: &InquiryPanelState, right: &InquiryPanelState) -> bool {
        if left.question.trim() != right.question.trim() || left.routes.len() != right.routes.len()
        {
            return false;
        }
        left.routes
            .iter()
            .zip(right.routes.iter())
            .all(|(a, b)| a.label == b.label && a.description == b.description)
    }

    fn activate_inquiry_panel(&mut self, panel: InquiryPanelState, emit_log: bool) {
        let already_same = self
            .active_inquiry_panel
            .as_ref()
            .map(|existing| self.is_same_inquiry_panel(existing, &panel))
            .unwrap_or(false);
        let recommended_index = panel
            .routes
            .iter()
            .position(|route| route.recommended)
            .unwrap_or(0);
        self.inquiry_selected_index = recommended_index;
        self.active_inquiry_panel = Some(panel.clone());
        if emit_log && !already_same {
            self.show_inquiry_panel_prompt(&panel);
        }
    }

    fn parse_inquiry_panel_state(&self, payload: &Value) -> Option<InquiryPanelState> {
        let question = payload
            .get("question")
            .or_else(|| payload.get("prompt"))
            .or_else(|| payload.get("title"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        let routes = payload
            .get("routes")
            .or_else(|| payload.get("options"))
            .or_else(|| payload.get("choices"))
            .and_then(Value::as_array)?;
        let mut normalized_routes = Vec::new();
        for item in routes {
            let (label, description, recommended) = match item {
                Value::String(value) => (value.trim().to_string(), None, false),
                Value::Object(map) => {
                    let label = map
                        .get("label")
                        .or_else(|| map.get("title"))
                        .or_else(|| map.get("name"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    let description = map
                        .get("description")
                        .or_else(|| map.get("detail"))
                        .or_else(|| map.get("desc"))
                        .or_else(|| map.get("summary"))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToString::to_string);
                    let recommended = map
                        .get("recommended")
                        .or_else(|| map.get("preferred"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    (label, description, recommended)
                }
                _ => continue,
            };
            if label.is_empty() {
                continue;
            }
            normalized_routes.push(InquiryRoute {
                label,
                description,
                recommended,
            });
        }
        if normalized_routes.is_empty() {
            return None;
        }
        let question = if question.is_empty() {
            crate::locale::tr(
                self.display_language.as_str(),
                "请选择继续方式",
                "Choose a route to continue",
            )
        } else {
            question
        };
        let multiple = payload
            .get("multiple")
            .or_else(|| payload.get("allow_multiple"))
            .or_else(|| payload.get("multi"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Some(InquiryPanelState {
            question,
            routes: normalized_routes,
            multiple,
        })
    }

    fn parse_inquiry_panel_from_tool_result(&self, payload: &Value) -> Option<InquiryPanelState> {
        self.parse_inquiry_panel_state(payload)
            .or_else(|| {
                payload
                    .get("result")
                    .and_then(|value| self.parse_inquiry_panel_state(value))
            })
            .or_else(|| {
                payload
                    .get("output")
                    .and_then(|value| self.parse_inquiry_panel_state(value))
            })
            .or_else(|| {
                payload
                    .get("data")
                    .and_then(|value| self.parse_inquiry_panel_state(value))
            })
    }

    fn show_inquiry_panel_prompt(&mut self, panel: &InquiryPanelState) {
        if self.is_zh_language() {
            self.push_log(LogKind::Tool, format!("[问询面板] {}", panel.question));
        } else {
            self.push_log(LogKind::Tool, format!("[Inquiry Panel] {}", panel.question));
        }
        for (index, route) in panel.routes.iter().enumerate() {
            let badge = if route.recommended {
                if self.is_zh_language() {
                    "（推荐）"
                } else {
                    " (recommended)"
                }
            } else {
                ""
            };
            let line = if let Some(description) = route.description.as_deref() {
                if self.is_zh_language() {
                    format!("  {}. {}{}：{}", index + 1, route.label, badge, description)
                } else {
                    format!("  {}. {}{}: {}", index + 1, route.label, badge, description)
                }
            } else {
                format!("  {}. {}{}", index + 1, route.label, badge)
            };
            self.push_log(LogKind::Tool, line);
        }
        let hint = if panel.multiple {
            crate::locale::tr(
                self.display_language.as_str(),
                "输入多个路由编号并用逗号分隔（如 1,3）后回车；也可以直接输入自由文本继续。",
                "Type route numbers for multi-select (e.g. 1,3) then Enter; or send free text to continue.",
            )
        } else {
            crate::locale::tr(
                self.display_language.as_str(),
                "输入路由编号（如 1）后回车；也可以直接输入自由文本继续。",
                "Type a route number (e.g. 1) then Enter; or send free text to continue.",
            )
        };
        self.push_log(LogKind::Tool, hint);
    }

    fn try_handle_inquiry_panel_navigation_key(&mut self, key: KeyEvent) -> Option<Option<String>> {
        let panel = self.active_inquiry_panel.as_ref()?;
        if !self.input.trim().is_empty() {
            return None;
        }
        let route_len = panel.routes.len();
        if route_len == 0 {
            return None;
        }
        match key.code {
            KeyCode::Up => {
                self.inquiry_selected_index = self.inquiry_selected_index.saturating_sub(1);
                Some(None)
            }
            KeyCode::Down => {
                self.inquiry_selected_index =
                    (self.inquiry_selected_index + 1).min(route_len.saturating_sub(1));
                Some(None)
            }
            KeyCode::Enter => Some(Some((self.inquiry_selected_index + 1).to_string())),
            KeyCode::Esc => {
                self.active_inquiry_panel = None;
                self.inquiry_selected_index = 0;
                Some(None)
            }
            KeyCode::Char(ch)
                if key.modifiers == KeyModifiers::NONE && ch.is_ascii_digit() && ch != '0' =>
            {
                let index = (ch as usize).saturating_sub('1' as usize);
                if index < route_len {
                    self.inquiry_selected_index = index;
                    Some(Some((index + 1).to_string()))
                } else {
                    Some(None)
                }
            }
            _ => None,
        }
    }

    fn parse_inquiry_selection_indexes(
        &self,
        input: &str,
        max_routes: usize,
    ) -> Option<Vec<usize>> {
        let normalized = input.replace(['，', '、', ';', '；'], ",");
        let mut selected = Vec::new();
        for token in normalized.split(|ch: char| ch == ',' || ch.is_whitespace()) {
            let trimmed = token.trim();
            if trimmed.is_empty() {
                continue;
            }
            if !trimmed.chars().all(|ch| ch.is_ascii_digit()) {
                return None;
            }
            let Ok(index) = trimmed.parse::<usize>() else {
                return Some(Vec::new());
            };
            if index == 0 || index > max_routes {
                return Some(Vec::new());
            }
            let normalized_index = index - 1;
            if !selected.contains(&normalized_index) {
                selected.push(normalized_index);
            }
        }
        if selected.is_empty() {
            return None;
        }
        Some(selected)
    }

    fn try_convert_inquiry_input(&mut self, input: &str) -> Option<String> {
        let panel = self.active_inquiry_panel.clone()?;
        let selected_indexes = self.parse_inquiry_selection_indexes(input, panel.routes.len())?;
        if selected_indexes.is_empty() {
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "问询选择无效：编号超出范围。",
                    "invalid inquiry selection: index out of range.",
                ),
            );
            return Some(String::new());
        }
        if !panel.multiple && selected_indexes.len() > 1 {
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "当前问询仅支持单选，请只提供一个编号。",
                    "this inquiry panel is single-select; provide one index only.",
                ),
            );
            return Some(String::new());
        }
        let mut lines = Vec::new();
        if self.is_zh_language() {
            lines.push("[问询面板选择]".to_string());
            lines.push(format!("问题：{}", panel.question));
        } else {
            lines.push("[Inquiry Panel Selection]".to_string());
            lines.push(format!("Question: {}", panel.question));
        }
        for index in selected_indexes {
            if let Some(route) = panel.routes.get(index) {
                if let Some(description) = route.description.as_deref() {
                    if self.is_zh_language() {
                        lines.push(format!("- {}：{}", route.label, description));
                    } else {
                        lines.push(format!("- {}: {}", route.label, description));
                    }
                } else {
                    lines.push(format!("- {}", route.label));
                }
            }
        }
        self.active_inquiry_panel = None;
        self.inquiry_selected_index = 0;
        Some(lines.join("\n"))
    }

    pub fn shortcuts_lines(&self) -> Vec<String> {
        if self.is_zh_language() {
            return vec![
                "/ 命令 · Enter 发送".to_string(),
                "Shift + Enter / Ctrl + J 换行 · Tab 补全".to_string(),
                "@ 文件路径 · Ctrl + V / Shift + Insert 粘贴图片".to_string(),
                "F3 查看输出 · Ctrl + C 退出".to_string(),
                "Esc 关闭快捷键 · 拖入图片或文件即可附加".to_string(),
            ];
        }
        vec![
            "/ for commands                             enter to send".to_string(),
            "shift + enter for newline                  tab to complete".to_string(),
            "@ for file paths                           ctrl + v to paste images".to_string(),
            "f3 to view transcript                      ctrl + c to exit".to_string(),
            "esc to close shortcuts                    drag images/files to attach".to_string(),
        ]
    }

    pub fn set_mouse_regions(&mut self, transcript: Rect, input: Rect) {
        self.transcript_mouse_region = transcript;
        self.input_mouse_region = input;
    }

    pub fn set_input_viewport(&mut self, width: u16) {
        self.input_viewport_width = width.max(1);
    }

    pub fn set_transcript_viewport(&mut self, width: u16, height: u16) {
        let width = width.max(1);
        let height = height.max(1);
        if self.transcript_viewport_width != width || self.transcript_viewport_height != height {
            self.transcript_viewport_width = width;
            self.transcript_viewport_height = height;
            self.invalidate_transcript_metrics();
        }
    }

    pub fn set_transcript_rendered_lines(&mut self, lines: usize) {
        self.transcript_rendered_lines = lines;
    }

    pub fn drain_pending_scrollback_lines(&mut self) -> Vec<Line<'static>> {
        std::mem::take(&mut self.pending_scrollback_lines)
    }

    fn invalidate_transcript_metrics(&mut self) {
        self.transcript_rendered_lines = 0;
    }

    fn reset_scrollback_archive(&mut self) {
        self.history_archived_entries = 0;
        self.pending_scrollback_lines.clear();
    }

    fn clear_markdown_streams(&mut self) {
        self.assistant_markdown_stream = None;
        self.reasoning_markdown_stream = None;
    }

    fn clear_markdown_stream_for(&mut self, kind: LogKind, index: usize) {
        match kind {
            LogKind::Assistant => {
                if self
                    .assistant_markdown_stream
                    .as_ref()
                    .is_some_and(|state| state.index == index)
                {
                    self.assistant_markdown_stream = None;
                }
            }
            LogKind::Reasoning => {
                if self
                    .reasoning_markdown_stream
                    .as_ref()
                    .is_some_and(|state| state.index == index)
                {
                    self.reasoning_markdown_stream = None;
                }
            }
            _ => {}
        }
    }

    fn adjust_markdown_stream_indices_after_remove(&mut self, removed_index: usize) {
        if let Some(state) = self.assistant_markdown_stream.as_mut() {
            if state.index == removed_index {
                self.assistant_markdown_stream = None;
            } else if state.index > removed_index {
                state.index = state.index.saturating_sub(1);
            }
        }
        if let Some(state) = self.reasoning_markdown_stream.as_mut() {
            if state.index == removed_index {
                self.reasoning_markdown_stream = None;
            } else if state.index > removed_index {
                state.index = state.index.saturating_sub(1);
            }
        }
    }

    fn stream_markdown_delta(&mut self, kind: LogKind, index: usize, delta: &str) -> bool {
        let Some(state) = self.ensure_markdown_stream_state(kind, index) else {
            return false;
        };
        if delta.is_empty() {
            return false;
        }
        state.collector.push_delta(delta);
        if !delta.contains('\n') {
            return false;
        }
        let newly_completed = state.collector.commit_complete_lines();
        if newly_completed.is_empty() {
            return false;
        }
        self.append_stream_lines(kind, index, newly_completed);
        true
    }

    fn ensure_markdown_stream_state(
        &mut self,
        kind: LogKind,
        index: usize,
    ) -> Option<&mut StreamedMarkdownState> {
        let wrap_width = markdown_stream_wrap_width(kind, self.transcript_viewport_width);
        match kind {
            LogKind::Assistant => {
                let needs_reset = self
                    .assistant_markdown_stream
                    .as_ref()
                    .is_none_or(|state| state.index != index || state.wrap_width != wrap_width);
                if needs_reset {
                    self.assistant_markdown_stream = Some(StreamedMarkdownState {
                        index,
                        wrap_width,
                        collector: super::markdown_stream::MarkdownStreamCollector::new(Some(
                            wrap_width,
                        )),
                    });
                }
                self.assistant_markdown_stream.as_mut()
            }
            LogKind::Reasoning => {
                let needs_reset = self
                    .reasoning_markdown_stream
                    .as_ref()
                    .is_none_or(|state| state.index != index || state.wrap_width != wrap_width);
                if needs_reset {
                    self.reasoning_markdown_stream = Some(StreamedMarkdownState {
                        index,
                        wrap_width,
                        collector: super::markdown_stream::MarkdownStreamCollector::new(Some(
                            wrap_width,
                        )),
                    });
                }
                self.reasoning_markdown_stream.as_mut()
            }
            _ => None,
        }
    }

    fn append_stream_lines(&mut self, kind: LogKind, index: usize, lines: Vec<Line<'static>>) {
        let Some(entry) = self.logs.get_mut(index) else {
            return;
        };
        let width = self.transcript_viewport_width.max(1);
        let cache = entry.markdown_cache.get_or_insert(MarkdownCache {
            width,
            lines: Vec::new(),
        });
        if cache.width != width {
            cache.width = width;
            cache.lines.clear();
        }
        let add_prefix = cache.lines.is_empty();
        let mut styled = style_markdown_lines(kind, lines, add_prefix);
        cache.lines.append(&mut styled);
    }

    fn finalize_markdown_stream_for(&mut self, kind: LogKind, index: usize) {
        match kind {
            LogKind::Assistant => {
                if let Some(mut state) = self.assistant_markdown_stream.take() {
                    if state.index == index {
                        let remaining = state.collector.finalize_and_drain();
                        if !remaining.is_empty() {
                            self.append_stream_lines(kind, index, remaining);
                        }
                    } else {
                        self.assistant_markdown_stream = Some(state);
                    }
                }
            }
            LogKind::Reasoning => {
                if let Some(mut state) = self.reasoning_markdown_stream.take() {
                    if state.index == index {
                        let remaining = state.collector.finalize_and_drain();
                        if !remaining.is_empty() {
                            self.append_stream_lines(kind, index, remaining);
                        }
                    } else {
                        self.reasoning_markdown_stream = Some(state);
                    }
                }
            }
            _ => {}
        }
    }

    fn finalize_all_markdown_streams(&mut self) {
        if let Some(mut state) = self.assistant_markdown_stream.take() {
            let index = state.index;
            let remaining = state.collector.finalize_and_drain();
            if !remaining.is_empty() {
                self.append_stream_lines(LogKind::Assistant, index, remaining);
            }
        }
        if let Some(mut state) = self.reasoning_markdown_stream.take() {
            let index = state.index;
            let remaining = state.collector.finalize_and_drain();
            if !remaining.is_empty() {
                self.append_stream_lines(LogKind::Reasoning, index, remaining);
            }
        }
    }

    fn mouse_in_region(&self, mouse: &MouseEvent, region: Rect) -> bool {
        if region.width == 0 || region.height == 0 {
            return false;
        }
        let max_x = region.x.saturating_add(region.width);
        let max_y = region.y.saturating_add(region.height);
        mouse.column >= region.x
            && mouse.column < max_x
            && mouse.row >= region.y
            && mouse.row < max_y
    }

    fn mouse_in_transcript_region(&self, mouse: &MouseEvent) -> bool {
        self.mouse_in_region(mouse, self.transcript_mouse_region)
    }

    fn set_mouse_mode(&mut self, mode: MouseMode) {
        if self.mouse_mode == mode {
            return;
        }
        self.mouse_passthrough_until = None;
        self.mouse_mode = mode;
        let notice = match mode {
            MouseMode::Auto => {
                "mouse mode: auto (native selection and terminal wheel scrolling enabled; switch to scroll for app-captured transcript scrolling)"
            }
            MouseMode::Scroll => "mouse mode: scroll (capture wheel events for transcript scrolling)",
            MouseMode::Select => {
                "mouse mode: select/copy (native terminal selection enabled)"
            }
        };
        self.push_log(LogKind::Info, notice.to_string());
    }

    fn toggle_mouse_mode(&mut self) {
        let next = if self.mouse_mode == MouseMode::Scroll {
            MouseMode::Select
        } else if self.mouse_mode == MouseMode::Select {
            MouseMode::Auto
        } else {
            MouseMode::Scroll
        };
        self.set_mouse_mode(next);
    }

    fn mouse_passthrough_active(&self) -> bool {
        self.mouse_passthrough_until
            .map(|deadline| Instant::now() < deadline)
            .unwrap_or(false)
    }

    fn clear_mouse_passthrough(&mut self) {
        self.mouse_passthrough_until = None;
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

    pub fn input_view(&mut self, viewport_width: u16, viewport_height: u16) -> (String, u16, u16) {
        self.sync_large_paste_placeholders();
        self.sync_attachment_placeholders();
        self.clamp_input_cursor_outside_attachment_placeholder();
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

    pub fn transcript_render_window(&mut self, viewport_height: u16) -> TranscriptRenderWindow {
        self.archive_scrollback_entries(viewport_height);

        let width = usize::from(self.transcript_viewport_width.max(1));
        let archived = self.history_archived_entries.min(self.logs.len());
        let mut line_counts = Vec::with_capacity(self.logs.len().saturating_sub(archived));
        for index in archived..self.logs.len() {
            line_counts.push(self.entry_visual_line_count(index, width));
        }
        let window = compute_transcript_window_spec(
            line_counts.as_slice(),
            viewport_height,
            self.transcript_offset_from_bottom,
        );
        let entries = (window.start_entry..window.end_entry_exclusive)
            .map(|index| TranscriptRenderEntry {
                global_index: archived + index,
            })
            .collect::<Vec<_>>();

        TranscriptRenderWindow {
            entries,
            local_scroll: window.local_scroll,
            total_lines: window.total_lines,
        }
    }

    fn archive_scrollback_entries(&mut self, viewport_height: u16) {
        if self.transcript_offset_from_bottom > 0 {
            return;
        }

        let archived = self.history_archived_entries.min(self.logs.len());
        if archived >= self.logs.len() {
            return;
        }

        let width = usize::from(self.transcript_viewport_width.max(1));
        let mut line_counts = Vec::with_capacity(self.logs.len().saturating_sub(archived));
        for index in archived..self.logs.len() {
            line_counts.push(self.entry_visual_line_count(index, width));
        }
        let window = compute_transcript_window_spec(line_counts.as_slice(), viewport_height, 0);
        if window.start_entry == 0 {
            return;
        }

        let archive_end = archived
            .saturating_add(window.start_entry)
            .min(self.logs.len());
        let mut lines = Vec::new();
        let render_width = self.transcript_viewport_width.max(1);
        for index in archived..archive_end {
            lines.extend(self.render_entry_lines(index, false, render_width));
        }
        if lines.is_empty() {
            return;
        }

        self.pending_scrollback_lines.extend(lines);
        self.history_archived_entries = archive_end;
        if let Some(selected) = self.transcript_selected {
            if selected < self.history_archived_entries {
                self.transcript_selected = if self.history_archived_entries < self.logs.len() {
                    Some(self.history_archived_entries)
                } else {
                    None
                };
            }
        }
        self.invalidate_transcript_metrics();
    }

    fn scroll_transcript_up(&mut self, lines: u16) {
        let max_scroll =
            self.max_transcript_scroll(usize::from(self.transcript_viewport_height.max(1)));
        let current = self.transcript_offset_from_bottom.min(max_scroll);
        self.transcript_offset_from_bottom =
            current.saturating_add(usize::from(lines)).min(max_scroll);
    }

    fn scroll_transcript_down(&mut self, lines: u16) {
        let max_scroll =
            self.max_transcript_scroll(usize::from(self.transcript_viewport_height.max(1)));
        let current = self.transcript_offset_from_bottom.min(max_scroll);
        self.transcript_offset_from_bottom = current.saturating_sub(usize::from(lines));
    }

    fn scroll_transcript_to_top(&mut self) {
        self.transcript_offset_from_bottom =
            self.max_transcript_scroll(usize::from(self.transcript_viewport_height.max(1)));
    }

    fn scroll_transcript_to_bottom(&mut self) {
        self.transcript_offset_from_bottom = 0;
    }

    fn max_transcript_scroll(&mut self, viewport_height: usize) -> usize {
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
        let viewport = usize::from(self.transcript_viewport_height.max(1));
        let max_scroll = self.max_transcript_scroll(viewport);
        let current_scroll =
            max_scroll.saturating_sub(self.transcript_offset_from_bottom.min(max_scroll));

        let width = usize::from(self.transcript_viewport_width.max(1));
        let mut start_line = 0usize;
        for index in 0..self.logs.len() {
            let line_count = self.entry_visual_line_count(index, width).max(1);
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

    fn total_transcript_lines(&mut self) -> usize {
        if self.transcript_rendered_lines > 0 {
            return self.transcript_rendered_lines;
        }

        let width = usize::from(self.transcript_viewport_width.max(1));
        let archived = self.history_archived_entries.min(self.logs.len());
        let mut total = 0usize;
        for index in archived..self.logs.len() {
            total = total.saturating_add(self.entry_visual_line_count(index, width));
        }
        total
    }

    fn entry_visual_line_count(&mut self, index: usize, width: usize) -> usize {
        let Some(kind) = self.logs.get(index).map(|entry| entry.kind) else {
            return 1;
        };
        let streaming = self.entry_is_streaming(kind, index);
        let Some(entry) = self.logs.get_mut(index) else {
            return 1;
        };
        if let Some(special) = entry.special.as_ref() {
            return special.line_count(width.min(u16::MAX as usize) as u16)
                + transcript_entry_spacing_before(index);
        }
        if matches!(kind, LogKind::Assistant | LogKind::Reasoning) {
            if streaming {
                return entry
                    .markdown_cache
                    .as_ref()
                    .map(|cache| cache.lines.len())
                    .unwrap_or(1)
                    .max(1)
                    + transcript_entry_spacing_before(index);
            }
            ensure_markdown_cache(entry, width as u16);
            return entry
                .markdown_cache
                .as_ref()
                .map(|cache| cache.lines.len())
                .unwrap_or(1)
                .max(1)
                + transcript_entry_spacing_before(index);
        }
        render_plain_lines(
            entry.kind,
            entry.text.as_str(),
            log_base_style(entry.kind),
            width as u16,
        )
        .len()
            + transcript_entry_spacing_before(index)
    }

    pub fn render_entry_lines(
        &mut self,
        index: usize,
        selected: bool,
        width: u16,
    ) -> Vec<Line<'static>> {
        let Some(kind) = self.logs.get(index).map(|entry| entry.kind) else {
            return Vec::new();
        };
        let streaming = self.entry_is_streaming(kind, index);
        let Some(entry) = self.logs.get_mut(index) else {
            return Vec::new();
        };
        let base_style = log_base_style(entry.kind);
        if let Some(special) = entry.special.as_ref() {
            let mut lines = special.render_lines_for_width(width);
            if selected {
                let selected_style = super::theme::transcript_selection(base_style);
                for line in &mut lines {
                    line.style = selected_style.patch(line.style);
                }
            }
            return lines;
        }
        let mut lines = if matches!(entry.kind, LogKind::Assistant | LogKind::Reasoning) {
            if streaming {
                entry
                    .markdown_cache
                    .as_ref()
                    .map(|cache| cache.lines.clone())
                    .unwrap_or_else(|| {
                        vec![Line::from(Span::styled(
                            log_prefix(entry.kind).to_string(),
                            base_style,
                        ))]
                    })
            } else {
                ensure_markdown_cache(entry, width);
                entry
                    .markdown_cache
                    .as_ref()
                    .map(|cache| cache.lines.clone())
                    .unwrap_or_else(|| {
                        render_plain_lines(entry.kind, entry.text.as_str(), base_style, width)
                    })
            }
        } else {
            render_plain_lines(entry.kind, entry.text.as_str(), base_style, width)
        };

        if selected {
            let selected_style = super::theme::transcript_selection(base_style);
            for line in &mut lines {
                line.style = selected_style.patch(line.style);
            }
        }

        if index > 0 {
            lines.insert(0, Line::from(String::new()));
        }

        lines
    }

    fn entry_is_streaming(&self, kind: LogKind, index: usize) -> bool {
        match kind {
            LogKind::Assistant => self
                .assistant_markdown_stream
                .as_ref()
                .is_some_and(|state| state.index == index),
            LogKind::Reasoning => self
                .reasoning_markdown_stream
                .as_ref()
                .is_some_and(|state| state.index == index),
            _ => false,
        }
    }

    pub fn popup_view(&mut self) -> PopupView {
        let lines = self.popup_lines_full();
        self.sync_popup_selection(lines.len());
        if lines.is_empty() {
            return PopupView {
                lines: Vec::new(),
                selected_index: None,
            };
        }
        let selected = self.popup_selected_index.min(lines.len().saturating_sub(1));
        let (start, end) = Self::popup_window_bounds(lines.len(), selected);
        PopupView {
            lines: lines[start..end].to_vec(),
            selected_index: Some(selected.saturating_sub(start)),
        }
    }

    fn popup_lines_full(&self) -> Vec<String> {
        let trimmed = self.input.trim_start();
        if trimmed.starts_with('/') {
            let body = trimmed.trim_start_matches('/');
            return slash_command::popup_lines_with_language(
                body,
                POPUP_MAX_CANDIDATES,
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
            return self.mention_popup_lines(query, POPUP_MAX_CANDIDATES);
        }
        if let Some(query) = token.strip_prefix('$') {
            return self.app_popup_lines(query, POPUP_MAX_CANDIDATES);
        }
        if let Some(query) = token.strip_prefix('#') {
            return self.skill_popup_lines(query, POPUP_MAX_CANDIDATES);
        }

        Vec::new()
    }

    fn popup_signature(&self) -> String {
        let trimmed = self.input.trim_start();
        if trimmed.starts_with('/') {
            return format!("/{}", trimmed.trim_start_matches('/').trim());
        }

        let cursor = self.input_cursor.min(self.input.len());
        let head = &self.input[..cursor];
        let token_start = head
            .rfind(char::is_whitespace)
            .map(|index| index.saturating_add(1))
            .unwrap_or(0);
        let token = &head[token_start..];
        if token.starts_with('@') || token.starts_with('$') || token.starts_with('#') {
            return token.trim().to_string();
        }
        String::new()
    }

    fn sync_popup_selection(&mut self, popup_len: usize) {
        let signature = self.popup_signature();
        if self.popup_signature != signature {
            self.popup_signature = signature;
            self.popup_selected_index = 0;
        }
        if popup_len == 0 {
            self.popup_selected_index = 0;
            return;
        }
        self.popup_selected_index = self.popup_selected_index.min(popup_len.saturating_sub(1));
    }

    fn popup_window_bounds(popup_len: usize, selected: usize) -> (usize, usize) {
        if popup_len <= POPUP_VISIBLE_LIMIT {
            return (0, popup_len);
        }
        let half = POPUP_VISIBLE_LIMIT / 2;
        let max_start = popup_len.saturating_sub(POPUP_VISIBLE_LIMIT);
        let start = selected.saturating_sub(half).min(max_start);
        (start, start.saturating_add(POPUP_VISIBLE_LIMIT))
    }

    fn move_popup_selection(&mut self, step: isize) -> bool {
        let lines = self.popup_lines_full();
        self.sync_popup_selection(lines.len());
        if lines.is_empty() {
            return false;
        }
        let max_index = lines.len().saturating_sub(1);
        if step < 0 {
            self.popup_selected_index = self
                .popup_selected_index
                .saturating_sub(step.unsigned_abs());
        } else {
            self.popup_selected_index = self
                .popup_selected_index
                .saturating_add(step as usize)
                .min(max_index);
        }
        true
    }

    pub fn popup_title(&self) -> &'static str {
        let trimmed = self.input.trim_start();
        if trimmed.starts_with('/') {
            return " Commands ";
        }
        let cursor = self.input_cursor.min(self.input.len());
        let head = &self.input[..cursor];
        let token_start = head
            .rfind(char::is_whitespace)
            .map(|index| index.saturating_add(1))
            .unwrap_or(0);
        let token = &head[token_start..];
        if token.starts_with('@') {
            return " Files ";
        }
        if token.starts_with('$') {
            return " Apps ";
        }
        if token.starts_with('#') {
            return " Skills ";
        }
        " Commands "
    }

    fn mention_popup_lines(&self, query: &str, limit: usize) -> Vec<String> {
        self.mention_popup_tokens(query, limit)
    }

    fn app_popup_lines(&self, query: &str, limit: usize) -> Vec<String> {
        self.app_popup_tokens(query, limit)
    }

    fn skill_popup_lines(&self, query: &str, limit: usize) -> Vec<String> {
        self.skill_popup_tokens(query, limit)
            .into_iter()
            .map(|token| self.decorate_skill_popup_line(token.as_str()))
            .collect()
    }

    fn mention_popup_tokens(&self, query: &str, limit: usize) -> Vec<String> {
        let lowered = query.trim().to_ascii_lowercase();
        let mut tokens = self.collect_recent_popup_tokens('@', lowered.as_str(), limit, |token| {
            self.workspace_token_exists(token)
        });
        for item in &self.workspace_files {
            if tokens.len() >= limit {
                break;
            }
            if !lowered.is_empty() && !item.lowered.contains(lowered.as_str()) {
                continue;
            }
            let token = format!("@{}", item.path);
            if !contains_token_case_insensitive(&tokens, token.as_str()) {
                tokens.push(token);
            }
        }
        tokens
    }

    fn app_popup_tokens(&self, query: &str, limit: usize) -> Vec<String> {
        let lowered = query.trim().to_ascii_lowercase();
        self.rank_catalog_tokens(&self.app_hints, '$', lowered.as_str(), limit)
    }

    fn skill_popup_tokens(&self, query: &str, limit: usize) -> Vec<String> {
        let lowered = query.trim().to_ascii_lowercase();
        self.rank_catalog_tokens(&self.skill_hints, '#', lowered.as_str(), limit)
    }

    fn rank_catalog_tokens(
        &self,
        catalog: &[String],
        prefix: char,
        lowered_query: &str,
        limit: usize,
    ) -> Vec<String> {
        let mut tokens = self.collect_recent_popup_tokens(prefix, lowered_query, limit, |token| {
            catalog.iter().any(|item| item.eq_ignore_ascii_case(token))
        });
        for item in catalog {
            if tokens.len() >= limit {
                break;
            }
            if !lowered_query.is_empty() && !item.to_ascii_lowercase().contains(lowered_query) {
                continue;
            }
            if !contains_token_case_insensitive(&tokens, item) {
                tokens.push(item.clone());
            }
        }
        tokens
    }

    fn collect_recent_popup_tokens<F>(
        &self,
        prefix: char,
        lowered_query: &str,
        limit: usize,
        exists: F,
    ) -> Vec<String>
    where
        F: Fn(&str) -> bool,
    {
        if limit == 0 {
            return Vec::new();
        }
        let mut output = Vec::new();
        for item in &self.popup_recents {
            if output.len() >= limit {
                break;
            }
            if !popup_token_matches(item, prefix, lowered_query) {
                continue;
            }
            if !exists(item.as_str()) {
                continue;
            }
            if !contains_token_case_insensitive(&output, item) {
                output.push(item.clone());
            }
        }
        output
    }

    fn workspace_token_exists(&self, token: &str) -> bool {
        let Some(path) = token.strip_prefix('@') else {
            return false;
        };
        self.workspace_files
            .iter()
            .any(|item| item.path.eq_ignore_ascii_case(path))
    }

    fn is_skill_enabled_token(&self, token: &str) -> bool {
        let Some(name) = token.strip_prefix('#') else {
            return false;
        };
        let lowered = name.trim().to_ascii_lowercase();
        self.enabled_skill_names.contains(lowered.as_str())
    }

    fn decorate_skill_popup_line(&self, token: &str) -> String {
        if self.is_skill_enabled_token(token) {
            return token.to_string();
        }
        let skill_name = token.strip_prefix('#').unwrap_or(token).trim();
        if self.is_zh_language() {
            format!("{token}  [已禁用，运行 /skills enable {skill_name}]")
        } else {
            format!("{token}  [disabled, run /skills enable {skill_name}]")
        }
    }

    pub async fn drain_stream_events(&mut self) {
        self.flush_key_paste_burst_if_due(false);
        self.flush_pending_paste();
        self.drain_pending_attachment_paths().await;
        self.drain_approval_requests();
        let drain_budget = self.stream_drain_budget();

        let mut drained = 0usize;
        loop {
            let Some(receiver) = self.stream_rx.as_mut() else {
                self.reset_stream_catchup_state();
                break;
            };
            if drained >= drain_budget {
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
                    self.refresh_workspace_context();
                    self.reset_stream_catchup_state();
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
            self.approval_selected_index = 0;
            return;
        }
        self.approval_queue.push_back(request);
    }

    fn reset_stream_catchup_state(&mut self) {
        self.stream_catchup_mode = false;
        self.stream_catchup_enter_since = None;
        self.stream_catchup_exit_since = None;
    }

    fn stream_drain_budget(&mut self) -> usize {
        let Some(receiver) = self.stream_rx.as_ref() else {
            self.reset_stream_catchup_state();
            return MAX_DRAIN_MESSAGES_PER_TICK_BASE;
        };
        let depth = receiver.len();
        let now = Instant::now();
        if self.stream_catchup_mode {
            if depth <= STREAM_CATCHUP_EXIT_DEPTH {
                if let Some(since) = self.stream_catchup_exit_since {
                    if now.saturating_duration_since(since) >= STREAM_CATCHUP_EXIT_HOLD {
                        self.reset_stream_catchup_state();
                    }
                } else {
                    self.stream_catchup_exit_since = Some(now);
                }
            } else {
                self.stream_catchup_exit_since = None;
            }
        } else if depth >= STREAM_CATCHUP_ENTER_DEPTH {
            if let Some(since) = self.stream_catchup_enter_since {
                if now.saturating_duration_since(since) >= STREAM_CATCHUP_ENTER_HOLD {
                    self.stream_catchup_mode = true;
                    self.stream_catchup_exit_since = None;
                }
            } else {
                self.stream_catchup_enter_since = Some(now);
            }
        } else {
            self.stream_catchup_enter_since = None;
        }

        if self.stream_catchup_mode {
            MAX_DRAIN_MESSAGES_PER_TICK_CATCHUP
        } else {
            MAX_DRAIN_MESSAGES_PER_TICK_BASE
        }
    }

    fn flush_pending_paste(&mut self) {
        while let Some(chunk) = self.pending_paste.pop_front() {
            self.apply_pasted_text(chunk.as_str());
        }
    }

    fn apply_pasted_text(&mut self, pasted: &str) {
        let char_count = pasted.chars().count();
        if char_count > LARGE_PASTE_CHAR_THRESHOLD {
            let placeholder =
                next_large_paste_placeholder(&mut self.large_paste_counters, char_count);
            self.insert_text_at_cursor(placeholder.as_str());
            self.pending_large_pastes
                .push((placeholder, pasted.to_string()));
            return;
        }
        self.insert_text_at_cursor(pasted);
    }

    fn key_paste_burst_active(&self) -> bool {
        !self.key_paste_burst_buffer.is_empty()
    }

    fn clear_input_draft(&mut self) -> bool {
        let had_draft = !self.input.is_empty()
            || !self.pending_paste.is_empty()
            || !self.pending_large_pastes.is_empty()
            || !self.pending_attachments.is_empty()
            || self.key_paste_burst_active()
            || !self.suppressed_clipboard_paste.is_empty();
        if !had_draft {
            return false;
        }
        self.input.clear();
        self.input_cursor = 0;
        self.pending_paste.clear();
        self.pending_large_pastes.clear();
        self.clear_pending_attachments();
        self.key_paste_burst_buffer.clear();
        self.key_paste_burst_last_at = None;
        self.clear_suppressed_clipboard_paste();
        self.clear_clipboard_probe_cache();
        self.history_cursor = None;
        self.reset_plain_char_burst();
        true
    }

    fn take_recent_input_chars(&mut self, char_count: usize) -> String {
        if char_count == 0 || self.input_cursor == 0 {
            return String::new();
        }
        let mut start = self.input_cursor.min(self.input.len());
        for _ in 0..char_count {
            if start == 0 {
                break;
            }
            start = prev_char_boundary(&self.input, start);
        }
        if start >= self.input_cursor {
            return String::new();
        }
        let grabbed = self.input[start..self.input_cursor].to_string();
        self.input.replace_range(start..self.input_cursor, "");
        self.input_cursor = start;
        self.sync_large_paste_placeholders();
        self.sync_attachment_placeholders();
        grabbed
    }

    fn flush_key_paste_burst(&mut self) -> bool {
        if self.key_paste_burst_buffer.is_empty() {
            self.key_paste_burst_last_at = None;
            return false;
        }
        let burst = std::mem::take(&mut self.key_paste_burst_buffer);
        self.key_paste_burst_last_at = None;
        self.on_paste(burst);
        self.flush_pending_paste();
        self.reset_plain_char_burst();
        true
    }

    fn flush_key_paste_burst_if_due(&mut self, force: bool) -> bool {
        let Some(last) = self.key_paste_burst_last_at else {
            return false;
        };
        let timeout = if self.key_paste_burst_active() {
            PASTE_BURST_ACTIVE_IDLE_TIMEOUT
        } else {
            PASTE_BURST_CHAR_GAP
        };
        if !force && Instant::now().saturating_duration_since(last) <= timeout {
            return false;
        }
        self.flush_key_paste_burst()
    }

    fn handle_plain_char_paste_burst(&mut self, ch: char) -> bool {
        let now = Instant::now();
        if self.key_paste_burst_active() {
            if self
                .key_paste_burst_last_at
                .is_some_and(|last| now.saturating_duration_since(last) <= PASTE_BURST_CHAR_GAP)
            {
                self.key_paste_burst_buffer.push(ch);
                self.key_paste_burst_last_at = Some(now);
                return true;
            }
            self.flush_key_paste_burst();
        }
        if self.key_char_burst_len < PASTE_BURST_CAPTURE_CHAR_THRESHOLD {
            return false;
        }
        let retro_text =
            self.take_recent_input_chars(PASTE_BURST_CAPTURE_CHAR_THRESHOLD.saturating_sub(1));
        self.key_paste_burst_buffer.push_str(retro_text.as_str());
        self.key_paste_burst_buffer.push(ch);
        self.key_paste_burst_last_at = Some(now);
        true
    }

    fn sync_large_paste_placeholders(&mut self) {
        if self.pending_large_pastes.is_empty() {
            return;
        }
        self.pending_large_pastes
            .retain(|(placeholder, _)| self.input.contains(placeholder));
    }

    pub fn large_paste_placeholders(&self) -> Vec<String> {
        self.pending_large_pastes
            .iter()
            .map(|(placeholder, _)| placeholder.clone())
            .collect()
    }

    fn sync_attachment_placeholders(&mut self) {
        if self.pending_attachments.is_empty() {
            return;
        }
        let previous = self.pending_attachments.clone();
        self.pending_attachments = previous
            .iter()
            .enumerate()
            .filter(|(index, item)| {
                let placeholder = attachment_placeholder(item, *index);
                self.input.contains(placeholder.as_str())
            })
            .map(|(_, item)| item.clone())
            .collect();
        if previous.len() != self.pending_attachments.len() {
            self.input = replace_attachment_placeholders(
                self.input.as_str(),
                previous.as_slice(),
                self.pending_attachments.as_slice(),
            );
            self.input_cursor = clamp_cursor_out_of_attachment_placeholder(
                self.input.as_str(),
                self.input_cursor.min(self.input.len()),
                self.pending_attachments.as_slice(),
            );
        }
    }

    fn clamp_input_cursor_outside_attachment_placeholder(&mut self) {
        self.input_cursor = clamp_cursor_out_of_attachment_placeholder(
            self.input.as_str(),
            self.input_cursor.min(self.input.len()),
            self.pending_attachments.as_slice(),
        );
    }

    fn insert_attachment_placeholder_at_cursor(&mut self, placeholder: &str) {
        self.clamp_input_cursor_outside_attachment_placeholder();
        let cursor = self.input_cursor.min(self.input.len());
        let needs_leading_space = cursor > 0
            && self
                .input
                .get(..cursor)
                .and_then(|value| value.chars().next_back())
                .is_some_and(|ch| !ch.is_whitespace());
        let needs_trailing_space = self
            .input
            .get(cursor..)
            .and_then(|value| value.chars().next())
            .map(|ch| !ch.is_whitespace())
            .unwrap_or(true);
        let mut token = String::new();
        if needs_leading_space {
            token.push(' ');
        }
        token.push_str(placeholder);
        if needs_trailing_space {
            token.push(' ');
        }
        self.insert_text_at_cursor(token.as_str());
    }

    fn remove_pending_attachment_at(
        &mut self,
        index: usize,
    ) -> Option<crate::attachments::PreparedAttachment> {
        if index >= self.pending_attachments.len() {
            return None;
        }
        let previous = self.pending_attachments.clone();
        let removed = self.pending_attachments.remove(index);
        self.input = replace_attachment_placeholders(
            self.input.as_str(),
            previous.as_slice(),
            self.pending_attachments.as_slice(),
        );
        self.input_cursor = clamp_cursor_out_of_attachment_placeholder(
            self.input.as_str(),
            self.input_cursor.min(self.input.len()),
            self.pending_attachments.as_slice(),
        );
        Some(removed)
    }

    fn remove_attachment_placeholder_match(&mut self, matched: AttachmentPlaceholderMatch) {
        let mut remove_start = matched.start;
        let mut remove_end = matched.end;
        if self
            .input
            .get(remove_end..)
            .is_some_and(|value| value.starts_with(' '))
        {
            remove_end = next_char_boundary(&self.input, remove_end);
        } else if remove_start > 0
            && self
                .input
                .get(..remove_start)
                .and_then(|value| value.chars().next_back())
                .is_some_and(char::is_whitespace)
        {
            remove_start = prev_char_boundary(&self.input, remove_start);
        }
        self.input.replace_range(remove_start..remove_end, "");
        self.input_cursor = remove_start;

        let previous = self.pending_attachments.clone();
        self.pending_attachments.remove(matched.index);
        self.input = replace_attachment_placeholders(
            self.input.as_str(),
            previous.as_slice(),
            self.pending_attachments.as_slice(),
        );
        self.input_cursor = clamp_cursor_out_of_attachment_placeholder(
            self.input.as_str(),
            self.input_cursor.min(self.input.len()),
            self.pending_attachments.as_slice(),
        );
    }

    fn clear_pending_attachments(&mut self) {
        if self.pending_attachments.is_empty() {
            return;
        }
        let previous = self.pending_attachments.clone();
        self.pending_attachments.clear();
        self.input = replace_attachment_placeholders(self.input.as_str(), previous.as_slice(), &[]);
        self.input_cursor = self.input_cursor.min(self.input.len());
    }

    fn recent_input_chars(&self, char_count: usize) -> String {
        if char_count == 0 || self.input_cursor == 0 {
            return String::new();
        }
        let mut start = self.input_cursor.min(self.input.len());
        for _ in 0..char_count {
            if start == 0 {
                break;
            }
            start = prev_char_boundary(&self.input, start);
        }
        if start >= self.input_cursor {
            return String::new();
        }
        self.input[start..self.input_cursor].to_string()
    }

    fn cached_clipboard_text_for_promotion(&mut self) -> Option<String> {
        let now = Instant::now();
        if let Some((cached_at, cached_text)) = &self.clipboard_probe_cache {
            if now.saturating_duration_since(*cached_at) <= CLIPBOARD_PASTE_PROBE_CACHE_TTL {
                return cached_text.clone();
            }
        }
        let detected = read_system_clipboard_text().ok().flatten();
        self.clipboard_probe_cache = Some((now, detected.clone()));
        detected
    }

    fn clear_clipboard_probe_cache(&mut self) {
        self.clipboard_probe_cache = None;
    }

    fn clear_suppressed_clipboard_paste(&mut self) {
        self.suppressed_clipboard_paste.clear();
        self.suppressed_clipboard_paste_last_at = None;
    }

    fn should_promote_clipboard_text(&self, text: &str) -> bool {
        text.chars().count() > LARGE_PASTE_CHAR_THRESHOLD
            || text.contains('\n')
            || detect_pasted_attachment_paths(self.runtime.launch_dir.as_path(), text).is_some()
    }

    fn try_consume_suppressed_clipboard_key(&mut self, key: KeyEvent) -> bool {
        if self.suppressed_clipboard_paste.is_empty() {
            return false;
        }
        let now = Instant::now();
        if self.suppressed_clipboard_paste_last_at.is_some_and(|last| {
            now.saturating_duration_since(last) > SUPPRESSED_CLIPBOARD_PASTE_TIMEOUT
        }) {
            self.clear_suppressed_clipboard_paste();
            return false;
        }
        let mut remaining = self.suppressed_clipboard_paste.chars();
        let Some(expected) = remaining.next() else {
            self.clear_suppressed_clipboard_paste();
            return false;
        };
        let matches = match key.code {
            KeyCode::Char(ch) => ch == expected,
            KeyCode::Enter => expected == '\n',
            _ => false,
        };
        if !matches {
            self.clear_suppressed_clipboard_paste();
            return false;
        }
        self.suppressed_clipboard_paste = remaining.collect();
        self.suppressed_clipboard_paste_last_at = Some(now);
        if self.suppressed_clipboard_paste.is_empty() {
            self.clear_suppressed_clipboard_paste();
        }
        true
    }

    fn try_promote_clipboard_paste(&mut self, ch: char) -> bool {
        let max_prior_chars = self
            .input
            .get(..self.input_cursor.min(self.input.len()))
            .map(|value| value.chars().count().min(8))
            .unwrap_or(0);
        let Some(clipboard_text) = self.cached_clipboard_text_for_promotion() else {
            return false;
        };
        if !self.should_promote_clipboard_text(clipboard_text.as_str()) {
            return false;
        }
        if max_prior_chars == 0 {
            if !clipboard_text.starts_with(ch) {
                return false;
            }
            let remaining = clipboard_text.chars().skip(1).collect::<String>();
            if !remaining.is_empty() {
                self.suppressed_clipboard_paste = remaining;
                self.suppressed_clipboard_paste_last_at = Some(Instant::now());
            } else {
                self.clear_suppressed_clipboard_paste();
            }
            self.on_paste(clipboard_text);
            self.flush_pending_paste();
            self.reset_plain_char_burst();
            return true;
        }
        for prior_chars in (1..=max_prior_chars).rev() {
            let mut candidate = self.recent_input_chars(prior_chars);
            candidate.push(ch);
            if !clipboard_text.starts_with(candidate.as_str()) {
                continue;
            }
            let _ = self.take_recent_input_chars(prior_chars);
            let consumed_prefix_chars = candidate.chars().count();
            let remaining = clipboard_text
                .chars()
                .skip(consumed_prefix_chars)
                .collect::<String>();
            if !remaining.is_empty() {
                self.suppressed_clipboard_paste = remaining;
                self.suppressed_clipboard_paste_last_at = Some(Instant::now());
            } else {
                self.clear_suppressed_clipboard_paste();
            }
            self.on_paste(clipboard_text);
            self.flush_pending_paste();
            self.reset_plain_char_burst();
            return true;
        }
        false
    }

    async fn drain_pending_attachment_paths(&mut self) {
        while let Some(raw_path) = self.pending_attachment_paths.pop_front() {
            let prepared = match crate::attachments::prepare_attachment_from_path(
                &self.runtime,
                raw_path.as_str(),
            )
            .await
            {
                Ok(prepared) => prepared,
                Err(err) => {
                    self.push_log(LogKind::Error, err.to_string());
                    continue;
                }
            };
            self.queue_prepared_attachment(prepared, false);
        }
    }

    fn queue_prepared_attachment(
        &mut self,
        prepared: crate::attachments::PreparedAttachment,
        emit_feedback: bool,
    ) {
        let previous = self.pending_attachments.clone();
        let existing_placeholder = previous
            .iter()
            .enumerate()
            .find(|(_, item)| item.source.eq_ignore_ascii_case(prepared.source.as_str()))
            .map(|(index, item)| attachment_placeholder(item, index));
        if let Some(existing) = self
            .pending_attachments
            .iter()
            .position(|item| item.source.eq_ignore_ascii_case(prepared.source.as_str()))
        {
            self.pending_attachments.remove(existing);
        }
        self.pending_attachments.push(prepared);
        self.input = replace_attachment_placeholders(
            self.input.as_str(),
            previous.as_slice(),
            self.pending_attachments.as_slice(),
        );
        self.input_cursor = clamp_cursor_out_of_attachment_placeholder(
            self.input.as_str(),
            self.input_cursor.min(self.input.len()),
            self.pending_attachments.as_slice(),
        );
        let placeholder_already_visible = existing_placeholder
            .as_deref()
            .is_some_and(|placeholder| self.input.contains(placeholder));
        if !placeholder_already_visible {
            let placeholder_index = self.pending_attachments.len().saturating_sub(1);
            if let Some(item) = self.pending_attachments.get(placeholder_index) {
                let placeholder = attachment_placeholder(item, placeholder_index);
                self.insert_attachment_placeholder_at_cursor(placeholder.as_str());
            }
        }
        self.history_cursor = None;
        if emit_feedback {
            if let Some(last) = self.pending_attachments.last() {
                let summary = crate::attachments::summarize_attachment(
                    last,
                    self.pending_attachments.len().saturating_sub(1),
                    self.display_language.as_str(),
                );
                let message = if self.is_zh_language() {
                    format!("attachment queued (auto-send on next turn): {summary}")
                } else {
                    format!("attachment queued (auto-send on next turn): {summary}")
                };
                self.push_log(LogKind::Info, message);
            }
        }
    }

    pub fn on_paste(&mut self, text: String) {
        let Some(normalized) = normalize_clipboard_text(text) else {
            return;
        };
        self.reset_plain_char_burst();
        self.key_paste_burst_buffer.clear();
        self.key_paste_burst_last_at = None;
        self.clear_suppressed_clipboard_paste();
        self.clear_clipboard_probe_cache();
        if let Some(paths) =
            detect_pasted_attachment_paths(self.runtime.launch_dir.as_path(), normalized.as_str())
        {
            self.pending_attachment_paths.extend(paths);
            return;
        }
        self.pending_paste.push_back(normalized);
    }

    fn paste_from_system_clipboard(&mut self) {
        let mut image_error = None;
        match read_system_clipboard_image_path() {
            Ok(Some(path)) => {
                self.pending_attachment_paths.push_back(path);
                return;
            }
            Ok(None) => {}
            Err(error) => image_error = Some(error),
        }

        match read_system_clipboard_text() {
            Ok(Some(text)) => {
                let normalized = normalize_clipboard_text(text.clone());
                self.on_paste(text);
                self.flush_pending_paste();
                if let Some(normalized) = normalized {
                    self.suppressed_clipboard_paste = normalized;
                    self.suppressed_clipboard_paste_last_at = Some(Instant::now());
                }
            }
            Ok(None) => {}
            Err(error) => {
                let hint = crate::locale::tr(
                    self.display_language.as_str(),
                    "读取系统剪贴板文本失败，请确认终端允许粘贴且剪贴板中包含文本",
                    "failed to read text from system clipboard; ensure terminal paste is allowed and clipboard has text",
                );
                self.push_log(LogKind::Info, format!("{hint}: {error}"));
                return;
            }
        }

        if let Some(error) = image_error {
            let hint = crate::locale::tr(
                self.display_language.as_str(),
                "读取系统剪贴板图片失败，请确认剪贴板中包含图片且 PowerShell 可访问系统剪贴板",
                "failed to read image from system clipboard; ensure the clipboard contains an image and PowerShell can access it",
            );
            self.push_log(LogKind::Info, format!("{hint}: {error}"));
        }
    }

    fn observe_plain_char_event(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.key_char_burst_last_at {
            if now.saturating_duration_since(last) <= PASTE_BURST_CHAR_GAP {
                self.key_char_burst_len = self.key_char_burst_len.saturating_add(1);
            } else {
                self.key_char_burst_len = 1;
            }
        } else {
            self.key_char_burst_len = 1;
        }
        self.key_char_burst_last_at = Some(now);
    }

    fn reset_plain_char_burst(&mut self) {
        self.key_char_burst_len = 0;
        self.key_char_burst_last_at = None;
    }

    fn should_treat_enter_as_paste_newline(&self) -> bool {
        let Some(last) = self.key_char_burst_last_at else {
            return false;
        };
        if Instant::now().saturating_duration_since(last) > PASTE_BURST_ENTER_GAP {
            return false;
        }
        self.key_char_burst_len >= PASTE_BURST_ENTER_CHAR_THRESHOLD
    }

    pub async fn on_key(&mut self, key: KeyEvent) -> Result<()> {
        if self
            .ctrl_c_hint_deadline
            .map(|deadline| Instant::now() > deadline)
            .unwrap_or(false)
        {
            self.ctrl_c_hint_deadline = None;
        }
        if self.mouse_mode == MouseMode::Auto && self.mouse_passthrough_active() {
            self.clear_mouse_passthrough();
        }
        self.clamp_input_cursor_outside_attachment_placeholder();
        if self.try_consume_suppressed_clipboard_key(key) {
            return Ok(());
        }

        let plain_char_input = matches!(key.code, KeyCode::Char(_))
            && (matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT)
                || is_altgr(key.modifiers));
        let interrupt_key = key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('d'));
        let enter_may_extend_paste = matches!(key.code, KeyCode::Enter)
            && (self.key_paste_burst_active() || self.should_treat_enter_as_paste_newline());
        if !plain_char_input && !interrupt_key && !enter_may_extend_paste {
            self.flush_key_paste_burst_if_due(true);
        }

        if self.active_approval.is_some() {
            self.reset_plain_char_burst();
            self.handle_approval_key(key);
            return Ok(());
        }

        if is_paste_shortcut(key) {
            self.reset_plain_char_burst();
            self.focus_area = FocusArea::Input;
            self.paste_from_system_clipboard();
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
                KeyCode::Char('\u{0016}') => {
                    self.reset_plain_char_burst();
                    self.focus_area = FocusArea::Input;
                    self.paste_from_system_clipboard();
                    return Ok(());
                }
                _ => {}
            }
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            self.reset_plain_char_burst();
            match key.code {
                KeyCode::Char('c') | KeyCode::Char('d') => {
                    self.handle_ctrl_c();
                    return Ok(());
                }
                KeyCode::Char('l') => {
                    self.logs.clear();
                    self.reset_scrollback_archive();
                    self.invalidate_transcript_metrics();
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
            self.reset_plain_char_burst();
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
            self.reset_plain_char_burst();
            if matches!(key.code, KeyCode::Esc) {
                self.shortcuts_visible = false;
            }
            return Ok(());
        }

        if self.has_resume_picker() {
            self.reset_plain_char_burst();
            self.handle_resume_picker_key(key).await?;
            return Ok(());
        }

        if let Some(action) = self.try_handle_inquiry_panel_navigation_key(key) {
            self.reset_plain_char_burst();
            if let Some(selection) = action {
                self.submit_line(selection).await?;
            }
            return Ok(());
        }

        if self.focus_area == FocusArea::Transcript {
            self.reset_plain_char_burst();
            if self.handle_transcript_focus_key(key) {
                return Ok(());
            }
            self.focus_area = FocusArea::Input;
        }

        match key.code {
            KeyCode::Esc => {
                self.clear_input_draft();
            }
            KeyCode::Enter => {
                if self.key_paste_burst_active() {
                    self.key_paste_burst_buffer.push('\n');
                    self.key_paste_burst_last_at = Some(Instant::now());
                    self.observe_plain_char_event();
                    return Ok(());
                }
                if self.config_wizard.is_none()
                    && key
                        .modifiers
                        .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT)
                {
                    self.observe_plain_char_event();
                    self.insert_char_at_cursor('\n');
                    return Ok(());
                }
                if self.config_wizard.is_none()
                    && !self.input.trim_start().starts_with('/')
                    && self.should_treat_enter_as_paste_newline()
                {
                    self.insert_char_at_cursor('\n');
                    self.observe_plain_char_event();
                    return Ok(());
                }
                self.reset_plain_char_burst();
                self.flush_key_paste_burst_if_due(true);
                self.flush_pending_paste();

                let raw_line = std::mem::take(&mut self.input);
                self.input_cursor = 0;
                self.history_cursor = None;
                if self.config_wizard.is_some() || !raw_line.trim().is_empty() {
                    self.submit_line(raw_line).await?;
                }
            }
            KeyCode::Backspace => {
                self.reset_plain_char_burst();
                self.backspace_at_cursor();
            }
            KeyCode::Delete => {
                self.reset_plain_char_burst();
                self.delete_at_cursor();
            }
            KeyCode::Left => {
                self.reset_plain_char_burst();
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.move_cursor_word_left();
                } else {
                    self.move_cursor_left();
                }
            }
            KeyCode::Right => {
                self.reset_plain_char_burst();
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.move_cursor_word_right();
                } else {
                    self.move_cursor_right();
                }
            }
            KeyCode::Tab => {
                self.reset_plain_char_burst();
                self.apply_first_suggestion();
            }
            KeyCode::F(2) => {
                self.reset_plain_char_burst();
                self.toggle_mouse_mode();
            }
            KeyCode::F(3) => {
                self.reset_plain_char_burst();
                self.toggle_focus_area();
            }
            KeyCode::PageUp => {
                self.reset_plain_char_burst();
                self.scroll_transcript_up(8);
            }
            KeyCode::PageDown => {
                self.reset_plain_char_burst();
                self.scroll_transcript_down(8);
            }
            KeyCode::Home => {
                self.reset_plain_char_burst();
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.scroll_transcript_to_top();
                } else {
                    self.move_cursor_to_line_start_with_wrap(false);
                }
            }
            KeyCode::End => {
                self.reset_plain_char_burst();
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.scroll_transcript_to_bottom();
                } else {
                    self.move_cursor_to_line_end_with_wrap(false);
                }
            }
            KeyCode::Up => {
                self.reset_plain_char_burst();
                if self.move_popup_selection(-1) {
                    return Ok(());
                }
                if self.should_use_multiline_navigation() {
                    self.move_cursor_up();
                } else {
                    self.history_up();
                }
            }
            KeyCode::Down => {
                self.reset_plain_char_burst();
                if self.move_popup_selection(1) {
                    return Ok(());
                }
                if self.should_use_multiline_navigation() {
                    self.move_cursor_down();
                } else {
                    self.history_down();
                }
            }
            KeyCode::Char('?') => {
                self.observe_plain_char_event();
                if self.try_promote_clipboard_paste('?') {
                    return Ok(());
                }
                if self.handle_plain_char_paste_burst('?') {
                    return Ok(());
                }
                self.insert_char_at_cursor('?');
            }
            KeyCode::Char(ch) => {
                if matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT)
                    || is_altgr(key.modifiers)
                {
                    self.observe_plain_char_event();
                    if self.try_promote_clipboard_paste(ch) {
                        return Ok(());
                    }
                    if self.handle_plain_char_paste_burst(ch) {
                        return Ok(());
                    }
                    self.insert_char_at_cursor(ch);
                }
            }
            _ => self.reset_plain_char_burst(),
        }
        Ok(())
    }

    fn handle_ctrl_c(&mut self) {
        if self.config_wizard.is_some() {
            self.cancel_config_wizard();
            return;
        }
        if self.clear_input_draft() {
            return;
        }

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
        if self.active_approval.is_none() {
            return;
        }

        match key.code {
            KeyCode::Up => {
                self.approval_selected_index = self.approval_selected_index.saturating_sub(1);
                return;
            }
            KeyCode::Down => {
                self.approval_selected_index = (self.approval_selected_index + 1).min(2);
                return;
            }
            _ => {}
        }

        let response = match key.code {
            KeyCode::Esc | KeyCode::Char('3') | KeyCode::Char('n') | KeyCode::Char('N') => {
                Some(ApprovalResponse::Deny)
            }
            KeyCode::Char('1') | KeyCode::Char('y') | KeyCode::Char('Y') => {
                Some(ApprovalResponse::ApproveOnce)
            }
            KeyCode::Char('2') | KeyCode::Char('a') | KeyCode::Char('A') => {
                Some(ApprovalResponse::ApproveSession)
            }
            KeyCode::Enter => Some(match self.approval_selected_index.min(2) {
                1 => ApprovalResponse::ApproveSession,
                2 => ApprovalResponse::Deny,
                _ => ApprovalResponse::ApproveOnce,
            }),
            _ => None,
        };

        let Some(response) = response else {
            return;
        };

        let Some(request) = self.active_approval.take() else {
            return;
        };

        let _ = request.respond_to.send(response);
        match response {
            ApprovalResponse::ApproveOnce => self.push_log(
                LogKind::Info,
                if self.is_zh_language() {
                    format!("审批通过（仅本次）：{}", request.summary)
                } else {
                    format!("approved once: {}", request.summary)
                },
            ),
            ApprovalResponse::ApproveSession => self.push_log(
                LogKind::Info,
                if self.is_zh_language() {
                    format!("审批通过（本会话）：{}", request.summary)
                } else {
                    format!("approved for session: {}", request.summary)
                },
            ),
            ApprovalResponse::Deny => self.push_log(
                LogKind::Info,
                if self.is_zh_language() {
                    format!("已拒绝：{}", request.summary)
                } else {
                    format!("denied: {}", request.summary)
                },
            ),
        };

        self.active_approval = self.approval_queue.pop_front();
        self.approval_selected_index = 0;
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
            KeyCode::Esc => {
                self.focus_area = FocusArea::Input;
                true
            }
            KeyCode::Enter => {
                if !self.prefill_selected_user_message() {
                    self.focus_area = FocusArea::Input;
                }
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

    fn prefill_selected_user_message(&mut self) -> bool {
        let Some(index) = self.transcript_selected else {
            return false;
        };
        let Some(entry) = self.logs.get(index) else {
            return false;
        };
        let Some(text) = backtrack_user_text(entry) else {
            return false;
        };
        self.input = text;
        self.input_cursor = self.input.len();
        self.pending_large_pastes.clear();
        self.history_cursor = None;
        self.focus_area = FocusArea::Input;
        self.push_log(
            LogKind::Info,
            crate::locale::tr(
                self.display_language.as_str(),
                "已将选中的用户消息填入输入框，可继续编辑后发送",
                "selected user message loaded into input; edit and send",
            ),
        );
        true
    }

    fn handle_backtrack_slash(&mut self, args: &str) {
        let candidates = collect_recent_user_logs(&self.logs, 20);
        if candidates.is_empty() {
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "当前会话没有可回溯的用户消息",
                    "no user turns available for backtrack in this session",
                ),
            );
            return;
        }

        let cleaned = args.trim();
        if cleaned.is_empty() {
            let first = candidates.first().cloned().unwrap_or_default();
            self.prefill_backtrack_text(first.as_str(), 1);
            return;
        }

        if cleaned.eq_ignore_ascii_case("list") {
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "最近的用户消息（1 为最新）：",
                    "recent user turns (1 is latest):",
                ),
            );
            for (index, item) in candidates.iter().enumerate() {
                self.push_log(
                    LogKind::Info,
                    format!(
                        "{:>2}. {}",
                        index + 1,
                        backtrack_preview_line(item.as_str(), 120)
                    ),
                );
            }
            return;
        }

        let index = cleaned.parse::<usize>().ok().filter(|value| *value >= 1);
        let Some(index) = index else {
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "用法：/backtrack [list|index]",
                    "usage: /backtrack [list|index]",
                ),
            );
            return;
        };
        let Some(selected) = candidates.get(index.saturating_sub(1)) else {
            if self.is_zh_language() {
                self.push_log(LogKind::Error, format!("回溯索引超出范围: {index}"));
            } else {
                self.push_log(
                    LogKind::Error,
                    format!("backtrack index out of range: {index}"),
                );
            }
            return;
        };
        self.prefill_backtrack_text(selected, index);
    }

    fn prefill_backtrack_text(&mut self, text: &str, index: usize) {
        let cleaned = text.trim();
        if cleaned.is_empty() {
            return;
        }
        self.input = cleaned.to_string();
        self.input_cursor = self.input.len();
        self.pending_large_pastes.clear();
        self.history_cursor = None;
        self.focus_area = FocusArea::Input;
        if self.is_zh_language() {
            self.push_log(
                LogKind::Info,
                format!("已将用户消息 #{index} 填入输入框，可继续编辑后发送"),
            );
        } else {
            self.push_log(
                LogKind::Info,
                format!("loaded user turn #{index} into input; edit and send"),
            );
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
                self.push_log(LogKind::Info, format!("当前已处于会话: {cleaned}"));
            } else {
                self.push_log(LogKind::Info, format!("already using session: {cleaned}"));
            }
            return Ok(());
        }

        if !crate::session_exists(&self.runtime, cleaned).await? {
            if self.is_zh_language() {
                self.push_log(LogKind::Error, format!("会话不存在: {cleaned}"));
                self.push_log(LogKind::Info, "提示：使用 /resume 列出可用会话".to_string());
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
        self.pending_large_pastes.clear();
        self.history_cursor = None;
        self.config_wizard = None;
        self.last_usage = None;
        self.reset_turn_metrics_snapshot();
        self.active_assistant = None;
        self.active_reasoning = None;
        self.stream_saw_output = false;
        self.stream_saw_final = false;
        self.stream_received_content_delta = false;
        self.stream_tool_markup_open = false;
        self.turn_final_answer.clear();
        self.turn_final_stop_reason = None;
        self.tool_phase_notice_emitted = false;
        self.reset_stream_catchup_state();
        self.reset_plain_char_burst();
        self.approval_rx = None;
        self.active_approval = None;
        self.approval_queue.clear();
        self.approval_selected_index = 0;
        self.ctrl_c_hint_deadline = None;
        self.transcript_offset_from_bottom = 0;
        self.transcript_selected = None;
        self.focus_area = FocusArea::Input;
        self.resume_picker = None;
        self.active_inquiry_panel = None;
        self.inquiry_selected_index = 0;
        self.logs.clear();
        self.reset_scrollback_archive();
        self.invalidate_transcript_metrics();

        let restored = self.restore_transcript_from_history(history);
        self.session_stats = crate::SessionStatsSnapshot::default();
        self.reload_session_stats().await;
        if self.is_zh_language() {
            self.push_log(
                LogKind::Info,
                format!("已恢复会话 {}（已恢复 {restored} 条消息）", self.session_id),
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
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Right)) {
            self.focus_area = FocusArea::Input;
            self.paste_from_system_clipboard();
            return;
        }

        match self.mouse_mode {
            MouseMode::Select => {}
            MouseMode::Scroll => match mouse.kind {
                MouseEventKind::ScrollUp if self.mouse_in_transcript_region(&mouse) => {
                    self.scroll_transcript_up(3)
                }
                MouseEventKind::ScrollDown if self.mouse_in_transcript_region(&mouse) => {
                    self.scroll_transcript_down(3)
                }
                _ => {}
            },
            MouseMode::Auto => match mouse.kind {
                MouseEventKind::Drag(MouseButton::Left) => {
                    if self.mouse_in_region(&mouse, self.input_mouse_region) {
                        self.focus_area = FocusArea::Input;
                    }
                }
                MouseEventKind::ScrollUp if self.mouse_in_transcript_region(&mouse) => {
                    self.scroll_transcript_up(3)
                }
                MouseEventKind::ScrollDown if self.mouse_in_transcript_region(&mouse) => {
                    self.scroll_transcript_down(3)
                }
                _ => {}
            },
        }
    }

    pub async fn submit_line(&mut self, line: String) -> Result<()> {
        self.flush_pending_paste();
        self.sync_attachment_placeholders();
        let visible_line = line.trim_end().to_string();
        let expanded_line =
            expand_large_paste_placeholders(visible_line.as_str(), &self.pending_large_pastes);
        self.pending_large_pastes.clear();
        if self.config_wizard.is_some() {
            return self.handle_config_wizard_input(expanded_line.trim()).await;
        }
        self.shortcuts_visible = false;
        self.resume_picker = None;
        self.focus_area = FocusArea::Input;

        self.drain_pending_attachment_paths().await;
        self.sync_attachment_placeholders();

        let has_attachments = !self.pending_attachments.is_empty();
        let mut prompt = strip_attachment_placeholders(
            expanded_line.as_str(),
            self.pending_attachments.as_slice(),
        )
        .trim_end()
        .to_string();
        let mut user_echo = visible_line;
        if prompt.trim().is_empty() && !has_attachments {
            return Ok(());
        }
        if let Some(converted) = self.try_convert_inquiry_input(prompt.as_str()) {
            if converted.trim().is_empty() && !has_attachments {
                return Ok(());
            }
            prompt = converted.clone();
            if !has_attachments {
                user_echo = converted;
            }
        } else if self.active_inquiry_panel.is_some() && !prompt.trim_start().starts_with('/') {
            self.active_inquiry_panel = None;
            self.inquiry_selected_index = 0;
        }

        self.scroll_transcript_to_bottom();
        self.track_popup_tokens_from_text(prompt.as_str());

        if prompt.trim_start().starts_with('/') {
            return self.handle_slash_command(prompt.trim().to_string()).await;
        }
        if !prompt.trim().is_empty() {
            self.push_history(user_echo.trim());
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
        self.turn_final_answer.clear();
        self.turn_final_stop_reason = None;
        self.tool_phase_notice_emitted = false;
        let request_attachments =
            crate::attachments::to_request_attachments(&self.pending_attachments);
        if let Err(err) = crate::input_guard::validate_request_text_input_size(
            self.display_language.as_str(),
            prompt.as_str(),
            request_attachments.as_deref(),
        ) {
            self.push_log(LogKind::Error, err.to_string());
            return Ok(());
        }
        self.start_stream_request(prompt, user_echo, request_attachments)
            .await?;
        self.clear_pending_attachments();
        Ok(())
    }

    async fn start_stream_request(
        &mut self,
        prompt: String,
        user_echo: String,
        attachments: Option<Vec<wunder_server::schemas::AttachmentPayload>>,
    ) -> Result<()> {
        if self.busy {
            self.push_log(
                LogKind::Error,
                "assistant is still running, wait for completion before sending a new prompt"
                    .to_string(),
            );
            return Ok(());
        }

        self.ctrl_c_hint_deadline = None;
        self.active_inquiry_panel = None;
        self.inquiry_selected_index = 0;
        self.push_log(LogKind::User, user_echo);
        self.busy = true;
        self.active_assistant = None;
        self.active_reasoning = None;
        self.turn_final_answer.clear();
        self.turn_final_stop_reason = None;
        self.begin_turn_metrics();
        self.request_redraw();

        let (approval_tx, approval_rx) = new_approval_channel();
        self.approval_rx = Some(approval_rx);
        self.approval_queue.clear();
        self.active_approval = None;
        self.approval_selected_index = 0;

        let mut request = crate::build_wunder_request(
            &self.runtime,
            &self.global,
            &prompt,
            &self.session_id,
            self.agent_id_override.as_deref(),
            attachments,
        )
        .await?;
        request.approval_tx = Some(approval_tx);
        let orchestrator = self.runtime.state.kernel.orchestrator.clone();
        let frame_requester = self.frame_requester.clone();
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
                        frame_requester.schedule_frame();
                    }
                }
                Err(err) => {
                    let _ = tx.send(StreamMessage::Error(err.to_string()));
                    frame_requester.schedule_frame();
                }
            }
            let _ = tx.send(StreamMessage::Done);
            frame_requester.schedule_frame();
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
            let suggestions = slash_command::command_completions(body, POPUP_MAX_CANDIDATES);
            self.sync_popup_selection(suggestions.len());
            let Some(suggestion) = suggestions
                .get(
                    self.popup_selected_index
                        .min(suggestions.len().saturating_sub(1)),
                )
                .cloned()
            else {
                return;
            };
            self.input = format!("/{suggestion} ");
            self.input_cursor = self.input.len();
            self.pending_large_pastes.clear();
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
        let suggestions = if let Some(query) = token.strip_prefix('@') {
            self.mention_popup_tokens(query, POPUP_MAX_CANDIDATES)
        } else if let Some(query) = token.strip_prefix('$') {
            self.app_popup_tokens(query, POPUP_MAX_CANDIDATES)
        } else if let Some(query) = token.strip_prefix('#') {
            self.skill_popup_tokens(query, POPUP_MAX_CANDIDATES)
        } else {
            return;
        };
        self.sync_popup_selection(suggestions.len());
        let Some(selected) = suggestions
            .get(
                self.popup_selected_index
                    .min(suggestions.len().saturating_sub(1)),
            )
            .cloned()
        else {
            return;
        };
        let replacement = format!("{selected} ");
        self.input.replace_range(token_start..cursor, &replacement);
        self.input_cursor = token_start.saturating_add(replacement.len());
        self.mark_popup_recent(selected.as_str());
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
            self.pending_large_pastes.clear();
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
            self.pending_large_pastes.clear();
            return;
        }
        self.history_cursor = Some(next);
        self.input = self.history.get(next).cloned().unwrap_or_default();
        self.input_cursor = self.input.len();
        self.pending_large_pastes.clear();
    }

    fn push_history(&mut self, value: &str) {
        if !should_store_history_entry(value) {
            return;
        }
        let cleaned = value.trim();
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

    fn track_popup_tokens_from_text(&mut self, text: &str) {
        for raw in text.split_whitespace() {
            if let Some(token) = normalize_popup_token(raw) {
                self.mark_popup_recent(token.as_str());
            }
        }
    }

    fn insert_char_at_cursor(&mut self, ch: char) {
        self.clamp_input_cursor_outside_attachment_placeholder();
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
        self.clamp_input_cursor_outside_attachment_placeholder();
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
        if let Some(matched) = find_attachment_placeholder_covering_cursor(
            self.input.as_str(),
            self.input_cursor.min(self.input.len()),
            self.pending_attachments.as_slice(),
            true,
        ) {
            self.remove_attachment_placeholder_match(matched);
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
        if let Some(matched) = find_attachment_placeholder_covering_cursor(
            self.input.as_str(),
            self.input_cursor.min(self.input.len()),
            self.pending_attachments.as_slice(),
            false,
        ) {
            self.remove_attachment_placeholder_match(matched);
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
        self.clamp_input_cursor_outside_attachment_placeholder();
    }

    fn move_cursor_right(&mut self) {
        if self.input_cursor >= self.input.len() {
            return;
        }
        self.input_cursor = next_char_boundary(&self.input, self.input_cursor);
        self.clamp_input_cursor_outside_attachment_placeholder();
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
        self.clamp_input_cursor_outside_attachment_placeholder();
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
        self.clamp_input_cursor_outside_attachment_placeholder();
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
        self.clamp_input_cursor_outside_attachment_placeholder();
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
        self.clamp_input_cursor_outside_attachment_placeholder();
    }

    fn delete_word_left(&mut self) {
        let end = self.input_cursor.min(self.input.len());
        if end == 0 {
            return;
        }
        if let Some(matched) = find_attachment_placeholder_covering_cursor(
            self.input.as_str(),
            end,
            self.pending_attachments.as_slice(),
            true,
        ) {
            self.remove_attachment_placeholder_match(matched);
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
        if let Some(matched) = find_attachment_placeholder_covering_cursor(
            self.input.as_str(),
            start,
            self.pending_attachments.as_slice(),
            false,
        ) {
            self.remove_attachment_placeholder_match(matched);
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
        if let Some(matched) = find_attachment_placeholder_covering_cursor(
            self.input.as_str(),
            end,
            self.pending_attachments.as_slice(),
            true,
        ) {
            self.remove_attachment_placeholder_match(matched);
            return;
        }
        let start = self.line_start_index();
        if start < end {
            self.input.replace_range(start..end, "");
            self.input_cursor = start;
        }
    }

    fn delete_to_line_end(&mut self) {
        let start = self.input_cursor.min(self.input.len());
        if let Some(matched) = find_attachment_placeholder_covering_cursor(
            self.input.as_str(),
            start,
            self.pending_attachments.as_slice(),
            false,
        ) {
            self.remove_attachment_placeholder_match(matched);
            return;
        }
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
        self.clamp_input_cursor_outside_attachment_placeholder();
    }

    fn move_cursor_down(&mut self) {
        self.input_cursor = move_cursor_vertical(
            &self.input,
            usize::from(self.input_viewport_width.max(1)),
            self.input_cursor,
            1,
        );
        self.clamp_input_cursor_outside_attachment_placeholder();
    }

    fn begin_turn_metrics(&mut self) {
        self.turn_llm_started_at = None;
        self.turn_llm_active_secs = 0.0;
        self.turn_output_tokens = 0;
        self.turn_tool_calls = 0;
    }

    fn reset_turn_metrics_snapshot(&mut self) {
        self.turn_llm_started_at = None;
        self.turn_llm_active_secs = 0.0;
        self.turn_output_tokens = 0;
        self.turn_tool_calls = 0;
        self.last_turn_elapsed_secs = None;
        self.last_turn_speed_tps = None;
        self.last_turn_tool_calls = 0;
    }

    fn start_llm_active_window(&mut self) {
        if self.turn_llm_started_at.is_none() {
            self.turn_llm_started_at = Some(Instant::now());
        }
    }

    fn stop_llm_active_window(&mut self) {
        if let Some(started) = self.turn_llm_started_at.take() {
            self.turn_llm_active_secs += started.elapsed().as_secs_f64();
        }
    }

    fn finalize_turn_metrics(&mut self) {
        self.stop_llm_active_window();
        let elapsed = if self.turn_llm_active_secs > f64::EPSILON {
            Some(self.turn_llm_active_secs)
        } else {
            None
        };
        self.last_turn_elapsed_secs = elapsed;
        self.last_turn_tool_calls = self.turn_tool_calls;
        self.last_turn_speed_tps = elapsed.map(|seconds| {
            if seconds <= f64::EPSILON {
                0.0
            } else {
                self.turn_output_tokens as f64 / seconds
            }
        });
        self.turn_llm_started_at = None;
        self.turn_llm_active_secs = 0.0;
        self.turn_output_tokens = 0;
        self.turn_tool_calls = 0;
    }

    fn handle_stream_message(&mut self, message: StreamMessage) {
        match message {
            StreamMessage::Event(event) => self.apply_stream_event(event),
            StreamMessage::Error(err) => {
                self.finalize_all_markdown_streams();
                self.push_log(LogKind::Error, err);
                self.finalize_turn_metrics();
                self.busy = false;
                self.active_assistant = None;
                self.active_reasoning = None;
                self.stream_rx = None;
                self.approval_rx = None;
                self.active_approval = None;
                self.approval_queue.clear();
                self.approval_selected_index = 0;
                self.stream_saw_output = false;
                self.stream_saw_final = false;
                self.stream_goal_continue_ready = false;
                self.stream_received_content_delta = false;
                self.stream_tool_markup_open = false;
                self.turn_final_answer.clear();
                self.turn_final_stop_reason = None;
                self.tool_phase_notice_emitted = false;
                self.session_stats_dirty = true;
                self.refresh_workspace_context();
            }
            StreamMessage::Done => {
                let should_continue_goal = self.stream_goal_continue_ready;
                self.finalize_all_markdown_streams();
                self.maybe_emit_tool_only_final_summary();
                if !self.stream_saw_output && !self.stream_saw_final {
                    self.push_log(
                        LogKind::Error,
                        "stream ended without model output or final answer".to_string(),
                    );
                }
                let final_event = self.notification_final_event();
                crate::emit_turn_complete_notification(
                    &self.runtime,
                    self.session_id.as_str(),
                    &final_event,
                    "tui",
                    Some(self.terminal_focused),
                );
                self.finalize_turn_metrics();
                self.busy = false;
                self.active_assistant = None;
                self.active_reasoning = None;
                self.stream_rx = None;
                self.approval_rx = None;
                self.active_approval = None;
                self.approval_queue.clear();
                self.approval_selected_index = 0;
                self.stream_saw_output = false;
                self.stream_saw_final = false;
                self.stream_goal_continue_ready = false;
                self.stream_received_content_delta = false;
                self.stream_tool_markup_open = false;
                self.turn_final_answer.clear();
                self.turn_final_stop_reason = None;
                self.tool_phase_notice_emitted = false;
                self.session_stats_dirty = true;
                self.refresh_workspace_context();
                if should_continue_goal {
                    let runtime = self.runtime.clone();
                    let user_id = self.runtime.user_id.clone();
                    let session_id = self.session_id.clone();
                    tokio::spawn(async move {
                        runtime
                            .state
                            .kernel
                            .thread_runtime
                            .spawn_goal_continuation_after_cooldown(user_id, session_id);
                    });
                }
            }
        }
    }

    fn notification_final_event(&self) -> FinalEvent {
        let answer = if !self.turn_final_answer.trim().is_empty() {
            self.turn_final_answer.clone()
        } else {
            self.logs
                .iter()
                .rev()
                .find(|entry| matches!(entry.kind, LogKind::Assistant))
                .map(|entry| entry.text.clone())
                .unwrap_or_default()
        };
        FinalEvent {
            answer,
            usage: None,
            stop_reason: self.turn_final_stop_reason.clone(),
        }
    }

    fn maybe_emit_tool_only_final_summary(&mut self) {
        if !self.stream_saw_final
            || !self.turn_final_answer.trim().is_empty()
            || self.turn_tool_calls == 0
            || !self
                .logs
                .last()
                .is_some_and(|entry| matches!(entry.kind, LogKind::Tool))
        {
            return;
        }

        let message = if self.is_zh_language() {
            "已完成本轮任务，结果见上方工具输出。"
        } else {
            "Done. Review the tool results above."
        }
        .to_string();
        self.turn_final_answer = message.clone();
        self.stream_saw_output = true;
        self.push_log(LogKind::Assistant, message);
    }

    fn apply_stream_event(&mut self, event: StreamEvent) {
        let payload = event_payload(&event.data);
        match event.event.as_str() {
            "goal_continuation_ready" => {
                self.stream_goal_continue_ready = true;
            }
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
                // Skip progress events in tool-only workflow rendering.
            }
            "llm_request" => {
                self.session_stats.model_calls = self.session_stats.model_calls.saturating_add(1);
                self.active_assistant = None;
                self.active_reasoning = None;
                self.stream_received_content_delta = false;
                self.stream_tool_markup_open = false;
                self.tool_phase_notice_emitted = false;
                self.clear_markdown_streams();
                self.stop_llm_active_window();
                self.start_llm_active_window();
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
                let speed_tokens = if output_tokens > 0 {
                    output_tokens
                } else {
                    total_tokens
                };
                self.turn_output_tokens = self.turn_output_tokens.saturating_add(speed_tokens);
                if total_tokens > 0 {
                    self.last_usage = Some(total_tokens.to_string());
                }
            }
            "tool_call" => {
                self.stop_llm_active_window();
                self.session_stats.tool_calls = self.session_stats.tool_calls.saturating_add(1);
                self.turn_tool_calls = self.turn_tool_calls.saturating_add(1);
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
                if is_apply_patch_tool_name(tool) {
                    if !self.push_patch_call_log(args) {
                        self.push_log(LogKind::Tool, format_tool_call_line(tool, args));
                    }
                } else if is_execute_command_tool_name(tool) {
                    if !self.push_command_call_log(args) {
                        self.push_log(LogKind::Tool, format_tool_call_line(tool, args));
                    }
                } else {
                    self.push_generic_tool_call_log(tool, args);
                }
            }
            "tool_result" => {
                self.session_stats.tool_results = self.session_stats.tool_results.saturating_add(1);
                let tool = payload
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                if is_apply_patch_tool_name(tool) {
                    self.complete_patch_log(payload);
                } else if is_execute_command_tool_name(tool) {
                    if !self.complete_command_log(payload) {
                        for line in format_tool_result_lines(tool, payload) {
                            self.push_log(LogKind::Tool, line);
                        }
                    }
                } else {
                    self.complete_generic_tool_log(tool, payload);
                }
                if let Some(panel) = self.parse_inquiry_panel_from_tool_result(payload) {
                    self.activate_inquiry_panel(panel, true);
                }
            }
            "question_panel" => {
                if let Some(panel) = self.parse_inquiry_panel_state(payload) {
                    self.activate_inquiry_panel(panel, true);
                }
            }
            "error" => {
                self.push_log(
                    LogKind::Error,
                    format!("[error] {}", parse_error_message(payload)),
                );
            }
            "final" => {
                self.stop_llm_active_window();
                self.stream_saw_final = true;
                self.turn_final_stop_reason = payload
                    .get("stop_reason")
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
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
                    let cleaned = sanitize_assistant_text(answer);
                    self.turn_final_answer = cleaned.clone();
                    self.merge_final_answer(cleaned.as_str());
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
        if index < self.history_archived_entries {
            self.history_archived_entries = self.history_archived_entries.saturating_sub(1);
        }
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
        self.adjust_markdown_stream_indices_after_remove(index);
        if self.logs.is_empty() {
            self.reset_scrollback_archive();
        }
        self.invalidate_transcript_metrics();
    }

    fn pop_oldest_log_entry(&mut self) {
        if self.logs.is_empty() {
            return;
        }
        self.logs.remove(0);
        if self.history_archived_entries > 0 {
            self.history_archived_entries = self.history_archived_entries.saturating_sub(1);
        }
        if let Some(index) = self.active_assistant.as_mut() {
            *index = index.saturating_sub(1);
        }
        if let Some(index) = self.active_reasoning.as_mut() {
            *index = index.saturating_sub(1);
        }
        if let Some(index) = self.transcript_selected.as_mut() {
            *index = index.saturating_sub(1);
        }
        self.adjust_markdown_stream_indices_after_remove(0);
        if self.logs.is_empty() {
            self.reset_scrollback_archive();
        }
        self.invalidate_transcript_metrics();
    }

    fn total_log_chars(&self) -> usize {
        self.logs
            .iter()
            .map(|entry| entry.text.chars().count())
            .sum()
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
                    entry.markdown_cache = None;
                    self.invalidate_transcript_metrics();
                    has_meaningful_assistant = true;
                }
            }
            if !has_meaningful_assistant {
                self.clear_markdown_stream_for(LogKind::Assistant, index);
                self.remove_log_entry(index);
            }
            if has_meaningful_assistant {
                self.clear_markdown_stream_for(LogKind::Assistant, index);
            }
        }

        if !has_meaningful_assistant {
            self.push_log(
                LogKind::Assistant,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "正在调用工具...",
                    "calling tools...",
                ),
            );
        }
        self.tool_phase_notice_emitted = true;
    }

    fn merge_assistant_delta(&mut self, delta: &str) {
        if delta.trim().is_empty() {
            return;
        }

        let index = self.ensure_assistant_entry();
        let appended = if let Some(entry) = self.logs.get_mut(index) {
            merge_stream_text_with_delta(&mut entry.text, delta)
        } else {
            None
        };
        let Some(appended) = appended else {
            return;
        };
        self.stream_markdown_delta(LogKind::Assistant, index, appended.as_str());
        self.invalidate_transcript_metrics();
    }

    fn replace_assistant_content(&mut self, content: &str) {
        let cleaned = sanitize_assistant_text(content);
        if cleaned.is_empty() {
            return;
        }

        let index = self.ensure_assistant_entry();
        let streaming_active = self.entry_is_streaming(LogKind::Assistant, index);
        let mut updated = false;
        if let Some(entry) = self.logs.get_mut(index) {
            if is_equivalent_text(entry.text.as_str(), cleaned.as_str())
                && entry.text.chars().count() >= cleaned.chars().count()
            {
                updated = false;
            } else {
                entry.text = cleaned;
                entry.markdown_cache = None;
                updated = true;
            }
        }
        let mut needs_invalidate = updated;
        if streaming_active {
            if updated {
                self.clear_markdown_stream_for(LogKind::Assistant, index);
            } else {
                self.finalize_markdown_stream_for(LogKind::Assistant, index);
                needs_invalidate = true;
            }
        }
        if needs_invalidate {
            self.invalidate_transcript_metrics();
        }
    }

    fn merge_reasoning_delta(&mut self, delta: &str) {
        let cleaned = sanitize_reasoning_text(delta);
        if cleaned.is_empty() {
            return;
        }

        let index = self.ensure_reasoning_entry();
        let appended = if let Some(entry) = self.logs.get_mut(index) {
            merge_stream_text_with_delta(&mut entry.text, cleaned.as_str())
        } else {
            None
        };
        let Some(appended) = appended else {
            return;
        };
        self.stream_markdown_delta(LogKind::Reasoning, index, appended.as_str());
        self.invalidate_transcript_metrics();
    }

    fn merge_reasoning_content(&mut self, reasoning: &str) {
        let cleaned = sanitize_reasoning_text(reasoning);
        if cleaned.is_empty() {
            return;
        }

        let index = self.ensure_reasoning_entry();
        let streaming_active = self.entry_is_streaming(LogKind::Reasoning, index);
        let mut updated = false;
        if let Some(entry) = self.logs.get_mut(index) {
            if is_equivalent_text(entry.text.as_str(), cleaned.as_str()) {
                if cleaned.chars().count() >= entry.text.chars().count() {
                    entry.text = cleaned;
                    entry.markdown_cache = None;
                    updated = true;
                }
            } else if compact_text_for_compare(cleaned.as_str())
                .starts_with(compact_text_for_compare(entry.text.as_str()).as_str())
            {
                entry.text = cleaned;
                entry.markdown_cache = None;
                updated = true;
            } else {
                merge_stream_text(&mut entry.text, cleaned.as_str());
                entry.markdown_cache = None;
                updated = true;
            }
        }
        let mut needs_invalidate = updated;
        if streaming_active {
            if updated {
                self.clear_markdown_stream_for(LogKind::Reasoning, index);
            } else {
                self.finalize_markdown_stream_for(LogKind::Reasoning, index);
                needs_invalidate = true;
            }
        }
        if needs_invalidate {
            self.invalidate_transcript_metrics();
        }
    }

    fn merge_assistant_content(&mut self, content: &str) {
        let cleaned = sanitize_assistant_text(content);
        if cleaned.is_empty() {
            return;
        }

        let index = self.ensure_assistant_entry();
        let streaming_active = self.entry_is_streaming(LogKind::Assistant, index);
        let mut updated = false;
        if let Some(entry) = self.logs.get_mut(index) {
            let current = entry.text.trim();
            if current.is_empty() {
                entry.text = cleaned;
                entry.markdown_cache = None;
                updated = true;
            } else if is_equivalent_text(current, cleaned.as_str())
                || compact_text_for_compare(current)
                    .ends_with(compact_text_for_compare(cleaned.as_str()).as_str())
            {
                updated = false;
            } else if compact_text_for_compare(cleaned.as_str())
                .starts_with(compact_text_for_compare(current).as_str())
            {
                entry.text = cleaned;
                entry.markdown_cache = None;
                updated = true;
            } else {
                if !entry.text.ends_with('\n') {
                    entry.text.push('\n');
                }
                entry.text.push_str(cleaned.as_str());
                entry.markdown_cache = None;
                updated = true;
            }
        }
        let mut needs_invalidate = updated;
        if streaming_active {
            if updated {
                self.clear_markdown_stream_for(LogKind::Assistant, index);
            } else {
                self.finalize_markdown_stream_for(LogKind::Assistant, index);
                needs_invalidate = true;
            }
        }
        if needs_invalidate {
            self.invalidate_transcript_metrics();
        }
    }

    fn merge_final_answer(&mut self, answer: &str) {
        let cleaned = sanitize_assistant_text(answer);
        if cleaned.is_empty() {
            return;
        }

        let index = self.ensure_assistant_entry();
        let streaming_active = self.entry_is_streaming(LogKind::Assistant, index);
        let mut updated = false;
        if let Some(entry) = self.logs.get_mut(index) {
            let current = entry.text.trim();
            if current.is_empty() {
                entry.text = cleaned;
                entry.markdown_cache = None;
                updated = true;
            } else if is_equivalent_text(current, cleaned.as_str())
                || compact_text_for_compare(current)
                    .ends_with(compact_text_for_compare(cleaned.as_str()).as_str())
            {
                if cleaned.chars().count() > current.chars().count() {
                    entry.text = cleaned;
                    entry.markdown_cache = None;
                    updated = true;
                }
            } else if compact_text_for_compare(cleaned.as_str())
                .starts_with(compact_text_for_compare(current).as_str())
            {
                entry.text = cleaned;
                entry.markdown_cache = None;
                updated = true;
            } else {
                if !entry.text.ends_with("\n\n") {
                    if !entry.text.ends_with('\n') {
                        entry.text.push('\n');
                    }
                    entry.text.push('\n');
                }
                entry.text.push_str(cleaned.as_str());
                entry.markdown_cache = None;
                updated = true;
            }
        }
        let mut needs_invalidate = updated;
        if streaming_active {
            if updated {
                self.clear_markdown_stream_for(LogKind::Assistant, index);
            } else {
                self.finalize_markdown_stream_for(LogKind::Assistant, index);
                needs_invalidate = true;
            }
        }
        if needs_invalidate {
            self.invalidate_transcript_metrics();
        }
    }

    fn push_log(&mut self, kind: LogKind, text: String) -> usize {
        let text = if matches!(kind, LogKind::Info | LogKind::Error) {
            localize_cli_notice(self.display_language.as_str(), text.as_str())
        } else {
            text
        };
        self.logs.push(LogEntry {
            kind,
            text,
            special: None,
            markdown_cache: None,
        });
        self.invalidate_transcript_metrics();
        while self.logs.len() > MAX_LOG_ENTRIES {
            self.pop_oldest_log_entry();
        }
        while self.logs.len() > 1 && self.total_log_chars() > MAX_LOG_TOTAL_CHARS {
            self.pop_oldest_log_entry();
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

    fn push_special_log(&mut self, kind: LogKind, text: String, special: SpecialLogEntry) -> usize {
        self.logs.push(LogEntry {
            kind,
            text,
            special: Some(special),
            markdown_cache: None,
        });
        self.invalidate_transcript_metrics();
        while self.logs.len() > MAX_LOG_ENTRIES {
            self.pop_oldest_log_entry();
        }
        while self.logs.len() > 1 && self.total_log_chars() > MAX_LOG_TOTAL_CHARS {
            self.pop_oldest_log_entry();
        }

        if self.logs.is_empty() {
            self.transcript_selected = None;
            0
        } else {
            if self.focus_area == FocusArea::Transcript {
                self.transcript_selected = Some(self.logs.len().saturating_sub(1));
            } else if let Some(index) = self.transcript_selected {
                let max_index = self.logs.len().saturating_sub(1);
                self.transcript_selected = Some(index.min(max_index));
            }
            self.logs.len().saturating_sub(1)
        }
    }

    fn push_patch_call_log(&mut self, args: &Value) -> bool {
        let Some(special) = build_pending_patch_log(args, self.is_zh_language()) else {
            return false;
        };
        let text = special.summary_text();
        self.push_special_log(LogKind::Tool, text, special);
        true
    }

    fn push_command_call_log(&mut self, args: &Value) -> bool {
        let Some(special) = build_pending_command_log(args, self.is_zh_language()) else {
            return false;
        };
        let text = special.summary_text();
        self.push_special_log(LogKind::Tool, text, special);
        true
    }

    fn push_generic_tool_call_log(&mut self, tool: &str, args: &Value) {
        let special = build_pending_tool_log(tool, args);
        let text = special.summary_text();
        self.push_special_log(LogKind::Tool, text, special);
    }

    fn complete_patch_log(&mut self, payload: &Value) {
        let mut special = build_completed_patch_log(payload, self.is_zh_language());
        if let Some(index) = self.logs.iter().rposition(|entry| {
            entry
                .special
                .as_ref()
                .is_some_and(SpecialLogEntry::is_pending_patch)
        }) {
            let previous_special = self.logs.get(index).and_then(|entry| entry.special.clone());
            if let Some(previous_special) = previous_special.as_ref() {
                special.inherit_patch_preview_from(previous_special);
            }
            let text = special.summary_text();
            if let Some(entry) = self.logs.get_mut(index) {
                entry.text = text;
                entry.special = Some(special);
                entry.markdown_cache = None;
            }
            self.invalidate_transcript_metrics();
            return;
        }
        let text = special.summary_text();
        self.push_special_log(LogKind::Tool, text, special);
    }

    fn complete_command_log(&mut self, payload: &Value) -> bool {
        let Some(special) = build_completed_command_log(payload, self.is_zh_language()) else {
            return false;
        };
        let text = special.summary_text();
        if let Some(index) = self.logs.iter().rposition(|entry| {
            entry
                .special
                .as_ref()
                .is_some_and(SpecialLogEntry::is_pending_command)
        }) {
            if let Some(entry) = self.logs.get_mut(index) {
                entry.text = text;
                entry.special = Some(special);
                entry.markdown_cache = None;
            }
            self.invalidate_transcript_metrics();
            return true;
        }
        self.push_special_log(LogKind::Tool, text, special);
        true
    }

    fn complete_generic_tool_log(&mut self, tool: &str, payload: &Value) {
        let special = build_completed_tool_log(tool, payload);
        let text = special.summary_text();
        if let Some(index) = self.logs.iter().rposition(|entry| {
            entry
                .special
                .as_ref()
                .is_some_and(|special| special.is_pending_tool_named(tool))
        }) {
            if let Some(entry) = self.logs.get_mut(index) {
                entry.text = text;
                entry.special = Some(special);
                entry.markdown_cache = None;
            }
            self.invalidate_transcript_metrics();
            return;
        }
        self.push_special_log(LogKind::Tool, text, special);
    }
}

fn log_base_style(kind: LogKind) -> Style {
    super::theme::log_style(kind)
}

// Keep plain transcript cells visually close to Codex: one leading bullet
// on the first line, then hanging indents for wrapped and explicit newline content.
fn render_plain_lines(kind: LogKind, text: &str, style: Style, width: u16) -> Vec<Line<'static>> {
    let prefix = log_prefix(kind);
    let prefix_width = UnicodeWidthStr::width(prefix).max(1);
    let leading_indent = Line::from(Span::styled(prefix.to_string(), style));
    let continuation_indent = Line::from(Span::styled(" ".repeat(prefix_width), style));
    let body_indent = Line::from(Span::styled(" ".repeat(prefix_width), style));
    let wrap_width = usize::from(width.max(1));
    let mut lines = Vec::new();

    for (segment_index, segment) in text.split('\n').enumerate() {
        let initial_indent = if segment_index == 0 {
            leading_indent.clone()
        } else {
            body_indent.clone()
        };
        let content = Line::from(vec![Span::styled(segment.to_string(), style)]);
        let wrapped = super::wrapping::word_wrap_line(
            &content,
            super::wrapping::RtOptions::new(wrap_width)
                .initial_indent(initial_indent)
                .subsequent_indent(continuation_indent.clone()),
        );

        if wrapped.is_empty() {
            lines.push(Line::from(Span::styled(prefix.to_string(), style)));
        } else {
            super::line_utils::push_owned_lines(wrapped.as_slice(), &mut lines);
        }
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(prefix.to_string(), style)));
    }
    lines
}
fn render_markdown_lines(kind: LogKind, text: &str, width: u16) -> Vec<Line<'static>> {
    let prefix = log_prefix(kind);
    let prefix_width = UnicodeWidthStr::width(prefix);
    let wrap_width = usize::from(width.max(1))
        .saturating_sub(prefix_width)
        .max(1);
    let style = log_base_style(kind);
    let rendered = super::markdown_render::render_markdown_text_with_width(text, Some(wrap_width));
    let mut lines = rendered.lines;
    if lines.is_empty() {
        return vec![Line::from(Span::styled(prefix.to_string(), style))];
    }
    lines[0]
        .spans
        .insert(0, Span::styled(prefix.to_string(), style));
    for line in &mut lines {
        line.style = style.patch(line.style);
    }
    lines
}

fn markdown_stream_wrap_width(kind: LogKind, width: u16) -> usize {
    let prefix = log_prefix(kind);
    let prefix_width = UnicodeWidthStr::width(prefix);
    usize::from(width.max(1))
        .saturating_sub(prefix_width)
        .max(1)
}

fn style_markdown_lines(
    kind: LogKind,
    mut lines: Vec<Line<'static>>,
    add_prefix: bool,
) -> Vec<Line<'static>> {
    if lines.is_empty() {
        return Vec::new();
    }
    let style = log_base_style(kind);
    if add_prefix {
        let prefix = log_prefix(kind);
        lines[0]
            .spans
            .insert(0, Span::styled(prefix.to_string(), style));
    }
    for line in &mut lines {
        line.style = style.patch(line.style);
    }
    lines
}

fn ensure_markdown_cache(entry: &mut LogEntry, width: u16) {
    let needs_refresh = entry
        .markdown_cache
        .as_ref()
        .is_none_or(|cache| cache.width != width);
    if !needs_refresh {
        return;
    }
    let lines = render_markdown_lines(entry.kind, entry.text.as_str(), width);
    entry.markdown_cache = Some(MarkdownCache { width, lines });
}

fn summarize_modal_text(text: &str, max_chars: usize) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let compact = normalized
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let compact = compact.split_whitespace().collect::<Vec<_>>().join(" ");
    let (mut output, truncated) = truncate_by_chars(compact.as_str(), max_chars);
    if truncated {
        output.push_str("...");
    }
    output
}

fn approval_prompt_text(request: &ApprovalRequest, is_zh: bool) -> String {
    match request.kind {
        ApprovalRequestKind::Patch => {
            if is_zh {
                "是否允许应用以下修改？".to_string()
            } else {
                "Would you like to make the following edits?".to_string()
            }
        }
        ApprovalRequestKind::Exec => {
            if is_zh {
                "是否允许执行这个命令？".to_string()
            } else {
                "Would you like to run this command?".to_string()
            }
        }
        ApprovalRequestKind::Control => {
            if is_apply_patch_tool_name(request.tool.as_str()) {
                if is_zh {
                    "是否允许应用以下修改？".to_string()
                } else {
                    "Would you like to make the following edits?".to_string()
                }
            } else if is_execute_command_tool_name(request.tool.as_str()) {
                if is_zh {
                    "是否允许执行这个命令？".to_string()
                } else {
                    "Would you like to run this command?".to_string()
                }
            } else if is_zh {
                "是否允许继续此次工具调用？".to_string()
            } else {
                "Would you like to continue with this tool call?".to_string()
            }
        }
    }
}

fn approval_option_labels(request: &ApprovalRequest, is_zh: bool) -> [String; 3] {
    match request.kind {
        ApprovalRequestKind::Patch => {
            if is_zh {
                [
                    "是，应用这些修改".to_string(),
                    "是，本会话内不再询问".to_string(),
                    "否，并告诉 Wunder 如何调整".to_string(),
                ]
            } else {
                [
                    "Yes, make the edits".to_string(),
                    "Yes, and don't ask again this session".to_string(),
                    "No, and tell Wunder what to do differently".to_string(),
                ]
            }
        }
        ApprovalRequestKind::Exec => {
            if is_zh {
                [
                    "是，仅执行这一次".to_string(),
                    "是，本会话内允许执行".to_string(),
                    "否，并告诉 Wunder 如何调整".to_string(),
                ]
            } else {
                [
                    "Yes, run it once".to_string(),
                    "Yes, allow it for this session".to_string(),
                    "No, and tell Wunder what to do differently".to_string(),
                ]
            }
        }
        ApprovalRequestKind::Control => {
            if is_execute_command_tool_name(request.tool.as_str()) {
                if is_zh {
                    [
                        "是，仅执行这一次".to_string(),
                        "是，本会话内允许执行".to_string(),
                        "否，并告诉 Wunder 如何调整".to_string(),
                    ]
                } else {
                    [
                        "Yes, run it once".to_string(),
                        "Yes, allow it for this session".to_string(),
                        "No, and tell Wunder what to do differently".to_string(),
                    ]
                }
            } else if is_zh {
                [
                    "是，继续".to_string(),
                    "是，本会话内持续允许".to_string(),
                    "否，并告诉 Wunder 如何调整".to_string(),
                ]
            } else {
                [
                    "Yes, continue".to_string(),
                    "Yes, allow it for this session".to_string(),
                    "No, and tell Wunder what to do differently".to_string(),
                ]
            }
        }
    }
}

fn next_large_paste_placeholder(counters: &mut HashMap<usize, usize>, char_count: usize) -> String {
    let base = format!("[Pasted Content {char_count} chars]");
    let next_suffix = counters.entry(char_count).or_insert(0);
    *next_suffix += 1;
    if *next_suffix == 1 {
        base
    } else {
        format!("{base} #{next_suffix}")
    }
}

fn expand_large_paste_placeholders(text: &str, pending: &[(String, String)]) -> String {
    if pending.is_empty() || text.is_empty() {
        return text.to_string();
    }

    let mut expanded = text.to_string();
    for (placeholder, actual) in pending {
        if expanded.contains(placeholder) {
            expanded = expanded.replacen(placeholder, actual, 1);
        }
    }
    expanded
}

// Keep one spacer row before every transcript entry after the first so the
// chat stream breathes more like the reference Codex TUI while preserving
// correct scroll math.
fn transcript_entry_spacing_before(index: usize) -> usize {
    usize::from(index > 0)
}

fn current_activity_elapsed_secs(started_at: Option<Instant>, settled_secs: f64) -> u64 {
    let live_secs = started_at
        .map(|started| settled_secs + started.elapsed().as_secs_f64())
        .unwrap_or(settled_secs);
    live_secs.max(0.0).round() as u64
}

fn format_busy_activity_line(
    is_zh: bool,
    elapsed_secs: u64,
    stream_catchup_mode: bool,
    terminal_focused: bool,
) -> String {
    let mut parts = vec![if is_zh {
        format!("• 运行中 ({elapsed_secs}s · Ctrl+C 可中断)")
    } else {
        format!("• Working ({elapsed_secs}s · ctrl+c to interrupt)")
    }];
    if stream_catchup_mode {
        parts.push(if is_zh {
            "追帧中".to_string()
        } else {
            "catch-up".to_string()
        });
    }
    if !terminal_focused {
        parts.push(if is_zh {
            "未聚焦".to_string()
        } else {
            "unfocused".to_string()
        });
    }
    parts.join(" · ")
}

fn format_footer_context_summary(
    is_zh: bool,
    used_tokens: i64,
    max_context: Option<u32>,
) -> String {
    if let Some(max_context) = max_context {
        let percent_left = crate::context_left_percent(used_tokens, Some(max_context)).unwrap_or(0);
        if is_zh {
            return format!("上下文余量 {percent_left}%");
        }
        return format!("{percent_left}% context left");
    }

    let used_tokens = used_tokens.max(0);
    if used_tokens > 0 {
        if is_zh {
            return format!("上下文占用 {used_tokens}");
        }
        return format!("{used_tokens} used");
    }

    if is_zh {
        "上下文余量 100%".to_string()
    } else {
        "100% context left".to_string()
    }
}

#[cfg(test)]
mod tests;
