//! Durable turn → flat row projection for the phone transcript.
//!
//! The engine owns this data on worker threads, so it must stay `Send`: rows
//! are plain strings, and the Slint types (`MdBlock`, `ModelRc`, `Image`) are
//! built only on the UI thread by [`RowData::to_row`]. 桌面版 computes the same
//! rows in `crates/wunder-desktop/frontend/src/timeline.rs`, bound to its own window; 安卓版 keeps
//! the same row kinds and field meanings while dropping the desktop
//! bookkeeping (group bars, process paging, subagent docks).

use crate::md::MarkdownRenderer;
use crate::{
    MdBlock, PatchCard, PatchLine, ProcessEntry, TimelineRow, TurnStatMetric, WorkflowLine,
    WorkflowSection,
};
use chrono::{Local, TimeZone};
use slint::{ModelRc, SharedString, VecModel};
use std::hash::{Hash, Hasher};
use wunder_desktop::native::NativeMessageStats;
use wunder_desktop::{
    NativeChatTurn, NativePatchFile, NativePatchLine, NativeWorkflowEntry, NativeWorkflowSection,
};

pub const KIND_USER: i32 = 0;
pub const KIND_BODY: i32 = 1;
pub const KIND_REASON: i32 = 2;
pub const KIND_TOOL: i32 = 3;
pub const KIND_PROCESS: i32 = 6;
pub const KIND_FOOTER: i32 = 7;
pub const KIND_ERROR: i32 = 8;

/// Payload sentinel for the expandable rows inside a work-process fold. The
/// child is packed as `(bar_row << 12) | child_index`; the fold callback
/// routes these back to the owning bar without keeping a side table.
pub const PROCESS_CHILD_BASE: i32 = 1 << 28;
pub const PROCESS_CHILD_SHIFT: u32 = 12;
const PROCESS_CHILD_MASK: i32 = (1 << PROCESS_CHILD_SHIFT) - 1;

pub const STATUS_DONE: i32 = 0;
pub const STATUS_RUNNING: i32 = 1;
pub const STATUS_FAILED: i32 = 2;
pub const STATUS_STOPPED: i32 = 3;

/// One row inside a work-process fold. 桌面版 keeps these as child rows of the
/// process bar; 安卓版 carries the same fields as plain data so the engine thread
/// stays `Send` and the UI thread builds the Slint model once per change.
#[derive(Clone, Debug, Default)]
pub struct ProcessChildData {
    pub id: String,
    pub kind: i32,
    pub text: String,
    pub blocks_src: String,
    pub summary: String,
    pub tool_name: String,
    pub tool_icon: String,
    pub target: String,
    pub detail: String,
    pub sections: Vec<NativeWorkflowSection>,
    pub patch: Vec<NativePatchFile>,
    pub tokens: String,
    pub duration: String,
    pub status_kind: i32,
    pub open: bool,
    pub payload: i32,
}

/// One transcript row without any Slint handle. `blocks_src` and
/// `reasoning_src` carry the Markdown source; the UI thread parses them once
/// per content change.
#[derive(Clone, Debug, Default)]
pub struct RowData {
    pub kind: i32,
    pub id: String,
    pub text: String,
    pub time: String,
    pub blocks_src: String,
    pub summary: String,
    pub tool_name: String,
    pub tool_icon: String,
    pub target: String,
    pub tool_metrics: String,
    pub detail: String,
    pub reasoning_src: String,
    pub status_kind: i32,
    pub foldable: bool,
    pub open: bool,
    pub payload: i32,
    pub user_round: i64,
    pub stats: Vec<(String, String)>,
    pub error_title: String,
    pub error_hint: String,
    /// Children of a `KIND_PROCESS` bar, in display order.
    pub process_children: Vec<ProcessChildData>,
}

/// Decode a fold callback address that points inside a work-process bar.
pub fn process_child_address(payload: i32) -> Option<(usize, usize)> {
    if payload < PROCESS_CHILD_BASE {
        return None;
    }
    let packed = payload - PROCESS_CHILD_BASE;
    let bar = (packed >> PROCESS_CHILD_SHIFT) as usize;
    let child = (packed & PROCESS_CHILD_MASK) as usize;
    Some((bar, child))
}

impl RowData {
    pub fn new(kind: i32, id: impl Into<String>) -> Self {
        Self {
            kind,
            id: id.into(),
            ..Default::default()
        }
    }

    /// Content signature used by the UI cache: a row is re-projected only when
    /// its visible content or fold state changed. Every displayed field has to
    /// participate: an error card whose text was omitted here kept the empty
    /// projection cached by the live row and rendered as a bare border.
    pub fn signature(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.kind.hash(&mut hasher);
        self.text.hash(&mut hasher);
        self.time.hash(&mut hasher);
        self.blocks_src.hash(&mut hasher);
        self.summary.hash(&mut hasher);
        self.tool_name.hash(&mut hasher);
        self.tool_icon.hash(&mut hasher);
        self.target.hash(&mut hasher);
        self.tool_metrics.hash(&mut hasher);
        self.detail.hash(&mut hasher);
        self.reasoning_src.hash(&mut hasher);
        self.status_kind.hash(&mut hasher);
        self.foldable.hash(&mut hasher);
        self.open.hash(&mut hasher);
        self.payload.hash(&mut hasher);
        self.user_round.hash(&mut hasher);
        self.stats.hash(&mut hasher);
        self.error_title.hash(&mut hasher);
        self.error_hint.hash(&mut hasher);
        for child in &self.process_children {
            child.id.hash(&mut hasher);
            child.kind.hash(&mut hasher);
            child.text.hash(&mut hasher);
            child.blocks_src.hash(&mut hasher);
            child.summary.hash(&mut hasher);
            child.tool_name.hash(&mut hasher);
            child.tool_icon.hash(&mut hasher);
            child.target.hash(&mut hasher);
            child.detail.hash(&mut hasher);
            for section in &child.sections {
                section.title.hash(&mut hasher);
                section.meta.hash(&mut hasher);
                section.kind.hash(&mut hasher);
                for line in &section.lines {
                    line.ratio.to_bits().hash(&mut hasher);
                    line.text.hash(&mut hasher);
                    line.kind.hash(&mut hasher);
                    line.number.hash(&mut hasher);
                }
            }
            for file in &child.patch {
                file.path.hash(&mut hasher);
                file.action.hash(&mut hasher);
                file.added.hash(&mut hasher);
                file.deleted.hash(&mut hasher);
                for line in &file.lines {
                    line.text.hash(&mut hasher);
                    line.kind.hash(&mut hasher);
                    line.number.hash(&mut hasher);
                }
            }
            child.tokens.hash(&mut hasher);
            child.duration.hash(&mut hasher);
            child.status_kind.hash(&mut hasher);
            child.open.hash(&mut hasher);
            child.payload.hash(&mut hasher);
        }
        hasher.finish()
    }

