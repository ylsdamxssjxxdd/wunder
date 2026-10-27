//! Passive native updates are independent of locally submitted executions.
use super::*;
use wunder_desktop::native::{
    NativeChatTurn, NativeThreadUpdate, NativeThreadWatch, NativeThreadWatchCancel,
};
use wunder_server::stable_core::thread_view::ChatViewRow;

pub(super) struct Observation {
    session: String,
    /// Worker-to-UI delivery stays bounded.  A worker blocks on the native
    /// receiver and wakes Slint only when a durable update arrives, instead
    /// of making the UI poll an empty channel every frame.
    updates: std::sync::mpsc::Receiver<(u64, NativeThreadUpdate)>,
    update_sender: std::sync::mpsc::SyncSender<(u64, NativeThreadUpdate)>,
    watch_cancel: Option<NativeThreadWatchCancel>,
    forward_generation: u64,
    /// A bounded forward queue can fill while the UI is blocked in a modal or
    /// the OS suspends it.  Record only its generation, then rebuild one
    /// compact snapshot rather than retain an unbounded change backlog.
    overflow_generation: std::sync::Arc<std::sync::atomic::AtomicU64>,
    turns: Vec<NativeChatTurn>,
    /// Durable rows of the history, rebuilt only when a turn actually changes.
    rows: Vec<TimelineRow>,
    list_refreshed: Option<Instant>,
    /// Backoff for a feed that closes immediately. Without this guard a
    /// broken/closing subscription would spawn one replacement per UI
    /// frame and turn a transient transport error into an unbounded task
    /// storm.
    watch_retry_at: Option<Instant>,
    history_dirty: bool,
    ready: bool,
}

impl Default for Observation {
    fn default() -> Self {
        let (update_sender, updates) = std::sync::mpsc::sync_channel(8);
        Self {
            session: String::new(),
            updates,
            update_sender,
            watch_cancel: None,
            forward_generation: 0,
            overflow_generation: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(u64::MAX)),
            turns: Vec::new(),
            rows: Vec::new(),
            list_refreshed: None,
            watch_retry_at: None,
            history_dirty: false,
            ready: false,
        }
    }
}

impl Observation {
    pub(super) fn detach(&mut self) {
        if let Some(cancel) = self.watch_cancel.take() {
            cancel.cancel();
        }
        self.forward_generation = self.forward_generation.wrapping_add(1);
        self.overflow_generation
            .store(u64::MAX, std::sync::atomic::Ordering::Release);
        while self.updates.try_recv().is_ok() {}
        self.ready = false;
        self.session.clear();
        self.turns.clear();
        self.rows.clear();
        self.watch_retry_at = None;
        self.history_dirty = false;
    }

    fn attach_watch(&mut self, app: &MainWindow, watch: NativeThreadWatch, session: &str) {
        if let Some(cancel) = self.watch_cancel.take() {
            cancel.cancel();
        }
        self.forward_generation = self.forward_generation.wrapping_add(1);
        let generation = self.forward_generation;
        let cancel = watch.cancel_handle();
        self.watch_cancel = Some(cancel.clone());
        let sender = self.update_sender.clone();
        let overflow_generation = self.overflow_generation.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let mut watch = watch;
            while !cancel.is_cancelled() {
                let Some(update) = watch.blocking_recv() else {
                    break;
                };
                // Never wait behind a stopped UI.  If the bounded queue is
                // full, the next UI wake replaces it with one compact durable
                // snapshot, so no current state is lost.
                match sender.try_send((generation, update)) {
                    Ok(()) => {}
                    Err(std::sync::mpsc::TrySendError::Full(_)) => {
                        overflow_generation.store(generation, std::sync::atomic::Ordering::Release);
                        let _ = weak.upgrade_in_event_loop(move |app| {
                            app.invoke_native_thread_update_ready();
                        });
                        break;
                    }
                    Err(std::sync::mpsc::TrySendError::Disconnected(_)) => break,
                }
                if cancel.is_cancelled() {
                    break;
                }
                let _ = weak.upgrade_in_event_loop(move |app| {
                    app.invoke_native_thread_update_ready();
                });
            }
        });
        self.session = session.to_string();
        self.watch_retry_at = None;
    }

    pub(super) fn is_ready(&self) -> bool {
        self.ready
    }

    /// Adopt a durable snapshot obtained while the selection worker was
    /// acquiring the thread lease. This puts meaningful history on screen
    /// before the passive watcher performs its own refresh.
    pub(super) fn seed(&mut self, session: &str, turns: Vec<NativeChatTurn>) {
        self.session = session.to_string();
        self.turns = turns;
        self.rows = project_history(&self.turns);
        self.ready = true;
    }

    pub(super) fn prepend(&mut self, mut turns: Vec<NativeChatTurn>) -> bool {
        turns.retain(|turn| !self.turns.iter().any(|old| old.root_id == turn.root_id));
        if turns.is_empty() {
            return false;
        }
        turns.append(&mut self.turns);
        self.turns = turns;
        self.turns.sort_by_key(|turn| turn.user.user_round);
        self.rows = project_history(&self.turns);
        true
    }

    /// A watcher reset only carries its bounded newest page. Preserve pages
    /// that the reader already fetched above it, then refresh overlapping
    /// roots from the durable source. This lets a reconnect happen at any
    /// point in an infinite scroll without snapping history back to 12 turns.
    pub(super) fn merge_reset(&mut self, turns: Vec<NativeChatTurn>) -> bool {
        let mut changed = false;
        for turn in turns {
            match self
                .turns
                .iter_mut()
                .find(|old| old.root_id == turn.root_id)
            {
                Some(old) if *old != turn => {
                    *old = turn;
                    changed = true;
                }
                Some(_) => {}
                None => {
                    self.turns.push(turn);
                    changed = true;
                }
            }
        }
        if changed {
            self.turns.sort_by_key(|turn| turn.user.user_round);
        }
        changed
    }

    /// Remove a withdrawn queue placeholder before the watcher catches up.
    pub(super) fn remove_turn(&mut self, root_id: &str) -> bool {
        let before = self.turns.len();
        self.turns.retain(|turn| turn.root_id != root_id);
        if self.turns.len() == before {
            return false;
        }
        self.rows = project_history(&self.turns);
        true
    }

    /// Publish the durable history into the shared model without touching the
    /// live turn that is already appended to it.
    pub(super) fn publish(
        &self,
        app: &MainWindow,
        timeline: &Rc<RefCell<crate::timeline::Timeline>>,
    ) {
        let root = crate::native_chat::markdown_root(app);
        let mut live = timeline.borrow_mut();
        live.set_markdown_root(root);
        live.publish_history(self.rows.clone());
        live.hydrate_markdown_images();
        crate::native_chat::publish_timeline_passive(app, &live);
    }
}

