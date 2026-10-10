//! Composer queue: turns the engine parked for the active thread, plus the
//! ArrowUp/ArrowDown recall of messages this thread already sent.
//!
//! The engine owns the queue and already parks a turn submitted to a busy
//! thread; this module keeps one subscriber per parked submission, hands the
//! live slot to whichever turn actually started, and projects the queue back
//! into the strip above the input box.

use crate::{DraftRecall, MainWindow, QueueTurn};
use slint::{ModelRc, SharedString, VecModel};
use std::collections::VecDeque;
use std::rc::Rc;
use wunder_desktop::{NativeChatEvent, NativeDesktop, NativeQueueTurn, NativeStream};

/// Parked submissions this window still holds a subscriber for. A thread's
/// queue is short by design and every entry owns a bounded event channel, so
/// the list is capped instead of growing with a stuck feeder.
pub const PARKED_LIMIT: usize = 8;
/// Rows the strip renders before the rest folds into "+N 更多".
pub const STRIP_ROWS: usize = 5;
/// Events taken out of one parked stream per tick. A stream that speaks is
/// promoted at once, so this only bounds the quiet ones.
const PARKED_POLL_BUDGET: usize = 6;
/// Messages the recall walks back through, newest first.
pub const RECALL_DEPTH: usize = 40;

/// One submission the engine may park. `prelude` holds the events that arrived
/// before the entry could take the live slot; the pump replays them in order so
/// the handover never loses a token.
pub struct ParkedTurn {
    pub stream: NativeStream,
    pub session: String,
    pub content: String,
    /// The id this window submitted with; it is the only key that ties an
    /// engine queue row back to the subscriber that owns it.
    pub client_message_id: String,
    /// Events taken out of the stream before the entry could take the live
    /// slot; the pump replays them in order so the handover never loses a token.
    pub prelude: Vec<NativeChatEvent>,
}

impl ParkedTurn {
    pub fn new(
        stream: NativeStream,
        session: String,
        content: String,
        client_message_id: String,
    ) -> Self {
        Self {
            stream,
            session,
            content,
            client_message_id,
            prelude: Vec::new(),
        }
    }
}

/// Cursor of the history walk. `stash` is the draft the user had before the
/// walk started, restored when ArrowDown returns to the bottom.
#[derive(Default)]
pub struct Recall {
    pub index: usize,
    pub stash: Option<String>,
}

/// What one parked stream reported this tick.
#[derive(Debug, PartialEq, Eq)]
pub enum EntrySignal {
    /// Nothing has arrived yet.
    Quiet,
    /// The engine confirmed it parked the turn.
    Announced,
    /// The turn began producing output: it should take the live slot.
    Ready,
    /// The subscription ended without the turn ever running (withdrawn or
    /// cancelled elsewhere).
    Settled,
    /// The turn failed before it ran.
    Failed(String),
}

/// Drain one parked stream without promoting it.
pub fn drain_entry(entry: &mut ParkedTurn, session: &str) -> EntrySignal {
    if entry.session != session || !entry.prelude.is_empty() {
        // A non-empty prelude means the entry already waits for the live slot;
        // the pump owns it from here, not this scan.
        return EntrySignal::Quiet;
    }
    let mut announced = false;
    for _ in 0..PARKED_POLL_BUDGET {
        let event = match entry.stream.try_recv() {
            Ok(Some(event)) => event,
            Ok(None) => break,
            Err(_) => return EntrySignal::Settled,
        };
        match event {
            NativeChatEvent::Queued => announced = true,
            NativeChatEvent::Failed(message) => return EntrySignal::Failed(message),
            NativeChatEvent::Finished => return EntrySignal::Settled,
            other => {
                entry.prelude.push(other);
                return EntrySignal::Ready;
            }
        }
    }
    if announced {
        return EntrySignal::Announced;
    }
    EntrySignal::Quiet
}

/// Engine rows → strip rows, plus how many rows were left out.
pub fn project_rows(rows: &[NativeQueueTurn]) -> (Vec<QueueTurn>, usize) {
    let hidden = rows.len().saturating_sub(STRIP_ROWS);
    let projected = rows
        .iter()
        .take(STRIP_ROWS)
        .map(|row| QueueTurn {
            queue_id: SharedString::from(row.queue_id.as_str()),
            text: SharedString::from(row.content.as_str()),
            attachments: row.attachment_count as i32,
            interjected: row.priority > 0,
        })
        .collect::<Vec<_>>();
    (projected, hidden)
}

