//! Flat, incremental chat timeline (§7).
//!
//! One bounded row per user bubble, assistant body block, reasoning entry, tool
//! entry, tool batch bar and completed-turn divider. Everything is projected
//! once on the Rust side: the Slint thread only lays out the cells it is handed
//! and reports each measured height back, so a streaming token never rebuilds
//! the history. Fold state lives in the model and is patched in place, which
//! keeps the reducer out of every user fold.
use crate::{TextBlock, TimelineRow};
#[allow(unused_imports)]
use slint::{Model, ModelRc, VecModel};
use std::rc::Rc;
use wunder_desktop::native::NativeWorkflowEntry;

/// Row kinds; keep in sync with `EntryKind` in ui/timeline.slint.
pub const KIND_USER: i32 = 0;
pub const KIND_BODY: i32 = 1;
pub const KIND_REASON: i32 = 2;
pub const KIND_TOOL: i32 = 3;
pub const KIND_GROUP: i32 = 4;
/// Status kinds: 0 done, 1 running, 2 failed.
pub const STATUS_DONE: i32 = 0;
pub const STATUS_RUNNING: i32 = 1;
pub const STATUS_FAILED: i32 = 2;
/// A row outside any tool batch.
pub const NO_GROUP: i32 = -1;
/// Entries and bounded fields kept per turn.
const MAX_GROUP_ENTRIES: usize = 24;
/// Batch bars the history projection leaves open by default, matching the
/// web messenger: only the newest turns read as expanded, older batches stay
/// folded bars the user can still open.
pub const MAX_OPEN_TURNS: usize = 8;

pub fn status_kind(state: &str) -> i32 {
    match state {
        "running" | "queued" | "pending" => STATUS_RUNNING,
        "failed" | "error" | "rejected" | "cancelled" | "canceled" => STATUS_FAILED,
        _ => STATUS_DONE,
    }
}

/// Runtime metrics to row metrics. One conversion keeps the live path and the
/// history projection producing identical rows.
pub fn stat_metrics(metrics: &[crate::turn_stats::Metric]) -> Vec<crate::TurnStatMetric> {
    metrics
        .iter()
        .map(|metric| crate::TurnStatMetric {
            key: metric.icon.into(),
            value: metric.value.as_str().into(),
        })
        .collect()
}