    pub fn to_row(&self, renderer: &mut MarkdownRenderer) -> TimelineRow {
        let mut row = blank(self.kind, self.id.as_str());
        row.text = self.text.as_str().into();
        row.time = self.time.as_str().into();
        row.summary = self.summary.as_str().into();
        row.tool_name = self.tool_name.as_str().into();
        row.tool_icon = self.tool_icon.as_str().into();
        row.target = self.target.as_str().into();
        row.tool_metrics = self.tool_metrics.as_str().into();
        row.tool_metrics_visible = !self.tool_metrics.is_empty();
        row.detail = self.detail.as_str().into();
        row.status_kind = self.status_kind;
        row.foldable = self.foldable;
        row.open = self.open;
        row.payload = self.payload;
        row.user_round = self.user_round.clamp(0, i32::MAX as i64) as i32;
        row.error_title = self.error_title.as_str().into();
        row.error_hint = self.error_hint.as_str().into();
        row.visible = self.kind != KIND_PROCESS || !self.process_children.is_empty();
        if !self.blocks_src.is_empty() {
            row.blocks = blocks_model(renderer, &self.blocks_src);
        }
        if !self.reasoning_src.is_empty() {
            row.reasoning_blocks = blocks_model(renderer, &self.reasoning_src);
        }
        if !self.process_children.is_empty() {
            let entries: Vec<ProcessEntry> = self
                .process_children
                .iter()
                .map(|child| {
                    // A failed or stopped call produced no change: its row keeps
                    // no change size beside the file chip.
                    let (diff_add, diff_del) =
                        if matches!(child.status_kind, STATUS_FAILED | STATUS_STOPPED) {
                            (String::new(), String::new())
                        } else {
                            patch_diff(&child.patch)
                        };
                    ProcessEntry {
                        kind: child.kind,
                        id: child.id.as_str().into(),
                        text: child.text.as_str().into(),
                        blocks: if child.blocks_src.is_empty() {
                            ModelRc::default()
                        } else {
                            blocks_model(renderer, &child.blocks_src)
                        },
                        summary: child.summary.as_str().into(),
                        tool_name: child.tool_name.as_str().into(),
                        tool_icon: child.tool_icon.as_str().into(),
                        target: child.target.as_str().into(),
                        detail: child.detail.as_str().into(),
                        sections: workflow_sections(&child.sections),
                        patch: patch_cards(&child.patch),
                        diff_add: diff_add.as_str().into(),
                        diff_del: diff_del.as_str().into(),
                        tokens: child.tokens.as_str().into(),
                        duration: child.duration.as_str().into(),
                        status_kind: child.status_kind,
                        open: child.open,
                        payload: child.payload,
                    }
                })
                .collect();
            row.process_entries = ModelRc::new(VecModel::from(entries));
        }
        if !self.stats.is_empty() {
            row.stats = ModelRc::new(VecModel::from(
                self.stats
                    .iter()
                    .map(|(key, value)| TurnStatMetric {
                        key: key.as_str().into(),
                        value: value.as_str().into(),
                    })
                    .collect::<Vec<_>>(),
            ));
        }
        row
    }
}

fn blank(kind: i32, id: impl Into<SharedString>) -> TimelineRow {
    TimelineRow {
        kind,
        id: id.into(),
        user_round: 0,
        visible: true,
        text: SharedString::default(),
        time: SharedString::default(),
        blocks: ModelRc::default(),
        summary: SharedString::default(),
        tool_name: SharedString::default(),
        tool_icon: SharedString::default(),
        target: SharedString::default(),
        tool_metrics: SharedString::default(),
        tool_metrics_visible: false,
        tokens: SharedString::default(),
        duration: SharedString::default(),
        detail: SharedString::default(),
        reasoning_blocks: ModelRc::default(),
        sections: ModelRc::default(),
        process_entries: ModelRc::default(),
        process_root: SharedString::default(),
        process_paged: false,
        process_local: false,
        process_older: false,
        process_newer: false,
        process_first: -1,
        process_last: -1,
        process_loading: false,
        process_error: false,
        process_revision: 0,
        status_kind: STATUS_DONE,
        foldable: false,
        open: false,
        group_idx: -1,
        group_open: false,
        process_idx: -1,
        process_open: false,
        payload: 0,
        patch: ModelRc::default(),
        stats: ModelRc::default(),
        error_title: SharedString::default(),
        error_hint: SharedString::default(),
    }
}

pub fn status_kind(state: &str) -> i32 {
    let state = state.trim();
    match state {
        "failed" | "error" | "rejected" | "执行失败" | "任务失败" => STATUS_FAILED,
        "cancelled" | "canceled" | "interrupted" | "stopped" | "已停止" | "已取消" | "已中断" => {
            STATUS_STOPPED
        }
        "" | "idle" | "ready" | "none" | "completed" | "complete" | "done" | "ok" | "success"
        | "任务完成" | "已完成" => STATUS_DONE,
        _ if state.starts_with("失败") => STATUS_FAILED,
        _ if state.starts_with("已停止")
            || state.starts_with("已取消")
            || state.starts_with("已中断") =>
        {
            STATUS_STOPPED
        }
        // Every other phrase is a live phase: "running", "streaming",
        // "正在生成…", "正在排队", "等待用户输入". Treating those as done
        // collapsed the durable work-process bar and hid its status chip.
        _ => STATUS_RUNNING,
    }
}

/// Collapse whitespace runs and cut at `limit` characters. A cut preview ends
/// with an ellipsis so it never reads as the whole message.
pub fn preview_text(text: &str, limit: usize) -> String {
    let mut out = String::with_capacity(limit + 4);
    let mut count = 0usize;
    let mut pending_space = false;
    let mut truncated = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            if count >= limit {
                truncated = true;
                break;
            }
            out.push(' ');
            count += 1;
            pending_space = false;
        }
        if count >= limit {
            truncated = true;
            break;
        }
        out.push(ch);
        count += 1;
    }
    if truncated {
        out.push('…');
    }
    out
}

/// One right-edge locator tick: user turn preview + its newest answer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TurnMarkData {
    pub fraction: f32,
    pub user: String,
    pub agent: String,
}

const MARK_USER_PREVIEW: usize = 80;
const MARK_AGENT_PREVIEW: usize = 120;