/// Put an already-read queue onto the strip.
pub fn publish_rows(app: &MainWindow, rows: &[NativeQueueTurn]) {
    let (projected, hidden) = project_rows(rows);
    app.set_queue_turns(ModelRc::from(Rc::new(VecModel::from(projected))));
    app.set_queue_overflow(hidden as i32);
}

/// Publish the engine's queue for `session`. This is a blocking call, so it
/// runs only on the boundaries that actually move the queue (send, action,
/// thread switch) and behind the tick's debounce.
pub fn publish(
    app: &MainWindow,
    desktop: &NativeDesktop,
    session: &str,
    cache: &mut Vec<NativeQueueTurn>,
) {
    let rows = if session.is_empty() {
        Vec::new()
    } else {
        desktop.list_queue_turns(session).unwrap_or_default()
    };
    *cache = rows;
    publish_rows(app, cache);
}

/// Ask the strip to refresh soon; `None` clears the pending request.
pub fn schedule_due(due: &mut Option<std::time::Instant>, delay: std::time::Duration) {
    let now = std::time::Instant::now();
    let target = now + delay;
    match due {
        Some(existing) if *existing <= target => {}
        _ => *due = Some(target),
    }
}

/// True when the debounce deadline has passed, consuming the request.
pub fn due_arrived(due: &mut Option<std::time::Instant>) -> bool {
    match due {
        Some(target) if *target <= std::time::Instant::now() => {
            *due = None;
            true
        }
        _ => false,
    }
}

pub fn parked_is_full(parked: &VecDeque<ParkedTurn>) -> bool {
    parked.len() >= PARKED_LIMIT
}

/// Move `queue_id` by `delta` inside the current order and hand back the whole
/// list, which is what the engine's reorder call wants. `None` means the drag
/// left the list (a row that no longer exists), `Some(empty)` means it was a
/// no-op and nothing should be written back.
pub fn move_within(ids: &[String], queue_id: &str, delta: i32) -> Option<Vec<String>> {
    let from = ids.iter().position(|id| id == queue_id)?;
    let target = i32::try_from(from).ok()? + delta;
    if target < 0 || target as usize >= ids.len() || target as usize == from {
        return Some(Vec::new());
    }
    let mut next = ids.to_vec();
    let moved = next.remove(from);
    next.insert(target as usize, moved);
    Some(next)
}

/// One step of the history walk. `direction` is +1 for ArrowUp, -1 for
/// ArrowDown; the caret offsets are UTF-8 byte offsets, which is why the
/// decision lives here rather than in the .slint key handler.
pub fn recall_step(
    state: &mut Recall,
    history: &[String],
    direction: i32,
    cursor: i32,
    anchor: i32,
    draft: &str,
) -> Option<DraftRecall> {
    let up = direction > 0;
    let selection = cursor != anchor;
    let at_start = cursor == 0 && anchor == 0;
    let at_end = !selection && cursor as usize == draft.len();
    if history.is_empty() || selection || (!draft.is_empty() && !(if up { at_start } else { at_end }))
    {
        return None;
    }
    // A draft that is not the row this walk just recalled means the user typed:
    // restart from the newest and keep the current text as the return target.
    let current = state.index.checked_sub(1).and_then(|i| history.get(i));
    if current != Some(&draft.to_string()) {
        state.index = 0;
        state.stash = (!draft.is_empty()).then(|| draft.to_string());
    }
    if up {
        if state.index >= history.len() {
            return None;
        }
        state.index += 1;
        return history.get(state.index - 1).map(|text| recalled_row(text));
    }
    if state.index == 0 {
        return None;
    }
    state.index -= 1;
    let text = match state.index {
        0 => state.stash.clone().unwrap_or_default(),
        index => history.get(index - 1)?.clone(),
    };
    Some(recalled_row(&text))
}

fn recalled_row(text: &str) -> DraftRecall {
    DraftRecall {
        handled: true,
        text: SharedString::from(text),
        caret: text.len() as i32,
    }
}

