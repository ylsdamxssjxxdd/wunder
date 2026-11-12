use crate::{MainWindow, TimelineRow};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use wunder_desktop::native::{
    NativeChatTurn, NativeThreadUpdate, NativeThreadWatch, NativeThreadWatchCancel,
};
use wunder_desktop::NativeDesktop;

const UPDATE_QUEUE: usize = 8;
const UPDATE_BUDGET: usize = 8;
const RETRY_DELAY: Duration = Duration::from_millis(250);

struct LiveProjection {
    model: ModelRc<TimelineRow>,
    root: String,
    answer: String,
    active: bool,
}

struct Slot {
    session: String,
    turns: Vec<NativeChatTurn>,
    timeline: crate::timeline::Timeline,
    live: Option<LiveProjection>,
    cancel: Option<NativeThreadWatchCancel>,
    sender: std::sync::mpsc::SyncSender<(u64, NativeThreadUpdate)>,
    receiver: std::sync::mpsc::Receiver<(u64, NativeThreadUpdate)>,
    generation: u64,
    overflow_generation: Arc<AtomicU64>,
    wake_pending: Arc<AtomicBool>,
    watch_retry_at: Option<Instant>,
    ready: bool,
}

impl Slot {
    fn empty() -> Self {
        let (sender, receiver) = std::sync::mpsc::sync_channel(UPDATE_QUEUE);
        Self {
            session: String::new(),
            turns: Vec::new(),
            timeline: crate::timeline::Timeline::new(),
            live: None,
            cancel: None,
            sender,
            receiver,
            generation: 0,
            overflow_generation: Arc::new(AtomicU64::new(u64::MAX)),
            wake_pending: Arc::new(AtomicBool::new(false)),
            watch_retry_at: None,
            ready: false,
        }
    }

    fn clear(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        self.generation = self.generation.wrapping_add(1);
        self.overflow_generation.store(u64::MAX, Ordering::Release);
        self.wake_pending.store(false, Ordering::Release);
        self.session.clear();
        self.turns.clear();
        self.timeline = crate::timeline::Timeline::new();
        self.live = None;
        let (sender, receiver) = std::sync::mpsc::sync_channel(UPDATE_QUEUE);
        self.sender = sender;
        self.receiver = receiver;
        self.watch_retry_at = None;
        self.ready = false;
    }
}

enum PendingAction {
    Send(String),
    Stop,
    Model(String),
}

struct PendingEntry {
    session: String,
    action: PendingAction,
    started: std::time::Instant,
}

thread_local! {
    static SLOTS: RefCell<Vec<Slot>> = RefCell::new((0..4).map(|_| Slot::empty()).collect());
    static PENDING: RefCell<HashMap<usize, PendingEntry>> = RefCell::new(HashMap::new());
    static DRAFTS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    static UPDATE_SCHEDULED: Cell<bool> = const { Cell::new(false) };
    /// One process page read in flight per (pane, process).
    static PROCESS_READS: RefCell<HashSet<(usize, String)>> = RefCell::new(HashSet::new());
    static PROCESS_READS_PENDING: Cell<usize> = const { Cell::new(0) };
}

fn wake_ui(weak: &slint::Weak<MainWindow>, pending: &AtomicBool) {
    if pending.swap(true, Ordering::AcqRel) {
        return;
    }
    let _ = weak.upgrade_in_event_loop(|app| {
        app.invoke_split_thread_update_ready();
    });
}

fn schedule_drain(app: &MainWindow) {
    if UPDATE_SCHEDULED.with(|scheduled| scheduled.replace(true)) {
        return;
    }
    let weak = app.as_weak();
    slint::Timer::single_shot(Duration::ZERO, move || {
        UPDATE_SCHEDULED.with(|scheduled| scheduled.set(false));
        if let Some(app) = weak.upgrade() {
            app.invoke_split_thread_update_ready();
        }
    });
}

fn session(app: &MainWindow, slot: usize) -> String {
    match slot {
        0 => app.get_split_session_0(),
        1 => app.get_split_session_1(),
        2 => app.get_split_session_2(),
        _ => app.get_split_session_3(),
    }
    .to_string()
}

fn set_session(app: &MainWindow, slot: usize, value: &str) {
    let value = value.into();
    match slot {
        0 => app.set_split_session_0(value),
        1 => app.set_split_session_1(value),
        2 => app.set_split_session_2(value),
        _ => app.set_split_session_3(value),
    }
}

fn set_title(app: &MainWindow, slot: usize, value: &str) {
    let value = value.into();
    match slot {
        0 => app.set_split_title_0(value),
        1 => app.set_split_title_1(value),
        2 => app.set_split_title_2(value),
        _ => app.set_split_title_3(value),
    }
}

fn draft(app: &MainWindow, slot: usize) -> String {
    match slot {
        0 => app.get_split_draft_0(),
        1 => app.get_split_draft_1(),
        2 => app.get_split_draft_2(),
        _ => app.get_split_draft_3(),
    }
    .to_string()
}

fn set_draft(app: &MainWindow, slot: usize, value: String) {
    let value = value.into();
    match slot {
        0 => app.set_split_draft_0(value),
        1 => app.set_split_draft_1(value),
        2 => app.set_split_draft_2(value),
        _ => app.set_split_draft_3(value),
    }
}

fn set_model(app: &MainWindow, slot: usize, name: &str, key: &str) {
    if slot >= 4 {
        return;
    }
    let mut names = app.get_split_model_names().iter().collect::<Vec<_>>();
    let mut keys = app.get_split_model_keys().iter().collect::<Vec<_>>();
    names.resize(4, Default::default());
    keys.resize(4, Default::default());
    names[slot] = name.into();
    keys[slot] = key.into();
    app.set_split_model_names(ModelRc::new(VecModel::from(names)));
    app.set_split_model_keys(ModelRc::new(VecModel::from(keys)));
}