/// Desktop turn-ruler projection for the phone: one tick per user turn, up to
/// the newest 100 turns, at the row-index fraction of the transcript. The
/// preview mirrors the desktop card (user text + newest answer), but with
/// shorter cuts because the phone card is narrower.
pub fn turn_marks(rows: &[&RowData]) -> Vec<TurnMarkData> {
    let total = rows.len();
    if total <= 1 {
        return Vec::new();
    }
    let users: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.kind == KIND_USER)
        .map(|(index, _)| index)
        .collect();
    let first = users.len().saturating_sub(100);
    let mut marks = Vec::with_capacity(users.len().saturating_sub(first));
    for (position, &index) in users.iter().enumerate().skip(first) {
        let end = users.get(position + 1).copied().unwrap_or(total);
        let user = preview_text(&rows[index].text, MARK_USER_PREVIEW);
        let agent = ((index + 1)..end)
            .rev()
            .find(|row_index| {
                rows[*row_index].kind == KIND_BODY && !rows[*row_index].text.is_empty()
            })
            .map(|row_index| preview_text(&rows[row_index].text, MARK_AGENT_PREVIEW))
            .unwrap_or_default();
        marks.push(TurnMarkData {
            fraction: index as f32 / total as f32,
            user,
            agent,
        });
    }
    marks
}

/// Companion bubble text: drop image/link URLs and inline markdown
/// punctuation, then collapse whitespace and cap at 96 characters (the same
/// bubble contract as the desktop pet and the web floating layer).
pub fn bubble_hint(raw: &str) -> String {
    let without_images = strip_link_pairs(raw, true);
    let without_links = strip_link_pairs(&without_images, false);
    let mut text = String::with_capacity(without_links.len());
    for ch in without_links.chars() {
        match ch {
            '`' | '#' | '>' | '*' | '_' | '~' | '-' => text.push(' '),
            other => text.push(other),
        }
    }
    preview_text(&text, 96)
}

/// Remove `![alt](url)` / `[label](url)` constructs. An image carries no text
/// worth reading, so its label goes with it; a link label stays.
fn strip_link_pairs(input: &str, image: bool) -> String {
    let opener = if image { "![" } else { "[" };
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find(opener) {
        out.push_str(&rest[..start]);
        let after = &rest[start + opener.len()..];
        let Some(end) = after.find(']') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let label = &after[..end];
        let tail = &after[end + 1..];
        let tail = match tail.strip_prefix('(') {
            Some(inner) => match inner.find(')') {
                Some(close) => &inner[close + 1..],
                None => "",
            },
            None => tail,
        };
        if !image {
            out.push_str(label);
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

fn clock_time(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return String::new();
    }
    let millis = (seconds * 1000.0) as i64;
    match Local.timestamp_millis_opt(millis).single() {
        Some(value) => value.format("%H:%M").to_string(),
        None => String::new(),
    }
}

pub fn session_time(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return String::new();
    }
    let millis = (seconds * 1000.0) as i64;
    match Local.timestamp_millis_opt(millis).single() {
        Some(value) => value.format("%m-%d %H:%M").to_string(),
        None => String::new(),
    }
}

fn blocks_model(renderer: &mut MarkdownRenderer, text: &str) -> ModelRc<MdBlock> {
    let blocks = renderer.render(text);
    if blocks.is_empty() {
        ModelRc::default()
    } else {
        ModelRc::new(VecModel::from(blocks))
    }
}

/// Same bounded projection as 桌面版: 12 sections, 201 lines each. The engine has
/// already clipped the payload; the caps keep a malformed frame from rebuilding
/// an unbounded Slint model on the UI thread.
fn workflow_sections(items: &[NativeWorkflowSection]) -> ModelRc<WorkflowSection> {
    if items.is_empty() {
        return ModelRc::default();
    }
    ModelRc::new(VecModel::from(
        items
            .iter()
            .take(12)
            .map(|section| WorkflowSection {
                title: section.title.as_str().into(),
                meta: section.meta.as_str().into(),
                kind: section.kind.as_str().into(),
                lines: ModelRc::new(VecModel::from(
                    section
                        .lines
                        .iter()
                        .take(201)
                        .map(|line| WorkflowLine {
                            ratio: line.ratio,
                            text: line.text.as_str().into(),
                            kind: line.kind.as_str().into(),
                            number: line.number.as_str().into(),
                        })
                        .collect::<Vec<_>>(),
                )),
            })
            .collect::<Vec<_>>(),
    ))
}

/// File diffs stay flat and pre-bounded, matching 桌面版's patch card budget.
fn patch_cards(files: &[NativePatchFile]) -> ModelRc<PatchCard> {
    if files.is_empty() {
        return ModelRc::default();
    }
    ModelRc::new(VecModel::from(
        files
            .iter()
            .take(8)
            .map(|file| PatchCard {
                path: file.path.as_str().into(),
                action: action_label(&file.action).into(),
                added: file.added.as_str().into(),
                deleted: file.deleted.as_str().into(),
                content_width: patch_content_width(&file.lines),
                lines: ModelRc::new(VecModel::from(
                    file.lines
                        .iter()
                        .take(200)
                        .map(|line| PatchLine {
                            number: line.number.as_str().into(),
                            text: line.text.as_str().into(),
                            kind: line.kind.as_str().into(),
                        })
                        .collect::<Vec<_>>(),
                )),
            })
            .collect::<Vec<_>>(),
    ))
}

/// Collapsed-row change size of an edit / patch call: the per-file counts the
/// patch cards already carry, summed. Empty means "no change on that side", so
/// the row never prints a meaningless "+0 −0".
pub fn patch_diff(files: &[NativePatchFile]) -> (String, String) {
    let (mut added, mut deleted) = (0u64, 0u64);
    for file in files.iter().take(8) {
        added = added.saturating_add(file.added.trim().parse::<u64>().unwrap_or(0));
        deleted = deleted.saturating_add(file.deleted.trim().parse::<u64>().unwrap_or(0));
    }
    (
        if added > 0 {
            added.to_string()
        } else {
            String::new()
        },
        if deleted > 0 {
            deleted.to_string()
        } else {
            String::new()
        },
    )
}

/// Natural width of one diff card's line area. Touch shells pan this area
/// horizontally, so it covers the longest line instead of letting the row
/// elide it on the right.
fn patch_content_width(lines: &[NativePatchLine]) -> f32 {
    const GUTTER: f32 = 100.0;
    let longest = lines
        .iter()
        .take(200)
        .map(|line| {
            line.text
                .chars()
                .map(|ch| {
                    if ch == '\t' {
                        60.0
                    } else if is_wide_char(ch) {
                        12.5
                    } else {
                        7.5
                    }
                })
                .sum::<f32>()
        })
        .fold(0.0f32, f32::max);
    (GUTTER + longest).clamp(260.0, 3000.0)
}

/// CJK and emoji take roughly a full monospace cell; the rest about 0.6.
fn is_wide_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1100..=0x115F
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE6F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1FAFF
    )
}

fn action_label(action: &str) -> &'static str {
    match action {
        "add" => "新增",
        "delete" => "删除",
        "move" => "移动",
        _ => "修改",
    }
}

