//! Native chat projection for the in-process desktop runtime.
use wunder_desktop::native::NativeWorkflowEntry;

use crate::{
    Conversation, DraftRecall, InquiryOption, InquiryQuestion, MainWindow, PlanStep, TimelineRow,
};
use serde_json::Value;
use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use wunder_desktop::native::NativeChatCommand;
use wunder_desktop::{
    NativeChatAttachment, NativeChatEvent, NativeChatInput, NativeDesktop, NativeQueueTurn,
    NativeStream,
};

#[path = "native_chat_commands.rs"]
mod commands;
#[path = "native_chat_observer.rs"]
mod observer;
pub(crate) use observer::project_cloud_history;
pub(crate) use observer::project_history;
#[path = "native_chat_queue.rs"]
mod queue;
use queue::{drain_entry, EntrySignal, ParkedTurn, Recall};

struct Active {
    stream: NativeStream,
    output: TurnOutput,
    /// Once the pump sees this request's first execution frame, hide its stale
    /// queue-strip row while the engine's queue snapshot catches up.
    client_message_id: String,
}

/// Live turn state. The timeline rows themselves live in `Timeline`; this
/// carries the stream identity plus the statistics row the composer shows.
struct TurnOutput {
    session: String,
    /// Shared with `State::timeline`: the live turn appends to the same model
    /// the history already occupies, so nothing is copied on send or settle.
    timeline: Rc<RefCell<crate::timeline::Timeline>>,
    state: String,
    settlement_failed: bool,
    terminal: bool,
    round: i64,
    /// Model round whose body block is currently open; a round change starts a
    /// new block instead of appending to the finished one.
    body_round: i64,
    /// Thread-wide user round supplied by the event envelope.
    user_round: i64,
    /// Interaction duration of the running turn, shown in the status line.
    stats_duration: String,
    /// Workflow entries of the running batch, keyed by tool-call id, bounded to
    /// the same entry count the timeline keeps.
    workflow: Vec<NativeWorkflowEntry>,
    /// Live stdout/stderr of the running shell command sessions, keyed by
    /// `command_session_id#command_index`. The runtime streams command output as
    /// `command_session_delta` frames; keeping the accumulated text here lets the
    /// workflow row grow while the command runs instead of only showing the
    /// result once it settles.
    command_output: std::collections::HashMap<String, String>,
    /// Execution-plan checklist (step text, status) published by `plan_update`
    /// events. Session-scoped in spirit, so a new turn resumes the last plan.
    plan_steps: Vec<(String, String)>,
    plan_explanation: String,
}

impl TurnOutput {
    fn new(session: String, timeline: Rc<RefCell<crate::timeline::Timeline>>) -> Self {
        Self {
            session,
            timeline,
            state: "请求中".into(),
            settlement_failed: false,
            terminal: false,
            round: 0,
            body_round: -1,
            user_round: 0,
            stats_duration: String::new(),
            workflow: Vec::new(),
            command_output: std::collections::HashMap::new(),
            plan_steps: Vec::new(),
            plan_explanation: String::new(),
        }
    }

    fn failed(&self) -> bool {
        self.settlement_failed || self.state == "已停止"
    }

    /// Upsert one tool entry and publish it to the timeline. The reducer calls
    /// this instead of touching rows directly, so a tool result can never
    /// create a duplicate entry for the same call id.
    fn upsert_workflow(
        &mut self,
        timeline: &mut crate::timeline::Timeline,
        mut entry: NativeWorkflowEntry,
    ) {
        let projected = if let Some(existing) = self
            .workflow
            .iter_mut()
            .find(|old| !entry.id.is_empty() && old.id == entry.id)
        {
            if entry.preview.is_empty() {
                entry.preview = existing.preview.clone();
            }
            *existing = entry.clone();
            existing.clone()
        } else {
            if self.workflow.len() >= crate::timeline::MAX_LIVE_PROCESS_ROWS {
                self.workflow.remove(0);
            }
            self.workflow.push(entry.clone());
            entry
        };
        // Results are not guaranteed to arrive in call order. Update the row
        // for this invocation id, never the current vector tail.
        timeline.upsert_tool(&projected);
    }

    /// Append one command-output chunk for a session and return the retained
    /// tail. `command_session_delta` frames arrive many times per command, so the
    /// buffer is bounded and only its newest bytes are kept.
    fn append_command_output(&mut self, key: &str, delta: &str) -> String {
        if !self.command_output.contains_key(key) && self.command_output.len() >= 8 {
            // Finished outputs already live in their workflow row. Keep only
            // a small accumulation set for commands still emitting deltas.
            if let Some(oldest) = self.command_output.keys().next().cloned() {
                self.command_output.remove(&oldest);
            }
        }
        let buffer = self.command_output.entry(key.to_string()).or_default();
        buffer.push_str(delta);
        if buffer.len() > COMMAND_OUTPUT_LIVE_LIMIT {
            let mut start = buffer.len() - COMMAND_OUTPUT_LIVE_LIMIT;
            while start < buffer.len() && !buffer.is_char_boundary(start) {
                start += 1;
            }
            *buffer = buffer.split_off(start);
        }
        buffer.clone()
    }

    fn command_output_text(&self, key: &str) -> String {
        self.command_output.get(key).cloned().unwrap_or_default()
    }
}

impl std::ops::Deref for Active {
    type Target = TurnOutput;
    fn deref(&self) -> &TurnOutput {
        &self.output
    }
}
impl std::ops::DerefMut for Active {
    fn deref_mut(&mut self) -> &mut TurnOutput {
        &mut self.output
    }
}

/// One option of a published 问询面板 question, carrying the user's tick.
#[derive(Clone, Default)]
struct InquiryOptionState {
    label: String,
    description: String,
    recommended: bool,
    selected: bool,
}

/// One question of the panel. `other` is the free-text row and `no_preference`
/// its shortcut; both live here so paging never loses an answer.
#[derive(Clone, Default)]
struct InquiryQuestionState {
    question: String,
    multiple: bool,
    no_preference: bool,
    other: String,
    options: Vec<InquiryOptionState>,
}

/// The live panel of one thread. The tool bounds a payload at four questions of
/// four options, so the whole panel is small enough to keep verbatim and the UI
/// only ever sees the current page — or every question once expanded, the same
/// toggle the web client's panel carries.
#[derive(Clone, Default)]
struct InquiryState {
    /// Durable turn id of the waiting panel. The answer is admitted as a new
    /// user turn and uses this id only to settle the old waiting turn.
    inquiry_turn_id: String,
    questions: Vec<InquiryQuestionState>,
    page: usize,
    expanded: bool,
}

impl InquiryState {
    fn current_index(&self) -> usize {
        self.page.min(self.questions.len().saturating_sub(1))
    }

    /// A question counts as answered by a tick, the free text or the 无偏好
    /// shortcut; the send button unlocks only once every question is.
    fn is_answered(question: &InquiryQuestionState) -> bool {
        question.no_preference
            || !question.other.trim().is_empty()
            || question.options.iter().any(|option| option.selected)
    }

    fn ready(&self) -> bool {
        !self.questions.is_empty()
            && self
                .questions
                .iter()
                .all(|question| Self::is_answered(question))
    }

    /// Stable client-side identity for one published panel. A late durable
    /// refresh may replay the same pending payload after the answer was sent;
    /// this key lets the native composer suppress that replay without keeping
    /// the full panel alive indefinitely.
    fn consumption_key(&self, session: &str) -> String {
        let mut key = format!("{}:{}", session, self.inquiry_turn_id);
        for question in &self.questions {
            key.push('|');
            key.push_str(&question.question);
            key.push(if question.multiple { 'm' } else { 's' });
            for option in &question.options {
                key.push('|');
                key.push_str(&option.label);
            }
        }
        key
    }

    /// Advance the guided single-choice flow. Multiple-choice questions stay
    /// on the current page so the user can finish the combination; the final
    /// completed single-choice answer asks the caller to submit exactly once.
    fn advance_after_answer(&mut self, index: usize) -> bool {
        if self
            .questions
            .get(index)
            .is_some_and(|question| question.multiple)
            || !self.questions.get(index).is_some_and(Self::is_answered)
        {
            return false;
        }
        if index + 1 < self.questions.len() {
            self.page = index + 1;
            self.expanded = false;
            return false;
        }
        self.ready()
    }

    fn toggle_option(&mut self, index: usize, option: usize) {
        let Some(question) = self.questions.get_mut(index) else {
            return;
        };
        if option >= question.options.len() {
            return;
        }
        question.no_preference = false;
        if question.multiple {
            question.options[option].selected = !question.options[option].selected;
            return;
        }
        // Single choice: re-picking the active option clears it, so the question
        // can go back to unanswered.
        let next = !question.options[option].selected;
        for candidate in question.options.iter_mut() {
            candidate.selected = false;
        }
        question.options[option].selected = next;
    }

    fn toggle_no_preference(&mut self, index: usize) {
        let Some(question) = self.questions.get_mut(index) else {
            return;
        };
        question.no_preference = !question.no_preference;
        if !question.no_preference {
            return;
        }
        // 无偏好 replaces every other choice of this question, ticks included.
        question.other.clear();
        for option in question.options.iter_mut() {
            option.selected = false;
        }
    }

    /// 使用推荐 ticks every recommended option of the question — the quick
    /// answer the web client's caret menu offers next to 无偏好.
    fn use_recommended(&mut self, index: usize) {
        let Some(question) = self.questions.get_mut(index) else {
            return;
        };
        let recommended: Vec<usize> = question
            .options
            .iter()
            .enumerate()
            .filter_map(|(position, option)| option.recommended.then_some(position))
            .collect();
        if recommended.is_empty() {
            return;
        }
        question.no_preference = false;
        question.other.clear();
        for (position, option) in question.options.iter_mut().enumerate() {
            option.selected = if question.multiple {
                recommended.contains(&position)
            } else {
                position == recommended[0]
            };
        }
    }

    fn move_page(&mut self, delta: i32) {
        let last = self.questions.len().saturating_sub(1) as i32;
        self.page = (self.page as i32 + delta).clamp(0, last).max(0) as usize;
    }

    /// The message the choices turn into: one question line and one answer line
    // each, in the same shape the web client sends.
    fn reply(&self) -> String {
        let total = self.questions.len();
        let mut lines = vec!["【问询面板选择】".to_string()];
        for (index, question) in self.questions.iter().enumerate() {
            lines.push(if total > 1 {
                format!("问题 {}/{total}：{}", index + 1, question.question)
            } else {
                format!("问题：{}", question.question)
            });
            let answer = if question.no_preference {
                "无偏好".to_string()
            } else {
                let mut parts: Vec<String> = question
                    .options
                    .iter()
                    .filter(|option| option.selected)
                    .map(|option| option.label.clone())
                    .collect();
                let other = question.other.trim();
                if !other.is_empty() {
                    parts.push(other.to_string());
                }
                parts.join("、")
            };
            lines.push(format!("答案：{answer}"));
        }
        lines.join("\n")
    }
}

/// Project a `question_panel` payload into panel state. The tool already
/// normalizes and bounds the payload; `None` means there is nothing to show.
fn inquiry_from_payload(data: &Value) -> Option<InquiryState> {
    // Durable replay carries the panel after its owning turn has been settled.
    // Only pending panels belong in the interactive composer; answered or
    // dismissed history must remain visible in the old bubble without reopening.
    if matches!(
        data.get("status").and_then(Value::as_str),
        Some("answered" | "dismissed" | "completed")
    ) {
        return None;
    }
    let questions: Vec<InquiryQuestionState> = data["questions"]
        .as_array()?
        .iter()
        .filter_map(|entry| {
            let options: Vec<InquiryOptionState> = entry["options"]
                .as_array()?
                .iter()
                .filter_map(|option| {
                    let label = option["label"].as_str()?;
                    Some(InquiryOptionState {
                        label: label.to_string(),
                        description: option["description"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                        recommended: option["recommended"].as_bool().unwrap_or(false),
                        selected: false,
                    })
                })
                .take(4)
                .collect();
            if options.is_empty() {
                return None;
            }
            Some(InquiryQuestionState {
                question: entry["question"].as_str()?.to_string(),
                multiple: entry["multiple"].as_bool().unwrap_or(false),
                no_preference: false,
                other: String::new(),
                options,
            })
        })
        .take(4)
        .collect();
    (!questions.is_empty()).then_some(InquiryState {
        inquiry_turn_id: String::new(),
        questions,
        page: 0,
        expanded: false,
    })
}

struct PendingSubmission {
    session: String,
    client_message_id: String,
    cancelled: Arc<AtomicBool>,
}

/// A stop may race the worker's synchronous lease acquisition. Keep the
/// session fenced until both the explicit runtime cancellation and a stream
/// that returned late have settled; otherwise that late stream can cancel a
/// newer request in the same thread.
struct CancellingSubmission {
    session: String,
    client_message_id: String,
    control_settled: bool,
    stream_settled: bool,
}

struct State {
    timer: Timer,
    observation: observer::Observation,
    desktop: Arc<NativeDesktop>,
    active: Option<Active>,
    /// One idle-thread submission owns its provisional timeline until the
    /// worker returns. Running threads retain their ordinary queue path.
    submitting: Option<PendingSubmission>,
    /// Stop barrier for a submission whose worker or returned stream is still
    /// settling. Unlike `submitting`, this survives navigation away and back.
    cancelling_submission: Option<CancellingSubmission>,
    /// Independent low-duty directory reconciliation while another local
    /// process owns one of the visible thread leases.
    lease_refresh_timer: Timer,
    /// History plus the live turn; the timeline is the single chat model.
    timeline: Rc<RefCell<crate::timeline::Timeline>>,
    history_generation: Arc<AtomicU64>,
    /// Oldest loaded durable user round and the next-page state.
    history_before: Option<i64>,
    history_has_more: bool,
    history_loading_older: bool,
    drafts: std::collections::HashMap<String, String>,
    recording: Option<Recording>,
    list_inflight: Arc<AtomicBool>,
    /// Only the first successful catalogue load may choose an initial thread.
    /// Ordinary refreshes (including archive completion) must leave the user's
    /// current workspace and empty state untouched.
    initial_auto_open: bool,
    /// Session-scoped plan state (explanation + steps) so the plan card
    /// survives turn boundaries within a thread.
    plans: std::collections::HashMap<String, (String, Vec<(String, String)>)>,
    /// Change key of the plan model last published to the UI; the flush runs
    /// per frame, so the slint model is only rebuilt when the plan moved.
    plan_sig: String,
    /// Live 问询面板 per thread: the panel outlives the turn that published it,
    /// and an entry is dropped as soon as it is answered or dismissed.
    inquiries: std::collections::HashMap<String, InquiryState>,
    /// Bounded replay fence for answered panels. The map itself is bounded by
    /// the visible conversation list; keep only the most recent keys here.
    consumed_inquiries: std::collections::VecDeque<String>,
    /// Submissions the engine parked for this window: one bounded subscriber
    /// each, drained on every tick so whichever turn actually started can take
    /// the live slot.
    parked: VecDeque<ParkedTurn>,
    /// Engine queue rows behind the strip projection; the façade bounds the list.
    queue_cache: Vec<NativeQueueTurn>,
    /// Only the latest asynchronous queue read may update the composer strip.
    queue_generation: Arc<AtomicU64>,
    /// Debounce deadline of the next strip refresh, while traffic is running.
    queue_due: Option<Instant>,
    /// ArrowUp/ArrowDown walk state of the composer draft.
    recall: Recall,
    /// Counter behind the per-send `client_message_id`: the only key that ties
    /// an engine queue row back to the parked subscriber this window owns.
    send_seq: u64,
}

thread_local! {
    static CHAT_STATE: RefCell<std::rc::Weak<RefCell<State>>> = RefCell::new(std::rc::Weak::new());
    static INITIAL_HISTORY: RefCell<Option<(String, Vec<wunder_desktop::native::NativeChatTurn>, i64, Option<i64>, bool)>> = RefCell::new(None);
    /// The goal elapsed label needs a one-second cadence only while a goal is
    /// actively running.  Keeping this guard on the Slint thread prevents a
    /// durable update from starting a second clock.
    static GOAL_CLOCK_ARMED: Cell<bool> = const { Cell::new(false) };
}

/// Retain one selected-thread snapshot until the passive watcher attaches. The
/// slot is deliberately single-entry and thread-local: it bounds memory and
/// stays on the Slint thread while workers only transfer their result through
/// `upgrade_in_event_loop`.
pub(crate) fn seed_initial_history(
    app: &MainWindow,
    session: &str,
    turns: Vec<wunder_desktop::native::NativeChatTurn>,
    cursor: i64,
    history_before: Option<i64>,
    history_has_more: bool,
) {
    if app.get_active_session_id() != session {
        return;
    }
    INITIAL_HISTORY.with(|slot| {
        *slot.borrow_mut() = Some((
            session.to_string(),
            turns,
            cursor,
            history_before,
            history_has_more,
        ));
    });
}

pub(crate) fn take_initial_history(
    session: &str,
) -> Option<(
    Vec<wunder_desktop::native::NativeChatTurn>,
    i64,
    Option<i64>,
    bool,
)> {
    INITIAL_HISTORY.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_ref().is_some_and(|(id, _, _, _, _)| id == session) {
            slot.take()
                .map(|(_, turns, cursor, before, has_more)| (turns, cursor, before, has_more))
        } else {
            None
        }
    })
}

fn clear_initial_history() {
    INITIAL_HISTORY.with(|slot| *slot.borrow_mut() = None);
}

/// Hand the existing timeline to a remote data source. Local execution keeps
/// running durably after its foreground subscription is released.
pub(crate) fn prepare_external_timeline(app: &MainWindow) {
    CHAT_STATE.with(|slot| {
        let Some(state) = slot.borrow().upgrade() else {
            return;
        };
        let mut state = state.borrow_mut();
        state.timer.stop();
        state.lease_refresh_timer.stop();
        state.active = None;
        // A local submission may still be acquiring its lease while cloud
        // navigation changes the visible page. Retain it (and any stop
        // barrier) so returning to its thread cannot start a second request
        // before the late worker has settled.
        state.parked.clear();
        state.observation.detach();
        state.history_generation.fetch_add(1, Ordering::AcqRel);
        state.history_before = None;
        state.history_has_more = false;
        state.history_loading_older = false;
        state.timeline.borrow_mut().clear();
        let local_id = app.get_active_session_id().to_string();
        if !local_id.is_empty() {
            if state.drafts.len() >= 100 {
                state.drafts.clear();
            }
            // Release navigation ownership through the idle-aware reaper. A
            // local run may continue after the cloud view takes focus; closing
            // its lease eagerly recreates the same self-conflict on return.
            let desktop = state.desktop.clone();
            let released_id = local_id.clone();
            desktop.defer_close_session(&released_id, Duration::from_secs(2));
            state.drafts.insert(local_id, app.get_draft().to_string());
            app.set_active_session_id("".into());
        }
        app.set_draft("".into());
        app.set_session_loading(false);
        app.set_busy(false);
        app.set_stopping(false);
        app.set_goal_visible(false);
        app.set_goal_active(false);
        app.set_inquiry_visible(false);
        app.set_queue_turns(ModelRc::default());
        app.set_queue_overflow(0);
        app.set_pending_attachments(ModelRc::default());
        reset_activity(app);
    });
}

