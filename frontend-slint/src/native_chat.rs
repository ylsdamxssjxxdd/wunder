//! Native chat projection for the in-process desktop runtime.
use wunder_desktop::native::NativeWorkflowEntry;

use crate::{terminal_grid, Conversation, MainWindow, PlanStep, TermSpan, TimelineRow};
use serde_json::Value;
use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use wunder_desktop::{
    NativeChatAttachment, NativeChatEvent, NativeChatInput, NativeDesktop, NativeStream,
    NativeTerminalSpec,
};
use wunder_desktop::native::NativeChatCommand;

#[path = "native_chat_commands.rs"]
mod commands;
#[path = "native_chat_observer.rs"]
mod observer;

struct Active {
    stream: NativeStream,
    output: TurnOutput,
}

/// Live turn state. The timeline rows themselves live in `Timeline`; this
/// carries the stream identity plus the statistics row the composer shows.
struct TurnOutput {
    session: String,
    /// Shared with `State::timeline`: the live turn appends to the same model
    /// the history already occupies, so nothing is copied on send or settle.
    timeline: Rc<RefCell<crate::timeline::Timeline>>,
    state: String,
    round: i64,
    /// Model round whose body block is currently open; a round change starts a
    /// new block instead of appending to the finished one.
    body_round: i64,
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
    /// events. Session-scoped in spirit, so a new turn resumes the last plan;
    /// file-change counters start at zero every turn.
    plan_steps: Vec<(String, String)>,
    plan_explanation: String,
    files_changed: i64,
    lines_added: i64,
    lines_deleted: i64,
}

impl TurnOutput {
    fn new(session: String, timeline: Rc<RefCell<crate::timeline::Timeline>>) -> Self {
        Self {
            session,
            timeline,
            state: "正在生成…".into(),
            round: 0,
            body_round: -1,
            stats_duration: String::new(),
            workflow: Vec::new(),
            command_output: std::collections::HashMap::new(),
            plan_steps: Vec::new(),
            plan_explanation: String::new(),
            files_changed: 0,
            lines_added: 0,
            lines_deleted: 0,
        }
    }

    fn failed(&self) -> bool {
        matches!(self.state.as_str(), "执行失败" | "思考中断")
            || self.state.contains("失败")
    }

    /// Upsert one tool entry and publish it to the timeline. The reducer calls
    /// this instead of touching rows directly, so a tool result can never
    /// create a duplicate entry for the same call id.
    fn upsert_workflow(
        &mut self,
        timeline: &mut crate::timeline::Timeline,
        entry: NativeWorkflowEntry,
    ) {
        if let Some(existing) = self
            .workflow
            .iter_mut()
            .find(|old| !entry.id.is_empty() && old.id == entry.id)
        {
            *existing = entry;
        } else {
            if self.workflow.len() >= 24 {
                self.workflow.remove(0);
            }
            self.workflow.push(entry);
        }
        if let Some(entry) = self.workflow.last() {
            timeline.upsert_tool(entry);
        }
    }