fn cache_drafts(app: &MainWindow) {
    for slot in 0..4 {
        let id = session(app, slot);
        if !id.is_empty() {
            DRAFTS.with(|drafts| drafts.borrow_mut().insert(id, draft(app, slot)));
        }
    }
}

fn restore_draft(app: &MainWindow, slot: usize) {
    let id = session(app, slot);
    let saved = DRAFTS
        .with(|drafts| drafts.borrow().get(&id).cloned())
        .unwrap_or_default();
    set_draft(app, slot, saved);
}

fn set_loading(app: &MainWindow, slot: usize, value: bool) {
    match slot {
        0 => app.set_split_loading_0(value),
        1 => app.set_split_loading_1(value),
        2 => app.set_split_loading_2(value),
        _ => app.set_split_loading_3(value),
    }
}

fn set_busy(app: &MainWindow, slot: usize, value: bool) {
    match slot {
        0 => app.set_split_busy_0(value),
        1 => app.set_split_busy_1(value),
        2 => app.set_split_busy_2(value),
        _ => app.set_split_busy_3(value),
    }
}

fn publish_model(app: &MainWindow, slot: usize, rows: ModelRc<TimelineRow>) {
    match slot {
        0 => app.set_split_rows_0(rows),
        1 => app.set_split_rows_1(rows),
        2 => app.set_split_rows_2(rows),
        _ => app.set_split_rows_3(rows),
    }
}

fn publish_rows(app: &MainWindow, slot: usize, rows: Vec<TimelineRow>) {
    publish_model(app, slot, ModelRc::new(VecModel::from(rows)));
}

fn clear_pane(app: &MainWindow, slot: usize) {
    PENDING.with(|pending| pending.borrow_mut().remove(&slot));
    SLOTS.with(|slots| slots.borrow_mut()[slot].clear());
    set_session(app, slot, "");
    set_title(app, slot, "");
    set_draft(app, slot, String::new());
    set_model(app, slot, "", "");
    publish_rows(app, slot, Vec::new());
    set_loading(app, slot, false);
    set_busy(app, slot, false);
    set_context(app, slot, wunder_desktop::NativeContextUsage::default());
}

fn slot_busy(slot: &Slot) -> bool {
    slot.turns.iter().any(|turn| {
        matches!(
            turn.assistant.state.as_str(),
            "running" | "queued" | "accepted" | "waiting_input" | "waiting_approval"
        )
    })
}

fn refresh_slot(slot: &mut Slot) -> bool {
    let rows = crate::native_chat::project_history(&slot.turns);
    slot.timeline.publish_history(rows);
    slot_busy(slot)
}

fn current_model(app: &MainWindow, slot: usize) -> ModelRc<TimelineRow> {
    match slot {
        0 => app.get_split_rows_0(),
        1 => app.get_split_rows_1(),
        2 => app.get_split_rows_2(),
        _ => app.get_split_rows_3(),
    }
}

fn durable_caught_up(slot: &Slot, live: &LiveProjection) -> bool {
    !live.root.is_empty()
        && slot
            .turns
            .iter()
            .find(|turn| turn.root_id == live.root)
            .is_some_and(|turn| {
                let status = if turn.assistant.stats_status.is_empty() {
                    turn.assistant.state.as_str()
                } else {
                    turn.assistant.stats_status.as_str()
                };
                let phase = crate::runtime_phase::Phase::of(status);
                (phase.terminal() || phase == crate::runtime_phase::Phase::Inquiry)
                    && turn.assistant.text == live.answer
            })
}

fn publish_slot(app: &MainWindow, index: usize) {
    SLOTS.with(|slots| {
        let mut slots = slots.borrow_mut();
        let Some(slot) = slots.get_mut(index) else {
            return;
        };
        let durable_busy = if slot.live.as_ref().is_some_and(|live| live.active) {
            slot_busy(slot)
        } else {
            refresh_slot(slot)
        };
        let caught_up = slot
            .live
            .as_ref()
            .is_some_and(|live| !live.active && durable_caught_up(slot, live));
        if caught_up {
            slot.live = None;
        }
        let (model, busy) = match slot.live.as_ref() {
            Some(live) => (live.model.clone(), durable_busy || live.active),
            None => (slot.timeline.model(), durable_busy),
        };
        if current_model(app, index) != model {
            publish_model(app, index, model);
        }
        set_busy(app, index, busy);
    });
    app.set_stream_updates(app.get_stream_updates().wrapping_add(1));
}

pub(crate) fn sync_live(app: &MainWindow, session: &str, model: ModelRc<TimelineRow>) {
    if app.get_split_count() <= 1 || app.get_active_session_id() != session {
        return;
    }
    let mut published = false;
    SLOTS.with(|slots| {
        let mut slots = slots.borrow_mut();
        let Some((index, slot)) = slots
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| slot.session == session)
        else {
            return;
        };
        let root = slot
            .live
            .as_ref()
            .map(|live| live.root.clone())
            .filter(|root| !root.is_empty())
            .unwrap_or_else(|| {
                (0..model.row_count())
                    .rev()
                    .filter_map(|index| model.row_data(index))
                    .find(|row| {
                        row.kind == crate::timeline::KIND_PROCESS && !row.process_root.is_empty()
                    })
                    .map(|row| row.process_root.to_string())
                    .unwrap_or_default()
            });
        slot.live = Some(LiveProjection {
            model: model.clone(),
            root,
            answer: String::new(),
            active: true,
        });
        if current_model(app, index) != model {
            publish_model(app, index, model);
        }
        set_busy(app, index, true);
        published = true;
    });
    if published {
        app.set_stream_updates(app.get_stream_updates().wrapping_add(1));
    }
}