/// Incrementally update the shared timeline by stable row id. Frozen cloud
/// rows keep their models, fold state and layout when only the tail changes.
pub(crate) fn replace_external_timeline(app: &MainWindow, mut rows: Vec<TimelineRow>, reset: bool) {
    CHAT_STATE.with(|slot| {
        let Some(state) = slot.borrow().upgrade() else {
            return;
        };
        let timeline = state.borrow().timeline.clone();
        let mut timeline = timeline.borrow_mut();
        if reset {
            timeline.clear();
            timeline.set_history(rows);
        } else {
            let model = timeline.model();
            if let Some(previous) = model.as_any().downcast_ref::<VecModel<TimelineRow>>() {
                crate::timeline_text::restore_folds(&mut rows, previous);
            }
            crate::timeline_text::sync_reasoning_tails(&rows);
            let topology_changed = model
                .iter()
                .map(|row| row.id)
                .ne(rows.iter().map(|row| row.id.clone()));
            if topology_changed {
                timeline.set_history(rows);
                publish_timeline(app, &timeline);
                return;
            }
            for (index, mut row) in rows.into_iter().enumerate() {
                if let Some(old) = model.row_data(index) {
                    row.payload = index as i32;
                    if old.text == row.text && old.status_kind == row.status_kind {
                        row.blocks = old.blocks;
                    }
                    let stats_equal = old.stats.iter().eq(row.stats.iter());
                    if old.text == row.text
                        && old.summary == row.summary
                        && old.detail == row.detail
                        && old.visible == row.visible
                        && old.status_kind == row.status_kind
                        && stats_equal
                    {
                        continue;
                    }
                }
                model.set_row_data(index, row);
            }
        }
        publish_timeline(app, &timeline);
    });
}

struct Recording {
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<Result<crate::audio_recording::RecordedAudio, String>>>,
    generation: u64,
    session: String,
    started: Instant,
}

impl Drop for Recording {
    fn drop(&mut self) {
        // Dropping the window must release the microphone promptly even when
        // the user did not press Stop first. The worker observes this flag and
        // closes its native capture handle before exiting.
        self.stop.store(true, Ordering::Release);
    }
}

pub fn install(app: &MainWindow, desktop: Arc<NativeDesktop>) {
    app.set_connected(true);
    app.set_status("正在加载内嵌运行时…".into());
    // `MainWindow` deliberately starts in a loading state so its first native
    // frame cannot expose preview fixtures. This installer is also used by
    // isolated native checks, which bypass `native_runtime::install`; clear
    // that bootstrap gate before the first real refresh below.
    app.set_chat_loading(false);
    // The dock ships with slint-viewer demo values; a real runtime clears them
    // once and lets the turn flush (or the selection reset) drive it after.
    reset_activity(app);
    app.set_conversations(ModelRc::default());
    app.set_agents(ModelRc::default());
    app.set_tools(ModelRc::default());
    app.set_models(ModelRc::default());
    let desktop_preview = desktop.clone();
    let state = Rc::new(RefCell::new(State {
        timer: Timer::default(),
        observation: observer::Observation::default(),
        desktop,
        active: None,
        submitting: None,
        cancelling_submission: None,
        lease_refresh_timer: Timer::default(),
        timeline: Rc::new(RefCell::new(crate::timeline::Timeline::new())),
        history_generation: Arc::new(AtomicU64::new(0)),
        history_before: None,
        history_has_more: false,
        history_loading_older: false,
        drafts: std::collections::HashMap::new(),
        recording: None,
        list_inflight: Arc::new(AtomicBool::new(false)),
        initial_auto_open: true,
        plans: std::collections::HashMap::new(),
        plan_sig: String::new(),
        inquiries: std::collections::HashMap::new(),
        consumed_inquiries: std::collections::VecDeque::new(),
        parked: VecDeque::new(),
        queue_cache: Vec::new(),
        queue_generation: Arc::new(AtomicU64::new(0)),
        queue_due: None,
        recall: Recall::default(),
        send_seq: 0,
    }));
    observer::install(app, state.clone());
    CHAT_STATE.with(|slot| *slot.borrow_mut() = Rc::downgrade(&state));
    bind_refresh(app, state.clone());
    bind_selection(app, state.clone());
    bind_history_paging(app, state.clone());
    bind_new_thread(app, state.clone());
    bind_send(app, state.clone());
    // 加号菜单「终端」：优先呼出补充包的 Git Bash，缺失时交给系统终端。
    let launch_weak = app.as_weak();
    let launch_desktop = state.borrow().desktop.clone();
    app.on_launch_terminal(move || {
        if let Some(app) = launch_weak.upgrade() {
            let workspace = app.get_active_workspace_root().to_string();
            if let Err(message) = launch_desktop.launch_terminal(&workspace) {
                app.set_status(message.into());
            }
        }
    });
    bind_reasoning_effort(app, state.clone());
    bind_thread_model(app, state.clone());
    bind_attachments(app);
    bind_voice_recording(app, state.clone());
    bind_stop(app, state.clone());
    bind_goal(app, state.clone());
    bind_activity(app);
    bind_inquiry(app, state.clone());
    bind_queue(app, state.clone());
    start_goal_clock(app.as_weak());
    crate::thread_trajectory::install(app, desktop_preview.clone());
    crate::subagent_detail::install(app, desktop_preview.clone());
    crate::workspace_files::install(app, desktop_preview.clone());
    crate::navigation_ui::install(app, desktop_preview.clone());
    crate::workspace_ui::install(app, desktop_preview.clone());
    let timeline = state.borrow().timeline.clone();
    bind_timeline(app, timeline, desktop_preview.clone());
    crate::chat_process::install(app, desktop_preview.clone());
    {
        let timeline = state.borrow().timeline.clone();
        let live = timeline.borrow();
        if live.needs_publish() {
            app.set_timeline(live.model());
            app.set_turn_marks(live.marks_model());
        }
    }
    let weak = app.as_weak();
    app.on_open_prompt_preview(move || {
        let Some(app) = weak.upgrade() else { return };
        app.set_prompt_preview_open(true);
        app.set_prompt_preview_loading(true);
        app.set_prompt_preview("".into());
        let cloud = app.get_cloud_chat_active();
        let session = if cloud {
            crate::cloud_threads::selected_id(&app)
        } else {
            app.get_active_session_id().to_string()
        };
        let agent = app.get_active_agent_id().to_string();
        let weak = app.as_weak();
        let desktop = desktop_preview.clone();
        std::thread::spawn(move || {
            let result = if cloud {
                desktop.cloud_thread_system_prompt(&session)
            } else {
                desktop.preview_thread_system_prompt(Some(&session), Some(&agent))
            };
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_prompt_preview_loading(false);
                let current = if cloud {
                    crate::cloud_threads::selected_id(&app)
                } else {
                    app.get_active_session_id().to_string()
                };
                if app.get_cloud_chat_active() != cloud || current != session {
                    app.set_prompt_preview_open(false);
                    return;
                }
                match result {
                    Ok(prompt) => app.set_prompt_preview(prompt.into()),
                    Err(error) => {
                        app.set_prompt_preview("".into());
                        crate::native_pages::show_error(
                            &app,
                            format!("系统提示词预览失败：{error}"),
                        );
                    }
                }
            });
        });
    });
    app.invoke_refresh_chat();
}

/// Timeline callbacks: fold toggles patch the model in place, a detail cell
/// routes resource opens back to the native layer, and the block actions save or
/// copy the answer text the row already carries.
///
/// These bind the row model's own handle instead of reaching through `State`:
/// the sidebar and pet projection invoke them from inside a `State` borrow, and
/// a second borrow of `State` there aborts the process.
fn bind_timeline(
    app: &MainWindow,
    timeline: Rc<RefCell<crate::timeline::Timeline>>,
    desktop: Arc<NativeDesktop>,
) {
    timeline.borrow_mut().set_markdown_desktop(desktop.clone());
    timeline.borrow_mut().set_markdown_root(markdown_root(app));
    let toggle_timeline = timeline.clone();
    app.on_timeline_toggle(move |payload| {
        // A fold patches only the rows it owns, so the click never re-runs the
        // reducer or rebuilds the list.
        toggle_timeline.borrow().toggle(payload);
    });
    let weak = app.as_weak();
    app.on_timeline_open(move |resource| {
        let Some(app) = weak.upgrade() else { return };
        let resource = resource.to_string();
        if resource.is_empty() {
            return;
        }
        app.invoke_open_file_native(resource.into());
    });
    let open_desktop = desktop;
    let weak = app.as_weak();
    app.on_open_file_native(move |resource| {
        let Some(app) = weak.upgrade() else { return };
        let resource = resource.to_string();
        if resource.trim().is_empty() {
            return;
        }
        let root = markdown_root(&app);
        if root.trim().is_empty() {
            app.set_status("当前会话没有可用的本地工作区".into());
            return;
        }
        let desktop = open_desktop.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = desktop.open_workspace_resource(&root, &resource);
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok(()) => app.set_status("已打开工作区文件".into()),
                Err(error) => app.set_status(format!("无法打开工作区文件：{error}").into()),
            });
        });
    });
    // Bounded introspection for the native smoke checks and the companion
    // bubble: both need the active thread's tail without reading every row.
    let counts_timeline = timeline.clone();
    app.on_timeline_counts(move || {
        let live = counts_timeline.borrow();
        crate::TimelineCounts {
            rows: live.row_count() as i32,
            answers: live.answer_count() as i32,
        }
    });
    let answer_timeline = timeline;
    app.on_timeline_last_answer(move || answer_timeline.borrow().last_answer().into());
}

/// Resolve the folder used for Markdown resources in the focused thread.
/// Workspace selection follows the thread; settings provide the default root
/// before a workspace has been selected or when the focused thread is local
/// but unassigned.
pub(crate) fn markdown_root(app: &MainWindow) -> String {
    let active = app.get_active_workspace_root().to_string();
    if active.trim().is_empty() {
        app.get_workspace_root().to_string()
    } else {
        active
    }
}

/// One projection point for every timeline publish: the row model and the
/// turn-ruler marks always move together, so the ruler can never drift from
/// the visible rows.
pub(crate) fn publish_timeline(app: &MainWindow, live: &crate::timeline::Timeline) {
    publish_timeline_with_follow(app, live, true);
}

/// Publish a durable refresh without treating it as new streamed output. A
/// history patch may briefly change nested process heights while Slint lays out
/// the page; asking the viewport to follow each such intermediate height was
/// what made the reader jump to the top and then return to the answer tail.
pub(crate) fn publish_timeline_passive(app: &MainWindow, live: &crate::timeline::Timeline) {
    publish_timeline_with_follow(app, live, false);
}

fn publish_timeline_with_follow(
    app: &MainWindow,
    live: &crate::timeline::Timeline,
    follow_output: bool,
) {
    if live.needs_publish() {
        app.set_timeline(live.model());
        app.set_turn_marks(live.marks_model());
    }
    live.sync_turn_marks();
    // One monotonic view revision covers live frames and durable history
    // refreshes alike. TimelineView uses it to schedule a bounded follow-tail
    // correction instead of polling content-y forever.
    if follow_output {
        app.set_stream_updates(app.get_stream_updates().wrapping_add(1));
    }
    crate::activity_ui::notify_timeline_rows(live.row_count());
}

/// Reset the status-dock attributes for a thread without a live plan. The
/// slint-viewer demo values (and any previous thread's state) must never leak
/// into the view of an idle session.
fn reset_activity(app: &MainWindow) {
    app.set_activity_running(false);
    app.set_activity_steps_done(0);
    app.set_activity_steps_total(0);
    app.set_activity_plan_explanation("".into());
    app.set_activity_plan_current("".into());
    app.set_activity_plan_position(0);
    app.set_activity_plan_steps(ModelRc::default());
    app.set_activity_plan_expanded(false);
}

/// Change key of the plan projection: a streaming flush runs per frame, so
/// the slint plan model is rebuilt only when the explanation, step texts or
/// statuses actually moved.
fn plan_signature(explanation: &str, steps: &[(String, String)]) -> String {
    if steps.is_empty() {
        return String::new();
    }
    let mut sig = String::with_capacity(24 + steps.len() * 24);
    sig.push_str(explanation);
    for (step, status) in steps {
        sig.push('\u{1}');
        sig.push_str(status);
        sig.push(':');
        sig.push_str(step);
    }
    sig
}

/// The step the capsule names: the running one, else the first unfinished one;
/// `(empty, 0)` once every step is done (the counter then reads done/total).
fn plan_current(steps: &[(String, String)]) -> (String, i32) {
    let index = steps
        .iter()
        .position(|(_, status)| status == "in_progress")
        .or_else(|| steps.iter().position(|(_, status)| status != "completed"));
    match index {
        Some(index) => (steps[index].0.clone(), index as i32 + 1),
        None => (String::new(), 0),
    }
}

/// Publish the status-dock projection from the live turn: the plan capsule and
/// its checklist. Returns the plan signature so the caller can persist it
/// (plus the session plan) without recomputing.
fn publish_activity(app: &MainWindow, active: &TurnOutput, last_sig: &str) -> String {
    // Terminal states stop the spinner; plan data stays for the next turn.
    let idle = matches!(
        active.state.as_str(),
        "任务完成" | "已停止" | "执行失败" | "思考中断" | "等待用户输入"
    );
    app.set_activity_running(!idle);
    let total = active.plan_steps.len() as i32;
    let done = active
        .plan_steps
        .iter()
        .filter(|(_, status)| status == "completed")
        .count() as i32;
    app.set_activity_steps_done(done);
    app.set_activity_steps_total(total);
    app.set_activity_plan_explanation(active.plan_explanation.as_str().into());
    let (current, position) = plan_current(&active.plan_steps);
    app.set_activity_plan_current(current.into());
    app.set_activity_plan_position(position);
    // A plan that went away must also fold the card back in, the way the web
    // client resets its popover when the plan disappears.
    if total == 0 {
        app.set_activity_plan_expanded(false);
    }
    let sig = plan_signature(&active.plan_explanation, &active.plan_steps);
    if sig != last_sig {
        let rows: Vec<PlanStep> = active
            .plan_steps
            .iter()
            .map(|(step, status)| PlanStep {
                step: step.as_str().into(),
                status: status.as_str().into(),
            })
            .collect();
        app.set_activity_plan_steps(ModelRc::new(VecModel::from(rows)));
    }
    sig
}

/// Project a thread's cached plan into the dock while no turn is live, so a
/// thread switch (or a reopened thread) still shows the plan it last published,
/// exactly like the web client's composer dock.
fn publish_idle_plan(app: &MainWindow, state: &State, session: &str) {
    let Some((explanation, steps)) = state.plans.get(session) else {
        reset_activity(app);
        return;
    };
    app.set_activity_running(false);
    let total = steps.len() as i32;
    let done = steps
        .iter()
        .filter(|(_, status)| status == "completed")
        .count() as i32;
    app.set_activity_steps_done(done);
    app.set_activity_steps_total(total);
    app.set_activity_plan_explanation(explanation.as_str().into());
    let (current, position) = plan_current(steps);
    app.set_activity_plan_current(current.into());
    app.set_activity_plan_position(position);
    let rows: Vec<PlanStep> = steps
        .iter()
        .map(|(step, status)| PlanStep {
            step: step.as_str().into(),
            status: status.as_str().into(),
        })
        .collect();
    app.set_activity_plan_steps(ModelRc::new(VecModel::from(rows)));
}

/// Restore a reopened thread's interactive metadata from its durable rows: the
/// last published plan feeds the dock cache and a still-pending 问询面板 comes
/// back as the card above the composer. The observer calls this once per open,
/// right after it seeds the compact history.
fn restore_thread_extras(
    app: &MainWindow,
    state: &mut State,
    session: &str,
    turns: &[wunder_desktop::NativeChatTurn],
) {
    if let Some(plan) = turns.iter().rev().find_map(|turn| turn.plan.clone()) {
        state.plans.insert(
            session.to_string(),
            (
                plan.explanation,
                plan.steps
                    .into_iter()
                    .map(|step| (step.step, step.status))
                    .collect(),
            ),
        );
    }
    if let Some(panel) = turns
        .iter()
        .rev()
        .find_map(|turn| turn.question_panel.clone())
    {
        if let Some(inquiry) = inquiry_from_payload(&panel) {
            let mut inquiry = inquiry;
            inquiry.inquiry_turn_id = turns
                .iter()
                .rev()
                .find(|turn| turn.question_panel.as_ref() == Some(&panel))
                .map(|turn| turn.root_id.clone())
                .unwrap_or_default();
            state.inquiries.insert(session.to_string(), inquiry);
        }
    }
    publish_idle_plan(app, state, session);
    publish_inquiry(app, session, state);
}

fn bind_activity(app: &MainWindow) {
    let weak = app.as_weak();
    app.on_activity_plan_dismiss(move || {
        let Some(app) = weak.upgrade() else { return };
        // Closing the checklist card is a view action: it folds the popover
        // back into the capsule and leaves the plan itself published, exactly
        // like the web client's dock. The plan only goes away when the 计划
        //面板 publishes an empty one.
        app.set_activity_plan_expanded(false);
    });
}

/// Publish one thread's 问询面板: the card is a pure projection of the visible
/// rows (the current page, or every question once expanded), and an absent
/// panel always means "hidden".
fn publish_inquiry(app: &MainWindow, session: &str, state: &State) {
    app.set_title_inquiry_count(state.inquiries.len() as i32);
    let Some(panel) = state
        .inquiries
        .get(session)
        .filter(|panel| !panel.questions.is_empty())
    else {
        app.set_inquiry_visible(false);
        app.set_inquiry_ready(false);
        app.set_inquiry_session(session.into());
        return;
    };
    let rows: Vec<InquiryQuestion> = if panel.expanded {
        panel
            .questions
            .iter()
            .enumerate()
            .map(|(index, question)| inquiry_row(index, question))
            .collect()
    } else {
        let index = panel.current_index();
        vec![inquiry_row(index, &panel.questions[index])]
    };
    app.set_inquiry_visible(true);
    app.set_inquiry_session(session.into());
    app.set_inquiry_page(panel.current_index() as i32);
    app.set_inquiry_total(panel.questions.len() as i32);
    app.set_inquiry_expanded(panel.expanded);
    app.set_inquiry_ready(panel.ready());
    app.set_inquiry_questions(ModelRc::new(VecModel::from(rows)));
}

fn inquiry_was_consumed(state: &State, key: &str) -> bool {
    !key.is_empty() && state.consumed_inquiries.iter().any(|item| item == key)
}

fn remember_consumed_inquiry(state: &mut State, key: String) {
    if key.is_empty() || inquiry_was_consumed(state, &key) {
        return;
    }
    state.consumed_inquiries.push_back(key);
    while state.consumed_inquiries.len() > 64 {
        state.consumed_inquiries.pop_front();
    }
}

fn forget_consumed_inquiry(state: &mut State, key: &str) {
    if let Some(index) = state.consumed_inquiries.iter().position(|item| item == key) {
        state.consumed_inquiries.remove(index);
    }
}

/// One visible question row. `index` is the question's ordinal in the published
/// panel (not the row position), which is what every callback addresses.
fn inquiry_row(index: usize, question: &InquiryQuestionState) -> InquiryQuestion {
    InquiryQuestion {
        index: index as i32,
        question: question.question.as_str().into(),
        multiple: question.multiple,
        no_preference: question.no_preference,
        other: question.other.as_str().into(),
        options: ModelRc::new(VecModel::from(
            question
                .options
                .iter()
                .map(|option| InquiryOption {
                    label: option.label.as_str().into(),
                    description: option.description.as_str().into(),
                    recommended: option.recommended,
                    selected: option.selected,
                })
                .collect::<Vec<_>>(),
        )),
    }
}

/// Map a row (page position while paged, question ordinal while expanded) back
/// to the question it edits.
fn inquiry_question_index(panel: &InquiryState, row: i32) -> usize {
    if panel.expanded {
        (row.max(0) as usize).min(panel.questions.len().saturating_sub(1))
    } else {
        panel.current_index()
    }
}

/// One panel interaction: apply `change` to the addressed question, then
/// republish the rows. The card owns nothing, so a republish is the whole
/// update path; free text reaches here on every keystroke and never republishes.
fn inquiry_interaction(
    weak: &slint::Weak<MainWindow>,
    state: &RefCell<State>,
    row: i32,
    change: &dyn Fn(&mut InquiryState, usize),
) -> bool {
    let Some(app) = weak.upgrade() else {
        return false;
    };
    let session = app.get_active_session_id().to_string();
    let mut current = state.borrow_mut();
    let Some(panel) = current.inquiries.get_mut(&session) else {
        return false;
    };
    let index = inquiry_question_index(panel, row);
    change(panel, index);
    let submit = panel.advance_after_answer(index);
    publish_inquiry(&app, &session, &current);
    submit
}

