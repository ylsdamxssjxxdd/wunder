//! Flat, incremental chat timeline (§7).
//!
//! One bounded row per user bubble, assistant body block, reasoning entry, tool
//! entry, tool batch bar, work-process bar and completed-turn divider.
//! Everything is projected once on the Rust side: the Slint thread only lays out
//! the cells it is handed and reports each measured height back, so a streaming
//! token never rebuilds the history. Fold state lives in the model and is
//! patched in place, which keeps the reducer out of every user fold.
//!
//! The work-process bar owns thinking, tools and earlier answer blocks. It
//! ships folded in a bounded viewport; batch rows retain invocation identity.
use crate::{MdBlock, TimelineRow, TurnMark};
#[allow(unused_imports)]
use slint::{Model, ModelRc, VecModel};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use wunder_desktop::native::{NativeSubagentCard, NativeWorkflowEntry};
use wunder_desktop::NativeDesktop;

/// Row kinds; keep in sync with `EntryKind` in ui/timeline.slint.
pub const KIND_USER: i32 = 0;
pub const KIND_BODY: i32 = 1;
pub const KIND_REASON: i32 = 2;
pub const KIND_TOOL: i32 = 3;
pub const KIND_GROUP: i32 = 4;
pub const KIND_SUBAGENT: i32 = 5;
pub const KIND_PROCESS: i32 = 6;
pub const KIND_FOOTER: i32 = 7;
/// The turn's failure card, rendered in the assistant's place.
pub const KIND_ERROR: i32 = 8;
/// Status kinds: 0 done, 1 running, 2 failed, 3 stopped by the user.
pub const STATUS_DONE: i32 = 0;
pub const STATUS_RUNNING: i32 = 1;
pub const STATUS_FAILED: i32 = 2;
/// Terminal, but at the user's request. Neither a success mark nor a failure
/// mark tells the truth about a turn the user stopped.
pub const STATUS_STOPPED: i32 = 3;
/// A row outside any tool batch.
pub const NO_GROUP: i32 = -1;
/// A row outside the work-process fold: the turn's newest answer block.
pub const NO_PROCESS: i32 = -1;

/// How a settled turn ended. The three outcomes are distinct on purpose: a
/// user stop is neither a success nor a failure, and conflating it with the
/// failure path is what painted stopped turns red.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settle {
    Done,
    Failed,
    Stopped,
}

/// Shared by live and durable projections: child runs keep their own cards,
/// and only the newest answer remains outside the turn's work process.
pub(crate) fn is_process_step(kind: i32, index: usize, newest_body: usize) -> bool {
    matches!(kind, KIND_REASON | KIND_TOOL) || (kind == KIND_BODY && index != newest_body)
}
/// Live rows form a rolling window. Durable process pagination keeps every
/// earlier step accessible without retaining the whole execution in memory.
pub(crate) const MAX_LIVE_PROCESS_ROWS: usize = 160;
/// Characters kept for the ruler's hover preview, matching the web ruler card
/// (user title 160, agent snippet 200).
const USER_PREVIEW_LIMIT: usize = 160;
const AGENT_PREVIEW_LIMIT: usize = 200;