fn freeze_live(live: &mut LiveProjection) {
    live.model = ModelRc::new(VecModel::from(live.model.iter().collect::<Vec<_>>()));
}

pub(crate) fn finish_live(app: &MainWindow, session: &str) {
    SLOTS.with(|slots| {
        for (index, slot) in slots.borrow_mut().iter_mut().enumerate() {
            if slot.session == session {
                if let Some(live) = slot.live.as_mut() {
                    freeze_live(live);
                    live.answer = (0..live.model.row_count())
                        .rev()
                        .filter_map(|index| live.model.row_data(index))
                        .find(|row| row.kind == crate::timeline::KIND_BODY)
                        .map(|row| row.text.to_string())
                        .unwrap_or_default();
                    live.active = false;
                    if current_model(app, index) != live.model {
                        publish_model(app, index, live.model.clone());
                    }
                }
            }
        }
    });
}

pub(crate) fn release_live(app: &MainWindow, session_id: &str) {
    let released = SLOTS.with(|slots| {
        let mut released = false;
        for slot in slots.borrow_mut().iter_mut() {
            if slot.session == session_id {
                if let Some(live) = slot.live.as_mut() {
                    freeze_live(live);
                    live.active = false;
                    released = true;
                }
            }
        }
        released
    });
    if released {
        for index in 0..4 {
            if session(app, index) == session_id {
                publish_slot(app, index);
            }
        }
    }
}

fn activate(app: &MainWindow, slot: usize) {
    let id = session(app, slot);
    if id.is_empty() || app.get_active_session_id() == id {
        return;
    }
    if let Some(index) = app.get_conversations().iter().position(|row| row.id == id) {
        cache_drafts(app);
        app.invoke_select_conversation(index as i32);
    }
}

fn run_pending(app: &MainWindow, slot: usize) {
    let snapshot = PENDING.with(|pending| {
        pending
            .borrow()
            .get(&slot)
            .map(|entry| (entry.session.clone(), entry.started))
    });
    let Some((id, started)) = snapshot else {
        return;
    };
    if started.elapsed() > Duration::from_secs(10)
        || (app.get_active_session_id().is_empty() && !app.get_session_loading())
    {
        PENDING.with(|pending| pending.borrow_mut().remove(&slot));
        app.set_status("分屏线程尚未就绪，操作已取消".into());
        return;
    }
    if app.get_active_session_id() != id || app.get_session_loading() {
        let weak = app.as_weak();
        slint::Timer::single_shot(Duration::from_millis(50), move || {
            if let Some(app) = weak.upgrade() {
                run_pending(&app, slot);
            }
        });
        return;
    }
    let action = PENDING.with(|pending| pending.borrow_mut().remove(&slot));
    let Some(entry) = action else { return };
    match entry.action {
        PendingAction::Send(content) => {
            app.set_draft(content.into());
            app.invoke_send_message();
        }
        PendingAction::Stop => app.invoke_stop_generation(),
        PendingAction::Model(key) => {
            set_model(app, slot, &key, &key);
            app.invoke_select_thread_model(key.into());
        }
    }
}

/// Fold one row in the pane the user clicked.
///
/// A pane renders its own model (`live` while its turn streams, otherwise the
/// pane's history timeline). Routing the click through the single view's
/// `timeline-toggle` folded the active thread's timeline behind the pane and
/// left the visible rows untouched, so the expand button did nothing once the
/// workspace was split. Folding is pure view state, so it also must not
/// activate the pane: switching the active thread just to collapse a row made
/// the other panes reload.
fn toggle_pane_row(slot: usize, payload: i32) {
    SLOTS.with(|slots| {
        let mut slots = slots.borrow_mut();
        let Some(slot) = slots.get_mut(slot) else {
            return;
        };
        let Some(live) = slot.live.as_ref().map(|live| live.model.clone()) else {
            slot.timeline.toggle(payload);
            return;
        };
        // A live model hands over to the pane's durable timeline as soon as the
        // snapshot catches up, so mirror the fold onto that same row by id and
        // the handoff keeps what the user opened.
        let id = usize::try_from(payload)
            .ok()
            .and_then(|index| live.row_data(index))
            .map(|row| row.id.to_string());
        crate::timeline::toggle_rows(&live, payload);
        match id.as_deref() {
            Some(id) => slot.timeline.toggle_id(id),
            None => slot.timeline.toggle(payload),
        }
    });
}

/// The model a pane currently renders: its live projection while a turn
/// streams, otherwise the pane's own history timeline.
fn pane_model(slot: usize) -> Option<ModelRc<TimelineRow>> {
    SLOTS.with(|slots| {
        let slots = slots.borrow();
        let slot = slots.get(slot)?;
        Some(match slot.live.as_ref() {
            Some(live) => live.model.clone(),
            None => slot.timeline.model(),
        })
    })
}