pub(super) fn install(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    let state_weak = Rc::downgrade(&state);
    app.on_native_thread_update_ready(move || {
        let (Some(app), Some(state)) = (weak.upgrade(), state_weak.upgrade()) else {
            return;
        };
        let mut current = state.borrow_mut();
        if app.get_cloud_chat_active() {
            current.observation.detach();
            return;
        }
        // The catalogue is bounded to 100 rows and is the only status
        // source for threads the selected-thread watcher cannot see:
        // background runs settle while this window pumps a live turn, and
        // discovery covers a new channel route. A live turn must not
        // starve that poll, or every other entry's status marker freezes
        // until the run finishes.
        // A running local turn changes the sidebar status several times
        // per second. Keep the catalogue as a bounded backstop at a
        // faster cadence while work is active; the direct event
        // projection remains the primary path for the selected row.
        let refresh_interval = if current.active.is_some() || !current.parked.is_empty() {
            Duration::from_millis(500)
        } else {
            Duration::from_secs(3)
        };
        let refresh = current
            .observation
            .list_refreshed
            .is_none_or(|time| time.elapsed() >= refresh_interval);
        if refresh {
            current.observation.list_refreshed = Some(Instant::now());
        }
        // Queue rows are a live admission projection too. Keep polling
        // them while a foreground or parked turn exists so an interject
        // and its successor are reflected even when the durable turn
        // watcher has not emitted a new history row yet.
        let queue_active = current.active.is_some() || !current.parked.is_empty();
        if queue_active {
            queue::schedule_due(&mut current.queue_due, Duration::from_millis(500));
        }
        let desktop = current.desktop.clone();
        // Cloned before the observation borrow: the history rows and a live
        // turn share one model.
        let timeline = current.timeline.clone();
        timeline
            .borrow_mut()
            .set_markdown_root(crate::native_chat::markdown_root(&app));
        let session = app.get_active_session_id().to_string();
        let initial = crate::native_chat::take_initial_history(&session);
        let overflowed = current
            .observation
            .overflow_generation
            .swap(u64::MAX, std::sync::atomic::Ordering::AcqRel)
            == current.observation.forward_generation;
        if overflowed {
            if let Some(cancel) = current.observation.watch_cancel.take() {
                cancel.cancel();
            }
            while current.observation.updates.try_recv().is_ok() {}
        }
        if current.observation.session != session {
            current.observation.detach();
            if !session.is_empty() {
                // Thread selection acquires its lease and reads the compact
                // history on a worker. Do not attach a cold watcher while
                // that request is in flight: it would immediately repeat
                // both the lease touch and the HDD history read, and then
                // consume the worker snapshot too late to use it. An
                // opening error clears `session_loading`, which makes the
                // next tick attach cold and report watcher errors normally.
                if initial.is_none() && app.get_session_loading() {
                    return;
                }
                current.observation.session = session.clone();
                if let Some((turns, _, history_before, history_has_more)) = initial.as_ref() {
                    // Project the background snapshot immediately. The
                    // watch still verifies and follows durable changes,
                    // but the loading overlay can now leave without a
                    // blank welcome screen between threads.
                    current.observation.seed(&session, turns.clone());
                    current.history_before = *history_before;
                    current.history_has_more = *history_has_more && history_before.is_some();
                    current.history_loading_older = false;
                    current.observation.publish(&app, &timeline);
                    app.set_session_loading(false);
                }
                // The thread's last published plan and a pending 问询面板
                // are durable presentation metadata; both come back with
                // the history instead of living only in a live turn. This
                // runs once per open, before the watcher is attached.
                if let Some((turns, _, _, _)) = initial.as_ref() {
                    crate::native_chat::restore_thread_extras(&app, &mut current, &session, turns);
                }
                let (turns, cursor) = initial
                    .map(|(turns, cursor, _, _)| (Some(turns), Some(cursor)))
                    .unwrap_or((None, None));
                let watch = desktop.watch_chat_with_initial_cursor(&session, turns, cursor);
                current.observation.attach_watch(&app, watch, &session);
            }
        } else if current.observation.watch_cancel.is_none()
            && !session.is_empty()
            && current
                .observation
                .watch_retry_at
                .is_none_or(|retry| retry <= Instant::now())
        {
            // A feed can close transiently while the runtime continues
            // committing the turn (for example during an interject). Do
            // not leave the selected thread permanently detached; attach
            // a fresh bounded watcher on the next UI frame.
            let watch = desktop.watch_chat_with_initial_cursor(&session, None, None);
            current.observation.attach_watch(&app, watch, &session);
        }
        let local_busy = current.active.is_some()
            || !current.parked.is_empty()
            || current
                .submitting
                .as_ref()
                .is_some_and(|pending| pending.session == session);
        let mut reconciled_history = false;
        let observation = &mut current.observation;
        let mut dirty = false;
        let mut reset = false;
        let mut queue_changed = false;
        let started = Instant::now();
        for _ in 0..8 {
            let Ok((generation, update)) = observation.updates.try_recv() else {
                break;
            };
            if generation != observation.forward_generation {
                continue;
            }
            match update {
                NativeThreadUpdate::Reset(turns) => {
                    observation.ready = true;
                    observation.watch_retry_at = None;
                    dirty |= observation.merge_reset(turns);
                    reset = true;
                    app.set_session_loading(false);
                }
                NativeThreadUpdate::Turns(turns) => {
                    for turn in turns {
                        match observation
                            .turns
                            .iter_mut()
                            .find(|old| old.root_id == turn.root_id)
                        {
                            Some(old) => {
                                if *old != turn {
                                    *old = turn;
                                    dirty = true;
                                }
                            }
                            None => {
                                observation.turns.push(turn);
                                dirty = true;
                            }
                        }
                    }
                    if dirty {
                        observation.turns.sort_by_key(|turn| turn.user.user_round);
                    }
                }
                NativeThreadUpdate::Context(usage) => apply_context_usage(&app, usage),
                NativeThreadUpdate::QueueChanged => {
                    queue_changed = true;
                }
                NativeThreadUpdate::Goal(goal) => {
                    app.set_goal_active(goal.active());
                    app.set_goal_visible(goal.visible());
                    app.set_goal_objective(goal.objective.clone().into());
                    app.set_goal_phase(goal.phase.clone().into());
                    app.set_goal_activation(goal.activation.clone().into());
                    app.set_goal_rounds(
                        format!("{}/{}", goal.rounds_started, goal.max_goal_rounds).into(),
                    );
                    app.set_goal_paused(goal.paused());
                    if goal.active() && !goal.paused() && !goal.objective.is_empty() {
                        crate::native_chat::start_goal_clock(app.as_weak());
                    }
                    let rows = app.get_conversations();
                    if let Some(index) = rows.iter().position(|row| row.id == session) {
                        if let Some(mut row) = rows.row_data(index) {
                            row.goal_active = goal.active();
                            row.goal_visible = goal.visible();
                            row.goal_objective = goal.objective.clone().into();
                            row.goal_phase = goal.phase.clone().into();
                            row.goal_activation = goal.activation.clone().into();
                            row.goal_rounds =
                                format!("{}/{}", goal.rounds_started, goal.max_goal_rounds).into();
                            row.goal_paused = goal.paused();
                            rows.set_row_data(index, row);
                            crate::navigation_ui::project(&app);
                        }
                    }
                }
                NativeThreadUpdate::Failed(error) => {
                    if let Some(cancel) = observation.watch_cancel.take() {
                        cancel.cancel();
                    }
                    observation.watch_retry_at = Some(Instant::now() + Duration::from_millis(250));
                    // Keep the existing projection visible and let the
                    // next frame reconnect the watcher. A transient feed
                    // close must not require navigation to restore live
                    // status, queue and context updates.
                    observation.ready = true;
                    app.set_session_loading(false);
                    app.set_status(error.into());
                    let weak = app.as_weak();
                    slint::Timer::single_shot(Duration::from_millis(250), move || {
                        if let Some(app) = weak.upgrade() {
                            app.invoke_native_thread_update_ready();
                        }
                    });
                }
            }
            if started.elapsed() > Duration::from_millis(5) {
                break;
            }
        }
        // While the foreground reducer owns a temporary tail, durable
        // rows are accumulated but never installed into the shared model:
        // their real root id differs from the client request id and would
        // otherwise replace the live bubble mid-stream. Reconcile once
        // the foreground owner has settled.
        let publish_history = !local_busy;
        if (dirty || reset || observation.history_dirty) && publish_history {
            // The durable rows are rebuilt only when a turn changed, and
            // only for the turns the observer actually holds.
            observation.rows = project_history(&observation.turns);
            observation.publish(&app, &timeline);
            reconciled_history = true;
            let durable_busy = observation.turns.iter().any(|turn| {
                matches!(
                    turn.assistant.stats_status.as_str(),
                    "正在生成…" | "正在排队"
                ) || turn.assistant.stats_status.starts_with("正在排队 ·")
            });
            // A durable snapshot can lag the foreground stream by one
            // commit. Never let that lag clear the local busy indicator or
            // stop the composer while the stream/parked hand-off is live.
            let busy = durable_busy || local_busy;
            let finished = app.get_busy() && !busy && !local_busy;
            app.set_busy(busy);
            // The activity capsule is a separate projection from the
            // composer busy flag. Keep its spinner aligned with durable
            // execution too, including runs resumed after an interject or
            // while the foreground stream is temporarily unavailable.
            app.set_activity_running(busy);
            if !local_busy {
                if let Some(latest) = observation.turns.last() {
                    crate::native_chat::set_session_status(
                        &app,
                        &session,
                        latest.assistant.stats_status.as_str(),
                        crate::runtime_phase::Phase::of(latest.assistant.stats_status.as_str()),
                    );
                }
            }
            if finished {
                crate::workspace_files::execution_finished(&app, desktop.clone());
            }
            if !busy {
                app.set_stopping(false);
            }
        }
        // The poll cadence was resolved before the watcher drained, so a
        // live turn and an idle tick share one refresh schedule.
        if reconciled_history {
            observation.history_dirty = false;
        } else if (dirty || reset) && local_busy {
            observation.history_dirty = true;
        }
        let queue_due = queue_active && queue::due_arrived(&mut current.queue_due);
        drop(current);
        if refresh {
            refresh_chat(&app, state.clone(), true);
        }
        if queue_changed || queue_due {
            let session = app.get_active_session_id().to_string();
            refresh_queue_async(&app, &state, &session);
        }
    });
}