fn bind_inquiry(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    app.on_inquiry_option_clicked({
        let weak = weak.clone();
        let state = state.clone();
        move |row, option| {
            let submit = inquiry_interaction(&weak, &state, row, &move |panel, index| {
                panel.toggle_option(index, option.max(0) as usize)
            });
            if submit {
                if let Some(app) = weak.upgrade() {
                    app.invoke_send_message();
                }
            }
        }
    });
    app.on_inquiry_no_preference({
        let weak = weak.clone();
        let state = state.clone();
        move |row| {
            let submit = inquiry_interaction(&weak, &state, row, &|panel, index| {
                panel.toggle_no_preference(index)
            });
            if submit {
                if let Some(app) = weak.upgrade() {
                    app.invoke_send_message();
                }
            }
        }
    });
    app.on_inquiry_use_recommended({
        let weak = weak.clone();
        let state = state.clone();
        move |row| {
            let submit = inquiry_interaction(&weak, &state, row, &|panel, index| {
                panel.use_recommended(index)
            });
            if submit {
                if let Some(app) = weak.upgrade() {
                    app.invoke_send_message();
                }
            }
        }
    });
    app.on_inquiry_other_edited({
        let weak = weak.clone();
        let state = state.clone();
        move |row, text| {
            // Free text lands without a republish: the TextInput keeps the
            // caret, Rust keeps the answer, and the next republish carries it.
            let Some(app) = weak.upgrade() else { return };
            let session = app.get_active_session_id().to_string();
            let mut current = state.borrow_mut();
            let Some(panel) = current.inquiries.get_mut(&session) else {
                return;
            };
            let index = inquiry_question_index(panel, row);
            if let Some(question) = panel.questions.get_mut(index) {
                question.other = text.to_string();
                if !question.other.trim().is_empty() {
                    question.no_preference = false;
                }
                // Ready state moves with every keystroke, but the rows model is
                // left alone so the caret and the input method never reset.
                app.set_inquiry_ready(panel.ready());
            }
        }
    });
    app.on_inquiry_page_move({
        let weak = weak.clone();
        let state = state.clone();
        move |delta| {
            inquiry_interaction(&weak, &state, -1, &move |panel, _| {
                panel.move_page(delta);
            });
        }
    });
    app.on_inquiry_expanded_toggle({
        let weak = weak.clone();
        let state = state.clone();
        move || {
            inquiry_interaction(&weak, &state, -1, &move |panel, _| {
                panel.expanded = !panel.expanded;
            });
        }
    });
}

fn bind_refresh(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    app.on_refresh_chat(move || {
        if let Some(app) = weak.upgrade() {
            refresh_chat(&app, state.clone(), false);
        }
    });
}

/// Close one side of the pending-stop handshake. `stream_settled` is only
/// true after a returned `NativeStream` has been cancelled and fully drained;
/// `control_settled` is only true after the session-wide cancellation call
/// completed. Both are required before a new request may enter this thread.
fn settle_cancelling_submission(
    app: &MainWindow,
    state: &Rc<RefCell<State>>,
    session: &str,
    client_message_id: &str,
    control_settled: bool,
    stream_settled: bool,
) {
    let release = {
        let mut state = state.borrow_mut();
        let Some(pending) = state.cancelling_submission.as_mut() else {
            return;
        };
        if pending.session != session
            || (!client_message_id.is_empty() && pending.client_message_id != client_message_id)
        {
            return;
        }
        pending.control_settled |= control_settled;
        pending.stream_settled |= stream_settled;
        if !(pending.control_settled && pending.stream_settled) {
            return;
        }
        state.cancelling_submission = None;
        true
    };
    if release && app.get_active_session_id() == session {
        app.set_stopping(false);
        app.invoke_native_thread_update_ready();
    }
}

/// Keep lock indicators current while another 蛹 or 舵机 owns a thread.
/// This timer is independent of the 33ms stream reducer, runs only when the
/// visible catalogue has a lock, and its worker captures only weak UI state.
fn sync_lease_refresh_timer(app: &MainWindow, state: &Rc<RefCell<State>>) {
    let locked = app.get_conversations().iter().any(|row| row.locked);
    let weak = app.as_weak();
    let state = Rc::downgrade(state);
    let current = state.upgrade();
    let Some(current) = current else { return };
    let current = current.borrow();
    if !locked {
        current.lease_refresh_timer.stop();
        return;
    }
    if current.lease_refresh_timer.running() {
        return;
    }
    current
        .lease_refresh_timer
        .start(TimerMode::Repeated, Duration::from_millis(500), move || {
            let (Some(app), Some(state)) = (weak.upgrade(), state.upgrade()) else {
                return;
            };
            if app.get_cloud_chat_active() || !app.get_conversations().iter().any(|row| row.locked)
            {
                state.borrow().lease_refresh_timer.stop();
                return;
            }
            refresh_chat(&app, state, true);
        });
}

fn refresh_chat(app: &MainWindow, state: Rc<RefCell<State>>, silent: bool) {
    if app.get_cloud_chat_active() {
        return;
    }
    if app.get_chat_loading() || app.get_session_loading() || app.get_creating_session() {
        return;
    }
    let (desktop, allow_initial_auto_open) = {
        let mut current = state.borrow_mut();
        let allow_initial_auto_open = current.initial_auto_open;
        // Consume this before the worker runs. A later ordinary refresh must
        // never become an implicit cross-workspace navigation merely because
        // the first list read is still in flight.
        current.initial_auto_open = false;
        (current.desktop.clone(), allow_initial_auto_open)
    };
    let weak = app.as_weak();
    let inflight = state.borrow().list_inflight.clone();
    if inflight.swap(true, Ordering::AcqRel) {
        return;
    }
    if !silent {
        app.set_chat_loading(true);
    }
    std::thread::spawn(move || {
        let result = desktop.list_sessions(None).and_then(|mut items| {
            if items.is_empty() {
                let created = desktop.create_session(None)?;
                items.push(created);
            }
            Ok(items)
        });
        let _ = weak.upgrade_in_event_loop(move |app| {
            inflight.store(false, Ordering::Release);
            if !silent {
                app.set_chat_loading(false);
            }
            if app.get_cloud_chat_active() {
                return;
            }
            match result {
                Ok(items) => {
                    let rows = items
                        .into_iter()
                        .take(100)
                        .map(|item| {
                            // Scalar goal state reads before the objective is
                            // consumed by the projection.
                            let goal = item.goal;
                            let goal_active = goal.active();
                            let goal_visible = goal.visible();
                            let goal_paused = goal.paused();
                            Conversation {
                                id: item.id.into(),
                                agent_id: crate::navigation_ui::agent_key(item.agent_id.as_deref())
                                    .into(),
                                workspace_id: item.workspace_id.unwrap_or_default().into(),
                                title: item.title.into(),
                                time: format_time(item.updated_at).into(),
                                consumed_tokens: format_count_i64(item.consumed_tokens).into(),
                                tool_calls: item.tool_calls.to_string().into(),
                                quota_used: format_count_i64(item.quota_used).into(),
                                runtime_status: item.runtime_status.into(),
                                locked: item.locked,
                                goal_active,
                                goal_visible,
                                goal_objective: goal.objective.into(),
                                goal_phase: goal.phase.into(),
                                goal_activation: goal.activation.into(),
                                goal_rounds: format!(
                                    "{}/{}",
                                    goal.rounds_started, goal.max_goal_rounds
                                )
                                .into(),
                                goal_seconds: 0,
                                goal_paused,
                                origin: item.origin.unwrap_or_default().into(),
                                ..Default::default()
                            }
                        })
                        .collect::<Vec<_>>();
                    let active = rows
                        .iter()
                        .find(|row| row.id == app.get_active_session_id());
                    app.set_goal_active(active.is_some_and(|row| row.goal_active));
                    app.set_goal_visible(active.is_some_and(|row| row.goal_visible));
                    app.set_goal_objective(
                        active
                            .map(|row| row.goal_objective.clone())
                            .unwrap_or_default(),
                    );
                    app.set_goal_phase(
                        active.map(|row| row.goal_phase.clone()).unwrap_or_default(),
                    );
                    app.set_goal_activation(
                        active
                            .map(|row| row.goal_activation.clone())
                            .unwrap_or_default(),
                    );
                    app.set_goal_rounds(
                        active
                            .map(|row| row.goal_rounds.clone())
                            .unwrap_or_default(),
                    );
                    set_goal_clock(&app, active.map(|row| row.goal_seconds).unwrap_or(0));
                    app.set_goal_paused(active.is_some_and(|row| row.goal_paused));
                    update_active_thread_filter(&app, rows.iter().cloned());
                    // Background transitions (another device, a goal round)
                    // only surface through this poll: every row goes through
                    // the completion gate before the projection replaces the
                    // list. First sight only records, so startup replays
                    // nothing.
                    for row in rows.iter() {
                        crate::completion_notice::observe_status(
                            &app,
                            row.id.as_str(),
                            row.runtime_status.as_str(),
                        );
                    }
                    if !app.get_conversations().iter().eq(rows.iter().cloned()) {
                        app.set_conversations(ModelRc::new(VecModel::from(rows)));
                    }
                    // This callback is running back on Slint's event loop.
                    // Look up the UI-owned state here rather than capturing
                    // its Rc in the list worker (Rc is deliberately !Send).
                    CHAT_STATE.with(|slot| {
                        if let Some(state) = slot.borrow().upgrade() {
                            sync_lease_refresh_timer(&app, &state);
                        }
                    });
                    crate::navigation_ui::project(&app);
                    crate::navigation_ui::project_sidebar(&app, Some(desktop.clone()));
                    // The working-directory panel opens on the selected
                    // workspace's folder, never on the cloud hint; once a
                    // thread is focused (auto-selected below) its own
                    // workspace refines the target.
                    if app.get_active_session_id().is_empty() {
                        let cards = app.get_workspaces();
                        let selected = usize::try_from(app.get_selected_workspace()).ok();
                        if let Some(card) = selected
                            .and_then(|index| cards.row_data(index))
                            .filter(|card| !card.root_path.is_empty())
                        {
                            crate::workspace_files::set_target(
                                &app,
                                desktop.clone(),
                                crate::workspace_files::Target::Local(card.root_path.to_string()),
                            );
                        }
                    }
                    if app.get_active_session_id().is_empty()
                        && app.get_conversations().row_count() > 0
                        && allow_initial_auto_open
                    {
                        // Opening a thread acquires a cross-process lease. Do
                        // not auto-open a row that another live蛹/舵机 owns;
                        // otherwise every fresh startup reports THREAD_IN_USE
                        // before the user has chosen a conversation.
                        if let Some(index) =
                            app.get_conversations().iter().position(|row| !row.locked)
                        {
                            app.invoke_select_conversation(index as i32);
                        }
                    }
                }
                Err(error) => app.set_status(format!("无法读取会话：{error}").into()),
            }
        });
    });
}

fn bind_selection(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    app.on_select_conversation(move |index| {
        let Some(app) = weak.upgrade() else { return };
        if index < 0 {
            return;
        }
        let Some(row) = app.get_conversations().row_data(index as usize) else {
            return;
        };
        crate::cloud_threads::leave(&app);
        let id = row.id.to_string();
        let desktop = state.borrow().desktop.clone();
        let previous_session = app.get_active_session_id().to_string();
        clear_initial_history();
        // Thread lease I/O is deliberately deferred to the worker below. On a
        // mechanical disk, acquiring the new lease and releasing the previous
        // one here froze the Slint event loop before it could paint feedback.
        app.set_goal_active(row.goal_active);
        app.set_goal_visible(row.goal_visible);
        app.set_goal_objective(row.goal_objective.clone());
        app.set_goal_phase(row.goal_phase.clone());
        app.set_goal_activation(row.goal_activation.clone());
        app.set_goal_rounds(row.goal_rounds.clone());
        set_goal_clock(&app, row.goal_seconds);
        app.set_goal_paused(row.goal_paused);
        if row.goal_active && !row.goal_paused && !row.goal_objective.is_empty() {
            start_goal_clock(app.as_weak());
        }
        // The dock mirrors the live turn only: switching to a thread without
        // one must not inherit the previous thread's activity (or the preview
        // demo values). The plan signature drops too, so the next flush
        // republishes whatever that thread's plan is.
        let keeps_live_turn = state
            .borrow()
            .active
            .as_ref()
            .is_some_and(|turn| turn.output.session == id);
        if !keeps_live_turn {
            reset_activity(&app);
            state.borrow_mut().plan_sig.clear();
        }
        // The inquiry card follows the same ownership rule as the dock: it
        // belongs to the thread that was asked, so switching shows that
        // thread's pending panel and hides any other one.
        publish_inquiry(&app, &id, &state.borrow());
        {
            let mut current = state.borrow_mut();
            // Navigation is independent from execution: switching away demotes
            // the foreground stream to background execution. The runtime still
            // settles durably, and the observer replays it on return.
            if let Some(active) = current.active.take() {
                if active.session == id {
                    current.active = Some(active);
                    return;
                }
                current.timer.stop();
            }
            current.observation.detach();
            current.history_before = None;
            current.history_has_more = false;
            current.history_loading_older = false;
            // Keep a pending submission (and its stop barrier) across
            // navigation. Returning to the thread must not bypass an
            // in-flight lease acquisition by sending a second request.
            // This window pumps one thread at a time, so a parked subscriber of
            // another thread would only back-pressure the engine once its
            // bounded channel fills. Dropping a subscriber never cancels the
            // turn: the observer replays that thread's output when it returns.
            current.parked.clear();
            queue::reset_recall(&mut current.recall);
            if current.drafts.len() >= 100 {
                current.drafts.retain(|key, _| {
                    app.get_conversations()
                        .iter()
                        .any(|row| row.id == key.as_str())
                });
            }
            let previous = app.get_active_session_id().to_string();
            if !previous.is_empty() {
                current.drafts.insert(previous, app.get_draft().to_string());
            }
            app.set_draft(current.drafts.get(&id).cloned().unwrap_or_default().into());
        }
        let generation = state.borrow().history_generation.clone();
        let request = generation.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        let weak = app.as_weak();
        app.set_busy(false);
        app.set_stopping(false);
        reset_context_usage(&app);
        app.set_active_session_id(id.clone().into());
        app.set_session_loading(true);
        app.set_status("正在打开线程，读取历史记录…".into());
        if !previous_session.is_empty() && previous_session != id {
            // The selected predecessor should not keep an open lease forever
            // when the newly selected thread cannot be opened. The reaper
            // waits for an in-flight run, while its lease activity generation
            // turns an A → B → A return into a no-op before this grace period
            // expires.
            desktop.defer_close_session(&previous_session, Duration::from_secs(2));
        }
        // The strip is the engine's queue of the thread now on screen, not a
        // leftover of the one that was just left behind. Queue reads are
        // SQLite I/O, so the visible selection never waits for them.
        refresh_queue_async(&app, &state, &id);
        let timeline = state.borrow().timeline.clone();
        timeline.borrow_mut().clear();
        // Keep MainWindow attached to the one stable VecModel owned by the
        // reducer. Replacing it with a detached empty model makes later stream
        // deltas look stale until a full page reload.
        {
            let timeline = state.borrow().timeline.clone();
            let live = timeline.borrow();
            if live.needs_publish() {
                app.set_timeline(live.model());
                app.set_turn_marks(live.marks_model());
            }
            live.sync_turn_marks();
        }
        std::thread::spawn(move || {
            // Lease acquisition and metadata stay on the worker. The first
            // history pass reads headers only; folded process details are
            // paged on demand after the user opens that process.
            let result = desktop.open_chat_snapshot(&id);
            let opened = result.is_ok();
            let _ = weak.upgrade_in_event_loop(move |app| {
                if app.get_active_session_id() != id
                    || generation.load(Ordering::Relaxed) != request
                {
                    // A later selection won. Release only when another id is
                    // currently active; for A → B → A the current A worker
                    // shares the lease and must not be disconnected by this
                    // stale response.
                    if opened && app.get_active_session_id() != id {
                        desktop.defer_close_session(&id, Duration::from_secs(2));
                    }
                    return;
                }
                match result {
                    Ok(snapshot) => {
                        // The observer is selected by the id and adopts this
                        // snapshot on its next UI tick. Keeping it in the
                        // window-independent response avoids moving Rc state
                        // across the native worker boundary.
                        let history = snapshot.turns;
                        let history_cursor = snapshot.change_cursor;
                        let history_before = snapshot.history_before;
                        let history_has_more = snapshot.history_has_more;
                        let session = snapshot.session;
                        let agent = session.agent_id.unwrap_or_else(|| "__default__".into());
                        if app.get_active_agent_id() != agent {
                            app.set_active_agent_id(agent.clone().into());
                        }
                        // The sidebar highlights the workspace the opened
                        // thread lives in, so a later new-task lands there,
                        // and the working-directory panel follows the thread's
                        // folder.
                        crate::navigation_ui::focus_workspace_of(
                            &app,
                            desktop.clone(),
                            session.workspace_id.as_deref(),
                        );
                        app.set_selected_model_key("".into());
                        crate::entity_state::restore_agent(&app, &agent);
                        update_active_thread_filter_model(&app, &app.get_conversations());
                        crate::navigation_ui::project(&app);
                        crate::navigation_ui::project_sidebar(&app, None);
                        app.set_reasoning_effort(session.reasoning_effort.into());
                        // A thread with a pinned model shows it in the composer;
                        // without one the label falls back to the agent model
                        // (already restored by `restore_agent` above).
                        if let Some(model) = session.session_model_name.as_deref() {
                            crate::native_pages::select_model_key(&app, model);
                        }
                        crate::native_chat::seed_initial_history(
                            &app,
                            id.as_str(),
                            history,
                            history_cursor,
                            history_before,
                            history_has_more,
                        );
                        // The selected-thread observer is event driven.  The
                        // snapshot is its activation edge, replacing the old
                        // permanent UI timer that noticed this on its next
                        // UI pass.
                        app.invoke_native_thread_update_ready();
                        // The durable snapshot is ready; let the observer
                        // attach on its next frame. Keep the loading overlay
                        // until that frame publishes history, so a slow disk
                        // never flashes the welcome card between threads.
                    }
                    Err(error) => {
                        app.set_session_loading(false);
                        // This id has no established lease or observer. Clear
                        // it before the next observer tick so it cannot attach
                        // through the "already opened" watcher variant after
                        // an acquisition failure. The next selection starts
                        // from a clean, empty timeline.
                        app.set_active_session_id("".into());
                        app.set_queue_turns(ModelRc::default());
                        app.set_queue_overflow(0);
                        app.set_status(format!("无法打开线程：{error}").into());
                    }
                }
            });
        });
    });
}

/// Request the next older, keyset-bounded chat page when the timeline reaches
/// its top edge. The UI only owns a small page at a time; older cards remain
/// stable in the model until the user explicitly continues scrolling.
fn bind_history_paging(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    app.on_timeline_load_older(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_cloud_chat_active() {
            return;
        }
        let session = app.get_active_session_id().to_string();
        if session.is_empty() {
            return;
        }
        let (desktop, before, generation, request) = {
            let mut current = state.borrow_mut();
            if current.history_loading_older
                || !current.history_has_more
                || current.history_before.is_none()
            {
                return;
            }
            current.history_loading_older = true;
            let generation = current.history_generation.clone();
            let request = generation.load(Ordering::Acquire);
            (
                current.desktop.clone(),
                current.history_before.expect("checked above"),
                generation,
                request,
            )
        };
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = desktop.load_chat_history_page(&session, before);
            let _ = weak.upgrade_in_event_loop(move |app| {
                let Some(state) = CHAT_STATE.with(|slot| slot.borrow().upgrade()) else {
                    return;
                };
                let mut current = state.borrow_mut();
                if generation.load(Ordering::Acquire) != request
                    || app.get_active_session_id() != session
                {
                    return;
                }
                current.history_loading_older = false;
                match result {
                    Ok((turns, next_before, has_more)) => {
                        // A non-progressing cursor would otherwise issue the
                        // same slow disk read forever if a malformed legacy
                        // record cannot fit into the response budget.
                        current.history_before = next_before;
                        current.history_has_more = has_more && next_before.is_some();
                        if current.observation.prepend(turns) {
                            let timeline = current.timeline.clone();
                            current.observation.publish(&app, &timeline);
                        }
                    }
                    Err(error) => {
                        app.set_status(format!("无法读取更早的历史记录：{error}").into());
                    }
                }
            });
        });
    });
}