/// Read one work-process page for a split pane.
///
/// The single view pages into the active thread's timeline, so the process bar
/// a pane unfolded asked for a page nobody answered and stayed empty — the
/// second half of "the expand button does nothing" once the workspace splits.
/// The read targets the pane's own session and patches the model the pane
/// renders, keeping one read in flight per (pane, process).
fn process_page(
    app: &MainWindow,
    desktop: Arc<NativeDesktop>,
    slot: usize,
    payload: i32,
    direction: i32,
) {
    let target = session(app, slot);
    let Ok(index) = usize::try_from(payload) else {
        return;
    };
    if target.is_empty() {
        return;
    }
    let Some(model) = pane_model(slot) else {
        return;
    };
    let Some(mut row) = model.row_data(index) else {
        return;
    };
    if !(row.open || (row.visible && row.status_kind == crate::timeline::STATUS_RUNNING))
        || row.process_root.is_empty()
    {
        return;
    }
    // The stream owns this model until settlement: a durable page is behind its
    // active tail and must never replace it.
    if row.process_local && row.status_kind == crate::timeline::STATUS_RUNNING {
        return;
    }
    if row.process_local && direction == 0 && !row.process_error {
        return;
    }
    let root = row.process_root.to_string();
    // A retry repeats the page the reader asked for.
    let direction = if direction == 0 && row.process_error && row.process_paged {
        if row.process_newer {
            1
        } else if row.process_older {
            -1
        } else {
            0
        }
    } else {
        direction
    };
    if direction == 0 && row.process_paged && !row.process_error {
        return;
    }
    if direction == 2 && row.process_newer {
        return;
    }
    let (before, after) = match direction {
        -1 if row.process_older => (crate::chat_process::page_edge(row.process_first), None),
        1 if row.process_newer => (None, crate::chat_process::page_edge(row.process_last)),
        -1 | 1 => return,
        _ => (None, None),
    };
    let key = (slot, root.clone());
    if PROCESS_READS.with(|reads| reads.borrow().contains(&key))
        || PROCESS_READS_PENDING.with(|pending| pending.get()) >= 4
    {
        return;
    }
    PROCESS_READS.with(|reads| {
        reads.borrow_mut().insert(key.clone());
    });
    PROCESS_READS_PENDING.with(|pending| pending.set(pending.get() + 1));
    let entries = row.process_entries.clone();
    row.process_loading = direction != 2;
    row.process_error = false;
    model.set_row_data(index, row);
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let lookup = target.clone();
    let root_id = root.clone();
    std::thread::spawn(move || {
        let _ = tx.send(desktop.chat_process_page(&lookup, &root_id, before, after));
    });
    let weak = app.as_weak();
    crate::chat_process::poll(move || {
        let result = match rx.try_recv() {
            Ok(value) => value,
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(_) => Err(anyhow::anyhow!("process reader stopped")),
        };
        PROCESS_READS_PENDING.with(|pending| pending.set(pending.get().saturating_sub(1)));
        PROCESS_READS.with(|reads| {
            reads.borrow_mut().remove(&key);
        });
        let Some(app) = weak.upgrade() else {
            return true;
        };
        if session(&app, slot) != target {
            return true;
        }
        let Some(model) = pane_model(slot) else {
            return true;
        };
        let Some(index) = model
            .iter()
            .position(|row| row.process_root == root.as_str())
        else {
            return true;
        };
        let Some(mut row) = model.row_data(index) else {
            return true;
        };
        if !crate::chat_process::page_is_current(&row, &entries) {
            return true;
        }
        row.process_loading = false;
        match result {
            Ok(page) => {
                crate::chat_process::apply_page(&mut row, page, direction);
                for reason in model.iter().filter(|reason| {
                    reason.kind == crate::timeline::KIND_REASON
                        && reason.process_idx == row.process_idx
                }) {
                    crate::timeline_text::merge_reasoning_tail(
                        &row,
                        crate::timeline_text::process_entry(&reason),
                    );
                }
            }
            // A failed tail refresh is retried by the live timer; only a read
            // the reader asked for is worth a retry strip.
            Err(_) => row.process_error = direction != 2,
        }
        model.set_row_data(index, row);
        true
    });
}

fn schedule(app: &MainWindow, slot: usize, action: PendingAction) {
    let id = session(app, slot);
    if id.is_empty() {
        return;
    }
    PENDING.with(|pending| {
        pending.borrow_mut().insert(
            slot,
            PendingEntry {
                session: id,
                action,
                started: std::time::Instant::now(),
            },
        );
    });
    activate(app, slot);
    run_pending(app, slot);
}