/// Durable history uses the same single work-process fold as streaming. Batch
/// identities are retained for call ownership but never add a visible header.
pub(crate) fn project_history(turns: &[NativeChatTurn]) -> Vec<TimelineRow> {
    let mut rows = Vec::new();
    // Fold identity is model-wide: `toggle` folds by id, so no two batches or
    // processes may share one, not even across turns.
    let mut batch = 0i32;
    let mut process = 0i32;
    for turn in turns {
        // Queue rows are rendered by the composer. Their durable turn is only
        // an admission placeholder and must not become a second user bubble.
        if is_queue_only_turn(turn) {
            continue;
        }
        let mut user = history_user(&turn.user);
        // The bubble carries the turn identity: it is the anchor the reducer's
        // live rows are matched against when the durable snapshot takes over.
        user.id = format!("user-turn-{}", turn.root_id).into();
        rows.push(user);
        process += 1;
        let process_bar_at = rows.len();
        let mut bar = blank_row(
            crate::timeline::KIND_PROCESS,
            format!("process-{}", turn.root_id),
        );
        bar.payload = process_bar_at as i32;
        bar.process_idx = process;
        bar.process_root = turn.root_id.as_str().into();
        bar.text = crate::timeline_text::process_label(0).into();
        bar.visible = false;
        rows.push(bar);
        let mut answer_at = None;
        // Header snapshots intentionally defer completed process details. The
        // process bar remains an actionable lightweight row; its first open
        // requests the normal keyset page in `chat_process`.
        if turn.rounds.is_empty() {
            let mut answer = blank_row(
                crate::timeline::KIND_BODY,
                format!("answer-{}", turn.root_id),
            );
            answer.text = turn.assistant.text.as_str().into();
            answer.blocks = crate::message_blocks::from_text(&turn.assistant.text);
            answer.foldable = true;
            answer_at = Some(rows.len());
            rows.push(answer);
        }
        for round in &turn.rounds {
            let tools = round.items.len() as i32;
            let activity = !round.reasoning.is_empty() || tools > 0;
            if !activity {
                // A plain-text round has no batch: the answer alone carries it.
                if !round.text.is_empty() {
                    let mut body = blank_row(
                        crate::timeline::KIND_BODY,
                        format!("body-{}-{}", round.execution, round.round),
                    );
                    body.text = round.text.as_str().into();
                    body.blocks = crate::message_blocks::from_text(&round.text);
                    body.foldable = true;
                    if round.is_answer {
                        answer_at = Some(rows.len());
                    }
                    rows.push(body);
                }
                continue;
            }
            let group = batch;
            batch += 1;
            // The folded bar reads out the newest entry of its batch, exactly
            // as the live reducer stamps it.
            let group_bar_at = rows.len();
            let mut latest = slint::SharedString::default();
            let mut bar = blank_row(
                crate::timeline::KIND_GROUP,
                format!("group-{}-{}", round.execution, round.round),
            );
            bar.text = crate::timeline_text::tool_calls(tools).into();
            bar.payload = group_bar_at as i32;
            bar.group_idx = group;
            bar.visible = false;
            bar.group_open = true;
            bar.open = true;
            bar.foldable = true;
            rows.push(bar);
            if !round.reasoning.is_empty() {
                let mut reason = blank_row(
                    crate::timeline::KIND_REASON,
                    // Match the durable process pager (`reason-{item_id}`)
                    // instead of deriving an id from a round ordinal. A
                    // continuation can restart those ordinals, and differing
                    // ids made one thought appear once in the live summary
                    // and again after the process page arrived.
                    format!(
                        "reason-{}",
                        if round.item_id.is_empty() {
                            format!("{}-{}", round.execution, round.round)
                        } else {
                            round.item_id.clone()
                        }
                    ),
                );
                reason.tool_name = crate::timeline_text::thought_done().into();
                reason.summary = crate::message_blocks::reasoning_preview(&round.reasoning)
                    .as_str()
                    .into();
                latest = reason.summary.clone();
                reason.detail = round.reasoning.as_str().into();
                reason.payload = rows.len() as i32;
                reason.group_idx = group;
                reason.group_open = true;
                reason.foldable = true;
                rows.push(reason);
            }
            if !round.text.is_empty() {
                let mut body = blank_row(
                    crate::timeline::KIND_BODY,
                    format!("body-{}-{}", round.execution, round.round),
                );
                body.text = round.text.as_str().into();
                body.blocks = crate::message_blocks::from_text(&round.text);
                body.foldable = true;
                if round.is_answer {
                    answer_at = Some(rows.len());
                }
                rows.push(body);
            }
            for item in &round.items {
                let mut row = blank_row(crate::timeline::KIND_TOOL, format!("tool-{}", item.id));
                let label = crate::timeline_text::tool_label(&item.title);
                let (target, patch) = crate::timeline_text::tool_target(item);
                row.tool_name = label.as_str().into();
                row.tool_icon = crate::tool_icons::workflow_icon(&item.title).into();
                row.target = target.as_str().into();
                row.tool_metrics = crate::timeline_text::tool_metrics(item).into();
                row.tool_metrics_visible = !row.tool_metrics.is_empty();
                row.summary = crate::timeline_text::tool_summary(item).as_str().into();
                latest = row.summary.clone();
                row.detail = crate::timeline_text::copy_text(&item.sections, item.detail.as_str());
                row.sections = crate::timeline_text::sections(&item.sections);
                row.tokens = item.tokens.as_str().into();
                row.duration = item.duration.as_str().into();
                row.status_kind = crate::timeline::status_kind(&item.state);
                row.patch = if patch {
                    crate::timeline_text::patch_cards(&item.patch_files)
                } else {
                    ModelRc::default()
                };
                row.payload = rows.len() as i32;
                row.group_idx = group;
                row.group_open = true;
                row.foldable = true;
                rows.push(row);
            }
            rows[group_bar_at].summary = latest;
        }
        // Child runs render as cards at the turn's tail, the same place the web
        // messenger parks its subagent panel. The projection is shared with the
        // live reducer, so a card looks and behaves identically in both paths.
        for card in &turn.subagents {
            if card.session_id.is_empty() {
                continue;
            }
            let mut row = crate::timeline::subagent_row(card);
            row.payload = rows.len() as i32;
            rows.push(row);
        }
        // The footer stays outside the process, including reasoning-only turns.
        let mut footer = blank_row(
            crate::timeline::KIND_FOOTER,
            format!("footer-{}", turn.root_id),
        );
        footer.stats = turn_stats(
            &turn.assistant,
            turn.subagents.iter().any(|card| card.is_running()),
        );
        if let Some(body) = answer_at {
            footer.text = rows[body].text.clone();
        }
        rows.push(footer);
        let end = rows.len();
        fold_history_process(
            rows.as_mut_slice(),
            process_bar_at,
            end,
            process,
            &turn.assistant.state,
            answer_at.unwrap_or(usize::MAX),
        );
        if turn.process_steps > 0 {
            rows[process_bar_at].text =
                crate::timeline_text::process_label(turn.process_steps.min(i32::MAX as i64) as i32)
                    .into();
            rows[process_bar_at].visible = true;
        }
    }
    crate::timeline_text::process_models(&mut rows);
    rows
}

