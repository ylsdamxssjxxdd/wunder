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
use wunder_desktop::{NativeChatAttachment, NativeChatEvent, NativeQueueTurn, NativeStream};

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

/// Keep a promoted turn off the composer strip while the durable queue snapshot
/// catches up. The engine list remains authoritative after that window.
pub fn without_client_id(rows: &mut Vec<NativeQueueTurn>, client_id: &str) {
    if client_id.is_empty() {
        return;
    }
    rows.retain(|row| row.client_message_id != client_id);
}

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
    /// Composer image attachments kept for the bubble's local echo when the
    /// turn is promoted: (name, data URL) pairs.
    pub images: Vec<(String, String)>,
    /// Events taken out of the stream before the entry could take the live
    /// slot; the pump replays them in order so the handover never loses a token.
    pub prelude: Vec<NativeChatEvent>,
    /// This submission answers a waiting 问询面板. Its promotion is the moment
    /// the old waiting bubble can be settled.
    pub settles_inquiry: bool,
}

impl ParkedTurn {
    pub fn new(
        stream: NativeStream,
        session: String,
        content: String,
        client_message_id: String,
        images: Vec<(String, String)>,
        settles_inquiry: bool,
    ) -> Self {
        Self {
            stream,
            session,
            content,
            client_message_id,
            images,
            prelude: Vec::new(),
            settles_inquiry,
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
    if entry.session != session {
        return EntrySignal::Quiet;
    }
    poll_entry(&mut entry.prelude, || entry.stream.try_recv())
}

fn poll_entry(
    prelude: &mut Vec<NativeChatEvent>,
    mut next: impl FnMut() -> Result<Option<NativeChatEvent>, &'static str>,
) -> EntrySignal {
    // Ready remains observable until promotion, even if queue reordering put
    // this entry behind a quiet subscriber. Never consume its buffered event.
    if !prelude.is_empty() {
        return EntrySignal::Ready;
    }
    let mut announced = false;
    for _ in 0..PARKED_POLL_BUDGET {
        let event = match next() {
            Ok(Some(event)) => event,
            Ok(None) => break,
            Err(_) => return EntrySignal::Settled,
        };
        match event {
            NativeChatEvent::Queued => announced = true,
            NativeChatEvent::Event(ref value) if is_admission_event(value) => announced = true,
            NativeChatEvent::Event(ref value)
                if value["event"] == "native_execution_status"
                    && matches!(
                        value["data"]["status"].as_str(),
                        Some("cancelled" | "interrupted")
                    ) =>
            {
                return EntrySignal::Settled;
            }
            NativeChatEvent::Failed(message) => return EntrySignal::Failed(message),
            NativeChatEvent::Finished => return EntrySignal::Settled,
            other => {
                prelude.push(other);
                return EntrySignal::Ready;
            }
        }
    }
    if announced {
        return EntrySignal::Announced;
    }
    EntrySignal::Quiet
}

/// Durable followers report the admitted execution before it has a lease.
/// Those frames update queue state and must not allocate a chat bubble.
fn is_admission_event(value: &serde_json::Value) -> bool {
    match value["event"].as_str() {
        Some("native_execution_started" | "queue_update") => true,
        Some("native_execution_status") => {
            matches!(value["data"]["status"].as_str(), Some("queued" | "pending"))
        }
        _ => false,
    }
}

/// Engine rows → strip rows, plus how many rows were left out.
pub fn project_rows(rows: &[NativeQueueTurn]) -> (Vec<QueueTurn>, usize) {
    project_rows_with_waiting(rows, &[])
}

/// A message this window submitted that the engine has not admitted yet: it
/// waits inside the composer's strip, not in the transcript.
///
/// A waiting turn is a *local* hold, not an engine queue row. The window keeps
/// everything needed to send it, and only calls the engine once the thread's
/// running turn has settled. That is the only way the transcript can stay free
/// of a second bubble and of a「排队中」status the user never asked for.
#[derive(Debug, Clone)]
pub struct WaitingTurn {
    pub client_message_id: String,
    /// What the transcript shows for this message (an attachment-only send has
    /// no text, so it carries the same placeholder the bubble uses).
    pub text: String,
    pub attachments: usize,
    /// Thread the message belongs to. A waiting turn is only ever released
    /// into the thread it was typed in.
    pub session: String,
    /// Exactly what the user typed, kept verbatim: the release puts it back
    /// into the composer, so it is indistinguishable from a direct send.
    pub content: String,
    /// Attachments the composer held when the message was sent.
    pub payload: Vec<NativeChatAttachment>,
}

impl WaitingTurn {
    /// The strip row for this message: it is not an engine queue row yet.
    pub fn to_queue_turn(&self) -> QueueTurn {
        QueueTurn {
            queue_id: SharedString::from(format!("waiting-{}", self.client_message_id)),
            text: SharedString::from(self.text.as_str()),
            attachments: self.attachments as i32,
            interjected: false,
            waiting: true,
        }
    }
}

/// The waiting turns that belong to one thread. A strip is the visible
/// thread's own: a message held for another thread must never be listed above
/// this thread's input box.
pub fn waiting_for_session(waiting: &[WaitingTurn], session: &str) -> Vec<WaitingTurn> {
    waiting
        .iter()
        .filter(|turn| turn.session == session)
        .cloned()
        .collect()
}

/// Strip projection that also carries this window's own waiting turns. They
/// are listed first: they are the messages the user just sent and the engine
/// has not started, while the engine's rows are turns already admitted and
/// parked behind a running one. A waiting turn whose engine row already
/// exists is dropped in favour of that row, so the strip never shows one
/// message twice.
pub fn project_rows_with_waiting(
    rows: &[NativeQueueTurn],
    waiting: &[WaitingTurn],
) -> (Vec<QueueTurn>, usize) {
    let mut projected = waiting
        .iter()
        .filter(|turn| {
            !rows
                .iter()
                .any(|row| row.client_message_id == turn.client_message_id)
        })
        .map(|turn| turn.to_queue_turn())
        .collect::<Vec<_>>();
    let hidden = rows.len().saturating_sub(STRIP_ROWS);
    projected.extend(rows.iter().take(STRIP_ROWS).map(|row| QueueTurn {
        queue_id: SharedString::from(row.queue_id.as_str()),
        text: SharedString::from(row.content.as_str()),
        attachments: row.attachment_count as i32,
        interjected: row.priority > 0,
        waiting: false,
    }));
    (projected, hidden)
}

/// Put an already-read queue onto the strip.
pub fn publish_rows(app: &MainWindow, rows: &[NativeQueueTurn]) {
    let (projected, hidden) = project_rows(rows);
    app.set_queue_turns(ModelRc::from(Rc::new(VecModel::from(projected))));
    app.set_queue_overflow(hidden as i32);
}

/// Put the engine rows and this window's waiting turns onto the strip.
///
/// The strip belongs to the thread on screen, so held messages are filtered by
/// their own session here rather than at each call site: switching threads used
/// to carry another thread's waiting rows above this thread's input box.
pub fn publish_rows_with_waiting(
    app: &MainWindow,
    rows: &[NativeQueueTurn],
    waiting: &[WaitingTurn],
) {
    let session = app.get_active_session_id().to_string();
    let here = waiting_for_session(waiting, session.as_str());
    let (projected, hidden) = project_rows_with_waiting(rows, &here);
    app.set_queue_turns(ModelRc::from(Rc::new(VecModel::from(projected))));
    app.set_queue_overflow(hidden as i32);
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

/// True when this window already holds as many local waiting messages for one
/// thread as the strip can show. Waiting turns never reach the engine, so their
/// bound is the strip's, not the parked-channel limit — and it is counted per
/// thread, so messages held for another thread never eat this thread's room.
pub fn waiting_is_full(waiting: &[WaitingTurn], session: &str) -> bool {
    waiting
        .iter()
        .filter(|turn| turn.session == session)
        .count()
        >= STRIP_ROWS
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
    if history.is_empty()
        || selection
        || (!draft.is_empty() && !(if up { at_start } else { at_end }))
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

    #[test]
    fn buffered_output_remains_ready_until_promotion_without_losing_events() {
        let mut events = VecDeque::from([
            NativeChatEvent::Queued,
            NativeChatEvent::Event(
                serde_json::json!({"event":"llm_output_delta","data":{"delta":"first"}}),
            ),
            NativeChatEvent::Event(
                serde_json::json!({"event":"llm_output_delta","data":{"delta":"second"}}),
            ),
        ]);
        let mut prelude = Vec::new();
        assert_eq!(
            poll_entry(&mut prelude, || Ok(events.pop_front())),
            EntrySignal::Ready
        );
        assert_eq!(prelude.len(), 1);
        assert_eq!(events.len(), 1);
        for _ in 0..3 {
            assert_eq!(
                poll_entry(&mut prelude, || panic!(
                    "buffered entries must not be polled"
                )),
                EntrySignal::Ready
            );
        }
        let NativeChatEvent::Event(first) = &prelude[0] else {
            panic!("output buffered")
        };
        assert_eq!(first["data"]["delta"], "first");
        let NativeChatEvent::Event(second) = events.pop_front().unwrap() else {
            panic!("next output intact")
        };
        assert_eq!(second["data"]["delta"], "second");
    }

    #[test]
    fn withdrawn_queue_settles_without_creating_output() {
        let mut prelude = Vec::new();
        let mut events = VecDeque::from([NativeChatEvent::Queued, NativeChatEvent::Finished]);
        assert_eq!(
            poll_entry(&mut prelude, || Ok(events.pop_front())),
            EntrySignal::Settled
        );
        assert!(prelude.is_empty());
    }

    #[test]
    fn queue_admission_is_not_execution_and_withdrawal_has_no_bubble() {
        let event = |kind: &str, status: &str| {
            NativeChatEvent::Event(serde_json::json!({"event":kind,"data":{"status":status}}))
        };
        let mut events = VecDeque::from([
            NativeChatEvent::Queued,
            event("native_execution_started", ""),
            event("native_execution_status", "queued"),
            event("queue_update", "queued"),
        ]);
        let mut prelude = Vec::new();
        assert_eq!(
            poll_entry(&mut prelude, || Ok(events.pop_front())),
            EntrySignal::Announced
        );
        assert!(prelude.is_empty());
        events.push_back(event("native_execution_status", "cancelled"));
        assert_eq!(
            poll_entry(&mut prelude, || Ok(events.pop_front())),
            EntrySignal::Settled
        );
        assert!(prelude.is_empty());

        events.push_back(event("native_execution_status", "running"));
        assert_eq!(
            poll_entry(&mut prelude, || Ok(events.pop_front())),
            EntrySignal::Ready
        );
        assert_eq!(prelude.len(), 1);
    }

    fn row(id: &str, text: &str, priority: i64) -> NativeQueueTurn {
        NativeQueueTurn {
            queue_id: id.into(),
            root_turn_id: String::new(),
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
        let back = recall_step(&mut state, &history, -1, second.caret, second.caret, "更早")
            .expect("forward");
        assert_eq!(back.text.as_str(), "最新");
        let bottom = recall_step(&mut state, &history, -1, back.caret, back.caret, "最新")
            .expect("back to empty");
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

    fn waiting(id: &str, session: &str) -> WaitingTurn {
        WaitingTurn {
            client_message_id: format!("send-{}", id),
            text: format!("消息 {id}"),
            attachments: 0,
            session: session.into(),
            content: format!("消息 {id}"),
            payload: Vec::new(),
        }
    }

    #[test]
    fn a_strip_only_carries_its_own_threads_waiting_turns() {
        let waiting = vec![waiting("a", "线程甲"), waiting("b", "线程乙")];
        let here = waiting_for_session(&waiting, "线程甲");
        assert_eq!(here.len(), 1);
        assert_eq!(here[0].session, "线程甲");

        // Switching to 线程乙 must not list 线程甲's held message above its input.
        let (rows, _) = project_rows_with_waiting(&[], &waiting_for_session(&waiting, "线程乙"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].queue_id.as_str(), "waiting-send-b");

        let (rows, _) = project_rows_with_waiting(&[], &waiting_for_session(&waiting, "线程丙"));
        assert!(rows.is_empty(), "a thread holds none of these messages");
    }

    #[test]
    fn the_waiting_limit_is_counted_per_thread() {
        let mut waiting = (0..STRIP_ROWS)
            .map(|index| waiting(&format!("a{}", index), "线程甲"))
            .collect::<Vec<_>>();
        assert!(waiting_is_full(&waiting, "线程甲"));
        assert!(
            !waiting_is_full(&waiting, "线程乙"),
            "another thread's holds must not block this thread's composer"
        );
        waiting.pop();
        assert!(!waiting_is_full(&waiting, "线程甲"));
    }
}