/// Align a raw tool id with 桌面版's Chinese row labels.
pub(crate) fn tool_label(tool: &str) -> String {
    let mapped = match tool.trim() {
        "read_file" | "read files" => "读取文件",
        "write_file" => "写入文件",
        "edit_file" | "edit_file2" | "text_editor" => "编辑文件",
        "apply_patch" => "应用补丁",
        "list_files" | "list_file" => "列出文件",
        "glob" | "glob_files" | "文件名匹配" => "文件名匹配",
        "search_content" | "search_files" => "搜索内容",
        "web_search" | "web_search_tool" | "web search" | "websearch" | "网页搜索" => {
            "网页搜索"
        }
        "web_fetch" | "web_fetch_tool" | "web fetch" | "webfetch" | "网页抓取" => "网页抓取",
        "execute_command" | "run_command" | "shell" | "运行命令" => "运行命令",
        "command_session" => "命令会话",
        "question_panel" | "ask_panel" => "问询面板",
        "update_plan" | "plan_update" => "计划面板",
        "read_image" | "view_image" => "读图",
        "skill_call" | "skill_get" => "技能调用",
        "schedule_task" => "定时任务",
        "context_compaction" | "compact" => "上下文压缩",
        "" => "工具",
        other => other,
    };
    mapped.chars().take(64).collect()
}

/// Icon key resolved by the shared `ToolIcons` global. This mirrors the common
/// rules of 桌面版's `tool_icons::workflow_icon`; unknown tools fall back to the
/// toolbox glyph already used by the desktop.
pub(crate) fn tool_icon_key(tool: &str) -> &'static str {
    let text = tool.trim().to_lowercase();
    if text.contains("compact") || tool.contains("压缩") {
        return "compress";
    }
    if text.contains("browser") || tool.contains("浏览器") {
        return "window-maximize";
    }
    if text.contains("question_panel") || text.contains("ask_panel") || tool.contains("问询") {
        return "circle-question";
    }
    if text.contains("update_plan") || text.contains("plan_update") || tool.contains("计划") {
        return "table-columns";
    }
    if text.contains("skill_") || tool.contains("技能") {
        return "book-open";
    }
    if text.contains("memory") || tool.contains("记忆") {
        return "memory";
    }
    if text.contains("subagent") || text.contains("蜂群") || tool.contains("子智能体") {
        return "diagram-project";
    }
    if text.contains("web_search")
        || text.contains("search_content")
        || text.contains("search_files")
        || tool.contains("搜索")
    {
        return "magnifying-glass";
    }
    if text.contains("web_fetch") || text.contains("fetch") || tool.contains("抓取") {
        return "globe";
    }
    if text.contains("glob") || tool.contains("文件名匹配") {
        return "folder-tree";
    }
    if text.contains("list_file")
        || text.contains("list_files")
        || text.contains("read_file")
        || text.contains("read files")
        || tool.contains("读取")
        || tool.contains("列出")
    {
        return "file-lines";
    }
    if text.contains("write_file")
        || text.contains("edit_file")
        || text.contains("apply_patch")
        || text.contains("text_editor")
        || tool.contains("写入")
        || tool.contains("编辑")
        || tool.contains("补丁")
    {
        return "file-pen";
    }
    if text.contains("shell") || text.contains("command") || tool.contains("命令") {
        return "terminal";
    }
    if text.contains("read_image") || text.contains("view_image") || tool.contains("读图") {
        return "eye";
    }
    "toolbox"
}

/// Resource target for the chip in the header; only file-like tools expose the
/// preview as a path (a truncated preview must not become a broken link).
pub(crate) fn tool_target(entry: &NativeWorkflowEntry) -> String {
    if matches!(
        entry.title.as_str(),
        "read_file"
            | "读取文件"
            | "write_file"
            | "写入文件"
            | "list_files"
            | "列出文件"
            | "list_file"
            | "read_image"
            | "view_image"
            | "读图"
    ) && entry.preview.chars().count() < 640
    {
        return entry.preview.clone();
    }
    String::new()
}

fn tool_child(turn_id: &str, entry: &NativeWorkflowEntry) -> ProcessChildData {
    ProcessChildData {
        id: format!("{turn_id}-tool-{}", entry.id),
        kind: KIND_TOOL,
        summary: entry.preview.clone(),
        tool_name: tool_label(&entry.title),
        tool_icon: tool_icon_key(&entry.title).into(),
        target: tool_target(entry),
        detail: entry.detail.clone(),
        sections: entry.sections.clone(),
        patch: entry.patch_files.clone(),
        tokens: entry.tokens.clone(),
        duration: entry.duration.clone(),
        status_kind: status_kind(&entry.state),
        open: false,
        ..Default::default()
    }
}

fn reason_child(turn_id: &str, item_id: &str, text: &str) -> ProcessChildData {
    ProcessChildData {
        id: format!("{turn_id}-reason-{item_id}"),
        kind: KIND_REASON,
        summary: preview_text(text, 120),
        tool_name: "思考".into(),
        detail: text.to_string(),
        open: false,
        ..Default::default()
    }
}

fn body_child(turn_id: &str, item_id: &str, text: &str) -> ProcessChildData {
    ProcessChildData {
        id: format!("{turn_id}-body-{item_id}"),
        kind: KIND_BODY,
        text: text.to_string(),
        blocks_src: text.to_string(),
        summary: preview_text(text, 120),
        tool_name: "模型输出".into(),
        detail: text.to_string(),
        open: false,
        ..Default::default()
    }
}

/// Footer values mirror 桌面版's `turn_stats`: the icon key selects the glyph, the
/// value is already formatted, and the order is the same on every shell.
fn footer_stats(stats: &NativeMessageStats, round: i64) -> Vec<(String, String)> {
    let mut metrics: Vec<(String, String)> = Vec::new();
    if round > 0 {
        metrics.push(("hashtag".into(), round.to_string()));
    }
    if !stats.is_empty() {
        metrics.push((
            "stopwatch".into(),
            stats
                .duration_s
                .map(duration_text)
                .unwrap_or_else(|| "-".into()),
        ));
        metrics.push((
            "gauge-high".into(),
            stats
                .speed_tps
                .map(|value| format!("{value:.1} token/s"))
                .unwrap_or_else(|| "-".into()),
        ));
        metrics.push((
            "layer-group".into(),
            stats
                .context_tokens
                .map(count_text)
                .unwrap_or_else(|| "--".into()),
        ));
        metrics.push((
            "bolt".into(),
            stats
                .consumed_tokens
                .map(count_text)
                .unwrap_or_else(|| "--".into()),
        ));
        metrics.push((
            "screwdriver-wrench".into(),
            stats
                .tool_calls
                .map(count_text)
                .unwrap_or_else(|| "--".into()),
        ));
        metrics.push((
            "coins".into(),
            stats.credits.map(count_text).unwrap_or_else(|| "--".into()),
        ));
    }
    metrics
}