/// Project the cloud presentation page into the same row vocabulary used by
/// the local timeline.  Cloud rows deliberately stay bounded by the page
/// returned by the server; the cloud watcher replaces this model on every
/// revision, so the UI never grows an unbounded second transcript.
pub(crate) fn project_cloud_history(
    chat_rows: &[ChatViewRow],
    previous: ModelRc<TimelineRow>,
) -> Vec<TimelineRow> {
    let mut rows = Vec::with_capacity(chat_rows.len().saturating_mul(4));
    for (index, chat) in chat_rows.iter().enumerate() {
        let mut user = blank_row(
            crate::timeline::KIND_USER,
            format!("cloud-user-{}", chat.root_turn_id),
        );
        user.text = chat.user.content.as_str().into();
        user.blocks = previous
            .iter()
            .find(|row| row.id == user.id && row.text == user.text)
            .map(|row| row.blocks)
            .unwrap_or_else(|| crate::message_blocks::from_text(&chat.user.content));
        rows.push(user);

        let mut process = blank_row(
            crate::timeline::KIND_PROCESS,
            format!("cloud-process-{}", chat.root_turn_id),
        );
        process.process_root = chat.root_turn_id.as_str().into();
        process.process_idx = index as i32 + 1;
        process.payload = rows.len() as i32;
        process.text = crate::timeline_text::process_label(
            (chat.workflow_count + chat.reasoning_count).min(i32::MAX as i64) as i32,
        )
        .into();
        process.summary = cloud_status_label(chat.status.as_str()).into();
        process.visible = chat.workflow_count > 0
            || chat.reasoning_count > 0
            || !chat.assistant.reasoning.is_empty();
        process.status_kind = crate::timeline::status_kind(chat.status.as_str());
        let process_id = process.process_idx;
        let running_thought = process.status_kind == crate::timeline::STATUS_RUNNING
            && chat.process_activity.kind == "thinking";
        if running_thought {
            process.summary = crate::timeline_text::thought_running().into();
        }
        rows.push(process);

        // The presentation feed already carries the current thought. Do not
        // wait for the lazy process page (or its 1.5 s refresh) to render it.
        if !chat.assistant.reasoning.is_empty() {
            let id = if chat.reasoning_item_id.is_empty() {
                &chat.assistant.item_id
            } else {
                &chat.reasoning_item_id
            };
            let mut reason = blank_row(crate::timeline::KIND_REASON, format!("reason-{id}"));
            reason.process_idx = process_id;
            reason.tool_name = if running_thought {
                crate::timeline_text::thought_running()
            } else {
                crate::timeline_text::thought_done()
            }
            .into();
            reason.detail = chat.assistant.reasoning.as_str().into();
            reason.summary = crate::message_blocks::reasoning_preview(&chat.assistant.reasoning);
            reason.status_kind = if running_thought {
                crate::timeline::STATUS_RUNNING
            } else {
                crate::timeline::STATUS_DONE
            };
            reason.foldable = true;
            rows.push(reason);
        }

        let mut answer = blank_row(
            crate::timeline::KIND_BODY,
            format!("cloud-answer-{}", chat.root_turn_id),
        );
        let text = if chat.assistant.tail.is_empty() {
            chat.assistant.content.clone()
        } else if chat.assistant.tail_offset == 0 {
            chat.assistant.tail.clone()
        } else if chat.assistant.content.is_empty() {
            format!("…\n{}", chat.assistant.tail)
        } else {
            format!("{}\n…\n{}", chat.assistant.content, chat.assistant.tail)
        };
        answer.text = text.as_str().into();
        answer.status_kind = crate::timeline::status_kind(chat.status.as_str());
        answer.blocks = previous
            .iter()
            .find(|row| {
                row.id == answer.id
                    && row.text == answer.text
                    && row.status_kind == answer.status_kind
            })
            .map(|row| row.blocks)
            .unwrap_or_else(|| {
                let mut blocks = crate::message_blocks::Blocks::new();
                blocks.raw = text;
                if matches!(chat.status.as_str(), "running" | "queued" | "pending") {
                    blocks.flush();
                } else {
                    blocks.finalize_markdown();
                }
                ModelRc::from(blocks.model)
            });
        answer.foldable = true;
        let mut footer = blank_row(
            crate::timeline::KIND_FOOTER,
            format!("cloud-footer-{}", chat.root_turn_id),
        );
        footer.summary = answer.id.clone();
        footer.text = answer.text.clone();
        footer.stats = ModelRc::new(VecModel::from(crate::timeline::stat_metrics(
            &crate::turn_stats::with_status(
                crate::turn_stats::metrics(&chat.assistant.stats.clone().unwrap_or(Value::Null)),
                cloud_status_label(chat.status.as_str()),
            ),
        )));
        rows.push(answer);
        rows.push(footer);
    }
    for (index, row) in rows.iter_mut().enumerate() {
        row.payload = index as i32;
    }
    rows
}

fn cloud_status_label(status: &str) -> &'static str {
    match status {
        "completed" | "done" | "finished" | "success" => "任务完成",
        "failed" | "error" | "rejected" => "执行失败",
        "cancelled" | "canceled" | "stopped" => "已停止",
        "queued" | "pending" => "正在排队",
        "waiting_input" | "waiting_user_input" => "等待用户输入",
        "waiting_approval" => "等待确认",
        _ => "正在生成…",
    }
}

fn is_queue_only_turn(turn: &NativeChatTurn) -> bool {
    let status = turn.assistant.stats_status.trim();
    let queued = status == "正在排队"
        || status == "任务已排队"
        || status.starts_with("正在排队 ·")
        || status.starts_with("任务已排队 ·");
    queued
        && turn.assistant.text.trim().is_empty()
        && turn.rounds.iter().all(|round| {
            round.reasoning.trim().is_empty()
                && round.text.trim().is_empty()
                && round.items.is_empty()
        })
        && turn.subagents.is_empty()
}