/// Collapse whitespace runs into single spaces and cut at `limit` characters.
/// A cut preview ends with an ellipsis so it never reads as the whole message.
fn preview_text(text: &str, limit: usize) -> String {
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

pub fn status_kind(state: &str) -> i32 {
    match state {
        "running" | "queued" | "pending" => STATUS_RUNNING,
        "failed" | "error" | "rejected" | "cancelled" | "canceled" => STATUS_FAILED,
        _ => STATUS_DONE,
    }
}

/// Footer status for the turn's fork action. Live turns arrive as localized
/// phases ("请求中" / "正在生成…"), which the English-only `status_kind` would
/// settle as done — exposing the fork action under an answer that is still
/// streaming. Every phrase that is not an explicit terminal state counts as
/// running; this stays allocation-free because it runs once per stream frame.
pub fn footer_status_kind(state: &str) -> i32 {
    let state = state.trim();
    match state {
        "failed" | "error" | "rejected" | "执行失败" | "任务失败" => STATUS_FAILED,
        "cancelled" | "canceled" | "interrupted" | "stopped" | "已停止" | "已取消" | "已中断" => {
            STATUS_STOPPED
        }
        "" | "idle" | "ready" | "none" | "completed" | "complete" | "done" | "ok" | "success"
        | "final" | "任务完成" | "输出已结束" | "已完成" => STATUS_DONE,
        _ if state.starts_with("失败") => STATUS_FAILED,
        _ if state.starts_with("已停止")
            || state.starts_with("已取消")
            || state.starts_with("已中断") =>
        {
            STATUS_STOPPED
        }
        _ => STATUS_RUNNING,
    }
}

/// Runtime metrics to row metrics. One conversion keeps the live path and the
/// history projection producing identical rows.
pub fn stat_metrics(metrics: &[crate::turn_stats::Metric]) -> Vec<crate::TurnStatMetric> {
    metrics
        .iter()
        .map(|metric| crate::TurnStatMetric {
            // Preserve the status-specific icon key so the footer can use the
            // same icon and motion as the web status line.
            key: metric.icon.into(),
            value: metric.value.as_str().into(),
        })
        .collect()
}

/// One child run to its zero-height status marker. The marker feeds turn
/// status accounting; child details are presented by the composer dock.
pub fn subagent_row(card: &NativeSubagentCard) -> TimelineRow {
    let status = if card.failed || matches!(card.status.as_str(), "failed" | "error") {
        STATUS_FAILED
    } else if card.is_running() {
        STATUS_RUNNING
    } else {
        STATUS_DONE
    };
    let message = if card.latest_message.is_empty() {
        card.error_message.as_str()
    } else {
        card.latest_message.as_str()
    };
    let mut summary: String = message
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(160)
        .collect();
    if summary.is_empty() {
        // A settled card without a message would repeat its status label, so
        // only the unfinished states carry a placeholder line.
        if status != STATUS_DONE {
            summary = if status == STATUS_RUNNING {
                "正在执行…".into()
            } else {
                "执行失败".into()
            };
        }
    }
    // The child session is the card's identity: a live card starts from the
    // dispatch event (which has no durable item yet) and the same row must be
    // patched, not duplicated, when the durable `subagent_run` item replays.
    let mut row = if card.session_id.is_empty() {
        blank(KIND_SUBAGENT, format!("sub-{}", card.item_id))
    } else {
        blank(KIND_SUBAGENT, format!("sub-{}", card.session_id))
    };
    row.text = card.title.as_str().into();
    row.summary = summary.as_str().into();
    row.tool_icon = wunder_desktop::tool_icons::workflow_icon("subagent_control").into();
    row.status_kind = status;
    row
}

fn blank(kind: i32, id: String) -> TimelineRow {
    TimelineRow {
        user_round: 0,
        kind,
        id: id.into(),
        visible: true,
        text: slint::SharedString::default(),
        time: slint::SharedString::default(),
        blocks: ModelRc::default(),
        summary: slint::SharedString::default(),
        tool_name: slint::SharedString::default(),
        tool_icon: slint::SharedString::default(),
        target: slint::SharedString::default(),
        tool_metrics: slint::SharedString::default(),
        tool_metrics_visible: false,
        tokens: Default::default(),
        duration: Default::default(),
        read_lines: Default::default(),
        detail: slint::SharedString::default(),
        reasoning_blocks: ModelRc::default(),
        sections: ModelRc::default(),
        process_entries: ModelRc::default(),
        process_root: Default::default(),
        process_paged: false,
        process_local: false,
        process_older: false,
        process_newer: false,
        // Unknown page edges; a loaded page writes its own cursors.
        process_first: -1,
        process_last: -1,
        process_loading: false,
        process_error: false,
        process_revision: 0,
        status_kind: STATUS_DONE,
        foldable: false,
        open: false,
        group_idx: NO_GROUP,
        group_open: false,
        process_idx: NO_PROCESS,
        process_open: false,
        payload: 0,
        patch: ModelRc::default(),
        stats: ModelRc::default(),
        error_title: slint::SharedString::default(),
        error_hint: slint::SharedString::default(),
    }
}

/// One body block streamed by a model round and frozen when the round ends.
struct Body {
    text: String,
    blocks: crate::message_blocks::Blocks,
    index: usize,
    /// Set by every streamed delta and cleared by `flush_body`, so a frame with
    /// no new text never re-projects the block.
    pending: bool,
}

/// One streamed reasoning segment; closed as soon as the model acts.
struct Thinking {
    text: String,
    index: usize,
    /// The segment so far, kept out of the borrow taken for patching.
    summary: slint::SharedString,
    pending: bool,
}

/// One tool invocation of the current batch.
struct ToolItem {
    id: String,
    index: usize,
    /// Batch that owns this invocation. Result frames may arrive after a later
    /// batch has started, so the current `group` is not a safe lookup key.
    group: i32,
}
/// One batch bar and its bounded readout, kept while the turn lives so a late
/// result frame still updates the bar it belongs to.
struct BatchBar {
    group: i32,
    index: usize,
    entries: usize,
    /// The newest entry of the batch, read out while the bar is folded.
    latest: slint::SharedString,
}

/// The live turn reducer.
pub struct Timeline {
    model: Rc<VecModel<TimelineRow>>,
    marks: Rc<VecModel<TurnMark>>,
    marks_dirty: Cell<bool>,
    body: Option<Body>,
    thinking: Option<Thinking>,
    tools: Vec<ToolItem>,
    /// Known batch bars of this turn, newest last.
    bars: Vec<BatchBar>,
    group: i32,
    next_group: i32,
    reasons: usize,
    body_serial: u64,
    discarded_steps: i32,
    /// Work-process fold of the live turn: its id, its bar row, and the newest
    /// answer block, which is the one row the fold leaves open.
    process: i32,
    next_process: i32,
    process_bar: usize,
    last_body: usize,
    /// A body opened immediately after a round boundary is the new answer
    /// candidate.  A later reasoning frame in that same round must not pull
    /// it into the process fold: the boundary already established its
    /// ownership.  Tool calls still explicitly admit it as intermediate.
    protected_body: usize,
    /// The next body opened after `close_segments` starts as the answer tail.
    protect_next_body: bool,
    /// Independent turn footer, always after body/process/card rows.
    footer: Cell<usize>,
    turn: Option<String>,
    /// Row index of the live turn's own user bubble; the ruler's newest mark is
    /// that row, so a streaming flush can patch its preview without a scan.
    active_user_row: usize,
    /// The live turn ran to its end but its rows are still the only copy: the
    /// durable snapshot has not caught up with it yet.
    settled: bool,
    /// Whether frozen body blocks get syntax highlighting; the reducer turns
    /// this on for the chat view and off elsewhere.
    /// Whether MainWindow.timeline already observes `model`. Binding once
    /// avoids replacing the list's model handle during every 33ms stream tick.
    published: Cell<bool>,
    markdown_desktop: Option<Arc<NativeDesktop>>,
    markdown_root: String,
}

impl Timeline {
    pub fn set_markdown_desktop(&mut self, desktop: Arc<NativeDesktop>) {
        self.markdown_desktop = Some(desktop);
    }

    pub fn set_markdown_root(&mut self, root: String) {
        if self.markdown_root == root {
            return;
        }
        self.markdown_root = root;
        self.hydrate_markdown_images();
    }

    /// Start bounded image loading for the rows already projected into the
    /// timeline. This is called after history replacement because restored
    /// rows do not pass through the streaming body builder.
    pub fn hydrate_markdown_images(&self) {
        let Some(desktop) = self.markdown_desktop.clone() else {
            return;
        };
        if self.markdown_root.trim().is_empty() {
            return;
        }
        for row in self.model.iter() {
            if row.blocks.row_count() > 0 {
                crate::native_markdown::hydrate_model(
                    row.blocks,
                    desktop.clone(),
                    self.markdown_root.clone(),
                );
            }
        }
    }
}

impl Default for Timeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Timeline {
    pub fn new() -> Self {
        Self {
            model: Rc::new(VecModel::default()),
            marks: Rc::new(VecModel::default()),
            marks_dirty: Cell::new(true),

            body: None,
            thinking: None,
            tools: Vec::new(),
            bars: Vec::new(),
            group: NO_GROUP,
            next_group: 0,
            reasons: 0,
            body_serial: 0,
            discarded_steps: 0,
            process: NO_PROCESS,
            next_process: 0,
            process_bar: 0,
            last_body: usize::MAX,
            protected_body: usize::MAX,
            protect_next_body: false,
            footer: Cell::new(usize::MAX),
            turn: None,
            active_user_row: usize::MAX,
            settled: false,
            published: Cell::new(false),
            markdown_desktop: None,
            markdown_root: String::new(),
        }
    }

    pub fn model(&self) -> ModelRc<TimelineRow> {
        ModelRc::from(self.model.clone())
    }

    pub fn marks_model(&self) -> ModelRc<TurnMark> {
        ModelRc::from(self.marks.clone())
    }

    /// Refresh the small turn ruler only when the turn topology changes. The
    /// transcript model is frame-stable; replacing the marks handle on each
    /// stream tick made the overlay do needless work alongside it.
    pub fn sync_turn_marks(&self) {
        if !self.marks_dirty.replace(false) {
            return;
        }
        let next = self.turn_marks();
        if self.marks.iter().ne(next.iter().cloned()) {
            self.marks.set_vec(next);
        }
    }

    /// Return true exactly once when the owning window should bind this model.
    pub fn needs_publish(&self) -> bool {
        !self.published.replace(true)
    }

    pub fn row_count(&self) -> usize {
        self.model.row_count()
    }

    /// Number of answer blocks currently published; used by the smoke checks
    /// and the companion bubble instead of walking the row list.
    pub fn answer_count(&self) -> usize {
        self.model
            .iter()
            .filter(|row| row.kind == KIND_BODY && !row.text.is_empty())
            .count()
    }

    /// The newest non-empty answer block text, or empty when the thread has no
    /// answer yet.
    pub fn last_answer(&self) -> String {
        for index in (0..self.model.row_count()).rev() {
            if let Some(row) = self.model.row_data(index) {
                if row.kind == KIND_BODY && !row.text.is_empty() {
                    return row.text.to_string();
                }
            }
        }
        String::new()
    }

    /// One mark per user turn for the chat view's turn ruler. The mark carries
    /// the turn row's fraction, the user's message and the turn's newest answer,
    /// so the hover card needs no second lookup into the row model. One bounded
    /// pass per structural change keeps it in sync without reporting per-row
    /// heights; capped to the newest 100 turns so the strip stays readable on
    /// marathon threads.
    pub fn turn_marks(&self) -> Vec<TurnMark> {
        let total = self.model.row_count();
        if total <= 1 {
            return Vec::new();
        }
        let users: Vec<usize> = self
            .model
            .iter()
            .enumerate()
            .filter(|(_, row)| row.kind == KIND_USER)
            .map(|(index, _)| index)
            .collect();
        let first = users.len().saturating_sub(100);
        let mut marks = Vec::with_capacity(users.len() - first);
        for (position, &index) in users.iter().enumerate().skip(first) {
            let end = users.get(position + 1).copied().unwrap_or(total);
            let user = self
                .model
                .row_data(index)
                .map(|row| preview_text(&row.text, USER_PREVIEW_LIMIT))
                .unwrap_or_default();
            // The turn's newest answer is its last non-empty body row; earlier
            // bodies are intermediate notes inside the work process. Walking
            // back from the turn's end usually hits it after a few rows.
            let answer = ((index + 1)..end)
                .rev()
                .find(|row_index| {
                    self.model
                        .row_data(*row_index)
                        .is_some_and(|row| row.kind == KIND_BODY && !row.text.is_empty())
                })
                .and_then(|row_index| self.model.row_data(row_index))
                .map(|row| preview_text(&row.text, AGENT_PREVIEW_LIMIT))
                .unwrap_or_default();
            marks.push(TurnMark {
                fraction: index as f32 / total as f32,
                user: user.into(),
                agent: answer.into(),
            });
        }
        marks
    }

    /// Mirror the streaming answer into the newest tick's preview so hovering
    /// the live turn reads the reply as it grows. Skipped while the marks are
    /// dirty: a structural change is about to rebuild them, and until that sync
    /// the tail mark is not guaranteed to be this turn's.
    fn update_tail_mark(&self, text: &str) {
        if self.marks_dirty.get() || self.active_user_row == usize::MAX {
            return;
        }
        let total = self.model.row_count();
        let count = self.marks.row_count();
        if total == 0 || count == 0 {
            return;
        }
        let Some(mut mark) = self.marks.row_data(count - 1) else {
            return;
        };
        if mark.fraction.to_bits() != (self.active_user_row as f32 / total as f32).to_bits() {
            return;
        }
        let agent: slint::SharedString = preview_text(text, AGENT_PREVIEW_LIMIT).into();
        if mark.agent == agent {
            return;
        }
        mark.agent = agent;
        self.marks.set_row_data(count - 1, mark);
    }

    /// Messages this thread already sent, newest first — the walk behind the
    /// composer's ArrowUp recall. Consecutive repeats are collapsed so a
    /// resend does not make the user press the key twice for the same text.
    pub fn recent_user_texts(&self, limit: usize) -> Vec<String> {
        let mut texts: Vec<String> = self
            .model
            .iter()
            .filter(|row| row.kind == KIND_USER)
            .map(|row| row.text.to_string())
            .filter(|text| !text.trim().is_empty())
            .collect();
        // Slint's model iterator is not double-ended, so the newest-first order
        // the walk needs is made by reversing the collected rows.
        texts.reverse();
        texts.dedup();
        texts.truncate(limit);
        texts
    }

    /// Drop the whole timeline, e.g. when another thread is opened.
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.model.set_vec(Vec::new());
        self.marks_dirty.set(true);
        self.active_user_row = usize::MAX;

        self.body = None;
        self.thinking = None;
        self.tools.clear();
        self.bars.clear();
        self.group = NO_GROUP;
        self.next_group = 0;
        self.reasons = 0;
        self.discarded_steps = 0;
        self.process = NO_PROCESS;
        self.process_bar = 0;
        self.last_body = usize::MAX;
        self.protected_body = usize::MAX;
        self.protect_next_body = false;
        self.footer.set(usize::MAX);
        self.turn = None;
        self.settled = false;
    }

    fn push(&mut self, row: TimelineRow) -> usize {
        self.marks_dirty.set(true);
        let footer = self.footer.get();
        if footer < self.model.row_count() {
            // Only the footer moves; existing step handles remain stable.
            self.model.insert(footer, row);
            self.footer.set(footer + 1);
            footer
        } else {
            let index = self.model.row_count();
            self.model.push(row);
            index
        }
    }

    fn patch(&self, index: usize, edit: impl FnOnce(&mut TimelineRow)) {
        let Some(mut row) = self.model.row_data(index) else {
            return;
        };
        edit(&mut row);
        self.sync_process_entry(&row);
        self.model.set_row_data(index, row);
    }

    fn sync_process_entry(&self, row: &TimelineRow) {
        if row.process_idx == NO_PROCESS || !matches!(row.kind, KIND_REASON | KIND_TOOL | KIND_BODY)
        {
            return;
        }
        let Some(bar) = self
            .model
            .row_data(self.process_bar)
            .filter(|bar| bar.process_idx == row.process_idx)
        else {
            return;
        };
        let Some(children) = bar
            .process_entries
            .as_any()
            .downcast_ref::<VecModel<crate::ProcessEntry>>()
        else {
            return;
        };
        let mut entry = crate::timeline_text::process_entry(row);
        if let Some(index) = children.iter().position(|child| child.id == row.id) {
            if bar.process_paged {
                if bar.process_newer {
                    return;
                }
                if let Some(previous) = children.row_data(index) {
                    entry.payload = previous.payload;
                    entry.open = previous.open;
                }
            }
            children.set_row_data(index, entry);
        } else if !bar.process_paged {
            let index = children
                .iter()
                .position(|child| child.payload > row.payload)
                .unwrap_or(children.row_count());
            children.insert(index, entry);
        }
    }

    /// Patch only when a process-fold field actually changed. Streaming tool
    /// results arrive much more often than the fold topology changes; writing
    /// an identical row still invalidates Slint's layout and was the source of
    /// the visible work-process jitter.
    fn patch_process_state(&self, index: usize, process: i32, open: bool) {
        let Some(mut row) = self.model.row_data(index) else {
            return;
        };
        let next_open = process != NO_PROCESS && open;
        if row.process_idx == process && row.process_open == next_open {
            return;
        }
        // Remove a row's old child projection when it leaves the live process.
        // Otherwise the process keeps a stale copy until durable history wins.
        if row.process_idx != NO_PROCESS && row.process_idx != process {
            if let Some(bar) = self
                .model
                .iter()
                .find(|bar| bar.kind == KIND_PROCESS && bar.process_idx == row.process_idx)
            {
                if !bar.process_paged {
                    if let Some(children) = bar
                        .process_entries
                        .as_any()
                        .downcast_ref::<VecModel<crate::ProcessEntry>>()
                    {
                        if let Some(child) = children.iter().position(|child| child.id == row.id) {
                            children.remove(child);
                        }
                    }
                }
            }
        }
        row.process_idx = process;
        row.process_open = next_open;
        self.sync_process_entry(&row);
        self.model.set_row_data(index, row);
    }

    /// Start a new user turn with its bubble. The previous turn is already
    /// frozen by `finish`, so this only resets the builder state. The bubble
    /// carries the turn identity: it is the anchor the durable snapshot
    /// matches when it takes the settled turn over.
    pub fn begin_turn(&mut self, root: &str, text: &str) -> usize {
        self.body = None;
        self.thinking = None;
        self.tools.clear();
        self.bars.clear();
        self.group = NO_GROUP;
        self.reasons = 0;
        self.discarded_steps = 0;
        self.footer.set(usize::MAX);
        self.turn = Some(root.to_string());
        self.settled = false;
        let user_id = format!("user-turn-{root}");
        if let Some(index) = self.model.iter().position(|row| row.id == user_id) {
            if let Some(mut row) = self.model.row_data(index) {
                row.text = text.into();
                row.blocks = crate::message_blocks::from_text(text);
                self.model.set_row_data(index, row);
            }
            self.active_user_row = index;
            // The ruler preview reads this bubble's text, so the marks refresh
            // even though the row's own position did not move.
            self.marks_dirty.set(true);
            let process_id = format!("process-{root}");
            if let Some((bar, row)) = self
                .model
                .iter()
                .enumerate()
                .find(|(_, row)| row.id == process_id)
            {
                self.process_bar = bar;
                self.process = row.process_idx;
                self.next_process = self.next_process.max(self.process);
                self.last_body = usize::MAX;
                self.protected_body = usize::MAX;
                self.footer.set(usize::MAX);
                return index;
            }
            // Hydration can publish the user anchor before its process row.
            // Reuse that anchor when the subscriber attaches instead of
            // adding a second bubble for the same submitted message.
            self.open_process(root);
            return index;
        }
        let mut user = blank(KIND_USER, format!("user-turn-{root}"));
        user.text = text.into();
        user.blocks = crate::message_blocks::from_text(text);
        user.time = crate::timeline_text::clock_hhmm_now().into();
        let index = self.push(user);
        self.active_user_row = index;
        self.open_process(root);
        index
    }

    /// Repaint the live turn's user bubble, e.g. with the composer's local
    /// attachment cards, until the durable projection takes the row over.
    pub fn set_user_blocks(&self, index: usize, blocks: ModelRc<MdBlock>) {
        self.patch(index, |row| {
            if row.kind == KIND_USER {
                row.blocks = blocks;
            }
        });
    }

    /// Remove a provisional live turn when the backend rejects submission.
    /// Only the current turn can own the provisional suffix. An older send's
    /// failure must never remove a newer submission or a durable replacement.
    pub fn remove_live_turn(&mut self, root: &str) -> bool {
        if self.turn.as_deref() != Some(root) {
            return false;
        }
        let key = format!("user-turn-{root}");
        let Some(start) = self.model.iter().position(|row| row.id == key) else {
            return false;
        };
        while self.model.row_count() > start {
            self.model.remove(start);
        }
        self.forget_turn();
        self.sync_fold_ids();
        self.marks_dirty.set(true);
        true
    }

    pub fn set_process_root(&self, root: &str) {
        if self.process != NO_PROCESS && !root.is_empty() {
            self.patch(self.process_bar, |bar| {
                bar.process_root = root.into();
                bar.status_kind = STATUS_RUNNING;
            });
        }
    }

    /// Publish the live turn's work-process bar right behind its bubble. The bar
    /// is pushed once per turn so its row index never moves, and stays invisible
    /// until the turn actually has a process worth folding.
    fn open_process(&mut self, root: &str) {
        self.next_process += 1;
        self.process = self.next_process;
        self.last_body = usize::MAX;
        self.footer.set(usize::MAX);
        let mut bar = blank(KIND_PROCESS, format!("process-{root}"));
        bar.process_local = true;
        bar.text = crate::timeline_text::process_label(0).into();
        bar.process_entries = ModelRc::new(VecModel::<crate::ProcessEntry>::default());
        // The local stream can emit a reasoning delta immediately after the
        // user send. Mark the fold live here rather than waiting for the
        // optional native identity frame, so its first child is rendered in
        // that same coalesced UI frame.
        bar.status_kind = STATUS_RUNNING;
        bar.visible = false;
        let index = self.push(bar);
        self.process_bar = index;
        self.protected_body = usize::MAX;
        self.protect_next_body = false;
        self.patch(index, |row| {
            row.payload = index as i32;
            row.process_idx = self.process;
        });
    }

    /// Batches retain invocation ownership only. The process bar is the sole
    /// visible fold, so its entries never wait on a second batch toggle.
    fn batch_open_state(&self) -> bool {
        true
    }

    /// Current open state of the live turn's process bar. Read off the model so
    /// a user click (`toggle` takes `&self`) is what decides it.
    fn process_is_open(&self) -> bool {
        self.model
            .row_data(self.process_bar)
            .is_some_and(|row| row.open)
    }

    /// Keep one process fold from the first action onward. Tool ownership
    /// batches are internal; the newest answer alone stays outside the fold.
    fn refresh_process_bar(&mut self) {
        if self.process == NO_PROCESS {
            return;
        }
        let process = self.process;
        self.trim_process();
        let mut steps = self.discarded_steps;
        let mut region = Vec::new();
        for index in (self.process_bar + 1)..self.model.row_count() {
            let Some(row) = self.model.row_data(index) else {
                continue;
            };
            // A body block without text is a round that never printed; it is
            // not a step and must never occupy a fold entry, or the process
            // shows a blank row in its place.
            if row.kind == KIND_BODY && row.text.is_empty() {
                continue;
            }
            // A body that was already emitted before a tool call is marked as
            // a process item immediately by `include_active_body_in_process`.
            // Its position is preserved even though it is still `last_body`
            // until the next model round opens a new tail.
            let admitted_live_body = row.kind == KIND_BODY
                && row.process_idx == process
                && !(self.settled && index == self.last_body);
            if row.kind == KIND_GROUP
                || is_process_step(row.kind, index, self.last_body)
                || admitted_live_body
            {
                region.push(index);
                // The currently streaming print is already visible as the
                // answer tail when the process is open; it is a child for
                // ordering but does not add a second step until a newer body
                // replaces it.
                steps += i32::from(
                    row.kind != KIND_GROUP && !(admitted_live_body && index == self.last_body),
                );
            }
        }
        let enabled = steps > 0;
        // First reveal of a live turn's work-process bar: unfold it by default.
        // The bar's own `open` is the single source of truth for the fold, so a
        // click can collapse it again mid-run. The view used to force a running
        // process open off `status-kind`, which made the bar handle inert until
        // the turn settled; keeping the default here fixes that.
        if enabled && !self.settled {
            let first_reveal = self
                .model
                .row_data(self.process_bar)
                .is_some_and(|row| !row.visible);
            if first_reveal {
                self.patch(self.process_bar, |row| row.open = true);
            }
        }
        let open = self.process_is_open();
        for index in region {
            self.patch_process_state(index, process, open);
        }
        if self.settled && self.last_body != usize::MAX {
            self.patch_process_state(self.last_body, NO_PROCESS, false);
        }
        let bar = self.process_bar;
        let label = crate::timeline_text::process_label(steps);
        // Only publish execution phase here. The entry owns its preview and
        // output; copying that into this header hides the live state.
        let mut activity = if self.settled {
            "已完成"
        } else {
            "正在处理…"
        }
        .to_string();
        if !self.settled {
            if self.body.is_some() {
                activity = "正在生成回复…".into();
            }
            for index in (self.process_bar + 1)..self.model.row_count() {
                let Some(row) = self.model.row_data(index) else {
                    continue;
                };
                if row.status_kind != STATUS_RUNNING {
                    continue;
                }
                if row.kind == KIND_REASON {
                    activity = crate::timeline_text::thought_running().into();
                } else if row.kind == KIND_TOOL {
                    activity = format!("执行工具：{}", row.tool_name);
                }
            }
        }
        let summary: slint::SharedString = activity.into();
        if self.model.row_data(bar).is_some_and(|row| {
            row.text == label.as_str() && row.summary == summary && row.visible == enabled
        }) {
            return;
        }
        self.patch(bar, |row| {
            row.text = label.as_str().into();
            row.summary = summary;
            row.visible = enabled;
        });
    }

    /// Admit the active body only when a later process item proves that it is
    /// an intermediate print. Process items that precede the body belong to
    /// the preceding model round and must not pull the current final answer
    /// into the fold while it is still streaming. `after` is the newly added
    /// structural row; keeping this local avoids treating an old process row
    /// as evidence when history was rebased or a new final body was opened.
    fn sync_active_body_process(&mut self, after: Option<usize>) {
        if self.process == NO_PROCESS || self.last_body == usize::MAX {
            return;
        }
        let index = self.last_body;
        let Some(row) = self.model.row_data(index) else {
            return;
        };
        if row.kind != KIND_BODY || row.text.is_empty() || row.process_idx == self.process {
            return;
        }
        let proven_intermediate = after.is_some_and(|at| {
            at > index
                && self.model.row_data(at).is_some_and(|candidate| {
                    matches!(candidate.kind, KIND_REASON | KIND_TOOL | KIND_GROUP)
                })
        });
        if proven_intermediate {
            self.patch_process_state(index, self.process, self.process_is_open());
        }
    }

    fn trim_process(&mut self) {
        while self.model.row_count().saturating_sub(self.process_bar + 1) > MAX_LIVE_PROCESS_ROWS {
            let protected_body = self.body.as_ref().map(|body| body.index);
            let protected_thinking = self.thinking.as_ref().map(|thinking| thinking.index);
            let Some(index) = ((self.process_bar + 1)..self.model.row_count()).find(|index| {
                *index != self.last_body
                    && Some(*index) != protected_body
                    && Some(*index) != protected_thinking
                    && self.model.row_data(*index).is_some_and(|row| {
                        matches!(row.kind, KIND_TOOL | KIND_REASON | KIND_BODY | KIND_GROUP)
                    })
            }) else {
                break;
            };
            let removed = self.model.row_data(index).unwrap();
            if removed.kind != KIND_GROUP {
                self.discarded_steps = self.discarded_steps.saturating_add(1);
            }
            if let Some(bar) = self.model.row_data(self.process_bar) {
                if !bar.process_paged {
                    if let Some(children) = bar
                        .process_entries
                        .as_any()
                        .downcast_ref::<VecModel<crate::ProcessEntry>>()
                    {
                        if let Some(child) =
                            children.iter().position(|entry| entry.id == removed.id)
                        {
                            children.remove(child);
                        }
                    }
                }
            }
            self.model.remove(index);
            if self.footer.get() != usize::MAX && self.footer.get() > index {
                self.footer.set(self.footer.get() - 1);
            }
            self.patch(self.process_bar, |bar| bar.process_older = true);
            self.tools.retain(|tool| tool.index != index);
            self.bars.retain(|bar| bar.index != index);
            for tool in &mut self.tools {
                if tool.index > index {
                    tool.index -= 1;
                }
            }
            for bar in &mut self.bars {
                if bar.index > index {
                    bar.index -= 1;
                }
            }
            if let Some(body) = &mut self.body {
                if body.index > index {
                    body.index -= 1;
                }
            }
            if let Some(thinking) = &mut self.thinking {
                if thinking.index > index {
                    thinking.index -= 1;
                }
            }
            if self.last_body != usize::MAX && self.last_body > index {
                self.last_body -= 1;
            }
            if self.protected_body == index {
                self.protected_body = usize::MAX;
            } else if self.protected_body > index {
                self.protected_body -= 1;
            }
            for shifted in index..self.model.row_count() {
                if let Some(mut row) = self.model.row_data(shifted) {
                    row.payload = shifted as i32;
                    self.sync_process_entry(&row);
                    self.model.set_row_data(shifted, row);
                }
            }
            self.marks_dirty.set(true);
        }
    }

    /// Begin the next assistant body block; the runtime registers exactly one
    /// text item per model round, so a block starts where the last one ended.
    pub fn start_body(&mut self, round: i64) -> usize {
        self.close_thinking(false);
        self.close_body();
        let blocks = crate::message_blocks::Blocks::new();
        self.body_serial = self.body_serial.wrapping_add(1);
        let mut row = blank(
            KIND_BODY,
            format!("body-{}-{}-{round}", self.process, self.body_serial),
        );
        row.blocks = ModelRc::from(blocks.model.clone());
        let index = self.push(row);
        // The process child list orders entries by payload, so the body row
        // must carry its own index like every other structural row; the blank
        // default 0 would insert it at the top of the fold.
        self.patch(index, |row| row.payload = index as i32);
        let protected = self.protect_next_body;
        self.protect_next_body = false;
        self.protected_body = if protected { index } else { usize::MAX };
        self.body = Some(Body {
            text: String::new(),
            blocks,
            index,
            pending: false,
        });
        // The block this round replaces becomes process content: only the
        // newest answer block of the turn stays outside the fold.
        self.last_body = index;
        self.refresh_process_bar();
        self.body.as_ref().map(|body| body.index).unwrap_or(index)
    }

    pub fn append_body(&mut self, delta: &str) -> Result<(), String> {
        {
            let Some(body) = self.body.as_mut() else {
                return Ok(());
            };
            body.blocks.append(delta)?;
            body.text.push_str(delta);
            body.pending = true;
        }
        // The preceding structural event may already have placed this body in
        // the work process. Keep the child projection current immediately;
        // the normal 33 ms flush still owns the visible markdown model.
        self.sync_active_body_process(None);
        Ok(())
    }

    pub fn replace_body(&mut self, text: &str) -> Result<(), String> {
        {
            let Some(body) = self.body.as_mut() else {
                return Ok(());
            };
            body.blocks.replace(text)?;
            body.text = text.to_owned();
            body.pending = true;
        }
        self.sync_active_body_process(None);
        Ok(())
    }

    /// Length of the active block, for the bounded stream-byte counter. The
    /// text itself is already published on the row; cloning it per frame only
    /// to read its length was pure copy traffic.
    pub fn body_len(&self) -> usize {
        self.body.as_ref().map_or(0, |body| body.text.len())
    }

    /// Append a reasoning delta, opening a thinking entry for this segment.
    /// Test fixture convenience; production reducers call
    /// [`Timeline::append_reasoning_item`] with the row id directly.
    #[cfg(test)]
    pub fn append_reasoning(&mut self, delta: &str) -> Result<(), String> {
        self.append_reasoning_item("", delta)
    }

    pub fn append_reasoning_item(&mut self, item_id: &str, delta: &str) -> Result<(), String> {
        let stable_id = (!item_id.is_empty()).then(|| format!("reason-{item_id}"));
        if let Some(thinking) = self.thinking.as_ref() {
            if stable_id.as_ref().is_some_and(|identity| {
                self.model
                    .row_data(thinking.index)
                    .is_some_and(|row| row.id != identity.as_str())
            }) {
                self.close_thinking(false);
            }
        }
        if self.thinking.is_none() {
            // A body delta may still be pending in the same reducer frame.
            // Flush before creating the structural row so admission can copy
            // the actual intermediate text into the process child.
            self.flush_body();
            self.mark_batch();
            let index = self.push(blank(
                KIND_REASON,
                stable_id.unwrap_or_else(|| format!("reason-{}-{}", self.process, self.reasons)),
            ));
            let group = self.group;
            self.patch(index, |row| {
                row.tool_name = crate::timeline_text::thought_running().into();
                row.status_kind = STATUS_RUNNING;
                row.payload = index as i32;
                row.group_idx = group;
                row.group_open = self.batch_open_state();
                row.open = false;
                row.foldable = true;
            });
            self.reasons += 1;
            self.thinking = Some(Thinking {
                text: String::new(),
                index,
                summary: slint::SharedString::default(),
                pending: false,
            });
            if self.protected_body != self.last_body {
                self.sync_active_body_process(Some(index));
            }
            self.refresh_process_bar();
        }
        let Some(thinking) = self.thinking.as_mut() else {
            return Ok(());
        };
        if thinking.text.len().saturating_add(delta.len()) > crate::message_blocks::MAX_TEXT_BYTES {
            return Err("思考内容超过显示容量".into());
        }
        thinking.text.push_str(delta);
        thinking.pending = true;
        Ok(())
    }

    /// A tool call makes preceding model text an intermediate print, even
    /// before the next model round has started. The old reducer only moved it
    /// after that next round, so the body jumped from above the process into it
    /// late and then jumped again at settlement.
    fn include_active_body_in_process(&mut self, force: bool) {
        if self.process == NO_PROCESS || self.last_body == usize::MAX {
            return;
        }
        if !force
            && !(self.process_bar + 1..self.model.row_count()).any(|index| {
                self.model
                    .row_data(index)
                    .is_some_and(|row| matches!(row.kind, KIND_REASON | KIND_TOOL | KIND_GROUP))
            })
        {
            return;
        }
        self.flush_body();
        let index = self.last_body;
        if !self
            .model
            .row_data(index)
            .is_some_and(|row| row.kind == KIND_BODY && !row.text.is_empty())
        {
            return;
        }
        self.patch_process_state(index, self.process, self.process_is_open());
        self.refresh_process_bar();
    }

    /// Freeze the active thinking entry when the model starts acting.
    pub fn close_thinking(&mut self, failed: bool) {
        let Some(thinking) = self.thinking.take() else {
            return;
        };
        let text = thinking.text.clone();
        let summary = crate::message_blocks::reasoning_preview(&text);
        let label = if failed {
            crate::timeline_text::thought_failed()
        } else {
            crate::timeline_text::thought_done()
        };
        self.patch(thinking.index, |row| {
            row.tool_name = label.into();
            row.summary = summary;
            row.detail = text.as_str().into();
            // The thought is finished: parse it once, the same way a settled
            // answer is parsed, so the expanded box renders its Markdown.
            row.reasoning_blocks = crate::message_blocks::from_text(&text);
            row.status_kind = if failed { STATUS_FAILED } else { STATUS_DONE };
        });
    }

    /// Insert the batch bar before its first entry. The bar label is rewritten
    /// after every entry, so the count is always the current one.
    fn mark_batch(&mut self) {
        if self.group != NO_GROUP {
            return;
        }
        let group = self.next_group;
        self.next_group += 1;
        let bar = self.push(blank(KIND_GROUP, format!("group-{group}")));
        let open = self.batch_open_state();
        self.patch(bar, |row| {
            row.visible = false;
            row.open = open;
            row.payload = bar as i32;
            row.group_idx = group;
            row.group_open = open;
        });
        self.bars.push(BatchBar {
            group,
            index: bar,
            entries: 0,
            latest: Default::default(),
        });
        self.group = group;
        self.refresh_process_bar();
    }

    /// Rewrite one batch label. Result frames can be out of order, therefore
    /// this must address the bar by its stable group id rather than by the
    /// newest bar in the turn.
    fn refresh_batch_label_for(&mut self, group: i32) {
        let Some((bar, entries, latest)) = self
            .bars
            .iter()
            .find(|bar| bar.group == group)
            .map(|bar| (bar.index, bar.entries, bar.latest.clone()))
        else {
            return;
        };
        let label = crate::timeline_text::tool_calls(entries as i32);
        self.patch(bar, |row| {
            row.text = label.as_str().into();
            row.summary = latest;
        });
    }

    fn refresh_batch_label(&mut self) {
        if self.group != NO_GROUP {
            self.refresh_batch_label_for(self.group);
        }
    }

    /// Append one tool entry, or update the entry of an already seen id.
    pub fn upsert_tool(&mut self, entry: &NativeWorkflowEntry) {
        let label = crate::timeline_text::tool_label(&entry.title);
        let pending = status_kind(&entry.state) == STATUS_RUNNING;
        let (target, _) = crate::timeline_text::tool_target(entry);
        if let Some(tool) = self.tools.iter().find(|tool| tool.id == entry.id) {
            let index = tool.index;
            // A tool entry settles once. Trailing frames of a command that was
            // handed to the background, or an out-of-order replay, must not pull
            // a settled entry back into the running state.
            if pending
                && self
                    .model
                    .row_data(index)
                    .is_some_and(|row| row.status_kind != STATUS_RUNNING)
            {
                return;
            }
            let group = tool.group;
            let summary = crate::timeline_text::tool_summary(entry);
            let row_summary = crate::timeline_text::tool_row_summary(entry, &target);
            let metrics = crate::timeline_text::tool_metrics(entry);
            let metrics_visible = !metrics.is_empty();
            let copy = crate::timeline_text::copy_text(&entry.sections, entry.detail.as_str());
            // The call frame of a write has no diff yet; the result frame
            // brings the cards. Read them from the current entry, never from a
            // flag cached when the call was first shown.
            let cards = crate::timeline_text::patch_cards(&entry.patch_files);
            let status = status_kind(&entry.state);
            let icon = wunder_desktop::tool_icons::workflow_icon(&entry.title).to_string();
            let sections = crate::timeline_text::sections(&entry.sections);
            self.patch(index, |row| {
                row.tool_name = label.as_str().into();
                row.tool_icon = icon.as_str().into();
                row.target = target.as_str().into();
                row.summary = row_summary.as_str().into();
                row.tool_metrics = metrics.as_str().into();
                row.tool_metrics_visible = metrics_visible;
                row.detail = copy.clone();
                row.sections = sections;
                row.tokens = entry.tokens.as_str().into();
                row.duration = entry.duration.as_str().into();
                row.read_lines = entry.read_lines.as_str().into();
                row.status_kind = status;
                row.patch = cards.clone();
            });
            // A settling result rewrites what the folded bar reads out, even
            // though the batch count stayed the same.
            if let Some(bar) = self.bars.iter_mut().find(|bar| bar.group == group) {
                bar.latest = summary.as_str().into();
            }
            self.refresh_batch_label_for(group);
            self.refresh_process_bar();
            return;
        }
        if pending {
            self.include_active_body_in_process(true);
            self.close_thinking(false);
        }
        if pending {
            self.mark_batch();
        } else if self.group == NO_GROUP {
            // A replay can contain a result without its call frame. Preserve a
            // compact row instead of attaching it to an unrelated later batch.
            self.mark_batch();
        }
        let index = self.push(blank(KIND_TOOL, format!("tool-{}", entry.id)));
        let icon = wunder_desktop::tool_icons::workflow_icon(&entry.title).to_string();
        let summary = crate::timeline_text::tool_summary(entry);
        let row_summary = crate::timeline_text::tool_row_summary(entry, &target);
        let metrics = crate::timeline_text::tool_metrics(entry);
        let metrics_visible = !metrics.is_empty();
        let copy = crate::timeline_text::copy_text(&entry.sections, entry.detail.as_str());
        let cards = crate::timeline_text::patch_cards(&entry.patch_files);
        let status = status_kind(&entry.state);
        let group = self.group;
        let sections = crate::timeline_text::sections(&entry.sections);
        self.patch(index, |row| {
            row.tool_name = label.as_str().into();
            row.tool_icon = icon.as_str().into();
            row.target = target.as_str().into();
            row.summary = row_summary.as_str().into();
            row.tool_metrics = metrics.as_str().into();
            row.tool_metrics_visible = metrics_visible;
            row.detail = copy;
            row.sections = sections;
            row.tokens = entry.tokens.as_str().into();
            row.duration = entry.duration.as_str().into();
            row.read_lines = entry.read_lines.as_str().into();
            row.status_kind = status;
            row.patch = cards;
            row.payload = index as i32;
            row.group_idx = group;
            row.group_open = self.batch_open_state();
            row.open = false;
            row.foldable = true;
        });
        self.tools.push(ToolItem {
            id: entry.id.clone(),
            index,
            group,
        });
        // A result-only replay has no pending call frame to take the fast
        // admission path above. The inserted tool row still proves that the
        // preceding body is intermediate.
        self.flush_body();
        self.sync_active_body_process(Some(index));
        // Count the entry in the batch it was actually attached to.
        if let Some(bar) = self.bars.iter_mut().find(|bar| bar.group == group) {
            bar.entries += 1;
            bar.latest = summary.as_str().into();
        }
        self.refresh_batch_label_for(group);
        self.refresh_process_bar();
    }

    /// End a tool batch so the next reasoning or tool opens a fresh bar. The
    /// batch entries stay known, because a later result frame still updates
    /// the row it created.
    fn close_batch(&mut self) {
        if self.group != NO_GROUP {
            self.refresh_batch_label();
        }
        self.group = NO_GROUP;
    }

    /// Append one child-run status marker, or update the already seen marker.
    /// Progress revisions patch it in place so turn-level status stays live.
    pub fn upsert_subagent(&mut self, card: &NativeSubagentCard) {
        if card.session_id.is_empty() {
            return;
        }
        let row = subagent_row(card);
        let id = row.id.clone();
        if let Some(index) = (0..self.model.row_count()).find(|index| {
            self.model
                .row_data(*index)
                .is_some_and(|existing| existing.kind == KIND_SUBAGENT && existing.id == id)
        }) {
            self.patch(index, |existing| {
                existing.text = row.text.clone();
                existing.summary = row.summary.clone();
                existing.status_kind = row.status_kind;
            });
            return;
        }
        let index = self.push(row);
        self.patch(index, |existing| existing.payload = index as i32);
        self.refresh_process_bar();
    }

    /// Close the trailing segment before the turn settles.
    pub fn close_segments(&mut self) {
        // A round boundary proves the preceding print was intermediate. Move
        // it before freezing so its position never jumps after the next tool.
        // The previous round's protected answer marker expires at this
        // boundary; a later reasoning/tool event is now evidence that the
        // older body was an intermediate print. `start_body` installs the
        // marker for the new round after this method returns.
        self.protected_body = usize::MAX;
        self.include_active_body_in_process(false);
        self.close_thinking(false);
        self.close_body();
        self.close_batch();
        // The next body belongs to the answer tail opened by the boundary.
        // Keep this marker until `start_body` consumes it; a tool call can
        // still override the marker through `include_active_body_in_process`.
        self.protect_next_body = true;
    }

    /// Settle the turn: the last block freezes and the tail stops spinning.
    pub fn finish(&mut self, settle: Settle) {
        let failed = settle == Settle::Failed;
        self.close_thinking(failed);
        self.close_body();
        self.close_batch();
        self.interrupt_running();
        self.settled = true;
        if let Some(footer) = self.ensure_footer() {
            let text = self
                .model
                .row_data(self.last_body)
                .map(|row| row.text)
                .unwrap_or_default();
            self.patch(footer, |row| row.text = text);
        }
        self.refresh_process_bar();
        self.patch(self.process_bar, |row| {
            row.status_kind = match settle {
                Settle::Done => STATUS_DONE,
                Settle::Failed => STATUS_FAILED,
                Settle::Stopped => STATUS_STOPPED,
            }
        });
        if failed {
            self.patch(self.process_bar, |row| row.summary = "错误".into());
        }
    }

    /// Put the turn's failure in the assistant's place. The runtime reports at
    /// most one terminal failure per turn and a retry updates the same reason,
    /// so the card is inserted once (before the footer) and patched afterwards.
    pub fn upsert_error(&mut self, card: &crate::error_card::ErrorCard) {
        let Some(root) = self.turn.clone() else {
            return;
        };
        let id = format!("error-{root}");
        let existing = (0..self.model.row_count()).find(|index| {
            self.model
                .row_data(*index)
                .is_some_and(|row| row.kind == KIND_ERROR && row.id == id)
        });
        let index = match existing {
            Some(index) => index,
            None => {
                let mut row = blank(KIND_ERROR, id);
                row.status_kind = STATUS_FAILED;
                self.push(row)
            }
        };
        self.patch(index, |row| {
            row.error_title = card.title.as_str().into();
            row.text = card.message.as_str().into();
            row.detail = card.detail.as_str().into();
            row.error_hint = card.hint.as_str().into();
            row.visible = true;
            row.status_kind = STATUS_FAILED;
        });
    }

    /// A settled turn must never leave an unreported entry spinning: every row
    /// that was still running settles as interrupted, never as successful. A child
    /// run keeps the independent status that the runtime reports for it.
    fn interrupt_running(&mut self) {
        for index in self.process_bar.saturating_add(1)..self.model.row_count() {
            let Some(row) = self.model.row_data(index) else {
                continue;
            };
            if row.status_kind != STATUS_RUNNING || row.kind == KIND_SUBAGENT {
                continue;
            }
            let mut row = row;
            row.status_kind = STATUS_FAILED;
            if row.kind == KIND_REASON {
                row.tool_name = crate::timeline_text::thought_failed().into();
            }
            crate::timeline_text::sync_process_child(&self.model, &row);
            self.model.set_row_data(index, row);
        }
    }

    /// Freeze the active body block: parse its markdown once and stop touching
    /// it, so completed blocks stay byte-stable while the next one streams.
    fn close_body(&mut self) {
        let Some(body) = self.body.take() else {
            return;
        };
        let mut blocks = body.blocks;
        blocks.finalize_markdown();
        if let Some(desktop) = self.markdown_desktop.clone() {
            crate::native_markdown::hydrate(&mut blocks, desktop, self.markdown_root.clone());
        }
        // The whole answer is projected here, once: streaming published plain
        // text, and this is the only Markdown parse the message ever gets.
        let model = ModelRc::from(blocks.model.clone());
        let text = body.text.clone();
        let index = body.index;
        self.patch(index, |row| {
            row.text = text.as_str().into();
            row.foldable = true;
            row.blocks = model;
        });
    }

    /// One framed flush: only the active tail block is re-projected.
    pub fn flush(&mut self) {
        self.flush_body();
        self.flush_thinking();
    }

    /// Publish reasoning once per UI frame, including the expanded body. A
    /// capped preview cannot be used as the change clock for the full text.
    fn flush_thinking(&mut self) {
        let Some(thinking) = self.thinking.as_mut() else {
            return;
        };
        if !thinking.pending {
            return;
        }
        thinking.pending = false;
        thinking.summary = crate::message_blocks::reasoning_preview(&thinking.text);
        let index = thinking.index;
        let summary = thinking.summary.clone();
        let mut start = thinking.text.len().saturating_sub(32768);
        while !thinking.text.is_char_boundary(start) {
            start += 1;
        }
        let detail: slint::SharedString = thinking.text[start..].into();
        self.patch(index, |row| {
            row.summary = summary;
            row.detail = detail;
        });
    }

    /// Publish the bounded live view of the streaming block. Only the active
    /// tail block is touched; frozen blocks are never re-flushed.
    fn flush_body(&mut self) {
        let (index, text) = {
            let Some(body) = self.body.as_mut() else {
                return;
            };
            if !body.pending {
                return;
            }
            body.blocks.flush();
            body.pending = false;
            (body.index, body.text.clone())
        };
        self.patch(index, |row| {
            if row.text != text.as_str() {
                row.text = text.as_str().into();
            }
        });
        // Keep an already-admitted process child in sync with the same frame
        // as the body text. Admission itself is performed by the structural
        // event (reasoning/tool/round boundary), never by a body-only flush;
        // this preserves the newest answer outside the fold.
        self.refresh_process_bar();
        // The newest tick previews the reply while it streams; a structural
        // change above rebuilds every mark, so this only patches the tail.
        self.update_tail_mark(&text);
    }

    /// Replace the whole model with durable history rows. Only for a timeline
    /// that holds no live turn: tests, probes and the thread-log column.
    pub fn set_history(&mut self, mut rows: Vec<TimelineRow>) {
        crate::timeline_text::process_models(&mut rows);
        self.marks_dirty.set(true);
        self.active_user_row = usize::MAX;
        self.model.set_vec(rows);
        self.sync_fold_ids();
        self.sync_turn_marks();
    }

    /// Publish the durable history in front of the live turn. The two share one
    /// model, so a reload may only replace what the snapshot actually covers:
    /// rows from the live turn's own bubble onward stay, and stop staying the
    /// moment the snapshot carries that bubble. Replacing the whole model
    /// instead is what made a finished answer drop its thinking and tool
    /// entries before the durable rows arrived for them.
    pub fn publish_history(&mut self, mut rows: Vec<TimelineRow>) {
        let anchor = self
            .turn
            .as_ref()
            .map(|root| format!("user-turn-{root}"))
            .unwrap_or_default();
        let count = self.model.row_count();
        let start = (0..count).find(|index| {
            self.model
                .row_data(*index)
                .is_some_and(|row| row.kind == KIND_USER && row.id == anchor)
        });
        let Some(start) = start else {
            // Either there is no live turn, or the model no longer holds its
            // rows: the snapshot owns everything either way.
            self.forget_turn();
            self.replace_rows(rows);
            self.sync_fold_ids();
            return;
        };
        if anchor.is_empty()
            || rows.iter().any(|row| row.id == anchor)
            || (self.settled && self.snapshot_moved_past(rows.as_slice(), start))
        {
            // The snapshot carries this turn's own rows: the live builder is
            // done with it and its copy goes with them. The bubble's locally
            // echoed attachment pixels move to their durable cards first, so
            // the handoff never flashes the unloaded label.
            self.carry_user_image_pixels(&mut rows, start);
            self.forget_turn();
            self.replace_rows(rows);
            self.sync_fold_ids();
            return;
        }
        let mut published = rows;
        let delta = published.len() as isize - start as isize;
        for index in start..count {
            let Some(mut row) = self.model.row_data(index) else {
                continue;
            };
            // Fold handles address their row by index, so the tail keeps
            // toggling itself after the prefix moved. Body rows are foldable
            // too and their payload orders the process child list.
            if matches!(
                row.kind,
                KIND_GROUP | KIND_REASON | KIND_TOOL | KIND_PROCESS | KIND_BODY
            ) {
                row.payload = (index as isize + delta) as i32;
            }
            published.push(row);
        }
        self.replace_rows(published);
        self.rebase(delta);
        self.sync_fold_ids();
    }

    /// Preserve the VecModel handle after the window binds it. Replacing its
    /// backing vector in place emits one model notification while keeping the
    /// UI subscription stable across history refreshes and streaming updates.
    fn replace_rows(&self, mut rows: Vec<TimelineRow>) {
        crate::timeline_text::restore_folds(&mut rows, &self.model);
        self.marks_dirty.set(true);
        let same_topology = self.model.row_count() == rows.len()
            && self
                .model
                .iter()
                .map(|row| row.id)
                .eq(rows.iter().map(|row| row.id.clone()));
        if same_topology {
            // Durable polling often changes one tail row only. Replacing the
            // whole VecModel briefly gives ScrollView a zero-height column on
            // older Slint/Windows backends; it clamps to the top, then the
            // next layout pass scrolls back to the answer. Patch the stable
            // rows instead so neither the scroll anchor nor completed cells
            // are torn down during a passive refresh.
            for (index, mut row) in rows.into_iter().enumerate() {
                let Some(old) = self.model.row_data(index) else {
                    continue;
                };
                if old.text == row.text && old.status_kind == row.status_kind {
                    row.blocks = old.blocks.clone();
                }
                if old.detail == row.detail {
                    row.sections = old.sections.clone();
                    row.patch = old.patch.clone();
                }
                if old.stats.iter().eq(row.stats.iter()) {
                    row.stats = old.stats.clone();
                }
                if old == row {
                    continue;
                }
                self.model.set_row_data(index, row);
            }
        } else {
            // A full swap would otherwise rebuild every row's block model with
            // empty images and reparse styling. Rows whose identity and text
            // survived keep their decoded blocks, so a topology change never
            // blinks a picture back to its label.
            let previous: std::collections::HashMap<
                (slint::SharedString, slint::SharedString),
                ModelRc<MdBlock>,
            > = self
                .model
                .iter()
                .filter(|row| row.blocks.row_count() > 0)
                .map(|row| ((row.id.clone(), row.text.clone()), row.blocks.clone()))
                .collect();
            if !previous.is_empty() {
                for row in rows.iter_mut() {
                    if row.blocks.row_count() == 0 {
                        continue;
                    }
                    if let Some(blocks) = previous.get(&(row.id.clone(), row.text.clone())) {
                        row.blocks = blocks.clone();
                    }
                }
            }
            self.model.set_vec(rows);
        }
        self.sync_turn_marks();
    }

    /// Move the live bubble's already decoded attachment pixels onto the
    /// durable user row that replaces it. The local echo loads the composer's
    /// data URLs, so without this transfer the handoff blanked every card
    /// until the stored paths decoded. The durable twin is the user row with
    /// a fresh id and the same text; older bubbles keep their ids across
    /// every refresh, which rules out pairing with a repeated message.
    fn carry_user_image_pixels(&self, rows: &mut [TimelineRow], start: usize) {
        let Some(old) = self
            .model
            .row_data(start)
            .filter(|row| row.kind == KIND_USER)
        else {
            return;
        };
        let pixels: Vec<slint::Image> = old
            .blocks
            .iter()
            .filter(|block| block.kind == crate::message_blocks::KIND_IMAGE)
            .map(|block| block.image)
            .collect();
        if pixels.is_empty() || pixels.iter().all(|image| image.size().width == 0) {
            return;
        }
        let Some(target) = rows.iter().rposition(|row| {
            row.kind == KIND_USER
                && row.text == old.text
                && !self.model.iter().any(|held| held.id == row.id)
        }) else {
            return;
        };
        let blocks = rows[target].blocks.clone();
        let mut image_index = 0usize;
        for index in 0..blocks.row_count() {
            let Some(mut block) = blocks.row_data(index) else {
                continue;
            };
            if block.kind != crate::message_blocks::KIND_IMAGE {
                continue;
            }
            if let Some(image) = pixels.get(image_index) {
                if image.size().width > 0 {
                    block.image = image.clone();
                }
            }
            image_index += 1;
            blocks.set_row_data(index, block);
        }
    }

    /// Whether the snapshot's tail is no longer the durable tail the model
    /// already shows, i.e. it has taken the settled turn over. The live turn is
    /// keyed by session and a durable turn by its root message, so the two ids
    /// never meet and ownership has to be read off the turn tail. A bounded
    /// turn list also drops its oldest turns, which moves the count without
    /// moving the newest bubble, so both are compared.
    fn snapshot_moved_past(&self, rows: &[TimelineRow], start: usize) -> bool {
        let mut held = (0usize, slint::SharedString::default());
        for index in 0..start {
            if let Some(row) = self.model.row_data(index) {
                if row.kind == KIND_USER {
                    held.0 += 1;
                    held.1 = row.id.clone();
                }
            }
        }
        let mut next = (0usize, slint::SharedString::default());
        for row in rows {
            if row.kind == KIND_USER {
                next.0 += 1;
                next.1 = row.id.clone();
            }
        }
        held != next
    }

    /// Release the reducer's hold on a turn the snapshot has taken over.
    fn forget_turn(&mut self) {
        self.turn = None;
        self.active_user_row = usize::MAX;
        self.settled = false;
        self.body = None;
        self.thinking = None;
        self.tools.clear();
        self.bars.clear();
        self.discarded_steps = 0;
        self.group = NO_GROUP;
        self.process = NO_PROCESS;
        self.last_body = usize::MAX;
        self.protected_body = usize::MAX;
        self.protect_next_body = false;
        self.footer.set(usize::MAX);
    }

    /// Keep the reducer's fold ids ahead of whatever the snapshot published:
    /// a toggle folds by id across the whole model, so a live batch or process
    /// may never share one with a durable turn's.
    fn sync_fold_ids(&mut self) {
        let mut group = NO_GROUP;
        let mut process = NO_PROCESS;
        for row in self.model.iter() {
            group = group.max(row.group_idx);
            process = process.max(row.process_idx);
        }
        self.next_group = group + 1;
        self.next_process = process + 1;
    }

    /// Move every row index the reducer still holds by the amount the durable
    /// prefix grew or shrank.
    fn rebase(&mut self, delta: isize) {
        if delta == 0 {
            return;
        }
        let shift = |index: usize| (index as isize + delta).max(0) as usize;
        if self.active_user_row != usize::MAX {
            self.active_user_row = shift(self.active_user_row);
        }
        if let Some(body) = self.body.as_mut() {
            body.index = shift(body.index);
        }
        if let Some(thinking) = self.thinking.as_mut() {
            thinking.index = shift(thinking.index);
        }
        for tool in self.tools.iter_mut() {
            tool.index = shift(tool.index);
        }
        for bar in self.bars.iter_mut() {
            bar.index = shift(bar.index);
        }
        self.process_bar = shift(self.process_bar);
        if self.footer.get() != usize::MAX {
            self.footer.set(shift(self.footer.get()));
        }
        if self.last_body != usize::MAX {
            self.last_body = shift(self.last_body);
        }
        if self.protected_body != usize::MAX {
            self.protected_body = shift(self.protected_body);
        }
    }

    /// Fold toggle: `payload` is the row the user clicked. Reasoning and tool
    /// entries toggle themselves, and a batch bar toggles its batch. Fold
    /// state is never re-derived from the reducer, so a click costs one
    /// bounded model scan.
    pub fn toggle(&self, payload: i32) {
        if payload < 0 {
            crate::chat_process::toggle_entry(&self.model, payload);
            return;
        }
        let Ok(payload) = usize::try_from(payload) else {
            return;
        };
        let Some(row) = self.model.row_data(payload) else {
            return;
        };
        match row.kind {
            KIND_PROCESS => {
                let open = !row.open;
                let process = row.process_idx;
                for index in 0..self.model.row_count() {
                    let Some(mut target) = self.model.row_data(index) else {
                        continue;
                    };
                    if target.process_idx != process {
                        continue;
                    }
                    if target.kind == KIND_PROCESS {
                        target.open = open;
                    }
                    target.process_open = open;
                    // Opening the process is the user asking to see the work, so
                    // the batches inside it come with it. Their own state is left
                    // alone when folding: it is not on screen either way.
                    if open {
                        if target.kind == KIND_GROUP {
                            target.open = true;
                        }
                        target.group_open = true;
                    }
                    self.model.set_row_data(index, target);
                }
            }
            KIND_GROUP => {
                let open = !row.open;
                let group = row.group_idx;
                for index in 0..self.model.row_count() {
                    let Some(mut target) = self.model.row_data(index) else {
                        continue;
                    };
                    if target.group_idx != group {
                        continue;
                    }
                    if target.kind == KIND_GROUP {
                        target.open = open;
                    }
                    target.group_open = open;
                    self.model.set_row_data(index, target);
                }
            }
            _ => {
                let mut row = row;
                row.open = !row.open;
                crate::timeline_text::sync_process_child(&self.model, &row);
                self.model.set_row_data(payload, row);
            }
        }
    }

    /// Update the independent footer without invalidating body/process layout.
    pub fn set_turn_stats(&self, stats: Vec<crate::TurnStatMetric>) {
        let Some(index) = self.ensure_footer() else {
            return;
        };
        let row = self.model.row_data(index).unwrap();
        let user_round = stats
            .iter()
            .find(|metric| metric.key == "hashtag")
            .and_then(|metric| metric.value.parse::<i32>().ok())
            .unwrap_or(0);
        if row.stats.iter().ne(stats.iter().cloned()) || row.user_round != user_round {
            self.patch(index, |row| {
                row.stats = ModelRc::new(VecModel::from(stats));
                row.user_round = user_round;
            });
        }
    }

    fn ensure_footer(&self) -> Option<usize> {
        let root = self.turn.as_ref()?;
        if self.footer.get() < self.model.row_count() {
            return Some(self.footer.get());
        }
        let index = self.model.row_count();
        self.model
            .push(blank(KIND_FOOTER, format!("footer-{root}")));
        self.footer.set(index);
        self.marks_dirty.set(true);
        Some(index)
    }

    /// Settle the waiting 问询面板 bubble the user just answered.
    ///
    /// The answer is admitted as a new turn, so the durable watcher cannot
    /// install the old turn's completed rows while that new turn owns the live
    /// tail: its snapshot would put the new turn's own bubble in front of the
    /// optimistic one. The old bubble must therefore be settled here, at the
    /// moment the answer leaves the composer, or it keeps claiming the thread
    /// waits for input through the whole next turn.
    ///
    /// Returns whether a waiting bubble was found. A failed submission is
    /// restored lazily by the durable projection, which still holds the
    /// unanswered turn.
    pub fn settle_answered_inquiry(&mut self) -> bool {
        let count = self.model.row_count();
        let Some(bar) = (0..count).rev().find(|index| {
            self.model.row_data(*index).is_some_and(|row| {
                row.kind == KIND_PROCESS
                    && matches!(
                        row.summary.as_str(),
                        "等待用户输入" | "等待回答" | "等待确认" | "waiting_input"
                    )
            })
        }) else {
            return false;
        };
        self.patch(bar, |row| {
            row.summary = "已完成".into();
            row.status_kind = STATUS_DONE;
        });
        // The same turn's footer still carries the waiting status metric. Drop
        // it exactly as the durable projection does for a completed turn, so
        // the settled bubble cannot contradict its own tail.
        for index in bar + 1..count {
            let Some(row) = self.model.row_data(index) else {
                continue;
            };
            if row.kind == KIND_USER {
                break;
            }
            if row.kind != KIND_FOOTER {
                continue;
            }
            let stats: Vec<crate::TurnStatMetric> = row
                .stats
                .iter()
                .filter(|metric| !metric.key.starts_with("status-"))
                .collect();
            self.patch(index, move |row| {
                row.stats = ModelRc::new(VecModel::from(stats));
            });
            break;
        }
        self.marks_dirty.set(true);
        true
    }

    pub fn set_turn_status(&mut self, state: &str, round: i64) {
        let Some(index) = self.ensure_footer() else {
            return;
        };
        let Some(row) = self.model.row_data(index) else {
            return;
        };
        let mut stats: Vec<_> = row
            .stats
            .iter()
            .filter(|metric| !metric.key.starts_with("status-") && metric.key != "hashtag")
            .collect();
        let has_running_subagent = (self.process_bar + 1..index).any(|at| {
            self.model.row_data(at).is_some_and(|entry| {
                entry.kind == KIND_SUBAGENT && entry.status_kind == STATUS_RUNNING
            })
        });
        let state = crate::turn_stats::resolve_status(state, has_running_subagent);
        let footer_status = footer_status_kind(state);
        let metric = crate::turn_stats::with_status(
            crate::turn_stats::with_user_round(Vec::new(), round),
            state,
        );
        let mut prefix = stat_metrics(&metric);
        prefix.append(&mut stats);
        if row.stats.iter().collect::<Vec<_>>() != prefix {
            self.set_turn_stats(prefix);
        }
        if row.status_kind != footer_status {
            self.patch(index, |row| row.status_kind = footer_status);
        }
        if state == "已停止" || state == "等待用户输入" || state == "等待确认" {
            self.patch(self.process_bar, |bar| bar.summary = state.into());
        }
    }

    /// A terminal frame can carry the turn identity without usage statistics.
    /// Keep any already received metrics while adding the authoritative round.
    pub fn set_user_round(&self, round: i64) {
        if round <= 0 {
            return;
        }
        if let Some(index) = self.ensure_footer() {
            let Some(row) = self.model.row_data(index) else {
                return;
            };
            let mut stats: Vec<_> = row
                .stats
                .iter()
                .filter(|metric| metric.key != "hashtag")
                .collect();
            stats.insert(
                0,
                crate::TurnStatMetric {
                    key: "hashtag".into(),
                    value: round.to_string().into(),
                },
            );
            self.set_turn_stats(stats);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footer_status_keeps_localized_live_phases_running() {
        for state in [
            "请求中",
            "模型响应中",
            "正在生成…",
            "正在思考…",
            "模型输出中",
            "正在执行工具…",
            "使用工具中",
            "上下文压缩中…",
            "模型请求正在排队 · 前方 2 个任务",
            "任务已排队…",
            "子智能体执行中",
            "等待用户输入",
        ] {
            assert_eq!(footer_status_kind(state), STATUS_RUNNING, "{state}");
        }
        for state in ["任务完成", "输出已结束", "completed", "done", "final", ""] {
            assert_eq!(footer_status_kind(state), STATUS_DONE, "{state}");
        }
        assert_eq!(footer_status_kind("执行失败"), STATUS_FAILED);
        assert_eq!(footer_status_kind("已停止"), STATUS_STOPPED);
    }

    #[test]
    fn streaming_turn_footer_never_reads_as_finished() {
        let mut timeline = Timeline::new();
        timeline.begin_turn("fixture-live", "Fixture input");
        let footer = |timeline: &Timeline| {
            timeline
                .model
                .iter()
                .find(|row| row.kind == KIND_FOOTER)
                .map(|row| row.status_kind)
                .expect("footer")
        };
        timeline.set_turn_status("请求中", 1);
        assert_eq!(footer(&timeline), STATUS_RUNNING);
        timeline.set_turn_status("正在生成…", 1);
        assert_eq!(footer(&timeline), STATUS_RUNNING);
        timeline.set_turn_status("任务完成", 1);
        assert_eq!(footer(&timeline), STATUS_DONE);
    }

    #[test]
    fn answered_inquiry_settles_the_previous_waiting_bubble() {
        let mut timeline = Timeline::new();
        let mut user = blank(KIND_USER, "user-turn-ask".into());
        user.text = "Fixture question".into();
        let mut bar = blank(KIND_PROCESS, "process-ask".into());
        bar.summary = "等待用户输入".into();
        bar.status_kind = STATUS_RUNNING;
        bar.visible = true;
        let mut footer = blank(KIND_FOOTER, "footer-ask".into());
        footer.stats = ModelRc::new(VecModel::from(vec![
            crate::TurnStatMetric {
                key: "status-waiting".into(),
                value: "等待确认".into(),
            },
            crate::TurnStatMetric {
                key: "stopwatch".into(),
                value: "1.2s".into(),
            },
        ]));
        timeline.push(user);
        timeline.push(bar);
        timeline.push(footer);

        assert!(timeline.settle_answered_inquiry());
        let bar = timeline
            .model
            .iter()
            .find(|row| row.kind == KIND_PROCESS)
            .unwrap();
        assert_eq!(bar.summary, "已完成");
        assert_eq!(bar.status_kind, STATUS_DONE);
        let footer = timeline
            .model
            .iter()
            .find(|row| row.kind == KIND_FOOTER)
            .unwrap();
        assert_eq!(
            footer
                .stats
                .iter()
                .map(|metric| metric.key.to_string())
                .collect::<Vec<_>>(),
            vec!["stopwatch".to_string()],
            "the waiting status metric must not survive the answer"
        );
        assert!(!timeline.settle_answered_inquiry());
    }

    #[test]
    fn rejected_older_submission_cannot_remove_the_new_live_turn() {
        let mut timeline = Timeline::new();
        timeline.begin_turn("fixture-older", "Fixture input");
        timeline.begin_turn("fixture-newer", "Fixture next input");
        timeline.start_body(1);
        timeline.append_body("Fixture answer").unwrap();
        timeline.flush();
        let row_ids = timeline.model.iter().map(|row| row.id).collect::<Vec<_>>();

        assert!(!timeline.remove_live_turn("fixture-older"));
        assert_eq!(
            timeline.model.iter().map(|row| row.id).collect::<Vec<_>>(),
            row_ids,
            "a delayed failure leaves the newer turn intact"
        );
        timeline.append_body(" continued").unwrap();
        timeline.flush();
        assert_eq!(timeline.last_answer(), "Fixture answer continued");
        assert!(timeline.remove_live_turn("fixture-newer"));
        assert_eq!(timeline.model.row_count(), 2);
        assert_eq!(
            timeline.model.row_data(0).unwrap().id,
            "user-turn-fixture-older"
        );
    }

    #[test]
    fn repeated_submission_identity_reuses_the_user_anchor_and_process() {
        let mut timeline = Timeline::new();
        let mut anchor = blank(KIND_USER, "user-turn-fixture-repeat".into());
        anchor.text = "Fixture input".into();
        // The watcher may hydrate the persisted bubble before the process
        // row, while the submission worker is returning its subscriber.
        timeline.set_history(vec![anchor]);

        let first = timeline.begin_turn("fixture-repeat", "Fixture input");
        let second = timeline.begin_turn("fixture-repeat", "Fixture input");
        assert_eq!(first, 0);
        assert_eq!(second, first);
        assert_eq!(timeline.model.row_count(), 2);
        assert_eq!(
            timeline
                .model
                .iter()
                .filter(|row| row.id == "user-turn-fixture-repeat")
                .count(),
            1
        );
        assert_eq!(
            timeline
                .model
                .iter()
                .filter(|row| row.id == "process-fixture-repeat")
                .count(),
            1
        );
        timeline.start_body(1);
        timeline.append_body("Fixture answer").unwrap();
        timeline.flush();
        assert_eq!(timeline.last_answer(), "Fixture answer");
    }

    #[test]
    fn long_live_process_rolls_forward_without_losing_active_reasoning_or_answer() {
        let mut timeline = Timeline::new();
        timeline.begin_turn("fixture-long", "Fixture input");
        timeline.set_process_root("fixture-root");
        for index in 0..240 {
            timeline.start_body(index + 1);
            timeline.append_body(&format!("Note {index}")).unwrap();
            timeline
                .append_reasoning(&format!("Thought {index}"))
                .unwrap();
            timeline.close_thinking(false);
            timeline.upsert_tool(&NativeWorkflowEntry::from_payload(
                &format!("fixture-tool-{index}"),
                "read_file",
                "Fixture result".into(),
                "completed",
                &serde_json::json!({"args":{"path":"sample.txt"}}),
            ));
        }
        timeline.start_body(241);
        timeline.append_body("Final answer").unwrap();
        timeline.flush();
        assert!(timeline.row_count() <= MAX_LIVE_PROCESS_ROWS + 3);
        let row = timeline
            .model
            .iter()
            .find(|row| row.id == "tool-fixture-tool-239")
            .unwrap();
        timeline.toggle(row.payload);
        assert!(timeline.model.row_data(row.payload as usize).unwrap().open);
        assert_eq!(timeline.last_answer(), "Final answer");
        assert!(
            timeline
                .model
                .row_data(timeline.process_bar)
                .unwrap()
                .process_older
        );
        let children = timeline
            .model
            .row_data(timeline.process_bar)
            .unwrap()
            .process_entries;
        let ids: std::collections::HashSet<_> = children.iter().map(|child| child.id).collect();
        assert_eq!(ids.len(), children.row_count());
        assert!(children.iter().any(|child| child.detail == "Thought 239"));
    }

    #[test]
    fn live_footer_is_unique_and_outside_every_process_step() {
        let mut timeline = Timeline::new();
        timeline.begin_turn("fixture-footer", "Fixture input");
        timeline.set_turn_status("requesting", 1);
        assert_eq!(
            timeline.last_body,
            usize::MAX,
            "status must not create body placeholders"
        );
        timeline.append_reasoning("Fixture thought").unwrap();
        timeline.flush();
        timeline.start_body(1);
        timeline.append_body("Fixture intermediate note").unwrap();
        timeline.flush();
        timeline.upsert_tool(&NativeWorkflowEntry::from_payload(
            "fixture-footer-tool",
            "read_file",
            "Fixture result".into(),
            "running",
            &serde_json::json!({"args":{"path":"sample.txt"}}),
        ));
        timeline.set_turn_status("tool_running", 1);
        let body = timeline.model.row_data(timeline.last_body).unwrap();
        assert_ne!(body.process_idx, NO_PROCESS);
        timeline.toggle(timeline.process_bar as i32);
        let footer = timeline.model.row_data(timeline.footer.get()).unwrap();
        assert_eq!(
            timeline
                .model
                .iter()
                .filter(|row| row.kind == KIND_FOOTER)
                .count(),
            1
        );
        assert_eq!(timeline.footer.get(), timeline.row_count() - 1);
        assert_eq!(footer.process_idx, NO_PROCESS);
        assert_eq!(footer.group_idx, NO_GROUP);
        assert_eq!(footer.stats.row_data(0).unwrap().key, "status-tool");
        assert!(footer.visible);
    }

    #[test]
    fn prior_turn_child_activity_does_not_override_current_footer() {
        let mut timeline = Timeline::new();
        timeline.begin_turn("fixture-child", "Fixture input");
        let mut child = blank(KIND_SUBAGENT, "fixture-child-card".into());
        child.status_kind = STATUS_RUNNING;
        timeline.push(child);
        timeline.set_turn_status("requesting", 1);
        assert_eq!(
            timeline
                .model
                .row_data(timeline.footer.get())
                .unwrap()
                .stats
                .row_data(0)
                .unwrap()
                .key,
            "status-subagent"
        );
        timeline.set_turn_status("已停止", 1);
        assert_eq!(
            timeline
                .model
                .row_data(timeline.footer.get())
                .unwrap()
                .stats
                .row_data(0)
                .unwrap()
                .key,
            "status-stopped"
        );
        timeline.finish(Settle::Done);
        timeline.begin_turn("fixture-next", "Fixture next input");
        timeline.set_turn_status("requesting", 2);
        assert_eq!(
            timeline
                .model
                .row_data(timeline.footer.get())
                .unwrap()
                .stats
                .row_data(0)
                .unwrap()
                .key,
            "status-requesting"
        );
        timeline.publish_history(durable_rows(2));
        timeline.append_reasoning("Fixture next thought").unwrap();
        timeline.set_turn_status("streaming", 2);
        assert_eq!(timeline.footer.get(), timeline.row_count() - 1);
        assert_eq!(
            timeline
                .model
                .row_data(timeline.footer.get())
                .unwrap()
                .stats
                .row_data(0)
                .unwrap()
                .key,
            "status-outputting"
        );
    }

    #[test]
    fn state_footer_survives_streaming_stopping_and_durable_handoff() {
        let mut timeline = Timeline::new();
        timeline.begin_turn("fixture-state", "Fixture input");
        timeline.set_turn_status("正在思考…", 57);
        timeline.start_body(1);
        timeline.append_body("Fixture answer").unwrap();
        timeline.flush();
        assert_eq!(
            timeline
                .model
                .iter()
                .filter(|row| row.kind == KIND_BODY)
                .count(),
            1
        );
        timeline.set_turn_stats(stat_metrics(&crate::turn_stats::metrics(
            &serde_json::json!({"toolCalls":0}),
        )));
        timeline.finish(Settle::Failed);
        timeline.set_turn_status("已停止", 57);
        let row = timeline.model.row_data(timeline.footer.get()).unwrap();
        assert_eq!(row.stats.row_data(0).unwrap().key, "status-stopped");
        assert!(row
            .stats
            .iter()
            .any(|stat| stat.key == "screwdriver-wrench" && stat.value == "0"));
        assert_eq!(row.kind, KIND_FOOTER);
        assert_eq!(
            timeline
                .model
                .row_data(timeline.process_bar)
                .unwrap()
                .summary,
            "已停止"
        );
    }

    #[test]
    fn reasoning_frames_publish_live_body_and_phase_without_output_in_header() {
        let mut timeline = Timeline::new();
        timeline.begin_turn("fixture-live", "Fixture input");
        timeline.start_body(1);
        timeline.append_body("Fixture note").unwrap();
        timeline.append_reasoning("Fixture thought").unwrap();
        timeline.flush();
        let reason = timeline
            .model()
            .iter()
            .find(|row| row.kind == KIND_REASON)
            .unwrap();
        assert_eq!(reason.detail, "Fixture thought");
        assert_eq!(reason.status_kind, STATUS_RUNNING);
        timeline.start_body(2);
        timeline.append_reasoning(" next").unwrap();
        timeline.flush();
        let process = timeline
            .model()
            .iter()
            .find(|row| row.kind == KIND_PROCESS)
            .unwrap();
        assert!(process.visible);
        assert_eq!(process.summary, crate::timeline_text::thought_running());
        timeline.finish(Settle::Done);
        let process = timeline
            .model()
            .iter()
            .find(|row| row.kind == KIND_PROCESS)
            .unwrap();
        assert_eq!(process.summary, "已完成");
    }

    #[test]
    fn running_thought_is_projected_into_the_process_before_settlement() {
        let mut timeline = Timeline::new();
        timeline.begin_turn("fixture-live-thought", "Fixture input");
        timeline.append_reasoning("First live thought").unwrap();
        timeline.flush();

        let bar = timeline.model.row_data(timeline.process_bar).unwrap();
        assert!(bar.visible);
        assert_eq!(bar.status_kind, STATUS_RUNNING);
        assert_eq!(bar.process_entries.row_count(), 1);
        let children = bar.process_entries.clone();
        assert_eq!(children.row_data(0).unwrap().kind, KIND_REASON);
        assert_eq!(children.row_data(0).unwrap().detail, "First live thought");

        timeline.append_reasoning(" · continued").unwrap();
        timeline.flush();
        assert_eq!(
            timeline
                .model
                .row_data(timeline.process_bar)
                .unwrap()
                .process_entries,
            children
        );
        assert_eq!(
            children.row_data(0).unwrap().detail,
            "First live thought · continued"
        );
    }

    #[test]
    fn process_children_patch_in_place_and_keep_folds_on_history_refresh() {
        let mut timeline = Timeline::new();
        timeline.begin_turn("fixture-root", "Fixture input");
        timeline.append_reasoning("First thought").unwrap();
        timeline.flush();
        let bar = timeline.process_bar;
        // First reveal unfolds the bar by default and its `open` is the fold's
        // single source of truth, so the live model already carries the
        // expanded choice the refresh below must preserve.
        let reason = timeline.thinking.as_ref().unwrap().index;
        timeline.toggle(reason as i32);
        let children = timeline.model.row_data(bar).unwrap().process_entries;
        assert_eq!(children.row_count(), 1);
        timeline.append_reasoning(" extended").unwrap();
        timeline.flush();
        assert_eq!(
            children.row_data(0).unwrap().detail,
            "First thought extended"
        );
        assert!(children.row_data(0).unwrap().open);
        let mut snapshot: Vec<_> = timeline.model.iter().collect();
        snapshot[bar].open = false;
        snapshot[reason].open = false;
        snapshot[bar].process_entries = ModelRc::default();
        timeline.finish(Settle::Done);
        timeline.publish_history(snapshot);
        assert!(timeline.model.row_data(bar).unwrap().open);
        assert!(children.row_data(0).unwrap().open);
        // The list model survives the snapshot and still receives toggles.
        timeline.toggle(reason as i32);
        assert!(!children.row_data(0).unwrap().open);
    }

    #[test]
    fn process_keeps_intermediate_body_before_its_tool_and_settles_running_children() {
        let mut timeline = Timeline::new();
        timeline.begin_turn("fixture-root", "Fixture input");
        timeline.start_body(1);
        timeline.append_body("Fixture note").unwrap();
        timeline.flush();
        timeline.upsert_tool(&NativeWorkflowEntry::from_payload(
            "fixture-call",
            "read_file",
            "Fixture result".into(),
            "running",
            &serde_json::json!({"args":{"path":"sample.txt"}}),
        ));
        timeline.start_body(2);
        let children = timeline
            .model
            .row_data(timeline.process_bar)
            .unwrap()
            .process_entries;
        assert_eq!(
            children.iter().map(|entry| entry.kind).collect::<Vec<_>>(),
            vec![KIND_BODY, KIND_TOOL]
        );
        timeline.finish(Settle::Failed);
        assert_eq!(children.row_data(1).unwrap().status_kind, STATUS_FAILED);
    }

    #[test]
    fn empty_body_block_is_never_a_process_step() {
        let mut timeline = Timeline::new();
        timeline.begin_turn("fixture-empty", "Fixture input");
        timeline.append_reasoning("Fixture thought").unwrap();
        // A round that never prints can still leave its opened block behind;
        // the fold must skip it instead of showing a blank entry.
        timeline.start_body(1);
        timeline.upsert_tool(&NativeWorkflowEntry::from_payload(
            "fixture-empty-tool",
            "read_file",
            "Fixture result".into(),
            "completed",
            &serde_json::json!({"args":{"path":"sample.txt"}}),
        ));
        timeline.start_body(2);
        timeline.append_body("Fixture answer").unwrap();
        timeline.flush();
        timeline.finish(Settle::Done);
        let bar = timeline.model.row_data(timeline.process_bar).unwrap();
        assert!(
            bar.process_entries
                .iter()
                .all(|entry| entry.kind != KIND_BODY || !entry.text.is_empty()),
            "an empty block is not a fold entry"
        );
        assert_eq!(bar.text, crate::timeline_text::process_label(2));
    }

    #[test]
    fn live_print_moves_into_process_before_the_following_tool() {
        let mut timeline = Timeline::new();
        timeline.begin_turn("fixture-live-print", "Fixture input");
        timeline.start_body(1);
        timeline.append_body("Intermediate model print").unwrap();
        timeline.flush();

        timeline.upsert_tool(&NativeWorkflowEntry::from_payload(
            "fixture-call",
            "read_file",
            "Fixture result".into(),
            "running",
            &serde_json::json!({"args":{"path":"sample.txt"}}),
        ));

        let bar = timeline.model.row_data(timeline.process_bar).unwrap();
        assert!(bar.visible);
        assert_eq!(bar.process_entries.row_count(), 2);
        assert_eq!(bar.process_entries.row_data(0).unwrap().kind, KIND_BODY);
        assert_eq!(
            bar.process_entries.row_data(0).unwrap().text,
            "Intermediate model print"
        );
        assert_eq!(bar.process_entries.row_data(1).unwrap().kind, KIND_TOOL);
        let body = timeline.model.row_data(timeline.last_body).unwrap();
        assert_eq!(body.process_idx, bar.process_idx);

        timeline.finish(Settle::Done);
        let bar = timeline.model.row_data(timeline.process_bar).unwrap();
        assert_eq!(bar.process_entries.row_count(), 1);
        assert_eq!(bar.process_entries.row_data(0).unwrap().kind, KIND_TOOL);
        assert_eq!(
            timeline
                .model
                .row_data(timeline.last_body)
                .unwrap()
                .process_idx,
            NO_PROCESS
        );
    }

    #[test]
    fn streaming_intermediate_body_stays_in_the_process_on_every_frame() {
        let mut timeline = Timeline::new();
        timeline.begin_turn("fixture-streaming-print", "Fixture input");

        timeline.start_body(1);
        timeline.append_body("First streamed note").unwrap();
        timeline.flush();

        // Starting the following work step proves the existing model print is
        // intermediate. It must move before the next body token arrives.
        timeline.append_reasoning("Next action").unwrap();
        let bar = timeline.model.row_data(timeline.process_bar).unwrap();
        assert_eq!(bar.process_entries.row_count(), 2);
        assert_eq!(bar.process_entries.row_data(0).unwrap().kind, KIND_BODY);
        assert_eq!(
            bar.process_entries.row_data(0).unwrap().text,
            "First streamed note"
        );

        // Further deltas patch that same process child in place. This mirrors
        // the frame path that previously left all streaming text above the
        // fold until the terminal history projection arrived.
        timeline.append_body(" · continued").unwrap();
        timeline.flush();
        let bar = timeline.model.row_data(timeline.process_bar).unwrap();
        assert_eq!(bar.process_entries.row_count(), 2);
        assert_eq!(
            bar.process_entries.row_data(0).unwrap().text,
            "First streamed note · continued"
        );
        assert_eq!(
            timeline
                .model
                .row_data(timeline.last_body)
                .unwrap()
                .process_idx,
            bar.process_idx
        );
    }

    /// The process child list orders entries by payload, so an admitted body
    /// row must carry its own row index. The blank default 0 inserted every
    /// intermediate print at the top of the fold, ahead of the thinking and
    /// tool entries that preceded it, until settlement or the rolling trim
    /// restamped every payload.
    #[test]
    fn intermediate_bodies_keep_chronological_order_inside_the_process() {
        let mut timeline = Timeline::new();
        timeline.begin_turn("fixture-order", "Fixture input");
        timeline.append_reasoning("First thought").unwrap();
        timeline.flush();

        timeline.start_body(1);
        timeline.append_body("First intermediate note").unwrap();
        timeline.flush();
        timeline.upsert_tool(&NativeWorkflowEntry::from_payload(
            "fixture-call-1",
            "read_file",
            "Fixture result".into(),
            "running",
            &serde_json::json!({"args":{"path":"sample.txt"}}),
        ));

        timeline.close_segments();
        timeline.start_body(2);
        timeline.append_body("Second intermediate note").unwrap();
        timeline.flush();
        timeline.upsert_tool(&NativeWorkflowEntry::from_payload(
            "fixture-call-2",
            "read_file",
            "Fixture result".into(),
            "running",
            &serde_json::json!({"args":{"path":"sample.txt"}}),
        ));

        let children = timeline
            .model
            .row_data(timeline.process_bar)
            .unwrap()
            .process_entries;
        assert_eq!(
            children.iter().map(|entry| entry.kind).collect::<Vec<_>>(),
            vec![KIND_REASON, KIND_BODY, KIND_TOOL, KIND_BODY, KIND_TOOL],
            "admitted prints stay where the turn produced them, not at the top"
        );
        assert_eq!(
            children.row_data(1).unwrap().text,
            "First intermediate note"
        );
        assert_eq!(
            children.row_data(3).unwrap().text,
            "Second intermediate note"
        );

        // A child toggle addresses its own row through the stamped payload.
        let body_entry = children.row_data(1).unwrap();
        let body_index = timeline
            .model
            .iter()
            .position(|row| row.id == body_entry.id)
            .unwrap();
        assert_eq!(body_entry.payload, body_index as i32);
        timeline.toggle(body_entry.payload);
        assert!(timeline.model.row_data(body_index).unwrap().open);
    }

    /// The durable prefix growing under a live turn shifts every kept row; the
    /// body rows must be restamped together with the fold handles, or their
    /// stale payload again orders the process child list wrong.
    #[test]
    fn history_prefix_move_restamps_live_body_payload() {
        let mut timeline = Timeline::new();
        timeline.set_history(durable_rows(1));
        timeline.begin_turn("fixture-live", "Fixture input");
        timeline.append_reasoning("Fixture thought").unwrap();
        timeline.start_body(1);
        timeline.append_body("Intermediate note").unwrap();
        timeline.flush();
        timeline.upsert_tool(&NativeWorkflowEntry::from_payload(
            "fixture-call",
            "read_file",
            "Fixture result".into(),
            "running",
            &serde_json::json!({"args":{"path":"sample.txt"}}),
        ));

        timeline.publish_history(durable_rows(3));
        let model = timeline.model();
        for row in model.iter() {
            if row.foldable || row.kind == KIND_PROCESS {
                assert_eq!(
                    model.row_data(row.payload as usize).unwrap().id,
                    row.id,
                    "a foldable row must point at itself after the prefix moved"
                );
            }
        }
        drop(model);
        let children = timeline
            .model
            .row_data(timeline.process_bar)
            .unwrap()
            .process_entries;
        assert_eq!(
            children.iter().map(|entry| entry.kind).collect::<Vec<_>>(),
            vec![KIND_REASON, KIND_BODY, KIND_TOOL]
        );
    }

    #[test]
    fn terminal_round_identity_preserves_previously_received_metrics() {
        let mut timeline = Timeline::new();
        timeline.begin_turn("fixture-root", "Fixture input");
        timeline.start_body(1);
        timeline.set_turn_stats(stat_metrics(&crate::turn_stats::metrics(
            &serde_json::json!({"toolCalls":0}),
        )));
        timeline.set_user_round(57);
        timeline.set_user_round(57);
        let body = timeline.model.row_data(timeline.footer.get()).unwrap();
        assert_eq!(
            body.stats
                .iter()
                .filter(|metric| metric.key == "hashtag")
                .count(),
            1
        );
        assert_eq!(body.stats.row_data(0).unwrap().value, "57");
        assert_eq!(
            body.stats
                .iter()
                .find(|metric| metric.key == "screwdriver-wrench")
                .unwrap()
                .value,
            "0"
        );
    }

    #[test]
    fn live_reasoning_body_is_bounded_but_final_text_is_complete() {
        let mut timeline = Timeline::new();
        timeline.begin_turn("fixture-bound", "Fixture input");
        let text = "思考".repeat(8000);
        timeline.append_reasoning(&text).unwrap();
        timeline.append_reasoning(" fixture tail").unwrap();
        timeline.flush();
        let row = timeline
            .model()
            .iter()
            .find(|row| row.kind == KIND_REASON)
            .unwrap();
        assert!(row.detail.len() <= 32768);
        assert!(row.detail.ends_with("fixture tail"));
        timeline.finish(Settle::Done);
        let row = timeline
            .model()
            .iter()
            .find(|row| row.kind == KIND_REASON)
            .unwrap();
        assert_eq!(row.detail, format!("{text} fixture tail"));
    }

    /// One turn the way `project_history` lays it out: a user bubble, a batch
    /// bar, `entries` tool rows and an answer body. Every row of every turn
    /// stays visible — the web form — so this fixture also fixes the layout
    /// bound the view actually pays.
    fn history(turns: usize, entries: usize) -> Timeline {
        let mut rows = Vec::new();
        for turn in 0..turns {
            let group = turn as i32;
            let mut bar = blank(KIND_GROUP, format!("group-{turn}"));
            bar.text = "执行工具 24 次".into();
            bar.group_idx = group;
            bar.group_open = true;
            bar.open = true;
            bar.payload = rows.len() as i32;
            rows.push(bar);
            let user = blank(KIND_USER, format!("user-turn-root-{turn}"));
            rows.push(user);
            let mut body = blank(KIND_BODY, format!("body-{turn}"));
            body.text = "回答正文".into();
            rows.push(body);
            for entry in 0..entries {
                let mut row = blank(KIND_TOOL, format!("tool-{turn}-{entry}"));
                row.tool_name = "读取文件".into();
                row.foldable = true;
                row.group_idx = group;
                row.group_open = true;
                row.payload = rows.len() as i32;
                rows.push(row);
            }
        }
        let mut timeline = Timeline::new();
        timeline.set_history(rows);
        timeline
    }

    fn visible(timeline: &Timeline) -> usize {
        timeline.model().iter().filter(|row| row.visible).count()
    }

    /// Two rows per turn: the bubble carries the turn identity the observer
    /// matches a live turn against.
    fn durable_rows(turns: usize) -> Vec<TimelineRow> {
        (0..turns)
            .flat_map(|turn| {
                vec![
                    blank(KIND_USER, format!("user-turn-root-{turn}")),
                    blank(KIND_BODY, format!("body-{turn}")),
                ]
            })
            .collect()
    }

    /// The durable snapshot and the live turn share one model, so a reload may
    /// only replace the prefix it actually covers. Replacing the whole model is
    /// what made a finished answer lose its own entries before its durable rows
    /// arrived for them.
    #[test]
    fn publish_history_replaces_only_the_durable_prefix() {
        let mut timeline = Timeline::new();
        timeline.set_history(durable_rows(2));
        timeline.begin_turn("live", "Fixture input");
        timeline.start_body(1);
        timeline.append_body("直播中的答案").unwrap();
        timeline.flush();

        // More turns land in storage while this one is still live.
        timeline.publish_history(durable_rows(4));
        let model = timeline.model();
        assert_eq!(
            model
                .iter()
                .filter(|row| row.id == "user-turn-live")
                .count(),
            1,
            "the live turn keeps exactly one bubble"
        );
        assert_eq!(
            model.row_count(),
            11,
            "four durable turns plus the live turn's bubble, process bar and block"
        );
        assert_eq!(
            timeline.last_answer(),
            "直播中的答案",
            "the live block survives the reload"
        );
        let bubble = model
            .iter()
            .position(|row| row.id == "user-turn-live")
            .expect("live bubble");
        assert_eq!(
            model.row_data(bubble).expect("live bubble").text,
            "Fixture input",
            "the anchor bubble survived the prefix move"
        );
        drop(model);
        // The reducer's own indices moved with the prefix, so a later frame still
        // patches its own row instead of an older history row.
        timeline.append_body("续写").unwrap();
        timeline.flush();
        assert_eq!(timeline.last_answer(), "直播中的答案续写");

        // Once storage carries the live turn, its durable rows take over.
        let mut settled = durable_rows(4);
        settled.push(blank(KIND_USER, "user-turn-live".into()));
        let mut durable_answer = blank(KIND_BODY, "body-live".into());
        durable_answer.text = "durable-answer".into();
        settled.push(durable_answer);
        timeline.publish_history(settled);
        let model = timeline.model();
        assert_eq!(
            model
                .iter()
                .filter(|row| row.id == "user-turn-live")
                .count(),
            1,
            "the live copy is replaced, not appended"
        );
        assert_eq!(model.row_count(), 10);
        assert_eq!(
            model.row_data(9).expect("settled body").text,
            "durable-answer",
            "the durable rows are the authority once they arrive"
        );
    }

    #[test]
    fn the_window_model_handle_stays_stable_through_publish_and_clear() {
        let mut timeline = Timeline::new();
        let bound = timeline.model.clone();
        assert!(timeline.needs_publish(), "the owner binds the model once");
        assert!(
            !timeline.needs_publish(),
            "later frames mutate the bound model"
        );

        timeline.begin_turn("live", "Fixture input");
        timeline.append_reasoning("Fixture reasoning").unwrap();
        timeline.clear();

        assert!(Rc::ptr_eq(&bound, &timeline.model));
        assert_eq!(bound.row_count(), 0);
        assert!(
            !timeline.needs_publish(),
            "clear does not replace the binding"
        );
    }

    #[test]
    fn turn_marks_are_dirty_on_structure_changes_not_stream_patches() {
        let mut timeline = Timeline::new();
        timeline.begin_turn("session", "Fixture input");
        timeline.append_reasoning("Fixture thought").unwrap();
        timeline.sync_turn_marks();
        assert!(!timeline.marks_dirty.get());

        timeline.append_reasoning(" continued").unwrap();
        assert!(
            !timeline.marks_dirty.get(),
            "reasoning deltas patch a stable row and do not change turn positions"
        );

        timeline.close_thinking(false);
        timeline.start_body(1);
        timeline.sync_turn_marks();
        timeline.append_body("Fixture answer").unwrap();
        timeline.flush();
        assert_eq!(timeline.last_answer(), "Fixture answer");
        assert!(
            !timeline.marks_dirty.get(),
            "streaming answer text patches an existing row"
        );

        timeline.begin_turn("session-2", "Another user turn");
        assert!(timeline.marks_dirty.get(), "a new user row moves the marks");
        timeline.sync_turn_marks();
        assert!(!timeline.marks_dirty.get());
    }

    #[test]
    fn turn_marks_preview_the_user_message_and_newest_answer() {
        let mut timeline = Timeline::new();
        timeline.begin_turn("turn-a", "First question");
        timeline.start_body(1);
        timeline.append_body("Intermediate note").unwrap();
        timeline.flush();
        timeline.start_body(2);
        timeline.append_body("First answer").unwrap();
        timeline.flush();
        timeline.begin_turn("turn-b", "Second question");
        timeline.sync_turn_marks();

        let marks = timeline.turn_marks();
        assert_eq!(marks.len(), 2);
        assert_eq!(marks[0].user, "First question");
        assert_eq!(
            marks[0].agent, "First answer",
            "the preview reads the turn's newest answer, not its notes"
        );
        assert_eq!(marks[1].user, "Second question");
        assert_eq!(
            marks[1].agent, "",
            "a question still awaiting an answer previews only itself"
        );
    }

    #[test]
    fn the_newest_turn_preview_follows_the_streaming_answer() {
        let mut timeline = Timeline::new();
        timeline.begin_turn("live", "Fixture input");
        timeline.start_body(1);
        timeline.append_body("Partial").unwrap();
        timeline.flush();
        timeline.sync_turn_marks();
        assert_eq!(timeline.turn_marks()[0].agent, "Partial");

        timeline.append_body(" answer").unwrap();
        timeline.flush();
        let marks = timeline.marks_model();
        assert_eq!(marks.row_count(), 1);
        assert_eq!(
            marks.row_data(0).unwrap().agent,
            "Partial answer",
            "a streamed delta patches the newest tick instead of rebuilding the ruler"
        );
    }

    #[test]
    fn first_action_uses_one_stable_process_fold_and_later_thought_keeps_its_row() {
        let mut timeline = Timeline::new();
        timeline.begin_turn("fixture-root", "Fixture input");
        timeline.append_reasoning("First thought").unwrap();
        timeline.flush();
        let process = timeline.process_bar;
        let first = timeline.thinking.as_ref().unwrap().index;
        assert!(timeline.model.row_data(process).unwrap().visible);
        assert!(timeline
            .model
            .iter()
            .filter(|row| row.kind == KIND_GROUP)
            .all(|row| !row.visible));
        assert!(
            timeline.model.row_data(first).unwrap().process_open,
            "first reveal unfolds the fold and its rows mirror it"
        );
        timeline.toggle(process as i32);
        assert!(!timeline.model.row_data(first).unwrap().process_open);
        timeline.toggle(process as i32);
        assert!(timeline.model.row_data(first).unwrap().process_open);

        timeline.close_segments();
        timeline.start_body(1);
        timeline.append_body("Intermediate note").unwrap();
        timeline.close_segments();
        timeline.append_reasoning("Second thought").unwrap();
        timeline.flush();
        let second = timeline.thinking.as_ref().unwrap().index;
        assert_ne!(first, second);
        assert_eq!(
            timeline.model.row_data(first).unwrap().detail,
            "First thought"
        );
        assert_eq!(
            timeline.model.row_data(second).unwrap().detail,
            "Second thought"
        );
        assert!(timeline.model.row_data(second).unwrap().process_open);
        assert_eq!(
            timeline.model.row_data(process).unwrap().text,
            crate::timeline_text::process_label(2)
        );
    }

    #[test]
    fn history_prefix_move_rebases_process_handle_and_latest_body() {
        let mut timeline = Timeline::new();
        timeline.set_history(durable_rows(1));
        timeline.begin_turn("fixture-live", "Fixture input");
        timeline.append_reasoning("First thought").unwrap();
        timeline.start_body(1);
        timeline.append_body("First answer").unwrap();
        timeline.publish_history(durable_rows(3));
        let bar = timeline.process_bar;
        assert_eq!(timeline.model.row_data(bar).unwrap().kind, KIND_PROCESS);
        // The fold survives the rebase already unfolded; the toggle must reach
        // it through the rebased handle in both directions.
        assert!(timeline.model.row_data(bar).unwrap().open);
        timeline.toggle(bar as i32);
        assert!(!timeline.model.row_data(bar).unwrap().open);
        timeline.toggle(bar as i32);
        assert!(timeline.model.row_data(bar).unwrap().open);
        timeline.close_segments();
        timeline.start_body(2);
        timeline.append_body("Final answer").unwrap();
        timeline.append_reasoning("Later thought").unwrap();
        timeline.flush();
        let rows: Vec<_> = timeline.model.iter().collect();
        assert_eq!(rows[bar].text, crate::timeline_text::process_label(3));
        assert_eq!(rows[timeline.last_body].process_idx, NO_PROCESS);
        assert_eq!(rows[timeline.last_body].text, "Final answer");
        let note = rows.iter().find(|row| row.text == "First answer").unwrap();
        assert_eq!(note.process_idx, rows[bar].process_idx);
        let reason = rows
            .iter()
            .find(|row| row.detail == "Later thought")
            .unwrap();
        assert!(reason.process_open);
    }

    #[test]
    fn write_result_upgrades_its_call_row_with_the_change_size() {
        let mut timeline = Timeline::new();
        timeline.begin_turn("fixture-write-turn", "Fixture input");
        // The call frame carries no diff yet; only the result knows the change.
        timeline.upsert_tool(&NativeWorkflowEntry::from_payload(
            "fixture-write",
            "写入文件",
            String::new(),
            "running",
            &serde_json::json!({"args":{"path":"fixture/new.md","content":"a\nb\n"}}),
        ));
        timeline.upsert_tool(&NativeWorkflowEntry::from_payload(
            "fixture-write",
            "写入文件",
            String::new(),
            "completed",
            &serde_json::json!({
                "tool":"写入文件","ok":true,
                "args":{"path":"fixture/new.md","content":"a\nb\n"},
                "data":{"path":"fixture/new.md","added_lines":2,"deleted_lines":0,
                    "changed_files":1,"files":[{"path":"fixture/new.md","action":"add",
                        "added_lines":2,"deleted_lines":0,
                        "diff_blocks":[{"header":"new file","lines":[
                            {"kind":"add","new_line":1,"text":"a"},
                            {"kind":"add","new_line":2,"text":"b"}
                        ]}]
                    }]
                }
            }),
        ));
        let bar = timeline.model.row_data(timeline.process_bar).unwrap();
        let child = (0..bar.process_entries.row_count())
            .filter_map(|index| bar.process_entries.row_data(index))
            .find(|entry| entry.id == "tool-fixture-write")
            .expect("write child");
        assert_eq!(child.patch.row_count(), 1);
        assert_eq!(child.diff_add.as_str(), "2");
        assert!(child.diff_del.is_empty());
    }

    #[test]
    fn late_tool_result_updates_its_original_batch_only() {
        let mut timeline = Timeline::new();
        timeline.begin_turn("live", "Fixture input");
        let entry = |id: &str, state: &str, preview: &str| NativeWorkflowEntry {
            id: id.into(),
            title: "read_file".into(),
            preview: preview.into(),
            detail: String::new(),
            state: state.into(),
            tokens: String::new(),
            duration: String::new(),
            read_lines: String::new(),
            sections: Vec::new(),
            patch_files: Vec::new(),
        };

        timeline.upsert_tool(&entry("tool-a", "running", "first call"));
        timeline.close_segments();
        timeline.upsert_tool(&entry("tool-b", "running", "second call"));
        timeline.upsert_tool(&entry("tool-a", "completed", "first result"));

        let rows: Vec<_> = timeline.model().iter().collect();
        let tool_a = rows
            .iter()
            .find(|row| row.id == "tool-tool-a")
            .expect("tool A");
        let tool_b = rows
            .iter()
            .find(|row| row.id == "tool-tool-b")
            .expect("tool B");
        let groups: Vec<_> = rows.iter().filter(|row| row.kind == KIND_GROUP).collect();

        assert_eq!(groups.len(), 2, "the late result cannot open a third batch");
        assert_eq!(tool_a.group_idx, groups[0].group_idx);
        assert_eq!(tool_a.status_kind, STATUS_DONE);
        assert_eq!(groups[0].summary, "first result");
        assert_eq!(tool_b.group_idx, groups[1].group_idx);
        assert_eq!(tool_b.status_kind, STATUS_RUNNING);
        assert_eq!(groups[1].summary, "second call");
    }

    /// A command handed to the background keeps streaming after its call frame
    /// settled. Those trailing frames must not reopen the settled entry, and a
    /// command stream folded into its own call must not add a second entry.
    #[test]
    fn a_settled_tool_entry_never_returns_to_running() {
        let mut timeline = Timeline::new();
        timeline.begin_turn("live", "Fixture input");
        let mut entry = NativeWorkflowEntry {
            id: "call-1".into(),
            title: "执行命令".into(),
            preview: "check".into(),
            detail: "done".into(),
            state: "completed".into(),
            tokens: String::new(),
            duration: String::new(),
            read_lines: String::new(),
            sections: Vec::new(),
            patch_files: Vec::new(),
        };
        timeline.upsert_tool(&entry);
        entry.state = "running".into();
        entry.detail = "still running".into();
        timeline.upsert_tool(&entry);

        let rows: Vec<_> = timeline.model().iter().collect();
        assert_eq!(
            rows.iter().filter(|row| row.kind == KIND_TOOL).count(),
            1,
            "one invocation, one entry"
        );
        let row = rows
            .iter()
            .find(|row| row.id == "tool-call-1")
            .expect("tool row");
        assert_eq!(row.status_kind, STATUS_DONE);
        assert_eq!(row.detail.as_str(), "done");
    }

    /// The live turn is keyed by the session and a durable turn by its root
    /// message, so the handoff cannot wait for those two ids to meet: waiting is
    /// what left a finished answer on screen twice, once live and once durable.
    #[test]
    fn a_settled_turn_hands_over_to_durable_rows_under_another_id() {
        let mut timeline = Timeline::new();
        timeline.set_history(durable_rows(1));
        timeline.begin_turn("session-x", "Fixture input");
        timeline.start_body(1);
        timeline.append_body("答案").unwrap();
        timeline.flush();
        timeline.finish(Settle::Done);

        // Storage carries that very turn, under its own root id.
        let mut settled = durable_rows(1);
        settled.push(blank(KIND_USER, "user-turn-root-9".into()));
        settled.push(blank(KIND_BODY, "body-root-9".into()));
        timeline.publish_history(settled.clone());

        let model = timeline.model();
        assert_eq!(
            model
                .iter()
                .filter(|row| row.id == "user-turn-session-x")
                .count(),
            0,
            "the live copy is gone once storage owns the turn"
        );
        assert_eq!(
            model.iter().filter(|row| row.kind == KIND_BODY).count(),
            2,
            "one durable answer per turn, not an extra live bubble"
        );
        assert_eq!(model.row_count(), 4);
        drop(model);

        // The handoff is final: the next snapshot of the same shape cannot
        // append the turn a second time.
        timeline.publish_history(settled);
        assert_eq!(timeline.model().row_count(), 4);
    }

    /// The web-form history lays out every turn, so a click on a batch bar is
    /// the only fold left: it must toggle exactly its own entries and cost one
    /// bounded model scan.
    #[test]
    fn toggling_a_batch_bar_folds_only_its_own_entries() {
        let turns = 6;
        let entries = 4;
        let timeline = history(turns, entries);
        let total = timeline.row_count();
        assert_eq!(
            total,
            turns * (entries + 3),
            "bubble + bar + entries + body"
        );
        assert_eq!(
            visible(&timeline),
            total,
            "every history row stays laid out"
        );

        // The newest batch bar of turn 0 is the fourth row (bubble, bar, tools,
        // body per turn). Toggle it and only its group follows.
        let bar = timeline
            .model()
            .iter()
            .position(|row| row.kind == KIND_GROUP)
            .expect("a batch bar");
        let group = timeline.model().row_data(bar).unwrap().group_idx;
        timeline.toggle(bar as i32);
        let model = timeline.model();
        assert_eq!(
            model.iter().filter(|row| row.group_idx == group).count(),
            entries + 1,
            "the bar plus its own entries share one group"
        );
        assert!(
            model
                .iter()
                .filter(|row| row.group_idx == group && row.kind == KIND_TOOL)
                .all(|row| !row.group_open),
            "folding the bar hides its entries"
        );
        assert!(
            model
                .iter()
                .filter(|row| row.group_idx != group && row.kind == KIND_TOOL)
                .all(|row| row.group_open),
            "other batches stay open"
        );
        drop(model);
        timeline.toggle(bar as i32);
        assert!(
            timeline
                .model()
                .iter()
                .filter(|row| row.group_idx == group)
                .all(|row| row.group_open),
            "reopening restores the batch"
        );
    }

    #[test]
    fn durable_handoff_keeps_the_live_bubbles_decoded_attachment_pixels() {
        use slint::Model;
        let pixels = slint::Image::from_rgba8(slint::SharedPixelBuffer::clone_from_slice(
            &[255, 0, 0, 255],
            1,
            1,
        ));
        let mut timeline = Timeline::new();
        let row = timeline.begin_turn("fixture-client", "Fixture input");
        let echo = crate::message_blocks::with_image_cards(
            "Fixture input",
            &[("shot.png".into(), "data:image/png;base64,AAAA".into())],
        );
        // The composer echo decodes the data URL into the card; emulate the
        // loaded state the handoff must preserve.
        let held = echo.as_any().downcast_ref::<VecModel<MdBlock>>().unwrap();
        let mut card = held.row_data(held.row_count() - 1).unwrap();
        card.image = pixels.clone();
        held.set_row_data(held.row_count() - 1, card);
        timeline.set_user_blocks(row, echo);
        timeline.finish(Settle::Done);

        // The settled snapshot carries the same message under its durable id
        // and its attachment card waits on the stored path's decode.
        let mut durable_user = blank(KIND_USER, "user-turn-fixture-root".into());
        durable_user.text = "Fixture input".into();
        durable_user.blocks = crate::message_blocks::with_image_cards(
            "Fixture input",
            &[(
                "shot.png".into(),
                "workspaces/fixture/chat_attachments/shot.png".into(),
            )],
        );
        timeline.publish_history(vec![durable_user]);

        let model = timeline.model();
        let bubble = model
            .iter()
            .find(|row| row.id == "user-turn-fixture-root")
            .expect("durable bubble published");
        let card = bubble
            .blocks
            .iter()
            .find(|block| block.kind == crate::message_blocks::KIND_IMAGE)
            .expect("attachment card");
        assert_eq!(
            card.image.size().width,
            1,
            "the handoff keeps the decoded pixels instead of blanking the card"
        );
        assert_eq!(
            card.image_source, "workspaces/fixture/chat_attachments/shot.png",
            "the durable card keeps its own stored source"
        );
    }
}