// A thread counts as an untouched new one while it has seen no model work:
// zero tokens, zero tool calls and no queued or running status. Scoped to the
// workspace a new task would open in.
fn is_untouched_thread(row: &Conversation, workspace_id: Option<&str>) -> bool {
    let in_scope = match workspace_id {
        Some(workspace) => row.workspace_id.as_str() == workspace,
        None => false,
    };
    in_scope
        && row.consumed_tokens == "0"
        && row.tool_calls == "0"
        && !matches!(
            runtime_status_kind(row.runtime_status.as_str()),
            "running" | "pending"
        )
}

// The workspace's single draft thread, derived from the list itself instead of
// a remembered id: switches, refreshes and restarts all keep working.
fn find_untouched_thread(app: &MainWindow, workspace_id: Option<&str>) -> Option<String> {
    app.get_conversations()
        .iter()
        .find(|row| is_untouched_thread(row, workspace_id))
        .map(|row| row.id.to_string())
}

fn bind_new_thread(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    // Workspace-first: a new task opens in the selected workspace (§5.2.1);
    // repeated clicks reopen the untouched draft thread instead of stacking
    // server-side empty sessions.
    app.on_new_thread(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_cloud_chat_active() {
            app.invoke_cloud_new_thread();
            return;
        }
        if app.get_creating_session() || app.get_session_loading() || app.get_chat_loading() {
            return;
        }
        let workspace = app
            .get_workspaces()
            .row_data(usize::try_from(app.get_selected_workspace()).unwrap_or(0))
            .map(|row| row.workspace_id.to_string());
        if let Some(id) = find_untouched_thread(&app, workspace.as_deref()) {
            let index = app
                .get_conversations()
                .iter()
                .position(|row| row.id == id)
                .unwrap_or(0);
            app.set_page(crate::DesktopPage::Messages);
            app.invoke_select_conversation(index as i32);
            return;
        }
        app.set_creating_session(true);
        let desktop = state.borrow().desktop.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = desktop.create_session(workspace.as_deref());
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_creating_session(false);
                match result {
                    Ok(session) => {
                        // Pin the new thread above older activity before the
                        // next refresh re-sorts the list.
                        let row = Conversation {
                            id: session.id.clone().into(),
                            agent_id: crate::navigation_ui::agent_key(session.agent_id.as_deref())
                                .into(),
                            workspace_id: session.workspace_id.clone().unwrap_or_default().into(),
                            title: session.title.into(),
                            time: format_time(session.updated_at).into(),
                            consumed_tokens: format_count_i64(session.consumed_tokens).into(),
                            tool_calls: session.tool_calls.to_string().into(),
                            quota_used: format_count_i64(session.quota_used).into(),
                            runtime_status: session.runtime_status.into(),
                            locked: session.locked,
                            ..Default::default()
                        };
                        let mut rows = app.get_conversations().iter().collect::<Vec<_>>();
                        rows.insert(0, row);
                        rows.truncate(100);
                        update_active_thread_filter(&app, rows.iter().cloned());
                        app.set_conversations(ModelRc::new(VecModel::from(rows)));
                        crate::navigation_ui::project(&app);
                        app.set_page(crate::DesktopPage::Messages);
                        app.invoke_select_conversation(0);
                    }
                    Err(error) => app.set_status(format!("无法新建会话：{error}").into()),
                }
            });
        });
    });
}

/// Thread model pinning: the composer's model picker writes a per-thread
/// override instead of the global default. The label moves optimistically in
/// the same frame; the meta write races behind and reverts on failure.
fn bind_thread_model(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    app.on_select_thread_model(move |key| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_cloud_chat_active() {
            crate::cloud_threads::set_model(&app, state.borrow().desktop.clone(), key.to_string());
            return;
        }
        let key = key.trim().to_string();
        if key.is_empty() {
            return;
        }
        let session = app.get_active_session_id().to_string();
        if session.is_empty() {
            return;
        }
        // Optimistic switch: project the picked card into the composer label
        // right away; persistence races behind.
        let previous_key = app.get_selected_model_key().to_string();
        crate::native_pages::select_model_key(&app, &key);
        let desktop = state.borrow().desktop.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = desktop.set_session_model(&session, &key);
            let _ = weak.upgrade_in_event_loop(move |app| {
                if app.get_cloud_chat_active() || app.get_active_session_id() != session {
                    return;
                }
                match result {
                    Ok(()) => app.set_status("已切换本线程模型".into()),
                    Err(error) => {
                        crate::native_pages::select_model_key(&app, &previous_key);
                        app.set_status(format!("无法切换模型：{error}").into());
                    }
                }
            });
        });
    });
}

fn bind_reasoning_effort(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    app.on_save_session_reasoning_effort(move |value| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_cloud_chat_active() {
            crate::cloud_threads::set_effort(
                &app,
                state.borrow().desktop.clone(),
                value.to_string(),
            );
            return;
        }
        let session = app.get_active_session_id().to_string();
        if session.is_empty() {
            return;
        }
        let desktop = state.borrow().desktop.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = desktop.save_session_reasoning_effort(&session, &value);
            let _ = weak.upgrade_in_event_loop(move |app| {
                if app.get_cloud_chat_active() || app.get_active_session_id() != session {
                    return;
                }
                match result {
                    Ok(value) => app.set_reasoning_effort(value.into()),
                    Err(error) => app.set_status(format!("无法保存思考等级：{error}").into()),
                }
            });
        });
    });
}

fn bind_send(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    // Serializes the worker half of a send: the engine parks a busy thread in
    // arrival order, so the completion of an earlier click must be posted
    // before the next send_chat starts, or the live slot could go to a turn
    // the engine actually queued behind another one.
    let send_lock = Arc::new(std::sync::Mutex::new(()));
    app.on_send_message(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_command_pending() {
            return;
        }
        if !app.get_cloud_chat_active() {
            // `State` has one foreground timeline and one provisional
            // submission slot. Do not let a quick A -> B navigation replace
            // A's identity while its worker is still acquiring A's lease:
            // the late completion must either own that exact slot or settle
            // its stop barrier. The lease trip is short, and blocking this
            // one local send preserves thread isolation without making Rc UI
            // state cross a worker boundary.
            let pending = state.borrow();
            if pending.cancelling_submission.is_some() {
                app.set_status("正在停止上一条请求，请稍候…".into());
                return;
            }
            if pending.submitting.is_some() {
                app.set_status("正在提交，请稍候…".into());
                return;
            }
        }
        let content = app.get_draft().trim().to_string();
        if app.get_cloud_chat_active() {
            if !crate::cloud_threads::is_active(&app) {
                app.set_status("请先选择或新建云端线程".into());
            } else if app.get_pending_attachments().row_count() > 0 {
                app.set_status("云端线程暂不支持此附件，请先移除附件".into());
            } else {
                crate::cloud_threads::send(&app, state.borrow().desktop.clone(), content);
            }
            return;
        }
        if commands::dispatch(&app, &state, &content) {
            return;
        }
        // Guard the not-ready states, but never silently: a dead-looking send
        // button is how a broken runtime used to hide from the user. A running
        // turn is no longer one of them — the engine parks this submission in
        // the thread's queue and the strip above the input shows it.
        let waiting_for_history = {
            let state = state.borrow();
            !state.observation.is_ready() && state.active.is_none()
        };
        if waiting_for_history
            || app.get_session_loading()
            || app.get_chat_loading()
            || app.get_creating_session()
        {
            if waiting_for_history {
                app.set_status("运行时尚未就绪，请稍候再试".into());
            } else {
                app.set_status("正在加载会话，请稍候再试".into());
            }
            return;
        }
        if state.borrow().active.is_some() && queue::parked_is_full(&state.borrow().parked) {
            app.set_status("等待发送的轮次已满，请先撤下几条或稍候".into());
            return;
        }
        let session = app.get_active_session_id().to_string();
        let draft_content = app.get_draft().trim().to_string();
        // A fully answered 问询面板 *is* the message: the choices go out first
        // and the draft follows as the user's own note. An unanswered card is
        // left alone, so a plain message cannot silently consume a question the
        // user has not answered yet.
        let answered = state
            .borrow()
            .inquiries
            .get(&session)
            .filter(|panel| panel.ready())
            .cloned();
        let inquiry_key = answered
            .as_ref()
            .map(|panel| panel.consumption_key(&session))
            .unwrap_or_default();
        if answered.is_some() && inquiry_was_consumed(&state.borrow(), &inquiry_key) {
            return;
        }
        let content = match answered.as_ref() {
            Some(panel) => {
                let reply = panel.reply();
                if draft_content.is_empty() {
                    reply
                } else {
                    format!("{reply}\n\n用户补充：{draft_content}")
                }
            }
            None => draft_content.clone(),
        };
        let pending_attachments = app.get_pending_attachments().iter().collect::<Vec<_>>();
        let attachments = pending_attachments
            .iter()
            .map(|attachment| NativeChatAttachment {
                name: attachment.name.to_string(),
                content: attachment.data_url.to_string(),
                content_type: attachment.mime_type.to_string(),
            })
            .collect::<Vec<_>>();
        if session.is_empty() {
            // No live thread selected (usually a failed startup projection):
            // say so instead of swallowing the send.
            app.set_status("当前没有活动的线程，请先在左侧新建线程".into());
            return;
        }
        if content.is_empty() && attachments.is_empty() {
            return;
        }
        if content.len() > 16_384 {
            app.set_status("输入过长，请分段发送".into());
            return;
        }
        let display_content = if content.is_empty() {
            "[图片附件]".to_string()
        } else {
            content.clone()
        };
        let reasoning_effort = app.get_reasoning_effort().to_string();
        let reasoning_effort = (reasoning_effort != "default").then_some(reasoning_effort);
        // Every submission carries its own id: a parked row comes back through
        // the engine projection, and this is what lets a withdraw drop exactly
        // the subscriber that belongs to it.
        let client_message_id = {
            let mut st = state.borrow_mut();
            st.send_seq += 1;
            format!("native-send-{}-{}", std::process::id(), st.send_seq)
        };
        // send_chat walks the thread-lease SQLite path, far too slow for the
        // click handler on a cold disk. The composer acknowledges the send at
        // once — draft, chips and the answered panel clear exactly as before —
        // and the worker wires the returned stream into the live slot or the
        // parked list through the event loop. A failure restores the composer,
        // so nothing the user typed is lost.
        app.set_draft("".into());
        app.set_pending_attachments(ModelRc::default());
        // Only an answered panel was carried out by this turn. Ordinary
        // messages must leave an unanswered panel available to the user.
        if answered.is_some() {
            let mut current = state.borrow_mut();
            current.inquiries.remove(&session);
            remember_consumed_inquiry(&mut current, inquiry_key.clone());
        }
        publish_inquiry(&app, &session, &state.borrow());
        // An answered inquiry is a user reply even before the worker finishes
        // the lease round-trip. Project its bubble now when this thread has no
        // foreground run. A normal interjection still uses its queue path so
        // it cannot overwrite another live reducer's timeline state.
        let optimistic = answered.is_some() && state.borrow().active.is_none();
        let submission_cancelled = Arc::new(AtomicBool::new(false));
        if state.borrow().active.is_none() {
            state.borrow_mut().submitting = Some(PendingSubmission {
                session: session.clone(),
                client_message_id: client_message_id.clone(),
                cancelled: submission_cancelled.clone(),
            });
        }
        let previous_busy = app.get_busy();
        let previous_activity_running = app.get_activity_running();
        let previous_session_status = app
            .get_conversations()
            .iter()
            .find(|row| row.id == session)
            .map(|row| row.runtime_status.to_string())
            .unwrap_or_default();
        if optimistic {
            let timeline = state.borrow().timeline.clone();
            let mut live = timeline.borrow_mut();
            live.set_highlight(true);
            live.begin_turn(&client_message_id, &display_content);
            live.set_turn_status("请求中", 0);
            publish_timeline(&app, &live);
            app.set_busy(true);
            app.set_activity_running(true);
            app.set_stopping(false);
            app.set_follow_output(true);
            set_session_status(&app, &session, "请求中");
        }
        let desktop = state.borrow().desktop.clone();
        let send_order = Arc::clone(&send_lock);
        let weak = weak.clone();
        std::thread::spawn(move || {
            let _order = send_order.lock().ok();
            let result = if submission_cancelled.load(Ordering::Acquire) {
                Err(anyhow::anyhow!("submission cancelled"))
            } else {
                desktop.send_chat(NativeChatInput {
                    session_id: session.clone(),
                    content: content.clone(),
                    client_message_id: Some(client_message_id.clone()),
                    reasoning_effort,
                    inquiry_turn_id: answered
                        .as_ref()
                        .map(|panel| panel.inquiry_turn_id.clone())
                        .filter(|id| !id.is_empty()),
                    attachments,
                })
            };
            let _ = weak.upgrade_in_event_loop(move |app| {
                CHAT_STATE.with(|slot| {
                    let Some(state) = slot.borrow().upgrade() else {
                        return;
                    };
                    let same_thread = app.get_active_session_id() == session;
                    let owns_submission = {
                        let mut st = state.borrow_mut();
                        let owns = st.submitting.as_ref().is_some_and(|pending| {
                            pending.session == session
                                && pending.client_message_id == client_message_id
                        });
                        if owns {
                            st.submitting = None;
                        }
                        owns
                    };
                    // Stop releases the UI immediately, even while a slow
                    // lease read is finishing. Its late callback must never
                    // re-install the stopped request or restore an old panel.
                    if submission_cancelled.load(Ordering::Acquire) {
                        if let Ok(mut stream) = result {
                            stream.cancel();
                            let weak = app.as_weak();
                            let session = session.clone();
                            let client_message_id = client_message_id.clone();
                            std::thread::spawn(move || {
                                // This is the cancellation acknowledgement:
                                // do not release the session fence until the
                                // native stream task has observed its token
                                // and stopped producing events.
                                while stream.blocking_recv().is_some() {}
                                let _ = weak.upgrade_in_event_loop(move |app| {
                                    CHAT_STATE.with(|slot| {
                                        if let Some(state) = slot.borrow().upgrade() {
                                            settle_cancelling_submission(
                                                &app,
                                                &state,
                                                &session,
                                                &client_message_id,
                                                false,
                                                true,
                                            );
                                        }
                                    });
                                });
                            });
                        } else {
                            settle_cancelling_submission(
                                &app,
                                &state,
                                &session,
                                &client_message_id,
                                false,
                                true,
                            );
                        }
                        return;
                    }
                    let optimistic_visible = optimistic && owns_submission;
                    let stream = match result {
                        Ok(stream) => stream,
                        Err(error) => {
                            // Give the composer back what the failed send
                            // consumed, unless the user already typed on.
                            if same_thread {
                                if app.get_draft().trim().is_empty() {
                                    app.set_draft(draft_content.as_str().into());
                                }
                                if app.get_pending_attachments().row_count() == 0
                                    && !pending_attachments.is_empty()
                                {
                                    app.set_pending_attachments(ModelRc::new(VecModel::from(
                                        pending_attachments,
                                    )));
                                }
                            } else {
                                let mut st = state.borrow_mut();
                                let untouched = st
                                    .drafts
                                    .get(&session)
                                    .is_none_or(|draft| draft.trim().is_empty());
                                if untouched {
                                    st.drafts.insert(session.clone(), draft_content.clone());
                                }
                            }
                            if let Some(panel) = answered {
                                let mut st = state.borrow_mut();
                                forget_consumed_inquiry(&mut st, &inquiry_key);
                                st.inquiries.entry(session.clone()).or_insert(panel);
                                if same_thread {
                                    publish_inquiry(&app, &session, &st);
                                }
                            }
                            if same_thread && optimistic_visible {
                                // The optimistic bubble did not reach the
                                // engine. Remove only its provisional tail;
                                // a durable snapshot can be stale while the
                                // selected-thread watcher is mid-refresh.
                                let st = state.borrow();
                                let timeline = st.timeline.clone();
                                let mut live = timeline.borrow_mut();
                                // A newer submission may already own the
                                // stream and timeline. Its state must survive
                                // this older worker's failed completion.
                                if st.active.is_none()
                                    && st.parked.is_empty()
                                    && live.remove_live_turn(&client_message_id)
                                {
                                    publish_timeline_passive(&app, &live);
                                    app.set_busy(previous_busy);
                                    app.set_activity_running(previous_activity_running);
                                    set_session_status(&app, &session, &previous_session_status);
                                }
                            }
                            app.set_status(format!("无法发送：{error}").into());
                            return;
                        }
                    };
                    if !same_thread {
                        // The view moved to another thread while the request was
                        // in flight. The engine owns the submitted turn; dropping
                        // the subscriber releases only this window's channel.
                        return;
                    }
                    if state.borrow().active.is_some() {
                        // A turn already owns the live slot, so this one waits on
                        // its own subscriber: the pump promotes whichever parked
                        // stream starts, and the strip shows the engine's queue
                        // until then.
                        if queue::parked_is_full(&state.borrow().parked) {
                            app.set_status("等待发送的轮次已满，请先撤下几条或稍候".into());
                            refresh_queue_async(&app, &state, &session);
                            return;
                        }
                        state.borrow_mut().parked.push_back(ParkedTurn::new(
                            stream,
                            session.clone(),
                            display_content.clone(),
                            client_message_id,
                        ));
                        refresh_queue_async(&app, &state, &session);
                        queue::schedule_due(
                            &mut state.borrow_mut().queue_due,
                            Duration::from_millis(220),
                        );
                        app.set_status("已加入等待发送".into());
                        return;
                    }
                    // An answered inquiry may already own the timeline.
                    // Ordinary sends get their bubble only after the worker
                    // returns, preserving a running turn's live projection.
                    let timeline = state.borrow().timeline.clone();
                    if !optimistic_visible {
                        let mut live = timeline.borrow_mut();
                        live.set_highlight(true);
                        live.begin_turn(&client_message_id, &display_content);
                        live.set_turn_status("请求中", 0);
                        publish_timeline(&app, &live);
                        app.set_busy(true);
                        app.set_activity_running(true);
                        app.set_stopping(false);
                        app.set_follow_output(true);
                        set_session_status(&app, &session, "请求中");
                    }
                    // A fresh turn starts without a stale queue banner.
                    app.set_cloud_queue_position(-1);
                    app.set_stream_bytes(0);
                    app.set_stream_updates(0);
                    app.set_stream_max_ui_ms(0.0);
                    app.set_stream_max_backlog(0);
                    // Keep the passive thread watcher attached while the
                    // foreground stream is running. It is the authoritative
                    // source for queue admission, context occupancy and
                    // sibling thread status; detaching it here made an
                    // interjected turn invisible until the next navigation.
                    // Plan state is session-scoped: a new turn resumes the
                    // thread's last published checklist; file counters always
                    // start from zero.
                    let mut output = TurnOutput::new(session.clone(), timeline);
                    if let Some((explanation, steps)) = state.borrow().plans.get(&session) {
                        output.plan_explanation = explanation.clone();
                        output.plan_steps = steps.clone();
                    }
                    state.borrow_mut().active = Some(Active {
                        stream,
                        output,
                        client_message_id,
                    });
                    start_timer(&app, state.clone());
                });
            });
        });
    });
}