/// Apply the same single process fold as the live reducer, including actions
/// after the newest answer. A stopped turn need not have an answer at all.
fn fold_history_process(
    rows: &mut [TimelineRow],
    bar_at: usize,
    end: usize,
    process: i32,
    state: &str,
    newest: usize,
) {
    let mut steps = 0i32;
    for index in (bar_at + 1)..end {
        if rows[index].kind == crate::timeline::KIND_GROUP
            || crate::timeline::is_process_step(rows[index].kind, index, newest)
        {
            rows[index].process_idx = process;
            steps += i32::from(rows[index].kind != crate::timeline::KIND_GROUP);
        }
    }
    let label = crate::timeline_text::process_label(steps);
    let phase = match state {
        "completed" | "done" | "任务完成" => "已完成",
        "failed" | "error" | "rejected" | "system_error" | "执行失败" => "错误",
        "cancelled" | "stopped" | "interrupted" | "已停止" => "已停止",
        "queued" | "pending" | "正在排队" => "排队中",
        "waiting_input" | "waiting_user_input" | "等待用户输入" => "等待回答",
        "waiting_approval" | "等待确认" => "等待确认",
        _ => "正在处理…",
    };
    let mut activity = phase.to_string();
    if phase == "正在处理…" {
        for row in &rows[(bar_at + 1)..end] {
            if row.status_kind != crate::timeline::STATUS_RUNNING {
                continue;
            }
            if row.kind == crate::timeline::KIND_TOOL {
                activity = format!("执行工具：{}", row.tool_name);
            } else if row.kind == crate::timeline::KIND_REASON {
                activity = crate::timeline_text::thought_running().into();
            }
        }
    }
    let bar = &mut rows[bar_at];
    bar.status_kind = if phase == "正在处理…" {
        crate::timeline::STATUS_RUNNING
    } else if phase == "错误" {
        crate::timeline::STATUS_FAILED
    } else {
        crate::timeline::STATUS_DONE
    };
    bar.text = label.as_str().into();
    bar.summary = activity.into();
    bar.visible = steps > 0;
}

/// The turn's statistics, shared by live and durable independent footer rows.
fn turn_stats(
    assistant: &wunder_desktop::NativeMessage,
    has_running_subagent: bool,
) -> ModelRc<crate::TurnStatMetric> {
    ModelRc::new(VecModel::from(crate::timeline::stat_metrics(
        &crate::turn_stats::with_status(
            crate::turn_stats::with_user_round(
                crate::turn_stats::metrics_from_native(&assistant.stats),
                assistant.user_round,
            ),
            crate::turn_stats::resolve_status(&assistant.stats_status, has_running_subagent),
        ),
    )))
}

fn history_user(message: &wunder_desktop::NativeMessage) -> TimelineRow {
    let mut row = blank_row(crate::timeline::KIND_USER, String::new());
    row.text = message.text.as_str().into();
    row.blocks = crate::message_blocks::from_text(&user_display_source(message));
    row.foldable = false;
    row
}

/// The user bubble keeps its original text for copy; persisted attachments
/// render as trailing image or file cards derived from their stored paths.
fn user_display_source(message: &wunder_desktop::NativeMessage) -> String {
    let mut source = message.text.clone();
    for attachment in &message.attachments {
        let name = attachment.name.replace(['[', ']'], "");
        let name = if name.is_empty() {
            attachment
                .public_path
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or("附件")
                .to_string()
        } else {
            name
        };
        if attachment.public_path.is_empty() {
            source.push_str("\n\n[附件] ");
            source.push_str(&name);
        } else {
            source.push_str("\n\n![");
            source.push_str(&name);
            source.push_str("](<");
            source.push_str(&attachment.public_path);
            source.push_str(">)");
        }
    }
    source
}

