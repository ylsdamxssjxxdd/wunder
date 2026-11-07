//! Durable turn → flat row projection for the phone transcript.
//!
//! The engine owns this data on worker threads, so it must stay `Send`: rows
//! are plain strings, and the Slint types (`MdBlock`, `ModelRc`, `Image`) are
//! built only on the UI thread by [`RowData::to_row`]. 蛹 computes the same
//! rows in `frontend-slint/src/timeline.rs`, bound to its own window; 茧 keeps
//! the same row kinds and field meanings while dropping the desktop
//! bookkeeping (group bars, process paging, subagent docks).

use crate::md::MarkdownRenderer;
use crate::{MdBlock, TimelineRow, TurnStatMetric};
use chrono::{Local, TimeZone};
use slint::{ModelRc, SharedString, VecModel};
use std::hash::{Hash, Hasher};
use wunder_desktop::{NativeChatTurn, NativeWorkflowEntry};

pub const KIND_USER: i32 = 0;
pub const KIND_BODY: i32 = 1;
pub const KIND_REASON: i32 = 2;
pub const KIND_TOOL: i32 = 3;
pub const KIND_FOOTER: i32 = 7;
pub const KIND_ERROR: i32 = 8;

pub const STATUS_DONE: i32 = 0;
pub const STATUS_RUNNING: i32 = 1;
pub const STATUS_FAILED: i32 = 2;
pub const STATUS_STOPPED: i32 = 3;

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
    pub stats: Vec<(String, String)>,
    pub error_title: String,
    pub error_hint: String,
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
        self.stats.hash(&mut hasher);
        self.error_title.hash(&mut hasher);
        self.error_hint.hash(&mut hasher);
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
        row.error_title = self.error_title.as_str().into();
        row.error_hint = self.error_hint.as_str().into();
        if !self.blocks_src.is_empty() {
            row.blocks = blocks_model(renderer, &self.blocks_src);
        }
        if !self.reasoning_src.is_empty() {
            row.reasoning_blocks = blocks_model(renderer, &self.reasoning_src);
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
    match state {
        "running" | "queued" | "pending" | "in_progress" => STATUS_RUNNING,
        "failed" | "error" | "rejected" | "cancelled" | "canceled" => STATUS_FAILED,
        "stopped" | "interrupted" => STATUS_STOPPED,
        _ => STATUS_DONE,
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

fn tool_row(turn_id: &str, entry: &NativeWorkflowEntry) -> RowData {
    let mut row = RowData::new(KIND_TOOL, format!("{turn_id}-tool-{}", entry.id));
    row.tool_name = entry.title.clone();
    row.summary = entry.preview.clone();
    row.detail = entry.detail.clone();
    row.foldable = !entry.detail.is_empty();
    row.status_kind = status_kind(&entry.state);
    row.tool_metrics = tool_metrics(&entry.tokens, &entry.duration);
    // The collapsed row is a single outcome line; the full result is parsed
    // only after the user unfolds it, exactly like the desktop fold.
    row.reasoning_src = entry.detail.clone();
    row
}

fn tool_metrics(tokens: &str, duration: &str) -> String {
    match (tokens.trim().is_empty(), duration.trim().is_empty()) {
        (true, true) => String::new(),
        (false, true) => tokens.to_string(),
        (true, false) => duration.to_string(),
        (false, false) => format!("{tokens} {duration}"),
    }
}

fn footer_row(turn: &NativeChatTurn) -> Option<RowData> {
    let stats = &turn.assistant.stats;
    let mut metrics: Vec<(String, String)> = Vec::new();
    if let Some(duration) = stats.duration_s.filter(|value| *value > 0.0) {
        metrics.push(("stopwatch".into(), format!("{duration:.1}s")));
    }
    if let Some(tools) = stats.tool_calls.filter(|value| *value > 0.0) {
        metrics.push(("screwdriver-wrench".into(), format!("{tools:.0}")));
    }
    if let Some(tokens) = stats.consumed_tokens.filter(|value| *value > 0.0) {
        metrics.push(("bolt".into(), format_tokens(tokens)));
    }
    if metrics.is_empty() {
        return None;
    }
    let mut row = RowData::new(KIND_FOOTER, format!("{}-footer", turn.root_id));
    row.text = turn.assistant.text.clone();
    row.status_kind = status_kind(&turn.assistant.state);
    row.stats = metrics;
    Some(row)
}

fn format_tokens(value: f64) -> String {
    if value >= 1_000_000.0 {
        format!("{:.1}M", value / 1_000_000.0)
    } else if value >= 1_000.0 {
        format!("{:.1}k", value / 1_000.0)
    } else {
        format!("{value:.0}")
    }
}

/// Project one durable user turn into the rows the transcript renders.
pub fn project_turn(turn: &NativeChatTurn) -> Vec<RowData> {
    let mut rows = Vec::new();

    let mut user = RowData::new(KIND_USER, format!("{}-user", turn.root_id));
    user.text = turn.user.text.clone();
    user.time = clock_time(turn.user.created_at);
    rows.push(user);

    let mut answer_shown = false;
    for round in &turn.rounds {
        if !round.reasoning.trim().is_empty() {
            // Round ordinals restart across continuation executions, and the
            // durable item id is the only stable identity.
            let reason_id = if round.item_id.is_empty() {
                format!("{}-reason-{}-{}", turn.root_id, round.execution, round.round)
            } else {
                format!("{}-reason-{}", turn.root_id, round.item_id)
            };
            let mut row = RowData::new(KIND_REASON, reason_id);
            row.tool_name = "思考".into();
            row.summary = preview_text(&round.reasoning, 120);
            row.detail = round.reasoning.clone();
            row.reasoning_src = round.reasoning.clone();
            row.foldable = true;
            rows.push(row);
        }
        for entry in &round.items {
            rows.push(tool_row(&turn.root_id, entry));
        }
        // 蛹 renders every non-empty round text as a body block; `is_answer`
        // only picks which one owns the footer. Restricting this to the answer
        // round dropped the visible reply of waiting-input turns after reload.
        if !round.text.trim().is_empty() {
            let mut row = RowData::new(
                KIND_BODY,
                format!("{}-body-{}-{}", turn.root_id, round.execution, round.round),
            );
            row.text = round.text.clone();
            row.blocks_src = round.text.clone();
            rows.push(row);
            answer_shown = true;
        }
    }
    // Older threads may not mark an answer round; fall back to the assistant
    // message so the reply is never dropped.
    if !answer_shown && !turn.assistant.text.trim().is_empty() {
        let mut row = RowData::new(KIND_BODY, format!("{}-body-final", turn.root_id));
        row.text = turn.assistant.text.clone();
        row.blocks_src = turn.assistant.text.clone();
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

/// Rows for the turn currently streaming. The durable projection replaces
/// them as soon as the runtime commits the turn.
pub struct LiveTurn {
    pub root_id: String,
    pub user: String,
    pub reasoning: String,
    pub answer: String,
    pub tools: Vec<LiveTool>,
    pub status: i32,
    pub failure: Option<String>,
}

pub struct LiveTool {
    pub id: String,
    pub title: String,
    pub state: String,
    pub detail: String,
    pub target: String,
}

impl LiveTurn {
    pub fn new(user: String) -> Self {
        Self {
            root_id: "live".into(),
            user,
            reasoning: String::new(),
            answer: String::new(),
            tools: Vec::new(),
            status: STATUS_RUNNING,
            failure: None,
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
        if !self.reasoning.is_empty() {
            let mut row = RowData::new(KIND_REASON, format!("{}-reason", self.root_id));
            row.tool_name = "思考".into();
            // The fold header shows one outcome line; while streaming it is the
            // tail of the thought, matching the desktop marquee row.
            row.summary = self
                .reasoning
                .lines()
                .last()
                .map(|line| preview_text(line, 120))
                .unwrap_or_default();
            row.detail = self.reasoning.clone();
            row.reasoning_src = self.reasoning.clone();
            row.foldable = true;
            row.status_kind = if self.status == STATUS_RUNNING {
                STATUS_RUNNING
            } else {
                STATUS_DONE
            };
            rows.push(row);
        }
        for tool in &self.tools {
            let mut row = RowData::new(KIND_TOOL, format!("{}-tool-{}", self.root_id, tool.id));
            row.tool_name = tool.title.clone();
            row.target = tool.target.clone();
            row.summary = preview_text(&tool.detail, 160);
            row.detail = tool.detail.clone();
            row.reasoning_src = tool.detail.clone();
            row.foldable = !tool.detail.is_empty();
            row.status_kind = status_kind(&tool.state);
            rows.push(row);
        }
        if !self.answer.is_empty() {
            let mut row = RowData::new(KIND_BODY, format!("{}-body", self.root_id));
            row.text = self.answer.clone();
            row.blocks_src = self.answer.clone();
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
        for row in &mut rows {
            // Live rows cannot be folded: their index is not stable while the
            // durable prefix grows, so the fold callback ignores them.
            row.payload = -1;
        }
        rows
    }
}