fn attach_watch(app: &MainWindow, slot: usize, mut watch: NativeThreadWatch) {
    let (generation, cancel, sender, overflow, wake_pending, session) = SLOTS.with(|slots| {
        let mut slots = slots.borrow_mut();
        let slot = &mut slots[slot];
        if let Some(cancel) = slot.cancel.take() {
            cancel.cancel();
        }
        slot.generation = slot.generation.wrapping_add(1);
        slot.watch_retry_at = None;
        slot.overflow_generation.store(u64::MAX, Ordering::Release);
        (
            slot.generation,
            watch.cancel_handle(),
            slot.sender.clone(),
            slot.overflow_generation.clone(),
            slot.wake_pending.clone(),
            slot.session.clone(),
        )
    });
    SLOTS.with(|slots| slots.borrow_mut()[slot].cancel = Some(cancel.clone()));
    let weak = app.as_weak();
    std::thread::spawn(move || {
        while !cancel.is_cancelled() {
            let Some(update) = watch.blocking_recv() else {
                break;
            };
            match sender.try_send((generation, update)) {
                Ok(()) => {}
                Err(std::sync::mpsc::TrySendError::Full(_)) => {
                    overflow.store(generation, Ordering::Release);
                    wake_ui(&weak, &wake_pending);
                    break;
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => break,
            }
            wake_ui(&weak, &wake_pending);
        }
        if !cancel.is_cancelled() && !session.is_empty() {
            wake_ui(&weak, &wake_pending);
        }
    });
}

/// Publish one pane's context gauge. Every pane is a different thread, so the
/// numbers and the "history is a summary" flag are per slot; a pane that has
/// not been read yet stays unknown rather than borrowing the visible thread's
/// values.
fn set_context(app: &MainWindow, slot: usize, usage: wunder_desktop::NativeContextUsage) {
    if slot >= 4 {
        return;
    }
    let mut contexts = app.get_split_contexts().iter().collect::<Vec<_>>();
    contexts.resize(4, crate::SplitContext::default());
    let used = usage.used.filter(|value| *value >= 0.0);
    let capacity = usage.capacity.filter(|value| *value > 0.0);
    let known = used.is_some() && capacity.is_some();
    let count = |value: Option<f64>| {
        value
            .map(|value| crate::native_chat::format_count_i64(value.max(0.0) as i64))
            .unwrap_or_else(|| "—".to_string())
    };
    let next = crate::SplitContext {
        usage: match (used, capacity) {
            (Some(used), Some(capacity)) if capacity > 0.0 => {
                (used / capacity).clamp(0.0, 1.0) as f32
            }
            _ => 0.0,
        },
        known,
        counts: format!("{} / {} tokens", count(used), count(capacity)).into(),
        compacted: usage.compacted,
    };
    // A context frame arrives every model round; rewriting the four-entry model
    // for an unchanged gauge would re-lay out the whole split grid.
    let current = contexts[slot].clone();
    if current.usage == next.usage
        && current.known == next.known
        && current.counts == next.counts
        && current.compacted == next.compacted
    {
        return;
    }
    contexts[slot] = next;
    app.set_split_contexts(ModelRc::new(VecModel::from(contexts)));
}

/// Read one pane's context snapshot off the UI thread. The reply is dropped if
/// the pane was reassigned meanwhile, so a slow read can never paint another
/// thread's numbers into this pane.
fn refresh_pane_context(app: &MainWindow, desktop: Arc<NativeDesktop>, slot: usize, session_id: &str) {
    let target = session_id.trim().to_string();
    if target.is_empty() {
        return;
    }
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let Ok(usage) = desktop.context_usage(&target) else {
            return;
        };
        let _ = weak.upgrade_in_event_loop(move |app| {
            // A pane can be reassigned while the read is in flight; its reply
            // must not land on the new thread.
            if session(&app, slot) != target {
                return;
            }
            set_context(&app, slot, usage);
        });
    });
}

fn open_slot(app: &MainWindow, desktop: Arc<NativeDesktop>, slot: usize, row: usize) {
    let Some(conversation) = app.get_conversations().row_data(row) else {
        return;
    };
    let id = conversation.id.to_string();
    cache_drafts(app);
    if app.get_split_count() == 1 {
        app.set_split_count(2);
        let active = app.get_active_session_id().to_string();
        set_session(app, 0, &active);
        let active_title = app
            .get_conversations()
            .iter()
            .find(|row| row.id == active)
            .map(|row| row.title.to_string())
            .unwrap_or_default();
        set_title(app, 0, &active_title);
        publish_rows(app, 0, app.get_timeline().iter().collect());
        set_draft(app, 0, app.get_draft().to_string());
        if let Some(active_index) = app
            .get_conversations()
            .iter()
            .position(|row| row.id == active)
        {
            open_slot(app, desktop.clone(), 0, active_index);
        }
    }
    let previous = session(app, slot);
    let watching = SLOTS.with(|slots| slots.borrow()[slot].cancel.is_some());
    if previous == id && watching {
        return;
    }
    let duplicate = (0..app.get_split_count() as usize)
        .find(|index| *index != slot && session(app, *index) == id);
    if previous.is_empty() {
        if let Some(existing) = duplicate {
            activate(app, existing);
            return;
        }
    }
    SLOTS.with(|slots| slots.borrow_mut()[slot].clear());
    set_session(app, slot, &id);
    restore_draft(app, slot);
    set_title(app, slot, &conversation.title);
    let selected_key = app.get_selected_model_key().to_string();
    let fallback_name = if selected_key.is_empty() {
        app.get_selected_agent_model().to_string()
    } else {
        selected_key.clone()
    };
    if app.get_active_session_id() == id {
        set_model(app, slot, &fallback_name, &selected_key);
    } else {
        set_model(app, slot, &fallback_name, &fallback_name);
    }
    publish_rows(app, slot, Vec::new());
    set_loading(app, slot, true);
    set_busy(app, slot, false);
    if let Some(other) = duplicate {
        SLOTS.with(|slots| slots.borrow_mut()[other].clear());
        set_session(app, other, &previous);
        let previous_title = app
            .get_conversations()
            .iter()
            .find(|row| row.id == previous)
            .map(|row| row.title.to_string())
            .unwrap_or_default();
        set_title(app, other, &previous_title);
        publish_rows(app, other, Vec::new());
        if !previous.is_empty() {
            if let Some(index) = app
                .get_conversations()
                .iter()
                .position(|row| row.id == previous)
            {
                open_slot(app, desktop.clone(), other, index);
            }
        }
    }
    let open = (0..4)
        .filter(|slot| !session(app, *slot).is_empty())
        .count();
    if open > 0 {
        app.set_split_count(if open > 2 { 4 } else { 2 });
    }
    SLOTS.with(|slots| slots.borrow_mut()[slot].session = id.clone());
    refresh_pane_context(app, desktop.clone(), slot, &id);
    let generation = SLOTS.with(|slots| slots.borrow()[slot].generation);
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = desktop.load_chat_headers(&id).and_then(|turns| {
            let session = desktop.get_session_info(&id)?;
            Ok((turns, session.session_model_name))
        });
        let _ = weak.upgrade_in_event_loop(move |app| {
            let slot = SLOTS.with(|slots| {
                slots.borrow().iter().position(|candidate| {
                    candidate.generation == generation && candidate.session == id
                })
            });
            let Some(slot) = slot else { return };
            match result {
                Ok((turns, pinned_model)) => {
                    if let Some(model) = pinned_model {
                        set_model(&app, slot, &model, &model);
                    }
                    let watch = desktop.watch_chat_with_initial(&id, Some(turns.clone()));
                    SLOTS.with(|slots| {
                        let mut slots = slots.borrow_mut();
                        slots[slot].turns = turns;
                        slots[slot].ready = true;
                    });
                    publish_slot(&app, slot);
                    set_loading(&app, slot, false);
                    attach_watch(&app, slot, watch);
                }
                Err(error) => {
                    set_loading(&app, slot, false);
                    app.set_status(format!("无法读取分屏线程：{error}").into());
                }
            }
        });
    });
    activate(app, slot);
}