    /// Append one command-output chunk for a session and return the retained
    /// tail. `command_session_delta` frames arrive many times per command, so the
    /// buffer is bounded and only its newest bytes are kept.
    fn append_command_output(&mut self, key: &str, delta: &str) -> String {
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

struct State {
    timer: Timer,
    observation: observer::Observation,
    desktop: Arc<NativeDesktop>,
    active: Option<Active>,
    /// History plus the live turn; the timeline is the single chat model.
    timeline: Rc<RefCell<crate::timeline::Timeline>>,
    history_generation: Arc<AtomicU64>,
    drafts: std::collections::HashMap<String, String>,
    recording: Option<Recording>,
    list_inflight: Arc<AtomicBool>,
    /// Live local-shell session owned by the composer's terminal mode. The
    /// process keeps running while the terminal panel stays open; leaving the
    /// mode merely hides it, so buffered output survives re-entry.
    terminal: Option<TerminalSession>,
    /// Poll timer for the terminal output; runs for the whole desktop session
    /// and is a no-op while no terminal is alive (kept separate from the
    /// stream pump so chat generation never steals the tick).
    terminal_timer: Timer,
    /// The shell's screen: output bytes go in as they arrive and the panel shows
    /// one viewport projected out of it, so redraws, wrapping and colours behave
    /// like a terminal instead of an appending text box.
    grid: terminal_grid::TerminalGrid,
    /// Human-readable status line shown above the terminal output.
    terminal_status: String,
    /// Whether the "[较早输出已截断]" marker has been inserted for this
    /// session; reset on clear and on session restart.
    terminal_loss_marked: bool,
    /// Whether the last observed shell state was a failure/error exit; drives
    /// the status dot color in `TerminalView`.
    terminal_error: bool,
    /// Whether the durable transcript of earlier runs was replayed into the
    /// body. Restored once per app session so clearing the panel stays clear.
    terminal_restored: bool,
    /// Session-scoped plan state (explanation + steps) so the plan card
    /// survives turn boundaries within a thread.
    plans: std::collections::HashMap<String, (String, Vec<(String, String)>)>,
    /// Change key of the plan model last published to the UI; the flush runs
    /// per frame, so the slint model is only rebuilt when the plan moved.
    plan_sig: String,
}

/// A running interactive shell started by the terminal mode.
struct TerminalSession {
    terminal_id: String,
    session_id: String,
    last_seq: u64,
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
        timeline: Rc::new(RefCell::new(crate::timeline::Timeline::new())),
        history_generation: Arc::new(AtomicU64::new(0)),
        drafts: std::collections::HashMap::new(),
        recording: None,
        list_inflight: Arc::new(AtomicBool::new(false)),
        terminal: None,
        terminal_timer: Timer::default(),
        // Starts at the size the panel declares before it knows its own
        // geometry; the first layout reports the real one.
        grid: terminal_grid::TerminalGrid::new(24, 80),
        terminal_status: String::new(),
        terminal_loss_marked: false,
        terminal_error: false,
        terminal_restored: false,
        plans: std::collections::HashMap::new(),
        plan_sig: String::new(),
    }));
    observer::install(app, state.clone());
    bind_refresh(app, state.clone());
    bind_selection(app, state.clone());
    bind_new_thread(app, state.clone());
    bind_send(app, state.clone());
    bind_terminal(app, state.clone());
    bind_reasoning_effort(app, state.clone());
    bind_attachments(app);
    bind_voice_recording(app, state.clone());
    bind_stop(app, state.clone());
    bind_goal(app, state.clone());
    bind_activity(app, state.clone());
    start_goal_clock(app.as_weak());
    crate::thread_log_ui::install(app, desktop_preview.clone());
    crate::navigation_ui::install(app, desktop_preview.clone());
    crate::workspace_ui::install(app, desktop_preview.clone());
    let timeline = state.borrow().timeline.clone();
    bind_timeline(app, timeline);
    let weak = app.as_weak();
    app.on_open_prompt_preview(move || {
        let Some(app) = weak.upgrade() else { return };
        app.set_prompt_preview_open(true);
        app.set_prompt_preview_loading(true);
        app.set_prompt_preview("".into());
        let weak = app.as_weak();
        let desktop = desktop_preview.clone();
        std::thread::spawn(move || {
            let result = desktop.preview_system_prompt();
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_prompt_preview_loading(false);
                match result {
                    Ok(prompt) => app.set_prompt_preview(prompt.into()),
                    Err(error) => {
                        app.set_prompt_preview("".into());
                        app.set_status(format!("系统提示词预览失败：{error}").into());
                        app.set_dialog_title("操作失败".into());
                        app.set_dialog_text(format!("系统提示词预览失败：{error}").into());
                        app.set_dialog_open(true);
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
fn bind_timeline(app: &MainWindow, timeline: Rc<RefCell<crate::timeline::Timeline>>) {
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
    let weak = app.as_weak();
    app.on_timeline_download(move |text| {
        let Some(app) = weak.upgrade() else { return };
        let text = text.to_string();
        if text.trim().is_empty() {
            return;
        }
        match crate::file_dialog::save_text_file("保存回复", &text) {
            Ok(Some(path)) => app.set_status(format!("已保存到 {path}").into()),
            Ok(None) => {}
            Err(error) => app.set_status(format!("保存失败：{error}").into()),
        }
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

/// One projection point for every timeline publish: the row model and the
/// turn-ruler marks always move together, so the ruler can never drift from
/// the visible rows.
pub(crate) fn publish_timeline(app: &MainWindow, live: &crate::timeline::Timeline) {
    app.set_timeline(live.model());
    let marks = live.turn_marks();
    app.set_turn_marks(ModelRc::new(VecModel::from(marks)));
}

/// Reset the status-dock attributes for a thread without a live turn. The
/// slint-viewer demo values (and any previous thread's state) must never leak
/// into the view of an idle session.
fn reset_activity(app: &MainWindow) {
    app.set_activity_active(false);
    app.set_activity_text("".into());
    app.set_activity_steps_done(0);
    app.set_activity_steps_total(0);
    app.set_activity_files_changed(0);
    app.set_activity_added_lines(0);
    app.set_activity_deleted_lines(0);
    app.set_activity_plan_explanation("".into());
    app.set_activity_plan_steps(ModelRc::default());
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

/// Publish the status-dock projection from the live turn: capsule state, plan
/// progress and file-change counters. Returns the plan signature so the
/// caller can persist it (plus the session plan) without recomputing.
fn publish_activity(app: &MainWindow, active: &TurnOutput, last_sig: &str) -> String {
    // Terminal states hide the capsule; plan data stays for the next turn.
    let idle = matches!(
        active.state.as_str(),
        "任务完成" | "已停止" | "执行失败" | "思考中断" | "等待用户输入"
    );
    app.set_activity_active(!idle);
    app.set_activity_text(active.state.as_str().into());
    let total = active.plan_steps.len() as i32;
    let done = active
        .plan_steps
        .iter()
        .filter(|(_, status)| status == "completed")
        .count() as i32;
    app.set_activity_steps_done(done);
    app.set_activity_steps_total(total);
    app.set_activity_files_changed(active.files_changed.clamp(0, i32::MAX as i64) as i32);
    app.set_activity_added_lines(active.lines_added.clamp(0, i32::MAX as i64) as i32);
    app.set_activity_deleted_lines(active.lines_deleted.clamp(0, i32::MAX as i64) as i32);
    app.set_activity_plan_explanation(active.plan_explanation.as_str().into());
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

fn bind_activity(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    app.on_activity_plan_dismiss(move || {
        let Some(app) = weak.upgrade() else { return };
        // Dismissal clears the UI projection and the turn/cache state, so the
        // next flush frame cannot silently restore the checklist; a later
        // plan_update republishes it.
        app.set_activity_plan_steps(ModelRc::default());
        app.set_activity_plan_explanation("".into());
        app.set_activity_steps_done(0);
        app.set_activity_steps_total(0);
        let session = app.get_active_session_id().to_string();
        let mut state = state.borrow_mut();
        state.plans.remove(&session);
        state.plan_sig.clear();
        if let Some(turn) = state.active.as_mut() {
            turn.output.plan_steps.clear();
            turn.output.plan_explanation.clear();
        }
    });
}

fn bind_refresh(app: &MainWindow, state: Rc<RefCell<State>>) {    let weak = app.as_weak();
    app.on_refresh_chat(move || {
        if let Some(app) = weak.upgrade() {
            refresh_chat(&app, state.clone(), false);
        }
    });
}

fn refresh_chat(app: &MainWindow, state: Rc<RefCell<State>>, silent: bool) {
    if app.get_chat_loading() || app.get_session_loading() || app.get_creating_session() {
        return;
    }
    let desktop = state.borrow().desktop.clone();
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
                            let goal_paused = goal.paused();
                            Conversation {
                                id: item.id.into(),
                                agent_id: crate::navigation_ui::agent_key(
                                    item.agent_id.as_deref(),
                                )
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
                                goal_objective: goal.objective.into(),
                                goal_seconds: 0,
                                goal_paused,
                                ..Default::default()
                            }
                        })
                        .collect::<Vec<_>>();
                    let active = rows
                        .iter()
                        .find(|row| row.id == app.get_active_session_id());
                    app.set_goal_active(active.is_some_and(|row| row.goal_active));
                    app.set_goal_objective(
                        active.map(|row| row.goal_objective.clone()).unwrap_or_default(),
                    );
                    set_goal_clock(&app, active.map(|row| row.goal_seconds).unwrap_or(0));
                    app.set_goal_paused(active.is_some_and(|row| row.goal_paused));
                    update_active_thread_filter(&app, rows.iter().cloned());
                    if !app.get_conversations().iter().eq(rows.iter().cloned()) {
                        app.set_conversations(ModelRc::new(VecModel::from(rows)));
                    }
                    crate::navigation_ui::project(&app);
                    crate::navigation_ui::project_sidebar(&app, Some(&desktop));
                    if app.get_active_session_id().is_empty()
                        && app.get_conversations().row_count() > 0
                    {
                        app.invoke_select_conversation(0);
                    }
                    if !silent {
                        app.set_status("内嵌运行时已就绪".into());
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
        let id = row.id.to_string();
        app.set_goal_active(row.goal_active);
        app.set_goal_objective(row.goal_objective.clone());
        set_goal_clock(&app, row.goal_seconds);
        app.set_goal_paused(row.goal_paused);
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
        let desktop = state.borrow().desktop.clone();
        let generation = state.borrow().history_generation.clone();
        let request = generation.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        let weak = app.as_weak();
        app.set_busy(false);
        app.set_stopping(false);
        reset_context_usage(&app);
        app.set_active_session_id(id.clone().into());
        app.set_heading(row.title);
        app.set_session_loading(true);
        state.borrow().timeline.borrow_mut().clear();
        app.set_timeline(ModelRc::default());
        app.set_turn_marks(ModelRc::default());
        std::thread::spawn(move || {
            let result = desktop.get_session_info(&id);
            let _ = weak.upgrade_in_event_loop(move |app| {
                if app.get_active_session_id() != id
                    || generation.load(Ordering::Relaxed) != request
                {
                    return;
                }
                app.set_session_loading(false);
                match result {
                    Ok(session) => {
                        let agent = session.agent_id.unwrap_or_else(|| "__default__".into());
                        if app.get_active_agent_id() != agent {
                            app.set_active_agent_id(agent.clone().into());
                        }
                        // The sidebar highlights the workspace the opened
                        // thread lives in, so a later new-task lands there.
                        crate::navigation_ui::focus_workspace_of(&app, session.workspace_id.as_deref());
                        crate::entity_state::restore_agent(&app, &agent);
                        update_active_thread_filter_model(&app, &app.get_conversations());
                        crate::navigation_ui::project(&app);
                        crate::navigation_ui::project_sidebar(&app, None);
                        app.set_reasoning_effort(session.reasoning_effort.into());
                        app.set_heading(session.title.into());
                        app.set_status("内嵌运行时已就绪".into());
                    }
                    Err(error) => app.set_status(format!("无法加载会话：{error}").into()),
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

fn bind_reasoning_effort(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    app.on_save_session_reasoning_effort(move |value| {
        let Some(app) = weak.upgrade() else { return };
        let session = app.get_active_session_id().to_string();
        if session.is_empty() {
            return;
        }
        let desktop = state.borrow().desktop.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = desktop.save_session_reasoning_effort(&session, &value);
            let _ = weak.upgrade_in_event_loop(move |app| {
                if app.get_active_session_id() != session {
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

/// Fixed scope under which the composer's local shell lives. Kept separate
/// from chat sessions so the terminal mode never collides with thread data.
const TERMINAL_SESSION_ID: &str = "terminal_main";

/// Start (or reuse) the live shell for terminal mode. The process stays alive
/// across mode switches; buffered output is capped by the runtime service.
fn ensure_terminal(state: &mut State) -> Result<String, String> {
    if let Some(session) = &state.terminal {
        return Ok(session.terminal_id.clone());
    }
    let cwd = std::env::current_dir()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default();
    let spec = NativeTerminalSpec {
        terminal_id: String::new(),
        session_id: TERMINAL_SESSION_ID.to_string(),
        command: String::new(),
        cwd,
        shell: String::new(),
    };
    let snapshot = state
        .desktop
        .terminal_start(&spec)
        .map_err(|error| error.to_string())?;
    let terminal_id = snapshot.terminal_id.clone();
    state.terminal = Some(TerminalSession {
        terminal_id: terminal_id.clone(),
        session_id: TERMINAL_SESSION_ID.to_string(),
        last_seq: 0,
    });
    Ok(terminal_id)
}

/// Shown once per session when the runtime dropped buffered head before the
/// panel consumed it, fed through the screen model like any other output.
const TRUNCATION_MARK: &str = "\x1b[33m[较早输出已截断]\x1b[0m\r\n";

/// Foreground the panel uses for cells that ask for the terminal default.
fn default_ink() -> slint::Color {
    slint::Color::from_rgb_u8(0xe8, 0xee, 0xf5)
}

/// One viewport of runs, in the panel's cell geometry. The view places them
/// without knowing anything about terminals, and only what is on screen crosses
/// the boundary, so an update costs the same however long the transcript is.
fn terminal_spans(grid: &mut terminal_grid::TerminalGrid) -> Vec<TermSpan> {
    grid.render()
        .into_iter()
        .map(|span| TermSpan {
            text: span.text.into(),
            fg: match span.fg {
                terminal_grid::Ink::Default => default_ink(),
                terminal_grid::Ink::Rgb(rgb) => slint::Color::from_rgb_u8(rgb[0], rgb[1], rgb[2]),
            },
            // Transparent means the panel background.
            bg: span
                .bg
                .map(|rgb| slint::Color::from_rgb_u8(rgb[0], rgb[1], rgb[2]))
                .unwrap_or_default(),
            bold: span.bold,
            underline: span.underline,
            row: span.row,
            col: span.col,
            cells: span.cells,
        })
        .collect()
}

/// Paint one viewport of the screen model onto the panel, sizing the model to
/// the panel first. Reading the geometry here rather than watching a change
/// callback means the very first frame is already laid out for the real box.
/// A geometry change is also reported to the shell: a console backend reflows
/// its lines to it, so the panel and the program agree on one width.
fn paint_terminal(app: &MainWindow, host: &mut State) {
    let rows = app.get_terminal_measured_rows() as u16;
    let cols = app.get_terminal_measured_cols() as u16;
    let resized = rows > 0 && cols > 0 && host.grid.size() != (rows, cols);
    if resized {
        host.grid.set_size(rows, cols);
    }
    app.set_terminal_spans(ModelRc::from(Rc::new(VecModel::from(
        terminal_spans(&mut host.grid),
    ))));
    let (rows, cols) = host.grid.size();
    app.set_terminal_rows(rows.into());
    app.set_terminal_cols(cols.into());
    app.set_terminal_view_offset(host.grid.view_offset() as i32);
    app.set_terminal_history_depth(host.grid.history_depth() as i32);
    match host.grid.caret() {
        Some((row, col)) => {
            app.set_terminal_caret_visible(true);
            app.set_terminal_caret_row(row);
            app.set_terminal_caret_col(col);
        }
        None => app.set_terminal_caret_visible(false),
    }
    if resized {
        if let Some(session) = host.terminal.as_ref() {
            let _ = host.desktop.terminal_resize(
                &session.session_id,
                &session.terminal_id,
                cols as i32,
                rows as i32,
            );
        }
    }
}

/// Most transcript bytes replayed into the screen model at once. The model shows
/// a bounded number of rows, so replaying a long history in full would spend
/// seconds producing lines that can never be seen.
const MAX_TERMINAL_REPLAY_BYTES: usize = 128 * 1024;

/// Replay the durable transcript of earlier shell runs into the screen model.
/// Raw bytes go back through the parser, so the restored viewport carries the
/// same cursor addressing and colours as the session that wrote them.
fn restore_terminal_transcript(state: &Rc<RefCell<State>>, app: &MainWindow) {
    let mut host = state.borrow_mut();
    if host.terminal_restored {
        return;
    }
    host.terminal_restored = true;
    let raw = host.desktop.terminal_transcript(TERMINAL_SESSION_ID);
    if raw.is_empty() {
        return;
    }
    // Start on a line boundary: a cut mid-sequence would feed half an escape.
    let mut from = raw.len().saturating_sub(MAX_TERMINAL_REPLAY_BYTES);
    while from < raw.len() && !raw.is_char_boundary(from) {
        from += 1;
    }
    from += raw[from..].find('\n').map_or(0, |offset| offset + 1);
    host.grid.feed(&raw.as_bytes()[from..]);
    paint_terminal(app, &mut host);
}

fn bind_terminal(app: &MainWindow, state: Rc<RefCell<State>>) {
    let callback_state = Rc::downgrade(&state);
    let weak = app.as_weak();
    app.on_terminal_toggled(move |active| {
        let (Some(app), Some(state)) = (weak.upgrade(), callback_state.upgrade()) else {
            return;
        };
        if !active {
            // Leaving terminal mode only hides the panel; the shell stays warm
            // so re-entry is instant and buffered output is not lost.
            return;
        }
        restore_terminal_transcript(&state, &app);
        let started = ensure_terminal(&mut state.borrow_mut());
        match started {
            Ok(_) => {
                state.borrow_mut().terminal_loss_marked = false;
                app.set_terminal_error(false);
                app.set_terminal_status("启动中…".into());
            }
            Err(error) => app.set_status(format!("无法启动终端：{error}").into()),
        }
        paint_terminal(&app, &mut state.borrow_mut());
    });
    // Interrupt is a Ctrl-C when the shell owns a console, so the session
    // survives and keeps its handle. A piped shell has no signal path and is
    // killed; the poll then observes the exit and drops the handle, so the next
    // command lazily starts a fresh shell either way.
    let callback_state = Rc::downgrade(&state);
    let weak = app.as_weak();
    app.on_terminal_interrupt(move || {
        let (Some(app), Some(state)) = (weak.upgrade(), callback_state.upgrade()) else {
            return;
        };
        let mut host = state.borrow_mut();
        let Some(session) = host.terminal.as_ref() else {
            return;
        };
        if let Err(error) = host
            .desktop
            .terminal_cancel(&session.session_id, &session.terminal_id)
        {
            app.set_status(format!("终端中断失败：{error}").into());
            return;
        }
        host.terminal_status = "已中断".into();
        app.set_terminal_error(false);
        app.set_terminal_status("已中断".into());
    });
    // Clearing only resets the screen model, never the shell.
    let callback_state = Rc::downgrade(&state);
    let weak = app.as_weak();
    app.on_terminal_clear(move || {
        let (Some(app), Some(state)) = (weak.upgrade(), callback_state.upgrade()) else {
            return;
        };
        let mut host = state.borrow_mut();
        host.grid.reset();
        host.terminal_loss_marked = false;
        paint_terminal(&app, &mut host);
    });
    // The viewport belongs to the screen model, so the panel only reports what
    // the user asked for and never keeps a scroll position of its own.
    let callback_state = Rc::downgrade(&state);
    let weak = app.as_weak();
    app.on_terminal_scrolled(move |delta| {
        let (Some(app), Some(state)) = (weak.upgrade(), callback_state.upgrade()) else {
            return;
        };
        let mut host = state.borrow_mut();
        host.grid.scroll_by(delta);
        paint_terminal(&app, &mut host);
    });
    let callback_state = Rc::downgrade(&state);
    let weak = app.as_weak();
    app.on_terminal_scroll_to(move |row| {
        let (Some(app), Some(state)) = (weak.upgrade(), callback_state.upgrade()) else {
            return;
        };
        let mut host = state.borrow_mut();
        host.grid.scroll_to_row(row.max(0) as usize);
        paint_terminal(&app, &mut host);
    });
    // A resize changes the cell box, so the screen model is re-laid out and the
    // shell told, even when nothing is printing.
    let callback_state = Rc::downgrade(&state);
    let weak = app.as_weak();
    app.on_terminal_geometry_changed(move || {
        let (Some(app), Some(state)) = (weak.upgrade(), callback_state.upgrade()) else {
            return;
        };
        if !app.get_terminal_mode() {
            return;
        }
        paint_terminal(&app, &mut state.borrow_mut());
    });
    bind_terminal_poll(app, state);
}

fn describe_terminal_status(frame: &wunder_desktop::NativeTerminalFrame) -> String {
    match frame.status.as_str() {
        "starting" => "启动中…".to_string(),
        // The console marker is worth showing: it means the shell really owns a
        // screen, so colours, reflow and Ctrl-C work. Piped output is the
        // fallback and needs no announcement.
        "running" if frame.backend == "console" => "运行中 · 控制台".to_string(),
        "running" => "运行中".to_string(),
        "failed" => {
            let error = frame.error.as_deref().unwrap_or("未知原因");
            format!("启动失败（{error}）")
        }
        "exited" => match frame.exit_code {
            Some(code) => format!("已退出（code={code}）"),
            None => "已退出".to_string(),
        },
        other => other.to_string(),
    }
}

/// Pulls incremental terminal output on a timer (300 ms), feeds it through the
/// screen model and repaints the viewport. Cheap no-op while no terminal is
/// alive, and the work per tick stays bounded by the screen however much the
/// shell printed.
fn bind_terminal_poll(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    let callback_state = Rc::downgrade(&state);
    state.borrow().terminal_timer.start(
        TimerMode::Repeated,
        Duration::from_millis(300),
        move || {
            let (Some(app), Some(state)) = (weak.upgrade(), callback_state.upgrade()) else {
                return;
            };
            let mut state = state.borrow_mut();
            let Some(session) = state.terminal.as_ref() else {
                return;
            };
            let session_id = session.session_id.clone();
            let terminal_id = session.terminal_id.clone();
            let last_seq = session.last_seq;
            match state
                .desktop
                .terminal_poll(&session_id, &terminal_id, last_seq)
            {
                Ok(frame) => {
                    if !frame.text.is_empty() {
                        if frame.truncated && !state.terminal_loss_marked {
                            // The runtime dropped buffered head before we
                            // consumed; mark the loss once per session.
                            state.terminal_loss_marked = true;
                            state.grid.feed(TRUNCATION_MARK.as_bytes());
                        }
                        state.grid.feed(frame.text.as_bytes());
                        paint_terminal(&app, &mut state);
                    }
                    let status = describe_terminal_status(&frame);
                    if state.terminal_status != status {
                        state.terminal_status = status.clone();
                        app.set_terminal_status(status.into());
                    }
                    let is_terminal_error = frame.status == "failed"
                        || (frame.status == "exited"
                            && frame.exit_code.map(|code| code != 0).unwrap_or(false));
                    if state.terminal_error != is_terminal_error {
                        state.terminal_error = is_terminal_error;
                        app.set_terminal_error(is_terminal_error);
                    }
                    if let Some(session) = state.terminal.as_mut() {
                        session.last_seq = frame.seq;
                    }
                    if frame.status == "exited" || frame.status == "failed" {
                        // Drop the handle so the next command or re-entry
                        // starts a fresh shell automatically.
                        state.terminal = None;
                    }
                }
                Err(_) => {
                    // The session vanished underneath us (closed or pruned);
                    // drop the local handle so a fresh shell can be started.
                    state.terminal = None;
                }
            }
        },
    );
}

fn bind_send(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    app.on_send_message(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_command_pending() {
            return;
        }
        let content = app.get_draft().trim().to_string();
        if app.get_terminal_mode() {
            // Terminal mode: the input belongs to the local shell, never the
            // model. "//" escapes a literal slash prefix; everything else is
            // passed through verbatim so "/"-prefixed binaries still work.
            if content.is_empty() {
                return;
            }
            let command = content.strip_prefix("//").unwrap_or(&content);
            let mut input = command.as_bytes().to_vec();
            input.extend_from_slice(if cfg!(windows) { b"\r\n" } else { b"\n" });
            let terminal_id = match ensure_terminal(&mut state.borrow_mut()) {
                Ok(id) => id,
                Err(error) => {
                    app.set_status(format!("无法启动终端：{error}").into());
                    return;
                }
            };
            let result =
                state
                    .borrow()
                    .desktop
                    .terminal_send(TERMINAL_SESSION_ID, &terminal_id, &input);
            if let Err(error) = result {
                app.set_status(format!("终端发送失败：{error}").into());
                return;
            }
            // A new command belongs at the bottom of the screen, whatever the
            // user was reading before pressing send.
            state.borrow_mut().grid.scroll_to_bottom();
            app.set_draft("".into());
            return;
        }
        if commands::dispatch(&app, &state, &content) {
            return;
        }
        // Guard the not-ready states, but never silently: a dead-looking send
        // button is how a broken runtime used to hide from the user.
        if !state.borrow().observation.is_ready()
            || app.get_busy()
            || app.get_session_loading()
            || app.get_chat_loading()
            || app.get_creating_session()
        {
            if !state.borrow().observation.is_ready() {
                app.set_status("运行时尚未就绪，请稍候再试".into());
            } else if app.get_busy() {
                app.set_status("正在生成回复，请先停止或稍候".into());
            } else {
                app.set_status("正在加载会话，请稍候再试".into());
            }
            return;
        }
        let session = app.get_active_session_id().to_string();
        let content = app.get_draft().trim().to_string();
        let attachments = app
            .get_pending_attachments()
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
        let stream = match state.borrow().desktop.send_chat(NativeChatInput {
            session_id: session.clone(),
            content: content.clone(),
            client_message_id: None,
            reasoning_effort,
            attachments,
        }) {
            Ok(stream) => stream,
            Err(error) => {
                app.set_status(format!("无法发送：{error}").into());
                return;
            }
        };
        // The live turn appends to the timeline that already holds this
        // thread's history; nothing is rebuilt from the conversation list.
        let timeline = state.borrow().timeline.clone();
        {
            let mut live = timeline.borrow_mut();
            live.set_highlight(true);
            live.begin_turn(&session, &display_content);
            publish_timeline(&app, &live);
        }
        app.set_draft("".into());
        app.set_pending_attachments(ModelRc::default());
        app.set_busy(true);
        // A fresh turn starts without a stale queue banner.
        app.set_cloud_queue_position(-1);
        set_session_status(&app, &session, "running");
        app.set_follow_output(true);
        app.set_stopping(false);
        app.set_stream_bytes(0);
        app.set_stream_updates(0);
        app.set_stream_max_ui_ms(0.0);
        app.set_stream_max_backlog(0);
        state.borrow_mut().observation.detach();
        // Plan state is session-scoped: a new turn resumes the thread's last
        // published checklist; file counters always start from zero.
        let mut output = TurnOutput::new(session.clone(), timeline);
        if let Some((explanation, steps)) = state.borrow().plans.get(&session) {
            output.plan_explanation = explanation.clone();
            output.plan_steps = steps.clone();
        }
        state.borrow_mut().active = Some(Active { stream, output });
        start_timer(&app, state.clone());
    });
}

fn bind_attachments(app: &MainWindow) {
    let weak = app.as_weak();
    app.on_capture_screenshot(move |hide_window, region| {
        if region {
            crate::screenshot::capture(weak.clone(), hide_window);
        } else {
            crate::screenshot::capture_fullscreen(weak.clone(), hide_window);
        }
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
}

// Keep the work-dock activity control in sync without inspecting message text.
// This mirrors the web task list: running wins over pending, and only running
// or pending threads contribute to the count and optional list filter.
fn update_active_thread_filter(app: &MainWindow, rows: impl IntoIterator<Item = Conversation>) {
    let mut count = 0;
    let mut running = false;
    for row in rows {
        let status = runtime_status_kind(row.runtime_status.as_str());
        if status == "running" {
            running = true;
            count += 1;
        } else if status == "pending" {
            count += 1;
        }
    }
    app.set_active_thread_count(count);
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
        if app.get_stopping() {
            return;
        }
        let mut current = state.borrow_mut();
        if let Some(active) = current.active.as_mut() {
            app.set_stopping(true);
            active.stream.cancel();
            active.state = "已停止".into();
            active.timeline.borrow_mut().flush();
            set_session_status(&app, &active.session, &active.state);
            app.set_status("正在停止…".into());
        } else if app.get_busy() || app.get_goal_active() {
            app.set_stopping(true);
            let desktop = current.desktop.clone();
            let session = app.get_active_session_id().to_string();
            let weak = app.as_weak();
            std::thread::spawn(move || {
                let result = desktop.cancel_chat(&session);
                let _ = weak.upgrade_in_event_loop(move |app| {
                    if app.get_active_session_id() == session {
                        app.set_stopping(false);
                        match result {
                            Err(error) => app.set_status(format!("无法停止：{error}").into()),
                            Ok(()) => {
                                app.set_goal_active(false);
                                app.invoke_refresh_chat();
                            }
                        }
                    }
                });
            });
        }
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
fn start_goal_clock(app: slint::Weak<MainWindow>) {
    slint::Timer::single_shot(Duration::from_secs(1), move || {
        let Some(app) = app.upgrade() else { return };
        if app.get_goal_active() && !app.get_goal_paused() && !app.get_goal_objective().is_empty() {
            set_goal_clock(&app, app.get_goal_seconds() + 1);
        }
        start_goal_clock(app.as_weak());
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
            NativeChatCommand::GoalSet(objective),
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
        let command = if app.get_goal_paused() {
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
            // The turn is taken out of the state for the tick so the flush can
            // also touch the plan cache and signature without fighting the
            // active borrow; it goes back (or drops on completion) below.
            let mut live_turn = state.active.take();
            let Some(active) = live_turn.as_mut() else {
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
                let event = match active.stream.try_recv() {
                    Ok(Some(event)) => event,
                    Ok(None) => break,
                    Err(error) => {
                        active.state = error.into();
                        done = true;
                        break;
                    }
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
                                app.set_cloud_queue_position(position.clamp(0, i32::MAX as i64) as i32);
                                active.state = format!("模型请求正在排队 · 前方 {position} 个任务");
                            }
                        } else if app.get_cloud_queue_position() >= 0 {
                            app.set_cloud_queue_position(-1);
                        }
                        match apply_event(active, &mut timeline.borrow_mut(), &event) {
                            Ok(changed) => dirty |= changed,
                            Err(error) => {
                                active.state = error;
                                done = true;
                            }
                        }
                    }
                    NativeChatEvent::Queued => {
                        active.state = "任务已排队…".into();
                        dirty = true;
                    }
                    NativeChatEvent::Failed(error) => {
                        active.state = error;
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
                publish_timeline(&app, &live);
                app.set_stream_bytes(live.body_text().len().min(i32::MAX as usize) as i32);
                drop(live);
                app.set_stream_updates(app.get_stream_updates().wrapping_add(1));
                app.set_status(active.state.as_str().into());
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
                live.flush();
                publish_timeline(&app, &live);
                drop(live);
                set_session_status(&app, &active.session, &active.state);
                app.set_busy(false);
                app.set_stopping(false);
                // Idle hides the whole dock row; plan and file stats stay in
                // the attributes and reappear with the next turn's flush.
                app.set_activity_active(false);
                app.set_status(active.state.as_str().into());
                state.timer.stop();
                let _ = desktop;
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
    if data["session_id"]
        .as_str()
        .is_some_and(|id| id != active.session)
    {
        return Ok(false);
    }
    if active.state == "已停止" {
        return Ok(false);
    }
    let round = data["model_round"].as_i64().unwrap_or(active.round);
    if round < active.round && matches!(kind, "llm_output_delta" | "llm_output" | "delta") {
        return Ok(false);
    }
    // A new model round ends the previous round's segments: the answer block
    // freezes and the tool batch that produced it closes. The next text delta
    // then opens a fresh block, which is what makes one entry per round.
    let streamed = matches!(kind, "llm_output_delta" | "llm_output" | "delta");
    if round > active.round && active.round > 0 && streamed {
        timeline.close_segments();
    }
    if matches!(
        kind,
        "llm_output_delta" | "llm_output" | "llm_request" | "delta" | "final"
    ) {
        active.round = active.round.max(round);
    }
    match kind {
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
            // Background goal executions restart model_round at one while the
            // timeline keeps the earlier entries of the same user turn.
            active.round = 0;
            timeline.close_segments();
            active.state = "目标继续执行…".into();
        }
        "native_execution_status" => {
            active.state = match data["status"].as_str() {
                Some("queued") => "正在排队",
                Some("running") => "正在生成…",
                Some("cancelled" | "interrupted") => "已停止",
                Some("failed") => "执行失败",
                Some("waiting_input" | "waiting_user_input") => "等待用户输入",
                _ => "目标继续执行…",
            }
            .into();
        }
        "native_stats" => return Ok(true),
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
                    &crate::turn_stats::metrics(stats),
                ));
            }
            if kind == "final" {
                active.state = "任务完成".into();
            }
            return Ok(true);
        }
        "turn_terminal" => {
            // A terminal event may be the only place a very short turn's
            // statistics arrive.
            if let Some(stats) = crate::turn_stats::message_stats(data) {
                timeline.set_turn_stats(crate::timeline::stat_metrics(
                    &crate::turn_stats::metrics(stats),
                ));
            }
            active.state = match data["status"].as_str() {
                Some("cancelled" | "canceled") => "已停止",
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
            active.upsert_workflow(timeline, NativeWorkflowEntry::from_payload(
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
            ));
        }
        "queue_finish" => active.state = "任务完成".into(),
        "plan_update" => {
            // The plan tool publishes the whole checklist each time, so a
            // wholesale replace is correct. Bounded: a runaway plan must not
            // grow the dock model without limit.
            active.plan_explanation = data["explanation"]
                .as_str()
                .unwrap_or_default()
                .to_string();
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
            // apply_patch results carry the patch summary; the dock's file
            // counters accumulate across the turn (missing fields count as 0).
            if kind == "tool_result" && tool == "apply_patch" {
                active.files_changed += data["changed_files"].as_i64().unwrap_or(0);
                active.lines_added += data["added_lines"].as_i64().unwrap_or(0);
                active.lines_deleted += data["deleted_lines"].as_i64().unwrap_or(0);
            }
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
            active.upsert_workflow(timeline, NativeWorkflowEntry::from_payload(
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
            ));
        }
        "command_session_start" | "command_session_delta"
        | "command_session_status" | "command_session_exit" | "command_session_summary" => {
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
                let terminal =
                    matches!(kind, "command_session_exit" | "command_session_summary");
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
                active.upsert_workflow(timeline, NativeWorkflowEntry::from_payload(
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
                    data,
                ));
                active.state = "正在执行工具…".into();
            }
        }
        "error" | "queue_fail" => {
            active.state = data["message"].as_str().unwrap_or("执行失败").into()
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
    if let Some(seconds) =
        sources
            .iter()
            .find_map(|source| stat_number(source, &["interaction_duration_s", "duration_s", "elapsed_s"]))
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
    use serde_json::json;

    /// The statistics footer must land on the body block that ends the turn,
    /// both when the runtime rides it on the final answer and when it only
    /// arrives with the terminal event.
    #[test]
    fn turn_statistics_reach_the_body_block_from_either_event() {
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
        let body_stats = |timeline: &Rc<RefCell<crate::timeline::Timeline>>| {
            let live = timeline.borrow();
            let model = live.model();
            let row = (0..slint::Model::row_count(&model))
                .filter_map(|index| slint::Model::row_data(&model, index))
                .find(|row| row.kind == crate::timeline::KIND_BODY)?;
            Some(
                row.stats
                    .iter()
                    .map(|metric| (metric.key.to_string(), metric.value.to_string()))
                    .collect::<Vec<_>>(),
            )
        };
        assert_eq!(
            body_stats(&timeline),
            Some(Vec::new()),
            "a streamed block has no metrics yet",
        );

        // The final answer carries the turn's statistics.
        reduce(
            &mut output,
            json!({"event":"final","data":{
                "answer":"Fixture answer",
                "meta":{"message_stats":{
                    "interaction_duration_s": 12.44,
                    "request_consumed_tokens": 4096,
                    "toolCalls": 3,
                    "contextTokens": 12345,
                    "avg_model_round_speed_rounds": 2.0,
                    "avg_model_round_speed_tps": 68234.0
                }}
            }}),
        );
        let metrics = body_stats(&timeline).expect("a body row");
        assert_eq!(
            metrics,
            vec![
                ("stopwatch".to_string(), "12.4s".to_string()),
                ("gauge-high".to_string(), "68.2k/s".to_string()),
                ("layer-group".to_string(), "12.3k".to_string()),
                ("bolt".to_string(), "4.1k".to_string()),
                ("screwdriver-wrench".to_string(), "3".to_string()),
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
            .find(|row| row.kind == crate::timeline::KIND_BODY)
            .map(|row| {
                row.stats
                    .iter()
                    .map(|metric| (metric.key.to_string(), metric.value.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        // A zero tool count is dropped rather than rendered as an empty metric.
        assert_eq!(metrics, vec![("stopwatch".to_string(), "1.5s".to_string())]);
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
                crate::timeline::KIND_DIVIDER,
                crate::timeline::KIND_USER,
                crate::timeline::KIND_GROUP,
                crate::timeline::KIND_REASON,
                crate::timeline::KIND_TOOL,
                crate::timeline::KIND_BODY,
            ]
        );
        let group = &rows[2];
        assert_eq!(group.text, "执行工具 1 次");
        assert!(group.open && group.group_idx == 0);
        assert_eq!(rows[3].tool_name, "已思考");
        assert_eq!(rows[3].status_kind, crate::timeline::STATUS_DONE);
        assert!(rows[3].detail.contains("Fixture thinking"));
        assert_eq!(rows[4].tool_name, "读取文件");
        assert_eq!(rows[4].target, "example.txt");
        assert_eq!(rows[4].status_kind, crate::timeline::STATUS_DONE);
        assert_eq!(rows[4].payload, 4);
        assert_eq!(rows[5].text, "Fixture answer");
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
    }

    /// A stopped turn keeps the partial answer and marks the tail, and later
    /// events never reopen it.
    #[test]
    fn stop_and_terminal_state_freeze_the_tail_without_new_rows() {
        let timeline = Rc::new(RefCell::new(crate::timeline::Timeline::new()));
        let mut output = TurnOutput::new("fixture-session".into(), timeline.clone());
        timeline.borrow_mut().begin_turn("fixture-root", "Fixture input");
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
}