pub(crate) fn status_metric(state: &str) -> Option<(&'static str, String)> {
    let state = state.trim();
    match state {
        "" | "completed" | "complete" | "done" | "ok" | "success" | "任务完成" | "已完成" => {
            None
        }
        "failed" | "error" | "rejected" | "执行失败" | "任务失败" => {
            Some(("status-error", "任务失败".into()))
        }
        "cancelled" | "canceled" | "stopped" | "interrupted" | "已停止" | "已取消" | "已中断" => {
            Some(("status-stopped", "已停止".into()))
        }
        "waiting_input" | "waiting_user_input" | "waiting_approval" | "等待用户输入"
        | "等待确认" => Some(("status-waiting", "等待确认".into())),
        "queued" | "pending" | "正在排队" | "排队中" => {
            Some(("status-queued", "排队中".into()))
        }
        "running" | "in_progress" | "正在处理…" | "正在处理" => {
            Some(("status-running", "处理中".into()))
        }
        "正在生成…" | "正在生成" | "正在思考…" | "正在思考" | "模型输出中" | "streaming" => {
            Some(("status-outputting", "模型输出中".into()))
        }
        "正在执行工具…" | "正在执行工具" | "使用工具中" => {
            Some(("status-tool", "使用工具中".into()))
        }
        "上下文压缩中…" | "上下文压缩中" | "compacting" => {
            Some(("status-compacting", "上下文压缩中".into()))
        }
        "目标继续执行…" | "目标继续执行" => {
            Some(("status-requesting", "请求中".into()))
        }
        other if other.starts_with("等待") => Some(("status-waiting", "等待确认".into())),
        other if other.starts_with("失败") => Some(("status-error", "任务失败".into())),
        other
            if other.starts_with("已停止")
                || other.starts_with("已取消")
                || other.starts_with("已中断") =>
        {
            Some(("status-stopped", "已停止".into()))
        }
        // The durable catalogue labels a running turn "正在生成…", but it may
        // also arrive as a queue phrase with detail such as "正在排队 · 前方 2 名".
        other if other.starts_with("正在排队") || other.starts_with("任务已排队") => {
            Some(("status-queued", other.to_owned()))
        }
        other if other.starts_with("正在生成") || other.starts_with("正在思考") => {
            Some(("status-outputting", "模型输出中".into()))
        }
        other if other.starts_with("正在执行工具") => {
            Some(("status-tool", "使用工具中".into()))
        }
        other if other.starts_with("正在") => Some(("status-running", other.to_owned())),
        _ => None,
    }
}

fn footer_row(turn: &NativeChatTurn) -> Option<RowData> {
    let mut metrics = footer_stats(&turn.assistant.stats, turn.user.user_round);
    if let Some((key, value)) = status_metric(&turn.assistant.state) {
        metrics.insert(0, (key.into(), value));
    }
    if metrics.is_empty() {
        return None;
    }
    let mut row = RowData::new(KIND_FOOTER, format!("{}-footer", turn.root_id));
    row.text = turn.assistant.text.clone();
    row.status_kind = status_kind(&turn.assistant.state);
    row.user_round = turn.user.user_round;
    row.stats = metrics;
    Some(row)
}

fn duration_text(seconds: f64) -> String {
    let minutes = (seconds / 60.0).floor() as u64;
    let hours = minutes / 60;
    if hours > 0 {
        format!("{hours}h {}m {:.1}s", minutes % 60, seconds % 60.0)
    } else if minutes > 0 {
        format!("{minutes}m {:.1}s", seconds % 60.0)
    } else {
        format!("{seconds:.1}s")
    }
}

fn count_text(value: f64) -> String {
    let value = value.trunc();
    if value < 1000.0 {
        return format!("{value:.0}");
    }
    let (scaled, unit) = if value >= 1_000_000.0 {
        (value / 1_000_000.0, "m")
    } else {
        (value / 1000.0, "k")
    };
    let text = if scaled >= 100.0 {
        format!("{scaled:.0}")
    } else {
        format!("{scaled:.1}")
    };
    format!("{}{unit}", text.strip_suffix(".0").unwrap_or(&text))
}

/// Project one durable user turn into the rows the transcript renders.
///
/// The turn's thinking, tools and earlier printed rounds belong to one
/// collapsible work-process bar; only the newest non-empty round text stays
/// outside as the answer, matching 桌面版's fold ownership rule.
pub fn project_turn(turn: &NativeChatTurn) -> Vec<RowData> {
    let mut rows = Vec::new();

    let mut user = RowData::new(KIND_USER, format!("{}-user", turn.root_id));
    user.text = turn.user.text.clone();
    user.time = clock_time(turn.user.created_at);
    rows.push(user);

    let final_round = turn
        .rounds
        .iter()
        .rposition(|round| !round.text.trim().is_empty());
    let mut children: Vec<ProcessChildData> = Vec::new();
    for (index, round) in turn.rounds.iter().enumerate() {
        if !round.reasoning.trim().is_empty() {
            // Round ordinals restart across continuation executions, and the
            // durable item id is the only stable identity.
            let item_id = if round.item_id.is_empty() {
                format!("{}-{}", round.execution, round.round)
            } else {
                round.item_id.clone()
            };
            children.push(reason_child(&turn.root_id, &item_id, &round.reasoning));
        }
        for entry in &round.items {
            children.push(tool_child(&turn.root_id, entry));
        }
        if Some(index) != final_round && !round.text.trim().is_empty() {
            let item_id = if round.item_id.is_empty() {
                format!("{}-{}", round.execution, round.round)
            } else {
                round.item_id.clone()
            };
            children.push(body_child(&turn.root_id, &item_id, &round.text));
        }
    }

    if !children.is_empty() {
        let mut bar = RowData::new(KIND_PROCESS, format!("{}-process", turn.root_id));
        bar.text = format!("工作过程 · {} 步", children.len());
        bar.summary = if turn.failure.is_some() {
            "已失败".into()
        } else if status_kind(&turn.assistant.state) == STATUS_RUNNING {
            "正在处理…".into()
        } else {
            "已完成".into()
        };
        bar.status_kind = status_kind(&turn.assistant.state);
        bar.foldable = true;
        bar.open = bar.status_kind == STATUS_RUNNING;
        bar.process_children = children;
        rows.push(bar);
    }

    // Older threads may not mark an answer round; fall back to the assistant
    // message so the reply is never dropped.
    let final_text = final_round
        .map(|index| turn.rounds[index].text.clone())
        .filter(|text| !text.trim().is_empty())
        .or_else(|| (!turn.assistant.text.trim().is_empty()).then(|| turn.assistant.text.clone()));
    if let Some(text) = final_text {
        let mut row = RowData::new(KIND_BODY, format!("{}-body-final", turn.root_id));
        row.text = text.clone();
        row.blocks_src = text;
        rows.push(row);
    }

    if let Some(failure) = &turn.failure {
        let mut row = RowData::new(KIND_ERROR, format!("{}-error", turn.root_id));
        row.error_title = "执行失败".into();
        row.text = failure.message.clone();
        row.status_kind = STATUS_FAILED;
        rows.push(row);
    }

    if let Some(footer) = footer_row(turn) {
        rows.push(footer);
    }
    rows
}

