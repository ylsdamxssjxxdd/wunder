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
pub const KIND_DIVIDER: i32 = 5;
/// Status kinds: 0 done, 1 running, 2 failed.
pub const STATUS_DONE: i32 = 0;
pub const STATUS_RUNNING: i32 = 1;
pub const STATUS_FAILED: i32 = 2;
/// A row outside any tool batch.
pub const NO_GROUP: i32 = -1;
/// Entries and bounded fields kept per turn.
const MAX_GROUP_ENTRIES: usize = 24;
/// Completed turns the user may hold open at once. The view lays out every
/// visible row, so this is the real bound on a frame's layout work: 8 turns x
/// (divider + bubble + batch bar + 24 entries + body) stays under 250 rows.
const MAX_OPEN_TURNS: usize = 8;

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
/// One batch bar and its entry count, kept while the turn lives so a late
/// result frame still updates the bar it belongs to.
struct BatchBar {
    index: usize,
    entries: usize,
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
    /// frozen by `finish`, so this only resets the builder state. The divider
    /// stays hidden until a later turn folds this one behind it.
    pub fn begin_turn(&mut self, root: &str, text: &str) -> usize {
        self.body = None;
        self.thinking = None;
        self.tools.clear();
        self.bars.clear();
        self.group = NO_GROUP;
        self.group_open = true;
        self.reasons = 0;
        self.turn = Some(root.to_string());
        let index = self.push(blank(KIND_DIVIDER, format!("turn-{root}")));
        self.patch(index, |row| {
            row.visible = false;
            row.payload = index as i32;
        });
        let mut user = blank(KIND_USER, format!("user-{}", self.model.row_count()));
        user.text = text.into();
        user.blocks = crate::message_blocks::from_text(text);
        self.push(user);
        index
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
            row.summary = summary;
        });
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
        });
        self.group = group;
    }

    /// Rewrite the bar label of the batch the last upsert landed in. The count
    /// is the batch's own entry count, not the open batch's.
    fn refresh_batch_label(&mut self) {
        let Some((bar, entries)) = self.bars.last().map(|bar| (bar.index, bar.entries)) else {
            return;
        };
        let label = crate::timeline_text::tool_calls(entries as i32);
        self.patch(bar, |row| {
            row.text = label.as_str().into();
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

    /// Replace the durable history rows in the shared model. Called by the
    /// observer before a live turn appends, so the two never interleave.
    pub fn set_history(&mut self, rows: Vec<TimelineRow>) {
        self.model.set_vec(rows);
    }

    /// Fold toggle: `payload` is the row the user clicked. Reasoning and tool
    /// entries toggle themselves, a batch bar toggles its batch, and a turn
    /// divider reveals or hides its whole turn. Fold state is never re-derived
    /// from the reducer, so a click costs one bounded model scan.
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
            KIND_DIVIDER => {
                let open = !row.open;
                self.toggle_turn(payload, open);
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
                // A divider marks the turn boundary; nothing to attach to.
                KIND_DIVIDER => return,
                _ => {}
            }
        }
    }

    /// Reveal or hide a completed turn: every row up to the next divider. The
    /// limit is applied first, so the column never briefly holds one turn more
    /// than it may lay out.
    fn toggle_turn(&self, divider: usize, open: bool) {
        if open {
            self.bound_open_turns(divider);
        }
        self.set_turn_open(divider, open, !open);
    }

    /// Set one turn's own rows to `open` and its divider to `divider_visible`.
    /// A turn the user opened keeps its divider as the fold handle; a turn the
    /// reducer folded back gets the same state the projection gives a completed
    /// turn, so it costs that single divider row again.
    fn set_turn_open(&self, divider: usize, open: bool, divider_visible: bool) {
        let count = self.model.row_count();
        self.patch(divider, |row| {
            row.open = open;
            row.visible = divider_visible;
        });
        for cursor in divider + 1..count {
            let Some(row) = self.model.row_data(cursor) else {
                break;
            };
            if row.kind == KIND_DIVIDER {
                break;
            }
            let mut row = row;
            row.visible = open;
            row.group_open = false;
            row.open = false;
            self.model.set_row_data(cursor, row);
        }
    }

    /// The timeline lays every visible row out, so the number of open turns is
    /// what bounds a frame. Opening a turn past this limit folds the oldest open
    /// turn back behind its divider: the history stays readable turn by turn and
    /// a frame never has to lay out the whole transcript at once.
    fn bound_open_turns(&self, opening: usize) {
        let mut open: Vec<usize> = Vec::new();
        let count = self.model.row_count();
        for index in 0..count {
            let Some(row) = self.model.row_data(index) else {
                continue;
            };
            // Count on the fold state alone: a turn the cap already folded back
            // stays `open` while its divider is hidden, and counting visibility
            // here would fold the next turn on every pass.
            if row.kind == KIND_DIVIDER && row.open && index != opening {
                open.push(index);
            }
        }
        if open.len() < MAX_OPEN_TURNS {
            return;
        }
        // Oldest first, so the history recedes in reading order.
        for index in open.drain(..open.len() - MAX_OPEN_TURNS + 1) {
            self.set_turn_open(index, false, false);
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
    use std::time::Instant;

    /// One turn the way `project_history` lays it out: a divider, a user bubble,
    /// a batch bar, `entries` tool rows and an answer body. A completed turn is
    /// folded, so only its divider is visible; the newest turn is open and its
    /// divider stays hidden.
    fn history(turns: usize, entries: usize) -> Timeline {
        let mut rows = Vec::new();
        for turn in 0..turns {
            let current = turn + 1 == turns;
            let mut divider = blank(KIND_DIVIDER, format!("turn-{turn}"));
            divider.visible = !current;
            divider.foldable = true;
            divider.payload = rows.len() as i32;
            rows.push(divider);
            for (kind, label) in [
                (KIND_USER, ""),
                (KIND_GROUP, "执行工具 24 次"),
                (KIND_BODY, "回答正文"),
            ] {
                let mut row = blank(kind, format!("{kind}-{turn}-{}", rows.len()));
                row.text = label.into();
                row.visible = current;
                rows.push(row);
            }
            for entry in 0..entries {
                let mut row = blank(KIND_TOOL, format!("tool-{turn}-{entry}"));
                row.tool_name = "读取文件".into();
                row.visible = current;
                row.foldable = true;
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

    /// §十二.2: the view lays out every row the projection marks visible, so the
    /// reducer has to bound that count. Unfolding a whole history may not hand
    /// the view a four-digit row count.
    #[test]
    fn unfolding_the_whole_history_stays_inside_the_open_turn_limit() {
        let turns = 50;
        let entries = 24;
        let timeline = history(turns, entries);
        let total = timeline.row_count();
        assert_eq!(total, turns * (entries + 4), "divider + bubble + bar + entries + body");

        let dividers: Vec<i32> = timeline
            .model()
            .iter()
            .filter(|row| row.kind == KIND_DIVIDER && row.visible)
            .map(|row| row.payload)
            .collect();
        assert_eq!(dividers.len(), turns - 1, "every completed turn folds");

        let started = Instant::now();
        for divider in &dividers {
            timeline.toggle(*divider);
        }
        let unfolded = started.elapsed();

        let model = timeline.model();
        let open = model
            .iter()
            .filter(|row| row.kind == KIND_DIVIDER && row.open)
            .count();
        let exposed = model.iter().filter(|row| row.visible).count();
        // Folded-back turns keep their divider hidden, exactly like the turns the
        // projection folded in the first place, so they cost one row each again.
        let shown_dividers = model
            .iter()
            .filter(|row| row.kind == KIND_DIVIDER && row.visible)
            .count();
        println!(
            "timeline open-turn limit: turns={turns} entries={entries} rows={total} \
             open={open} exposed={exposed} fold_handles={shown_dividers} \
             unfold_all={unfolded:?}",
        );
        // The bound the view actually pays: the open turns plus the live one.
        assert_eq!(open, MAX_OPEN_TURNS, "the limit leaves exactly its turns open");
        assert_eq!(exposed, (MAX_OPEN_TURNS + 1) * (entries + 3));
        assert!(
            exposed <= 250,
            "the unfolded column must stay near 250 laid-out rows, got {exposed}",
        );
        assert!(visible(&timeline) == exposed);

        // The user keeps control of every open turn. Folding one gives its rows
        // back to the column and leaves only its divider behind; opening it again
        // restores exactly the bounded column.
        let newest = dividers[dividers.len() - 1];
        timeline.toggle(newest);
        assert_eq!(
            visible(&timeline) + entries + 2,
            exposed,
            "folding a turn back must drop the rows it owns",
        );
        timeline.toggle(newest);
        assert_eq!(
            visible(&timeline),
            exposed,
            "reopening a turn must restore the same bounded column",
        );
    }
}