fn compact_layout(app: &MainWindow) {
    let sources = (0..4)
        .filter(|slot| !session(app, *slot).is_empty())
        .collect::<Vec<_>>();
    if sources == (0..sources.len()).collect::<Vec<_>>() {
        return;
    }
    let names = app.get_split_model_names();
    let keys = app.get_split_model_keys();
    let panes = sources
        .iter()
        .map(|source| {
            let source = *source;
            let title = match source {
                0 => app.get_split_title_0(),
                1 => app.get_split_title_1(),
                2 => app.get_split_title_2(),
                _ => app.get_split_title_3(),
            };
            let rows = match source {
                0 => app.get_split_rows_0(),
                1 => app.get_split_rows_1(),
                2 => app.get_split_rows_2(),
                _ => app.get_split_rows_3(),
            };
            let loading = match source {
                0 => app.get_split_loading_0(),
                1 => app.get_split_loading_1(),
                2 => app.get_split_loading_2(),
                _ => app.get_split_loading_3(),
            };
            let busy = match source {
                0 => app.get_split_busy_0(),
                1 => app.get_split_busy_1(),
                2 => app.get_split_busy_2(),
                _ => app.get_split_busy_3(),
            };
            (
                source,
                session(app, source),
                title,
                draft(app, source),
                rows,
                loading,
                busy,
                names.row_data(source).unwrap_or_default(),
                keys.row_data(source).unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();

    SLOTS.with(|slots| {
        let mut slots = slots.borrow_mut();
        let mut previous = std::mem::replace(&mut *slots, (0..4).map(|_| Slot::empty()).collect());
        for (target, source) in sources.iter().enumerate() {
            slots[target] = std::mem::replace(&mut previous[*source], Slot::empty());
        }
    });
    PENDING.with(|pending| {
        let mut pending = pending.borrow_mut();
        let mut previous = std::mem::take(&mut *pending);
        for (target, source) in sources.iter().enumerate() {
            if let Some(entry) = previous.remove(source) {
                pending.insert(target, entry);
            }
        }
    });

    for target in 0..4 {
        if let Some((_, id, title, draft, rows, loading, busy, _, _)) = panes.get(target) {
            set_session(app, target, id);
            set_title(app, target, title);
            set_draft(app, target, draft.clone());
            publish_model(app, target, rows.clone());
            set_loading(app, target, *loading);
            set_busy(app, target, *busy);
        } else {
            set_session(app, target, "");
            set_title(app, target, "");
            set_draft(app, target, String::new());
            publish_rows(app, target, Vec::new());
            set_loading(app, target, false);
            set_busy(app, target, false);
        }
    }
    let mut names = panes.iter().map(|pane| pane.7.clone()).collect::<Vec<_>>();
    let mut keys = panes.iter().map(|pane| pane.8.clone()).collect::<Vec<_>>();
    names.resize(4, Default::default());
    keys.resize(4, Default::default());
    app.set_split_model_names(ModelRc::new(VecModel::from(names)));
    app.set_split_model_keys(ModelRc::new(VecModel::from(keys)));
    let pending = PENDING.with(|pending| pending.borrow().keys().copied().collect::<Vec<_>>());
    for slot in pending {
        run_pending(app, slot);
    }
}

fn normalize_layout(app: &MainWindow) {
    let open = (0..4)
        .filter(|slot| !session(app, *slot).is_empty())
        .count();
    if open <= 1 {
        app.invoke_split_layout(1);
    } else {
        app.set_split_count(if open == 2 { 2 } else { 4 });
    }
}

pub(crate) fn install(app: &MainWindow, desktop: Arc<NativeDesktop>) {
    let weak = app.as_weak();
    let update_desktop = desktop.clone();
    app.on_split_thread_update_ready(move || {
        let Some(app) = weak.upgrade() else {
            return;
        };
        let started = Instant::now();
        for index in 0..4 {
            let mut changed = false;
            let mut reconnect = false;
            let mut retry_later = false;
            let mut drain_again = false;
            let mut queue_exhausted = false;
            SLOTS.with(|slots| {
                let mut slots = slots.borrow_mut();
                let slot = &mut slots[index];
                slot.wake_pending.store(false, Ordering::Release);
                let overflowed =
                    slot.overflow_generation.swap(u64::MAX, Ordering::AcqRel) == slot.generation;
                if overflowed {
                    if let Some(cancel) = slot.cancel.take() {
                        cancel.cancel();
                    }
                    let (sender, receiver) = std::sync::mpsc::sync_channel(UPDATE_QUEUE);
                    slot.sender = sender;
                    slot.receiver = receiver;
                    slot.watch_retry_at = Some(Instant::now());
                }
                for _ in 0..UPDATE_BUDGET {
                    match slot.receiver.try_recv() {
                        Ok((generation, _)) if generation != slot.generation => continue,
                        Ok((_, NativeThreadUpdate::Reset(turns))) => {
                            if slot.turns != turns {
                                slot.turns = turns;
                                changed = true;
                            }
                        }
                        Ok((_, NativeThreadUpdate::Turns(turns))) => {
                            for turn in turns {
                                if let Some(old) = slot
                                    .turns
                                    .iter_mut()
                                    .find(|old| old.root_id == turn.root_id)
                                {
                                    if *old != turn {
                                        *old = turn;
                                        changed = true;
                                    }
                                } else {
                                    slot.turns.push(turn);
                                    changed = true;
                                }
                            }
                            if changed {
                                slot.turns.sort_by_key(|turn| turn.user.user_round);
                            }
                        }
                        Ok((_, NativeThreadUpdate::Context(usage))) => {
                            // The pane runs its own thread: its gauge follows
                            // this pane's frames, including a compaction that
                            // just turned its history into a summary.
                            set_context(&app, index, usage);
                        }
                        Ok((_, NativeThreadUpdate::Failed(error))) => {
                            app.set_status(error.into());
                            if let Some(cancel) = slot.cancel.take() {
                                cancel.cancel();
                            }
                            slot.watch_retry_at = Some(Instant::now() + RETRY_DELAY);
                        }
                        Ok(_) => {}
                        Err(std::sync::mpsc::TryRecvError::Empty) => {
                            queue_exhausted = true;
                            break;
                        }
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            queue_exhausted = true;
                            if let Some(cancel) = slot.cancel.take() {
                                cancel.cancel();
                            }
                            slot.watch_retry_at = Some(Instant::now() + RETRY_DELAY);
                            break;
                        }
                    }
                    if started.elapsed() > Duration::from_millis(5) {
                        break;
                    }
                }
                drain_again = !queue_exhausted;
                if slot.ready && slot.cancel.is_none() && !slot.session.is_empty() {
                    if slot
                        .watch_retry_at
                        .is_none_or(|retry| retry <= Instant::now())
                    {
                        reconnect = true;
                    } else {
                        retry_later = true;
                    }
                }
            });
            if changed {
                publish_slot(&app, index);
            }
            if reconnect {
                let session = SLOTS.with(|slots| slots.borrow()[index].session.clone());
                let watch = update_desktop.watch_chat_with_initial(&session, None);
                attach_watch(&app, index, watch);
            } else if retry_later {
                let weak = app.as_weak();
                slint::Timer::single_shot(RETRY_DELAY, move || {
                    if let Some(app) = weak.upgrade() {
                        app.invoke_split_thread_update_ready();
                    }
                });
            }
            if drain_again {
                schedule_drain(&app);
            }
        }
    });
    let weak = app.as_weak();
    let layout_desktop = desktop.clone();
    app.on_split_layout(move |count| {
        let Some(app) = weak.upgrade() else { return };
        if count == 1 {
            cache_drafts(&app);
            let active = app.get_active_session_id();
            for slot in 0..4 {
                if active == session(&app, slot) {
                    app.set_draft(draft(&app, slot).into());
                }
            }
            app.set_split_count(1);
            PENDING.with(|pending| pending.borrow_mut().clear());
            for slot in 0..4 {
                clear_pane(&app, slot);
            }
            return;
        }
        let count = if count >= 4 { 4 } else { 2 };
        if app.get_split_count() == 1 {
            set_session(&app, 0, &app.get_active_session_id());
            set_draft(&app, 0, app.get_draft().to_string());
            publish_rows(&app, 0, app.get_timeline().iter().collect());
            let id = session(&app, 0);
            if let Some(index) = app.get_conversations().iter().position(|row| row.id == id) {
                let title = app
                    .get_conversations()
                    .row_data(index)
                    .map(|row| row.title.to_string())
                    .unwrap_or_default();
                set_title(&app, 0, &title);
                open_slot(&app, layout_desktop.clone(), 0, index);
            }
        }
        app.set_split_count(count);
    });
    let weak = app.as_weak();
    app.on_split_close(move |slot| {
        let Some(app) = weak.upgrade() else { return };
        let slot = slot.clamp(0, 3) as usize;
        let closed_active = app.get_active_session_id() == session(&app, slot);
        cache_drafts(&app);
        clear_pane(&app, slot);
        if closed_active {
            if let Some(next) = (0..4).find(|index| !session(&app, *index).is_empty()) {
                activate(&app, next);
            }
        }
        compact_layout(&app);
        normalize_layout(&app);
    });
    let weak = app.as_weak();
    let drop_desktop = desktop.clone();
    app.on_split_drop_row(move |slot, row| {
        if let Some(app) = weak.upgrade() {
            open_slot(
                &app,
                drop_desktop.clone(),
                slot.clamp(0, 3) as usize,
                row.max(0) as usize,
            );
        }
    });
    let weak = app.as_weak();
    app.on_split_activate(move |slot| {
        if let Some(app) = weak.upgrade() {
            activate(&app, slot.clamp(0, 3) as usize);
        }
    });
    let weak = app.as_weak();
    app.on_split_send(move |slot| {
        if let Some(app) = weak.upgrade() {
            let slot = slot.clamp(0, 3) as usize;
            let content = draft(&app, slot);
            set_draft(&app, slot, String::new());
            schedule(&app, slot, PendingAction::Send(content));
        }
    });
    let weak = app.as_weak();
    app.on_split_stop(move |slot| {
        if let Some(app) = weak.upgrade() {
            schedule(&app, slot.clamp(0, 3) as usize, PendingAction::Stop);
        }
    });
    let weak = app.as_weak();
    app.on_split_select_model(move |slot, key| {
        if let Some(app) = weak.upgrade() {
            schedule(
                &app,
                slot.clamp(0, 3) as usize,
                PendingAction::Model(key.to_string()),
            );
        }
    });
    app.on_split_toggle_row(|slot, index| {
        toggle_pane_row(slot.clamp(0, 3) as usize, index);
    });
    let weak = app.as_weak();
    let page_desktop = desktop.clone();
    app.on_split_process_page(move |slot, index, direction| {
        if let Some(app) = weak.upgrade() {
            process_page(
                &app,
                page_desktop.clone(),
                slot.clamp(0, 3) as usize,
                index,
                direction,
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use wunder_desktop::NativeMessage;

    fn turn(root: &str, text: &str) -> NativeChatTurn {
        let message = |mine: bool, text: &str| NativeMessage {
            turn_id: root.into(),
            text: text.into(),
            mine,
            created_at: 0.0,
            state: "completed".into(),
            stats_status: "任务完成".into(),
            user_round: 0,
            stats: Default::default(),
            attachments: Vec::new(),
        };
        NativeChatTurn {
            root_id: root.into(),
            user: message(true, "Fixture input"),
            assistant: message(false, text),
            rounds: Vec::new(),
            subagents: Vec::new(),
            process_steps: 0,
            plan: None,
            question_panel: None,
            failure: None,
        }
    }

    #[test]
    fn pane_fold_patches_the_pane_model() {
        // Each pane renders its own rows; folding has to land on that model,
        // not on the active thread's timeline behind the single view.
        let (model, payload) = SLOTS.with(|slots| {
            let mut slots = slots.borrow_mut();
            slots[0].turns = vec![turn("fixture-root", "Tail")];
            refresh_slot(&mut slots[0]);
            let model = slots[0].timeline.model();
            let position = model
                .iter()
                .position(|row| row.kind == crate::timeline::KIND_PROCESS)
                .expect("process bar");
            let payload = model.row_data(position).expect("bar row").payload;
            (model, payload)
        });
        assert!(
            !model
                .iter()
                .any(|row| row.kind == crate::timeline::KIND_PROCESS && row.open),
            "a durable process bar starts folded"
        );
        toggle_pane_row(0, payload);
        assert!(
            model
                .iter()
                .filter(|row| row.kind == crate::timeline::KIND_PROCESS)
                .all(|row| row.open),
            "the clicked pane model must carry the fold"
        );
    }

    #[test]
    fn durable_updates_patch_one_stable_slot_model() {
        let mut slot = Slot::empty();
        slot.turns = vec![
            turn("fixture-a", "Earlier answer"),
            turn("fixture-b", "Tail"),
        ];
        assert!(!refresh_slot(&mut slot));
        let model = slot.timeline.model();
        let frozen = model
            .iter()
            .find(|row| row.id == "answer-fixture-a")
            .map(|row| row.blocks)
            .expect("frozen answer blocks");

        slot.turns[1].assistant.text.push_str(" continued");
        refresh_slot(&mut slot);
        let next = slot.timeline.model();
        assert_eq!(next, model, "a durable frame must not replace the model");
        assert_eq!(
            next.iter()
                .find(|row| row.id == "answer-fixture-a")
                .map(|row| row.blocks),
            Some(frozen),
            "an unrelated turn must keep its projected blocks"
        );
        assert_eq!(
            next.iter()
                .find(|row| row.id == "answer-fixture-b")
                .map(|row| row.text.to_string()),
            Some("Tail continued".into())
        );
    }

    #[test]
    fn live_handoff_waits_for_its_own_durable_terminal() {
        let mut slot = Slot::empty();
        slot.turns = vec![turn("fixture-root", "Partial")];
        slot.turns[0].assistant.state = "running".into();
        slot.turns[0].assistant.stats_status = "正在生成…".into();
        let mut live = LiveProjection {
            model: ModelRc::default(),
            root: "fixture-root".into(),
            answer: "Partial".into(),
            active: false,
        };
        assert!(!durable_caught_up(&slot, &live));
        live.root = "fixture-other".into();
        assert!(!durable_caught_up(&slot, &live));
        live.root = "fixture-root".into();
        slot.turns[0].assistant.state = "running".into();
        slot.turns[0].assistant.stats_status = "正在生成…".into();
        assert!(!durable_caught_up(&slot, &live));
        slot.turns[0].assistant.state = "waiting_input".into();
        slot.turns[0].assistant.stats_status = "等待用户输入".into();
        assert!(durable_caught_up(&slot, &live));
        slot.turns[0].assistant.state = "completed".into();
        slot.turns[0].assistant.stats_status = "任务完成".into();
        live.answer = "Not yet durable".into();
        assert!(!durable_caught_up(&slot, &live));
        live.answer = "Partial".into();
        assert!(durable_caught_up(&slot, &live));
    }

    #[test]
    fn terminal_snapshot_does_not_follow_later_source_mutations() {
        let source = ModelRc::new(VecModel::from(crate::native_chat::project_history(&[
            turn("fixture-root", "Visible partial"),
        ])));
        let mut live = LiveProjection {
            model: source.clone(),
            root: "fixture-root".into(),
            answer: String::new(),
            active: true,
        };
        freeze_live(&mut live);
        let index = source
            .iter()
            .position(|row| row.id == "answer-fixture-root")
            .expect("source answer");
        let mut changed = source.row_data(index).expect("source row");
        changed.text = "Overwritten".into();
        source.set_row_data(index, changed);
        assert_eq!(
            live.model
                .iter()
                .find(|row| row.id == "answer-fixture-root")
                .map(|row| row.text.to_string()),
            Some("Visible partial".into())
        );
    }
}