/// One streamed step of the live turn, in arrival order. A reasoning segment
/// ends as soon as the model acts (a tool call, the answer, or a new round),
/// so the live process mirrors the durable per-round thinking entries instead
/// of folding every thought of the turn into the first one.
pub enum LiveStep {
    Reasoning(LiveReason),
    Tool(LiveTool),
}

pub struct LiveReason {
    pub id: String,
    pub text: String,
    pub status: i32,
    round: i64,
}

pub struct LiveTool {
    pub id: String,
    pub title: String,
    pub tool_icon: String,
    pub summary: String,
    pub state: String,
    pub detail: String,
    pub target: String,
    pub sections: Vec<NativeWorkflowSection>,
    pub patch: Vec<NativePatchFile>,
    pub tokens: String,
    pub duration: String,
}

/// Rows for the turn currently streaming. The durable projection replaces
/// them as soon as the runtime commits the turn.
pub struct LiveTurn {
    pub root_id: String,
    pub user: String,
    pub answer: String,
    /// Thinking and tool entries in the order the model produced them.
    pub steps: Vec<LiveStep>,
    pub status: i32,
    pub phase: String,
    pub failure: Option<String>,
    pub user_round: i64,
    pub stats: NativeMessageStats,
    pub question_panel: Option<serde_json::Value>,
    /// Bounded output tail per streamed command session, keyed by session#index.
    pub command_output: std::collections::HashMap<String, String>,
    /// Model round the events currently arriving belong to. A new round (or a
    /// continuation execution) opens a fresh reasoning entry.
    pub round: i64,
    /// Execution id of the current run (`native_execution_started`). Late
    /// frames from an earlier execution must not decorate this turn.
    pub execution: String,
    active_reason: Option<usize>,
    reason_serial: u32,
}

impl LiveTurn {
    pub fn new(user: String) -> Self {
        Self {
            root_id: "live".into(),
            user,
            answer: String::new(),
            steps: Vec::new(),
            status: STATUS_RUNNING,
            phase: "requesting".into(),
            failure: None,
            user_round: 0,
            stats: NativeMessageStats::default(),
            question_panel: None,
            command_output: std::collections::HashMap::new(),
            round: 0,
            execution: String::new(),
            active_reason: None,
            reason_serial: 0,
        }
    }

    pub fn tool_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| matches!(step, LiveStep::Tool(_)))
            .count()
    }

    pub fn tool_mut(&mut self, id: &str) -> Option<&mut LiveTool> {
        self.steps.iter_mut().find_map(|step| match step {
            LiveStep::Tool(tool) if tool.id == id => Some(tool),
            _ => None,
        })
    }

    pub fn push_tool(&mut self, tool: LiveTool) {
        self.steps.push(LiveStep::Tool(tool));
    }

    /// Finish the active reasoning entry. Called when the model acts, a new
    /// round begins, or the turn ends; the entry keeps its text and stops
    /// spinning.
    pub fn close_reason(&mut self, status: i32) {
        if let Some(index) = self.active_reason.take() {
            if let Some(LiveStep::Reasoning(reason)) = self.steps.get_mut(index) {
                reason.status = status;
            }
        }
    }

    /// Append one reasoning delta. `replace` is used by the full-text
    /// `reasoning` frame, `append` by stream deltas.
    pub fn append_reasoning(&mut self, round: i64, text: &str, replace: bool) {
        if self.active_reason.is_some() && self.reason_round() == round {
            if let Some(index) = self.active_reason {
                if let Some(LiveStep::Reasoning(reason)) = self.steps.get_mut(index) {
                    if replace {
                        reason.text = text.to_owned();
                    } else {
                        reason.text.push_str(text);
                    }
                    self.phase = "thinking".into();
                    return;
                }
            }
        }
        self.close_reason(STATUS_DONE);
        let id = format!("{}-reason-{}", self.root_id, self.reason_serial);
        self.reason_serial += 1;
        self.steps.push(LiveStep::Reasoning(LiveReason {
            id,
            text: text.to_owned(),
            status: if self.status == STATUS_RUNNING {
                STATUS_RUNNING
            } else {
                STATUS_DONE
            },
            round,
        }));
        self.active_reason = Some(self.steps.len() - 1);
        self.phase = "thinking".into();
    }

    fn reason_round(&self) -> i64 {
        self.active_reason
            .and_then(|index| match self.steps.get(index) {
                Some(LiveStep::Reasoning(reason)) => Some(reason.round),
                _ => None,
            })
            .unwrap_or(-1)
    }

    fn process_summary(&self) -> &'static str {
        match self.phase.as_str() {
            "requesting" => "请求中…",
            "thinking" | "outputting" => "正在生成回复…",
            "tool" => "正在执行工具…",
            "compacting" => "上下文压缩中…",
            "queued" => "正在排队…",
            "waiting" => "等待确认…",
            _ => "正在处理…",
        }
    }

    pub fn rows(&self) -> Vec<RowData> {
        let mut rows = Vec::new();
        if !self.user.trim().is_empty() {
            let mut row = RowData::new(KIND_USER, format!("{}-user", self.root_id));
            row.text = self.user.clone();
            row.time = Local::now().format("%H:%M").to_string();
            rows.push(row);
        }

        let mut children: Vec<ProcessChildData> = Vec::new();
        for (index, step) in self.steps.iter().enumerate() {
            match step {
                LiveStep::Reasoning(reason) => {
                    let mut child = ProcessChildData {
                        id: reason.id.clone(),
                        kind: KIND_REASON,
                        tool_name: "思考".into(),
                        detail: reason.text.clone(),
                        status_kind: if reason.status == STATUS_RUNNING
                            && self.status != STATUS_RUNNING
                        {
                            self.status
                        } else {
                            reason.status
                        },
                        open: false,
                        payload: -2 - index as i32,
                        ..Default::default()
                    };
                    // The fold header shows one outcome line; while streaming
                    // it is the tail of the thought, matching the desktop row.
                    child.summary = reason
                        .text
                        .lines()
                        .last()
                        .map(|line| preview_text(line, 120))
                        .unwrap_or_default();
                    children.push(child);
                }
                LiveStep::Tool(tool) => children.push(ProcessChildData {
                    id: format!("{}-tool-{}", self.root_id, tool.id),
                    kind: KIND_TOOL,
                    tool_name: tool.title.clone(),
                    tool_icon: tool.tool_icon.clone(),
                    target: tool.target.clone(),
                    summary: if tool.summary.is_empty() {
                        preview_text(&tool.detail, 160)
                    } else {
                        tool.summary.clone()
                    },
                    detail: tool.detail.clone(),
                    sections: tool.sections.clone(),
                    patch: tool.patch.clone(),
                    tokens: tool.tokens.clone(),
                    duration: tool.duration.clone(),
                    status_kind: status_kind(&tool.state),
                    open: false,
                    payload: -2 - index as i32,
                    ..Default::default()
                }),
            }
        }

        if !children.is_empty() {
            let mut bar = RowData::new(KIND_PROCESS, format!("{}-process", self.root_id));
            bar.text = format!("工作过程 · {} 步", children.len());
            bar.summary = self.process_summary().into();
            bar.status_kind = self.status;
            bar.foldable = true;
            // The live turn is unfolded from its first step; a tap writes the
            // state back into `live_rows`, and the next publish keeps it.
            bar.open = true;
            bar.process_children = children;
            rows.push(bar);
        }

        if !self.answer.is_empty() {
            let mut row = RowData::new(KIND_BODY, format!("{}-body", self.root_id));
            row.text = self.answer.clone();
            // Streamed text stays plain while tokens arrive; the whole answer
            // is parsed once when the turn settles. Re-parsing it on every
            // delta stalled the phone UI and delayed every status update.
            if self.status != STATUS_RUNNING {
                row.blocks_src = self.answer.clone();
            }
            row.status_kind = self.status;
            rows.push(row);
        }
        if let Some(message) = &self.failure {
            let mut row = RowData::new(KIND_ERROR, format!("{}-error", self.root_id));
            row.error_title = "执行失败".into();
            row.text = message.clone();
            row.status_kind = STATUS_FAILED;
            rows.push(row);
        }

        let mut stats = footer_stats(&self.stats, self.user_round);
        if let Some((key, value)) = live_status_metric(&self.phase) {
            stats.insert(0, (key.into(), value));
        }
        if !stats.is_empty() {
            let mut row = RowData::new(KIND_FOOTER, format!("{}-footer", self.root_id));
            row.text = self.answer.clone();
            row.status_kind = self.status;
            row.user_round = self.user_round;
            row.stats = stats;
            rows.push(row);
        }
        for row in &mut rows {
            // A live row's index is not stable while the durable prefix grows;
            // its payload only says "live" to the fold callback.
            row.payload = -1;
        }
        rows
    }
}