fn bind_attachments(app: &MainWindow) {
    let weak = app.as_weak();
    app.on_capture_screenshot(move |hide_window, region| {
        // The composer menu is closed by the same Slint callback that starts
        // the capture. Defer one event-loop turn so its popup and the final
        // composer frame are painted before the main window is hidden. Without
        // this boundary the desktop read can retain the pre-close frame in
        // the compositor, which looks like a partial ghost in the selector.
        slint::Timer::single_shot(std::time::Duration::from_millis(32), {
            let weak = weak.clone();
            move || {
                if region {
                    crate::screenshot::capture(weak, hide_window);
                } else {
                    crate::screenshot::capture_fullscreen(weak, hide_window);
                }
            }
        });
    });
    // §8.2 file attachment: the native picker blocks, so it runs off the UI
    // thread and the chips land back through the event loop.
    let weak = app.as_weak();
    app.on_pick_attachments(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_busy() {
            return;
        }
        app.set_status("正在读取附件…".into());
        let weak = weak.clone();
        std::thread::spawn(move || {
            let picked = crate::file_dialog::pick_attachments();
            let _ = weak.upgrade_in_event_loop(move |app| match picked {
                Ok(paths) if paths.is_empty() => app.set_status("".into()),
                Ok(paths) => {
                    let mut attachments = app.get_pending_attachments().iter().collect::<Vec<_>>();
                    let mut skipped = 0usize;
                    for path in paths {
                        match crate::file_dialog::read_attachment(&path) {
                            Some(attachment) => attachments.push(attachment),
                            None => skipped += 1,
                        }
                    }
                    let added = attachments.len();
                    app.set_pending_attachments(ModelRc::new(VecModel::from(attachments)));
                    app.set_status(
                        if skipped == 0 {
                            format!("已添加 {added} 个附件")
                        } else {
                            format!("已添加 {added} 个附件，{skipped} 个因超出大小上限被跳过")
                        }
                        .into(),
                    );
                }
                Err(error) => app.set_status(format!("无法读取附件：{error}").into()),
            });
        });
    });
    let weak = app.as_weak();
    app.on_remove_attachment(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        let mut attachments = app.get_pending_attachments().iter().collect::<Vec<_>>();
        if index < attachments.len() {
            attachments.remove(index);
            app.set_pending_attachments(ModelRc::new(VecModel::from(attachments)));
        }
    });
}

fn bind_voice_recording(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    app.on_record_voice(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_busy() || app.get_transcribing() {
            return;
        }
        let recording = { state.borrow_mut().recording.take() };
        if let Some(recording) = recording {
            let mut recording = recording;
            recording.stop.store(true, Ordering::Release);
            let session = std::mem::take(&mut recording.session);
            let generation = recording.generation;
            let worker = recording.worker.take();
            app.set_recording(false);
            app.set_transcribing(true);
            app.set_recording_elapsed("".into());
            app.set_status("正在识别录音…".into());
            let (generation_counter, desktop) = {
                let current = state.borrow();
                (current.history_generation.clone(), current.desktop.clone())
            };
            if let Some(worker) = worker {
                finish_voice_recording(
                    weak.clone(),
                    generation_counter,
                    desktop,
                    session,
                    generation,
                    worker,
                );
            }
            return;
        }
        if app.get_active_session_id().is_empty() {
            app.set_status("请先创建或选择一个会话".into());
            return;
        }
        let desktop = state.borrow().desktop.clone();
        if !desktop.has_asr_model() {
            app.set_status("请先在系统设置中配置默认语音识别模型".into());
            return;
        }
        let generation = state
            .borrow()
            .history_generation
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1);
        let session = app.get_active_session_id().to_string();
        let stop = Arc::new(AtomicBool::new(false));
        let worker = crate::audio_recording::spawn(stop.clone());
        state.borrow_mut().recording = Some(Recording {
            stop: stop.clone(),
            worker: Some(worker),
            generation,
            session,
            started: Instant::now(),
        });
        app.set_recording(true);
        app.set_recording_elapsed("00:00".into());
        app.set_status("正在录音，再次点击停止".into());
        start_recording_clock(weak.clone(), state.clone());
        // Device I/O starts after the UI state changes so a slow driver cannot
        // block the first visual feedback.
    });
}

fn start_recording_clock(app: slint::Weak<MainWindow>, state: Rc<RefCell<State>>) {
    slint::Timer::single_shot(Duration::from_millis(250), move || {
        let Some(app) = app.upgrade() else { return };
        let (stop, elapsed, pulse) = {
            let current = state.borrow();
            let Some(recording) = current.recording.as_ref() else {
                return;
            };
            let millis = recording.started.elapsed().as_millis() as u64;
            (
                recording.stop.clone(),
                (millis / 1000).min(crate::audio_recording::MAX_RECORDING_SECONDS),
                // Half-period 600ms; the UI animates between phases so the
                // outline reads as a continuous ~1.2s breathing loop.
                (millis / 600) % 2 == 0,
            )
        };
        app.set_recording_pulse(pulse);
        app.set_recording_elapsed(format!("{:02}:{:02}", elapsed / 60, elapsed % 60).into());
        if elapsed >= crate::audio_recording::MAX_RECORDING_SECONDS {
            stop.store(true, Ordering::Release);
            app.invoke_record_voice();
            return;
        }
        start_recording_clock(app.as_weak(), state.clone());
    });
}

fn finish_voice_recording(
    app: slint::Weak<MainWindow>,
    generation_counter: Arc<AtomicU64>,
    desktop: Arc<NativeDesktop>,
    session: String,
    generation: u64,
    worker: std::thread::JoinHandle<Result<crate::audio_recording::RecordedAudio, String>>,
) {
    // The recorder owns device cleanup. Join it on a worker before invoking
    // ASR so both operations remain off Slint's event loop.
    std::thread::spawn(move || {
        let result = worker
            .join()
            .map_err(|_| "录音线程异常退出".to_string())
            .and_then(|result| result)
            .and_then(|audio| {
                desktop
                    .transcribe_audio(audio.filename, audio.content_type, audio.bytes)
                    .map_err(|error| error.to_string())
            });
        let _ = app.upgrade_in_event_loop(move |app| {
            app.set_transcribing(false);
            if app.get_active_session_id() != session
                || generation_counter.load(Ordering::Relaxed) != generation
            {
                return;
            }
            match result {
                Ok(text) => {
                    let separator = if app.get_draft().trim().is_empty() {
                        ""
                    } else {
                        "\n"
                    };
                    let next = format!("{}{}{}", app.get_draft(), separator, text.trim());
                    app.set_draft(next.into());
                    app.set_status("语音已转写到输入区，可编辑后发送".into());
                }
                Err(error) => app.set_status(format!("语音识别失败：{error}").into()),
            }
        });
    });
}

/// Load a bounded queue snapshot off the Slint event loop. Selection and queue
/// actions share this helper so a cold SQLite cache on an old HDD never delays
/// the click acknowledgement.
fn refresh_queue_async(app: &MainWindow, state: &Rc<RefCell<State>>, session: &str) {
    let session = session.trim().to_string();
    if session.is_empty() {
        state.borrow_mut().queue_cache.clear();
        queue::publish_rows(app, &[]);
        return;
    }
    let (desktop, generation) = {
        let current = state.borrow();
        (current.desktop.clone(), current.queue_generation.clone())
    };
    request_queue_refresh(app, desktop, session, generation);
}

fn request_queue_refresh(
    app: &MainWindow,
    desktop: Arc<NativeDesktop>,
    session: String,
    generation: Arc<AtomicU64>,
) {
    let request = generation.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let rows = desktop.list_queue_turns(&session).unwrap_or_default();
        let _ = weak.upgrade_in_event_loop(move |app| {
            if app.get_active_session_id() != session
                || generation.load(Ordering::Relaxed) != request
            {
                return;
            }
            CHAT_STATE.with(|slot| {
                let Some(state) = slot.borrow().upgrade() else {
                    return;
                };
                state.borrow_mut().queue_cache = rows.clone();
                queue::publish_rows(&app, &rows);
            });
        });
    });
}

/// 撤下一轮：取消引擎里那条还没开跑的轮次，并丢弃本窗口为它持有的订阅。
/// `refill` puts the text back into the input (编辑), otherwise it is gone.
/// The engine call is a SQLite write, so it runs on a worker; the click
/// returns immediately and the outcome lands through the event loop.
fn withdraw_queue_turn(
    app: &MainWindow,
    state: &Rc<RefCell<State>>,
    session: &str,
    queue_id: &str,
    refill: bool,
) {
    let (client_id, root_turn_id, desktop) = {
        let st = state.borrow();
        let row = st.queue_cache.iter().find(|row| row.queue_id == queue_id);
        (
            row.map(|row| row.client_message_id.clone())
                .unwrap_or_default(),
            row.map(|row| row.root_turn_id.clone()).unwrap_or_default(),
            st.desktop.clone(),
        )
    };
    let session = session.to_string();
    let queue_id = queue_id.to_string();
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = desktop.withdraw_queue_turn(&session, &queue_id);
        let _ = weak.upgrade_in_event_loop(move |app| {
            CHAT_STATE.with(|slot| {
                let Some(state) = slot.borrow().upgrade() else {
                    return;
                };
                let content = match result {
                    Ok(content) => Some(content),
                    Err(error) => {
                        app.set_status(format!("撤下失败：{error}").into());
                        None
                    }
                };
                if content.is_some() && !root_turn_id.is_empty() {
                    let mut st = state.borrow_mut();
                    if st.observation.remove_turn(&root_turn_id) {
                        let timeline = st.timeline.clone();
                        st.observation.publish(&app, &timeline);
                    }
                }
                if content.is_some() && !client_id.is_empty() {
                    let mut st = state.borrow_mut();
                    if let Some(index) = st
                        .parked
                        .iter()
                        .position(|entry| entry.client_message_id == client_id)
                    {
                        st.parked.remove(index);
                    }
                }
                refresh_queue_async(&app, &state, &session);
                // The draft belongs to the thread on screen; another selection
                // since the click means the text stays gone from the composer.
                if refill && app.get_active_session_id() == session {
                    if let Some(text) = content.filter(|text| !text.trim().is_empty()) {
                        let draft = app.get_draft().to_string();
                        let merged = if draft.trim().is_empty() {
                            text
                        } else {
                            format!("{draft}\n{text}")
                        };
                        app.set_draft(merged.into());
                        queue::reset_recall(&mut state.borrow_mut().recall);
                    }
                }
            });
        });
    });
}

/// The composer's queue strip and the ArrowUp/ArrowDown walk over the messages
/// this thread already sent.
fn bind_queue(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    let state_interject = state.clone();
    app.on_queue_interject(move |queue_id| {
        let Some(app) = weak.upgrade() else { return };
        let session = app.get_active_session_id().to_string();
        if session.is_empty() {
            return;
        }
        // 插话 is a SQLite write; run it on a worker and post the outcome back.
        let (desktop, generation) = {
            let st = state_interject.borrow();
            (st.desktop.clone(), st.queue_generation.clone())
        };
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = desktop.interject_queue_turn(&session, queue_id.as_str());
            let _ = weak.upgrade_in_event_loop(move |app| {
                match result {
                    Ok(()) => app.set_status("已插话，将在当前动作边界优先执行".into()),
                    Err(error) => app.set_status(format!("插话失败：{error}").into()),
                }
                request_queue_refresh(&app, desktop, session, generation);
            });
        });
    });

    let weak = app.as_weak();
    let state_remove = state.clone();
    app.on_queue_remove(move |queue_id| {
        let Some(app) = weak.upgrade() else { return };
        let session = app.get_active_session_id().to_string();
        if session.is_empty() {
            return;
        }
        withdraw_queue_turn(&app, &state_remove, &session, queue_id.as_str(), false);
    });

    let weak = app.as_weak();
    let state_edit = state.clone();
    app.on_queue_edit(move |queue_id| {
        let Some(app) = weak.upgrade() else { return };
        let session = app.get_active_session_id().to_string();
        if session.is_empty() {
            return;
        }
        withdraw_queue_turn(&app, &state_edit, &session, queue_id.as_str(), true);
    });

    let weak = app.as_weak();
    let state_reorder = state.clone();
    app.on_queue_move_by(move |queue_id, delta| {
        let Some(app) = weak.upgrade() else { return };
        let session = app.get_active_session_id().to_string();
        if session.is_empty() {
            return;
        }
        let ids = state_reorder
            .borrow()
            .queue_cache
            .iter()
            .map(|row| row.queue_id.clone())
            .collect::<Vec<_>>();
        let Some(next) = queue::move_within(&ids, queue_id.as_str(), delta) else {
            // The row vanished underneath the drag: take the order from the
            // engine instead of writing back a list it no longer recognises.
            refresh_queue_async(&app, &state_reorder, &session);
            return;
        };
        if next.is_empty() {
            return;
        }
        // 拖拽排序 writes the whole order back through SQLite; run it on a
        // worker and refresh the strip once the write lands.
        let (desktop, generation) = {
            let st = state_reorder.borrow();
            (st.desktop.clone(), st.queue_generation.clone())
        };
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = desktop.reorder_queue_turns(&session, &next);
            let _ = weak.upgrade_in_event_loop(move |app| {
                if let Err(error) = result {
                    app.set_status(format!("顺序未保存：{error}").into());
                }
                request_queue_refresh(&app, desktop, session, generation);
            });
        });
    });

    // The walk is a pure read over this thread's timeline, so it needs neither
    // the window nor the runtime: the draft it returns is what the editor shows.
    app.on_recall_history(move |direction, cursor, anchor, draft| {
        let untouched = DraftRecall {
            handled: false,
            text: draft.clone(),
            caret: draft.as_str().len() as i32,
        };
        let timeline = state.borrow().timeline.clone();
        let history = timeline.borrow().recent_user_texts(queue::RECALL_DEPTH);
        let mut st = state.borrow_mut();
        queue::recall_step(
            &mut st.recall,
            &history,
            direction,
            cursor,
            anchor,
            draft.as_str(),
        )
        .unwrap_or(untouched)
    });
}

// Mutate just the matching thread row. Runtime state must not wait for catalogue polling.
fn set_session_status(app: &MainWindow, session: &str, status: &str) {
    let rows = app.get_conversations();
    if let Some(index) = rows.iter().position(|row| row.id == session) {
        if let Some(mut row) = rows.row_data(index) {
            if row.runtime_status != status {
                row.runtime_status = status.into();
                rows.set_row_data(index, row);
                update_active_thread_filter_model(app, &rows);
                crate::navigation_ui::project(app);
            }
        }
    }
    // The sidebar tree snapshots the conversation list, so it needs its own
    // point update; the periodic catalogue poll is only a backstop, not the
    // path a live transition should wait for.
    crate::navigation_ui::update_thread_status(app, session, status);
    // The hot→done completion gate must see every report, including repeated
    // ones, so it sits outside the row-change branch above.
    crate::completion_notice::observe_status(app, session, status);
}

// Keep the work-dock activity control in sync without inspecting message text.
// This mirrors the web task list: running wins over pending, and only running
// or pending threads contribute to the count and optional list filter.
fn update_active_thread_filter(app: &MainWindow, rows: impl IntoIterator<Item = Conversation>) {
    let mut count = 0;
    let mut running_count = 0;
    let mut running = false;
    for row in rows {
        let status = runtime_status_kind(row.runtime_status.as_str());
        if status == "running" {
            running = true;
            running_count += 1;
            count += 1;
        } else if status == "pending" {
            count += 1;
        }
    }
    app.set_active_thread_count(count);
    app.set_title_running_count(running_count);
    app.set_active_thread_status(
        if running {
            "running"
        } else if count > 0 {
            "pending"
        } else {
            "idle"
        }
        .into(),
    );
}

fn update_active_thread_filter_model(app: &MainWindow, rows: &ModelRc<Conversation>) {
    update_active_thread_filter(app, rows.iter());
}

fn runtime_status_kind(status: &str) -> &'static str {
    if matches!(
        status,
        "pending" | "queued" | "waiting_input" | "waiting_user_input"
    ) || status.starts_with("正在排队")
        || status.starts_with("任务已排队")
        || status.starts_with("等待")
    {
        "pending"
    } else if matches!(status, "done" | "finished" | "completed" | "success")
        || status.starts_with("任务完成")
        || status.starts_with("输出已结束")
    {
        "done"
    } else if matches!(status, "running" | "streaming" | "executing")
        || status.starts_with("正在")
        || status.starts_with("目标")
    {
        "running"
    } else {
        "idle"
    }
}

fn bind_stop(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    app.on_stop_generation(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_cloud_chat_active() {
            crate::cloud_threads::cancel(&app, state.borrow().desktop.clone());
            return;
        }
        if app.get_stopping() {
            return;
        }
        let session = app.get_active_session_id().to_string();
        // Branch 1: a stream owned by this process is cancelled in place, so
        // the borrowed state must be released before the projection below. The
        // `current` guard lives to the end of this closure, hence the status is
        // handed out of the block instead of reprojected while it is held — a
        // second `borrow_mut`/`borrow` here aborts the process.
        let mut stopped: Option<(String, String)> = None;
        let mut stopped_submission = false;
        {
            let mut current = state.borrow_mut();
            if current
                .submitting
                .as_ref()
                .is_some_and(|pending| pending.session == session)
            {
                let pending = current.submitting.take().unwrap();
                pending.cancelled.store(true, Ordering::Release);
                current.cancelling_submission = Some(CancellingSubmission {
                    session: pending.session.clone(),
                    client_message_id: pending.client_message_id.clone(),
                    control_settled: false,
                    stream_settled: false,
                });
                let mut timeline = current.timeline.borrow_mut();
                timeline.remove_live_turn(&pending.client_message_id);
                publish_timeline_passive(&app, &timeline);
                stopped_submission = true;
            }
            if let Some(active) = current.active.as_mut() {
                app.set_stopping(true);
                active.stream.cancel();
                active.state = "已停止".into();
                active.timeline.borrow_mut().flush();
                stopped = Some((active.session.clone(), active.state.clone()));
            }
        }
        if let Some((stopped_session, stopped_status)) = stopped {
            set_session_status(&app, &stopped_session, &stopped_status);
            app.set_status("正在停止…".into());
        } else if stopped_submission || app.get_busy() || app.get_goal_active() {
            app.set_stopping(true);
            if stopped_submission {
                app.set_busy(false);
                app.set_activity_running(false);
                set_session_status(&app, &session, "已停止");
            }
            let desktop = state.borrow().desktop.clone();
            let cancel_session = session.clone();
            let weak = app.as_weak();
            let waits_for_submission_stream = stopped_submission;
            std::thread::spawn(move || {
                let result = desktop.cancel_chat(&cancel_session);
                let _ = weak.upgrade_in_event_loop(move |app| {
                    if app.get_active_session_id() == cancel_session {
                        // A stopped submission can return a stream only after
                        // this control request did. Keep the composer fenced
                        // until that stream was cancelled and drained too;
                        // otherwise its session-wide cancellation can hit a
                        // new request submitted in the gap.
                        if !waits_for_submission_stream {
                            app.set_stopping(false);
                        }
                        match result {
                            Err(error) => app.set_status(format!("无法停止：{error}").into()),
                            Ok(()) => {
                                app.set_goal_active(false);
                                app.invoke_refresh_chat();
                            }
                        }
                    }
                    CHAT_STATE.with(|slot| {
                        if let Some(state) = slot.borrow().upgrade() {
                            settle_cancelling_submission(
                                &app,
                                &state,
                                &cancel_session,
                                "",
                                true,
                                false,
                            );
                        }
                    });
                });
            });
        }
        // Cancelling a thread also cancels its queued turns, so this window
        // drops their subscribers. The strip is re-read off the UI thread: the
        // pump may already have stopped, and a stale row would invite a click
        // on a turn the engine no longer has.
        {
            let mut current = state.borrow_mut();
            current.parked.retain(|entry| entry.session != session);
        }
        let desktop = state.borrow().desktop.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            let rows = desktop.list_queue_turns(&session).unwrap_or_default();
            let _ = weak.upgrade_in_event_loop(move |app| {
                if app.get_active_session_id() == session {
                    queue::publish_rows(&app, &rows);
                }
            });
        });
    });
}