fn blank_row(kind: i32, id: String) -> TimelineRow {
    TimelineRow {
        kind,
        id: id.into(),
        visible: true,
        text: Default::default(),
        blocks: ModelRc::default(),
        summary: Default::default(),
        tool_name: Default::default(),
        tool_icon: Default::default(),
        target: Default::default(),
        tool_metrics: Default::default(),
        tool_metrics_visible: false,
        tokens: Default::default(),
        duration: Default::default(),
        detail: Default::default(),
        sections: ModelRc::default(),
        process_entries: ModelRc::default(),
        process_root: Default::default(),
        process_paged: false,
        process_local: false,
        process_older: false,
        process_newer: false,
        process_first: -1,
        process_last: -1,
        process_loading: false,
        process_error: false,
        process_revision: 0,
        status_kind: crate::timeline::STATUS_DONE,
        foldable: false,
        open: false,
        group_idx: crate::timeline::NO_GROUP,
        group_open: false,
        process_idx: crate::timeline::NO_PROCESS,
        process_open: false,
        payload: 0,
        patch: ModelRc::default(),
        stats: ModelRc::default(),
        subagent_id: Default::default(),
        subagent_stoppable: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_reasoning_updates_the_same_paged_child_before_completion() {
        let mut chat = ChatViewRow {
            root_turn_id: "fixture-cloud".into(),
            status: "running".into(),
            reasoning_count: 1,
            reasoning_item_id: "fixture-execution:text-1".into(),
            ..Default::default()
        };
        chat.assistant.reasoning = "First thought".into();
        chat.process_activity.kind = "thinking".into();
        let mut rows = project_cloud_history(&[chat.clone()], ModelRc::default());
        crate::timeline_text::process_models(&mut rows);
        assert!(rows[1].visible);
        assert_eq!(rows[1].process_entries.row_count(), 1);
        let child = rows[1].process_entries.clone();
        assert_eq!(
            child.row_data(0).unwrap().status_kind,
            crate::timeline::STATUS_RUNNING
        );
        rows[1].process_paged = true;
        rows[1].open = true;
        let previous = VecModel::from(rows);
        chat.assistant.reasoning.push_str(" continued");
        let mut next = project_cloud_history(&[chat.clone()], ModelRc::default());
        crate::timeline_text::restore_folds(&mut next, &previous);
        crate::timeline_text::sync_reasoning_tails(&next);
        assert_eq!(next[1].process_entries, child);
        assert_eq!(child.row_count(), 1);
        assert_eq!(child.row_data(0).unwrap().detail, "First thought continued");
        assert!(next[1].open);

        chat.reasoning_item_id = "fixture-execution:text-2".into();
        chat.assistant.reasoning = "Next thought".into();
        let previous = VecModel::from(next);
        let mut next = project_cloud_history(&[chat], ModelRc::default());
        crate::timeline_text::restore_folds(&mut next, &previous);
        crate::timeline_text::sync_reasoning_tails(&next);
        assert_eq!(child.row_count(), 2);
        assert_eq!(
            child.row_data(0).unwrap().status_kind,
            crate::timeline::STATUS_DONE
        );
        assert_eq!(
            child.row_data(1).unwrap().status_kind,
            crate::timeline::STATUS_RUNNING
        );
        assert_eq!(child.row_data(1).unwrap().detail, "Next thought");
    }

    #[test]
    fn cloud_projection_uses_native_rows_and_keeps_frozen_blocks() {
        let mut chat = ChatViewRow {
            root_turn_id: "fixture-cloud".into(),
            workflow_count: 2,
            status: "completed".into(),
            ..Default::default()
        };
        chat.user.content = "Fixture input".into();
        chat.assistant.content = "## Fixture answer\n\n- First item\n- Second item".into();
        let rows = project_cloud_history(&[chat.clone()], ModelRc::default());
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].kind, crate::timeline::KIND_USER);
        assert_eq!(rows[1].kind, crate::timeline::KIND_PROCESS);
        assert_eq!(rows[1].process_root, "fixture-cloud");
        assert_eq!(rows[2].kind, crate::timeline::KIND_BODY);
        assert!(rows[2].blocks.row_count() > 0);
        let held = rows[2].blocks.clone();
        let next = project_cloud_history(&[chat], ModelRc::new(VecModel::from(rows)));
        assert_eq!(next[2].blocks, held);
        assert_eq!(next[1].payload, 1);
    }
    use slint::ModelRc;
    use std::time::Instant;
    use wunder_desktop::native::NativeChatRound;

    #[test]
    fn history_user_keeps_copy_text_and_projects_attachment_cards() {
        let message = wunder_desktop::NativeMessage {
            turn_id: "fixture".into(),
            text: "fixture question".into(),
            mine: true,
            created_at: 0.0,
            state: String::new(),
            stats_status: String::new(),
            user_round: 0,
            stats: Default::default(),
            attachments: vec![wunder_desktop::NativeMessageAttachment {
                name: "fixture.png".into(),
                content_type: "image/png".into(),
                public_path: "/workspaces/fixture/chat_attachments/fixture.png".into(),
            }],
        };
        let row = history_user(&message);
        assert_eq!(row.text.as_str(), "fixture question");
        let kinds = (0..row.blocks.row_count())
            .filter_map(|index| row.blocks.row_data(index))
            .map(|block| block.kind)
            .collect::<Vec<_>>();
        assert!(
            kinds.contains(&crate::message_blocks::KIND_IMAGE),
            "kinds: {kinds:?}"
        );
        // An attachment without a stored path degrades to a labeled text row.
        let orphan = wunder_desktop::NativeMessage {
            attachments: vec![wunder_desktop::NativeMessageAttachment {
                name: "fixture.txt".into(),
                ..Default::default()
            }],
            ..message
        };
        let row = history_user(&orphan);
        let text = (0..row.blocks.row_count())
            .filter_map(|index| row.blocks.row_data(index))
            .map(|block| block.text.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("[附件] fixture.txt"), "text: {text}");
    }

    fn fixture(root: &str, text: &str, status: &str) -> NativeChatTurn {
        let message = wunder_desktop::NativeMessage {
            turn_id: root.into(),
            text: text.into(),
            mine: false,
            created_at: 0.0,
            state: status.into(),
            stats_status: status.into(),
            user_round: 0,
            stats: Default::default(),
            attachments: Vec::new(),
        };
        NativeChatTurn {
            root_id: root.into(),
            user: wunder_desktop::NativeMessage {
                mine: true,
                text: "Fixture input".into(),
                ..message.clone()
            },
            assistant: message,
            rounds: vec![NativeChatRound {
                item_id: format!("{root}:text-1"),
                round: 1,
                execution: root.into(),
                is_answer: true,
                reasoning: String::new(),
                text: text.into(),
                items: Vec::new(),
            }],
            subagents: Vec::new(),
            process_steps: 0,
            plan: None,
            question_panel: None,
        }
    }

    /// Largest workspace file card the projections allow: `patch::project` keeps
    /// at most eight files and `timeline_text::patch_cards` at most 200 lines of
    /// each, so this is the real ceiling, not an invented one.
    fn near_limit_patch() -> Vec<wunder_desktop::NativePatchFile> {
        (0..8)
            .map(|file| wunder_desktop::NativePatchFile {
                path: format!("src/module_{file}.rs"),
                action: "update".into(),
                added: "200".into(),
                deleted: "200".into(),
                lines: (0..200)
                    .map(|line| wunder_desktop::NativePatchLine {
                        text: "    let value = compute(input_value);".into(),
                        kind: if line % 3 == 0 { "add" } else { "context" }.into(),
                        number: (line + 1).to_string(),
                    })
                    .collect(),
            })
            .collect()
    }

    /// Near-limit history: 50 turns (the observer's own cap) with 24 workflow
    /// entries each (`native_chat_turns` caps a turn at 24 entries) and one
    /// max-size patch card among them, plus a bounded answer body.
    fn near_limit_turns() -> Vec<NativeChatTurn> {
        let patch = near_limit_patch();
        let body = (0..40)
            .map(|line| format!("第 {line} 行输出内容，用于测量正文投影成本。"))
            .collect::<Vec<_>>()
            .join("\n\n");
        (0..50)
            .map(|index| {
                let mut turn = fixture(
                    &format!("fixture-turn-{index}"),
                    &format!("{body}\n\n结论：第 {index} 轮。"),
                    "任务完成",
                );
                turn.user.text = format!("第 {index} 轮输入").into();
                turn.rounds[0].items = (0..24)
                    .map(|slot| {
                        let base = NativeWorkflowEntry::from_payload(
                            &format!("fixture-tool-{index}-{slot}"),
                            "read_file",
                            "读取文件 · 完成\nsrc/module_0.rs\nFixture preview".into(),
                            "completed",
                            &serde_json::json!({"args":{"path":"src/module_0.rs"}}),
                        );
                        NativeWorkflowEntry {
                            patch_files: if slot % 12 == 0 {
                                patch.clone()
                            } else {
                                Vec::new()
                            },
                            ..base
                        }
                    })
                    .collect();
                turn
            })
            .collect()
    }

    /// A reloaded turn keeps its statistics: the projection reads the façade's
    /// resolved fields, and the footer it builds must match the one the live
    /// stream builds from the raw object.
    #[test]
    fn a_reloaded_turn_keeps_its_statistics_footer() {
        let mut turn = fixture("fixture-stats", "Fixture answer", "completed");
        turn.assistant.user_round = 57;
        turn.assistant.stats =
            wunder_desktop::native::NativeMessageStats::from_payload(&serde_json::json!({
                "interaction_duration_s": 12.44, "avg_model_round_speed_rounds": 2,
                "avg_model_round_speed_tps": 68234, "request_consumed_tokens": 4096,
                "toolCalls": 3, "contextTokens": 12345, "model_request_count": 4
            }));

        let rows = project_history(&[turn]);
        let metrics: Vec<(String, String)> = rows
            .iter()
            .find(|row| row.kind == crate::timeline::KIND_FOOTER)
            .map(|row| {
                row.stats
                    .iter()
                    .map(|metric| (metric.key.to_string(), metric.value.to_string()))
                    .collect()
            })
            .expect("a turn footer carries the metrics");
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
        );
    }

    /// A turn the runtime reported no statistics for keeps an empty metrics model
    /// rather than a row of placeholders. Its copy action remains available.
    #[test]
    fn a_reloaded_turn_without_statistics_has_no_placeholder_metrics() {
        let rows = project_history(&[fixture("fixture-plain", "Fixture answer", "completed")]);
        let count = rows
            .iter()
            .find(|row| row.kind == crate::timeline::KIND_FOOTER)
            .map(|row| row.stats.row_count())
            .expect("a footer row");
        assert_eq!(count, 0, "no metrics renders no row");
    }

    #[test]
    fn process_only_body_does_not_take_the_final_reply_footer() {
        let mut turn = fixture("fixture-process-only", "", "running");
        turn.process_steps = 1;
        turn.rounds[0].text = "Fixture intermediate note".into();
        turn.rounds[0].is_answer = false;
        let rows = project_history(&[turn]);
        let intermediate = rows
            .iter()
            .find(|row| row.text == "Fixture intermediate note")
            .unwrap();
        assert_ne!(intermediate.process_idx, crate::timeline::NO_PROCESS);
        assert_eq!(intermediate.stats.row_count(), 0);
        let footer = rows.last().unwrap();
        assert!(footer.text.is_empty());
        assert_eq!(footer.process_idx, crate::timeline::NO_PROCESS);
        assert_eq!(footer.stats.row_data(0).unwrap().key, "status-running");
    }

    /// A reloaded turn rebuilds the entries the live stream showed: per model
    /// round one batch holding its thinking and its tools, then that round's
    /// answer block. Collapsing the turn to its newest answer alone is what made
    /// the timeline look emptied once a reply finished.
    #[test]
    fn a_reloaded_turn_keeps_every_rounds_entries() {
        let mut turn = fixture("fixture-rounds", "结论", "任务完成");
        turn.rounds = vec![
            NativeChatRound {
                item_id: "fixture-rounds:text-1".into(),
                round: 1,
                execution: "fixture-rounds".into(),
                is_answer: false,
                reasoning: "先想想第一步".into(),
                text: "第一步说明".into(),
                items: vec![NativeWorkflowEntry::from_payload(
                    "fixture-round-tool",
                    "read_file",
                    "读取文件 · 完成\nsrc/module_0.rs\nFixture preview".into(),
                    "completed",
                    &serde_json::json!({"args":{"path":"src/module_0.rs"}}),
                )],
            },
            NativeChatRound {
                item_id: "fixture-rounds:text-2".into(),
                round: 2,
                execution: "fixture-rounds".into(),
                is_answer: true,
                reasoning: "再想想第二步".into(),
                text: "结论".into(),
                items: Vec::new(),
            },
        ];
        let rows = project_history(&[turn]);
        let kinds: Vec<i32> = rows.iter().map(|row| row.kind).collect();
        assert_eq!(
            kinds,
            vec![
                crate::timeline::KIND_USER,
                crate::timeline::KIND_PROCESS,
                crate::timeline::KIND_GROUP,
                crate::timeline::KIND_REASON,
                crate::timeline::KIND_BODY,
                crate::timeline::KIND_TOOL,
                crate::timeline::KIND_GROUP,
                crate::timeline::KIND_REASON,
                crate::timeline::KIND_BODY,
                crate::timeline::KIND_FOOTER,
            ],
        );
        // The bubble carries the turn identity the live tail is matched on.
        assert_eq!(rows[0].id, "user-turn-fixture-rounds");
        // Each round folds as its own batch, and thinking keeps its full text.
        assert_eq!(rows[2].group_idx, rows[3].group_idx);
        assert_eq!(rows[2].group_idx, rows[5].group_idx);
        assert_ne!(rows[2].group_idx, rows[6].group_idx);
        assert_eq!(rows[3].detail, "先想想第一步");
        assert_eq!(
            rows[3].tool_name.as_str(),
            crate::timeline_text::thought_done()
        );
        assert_eq!(rows[4].text, "第一步说明");
        assert_eq!(rows[8].text, "结论");
        // A fold handle always addresses its own row, or toggling would move a
        // different entry. A body block has no handle: for it `foldable` means
        // "frozen", which is what shows its metrics and actions line.
        for row in rows.iter().filter(|row| {
            matches!(
                row.kind,
                crate::timeline::KIND_GROUP
                    | crate::timeline::KIND_REASON
                    | crate::timeline::KIND_TOOL
                    | crate::timeline::KIND_PROCESS
            )
        }) {
            assert_eq!(
                rows[row.payload as usize].id, row.id,
                "a foldable row must point at itself"
            );
        }
        // The turn's process bar counts every step before the newest answer:
        // two thinking segments, one intermediate note and one tool call. The
        // batches inside it are stamped open — unfolding the process is the
        // request to see them — while the newest answer stays outside the fold.
        let process = rows[1].process_idx;
        assert_ne!(process, crate::timeline::NO_PROCESS);
        assert_eq!(rows[1].text, crate::timeline_text::process_label(4));
        assert_eq!(rows[1].summary, "已完成");
        assert!(rows[1].visible, "a real process earns its bar");
        assert!(!rows[1].open && !rows[1].process_open, "it ships folded");
        for row in &rows[2..8] {
            assert_eq!(row.process_idx, process, "step {row:?} escaped the fold");
            if row.group_idx != crate::timeline::NO_GROUP {
                assert!(row.group_open, "the process is the only fold gate");
            }
        }
        assert_eq!(
            rows[8].process_idx,
            crate::timeline::NO_PROCESS,
            "the newest answer block stays outside the fold"
        );
    }

    /// §12.2 evidence: the bounded worst case is measured, not estimated. Both
    /// phases are bounded by the projections themselves, so this is the whole
    /// cost the UI thread pays before Slint sees a single row.
    #[test]
    fn worst_case_history_projection_and_unfold_stay_bounded() {
        let started = Instant::now();
        let turns = near_limit_turns();
        let built = started.elapsed();

        let started = Instant::now();
        let rows = project_history(&turns);
        let projected = started.elapsed();

        let mut timeline = crate::timeline::Timeline::new();
        let started = Instant::now();
        timeline.set_history(rows.clone());
        let published = started.elapsed();

        // Before the user touches anything, each turn reads as its bubble, one
        // process bar and its newest answer: 50 turns x 3 laid-out rows.
        let entries_per_turn = turns[0].rounds[0].items.len();
        let laid_out = |model: &slint::ModelRc<TimelineRow>| {
            model
                .iter()
                .filter(|row| {
                    row.visible
                        // The two gates `TimelineRowView.visible` applies.
                        && (row.kind == crate::timeline::KIND_GROUP
                            || row.kind == crate::timeline::KIND_PROCESS
                            || row.group_idx == crate::timeline::NO_GROUP
                            || row.group_open)
                        && (row.kind == crate::timeline::KIND_PROCESS
                            || row.process_idx == crate::timeline::NO_PROCESS
                            || row.process_open)
                })
                .count()
        };
        assert_eq!(
            laid_out(&timeline.model()),
            50 * 4,
            "the default column is bubble, process, answer and footer per turn",
        );

        // Reach the fully opened column the way a user does: one handle at a
        // time. Every toggle is a bounded scan, so the whole history opens in
        // bounded clicks.
        let started = Instant::now();
        let mut opened = 0usize;
        for row in rows.iter() {
            if row.kind == crate::timeline::KIND_PROCESS && row.visible && !row.open {
                timeline.toggle(row.payload);
                opened += 1;
            }
        }
        let unfolded = started.elapsed();

        let rows = timeline.model();
        let exposed = laid_out(&rows);
        assert_eq!(opened, 50, "every turn's process ships folded");
        assert_eq!(
            exposed, 1400,
            "each process exposes its tools without an extra batch row",
        );
        let mut kinds = [0usize; 8];
        for row in rows.iter() {
            kinds[row.kind as usize] += 1;
        }
        println!(
            "timeline worst case: turns={} entries/turn={} rows={} exposed={} \
             build={built:?} project={projected:?} publish={published:?} unfold={unfolded:?}",
            turns.len(),
            entries_per_turn,
            rows.row_count(),
            exposed,
        );
        assert_eq!(kinds[0], 50, "one user bubble per turn");
        assert_eq!(kinds[1], 50, "one answer block per turn");
        assert_eq!(kinds[3], 1200, "24 tool entries x 50 turns");
        assert_eq!(kinds[4], 50, "one batch bar per turn");
        assert_eq!(kinds[6], 50, "one process bar per turn");
        assert_eq!(kinds[7], 50, "one footer per turn");
        // Rows are one flat vector: 50 turns x (1 bubble + 1 process bar +
        // 1 batch bar + 24 tool entries + 1 answer + 1 footer) = 1450, with no nesting or
        // duplication.
        assert_eq!(rows.row_count(), 1450);
        // The projection is O(rows); keep a loose ceiling so a future change that
        // makes it quadratic fails here instead of on the user's machine.
        assert!(
            projected.as_millis() < 4_000,
            "worst-case projection took {projected:?}",
        );
        // Patch cards are the most expensive single projection, so measure them
        // on their own instead of inferring their cost from the whole history: at
        // this row count the difference is inside the run-to-run noise.
        let cards = turns
            .iter()
            .flat_map(|turn| turn.rounds.iter().flat_map(|round| round.items.iter()))
            .filter(|entry| !entry.patch_files.is_empty())
            .count();
        let started = Instant::now();
        let mut lines = 0usize;
        for turn in &turns {
            for entry in turn.rounds.iter().flat_map(|round| round.items.iter()) {
                for card in crate::timeline_text::patch_cards(&entry.patch_files).iter() {
                    lines += card.lines.row_count();
                }
            }
        }
        let cards_ms = started.elapsed().as_secs_f64() * 1000.0;
        println!(
            "timeline patch projection: cards={cards} rendered_lines={lines} \
             rebuilt_once={cards_ms:.1}ms",
        );
        // 50 turns x 24 entries x 8 files x 200 lines is the projection ceiling;
        // a full rebuild is bounded and must stay far away from a frame budget.
        assert!(lines > 0 && cards > 0, "the fixture must carry patch cards");
        assert!(
            cards_ms < 2_000.0,
            "rebuilding every patch card took {cards_ms}ms",
        );
    }

    /// The shipped shape of a finished turn: the send path keys the live turn by
    /// session while storage keys it by its root message, so the two never share
    /// an id. Handing the turn over on that difference is what left a second
    /// bubble on screen after the last reply.
    #[test]
    fn a_finished_live_turn_is_published_once() {
        let mut timeline = crate::timeline::Timeline::new();
        timeline.set_history(project_history(&[fixture(
            "fixture-first",
            "Earlier answer",
            "任务完成",
        )]));
        timeline.begin_turn("fixture-session", "Fixture input");
        timeline.start_body(1);
        timeline.append_body("Latest answer").unwrap();
        timeline.flush();
        timeline.finish(false);

        // Storage commits the turn the live builder just settled.
        timeline.publish_history(project_history(&[
            fixture("fixture-first", "Earlier answer", "任务完成"),
            fixture("fixture-session-root", "Latest answer", "任务完成"),
        ]));

        let rows = timeline.model();
        assert_eq!(
            rows.iter()
                .filter(|row| row.kind == crate::timeline::KIND_USER)
                .count(),
            2,
            "one bubble per turn, the finished one not twice"
        );
        assert_eq!(
            rows.iter()
                .filter(|row| row.kind == crate::timeline::KIND_BODY)
                .count(),
            2,
            "one durable answer per turn"
        );
        drop(rows);
        assert_eq!(timeline.last_answer(), "Latest answer");
    }

    /// §12.2 evidence: the frame flush rewrites only the active tail block. Row
    /// handles of the frozen history must be the very same model instances
    /// before and after streaming, so no frame can rebuild the column.
    #[test]
    fn streaming_a_frame_rewrites_only_the_active_tail_block() {
        let rows = project_history(&near_limit_turns());
        let mut timeline = crate::timeline::Timeline::new();
        timeline.set_history(rows);

        let frozen = timeline.model();
        let before: Vec<(i32, ModelRc<crate::TextBlock>, ModelRc<crate::PatchCard>)> = frozen
            .iter()
            .map(|row| (row.kind, row.blocks.clone(), row.patch.clone()))
            .collect();

        timeline.begin_turn("fixture-live", "Fixture input");
        timeline.start_body(1);
        // Header cost of one live turn: user bubble + process bar + first body
        // block. Captured rather than guessed, so a reducer change that adds a
        // row shows up as a deliberate update here instead of a silent drift.
        let after_header = timeline.model().row_count();
        assert_eq!(
            after_header,
            before.len() + 3,
            "a live turn header is bubble + process bar + body block",
        );
        let mut frames = 0usize;
        let started = Instant::now();
        for index in 0..40 {
            timeline
                .append_body(&format!("第 {index} 帧增量\n"))
                .unwrap();
            timeline.flush();
            frames += 1;
        }
        let streamed = started.elapsed();

        let after = timeline.model();
        // Streaming must not add rows at all: every frame rewrites the body
        // block it already owns, so row count is frozen at the header cost.
        assert_eq!(
            after.row_count(),
            after_header,
            "streaming a frame appended rows instead of rewriting the tail block",
        );
        for (index, (kind, blocks, patch)) in before.iter().enumerate() {
            let row = after.row_data(index).expect("frozen row kept");
            assert_eq!(row.kind, *kind);
            assert_eq!(
                &row.blocks, blocks,
                "row {index} was re-projected by a stream frame",
            );
            assert_eq!(
                &row.patch, patch,
                "row {index} rebuilt its patch card on a stream frame",
            );
        }
        // 40 frames with no new text must do no work at all: only the frames
        // that carried a delta re-project the tail block.
        let started = Instant::now();
        let tail = after.row_data(after.row_count() - 1).unwrap();
        for _ in 0..1_000 {
            timeline.flush();
        }
        let idle = started.elapsed();
        let settled = timeline.model().row_data(after.row_count() - 1).unwrap();
        assert_eq!(
            settled.blocks, tail.blocks,
            "an idle frame re-projected the tail block",
        );
        println!(
            "timeline streaming: {frames} frames with deltas in {streamed:?}, \
             1000 idle flushes in {idle:?}, tail text {} bytes",
            settled.text.len(),
        );
    }

    #[test]
    fn history_keeps_every_turn_laid_out_without_a_process_to_fold() {
        // Both turns are plain text: bubble + answer, so their process bars
        // publish invisible rather than growing a shell around nothing.
        let rows = project_history(&[
            fixture("fixture-old", "Earlier answer", "任务完成"),
            fixture("fixture-new", "Latest answer", "任务完成"),
        ]);
        assert_eq!(rows.len(), 8);
        assert_eq!(rows[0].kind, crate::timeline::KIND_USER);
        assert_eq!(rows[0].id, "user-turn-fixture-old");
        assert_eq!(rows[1].kind, crate::timeline::KIND_PROCESS);
        assert!(!rows[1].visible, "a turn with no process shows no bar");
        assert_eq!(rows[1].text, crate::timeline_text::process_label(0));
        assert_eq!(rows[2].kind, crate::timeline::KIND_BODY);
        assert_eq!(rows[2].process_idx, crate::timeline::NO_PROCESS);
        assert_eq!(rows[4].kind, crate::timeline::KIND_USER);
        assert_eq!(rows[4].id, "user-turn-fixture-new");
        assert_eq!(rows[5].kind, crate::timeline::KIND_PROCESS);
        assert!(!rows[5].visible);
        assert_eq!(rows[6].kind, crate::timeline::KIND_BODY);
        assert_eq!(rows[6].text, "Latest answer");
        for row in rows
            .iter()
            .filter(|row| row.kind != crate::timeline::KIND_PROCESS)
        {
            assert!(row.visible, "no content row is hidden");
        }
        // Body content has no footer metrics.
        assert_eq!(rows[2].stats.row_count(), 0, "no metrics renders no footer");
    }

    #[test]
    fn history_keeps_the_compaction_entry_of_a_failed_turn() {
        let mut turn = fixture("fixture-compact", "", "执行失败");
        turn.rounds[0].items.push(NativeWorkflowEntry::from_payload(
            "fixture-compact-item",
            "上下文压缩",
            String::new(),
            "failed",
            &serde_json::json!({"status":"failed", "error_message":"fixture failure"}),
        ));
        let rows = project_history(&[turn]);
        // A failed turn without an answer still uses the same process handle.
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].kind, crate::timeline::KIND_USER);
        assert_eq!(rows[1].kind, crate::timeline::KIND_PROCESS);
        assert!(rows[1].visible);
        assert_eq!(rows[2].kind, crate::timeline::KIND_GROUP);
        assert_eq!(rows[2].text, "执行工具 1 次");
        assert!(!rows[2].visible, "ownership batch stays internal");
        assert_eq!(rows[3].kind, crate::timeline::KIND_TOOL);
        assert_eq!(rows[3].status_kind, crate::timeline::STATUS_FAILED);
        assert_eq!(rows[3].process_idx, rows[1].process_idx);
        assert!(rows[3].detail.contains("fixture failure"));
        assert_eq!(rows[4].stats.row_data(0).unwrap().key, "status-error");
    }
}