/// Root id of the newest durable turn whose user row matches `text` and whose
/// rows still carry a running marker. Used while a stream's identity frame is
/// still in flight, so an older identical message cannot capture the tail.
pub(crate) fn running_root_for_text(rows: &[RowData], text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    for (index, row) in rows.iter().enumerate().rev() {
        if row.kind != KIND_USER || row.text.trim() != text {
            continue;
        }
        let Some(root) = row.id.strip_suffix("-user") else {
            continue;
        };
        let mut running = false;
        for next in &rows[index + 1..] {
            if next.kind == KIND_USER {
                break;
            }
            if next.kind == KIND_FOOTER || next.kind == KIND_PROCESS {
                running = next.status_kind == STATUS_RUNNING;
                if running {
                    break;
                }
            }
        }
        if running {
            return Some(root.to_owned());
        }
    }
    None
}

pub(crate) fn live_status_metric(phase: &str) -> Option<(&'static str, String)> {
    match phase {
        "requesting" => Some(("status-requesting", "请求中".into())),
        "thinking" | "outputting" => Some(("status-outputting", "模型输出中".into())),
        "tool" => Some(("status-tool", "使用工具中".into())),
        "compacting" => Some(("status-compacting", "上下文压缩中".into())),
        "queued" => Some(("status-queued", "排队中".into())),
        "waiting" => Some(("status-waiting", "等待确认".into())),
        "stopped" => Some(("status-stopped", "已停止".into())),
        "error" => Some(("status-error", "任务失败".into())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::Model;
    use wunder_desktop::{NativePatchFile, NativePatchLine};

    fn entry() -> NativeWorkflowEntry {
        NativeWorkflowEntry {
            id: "call-1".into(),
            title: "read_file".into(),
            preview: "src/main.rs".into(),
            detail: "fn main() {}".into(),
            state: "completed".into(),
            tokens: "1.2k token".into(),
            duration: "0.3s".into(),
            sections: vec![NativeWorkflowSection {
                title: "src/main.rs".into(),
                meta: "12 行".into(),
                kind: "text".into(),
                lines: vec![wunder_desktop::NativeWorkflowLine {
                    ratio: 0.0,
                    text: "fn main() {}".into(),
                    kind: "text".into(),
                    number: "1".into(),
                }],
            }],
            patch_files: vec![NativePatchFile {
                path: "src/main.rs".into(),
                action: "modify".into(),
                added: "1".into(),
                deleted: "1".into(),
                lines: vec![NativePatchLine {
                    text: "fn main() {}".into(),
                    kind: "add".into(),
                    number: "1".into(),
                }],
            }],
        }
    }

    #[test]
    fn turn_marks_point_at_user_rows_and_newest_answers() {
        let mut user_one = RowData::new(KIND_USER, "u1");
        user_one.text = "第一个问题".into();
        let mut note = RowData::new(KIND_BODY, "b1");
        note.text = "中间说明".into();
        let mut answer_one = RowData::new(KIND_BODY, "b2");
        answer_one.text = "第一个回答".into();
        let mut user_two = RowData::new(KIND_USER, "u2");
        user_two.text = "第二个问题".into();
        let mut answer_two = RowData::new(KIND_BODY, "b3");
        answer_two.text = "第二个回答".into();
        let rows: Vec<&RowData> = vec![&user_one, &note, &answer_one, &user_two, &answer_two];
        let marks = turn_marks(&rows);
        assert_eq!(marks.len(), 2);
        assert_eq!(marks[0].user, "第一个问题");
        assert_eq!(marks[0].agent, "第一个回答");
        assert!((marks[0].fraction - 0.0).abs() < f32::EPSILON);
        assert_eq!(marks[1].user, "第二个问题");
        assert_eq!(marks[1].agent, "第二个回答");
        assert!((marks[1].fraction - 0.6).abs() < 1e-6);
    }

    #[test]
    fn tool_rows_keep_the_desktop_detail_projection() {
        let child = tool_child("turn", &entry());
        assert_eq!(child.tool_name, "读取文件");
        assert_eq!(child.tool_icon, "file-lines");
        assert_eq!(child.target, "src/main.rs");
        assert_eq!(workflow_sections(&child.sections).row_count(), 1);
        assert_eq!(patch_cards(&child.patch).row_count(), 1);
    }

    #[test]
    fn process_rows_carry_the_change_size_of_an_edit_call() {
        let child = tool_child("turn", &entry());
        let (diff_add, diff_del) = patch_diff(&child.patch);
        assert_eq!((diff_add.as_str(), diff_del.as_str()), ("1", "1"));
        // A tool that changed no file keeps both sides empty so the row never
        // prints "+0 −0".
        assert_eq!(
            patch_diff(&[]),
            (String::new(), String::new()),
            "no file change means no diff label"
        );
        let bar = RowData {
            kind: KIND_PROCESS,
            id: "bar-1".into(),
            process_children: vec![child],
            ..Default::default()
        };
        let row = bar.to_row(&mut MarkdownRenderer::default());
        let entries = row.process_entries;
        let first = entries.row_data(0).expect("process entry");
        assert_eq!(first.diff_add.as_str(), "1");
        assert_eq!(first.diff_del.as_str(), "1");
        // A failed call never reads as a real change, even though it retains its
        // pending preview card.
        let mut failed = tool_child("turn", &entry());
        failed.status_kind = STATUS_FAILED;
        let bar = RowData {
            kind: KIND_PROCESS,
            id: "bar-2".into(),
            process_children: vec![failed],
            ..Default::default()
        };
        let row = bar.to_row(&mut MarkdownRenderer::default());
        let first = row.process_entries.row_data(0).expect("process entry");
        assert!(first.diff_add.is_empty() && first.diff_del.is_empty());
    }

    #[test]
    fn glob_tool_rows_use_the_folder_tree_icon() {
        assert_eq!(tool_icon_key("文件名匹配"), "folder-tree");
        assert_eq!(tool_icon_key("glob"), "folder-tree");
        assert_eq!(tool_icon_key("glob_files"), "folder-tree");
    }

    #[test]
    fn bubble_hint_strips_markdown_and_caps_at_96_chars() {
        assert_eq!(
            bubble_hint("看看 ![图](https://example.com/a.png) 和 [文档](https://example.com) 吧"),
            "看看 和 文档 吧"
        );
        assert_eq!(
            bubble_hint("**完成**\n\n- 第一步\n- 第二步"),
            "完成 第一步 第二步"
        );
        let long = bubble_hint(&"字".repeat(120));
        assert_eq!(long.chars().count(), 97);
        assert!(long.ends_with('…'));
        assert_eq!(bubble_hint("[未闭合"), "[未闭合");
    }

    #[test]
    fn patch_cards_carry_a_natural_width_for_long_lines() {
        let mut entry = entry();
        entry.patch_files[0].lines[0].text = "x".repeat(200);
        let cards = patch_cards(&entry.patch_files);
        let card = cards.row_data(0).expect("patch card");
        assert!(card.content_width > 1200.0, "{}", card.content_width);
        assert_eq!(card.path, "src/main.rs");
    }

    #[test]
    fn tool_target_rejects_a_truncated_preview() {
        let mut entry = entry();
        entry.preview = "x".repeat(640);
        assert!(tool_target(&entry).is_empty());
    }

    #[test]
    fn running_durable_labels_keep_the_process_and_status_live() {
        for label in [
            "正在生成…",
            "正在排队",
            "等待用户输入",
            "正在执行工具…",
            "正在思考…",
        ] {
            assert_eq!(status_kind(label), STATUS_RUNNING, "{label}");
            assert!(status_metric(label).is_some(), "{label}");
        }
        assert_eq!(status_kind("任务完成"), STATUS_DONE);
        assert_eq!(status_kind("已停止"), STATUS_STOPPED);
        assert_eq!(status_kind("执行失败"), STATUS_FAILED);
        assert_eq!(status_metric("正在生成…").unwrap().0, "status-outputting");
        assert_eq!(status_metric("正在排队").unwrap().0, "status-queued");
    }

    #[test]
    fn live_reasoning_entries_split_on_rounds_and_tool_steps() {
        let mut live = LiveTurn::new("hi".into());
        live.append_reasoning(1, "第一段", false);
        live.append_reasoning(1, " 继续", false);
        assert_eq!(live.steps.len(), 1);
        live.close_reason(STATUS_DONE);
        live.push_tool(LiveTool {
            id: "call-1".into(),
            title: "读取文件".into(),
            tool_icon: "file-lines".into(),
            summary: "src/main.rs".into(),
            state: "completed".into(),
            detail: String::new(),
            target: String::new(),
            sections: Vec::new(),
            patch: Vec::new(),
            tokens: String::new(),
            duration: String::new(),
        });
        live.append_reasoning(1, "第二段", false);
        assert_eq!(live.steps.len(), 3);

        let rows = live.rows();
        let bar = rows
            .iter()
            .find(|row| row.kind == KIND_PROCESS)
            .expect("process bar");
        assert_eq!(bar.process_children.len(), 3);
        assert_eq!(bar.process_children[0].kind, KIND_REASON);
        assert_eq!(bar.process_children[0].detail, "第一段 继续");
        assert_eq!(bar.process_children[0].status_kind, STATUS_DONE);
        assert_eq!(bar.process_children[1].kind, KIND_TOOL);
        assert_eq!(bar.process_children[2].kind, KIND_REASON);
        assert_eq!(bar.process_children[2].status_kind, STATUS_RUNNING);

        // A settled turn must not leave the newest thought spinning.
        live.status = STATUS_DONE;
        let rows = live.rows();
        let bar = rows
            .iter()
            .find(|row| row.kind == KIND_PROCESS)
            .expect("process bar");
        assert_eq!(bar.process_children[2].status_kind, STATUS_DONE);
    }

    #[test]
    fn a_streaming_body_stays_plain_until_the_turn_settles() {
        let mut live = LiveTurn::new("hi".into());
        live.answer = "**流式**".into();
        live.user_round = 7;
        let rows = live.rows();
        let body = rows
            .iter()
            .find(|row| row.kind == KIND_BODY)
            .expect("body row");
        assert!(body.blocks_src.is_empty());
        let footer = rows
            .iter()
            .find(|row| row.kind == KIND_FOOTER)
            .expect("footer row");
        assert_eq!(footer.stats[0].0, "status-requesting");
        assert_eq!(footer.user_round, 7);
        let shared = footer.to_row(&mut MarkdownRenderer::default());
        assert_eq!(shared.user_round, 7);
        live.status = STATUS_DONE;
        let rows = live.rows();
        let body = rows
            .iter()
            .find(|row| row.kind == KIND_BODY)
            .expect("body row");
        assert_eq!(body.blocks_src, "**流式**");
    }
}