/// Banner clock text: leading zero units are dropped so a fresh goal reads
/// seconds only and a marathon one reads hours.
fn goal_elapsed(seconds: i64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let rest = seconds % 60;
    if hours > 0 {
        format!("{hours}小时{minutes}分{rest}秒")
    } else if minutes > 0 {
        format!("{minutes}分{rest}秒")
    } else {
        format!("{rest}秒")
    }
}

/// Every goal clock write goes through here so the numeric seconds and the
/// formatted label can never disagree.
pub(crate) fn set_goal_clock(app: &MainWindow, seconds: i32) {
    app.set_goal_seconds(seconds);
    app.set_goal_elapsed(goal_elapsed(seconds as i64).into());
}

/// Local one-second tick. The backend observer pushes durable goal updates,
/// but only while it is allowed to run; this keeps the banner counting during
/// a foreground generation and corrects itself on the next durable update.
pub(super) fn start_goal_clock(app: slint::Weak<MainWindow>) {
    let should_start = GOAL_CLOCK_ARMED.with(|armed| {
        if armed.get() {
            false
        } else {
            armed.set(true);
            true
        }
    });
    if should_start {
        tick_goal_clock(app);
    }
}

fn tick_goal_clock(app: slint::Weak<MainWindow>) {
    slint::Timer::single_shot(Duration::from_secs(1), move || {
        let Some(app) = app.upgrade() else { return };
        if !app.get_goal_active() || app.get_goal_paused() || app.get_goal_objective().is_empty() {
            // Do not keep a recursive one-second wake-up alive for every
            // ordinary idle thread.  A goal update explicitly arms the clock
            // again when it becomes active.
            GOAL_CLOCK_ARMED.with(|armed| armed.set(false));
            return;
        }
        set_goal_clock(&app, app.get_goal_seconds() + 1);
        tick_goal_clock(app.as_weak());
    });
}

/// Goal banner controls reuse the slash-command pipeline: the work runs on a
/// worker, the outcome lands as feedback plus a refresh, and the refresh or
/// the observer goal tick corrects the banner state.
fn run_goal_command(
    app: &MainWindow,
    state: Rc<RefCell<State>>,
    session: String,
    command: NativeChatCommand,
) {
    let desktop = state.borrow().desktop.clone();
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = desktop.execute_chat_command(&session, command);
        let _ = weak.upgrade_in_event_loop(move |app| {
            app.set_command_pending(false);
            if app.get_active_session_id() != session {
                return;
            }
            match result {
                Ok(message) => {
                    app.set_command_feedback(message.into());
                    app.invoke_refresh_chat();
                }
                Err(error) => app.set_command_feedback(format!("命令执行失败：{error}").into()),
            }
        });
    });
}

fn bind_goal(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    let state_save = state.clone();
    app.on_goal_save(move |objective| {
        let Some(app) = weak.upgrade() else { return };
        let objective = objective.trim().to_string();
        if objective.is_empty() {
            return;
        }
        let session = app.get_active_session_id().to_string();
        if session.is_empty() || app.get_command_pending() {
            return;
        }
        app.set_command_pending(true);
        app.set_goal_editing(false);
        run_goal_command(
            &app,
            state_save.clone(),
            session,
            NativeChatCommand::GoalEdit(objective),
        );
    });
    let weak = app.as_weak();
    let state_toggle = state.clone();
    app.on_goal_pause_toggle(move || {
        let Some(app) = weak.upgrade() else { return };
        let session = app.get_active_session_id().to_string();
        if session.is_empty() || app.get_command_pending() {
            return;
        }
        let command = if app.get_goal_paused()
            || app.get_goal_phase() == "blocked"
            || app.get_goal_activation() == "disarmed"
        {
            NativeChatCommand::GoalResume
        } else {
            NativeChatCommand::GoalPause
        };
        app.set_command_pending(true);
        run_goal_command(&app, state_toggle.clone(), session, command);
    });
    let weak = app.as_weak();
    app.on_goal_clear(move || {
        let Some(app) = weak.upgrade() else { return };
        let session = app.get_active_session_id().to_string();
        if session.is_empty() || app.get_command_pending() {
            return;
        }
        app.set_command_pending(true);
        app.set_goal_editing(false);
        run_goal_command(&app, state.clone(), session, NativeChatCommand::GoalClear);
    });
}

fn start_timer(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    // The timer belongs to State: a weak callback avoids retaining the runtime
    // after the native window closes during a stream.
    let callback_state = Rc::downgrade(&state);
    state
        .borrow()
        .timer
        .start(TimerMode::Repeated, Duration::from_millis(33), move || {
            let (Some(app), Some(state)) = (weak.upgrade(), callback_state.upgrade()) else {
                return;
            };
            let mut state = state.borrow_mut();
            // Cloned before the active borrow so the reducer can hold the
            // timeline while the stream is polled.
            let timeline = state.timeline.clone();
            let desktop = state.desktop.clone();
            let last_plan_sig = state.plan_sig.clone();
            // ---- parked submissions ----
            // A parked turn stays silent until the engine starts its own root
            // turn, so the first parked stream that speaks is the turn that just
            // began: it takes the live slot, and whatever it had already
            // emitted replays through the normal pump below.
            let active_session = app.get_active_session_id().to_string();
            let mut promote: Option<ParkedTurn> = None;
            let mut parked_moved = false;
            let mut index = 0usize;
            while index < state.parked.len() {
                let signal = match state.parked.get_mut(index) {
                    Some(entry) => drain_entry(entry, &active_session),
                    None => break,
                };
                match signal {
                    EntrySignal::Quiet => index += 1,
                    EntrySignal::Announced => {
                        queue::schedule_due(&mut state.queue_due, Duration::from_millis(250));
                        index += 1;
                    }
                    EntrySignal::Ready => {
                        if state.active.is_some() {
                            // The slot is still held: the event stays buffered
                            // in the entry and attaches when the turn settles.
                            break;
                        }
                        promote = state.parked.remove(index);
                        parked_moved = true;
                        break;
                    }
                    EntrySignal::Settled | EntrySignal::Failed(_) => {
                        if let EntrySignal::Failed(message) = signal {
                            app.set_status(message.into());
                        }
                        state.parked.remove(index);
                        parked_moved = true;
                        queue::schedule_due(&mut state.queue_due, Duration::from_millis(0));
                        break;
                    }
                }
            }
            let mut pending: VecDeque<NativeChatEvent> = VecDeque::new();
            if let Some(entry) = promote {
                let session = entry.session.clone();
                let content = entry.content.clone();
                pending = entry.prelude.into();
                {
                    let mut live = timeline.borrow_mut();
                    live.set_highlight(true);
                    live.begin_turn(&entry.client_message_id, &content);
                    live.set_turn_status("请求中", 0);
                    publish_timeline(&app, &live);
                }
                let mut output = TurnOutput::new(session.clone(), timeline.clone());
                if let Some((explanation, steps)) = state.plans.get(&session) {
                    output.plan_explanation = explanation.clone();
                    output.plan_steps = steps.clone();
                }
                state.active = Some(Active {
                    stream: entry.stream,
                    output,
                    client_message_id: entry.client_message_id,
                });
                app.set_busy(true);
                app.set_stopping(false);
                app.set_follow_output(true);
                app.set_activity_running(true);
                set_session_status(&app, &session, "正在生成…");
                queue::schedule_due(&mut state.queue_due, Duration::from_millis(0));
            }
            if parked_moved || queue::due_arrived(&mut state.queue_due) {
                // Promotion is the point at which the request owns the live
                // slot. Hide its queue card immediately; the next refresh will
                // still use the engine snapshot for every other row.
                let promoted_client_id = state
                    .active
                    .as_ref()
                    .map(|active| active.client_message_id.clone());
                if let Some(client_message_id) = promoted_client_id {
                    queue::without_client_id(&mut state.queue_cache, client_message_id.as_str());
                    queue::publish_rows(&app, &state.queue_cache);
                }
                let queue_generation = state.queue_generation.clone();
                request_queue_refresh(
                    &app,
                    desktop.clone(),
                    active_session.clone(),
                    queue_generation,
                );
            }
            // The turn is taken out of the state for the tick so the flush can
            // also touch the plan cache and signature without fighting the
            // active borrow; it goes back (or drops on completion) below.
            let mut live_turn = state.active.take();
            let Some(active) = live_turn.as_mut() else {
                // A parked subscriber of another thread can never be pumped
                // from here, and its bounded channel would only back-pressure
                // the engine, so it goes with the thread that was left behind.
                state.parked.retain(|entry| entry.session == active_session);
                if state.parked.is_empty() {
                    // Nothing left to pump: release the running state and stop
                    // the timer until the next submission.
                    if app.get_busy() {
                        app.set_busy(false);
                        app.set_activity_running(false);
                    }
                    state.timer.stop();
                }
                return;
            };
            let _ = &desktop;
            let started = Instant::now();
            let mut done = false;
            let mut dirty = false;
            let mut new_plan_sig: Option<String> = None;
            app.set_stream_max_backlog(
                app.get_stream_max_backlog()
                    .max(active.stream.pending_events() as i32),
            );
            for _ in 0..128 {
                // A promoted parked turn may already hold events; they replay
                // first so the handover never loses a token.
                let event = match pending.pop_front() {
                    Some(event) => event,
                    None => match active.stream.try_recv() {
                        Ok(Some(event)) => event,
                        Ok(None) => break,
                        Err(error) => {
                            app.set_status(error.into());
                            active.state = "执行失败".into();
                            active.settlement_failed = true;
                            done = true;
                            break;
                        }
                    },
                };
                match event {
                    NativeChatEvent::Event(event) => {
                        update_active_stats(active, &event);
                        apply_context_usage(
                            &app,
                            wunder_desktop::NativeContextUsage::from_value(&event),
                        );
                        // Cloud queue visibility (§4.4.4): `cloud_queue`
                        // incremental events carry the waiting position for the
                        // turn; any other streamed event means the turn is
                        // progressing again, so the banner hides.
                        if event["event"].as_str() == Some("cloud_queue") {
                            let data = &event["data"];
                            let position = data["position"].as_i64().unwrap_or(0);
                            let queue_session = data["session_id"].as_str().unwrap_or("");
                            if queue_session.is_empty() || queue_session == active.session {
                                app.set_cloud_queue_position(
                                    position.clamp(0, i32::MAX as i64) as i32
                                );
                                active.state = format!("模型请求正在排队 · 前方 {position} 个任务");
                            }
                        } else if app.get_cloud_queue_position() >= 0 {
                            app.set_cloud_queue_position(-1);
                        }
                        // The 问询面板 tool publishes its questions once and the
                        // turn ends right after, so the card is driven straight
                        // from this event rather than from the per-frame flush.
                        // The stream wraps every payload as
                        // `{session_id, timestamp, data}`, so the questions sit
                        // one level deeper than the raw emit.
                        if event["event"].as_str() == Some("question_panel") {
                            if let Some(panel) = inquiry_from_payload(event_data(&event)) {
                                let session = active.session.clone();
                                // One live panel per thread, and threads are
                                // bounded by the conversation list; drop the
                                // oldest extras rather than grow without limit.
                                if state.inquiries.len() > 64 {
                                    state.inquiries.clear();
                                }
                                let mut panel = panel;
                                panel.inquiry_turn_id = event_data(&event)
                                    .get("turn_id")
                                    .and_then(Value::as_str)
                                    .or_else(|| event.get("turn_id").and_then(Value::as_str))
                                    .unwrap_or_default()
                                    .to_string();
                                let key = panel.consumption_key(&session);
                                if !inquiry_was_consumed(&state, &key) {
                                    state.inquiries.insert(session.clone(), panel);
                                }
                                app.set_title_inquiry_count(state.inquiries.len() as i32);
                                if app.get_active_session_id() == session.as_str() {
                                    publish_inquiry(&app, &session, &state);
                                }
                            }
                        } else if event["event"].as_str() == Some("question_panel_clear") {
                            let session = active.session.clone();
                            state.inquiries.remove(&session);
                            if app.get_active_session_id() == session.as_str() {
                                publish_inquiry(&app, &session, &state);
                            }
                        }
                        match apply_event(active, &mut timeline.borrow_mut(), &event) {
                            Ok(changed) => dirty |= changed,
                            Err(error) => {
                                app.set_status(error.into());
                                active.state = "执行失败".into();
                                active.settlement_failed = true;
                                done = true;
                            }
                        }
                    }
                    NativeChatEvent::Queued => {
                        active.state = "任务已排队…".into();
                        dirty = true;
                        // Another device (or a goal round) held the thread, so
                        // this turn went into the queue after all: show it.
                        queue::schedule_due(&mut state.queue_due, Duration::from_millis(0));
                    }
                    NativeChatEvent::Failed(error) => {
                        app.set_status(error.into());
                        active.state = "执行失败".into();
                        active.settlement_failed = true;
                        done = true;
                        app.set_cloud_queue_position(-1);
                    }
                    NativeChatEvent::Finished => {
                        done = true;
                        app.set_cloud_queue_position(-1);
                    }
                }
                if done || started.elapsed() > Duration::from_millis(5) {
                    break;
                }
            }
            if dirty || done {
                // One frame flush: the active tail block and the measured
                // cells, never the whole history.
                let mut live = timeline.borrow_mut();
                live.flush();
                live.set_turn_status(&active.state, active.user_round);
                publish_timeline(&app, &live);
                app.set_stream_bytes(live.body_text().len().min(i32::MAX as usize) as i32);
                drop(live);
                set_session_status(&app, &active.session, &active.state);
                // Status dock: capsule state plus plan progress and file
                // stats; the session plan cache only moves when the plan did.
                let sig = publish_activity(&app, active, &last_plan_sig);
                if sig != last_plan_sig {
                    state.plans.insert(
                        active.session.clone(),
                        (active.plan_explanation.clone(), active.plan_steps.clone()),
                    );
                }
                new_plan_sig = Some(sig);
            }
            if done {
                if active.state == "正在生成…" {
                    active.state = "输出已结束".into();
                }
                let failed = active.failed();
                // Markdown is intentionally parsed once, after the terminal
                // event; the frozen block keeps its parsed form from here on.
                let mut live = timeline.borrow_mut();
                live.finish(failed);
                live.set_turn_status(&active.state, active.user_round);
                live.flush();
                publish_timeline(&app, &live);
                drop(live);
                set_session_status(&app, &active.session, &active.state);
                app.set_stopping(false);
                // Idle stops the capsule spinner; the plan attributes stay and
                // reappear with the next turn's flush.
                if state.parked.is_empty() {
                    app.set_busy(false);
                    app.set_activity_running(false);
                    state.timer.stop();
                }
                // With parked submissions still waiting the pump keeps running:
                // the engine starts the successor as soon as this lease drops,
                // and the next tick hands it the live slot.
                crate::workspace_files::execution_finished(&app, desktop.clone());
            }
            if done {
                live_turn = None;
            }
            state.active = live_turn;
            if let Some(sig) = new_plan_sig {
                state.plan_sig = sig;
            }
            app.set_stream_max_ui_ms(
                app.get_stream_max_ui_ms()
                    .max(started.elapsed().as_secs_f32() * 1000.0),
            );
        });
}