fn blank(kind: i32, id: String) -> TimelineRow {
    TimelineRow {
        kind,
        id: id.into(),
        visible: true,
        text: slint::SharedString::default(),
        blocks: ModelRc::default(),
        summary: slint::SharedString::default(),
        tool_name: slint::SharedString::default(),
        tool_icon: slint::SharedString::default(),
        target: slint::SharedString::default(),
        detail: slint::SharedString::default(),
        status_kind: STATUS_DONE,
        foldable: false,
        open: false,
        group_idx: NO_GROUP,
        group_open: false,
        payload: 0,
        patch: ModelRc::default(),
        stats: ModelRc::default(),
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
}

/// One tool invocation of the current batch.
struct ToolItem {
    id: String,
    index: usize,
    /// The `apply_patch`-family result renders as a diff card.
    patch: bool,
}
/// One batch bar and its bounded readout, kept while the turn lives so a late
/// result frame still updates the bar it belongs to.
struct BatchBar {
    index: usize,
    entries: usize,
    /// The newest entry of the batch, read out while the bar is folded.
    latest: slint::SharedString,
}

/// The live turn reducer.
pub struct Timeline {
    model: Rc<VecModel<TimelineRow>>,

    body: Option<Body>,
    thinking: Option<Thinking>,
    tools: Vec<ToolItem>,
    /// Known batch bars of this turn, newest last.
    bars: Vec<BatchBar>,
    group: i32,
    next_group: i32,
    reasons: usize,
    group_open: bool,
    turn: Option<String>,
    /// The live turn ran to its end but its rows are still the only copy: the
    /// durable snapshot has not caught up with it yet.
    settled: bool,
    /// Whether frozen body blocks get syntax highlighting; the reducer turns
    /// this on for the chat view and off elsewhere.
    highlight: bool,
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

            body: None,
            thinking: None,
            tools: Vec::new(),
            bars: Vec::new(),
            group: NO_GROUP,
            next_group: 0,
            reasons: 0,
            group_open: true,
            turn: None,
            settled: false,
            highlight: false,
        }
    }

    pub fn model(&self) -> ModelRc<TimelineRow> {
        ModelRc::from(self.model.clone())
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

    /// Row-index fractions of each user turn for the chat view's turn ruler.
    /// The ruler jumps proportionally, so one bounded pass per publish keeps
    /// it in sync without reporting per-row heights. Capped to the newest 100
    /// turns so the strip stays readable on marathon threads.
    pub fn turn_marks(&self) -> Vec<f32> {
        let total = self.model.row_count();
        if total <= 1 {
            return Vec::new();
        }
        let mut marks: Vec<f32> = self
            .model
            .iter()
            .enumerate()
            .filter(|(_, row)| row.kind == KIND_USER)
            .map(|(index, _)| index as f32 / total as f32)
            .collect();
        if marks.len() > 100 {
            marks.drain(..marks.len() - 100);
        }
        marks
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

        self.body = None;
        self.thinking = None;
        self.tools.clear();
        self.bars.clear();
        self.group = NO_GROUP;
        self.next_group = 0;
        self.reasons = 0;
        self.group_open = true;
        self.turn = None;
        self.settled = false;
        self.highlight = false;
    }

    /// Highlight the code blocks of frozen body blocks.
    pub fn set_highlight(&mut self, highlight: bool) {
        self.highlight = highlight;
    }

    fn push(&mut self, row: TimelineRow) -> usize {
        let index = self.model.row_count();
        self.model.push(row);
        index
    }

    fn patch(&self, index: usize, edit: impl FnOnce(&mut TimelineRow)) {
        let Some(mut row) = self.model.row_data(index) else {
            return;
        };
        edit(&mut row);
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
        self.group_open = true;
        self.reasons = 0;
        self.turn = Some(root.to_string());
        self.settled = false;
        let mut user = blank(KIND_USER, format!("user-turn-{root}"));
        user.text = text.into();
        user.blocks = crate::message_blocks::from_text(text);
        self.push(user)
    }

    /// Begin the next assistant body block; the runtime registers exactly one
    /// text item per model round, so a block starts where the last one ended.
    pub fn start_body(&mut self, round: i64) -> usize {
        self.close_thinking(false);
        self.close_body();
        let blocks = crate::message_blocks::Blocks::new();
        let mut row = blank(KIND_BODY, format!("body-{round}"));
        row.blocks = ModelRc::from(blocks.model.clone());
        let index = self.push(row);
        self.body = Some(Body {
            text: String::new(),
            blocks,
            index,
            pending: false,
        });
        index
    }

    pub fn append_body(&mut self, delta: &str) -> Result<(), String> {
        let Some(body) = self.body.as_mut() else {
            return Ok(());
        };
        body.blocks.append(delta)?;
        body.text.push_str(delta);
        body.pending = true;
        Ok(())
    }

    pub fn replace_body(&mut self, text: &str) -> Result<(), String> {
        let Some(body) = self.body.as_mut() else {
            return Ok(());
        };
        body.blocks.replace(text)?;
        body.text = text.to_owned();
        body.pending = true;
        Ok(())
    }

    /// The raw text of the active block, for copy and durable replay.
    pub fn body_text(&self) -> String {
        self.body
            .as_ref()
            .map(|body| body.text.clone())
            .unwrap_or_default()
    }

    /// Append a reasoning delta, opening a thinking entry for this segment.
    pub fn append_reasoning(&mut self, delta: &str) -> Result<(), String> {
        if self.thinking.is_none() {
            if self.reasons >= MAX_GROUP_ENTRIES {
                return Ok(());
            }
            self.mark_batch();
            let index = self.push(blank(
                KIND_REASON,
                format!("reason-{}", self.model.row_count()),
            ));
            let group = self.group;
            self.patch(index, |row| {
                row.tool_name = crate::timeline_text::thought_running().into();
                row.status_kind = STATUS_RUNNING;
                row.payload = index as i32;
                row.group_idx = group;
                // Every batch starts folded open; the user owns the fold after.
                row.group_open = true;
                row.open = false;
                row.foldable = true;
            });
            self.reasons += 1;
            self.thinking = Some(Thinking {
                text: String::new(),
                index,
                summary: slint::SharedString::default(),
            });
        }
        let Some(thinking) = self.thinking.as_mut() else {
            return Ok(());
        };
        if thinking.text.len().saturating_add(delta.len()) > crate::message_blocks::MAX_TEXT_BYTES {
            return Err("思考内容超过显示容量".into());
        }
        thinking.text.push_str(delta);
        thinking.summary = crate::message_blocks::reasoning_preview(&thinking.text);
        let index = thinking.index;
        let summary = thinking.summary.clone();
        self.patch(index, |row| {
            row.summary = summary.clone();
        });
        if let Some(bar) = self.bars.last_mut() {
            bar.latest = summary;
        }
        Ok(())
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
        let open = self.group_open;
        self.patch(bar, |row| {
            row.open = open;
            row.payload = bar as i32;
            row.group_idx = group;
            row.group_open = open;
        });
        self.bars.push(BatchBar {
            index: bar,
            entries: 0,
            latest: Default::default(),
        });
        self.group = group;
    }

    /// Rewrite the bar label of the batch the last upsert landed in. The count
    /// is the batch's own entry count, not the open batch's.
    fn refresh_batch_label(&mut self) {
        let Some((bar, entries, latest)) =
            self.bars.last().map(|bar| (bar.index, bar.entries, bar.latest.clone()))
        else {
            return;
        };
        let label = crate::timeline_text::tool_calls(entries as i32);
        self.patch(bar, |row| {
            row.text = label.as_str().into();
            row.summary = latest;
        });
    }


    /// Append one tool entry, or update the entry of an already seen id.
    pub fn upsert_tool(&mut self, entry: &NativeWorkflowEntry) {
        let label = crate::timeline_text::tool_label(&entry.title);
        let pending = status_kind(&entry.state) == STATUS_RUNNING;
        let (target, patch) = crate::timeline_text::target_of(
            &entry.title,
            entry.detail.as_str(),
            &entry.sections,
        );
        if pending {
            self.close_thinking(false);
        }
        self.mark_batch();
        if let Some(tool) = self.tools.iter().find(|tool| tool.id == entry.id) {
            let index = tool.index;
            let patch = tool.patch;
            let summary = crate::timeline_text::summary(entry.detail.as_str());
            let copy = crate::timeline_text::copy_text(&entry.sections, entry.detail.as_str());
            let cards = if patch {
                crate::timeline_text::patch_cards(&entry.patch_files)
            } else {
                ModelRc::default()
            };
            let status = status_kind(&entry.state);
            let icon = crate::tool_icons::workflow_icon(&entry.title).to_string();
            self.patch(index, |row| {
                row.tool_name = label.as_str().into();
                row.tool_icon = icon.as_str().into();
                row.target = target.as_str().into();
                row.summary = summary.as_str().into();
                row.detail = copy.clone();
                row.status_kind = status;
                row.patch = cards.clone();
            });
            // A settling result rewrites what the folded bar reads out, even
            // though the batch count stayed the same.
            if let Some(bar) = self.bars.last_mut() {
                bar.latest = summary.as_str().into();
            }
            self.refresh_batch_label();
            return;
        }
        if self.tools.len() >= MAX_GROUP_ENTRIES {
            return;
        }
        let index = self.push(blank(KIND_TOOL, format!("tool-{}", entry.id)));
        let icon = crate::tool_icons::workflow_icon(&entry.title).to_string();
        let summary = crate::timeline_text::summary(entry.detail.as_str());
        let copy = crate::timeline_text::copy_text(&entry.sections, entry.detail.as_str());
        let cards = if patch {
            crate::timeline_text::patch_cards(&entry.patch_files)
        } else {
            ModelRc::default()
        };
        let status = status_kind(&entry.state);
        let group = self.group;
        self.patch(index, |row| {
            row.tool_name = label.as_str().into();
            row.tool_icon = icon.as_str().into();
            row.target = target.as_str().into();
            row.summary = summary.as_str().into();
            row.detail = copy;
            row.status_kind = status;
            row.patch = cards;
            row.payload = index as i32;
            row.group_idx = group;
            row.group_open = true;
            row.open = false;
            row.foldable = true;
        });
        self.tools.push(ToolItem {
            id: entry.id.clone(),
            index,
            patch,
        });
        // Count this entry into its batch and refresh the bar label. `mark_batch`
        // published the fresh bar, so the first entry of a batch only counts.
        if let Some(bar) = self.bars.last_mut() {
            bar.entries += 1;
            bar.latest = summary.as_str().into();
        }
        self.refresh_batch_label();
    }

    /// End a tool batch so the next reasoning or tool opens a fresh bar. The
    /// batch entries stay known, because a later result frame still updates the
    /// row it created.
    fn close_batch(&mut self) {
        if self.group != NO_GROUP {
            self.refresh_batch_label();
        }
        self.group = NO_GROUP;
    }

    /// Close the trailing segment before the turn settles.
    pub fn close_segments(&mut self) {
        self.close_thinking(false);
        self.close_body();
        self.close_batch();
    }

    /// Settle the turn: the last block freezes and the tail stops spinning.
    pub fn finish(&mut self, failed: bool) {
        self.close_thinking(failed);
        self.close_body();
        self.close_batch();
        if failed {
            self.fail_running();
        }
        self.settled = true;
    }

    /// A cancelled or failed turn must never leave an entry spinning: every row
    /// that was still running settles as failed.
    fn fail_running(&mut self) {
        for index in 0..self.model.row_count() {
            let Some(row) = self.model.row_data(index) else {
                continue;
            };
            if row.status_kind != STATUS_RUNNING {
                continue;
            }
            let mut row = row;
            row.status_kind = STATUS_FAILED;
            if row.kind == KIND_REASON {
                row.tool_name = crate::timeline_text::thought_failed().into();
            }
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
        let styled = self.highlight.then(|| highlight_code(&mut blocks));
        let text = body.text.clone();
        let index = body.index;
        self.patch(index, |row| {
            row.text = text.as_str().into();
            row.foldable = true;
            if let Some(styled) = styled.as_ref() {
                row.blocks = styled.clone();
            }
        });
    }

    /// One framed flush: only the active tail block is re-projected.
    pub fn flush(&mut self) {
        self.flush_body();
    }

    /// Publish the bounded live view of the streaming block. Only the active
    /// tail block is touched; frozen blocks are never re-flushed.
    fn flush_body(&mut self) {
        let Some(body) = self.body.as_mut() else {
            return;
        };
        if !body.pending {
            return;
        }
        body.blocks.flush();
        body.pending = false;
        let index = body.index;
        let text = body.text.clone();
        self.patch(index, |row| {
            if row.text != text.as_str() {
                row.text = text.as_str().into();
            }
        });
    }

    /// Replace the whole model with durable history rows. Only for a timeline
    /// that holds no live turn: tests, probes and the thread-log column.
    pub fn set_history(&mut self, rows: Vec<TimelineRow>) {
        self.model.set_vec(rows);
    }

    /// Publish the durable history in front of the live turn. The two share one
    /// model, so a reload may only replace what the snapshot actually covers:
    /// rows from the live turn's own bubble onward stay, and stop staying the
    /// moment the snapshot carries that bubble. Replacing the whole model
    /// instead is what made a finished answer drop its thinking and tool
    /// entries before the durable rows arrived for them.
    pub fn publish_history(&mut self, rows: Vec<TimelineRow>) {
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
            self.model.set_vec(rows);
            return;
        };
        if anchor.is_empty() || rows.iter().any(|row| row.id == anchor)
            || (self.settled && self.snapshot_moved_past(rows.as_slice(), start))
        {
            // The snapshot carries this turn's own rows: the live builder is
            // done with it and its copy goes with them.
            self.forget_turn();
            self.model.set_vec(rows);
            return;
        }
        let mut published = rows;
        let delta = published.len() as isize - start as isize;
        for index in start..count {
            let Some(mut row) = self.model.row_data(index) else {
                continue;
            };
            // Fold handles address their row by index, so the tail keeps
            // toggling itself after the prefix moved.
            if matches!(row.kind, KIND_GROUP | KIND_REASON | KIND_TOOL) {
                row.payload = (index as isize + delta) as i32;
            }
            published.push(row);
        }
        self.model.set_vec(published);
        self.rebase(delta);
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
        self.settled = false;
        self.body = None;
        self.thinking = None;
        self.tools.clear();
        self.bars.clear();
        self.group = NO_GROUP;
    }

    /// Move every row index the reducer still holds by the amount the durable
    /// prefix grew or shrank.
    fn rebase(&mut self, delta: isize) {
        if delta == 0 {
            return;
        }
        let shift = |index: usize| (index as isize + delta).max(0) as usize;
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
    }

    /// Fold toggle: `payload` is the row the user clicked. Reasoning and tool
    /// entries toggle themselves, and a batch bar toggles its batch. Fold
    /// state is never re-derived from the reducer, so a click costs one
    /// bounded model scan.
    pub fn toggle(&self, payload: i32) {
        let Ok(payload) = usize::try_from(payload) else {
            return;
        };
        let Some(row) = self.model.row_data(payload) else {
            return;
        };
        match row.kind {
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
                self.model.set_row_data(payload, row);
            }
        }
    }

    /// Attach the turn's statistics footer to the body block that ends it.
    ///
    /// The footer belongs to the turn, not to one text block, but it renders on
    /// the body row's action line. Patching the trailing body row keeps a single
    /// owner per turn, so a streamed turn and a reloaded one cannot both add one.
    pub fn set_turn_stats(&self, stats: Vec<crate::TurnStatMetric>) {
        for index in (0..self.model.row_count()).rev() {
            let Some(row) = self.model.row_data(index) else {
                return;
            };
            match row.kind {
                KIND_BODY => {
                    let stats = ModelRc::new(VecModel::from(stats));
                    self.patch(index, |row| row.stats = stats);
                    return;
                }
                // A user bubble marks the turn boundary; nothing to attach to.
                KIND_USER => return,
                _ => {}
            }
        }
    }
}

