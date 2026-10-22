//! Passive, bounded observation of every user turn in the selected thread.
use super::{chat_turns, NativeChatTurn, NativeDesktop};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::{collections::BTreeSet, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use wunder_server::{blocking, state::AppState, watch_thread_changes, ThreadChangeFrame};

#[derive(Debug)]
pub enum NativeThreadUpdate {
    Reset(Vec<NativeChatTurn>),
    Turns(Vec<NativeChatTurn>),
    /// A durable queue item changed. Queue rows are projected separately from
    /// chat history, so the UI can refresh the bounded queue strip immediately.
    QueueChanged,
    Context(super::NativeContextUsage),
    Goal(super::NativeGoal),
    Failed(String),
}

pub struct NativeThreadWatch {
    receiver: mpsc::Receiver<NativeThreadUpdate>,
    cancel: CancellationToken,
}

/// A cloneable, receive-free handle for releasing a native thread observer
/// from a UI event loop while its receiver blocks on a worker thread.
#[derive(Clone)]
pub struct NativeThreadWatchCancel(CancellationToken);

impl NativeThreadWatchCancel {
    pub fn cancel(&self) {
        self.0.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
}

impl Drop for NativeThreadWatch {
    fn drop(&mut self) {
        // Unsubscribing must never interrupt a channel or background execution.
        self.cancel.cancel();
    }
}

impl NativeThreadWatch {
    pub fn try_recv(&mut self) -> Option<NativeThreadUpdate> {
        self.receiver.try_recv().ok()
    }

    /// Block a forwarding worker until a durable update arrives.  The UI can
    /// keep only [NativeThreadWatchCancel], so an idle selected thread causes
    /// no frontend timer or repeated channel probe.
    pub fn blocking_recv(&mut self) -> Option<NativeThreadUpdate> {
        self.receiver.blocking_recv()
    }

    pub fn cancel_handle(&self) -> NativeThreadWatchCancel {
        NativeThreadWatchCancel(self.cancel.clone())
    }

    /// Await the next durable update. Selection is event-driven so an idle
    /// thread observer does not keep the UI worker awake.
    pub async fn recv(&mut self) -> Option<NativeThreadUpdate> {
        self.receiver.recv().await
    }
}

impl NativeDesktop {
    pub fn watch_chat(&self, session: &str) -> NativeThreadWatch {
        // The legacy general-purpose watcher can be called without a prior
        // selection snapshot (for example by the egui shell), so preserve its
        // ownership guard. The Slint selection path uses the overload below
        // after `open_chat_snapshot` has already done this work on its worker.
        if let Err(error) = self.open_session(session) {
            let (output, receiver) = mpsc::channel(8);
            let _ = output.try_send(NativeThreadUpdate::Failed(error.to_string()));
            return NativeThreadWatch {
                receiver,
                cancel: CancellationToken::new(),
            };
        }
        self.watch_chat_with_initial(session, None)
    }

    /// Start watching a thread which the caller has already opened with
    /// `open_chat_snapshot` or `open_session`. A selection can pass the turns
    /// it already loaded so a slow disk is read only once; this method does
    /// not issue a duplicate lease heartbeat on its own.
    pub fn watch_chat_with_initial(
        &self,
        session: &str,
        initial: Option<Vec<NativeChatTurn>>,
    ) -> NativeThreadWatch {
        self.watch_chat_with_initial_cursor(session, initial, None)
    }

    /// Start an already-opened thread watcher with an optional cursor captured
    /// before its initial snapshot. Keeping this cursor with the snapshot
    /// closes the HDD race where a write lands between two independent reads.
    pub fn watch_chat_with_initial_cursor(
        &self,
        session: &str,
        initial: Option<Vec<NativeChatTurn>>,
        initial_cursor: Option<i64>,
    ) -> NativeThreadWatch {
        let (output, receiver) = mpsc::channel(8);
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        let state = self.state().clone();
        let user = self.user_id().to_owned();
        let session = session.to_owned();
        self.runtime.spawn(async move {
            tokio::select! {
                _ = token.cancelled() => {},
                result = async {
                    observe(state, user, session, initial, initial_cursor, &output, &token).await
                } => {
                    if let Err(error) = result {
                        tracing::warn!(%error, "native thread observation failed");
                        let _ = output.send(NativeThreadUpdate::Failed("线程更新失败，请重新选择线程".into())).await;
                    }
                }
            }
        });
        NativeThreadWatch { receiver, cancel }
    }
}

async fn observe(
    state: Arc<AppState>,
    user: String,
    session: String,
    mut initial: Option<Vec<NativeChatTurn>>,
    mut initial_cursor: Option<i64>,
    output: &mpsc::Sender<NativeThreadUpdate>,
    cancel: &CancellationToken,
) -> Result<()> {
    loop {
        let mut control_events = state.kernel.orchestrator.subscribe_thread_updates(&session);
        let db = state.clone();
        let owner = user.clone();
        let target = session.clone();
        let initial_turns = initial.take();
        let snapshot_cursor = initial_cursor.take();
        let (cursor, turns) = blocking::run_db("native.observe.initial", move || {
            db.user_store
                .get_chat_session(&owner, &target)?
                .ok_or_else(|| anyhow!("chat session not found"))?;
            // A selected-thread snapshot is read after this cursor. Replaying
            // concurrent roots is harmless because identities are stable;
            // starting after them would lose a commit that races a slow disk
            // history read. A cold watcher likewise reads before history.
            let cursor =
                snapshot_cursor.unwrap_or(db.storage.latest_thread_change_seq_by_session(&target)?);
            let turns = match initial_turns {
                Some(turns) => turns,
                None => chat_turns::load_turn_headers(&db, &owner, &target)?,
            };
            Ok((cursor, turns))
        })
        .await?;
        if output.send(NativeThreadUpdate::Reset(turns)).await.is_err() {
            return Ok(());
        }
        // Subscribe before the durable read so an update in between is queued
        // on the watch receiver instead of being lost until another change.
        let mut context_events = state.kernel.orchestrator.subscribe_context_usage(&session);
        let mut context = super::context::snapshot(&state, &user, &session).await?;
        if output
            .send(NativeThreadUpdate::Context(context))
            .await
            .is_err()
        {
            return Ok(());
        }
        // Subscribe before reading the durable snapshot so a mutation between
        // the read and the event loop cannot be missed.
        let mut goal_events = state.kernel.orchestrator.subscribe_goal_updates(&session);
        let goal = goal_snapshot(&state, &user, &session).await?;
        if output
            .send(NativeThreadUpdate::Goal(goal.clone()))
            .await
            .is_err()
        {
            return Ok(());
        }
        let feed_cancel = cancel.child_token();
        let _feed_guard = feed_cancel.clone().drop_guard();
        let mut feed =
            watch_thread_changes(state.clone(), session.clone(), cursor, Some(feed_cancel)).await?;
        let mut reset = false;
        // Context and goal snapshots are pushed by their owning services. Do
        // not retain periodic SQLite safety reads here: an idle selected
        // thread must stay silent.
        loop {
            let mut refresh_headers = false;
            let first = tokio::select! {
                _ = cancel.cancelled() => return Ok(()),
                _ = output.closed() => return Ok(()),
                frame = feed.recv() => match frame { Some(frame) => Some(frame), None => break },
                changed = control_events.changed() => {
                    if changed.is_err() { return Ok(()); }
                    control_events.borrow_and_update();
                    refresh_headers = true;
                    None
                }
                changed = context_events.changed() => {
                    if changed.is_ok() {
                        let payload = context_events.borrow_and_update().clone();
                        let next = super::NativeContextUsage::from_value(&payload);
                        if next.used.is_some() || next.capacity.is_some() {
                            if next.used.is_some() { context.used = next.used; }
                            if next.capacity.is_some() { context.capacity = next.capacity; }
                            if output.send(NativeThreadUpdate::Context(context)).await.is_err() {
                                return Ok(());
                            }
                        }
                    } else {
                        context_events = state
                            .kernel
                            .orchestrator
                            .subscribe_context_usage(&session);
                    }
                    continue;
                }
                changed = goal_events.changed() => {
                    if changed.is_ok() {
                        let goal = goal_from_update(&goal_events.borrow_and_update());
                        if output.send(NativeThreadUpdate::Goal(goal)).await.is_err() {
                            return Ok(());
                        }
                    } else {
                        goal_events = state
                            .kernel
                            .orchestrator
                            .subscribe_goal_updates(&session);
                    }
                    continue;
                }
            };
            let mut executions = BTreeSet::new();
            let mut queue_changed = refresh_headers;
            if let Some(first) = first {
                queue_changed |= collect_frame(first, &mut executions, &mut reset);
            }
            // Queue rows are projected separately from chat history. Wake the
            // composer as soon as the first durable queue frame arrives; the
            // history burst can still be coalesced independently below.
            if queue_changed && output.send(NativeThreadUpdate::QueueChanged).await.is_err() {
                return Ok(());
            }
            let queue_notified = queue_changed;
            // Merge durable commits from a burst, with bounded feed and output.
            tokio::select! {
                _ = cancel.cancelled() => return Ok(()),
                _ = output.closed() => return Ok(()),
                _ = tokio::time::sleep(Duration::from_millis(16)) => {},
            }
            if control_events.has_changed().unwrap_or(false) {
                control_events.borrow_and_update();
                refresh_headers = true;
                queue_changed = true;
            }
            for _ in 0..255 {
                match feed.try_recv() {
                    Ok(frame) => queue_changed |= collect_frame(frame, &mut executions, &mut reset),
                    Err(_) => break,
                }
            }
            if reset {
                break;
            }
            // Queue rows are projected separately from chat history. Publish
            // this control update before the history read so an interject or
            // terminal queue event does not wait for the polling backstop.
            if queue_changed
                && !queue_notified
                && output.send(NativeThreadUpdate::QueueChanged).await.is_err()
            {
                return Ok(());
            }
            let db = state.clone();
            let owner = user.clone();
            let target = session.clone();
            let (turns, used) = blocking::run_db("native.observe.turns", move || {
                if refresh_headers {
                    let turns = chat_turns::load_turn_headers(&db, &owner, &target)?;
                    let used = db
                        .workspace
                        .load_session_context_tokens(&owner, &target)
                        .max(0) as f64;
                    return Ok((turns, used));
                }
                let mut roots = BTreeSet::new();
                let mut turns = Vec::new();
                for execution in executions {
                    if let Some(detail) = db
                        .storage
                        .get_thread_turn(&owner, &target, &execution, -1, 1, false)?
                    {
                        roots.insert(
                            detail["root_turn_id"]
                                .as_str()
                                .unwrap_or(&execution)
                                .to_owned(),
                        );
                    }
                }
                // Preserve user-round ordering even when several channels write
                // during the same frame. No historical message reconstruction.
                let mut records = Vec::new();
                for root in roots {
                    if let Some(detail) = db
                        .storage
                        .get_thread_turn(&owner, &target, &root, -1, 1, false)?
                    {
                        records.push(detail);
                    }
                }
                records.sort_by_key(|record| record["user_turn_index"].as_i64().unwrap_or(0));
                for root in records {
                    if let Some(turn) = chat_turns::load_turn(&db, &owner, &target, &root)? {
                        turns.push(turn);
                    }
                }
                let used = db
                    .workspace
                    .load_session_context_tokens(&owner, &target)
                    .max(0) as f64;
                Ok((turns, used))
            })
            .await?;
            if !turns.is_empty() {
                if output.send(NativeThreadUpdate::Turns(turns)).await.is_err() {
                    return Ok(());
                }
                // Reuse the capacity and emit only changed occupancy. Token commits
                // must not repeat model resolution or enqueue identical UI updates.
                if context.used != Some(used) {
                    context.used = Some(used);
                    if output
                        .send(NativeThreadUpdate::Context(context))
                        .await
                        .is_err()
                    {
                        return Ok(());
                    }
                }
            }
        }
        if cancel.is_cancelled() || output.is_closed() {
            return Ok(());
        }
        if !reset {
            return Err(anyhow!("native durable subscription closed"));
        }
    }
}

async fn goal_snapshot(
    state: &Arc<AppState>,
    user: &str,
    session: &str,
) -> Result<super::NativeGoal> {
    let view = state
        .kernel
        .orchestrator
        .goal_handle()
        .get_view(&state.storage, user, session)
        .await
        .ok()
        .flatten();
    Ok(view
        .map(|view| super::NativeGoal {
            objective: view.record.objective,
            phase: view.record.phase,
            rounds_started: view.record.rounds_started,
            max_goal_rounds: view.record.max_goal_rounds,
            activation: view.activation.as_str().to_string(),
        })
        .unwrap_or_default())
}

/// A goal publisher always sends the complete stable projection, or null for
/// a successful clear. It is intentionally decoded without a storage read so
/// a selected idle thread stays event-driven.
fn goal_from_update(payload: &Value) -> super::NativeGoal {
    if payload.is_null() {
        return super::NativeGoal::default();
    }
    super::NativeGoal {
        objective: payload["objective"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        phase: payload["phase"].as_str().unwrap_or_default().to_string(),
        rounds_started: payload["rounds_started"].as_i64().unwrap_or_default(),
        max_goal_rounds: payload["max_goal_rounds"].as_i64().unwrap_or_default(),
        activation: payload["activation"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    }
}

/// Durable queue lifecycle events are stored as `kind: "queue"` item upserts.
/// Depending on the feeder projection the item may be wrapped under
/// `payload`; inspect this small bounded envelope without querying SQLite for
/// every history frame.
fn is_queue_change(data: &Value) -> bool {
    let mut current = data;
    for _ in 0..3 {
        if current.get("kind").and_then(Value::as_str) == Some("queue")
            || current
                .get("event_type")
                .and_then(Value::as_str)
                .is_some_and(|event| event.starts_with("queue_"))
        {
            return true;
        }
        let Some(next) = current.get("payload") else {
            break;
        };
        current = next;
    }
    false
}

fn collect_frame(
    frame: ThreadChangeFrame,
    executions: &mut BTreeSet<String>,
    reset: &mut bool,
) -> bool {
    match frame {
        ThreadChangeFrame::Change { data, .. } => {
            let queue = is_queue_change(&data);
            if let Some(id) = data["turn_id"].as_str().filter(|id| !id.is_empty()) {
                executions.insert(id.to_owned());
            }
            queue
        }
        _ => {
            *reset = true;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wunder_server::{config::Config, config_store::ConfigStore, state::AppStateInitOptions};

    async fn isolated_state() -> (tempfile::TempDir, Arc<AppState>) {
        let temp = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.storage.backend = "sqlite".into();
        config.storage.data_root = temp.path().join("data").to_string_lossy().into_owned();
        config.workspace.root = temp.path().join("workspace").to_string_lossy().into_owned();
        config.skills.enabled.clear();
        let store = ConfigStore::new(temp.path().join("fixture.yaml"));
        store.update(|value| *value = config.clone()).await.unwrap();
        let state = Arc::new(
            AppState::new_with_options(
                store,
                config,
                AppStateInitOptions::cli_default().with_start_thread_runtime(false),
            )
            .unwrap(),
        );
        (temp, state)
    }

    fn create_session(state: &AppState, user: &str, session: &str) {
        state
            .user_store
            .upsert_chat_session(&wunder_server::storage::ChatSessionRecord {
                session_id: session.into(),
                user_id: user.into(),
                title: "Fixture".into(),
                status: "active".into(),
                created_at: 0.0,
                updated_at: 0.0,
                last_message_at: 0.0,
                agent_id: None,
                workspace_id: None,
                tool_overrides: vec![],
                parent_session_id: None,
                parent_message_id: None,
                spawn_label: None,
                spawned_by: None,
            })
            .unwrap();
    }

    async fn update(receiver: &mut mpsc::Receiver<NativeThreadUpdate>) -> NativeThreadUpdate {
        tokio::time::timeout(Duration::from_secs(4), receiver.recv())
            .await
            .expect("thread observer did not publish an update")
            .expect("thread observer closed before the expected update")
    }

    async fn next(receiver: &mut mpsc::Receiver<NativeThreadUpdate>) -> Vec<NativeChatTurn> {
        loop {
            match tokio::time::timeout(Duration::from_secs(4), receiver.recv())
                .await
                .unwrap()
                .unwrap()
            {
                NativeThreadUpdate::Reset(turns) | NativeThreadUpdate::Turns(turns) => {
                    return turns
                }
                NativeThreadUpdate::QueueChanged
                | NativeThreadUpdate::Context(_)
                | NativeThreadUpdate::Goal(_) => {}
                NativeThreadUpdate::Failed(error) => panic!("{error}"),
            }
        }
    }

    async fn initialize_thread(state: &AppState, session: &str) {
        use wunder_server::stable_core::thread_store::*;
        let thread = ThreadId::new(session).unwrap();
        let rollout = RolloutId::new(format!("{session}-rollout")).unwrap();
        state
            .thread_store
            .create_thread(CreateThreadRequest {
                metadata: ThreadMetadata {
                    thread_id: thread.clone(),
                    current_rollout_id: rollout.clone(),
                    status: ThreadStatus::Active,
                    created_at_ms: 1,
                    updated_at_ms: 1,
                    title: None,
                    workspace_id: None,
                    pinned: false,
                },
                session_meta: SessionMeta {
                    thread_id: thread,
                    rollout_id: rollout,
                    created_at_ms: 1,
                    source: "fixture".into(),
                    initial_system_prompt: "sample prompt".into(),
                    initial_memory: None,
                    parent_thread_id: None,
                    history_base: None,
                    capabilities: Value::Null,
                },
            })
            .await
            .unwrap();
    }

    fn notify_committed(state: &AppState, session: &str) {
        let cursor = state
            .storage
            .latest_thread_change_seq_by_session(session)
            .unwrap();
        state
            .kernel
            .orchestrator
            .notify_thread_committed(session, cursor);
    }

    #[tokio::test]
    async fn channel_turns_arrive_without_local_send_and_survive_resubscription() {
        use wunder_server::stable_core::thread_store::*;
        let (_temp, state) = isolated_state().await;
        let user = "fixture-owner";
        let session = "fixture-channel-thread";
        create_session(&state, user, session);
        let (output, mut receiver) = mpsc::channel(8);
        let cancel = CancellationToken::new();
        let db = state.clone();
        let token = cancel.clone();
        let task = tokio::spawn(async move {
            observe(db, user.into(), session.into(), None, None, &output, &token).await
        });
        assert!(next(&mut receiver).await.is_empty());
        let mut ids = Vec::new();
        for input in ["sample input", "sample command", "sample attachment"] {
            let accepted = state
                .storage
                .accept_thread_turn(user, session, &json!({"content":input}))
                .unwrap();
            let id = accepted["turn_id"].as_str().unwrap().to_owned();
            if ids.is_empty() {
                initialize_thread(&state, session).await;
            }
            ids.push(id.clone());
            state
                .thread_store
                .begin_turn(
                    user,
                    &ThreadId::new(session).unwrap(),
                    &TurnId::new(&id).unwrap(),
                    "fixture-model",
                )
                .await
                .unwrap();
            notify_committed(&state, session);
            loop {
                let rows = next(&mut receiver).await;
                if let Some(row) = rows.iter().find(|row| row.root_id == id) {
                    assert_eq!(row.user.text, input);
                    break;
                }
            }
            let answer = format!("sample result {}", ids.len());
            state
                .storage
                .commit_thread_item(
                    user,
                    &json!({
                        "session_id":session,"turn_id":id,"item_id":format!("{id}:text-1"),
                        "kind":"assistant_message","role":"assistant","model_round":1,
                        "content":answer,"status":"completed",
                    }),
                )
                .unwrap();
            state
                .storage
                .update_thread_turn(user, session, &id, "completed", "", &json!({}))
                .unwrap();
            notify_committed(&state, session);
            loop {
                let rows = next(&mut receiver).await;
                if let Some(row) = rows
                    .iter()
                    .find(|row| row.root_id == id && row.assistant.stats_status == "任务完成")
                {
                    assert_eq!(row.assistant.text, answer);
                    break;
                }
            }
            assert!(!task.is_finished());
        }
        let accepted = state
            .storage
            .accept_thread_turn(user, session, &json!({"content":"sample pending"}))
            .unwrap();
        let id = accepted["turn_id"].as_str().unwrap();
        state
            .thread_store
            .begin_turn(
                user,
                &ThreadId::new(session).unwrap(),
                &TurnId::new(id).unwrap(),
                "fixture-model",
            )
            .await
            .unwrap();
        state
            .storage
            .commit_thread_item(
                user,
                &json!({
                    "session_id":session,"turn_id":id,"item_id":format!("{id}:text-1"),
                    "kind":"assistant_message","role":"assistant","model_round":1,
                    "content":"sample retained output","status":"completed",
                }),
            )
            .unwrap();
        notify_committed(&state, session);
        loop {
            if next(&mut receiver)
                .await
                .iter()
                .any(|row| row.root_id == id && row.assistant.text == "sample retained output")
            {
                break;
            }
        }
        // Detaching only releases observation; the canonical execution remains.
        drop(NativeThreadWatch { receiver, cancel });
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            state
                .storage
                .get_thread_turn(user, session, id, -1, 10, false)
                .unwrap()
                .unwrap()["status"],
            "running"
        );
        state
            .storage
            .update_thread_turn(
                user,
                session,
                id,
                "cancelled",
                "",
                &json!({"status":"cancelled"}),
            )
            .unwrap();
        let (output, mut receiver) = mpsc::channel(8);
        let db = state.clone();
        let task = tokio::spawn(async move {
            observe(
                db,
                user.into(),
                session.into(),
                None,
                None,
                &output,
                &CancellationToken::new(),
            )
            .await
        });
        let restored = next(&mut receiver).await;
        assert_eq!(restored.len(), 4);
        assert_eq!(restored[3].assistant.text, "sample retained output");
        assert_eq!(restored[3].assistant.stats_status, "已停止");
        task.abort();
        let (output, _receiver) = mpsc::channel(8);
        assert!(observe(
            state,
            "fixture-other".into(),
            session.into(),
            None,
            None,
            &output,
            &CancellationToken::new()
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn queue_control_and_completed_item_both_notify_idle_observers() {
        use wunder_server::stable_core::thread_store::*;
        let (_temp, state) = isolated_state().await;
        let user = "fixture-owner";
        let session = "fixture-queue-thread";
        create_session(&state, user, session);
        let accepted = state
            .storage
            .accept_thread_turn(user, session, &json!({"content":"sample input"}))
            .unwrap();
        let turn = accepted["turn_id"].as_str().unwrap();
        initialize_thread(&state, session).await;
        state
            .thread_store
            .begin_turn(
                user,
                &ThreadId::new(session).unwrap(),
                &TurnId::new(turn).unwrap(),
                "fixture-model",
            )
            .await
            .unwrap();
        let cursor = state
            .storage
            .latest_thread_change_seq_by_session(session)
            .unwrap();
        let (output, mut receiver) = mpsc::channel(8);
        let cancel = CancellationToken::new();
        let db = state.clone();
        let token = cancel.clone();
        let task = tokio::spawn(async move {
            observe(db, user.into(), session.into(), None, None, &output, &token).await
        });
        assert_eq!(next(&mut receiver).await.len(), 1);
        loop {
            if matches!(update(&mut receiver).await, NativeThreadUpdate::Goal(_)) {
                break;
            }
        }
        // A queue control edge has no canonical item and keeps the cursor.
        notify_committed(&state, session);
        loop {
            if matches!(
                update(&mut receiver).await,
                NativeThreadUpdate::QueueChanged
            ) {
                break;
            }
        }
        assert_eq!(
            state
                .storage
                .latest_thread_change_seq_by_session(session)
                .unwrap(),
            cursor
        );
        state.storage.commit_thread_item(user,&json!({
            "session_id":session,"turn_id":turn,"item_id":format!("{turn}:queue-fixture"),
            "kind":"queue","event_type":"queue_finish","status":"completed","visibility":"user",
        })).unwrap();
        notify_committed(&state, session);
        loop {
            if matches!(
                update(&mut receiver).await,
                NativeThreadUpdate::QueueChanged
            ) {
                break;
            }
        }
        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn goal_service_updates_reach_an_idle_native_observer_without_a_durable_frame() {
        let (_temp, state) = isolated_state().await;
        let user = "fixture-owner";
        let session = "fixture-goal-thread";
        create_session(&state, user, session);
        let (output, mut receiver) = mpsc::channel(8);
        let cancel = CancellationToken::new();
        let db = state.clone();
        let token = cancel.clone();
        let task = tokio::spawn(async move {
            tokio::select! {
                _ = token.cancelled() => Ok(()),
                result = observe(db, user.into(), session.into(), None, None, &output, &token) => result,
            }
        });

        assert!(next(&mut receiver).await.is_empty());
        assert!(matches!(
            update(&mut receiver).await,
            NativeThreadUpdate::Context(_)
        ));
        assert!(matches!(
            update(&mut receiver).await,
            NativeThreadUpdate::Goal(goal) if goal == super::super::NativeGoal::default()
        ));

        let service = state.kernel.orchestrator.goal_handle();
        service
            .create(
                state.storage.clone(),
                user,
                session,
                "Fixture objective",
                Some(8),
            )
            .await
            .unwrap();
        loop {
            match update(&mut receiver).await {
                NativeThreadUpdate::Goal(goal)
                    if goal.objective == "Fixture objective"
                        && goal.phase == "active"
                        && goal.activation == "armed" =>
                {
                    break;
                }
                NativeThreadUpdate::Failed(error) => panic!("{error}"),
                _ => {}
            }
        }

        service.disarm(&state.storage, user, session).await.unwrap();
        loop {
            match update(&mut receiver).await {
                NativeThreadUpdate::Goal(goal) if goal.activation == "disarmed" => break,
                NativeThreadUpdate::Failed(error) => panic!("{error}"),
                _ => {}
            }
        }

        service
            .clear(state.storage.clone(), user, session)
            .await
            .unwrap();
        loop {
            match update(&mut receiver).await {
                NativeThreadUpdate::Goal(goal) if goal == super::super::NativeGoal::default() => {
                    break;
                }
                NativeThreadUpdate::Failed(error) => panic!("{error}"),
                _ => {}
            }
        }

        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    #[test]
    fn queue_change_classification_accepts_bounded_durable_envelopes() {
        for data in [
            json!({"kind": "queue"}),
            json!({"event_type": "queue_start"}),
            json!({"payload": {"kind": "queue"}}),
            json!({"payload": {"payload": {"event_type": "queue_finish"}}}),
        ] {
            assert!(
                is_queue_change(&data),
                "queue frame was not classified: {data}"
            );
        }
        for data in [
            Value::Null,
            json!({"kind": "assistant_message"}),
            json!({"event_type": "question_panel"}),
            json!({"payload": {"payload": {"payload": {"kind": "queue"}}}}),
        ] {
            assert!(
                !is_queue_change(&data),
                "unrelated frame classified as queue: {data}"
            );
        }
    }

    #[test]
    fn durable_frames_collect_roots_and_control_frames_request_reset() {
        let mut executions = BTreeSet::new();
        let mut reset = false;
        assert!(collect_frame(
            ThreadChangeFrame::Change {
                seq: 1,
                event: "thread_item_upsert".into(),
                data: json!({"turn_id": "fixture-turn", "payload": {"kind": "queue"}}),
            },
            &mut executions,
            &mut reset,
        ));
        assert_eq!(executions, BTreeSet::from(["fixture-turn".to_owned()]));
        assert!(!reset);
        assert!(!collect_frame(
            ThreadChangeFrame::Change {
                seq: 2,
                event: "thread_turn_update".into(),
                data: json!({"turn_id": "fixture-turn", "status": "completed"}),
            },
            &mut executions,
            &mut reset,
        ));
        assert_eq!(
            executions.len(),
            1,
            "root identities are deduplicated per burst"
        );
        assert!(!reset);
        for frame in [
            ThreadChangeFrame::SnapshotRequired { data: json!({}) },
            ThreadChangeFrame::Overflow {
                cursor: 2,
                resume_recommended: true,
            },
        ] {
            reset = false;
            assert!(!collect_frame(frame, &mut executions, &mut reset));
            assert!(
                reset,
                "control frames must trigger durable snapshot recovery"
            );
        }
    }
}