/// Reduce one runtime event into the live turn. `output` carries the turn's
/// statistics and identity, `timeline` the rows; they are passed separately so
/// a single call holds one mutable borrow of each.
fn apply_event(
    active: &mut TurnOutput,
    timeline: &mut crate::timeline::Timeline,
    event: &Value,
) -> Result<bool, String> {
    let raw_kind = event["event"].as_str().unwrap_or("");
    let envelope = &event["data"];
    let data = if envelope.get("tool").is_some() || envelope.get("tool_name").is_some() {
        envelope
    } else {
        envelope.get("data").unwrap_or(envelope)
    };
    // The runtime collapses every online *_delta frame into thread_item_delta
    // and keeps the semantic type in data.source_event.
    let kind = if raw_kind == "thread_item_delta" {
        data["source_event"].as_str().unwrap_or(raw_kind)
    } else {
        raw_kind
    };
    // Subagent control events carry the child's session_id, so the plain
    // session filter below would drop them; they belong to the parent turn and
    // say so through parent_session_id when one is present.
    let subagent_control = matches!(
        raw_kind,
        "subagent_dispatch_item_update"
            | "subagent_status"
            | "subagent_dispatch_finish"
            | "subagent_close"
            | "subagent_interrupt"
            | "subagent_resume"
    );
    if subagent_control {
        if data["parent_session_id"]
            .as_str()
            .is_some_and(|id| id != active.session)
        {
            return Ok(false);
        }
    } else if data["session_id"]
        .as_str()
        .is_some_and(|id| id != active.session)
    {
        return Ok(false);
    }
    if active.state == "已停止"
        || (active.terminal
            && kind != "native_execution_started"
            && (active.failed() || kind != "final"))
    {
        return Ok(false);
    }
    if let Some(round) = data["user_round"]
        .as_i64()
        .or_else(|| envelope["user_round"].as_i64())
        .filter(|round| *round > 0)
    {
        active.user_round = round;
    }
    let round = data["model_round"].as_i64().unwrap_or(active.round);
    if round < active.round
        && matches!(
            kind,
            "llm_output_delta" | "llm_output" | "delta" | "reasoning" | "reasoning_delta"
        )
    {
        return Ok(false);
    }
    // A new model round ends the previous round's segments. Some providers
    // begin the next round with reasoning directly (without an intervening
    // llm_request), so this boundary must apply to reasoning as well as text.
    if round > active.round && active.round > 0 {
        timeline.close_segments();
    }
    if matches!(
        kind,
        "llm_output_delta"
            | "llm_output"
            | "llm_request"
            | "delta"
            | "final"
            | "reasoning"
            | "reasoning_delta"
    ) {
        active.round = active.round.max(round);
    }
    match kind {
        "native_turn_identity" | "native_execution_identity" => {
            if let Some(root) = data["root_turn_id"].as_str() {
                timeline.set_process_root(root);
            }
            return Ok(true);
        }
        "reasoning" | "reasoning_delta" => {
            if let Some(delta) = data["delta"]
                .as_str()
                .or_else(|| data["reasoning_delta"].as_str())
            {
                timeline.append_reasoning(delta)?;
                active.state = "正在思考…".into();
                return Ok(true);
            }
        }
        "native_execution_started" => {
            active.terminal = false;
            active.settlement_failed = false;
            if let Some(root) = data["root_turn_id"].as_str() {
                timeline.set_process_root(root);
            }
            // Background goal executions restart model_round at one while the
            // timeline keeps the earlier entries of the same user turn.
            active.round = 0;
            active.body_round = -1;
            timeline.close_segments();
            active.state = "目标继续执行…".into();
        }
        "native_execution_status" => {
            if let Some(root) = data["root_turn_id"].as_str() {
                timeline.set_process_root(root);
            }
            active.settlement_failed = matches!(
                data["status"].as_str(),
                Some("failed" | "error" | "rejected")
            );
            active.state = match data["status"].as_str() {
                Some("queued") => "正在排队",
                Some("running") => "请求中",
                Some("cancelled" | "interrupted") => "已停止",
                Some("failed") => "执行失败",
                Some("waiting_input" | "waiting_user_input") => "等待用户输入",
                _ => "目标继续执行…",
            }
            .into();
        }
        "native_stats" => {
            // Background/native replay emits a compact stats snapshot as a
            // dedicated event.  It is already the stats object itself (unlike
            // llm_output/final, where it lives under `message_stats`), so do
            // not discard it.  Keeping this path in the reducer makes the
            // live footer match the durable footer before the watcher catches
            // up with the committed assistant item.
            let stats = crate::turn_stats::message_stats(data).unwrap_or(data);
            if !stats.is_null() {
                timeline.set_turn_stats(crate::timeline::stat_metrics(
                    &crate::turn_stats::with_user_round(
                        crate::turn_stats::metrics(stats),
                        active.user_round,
                    ),
                ));
                timeline.set_user_round(active.user_round);
            }
            return Ok(true);
        }
        "llm_request" => {
            // Admission of a model call: the answer block belongs to a new
            // round, so the previous segments settle here.
            timeline.close_segments();
            active.state = "模型响应中".into();
            return Ok(true);
        }
        "llm_output_delta" | "delta" => {
            let mut changed = false;
            if let Some(reasoning) = data["reasoning_delta"]
                .as_str()
                .filter(|value| !value.is_empty())
            {
                timeline.append_reasoning(reasoning)?;
                active.state = "正在思考…".into();
                changed = true;
            }
            if let Some(delta) = data["delta"].as_str() {
                if delta.is_empty() {
                    return Ok(changed);
                }
                if active.round != active.body_round {
                    timeline.start_body(active.round);
                    active.body_round = active.round;
                }
                timeline.append_body(delta)?;
                active.state = "正在生成…".into();
                return Ok(true);
            }
            return Ok(changed);
        }
        "llm_output" | "final" => {
            if let Some(text) = data["answer"].as_str().or_else(|| data["content"].as_str()) {
                if active.round != active.body_round {
                    timeline.start_body(active.round);
                    active.body_round = active.round;
                }
                timeline.replace_body(text)?;
            }
            // The runtime rides the turn's statistics on the final message, so
            // the footer appears as soon as the answer does rather than only
            // after a reload. Intermediate deltas carry nothing here.
            if let Some(stats) = crate::turn_stats::message_stats(data) {
                timeline.set_turn_stats(crate::timeline::stat_metrics(
                    &crate::turn_stats::with_user_round(
                        crate::turn_stats::metrics(stats),
                        active.user_round,
                    ),
                ));
            } else if kind == "final" {
                timeline.set_user_round(active.user_round);
            }
            if kind == "final" && !active.terminal {
                active.settlement_failed = false;
                active.state = "任务完成".into();
            }
            return Ok(true);
        }
        "turn_terminal" => {
            active.terminal = true;
            active.settlement_failed = matches!(
                data["status"].as_str(),
                Some("failed" | "error" | "rejected")
            );
            // A terminal event may be the only place a very short turn's
            // statistics arrive.
            if let Some(stats) = crate::turn_stats::message_stats(data) {
                timeline.set_turn_stats(crate::timeline::stat_metrics(
                    &crate::turn_stats::with_user_round(
                        crate::turn_stats::metrics(stats),
                        active.user_round,
                    ),
                ));
            } else {
                timeline.set_user_round(active.user_round);
            }
            active.state = match data["status"].as_str() {
                Some("cancelled" | "canceled" | "interrupted" | "stopped") => "已停止",
                Some("failed" | "error" | "rejected") => "执行失败",
                Some("waiting_input" | "waiting_user_input") => "等待用户输入",
                _ => "任务完成",
            }
            .into();
        }
        "queued" | "queue_update" => {
            active.state = data["queue_ahead"]
                .as_i64()
                .map(|ahead| format!("模型请求正在排队 · 前方 {ahead} 个任务"))
                .unwrap_or_else(|| "模型请求正在排队".into());
        }
        "queue_start" => active.state = "正在生成…".into(),
        "compaction" | "compaction_completed" | "compaction_start" | "compaction_progress" => {
            let summary = data["summary_text"].as_str().unwrap_or("上下文压缩中…");
            let running = matches!(kind, "compaction_start" | "compaction_progress")
                || matches!(data["status"].as_str(), Some("running"));
            if running {
                active.state = "上下文压缩中…".into();
            }
            active.upsert_workflow(
                timeline,
                NativeWorkflowEntry::from_payload(
                    data["item_id"]
                        .as_str()
                        .or_else(|| data["compaction_id"].as_str())
                        .unwrap_or("compaction"),
                    "上下文压缩",
                    summary.into(),
                    if matches!(kind, "compaction_start" | "compaction_progress") {
                        "running"
                    } else {
                        data["status"].as_str().unwrap_or("completed")
                    },
                    data,
                ),
            );
        }
        "queue_finish" => active.state = "任务完成".into(),
        "plan_update" => {
            // The plan tool publishes the whole checklist each time, so a
            // wholesale replace is correct. Bounded: a runaway plan must not
            // grow the dock model without limit.
            active.plan_explanation = data["explanation"].as_str().unwrap_or_default().to_string();
            active.plan_steps = data["plan"]
                .as_array()
                .map(|steps| {
                    steps
                        .iter()
                        .take(32)
                        .filter_map(|entry| {
                            let step = entry["step"].as_str()?;
                            Some((
                                step.to_string(),
                                entry["status"].as_str().unwrap_or("pending").to_string(),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default();
        }
        "tool_call" | "tool_start" | "tool_result" | "tool_output" => {
            let pending = matches!(kind, "tool_call" | "tool_start");
            if pending {
                active.state = "正在执行工具…".into();
            }
            let id = data
                .get("tool_call_id")
                .or_else(|| data.get("item_id"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let tool = data["tool"]
                .as_str()
                .or_else(|| data["tool_name"].as_str())
                .unwrap_or("工具");
            let detail = event
                .get("display_result")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    wunder_server::tool_result_display::tool_result_display(tool, data, pending)
                });
            let failed = data["success"] == false
                || data["ok"] == false
                || data["result"]["ok"] == false
                || data["result"]["success"] == false;
            active.upsert_workflow(
                timeline,
                NativeWorkflowEntry::from_payload(
                    id,
                    tool,
                    detail,
                    if pending {
                        "running"
                    } else if failed {
                        "failed"
                    } else {
                        "completed"
                    },
                    data,
                ),
            );
        }
        "command_session_start"
        | "command_session_delta"
        | "command_session_status"
        | "command_session_exit"
        | "command_session_summary" => {
            // The runtime streams a running shell command as command_session_*
            // frames (id in `command_session_id`, one per `command_index`). Fold
            // them into one live workflow row so the output grows while the
            // command runs instead of appearing only with the settled result.
            let session = data["command_session_id"].as_str().unwrap_or("");
            if !session.is_empty() {
                let index = data["command_index"].as_i64().unwrap_or(0);
                let key = format!("{session}#{index}");
                let mut output = active.command_output_text(&key);
                if let Some(delta) = data["delta"].as_str() {
                    output = active.append_command_output(&key, delta);
                }
                let command = data["command"].as_str().unwrap_or("");
                let terminal = matches!(kind, "command_session_exit" | "command_session_summary");
                let exit_code = data["exit_code"].as_i64();
                let failed = exit_code.map(|code| code != 0).unwrap_or(false)
                    || data["success"] == false
                    || data["ok"] == false
                    || matches!(data["status"].as_str(), Some("failed" | "failed_to_start"));
                let cancelled = matches!(data["status"].as_str(), Some("cancelled" | "canceled"));
                let status_label = if cancelled {
                    "已取消"
                } else if terminal && failed {
                    "失败"
                } else if terminal {
                    "完成"
                } else {
                    "运行中"
                };
                let mut lines = vec![format!("执行 · {status_label}")];
                if !command.is_empty() {
                    lines.push(command.chars().take(240).collect());
                }
                if !output.is_empty() {
                    lines.push(bound_command_output(output.as_str()));
                }
                if let Some(code) = exit_code {
                    lines.push(format!("exit {code}"));
                }
                let mut command_payload = data.clone();
                command_payload["args"] = serde_json::json!({"command": command});
                command_payload["result"] = serde_json::json!({"data": {
                    "command": command, "stdout": bound_command_output(&output), "exit_code": exit_code
                }});
                active.upsert_workflow(
                    timeline,
                    NativeWorkflowEntry::from_payload(
                        key.as_str(),
                        "execute_command",
                        lines.join("\n"),
                        if terminal {
                            if failed {
                                "failed"
                            } else {
                                "completed"
                            }
                        } else {
                            "running"
                        },
                        &command_payload,
                    ),
                );
                active.state = "正在执行工具…".into();
            }
        }
        "subagent_run" => {
            // One child run, one stable item: the card patches in place as the
            // runtime republishes it. The child's own status rules the row, so
            // the parent turn's state never rewrites it.
            let card = wunder_desktop::NativeSubagentCard::from_payload(data);
            match card {
                Some(card) => {
                    timeline.upsert_subagent(&card);
                }
                None => return Ok(false),
            }
        }
        "subagent_dispatch_item_update"
        | "subagent_status"
        | "subagent_dispatch_finish"
        | "subagent_close"
        | "subagent_interrupt"
        | "subagent_resume" => {
            // Child cards must show up the moment the parent spawns them, not
            // when the durable `subagent_run` item replays at turn end. The
            // control events carry the same identity either flat (dispatch,
            // interrupt, close) or under `items` (status / wait summaries);
            // both project into the same session-keyed card.
            let items: Vec<Value> = match data["items"].as_array() {
                Some(list) => list
                    .iter()
                    .filter(|item| item.get("session_id").is_some())
                    .cloned()
                    .collect(),
                None => vec![data.clone()],
            };
            let mut changed = false;
            for item in &items {
                if let Some(card) = wunder_desktop::NativeSubagentCard::from_dispatch_item(item) {
                    timeline.upsert_subagent(&card);
                    changed = true;
                }
            }
            if !changed {
                return Ok(false);
            }
        }
        "error" | "queue_fail" => {
            active.settlement_failed = data["will_retry"].as_bool() != Some(true);
            active.state = if active.settlement_failed {
                "执行失败"
            } else {
                "正在重试…"
            }
            .into();
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// Live command output is retained as a bounded tail; a watcher wants the newest
/// bytes and an unbounded buffer would keep a chatty command's row growing.
const COMMAND_OUTPUT_LIVE_LIMIT: usize = 4096;

/// Mirror of the runtime's result preview: keep the first 16 lines under 2400
/// chars and mark a truncation, so a live command row stays a readable excerpt.
fn bound_command_output(text: &str) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(2400).collect();
    let truncated = chars.next().is_some();
    let mut lines = head.lines();
    let mut output = lines.by_ref().take(16).collect::<Vec<_>>().join("\n");
    if truncated || lines.next().is_some() {
        output.push_str("\n…");
    }
    output
}

fn event_data(event: &Value) -> &Value {
    let envelope = event.get("data").unwrap_or(event);
    envelope.get("data").unwrap_or(envelope)
}

fn stat_number(value: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_f64))
}

/// Interaction duration is the only per-turn number the timeline still shows;
/// every other statistic moved to the settings pages and thread log.
fn update_active_stats(active: &mut TurnOutput, event: &Value) {
    let data = event_data(event);
    let mut sources = vec![data];
    for key in ["stats", "usage", "round_usage", "context_usage"] {
        if let Some(value) = data.get(key) {
            sources.push(value);
        }
    }
    if let Some(seconds) = sources
        .iter()
        .find_map(|source| {
            stat_number(
                source,
                &["interaction_duration_s", "duration_s", "elapsed_s"],
            )
        })
        .filter(|value| *value > 0.0)
    {
        active.stats_duration = format_duration_value(seconds);
    }
}
fn format_duration_value(seconds: f64) -> String {
    if seconds < 60.0 {
        format!("{seconds:.1}s")
    } else {
        format!("{}m {:.0}s", (seconds / 60.0).floor(), seconds % 60.0)
    }
}

fn reset_context_usage(app: &MainWindow) {
    app.set_context_used(-1.0);
    app.set_context_capacity(-1.0);
    app.set_context_usage(0.0);
    app.set_context_known(false);
    app.set_context_counts("".into());
}

fn apply_context_usage(app: &MainWindow, usage: wunder_desktop::NativeContextUsage) {
    if usage.used.is_none() && usage.capacity.is_none() {
        return;
    }
    if let Some(used) = usage.used {
        app.set_context_used(used as f32);
    }
    if let Some(capacity) = usage.capacity {
        app.set_context_capacity(capacity as f32);
    }
    let used = app.get_context_used();
    let capacity = app.get_context_capacity();
    app.set_context_known(used >= 0.0 && capacity > 0.0);
    app.set_context_usage(if app.get_context_known() {
        (used / capacity).clamp(0.0, 1.0)
    } else {
        0.0
    });
    let count = |value: f32| {
        if value >= 0.0 {
            format_count_i64(value as i64)
        } else {
            "—".into()
        }
    };
    app.set_context_counts(format!("{} / {} tokens", count(used), count(capacity)).into());
}

fn format_time(value: f64) -> String {
    if value > 0.0 {
        "刚刚".to_string()
    } else {
        String::new()
    }
}

fn format_count_i64(value: i64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}m", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        value.max(0).to_string()
    }
}

#[cfg(test)]
mod turn_tests {
    use super::*;

    #[test]
    fn cancelled_terminal_settles_open_tool_without_a_success_mark() {
        let timeline = Rc::new(RefCell::new(crate::timeline::Timeline::new()));
        let mut output = TurnOutput::new("fixture-session".into(), timeline.clone());
        let mut live = timeline.borrow_mut();
        live.begin_turn("fixture-root", "Fixture input");
        apply_event(&mut output, &mut live, &serde_json::json!({
            "event":"tool_call", "data":{"tool_call_id":"fixture-call","tool":"fixture_tool","args":{}}
        })).unwrap();
        apply_event(
            &mut output,
            &mut live,
            &serde_json::json!({
                "event":"turn_terminal", "data":{"status":"cancelled"}
            }),
        )
        .unwrap();
        live.finish(output.failed());
        live.set_turn_status(&output.state, 1);
        let rows: Vec<_> = live.model().iter().collect();
        assert_eq!(output.state, "已停止");
        assert!(rows
            .iter()
            .all(|row| row.status_kind != crate::timeline::STATUS_RUNNING));
        assert!(rows.iter().any(|row| row.kind == crate::timeline::KIND_TOOL
            && row.status_kind == crate::timeline::STATUS_FAILED));
    }

    #[test]
    fn error_message_language_cannot_turn_failure_into_success() {
        let timeline = Rc::new(RefCell::new(crate::timeline::Timeline::new()));
        let mut output = TurnOutput::new("fixture-session".into(), timeline.clone());
        let mut live = timeline.borrow_mut();
        live.begin_turn("fixture-root", "Fixture input");
        apply_event(
            &mut output,
            &mut live,
            &serde_json::json!({
                "event":"error", "data":{"message":"fixture transport error"}
            }),
        )
        .unwrap();
        assert!(output.failed());
    }

    #[test]
    fn failed_terminal_rejects_late_output_without_reopening_the_turn() {
        let timeline = Rc::new(RefCell::new(crate::timeline::Timeline::new()));
        let mut output = TurnOutput::new("fixture-session".into(), timeline.clone());
        let mut live = timeline.borrow_mut();
        live.begin_turn("fixture-root", "Fixture input");
        apply_event(
            &mut output,
            &mut live,
            &serde_json::json!({
                "event":"turn_terminal", "data":{"status":"failed"}
            }),
        )
        .unwrap();
        assert!(!apply_event(
            &mut output,
            &mut live,
            &serde_json::json!({
                "event":"final", "data":{"answer":"fixture late answer"}
            })
        )
        .unwrap());
        assert!(output.failed());
        assert_eq!(output.state, "执行失败");
    }

    #[test]
    fn completed_turn_does_not_invent_success_for_an_unreported_tool() {
        let timeline = Rc::new(RefCell::new(crate::timeline::Timeline::new()));
        let mut output = TurnOutput::new("fixture-session".into(), timeline.clone());
        let mut live = timeline.borrow_mut();
        live.begin_turn("fixture-root", "Fixture input");
        apply_event(&mut output, &mut live, &serde_json::json!({
            "event":"tool_call", "data":{"tool_call_id":"fixture-call","tool":"fixture_tool","args":{}}
        })).unwrap();
        apply_event(
            &mut output,
            &mut live,
            &serde_json::json!({
                "event":"turn_terminal", "data":{"status":"completed"}
            }),
        )
        .unwrap();
        live.finish(output.failed());
        assert!(live
            .model()
            .iter()
            .any(|row| row.kind == crate::timeline::KIND_TOOL
                && row.status_kind == crate::timeline::STATUS_FAILED));
        assert!(!output.failed());
    }

    #[test]
    fn explicit_execution_start_allows_a_goal_successor_after_completion() {
        let timeline = Rc::new(RefCell::new(crate::timeline::Timeline::new()));
        let mut output = TurnOutput::new("fixture-session".into(), timeline.clone());
        let mut live = timeline.borrow_mut();
        live.begin_turn("fixture-root", "Fixture input");
        apply_event(
            &mut output,
            &mut live,
            &serde_json::json!({
                "event":"turn_terminal", "data":{"status":"completed"}
            }),
        )
        .unwrap();
        assert!(apply_event(&mut output, &mut live, &serde_json::json!({
            "event":"native_execution_started", "data":{"root_turn_id":"fixture-root","turn_id":"fixture-next"}
        })).unwrap());
        assert!(apply_event(
            &mut output,
            &mut live,
            &serde_json::json!({
                "event":"llm_output_delta", "data":{"model_round":1,"delta":"fixture next answer"}
            })
        )
        .unwrap());
        live.flush();
        assert!(live
            .model()
            .iter()
            .any(|row| row.text == "fixture next answer"));
    }

    #[test]
    fn final_receipt_after_durable_terminal_keeps_answer_and_waiting_status() {
        for status in ["completed", "waiting_input"] {
            let timeline = Rc::new(RefCell::new(crate::timeline::Timeline::new()));
            let mut output = TurnOutput::new("fixture-session".into(), timeline.clone());
            let mut live = timeline.borrow_mut();
            live.begin_turn("fixture-root", "Fixture input");
            apply_event(
                &mut output,
                &mut live,
                &serde_json::json!({
                    "event":"turn_terminal", "data":{"status":status}
                }),
            )
            .unwrap();
            let expected = output.state.clone();
            assert!(apply_event(
                &mut output,
                &mut live,
                &serde_json::json!({
                    "event":"final", "data":{"answer":"fixture final answer"}
                })
            )
            .unwrap());
            live.flush();
            assert!(live
                .model()
                .iter()
                .any(|row| row.text == "fixture final answer"));
            assert_eq!(output.state, expected);
        }
    }

    #[test]
    fn runtime_reasoning_envelope_updates_the_live_process_before_body_or_final() {
        let timeline = Rc::new(RefCell::new(crate::timeline::Timeline::new()));
        let mut output = TurnOutput::new("fixture-session".into(), timeline.clone());
        let mut live = timeline.borrow_mut();
        live.begin_turn("fixture-root", "Fixture input");
        for (index, delta) in ["First thought", " continued"].iter().enumerate() {
            assert!(apply_event(
                &mut output,
                &mut live,
                &serde_json::json!({
                    "event":"thread_item_delta", "data":{"session_id":"fixture-session", "data":{
                        "source_event":"llm_output_delta", "model_round":1, "reasoning_delta":delta
                    }}
                })
            )
            .unwrap());
            live.flush();
            let bar = live
                .model()
                .iter()
                .find(|row| row.kind == crate::timeline::KIND_PROCESS)
                .unwrap();
            assert!(bar.visible);
            assert!(bar.process_local);
            assert!(!bar.process_paged);
            assert_eq!(bar.status_kind, crate::timeline::STATUS_RUNNING);
            assert_eq!(bar.process_entries.row_count(), 1);
            let reason = bar.process_entries.row_data(0).unwrap();
            assert_eq!(reason.status_kind, crate::timeline::STATUS_RUNNING);
            assert_eq!(
                reason.detail,
                if index == 0 {
                    "First thought"
                } else {
                    "First thought continued"
                }
            );
            assert_eq!(live.answer_count(), 0);
        }
    }

    #[test]
    fn continuation_with_restarted_model_round_keeps_both_bodies() {
        let timeline = Rc::new(RefCell::new(crate::timeline::Timeline::new()));
        let mut output = TurnOutput::new("fixture-session".into(), timeline.clone());
        let mut live = timeline.borrow_mut();
        live.begin_turn("fixture-root", "Fixture input");
        apply_event(
            &mut output,
            &mut live,
            &json!({"event":"llm_output","data":{"model_round":1,"content":"Earlier reply"}}),
        )
        .unwrap();
        apply_event(&mut output, &mut live, &json!({"event":"native_execution_started","data":{"root_turn_id":"fixture-root","turn_id":"fixture-child"}})).unwrap();
        apply_event(
            &mut output,
            &mut live,
            &json!({"event":"llm_output_delta","data":{"model_round":1,"delta":"Next reply"}}),
        )
        .unwrap();
        live.flush();
        let rows = live.model();
        let bodies = rows
            .iter()
            .filter(|row| row.kind == crate::timeline::KIND_BODY)
            .collect::<Vec<_>>();
        assert_eq!(bodies.len(), 2);
        assert_ne!(bodies[0].id, bodies[1].id);
        assert_eq!(bodies[0].text, "Earlier reply");
        assert_eq!(bodies[1].text, "Next reply");
    }
    use serde_json::json;

    #[test]
    fn a_late_reasoning_frame_cannot_reopen_an_older_round() {
        let timeline = Rc::new(RefCell::new(crate::timeline::Timeline::new()));
        let mut output = TurnOutput::new("fixture-session".into(), timeline.clone());
        let mut live = timeline.borrow_mut();
        live.begin_turn("fixture-root", "Fixture input");
        for (round, text) in [(1, "First thought"), (2, "Second thought"), (1, " stale")] {
            apply_event(
                &mut output,
                &mut live,
                &json!({
                    "event":"reasoning_delta","data":{"model_round":round,"delta":text}
                }),
            )
            .unwrap();
        }
        live.flush();
        let reasons: Vec<_> = live
            .model()
            .iter()
            .filter(|row| row.kind == crate::timeline::KIND_REASON)
            .collect();
        assert_eq!(reasons.len(), 2);
        assert_eq!(reasons[0].detail, "First thought");
        assert_eq!(reasons[1].detail, "Second thought");
    }

    /// The statistics footer must land on the independent turn footer,
    /// both when the runtime rides it on the final answer and when it only
    /// arrives with the terminal event.
    #[test]
    fn turn_statistics_reach_the_footer_from_either_event() {
        let timeline = Rc::new(RefCell::new(crate::timeline::Timeline::new()));
        let mut output = TurnOutput::new("fixture-session".into(), timeline.clone());
        let mut live = timeline.borrow_mut();
        live.begin_turn("fixture-root", "Fixture input");
        drop(live);
        let reduce = |output: &mut TurnOutput, event: Value| {
            let mut live = timeline.borrow_mut();
            apply_event(output, &mut live, &event).unwrap()
        };

        // A delta carries no statistics: the footer must stay empty while the
        // answer is still streaming.
        reduce(
            &mut output,
            json!({"event":"llm_output_delta","data":{"model_round":1,"delta":"Fixture answer"}}),
        );
        {
            let mut live = timeline.borrow_mut();
            live.flush();
        }
        let footer_stats = |timeline: &Rc<RefCell<crate::timeline::Timeline>>| {
            let live = timeline.borrow();
            let model = live.model();
            let row = (0..slint::Model::row_count(&model))
                .filter_map(|index| slint::Model::row_data(&model, index))
                .find(|row| row.kind == crate::timeline::KIND_FOOTER)?;
            Some(
                row.stats
                    .iter()
                    .map(|metric| (metric.key.to_string(), metric.value.to_string()))
                    .collect::<Vec<_>>(),
            )
        };
        assert_eq!(
            footer_stats(&timeline),
            None,
            "streamed text alone does not create numeric metrics",
        );

        // The final answer carries the turn's statistics.
        reduce(
            &mut output,
            json!({"event":"final","data":{
                "answer":"Fixture answer",
                "user_round":57,
                "meta":{"message_stats":{
                    "interaction_duration_s": 12.44,
                    "request_consumed_tokens": 4096,
                    "toolCalls": 3,
                    "contextTokens": 12345,
                    "avg_model_round_speed_rounds": 2.0,
                    "avg_model_round_speed_tps": 68234.0,
                    "model_request_count": 4
                }}
            }}),
        );
        let metrics = footer_stats(&timeline).expect("a footer row");
        assert_eq!(
            metrics,
            vec![
                ("hashtag".to_string(), "57".to_string()),
                ("stopwatch".to_string(), "12.4s".to_string()),
                ("gauge-high".to_string(), "68234.0 token/s".to_string()),
                ("layer-group".to_string(), "12.3k".to_string()),
                ("bolt".to_string(), "4.1k".to_string()),
                ("screwdriver-wrench".to_string(), "3".to_string()),
                ("coins".to_string(), "4".to_string()),
            ],
            "the row mirrors the web messenger: icon key plus value, in order",
        );
    }

    /// A turn whose statistics only arrive with the terminal event still gets a
    /// footer: very short turns emit no separate final answer payload.
    #[test]
    fn terminal_only_statistics_still_produce_a_footer() {
        let timeline = Rc::new(RefCell::new(crate::timeline::Timeline::new()));
        let mut output = TurnOutput::new("fixture-session".into(), timeline.clone());
        let mut live = timeline.borrow_mut();
        live.begin_turn("fixture-root", "Fixture input");
        drop(live);
        let mut live = timeline.borrow_mut();
        apply_event(
            &mut output,
            &mut live,
            &json!({"event":"llm_output","data":{"model_round":1,"answer":"Fixture answer"}}),
        )
        .unwrap();
        apply_event(
            &mut output,
            &mut live,
            &json!({"event":"turn_terminal","data":{
                "status":"completed",
                "message_stats":{"interaction_duration_s": 1.5, "toolCalls": 0}
            }}),
        )
        .unwrap();
        let model = live.model();
        let metrics: Vec<(String, String)> = (0..slint::Model::row_count(&model))
            .filter_map(|index| slint::Model::row_data(&model, index))
            .find(|row| row.kind == crate::timeline::KIND_FOOTER)
            .map(|row| {
                row.stats
                    .iter()
                    .map(|metric| (metric.key.to_string(), metric.value.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(
            metrics,
            vec![
                ("stopwatch".into(), "1.5s".into()),
                ("gauge-high".into(), "-".into()),
                ("layer-group".into(), "--".into()),
                ("bolt".into(), "--".into()),
                ("screwdriver-wrench".into(), "0".into()),
                ("coins".into(), "--".into())
            ]
        );
    }

    /// One runtime round: admission, one reasoning segment, one tool, one
    /// answer block. The timeline must show exactly those entries, in order.
    #[test]
    fn one_tool_round_becomes_grouped_entries_and_frees_the_next_round() {
        let timeline = Rc::new(RefCell::new(crate::timeline::Timeline::new()));
        let mut output = TurnOutput::new("fixture-session".into(), timeline.clone());
        let mut live = timeline.borrow_mut();
        live.begin_turn("fixture-root", "Fixture input");
        drop(live);
        let reduce = |output: &mut TurnOutput, event: Value| {
            let mut live = timeline.borrow_mut();
            apply_event(output, &mut live, &event).unwrap()
        };
        reduce(
            &mut output,
            json!({"event":"llm_request","data":{"turn_id":"fixture-root","model_round":1}}),
        );
        reduce(
            &mut output,
            json!({"event":"reasoning_delta","data":{"model_round":1,"delta":"Fixture thinking"}}),
        );
        reduce(
            &mut output,
            json!({"event":"tool_call","data":{"tool_call_id":"fixture-tool","tool":"read_file","args":{"path":"example.txt"},"request_usage":{"total":2400}}}),
        );
        reduce(
            &mut output,
            json!({"event":"tool_result","data":{"tool_call_id":"fixture-tool","tool":"read_file","args":{"path":"example.txt"},"result":{"ok":true,"data":{"content":"Fixture output"}}}}),
        );
        reduce(
            &mut output,
            json!({"event":"llm_output_delta","data":{"model_round":1,"delta":"Fixture answer"}}),
        );
        // The frame flush is what publishes the streamed block, exactly as the
        // stream timer does once per tick.
        timeline.borrow_mut().flush();
        let live = timeline.borrow();
        let rows: Vec<_> = live.model().iter().collect();
        let kinds: Vec<_> = rows.iter().map(|row| row.kind).collect();
        assert_eq!(
            kinds,
            vec![
                crate::timeline::KIND_USER,
                crate::timeline::KIND_PROCESS,
                crate::timeline::KIND_GROUP,
                crate::timeline::KIND_REASON,
                crate::timeline::KIND_TOOL,
                crate::timeline::KIND_BODY,
            ]
        );
        let group = &rows[2];
        assert_eq!(group.text, "执行工具 1 次");
        assert!(
            !group.visible && group.group_open && group.group_idx == 0,
            "ownership batches have no second visible fold"
        );
        assert_eq!(rows[3].tool_name, "已思考");
        assert_eq!(rows[3].status_kind, crate::timeline::STATUS_DONE);
        assert!(rows[3].detail.contains("Fixture thinking"));
        assert_eq!(rows[4].tool_name, "读取文件");
        assert_eq!(rows[4].target, "example.txt");
        assert_eq!(rows[4].status_kind, crate::timeline::STATUS_DONE);
        assert_eq!(rows[4].payload, 4);
        assert_eq!(rows[5].text, "Fixture answer");
        // The process is the sole handle from the first thought onward.
        assert_eq!(
            rows[1].text,
            crate::timeline_text::process_label(2),
            "the bar counts the steps it already holds",
        );
        assert!(rows[1].visible, "one batch uses the process bar");
        assert_eq!(rows[5].process_idx, crate::timeline::NO_PROCESS);
        // A second round freezes the first block and opens a new batch.
        drop(live);
        reduce(
            &mut output,
            json!({"event":"llm_request","data":{"turn_id":"fixture-root","model_round":2}}),
        );
        reduce(
            &mut output,
            json!({"event":"tool_call","data":{"tool_call_id":"fixture-tool-2","tool":"execute_command","args":{"command":"fixture"},"result":{"data":{"stdout":"ok","returncode":0}}}}),
        );
        reduce(
            &mut output,
            json!({"event":"llm_output_delta","data":{"model_round":2,"delta":"Second answer"}}),
        );
        timeline.borrow_mut().flush();
        let live = timeline.borrow();
        let rows: Vec<_> = live.model().iter().collect();
        assert_eq!(rows.len(), 9);
        assert!(rows[5].foldable, "the first answer block freezes");
        assert_eq!(rows[6].kind, crate::timeline::KIND_GROUP);
        assert_eq!(rows[6].group_idx, 1);
        assert_eq!(rows[8].kind, crate::timeline::KIND_BODY);
        assert_eq!(rows[8].text, "Second answer");
        // Two batches plus an intermediate note is a process: the bar takes
        // over, its batches come open with it, and only the newest answer stays
        // outside.
        assert!(rows[1].visible, "the process bar earns its place");
        assert_eq!(
            rows[1].text,
            crate::timeline_text::process_label(4),
            "thinking, one call, the frozen note and the second call",
        );
        assert!(
            !rows[1].summary.is_empty(),
            "the folded bar reads out what the turn last did"
        );
        for row in rows.iter().skip(2).take(6) {
            assert_eq!(row.process_idx, rows[1].process_idx, "step {row:?} escaped");
            assert!(
                !row.process_open && (row.group_idx == crate::timeline::NO_GROUP || row.group_open),
                "the process ships folded without a second batch gate"
            );
        }
        assert_eq!(rows[8].process_idx, crate::timeline::NO_PROCESS);
    }

    /// A stopped turn keeps the partial answer and marks the tail, and later
    /// events never reopen it.
    #[test]
    fn stop_and_terminal_state_freeze_the_tail_without_new_rows() {
        let timeline = Rc::new(RefCell::new(crate::timeline::Timeline::new()));
        let mut output = TurnOutput::new("fixture-session".into(), timeline.clone());
        timeline
            .borrow_mut()
            .begin_turn("fixture-root", "Fixture input");
        let mut live = timeline.borrow_mut();
        apply_event(
            &mut output,
            &mut live,
            &json!({"event":"llm_output_delta","data":{"model_round":1,"delta":"Fixture partial"}}),
        )
        .unwrap();
        apply_event(
            &mut output,
            &mut live,
            &json!({"event":"compaction","data":{"summary_text":"Fixture summary"}}),
        )
        .unwrap();
        // A call that is still running when the turn is cancelled must not keep
        // spinning: the settlement marks it failed.
        apply_event(
            &mut output,
            &mut live,
            &json!({"event":"tool_call","data":{"tool_call_id":"fixture-open","tool":"execute_command","args":{"command":"fixture"}}}),
        )
        .unwrap();
        apply_event(
            &mut output,
            &mut live,
            &json!({"event":"turn_terminal","data":{"status":"cancelled"}}),
        )
        .unwrap();
        assert_eq!(output.state, "已停止");
        let before = live.row_count();
        assert!(!apply_event(
            &mut output,
            &mut live,
            &json!({"event":"final","data":{"content":"Late answer"}}),
        )
        .unwrap());
        assert_eq!(live.row_count(), before);
        live.flush();
        live.finish(true);
        let rows: Vec<_> = live.model().iter().collect();
        assert!(
            rows.iter().any(|row| row.kind == crate::timeline::KIND_TOOL
                && row.status_kind == crate::timeline::STATUS_FAILED),
            "a failed turn marks its running entries"
        );
        assert!(
            rows.iter()
                .all(|row| row.status_kind != crate::timeline::STATUS_RUNNING),
            "no entry keeps spinning after the turn settles"
        );
        let bodies: Vec<_> = rows
            .iter()
            .filter(|row| row.kind == crate::timeline::KIND_BODY)
            .collect();
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0].text, "Fixture partial");
    }

    fn fixture_panel() -> InquiryState {
        inquiry_from_payload(&json!({
            "questions": [
                {"question": "Fixture question one", "multiple": true, "options": [
                    {"label": "First", "description": "fixture note", "recommended": true},
                    {"label": "Second"},
                ]},
                {"question": "Fixture question two", "options": [
                    {"label": "Alpha"}, {"label": "Beta"},
                ]},
                {"question": "Fixture question three", "options": []},
            ]
        }))
        .expect("a payload with questions projects a panel")
    }

    /// The card is a projection of the payload: every question survives, an
    /// option-less one is dropped instead of rendering an empty page, and an
    /// empty payload means there is no card at all.
    #[test]
    fn inquiry_payload_projects_every_question() {
        let panel = fixture_panel();
        assert_eq!(panel.questions.len(), 2);
        assert!(panel.questions[0].multiple);
        assert!(!panel.questions[1].multiple);
        assert_eq!(panel.questions[0].options.len(), 2);
        assert_eq!(panel.questions[0].options[0].label, "First");
        assert_eq!(panel.questions[0].options[0].description, "fixture note");
        assert!(panel.questions[0].options[0].recommended);
        assert_eq!(panel.questions[1].options[1].label, "Beta");
        assert_eq!(panel.page, 0);
        assert!(inquiry_from_payload(&json!({"questions": []})).is_none());
        assert!(inquiry_from_payload(&json!({"questions": [{"question": "x"}]})).is_none());
    }

    /// The send button unlocks only when every question has an answer, and the
    /// answers become one message in the shape the model is fed.
    #[test]
    fn inquiry_answers_gate_the_send_and_compose_one_message() {
        let mut panel = fixture_panel();
        assert!(!panel.ready(), "an untouched panel is not an answer");

        panel.move_page(1);
        panel.toggle_option(1, 1);
        assert!(!panel.questions[0].options.iter().any(|o| o.selected));
        assert!(!panel.ready(), "the first question is still unanswered");

        panel.move_page(-1);
        panel.toggle_option(0, 0);
        panel.toggle_option(0, 1);
        assert!(
            panel.questions[0].options.iter().all(|o| o.selected),
            "a multi choice keeps both ticks"
        );
        assert!(panel.ready());
        assert_eq!(
            panel.reply(),
            "【问询面板选择】\n问题 1/2：Fixture question one\n答案：First、Second\n问题 2/2：Fixture question two\n答案：Beta"
        );

        // Re-picking the only single choice takes the answer back off.
        panel.move_page(1);
        panel.toggle_option(1, 1);
        assert!(!panel.ready());
        panel.toggle_option(1, 0);
        assert!(panel.questions[1].options[0].selected);
        assert!(!panel.questions[1].options[1].selected);
    }

    /// The expanded stack answers by question ordinal, not by page position,
    /// and 使用推荐 ticks the recommended option the way the quick menu does.
    #[test]
    fn inquiry_expanded_and_recommended_address_their_own_question() {
        let mut panel = fixture_panel();
        panel.expanded = true;
        panel.toggle_option(1, 1);
        assert!(panel.questions[1].options[1].selected);
        assert!(
            !panel.questions[0].options.iter().any(|o| o.selected),
            "the second question's tick never lands on the first"
        );

        panel.use_recommended(0);
        assert!(panel.questions[0].options[0].selected);
        assert!(
            !panel.questions[0].options[1].selected,
            "a single-choice quick answer keeps one tick"
        );
        panel.questions[0].multiple = true;
        panel.questions[0].options[0].selected = false;
        panel.use_recommended(0);
        assert!(panel.questions[0].options[0].selected);
    }

    /// 无偏好 is a full answer for its own question and nothing else, and the
    /// free text rides along with the ticks the user kept.
    #[test]
    fn inquiry_no_preference_and_free_text_answer_one_question() {
        let mut panel = fixture_panel();
        panel.toggle_option(0, 0);
        assert!(!InquiryState::is_answered(&panel.questions[1]));
        panel.questions[0].other = "  Fixture extra  ".into();
        assert!(
            InquiryState::is_answered(&panel.questions[0]),
            "free text alone answers a question"
        );
        assert!(panel.reply().contains("答案：First、Fixture extra"));

        panel.toggle_no_preference(0);
        assert!(panel.questions[0].no_preference);
        assert!(
            panel.questions[0]
                .options
                .iter()
                .all(|option| !option.selected),
            "无偏好 replaces the ticks of its own question"
        );
        assert!(panel.questions[0].other.is_empty());
        panel.move_page(1);
        panel.toggle_option(1, 0);
        assert!(panel.ready());
        assert_eq!(
            panel.reply(),
            "【问询面板选择】\n问题 1/2：Fixture question one\n答案：无偏好\n问题 2/2：Fixture question two\n答案：Alpha"
        );
        panel.move_page(-1);
        panel.toggle_no_preference(0);
        assert!(!panel.questions[0].no_preference);
        assert!(!panel.ready());
    }

    /// Paging is a cursor, not a reset: each question keeps what was chosen and
    /// the index never leaves the panel.
    #[test]
    fn inquiry_paging_keeps_answers_and_stays_in_range() {
        let mut panel = fixture_panel();
        panel.toggle_option(0, 1);
        panel.move_page(4);
        assert_eq!(panel.page, 1, "the cursor clamps at the last question");
        panel.questions[1].other = "fixture note".into();
        panel.move_page(-3);
        assert_eq!(panel.page, 0);
        assert!(panel.questions[0].options[1].selected);
        assert_eq!(panel.questions[1].other, "fixture note");
        assert_eq!(panel.current_index(), 0);
    }

    /// The runtime wraps every payload as `{session_id, timestamp, data}`, so
    /// the panel reads the questions one level below the stream envelope.
    #[test]
    fn inquiry_event_unwraps_the_stream_envelope() {
        let panel = inquiry_from_payload(event_data(&json!({
            "event": "question_panel",
            "data": {"session_id": "fixture", "timestamp": "0",
                     "data": {"questions": [
                        {"question": "Wrapped?", "options": [{"label": "Yes"}]}]}}
        })))
        .expect("the wrapped payload still projects a panel");
        assert_eq!(panel.questions.len(), 1);
        assert_eq!(panel.questions[0].question, "Wrapped?");
    }
}