/// Apply the linear syntax lexer to every code block of a frozen body and hand
/// back the model the row publishes. It runs once per block, never per token.
fn highlight_code(blocks: &mut crate::message_blocks::Blocks) -> ModelRc<TextBlock> {
    for index in 0..blocks.model.row_count() {
        let Some(mut block) = blocks.model.row_data(index) else {
            continue;
        };
        if block.kind == 1 {
            block.styled = crate::code_highlight::highlight(&block.text, &block.language);
            blocks.model.set_row_data(index, block);
        }
    }
    ModelRc::from(blocks.model.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            model.iter().filter(|row| row.id == "user-turn-live").count(),
            1,
            "the live turn keeps exactly one bubble"
        );
        assert_eq!(model.row_count(), 10, "four durable turns plus the live turn");
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
            model.iter().filter(|row| row.id == "user-turn-live").count(),
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
        timeline.finish(false);

        // Storage carries that very turn, under its own root id.
        let mut settled = durable_rows(1);
        settled.push(blank(KIND_USER, "user-turn-root-9".into()));
        settled.push(blank(KIND_BODY, "body-root-9".into()));
        timeline.publish_history(settled.clone());

        let model = timeline.model();
        assert_eq!(
            model.iter().filter(|row| row.id == "user-turn-session-x").count(),
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
        assert_eq!(total, turns * (entries + 3), "bubble + bar + entries + body");
        assert_eq!(visible(&timeline), total, "every history row stays laid out");

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
}