/// Forget the walk when the thread changes, so an ArrowUp in another thread
/// starts from that thread's newest message.
pub fn reset_recall(state: &mut Recall) {
    *state = Recall::default();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, text: &str, priority: i64) -> NativeQueueTurn {
        NativeQueueTurn {
            queue_id: id.into(),
            content: text.into(),
            attachment_count: if text.is_empty() { 1 } else { 0 },
            position: 0,
            priority,
            client_message_id: format!("send-{id}"),
        }
    }

    #[test]
    fn drag_reorder_writes_the_whole_order_back() {
        let ids = ["a", "b", "c"]
            .iter()
            .map(|id| (*id).to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            move_within(&ids, "c", -2),
            Some(vec!["c".to_string(), "a".to_string(), "b".to_string()])
        );
        assert_eq!(
            move_within(&ids, "a", 1),
            Some(vec!["b".to_string(), "a".to_string(), "c".to_string()])
        );
        // Off the ends and a zero-distance drop change nothing, so nothing is
        // written back to the engine.
        assert_eq!(move_within(&ids, "a", -1), Some(Vec::new()));
        assert_eq!(move_within(&ids, "c", 1), Some(Vec::new()));
        assert_eq!(move_within(&ids, "gone", 1), None);
    }

    #[test]
    fn projection_folds_the_tail_of_a_long_queue() {
        let rows = (0..9)
            .map(|index| row(&format!("task-{index}"), &format!("行 {index}"), 0))
            .collect::<Vec<_>>();
        let (projected, hidden) = project_rows(&rows);
        assert_eq!(projected.len(), STRIP_ROWS);
        assert_eq!(hidden, 9 - STRIP_ROWS);
        assert_eq!(projected[0].queue_id.as_str(), "task-0");
        assert_eq!(projected[0].text.as_str(), "行 0");
        assert!(!projected[0].interjected);
    }

    #[test]
    fn projection_marks_an_interjected_row() {
        let rows = vec![row("task-a", "甲", 1), row("task-b", "乙", 0)];
        let (projected, _) = project_rows(&rows);
        assert!(projected[0].interjected);
        assert!(!projected[1].interjected);
    }

    #[test]
    fn recall_walks_only_from_the_caret_edges() {
        let history = vec!["最新".to_string(), "更早".to_string()];
        let mut state = Recall::default();
        // A caret inside a draft must keep the arrow for the editor.
        assert!(recall_step(&mut state, &history, 1, 1, 1, "写到一半").is_none());
        // Empty draft: ArrowUp walks back, ArrowDown returns to the stash.
        let first = recall_step(&mut state, &history, 1, 0, 0, "").expect("newest");
        assert!(first.handled);
        assert_eq!(first.text.as_str(), "最新");
        assert_eq!(first.caret, "最新".len() as i32);
        let second = recall_step(&mut state, &history, 1, 0, 0, "最新").expect("older");
        assert_eq!(second.text.as_str(), "更早");
        assert!(recall_step(&mut state, &history, 1, 0, 0, "更早").is_none());
        let back = recall_step(&mut state, &history, -1, 0, 0, "更早").expect("forward");
        assert_eq!(back.text.as_str(), "最新");
        let bottom = recall_step(&mut state, &history, -1, 0, 0, "最新").expect("back to empty");
        assert_eq!(bottom.text.as_str(), "");
        assert!(recall_step(&mut state, &history, -1, 0, 0, "").is_none());
    }

    #[test]
    fn recall_restarts_after_the_user_edits_a_recalled_row() {
        let history = vec!["乙".to_string(), "甲".to_string()];
        let mut state = Recall::default();
        recall_step(&mut state, &history, 1, 0, 0, "").expect("first row");
        recall_step(&mut state, &history, 1, 0, 0, "乙").expect("second row");
        // The edited draft ends the walk: the next ArrowUp starts over from the
        // newest and keeps the edited text as the stash.
        let restarted = recall_step(&mut state, &history, 1, 0, 0, "乙 补充").expect("restart");
        assert_eq!(restarted.text.as_str(), "乙");
        assert_eq!(state.stash.as_deref(), Some("乙 补充"));
    }

    #[test]
    fn recall_keeps_a_selection_in_the_editor() {
        let history = vec!["甲".to_string()];
        let mut state = Recall::default();
        assert!(recall_step(&mut state, &history, 1, 0, 3, "甲乙").is_none());
    }
}
