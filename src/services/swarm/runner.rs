use crate::config_store::ConfigStore;
use crate::i18n;
use crate::monitor::MonitorState;
use crate::orchestrator::Orchestrator;
use crate::orchestrator::OrchestratorError;
use crate::schemas::WunderRequest;
use crate::storage::{
    ChatSessionRecord, SessionRunRecord, TeamRunRecord, TeamTaskRecord, UserAgentRecord,
    DEFAULT_HIVE_ID,
};
use crate::user_store::UserStore;
use crate::workspace::WorkspaceManager;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Instant;
use tokio::sync::{mpsc, Mutex};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{sleep, timeout, Duration};
use tracing::warn;
use uuid::Uuid;

use super::events::{
    TEAM_ERROR, TEAM_FINISH, TEAM_MERGE, TEAM_START, TEAM_TASK_RESULT, TEAM_TASK_UPDATE,
};

const RUNNER_CHANNEL_CAPACITY: usize = 128;
const RUNNER_POLL_INTERVAL_MS: u64 = 600;
const RUNNER_SCAN_BATCH: i64 = 256;
const TEAM_TASK_SESSION_TITLE: &str = "蜂群任务";
const TEAM_TASK_RESULT_MAX_CHARS: usize = 1500;
const TEAM_RUN_SUMMARY_MAX_CHARS: usize = 3000;
const TEAM_QUESTION_MAX_CHARS: usize = 4000;
const TEAM_HISTORY_LOOKBACK: i64 = 80;
const TEAM_TASK_READY_WAIT_MS: u64 = 1_500;
const TEAM_TASK_READY_POLL_MS: u64 = 80;

const TEAM_RUN_STATUS_QUEUED: &str = "queued";
const TEAM_RUN_STATUS_RUNNING: &str = "running";
const TEAM_RUN_STATUS_MERGING: &str = "merging";
const TEAM_RUN_STATUS_SUCCESS: &str = "success";
const TEAM_RUN_STATUS_FAILED: &str = "failed";
const TEAM_RUN_STATUS_TIMEOUT: &str = "timeout";
const TEAM_RUN_STATUS_CANCELLED: &str = "cancelled";

const TEAM_TASK_STATUS_RUNNING: &str = "running";
const TEAM_TASK_STATUS_SUCCESS: &str = "success";
const TEAM_TASK_STATUS_FAILED: &str = "failed";
const TEAM_TASK_STATUS_TIMEOUT: &str = "timeout";
const TEAM_TASK_STATUS_CANCELLED: &str = "cancelled";

struct ActiveRunControl {
    cancel: Arc<AtomicBool>,
    sessions: Arc<Mutex<HashSet<String>>>,
    handle: JoinHandle<()>,
}

#[derive(Clone)]
pub struct TeamRunRunner {
    config_store: ConfigStore,
    user_store: Arc<UserStore>,
    workspace: Arc<WorkspaceManager>,
    monitor: Arc<MonitorState>,
    orchestrator: Arc<Orchestrator>,
    queue_tx: mpsc::Sender<String>,
    queue_rx: Arc<Mutex<Option<mpsc::Receiver<String>>>>,
    active_runs: Arc<Mutex<HashMap<String, ActiveRunControl>>>,
}

impl TeamRunRunner {
    pub fn new(
        config_store: ConfigStore,
        user_store: Arc<UserStore>,
        workspace: Arc<WorkspaceManager>,
        monitor: Arc<MonitorState>,
        orchestrator: Arc<Orchestrator>,
    ) -> Arc<Self> {
        let (queue_tx, queue_rx) = mpsc::channel(RUNNER_CHANNEL_CAPACITY);
        Arc::new(Self {
            config_store,
            user_store,
            workspace,
            monitor,
            orchestrator,
            queue_tx,
            queue_rx: Arc::new(Mutex::new(Some(queue_rx))),
            active_runs: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn start(self: Arc<Self>) {
        let runner = self.clone();
        tokio::spawn(async move {
            runner.run_loop().await;
        });
    }

    pub async fn enqueue(&self, team_run_id: &str) {
        let cleaned = team_run_id.trim();
        if cleaned.is_empty() {
            let _ = self.queue_tx.try_send(String::new());
            return;
        }
        let _ = self.queue_tx.try_send(cleaned.to_string());
    }

    pub async fn wake(&self) {
        let _ = self.queue_tx.try_send(String::new());
    }

    pub async fn cancel(&self, team_run_id: &str) {
        let cleaned = team_run_id.trim();
        if cleaned.is_empty() {
            return;
        }

        let controls = {
            let active = self.active_runs.lock().await;
            active
                .get(cleaned)
                .map(|control| (control.cancel.clone(), control.sessions.clone()))
        };
        let Some((cancel_flag, sessions)) = controls else {
            return;
        };

        cancel_flag.store(true, Ordering::Relaxed);
        let session_ids = {
            let guard = sessions.lock().await;
            guard.iter().cloned().collect::<Vec<_>>()
        };
        for session_id in session_ids {
            let _ = self.monitor.cancel(&session_id);
        }
        let _ = self.queue_tx.try_send(cleaned.to_string());
    }

    async fn run_loop(self: Arc<Self>) {
        let mut rx = {
            let mut guard = self.queue_rx.lock().await;
            guard.take()
        };
        let Some(mut rx) = rx.take() else {
            warn!("team run runner loop skipped: receiver already taken");
            return;
        };

        loop {
            tokio::select! {
                _ = rx.recv() => {}
                _ = sleep(Duration::from_millis(RUNNER_POLL_INTERVAL_MS)) => {}
            }
            self.cleanup_finished_workers().await;
            if let Err(err) = self.dispatch_runs().await {
                warn!("team run dispatch failed: {err}");
            }
        }
    }

    async fn cleanup_finished_workers(&self) {
        let mut removed = Vec::new();
        {
            let mut active = self.active_runs.lock().await;
            let finished_ids = active
                .iter()
                .filter(|(_, control)| control.handle.is_finished())
                .map(|(team_run_id, _)| team_run_id.to_string())
                .collect::<Vec<_>>();
            for team_run_id in finished_ids {
                if let Some(control) = active.remove(&team_run_id) {
                    removed.push((team_run_id, control.handle));
                }
            }
        }

        for (team_run_id, handle) in removed {
            if let Err(err) = handle.await {
                warn!("team run worker join failed for {team_run_id}: {err}");
            }
        }
    }

    async fn dispatch_runs(self: &Arc<Self>) -> Result<()> {
        let config = self.config_store.get().await;
        let max_active = config.tools.swarm.max_active_team_runs.max(1);
        let runs = self.user_store.list_team_runs_by_status(
            &[
                TEAM_RUN_STATUS_QUEUED,
                TEAM_RUN_STATUS_RUNNING,
                TEAM_RUN_STATUS_MERGING,
            ],
            0,
            RUNNER_SCAN_BATCH,
        )?;
        if runs.is_empty() {
            return Ok(());
        }

        let mut to_spawn = Vec::new();
        {
            let active = self.active_runs.lock().await;
            let mut available_slots = max_active.saturating_sub(active.len());
            for run in runs {
                if active.contains_key(&run.team_run_id) {
                    continue;
                }
                let should_resume = matches!(
                    normalize_status(&run.status).as_str(),
                    TEAM_RUN_STATUS_RUNNING | TEAM_RUN_STATUS_MERGING
                );
                if !should_resume && available_slots == 0 {
                    continue;
                }
                if !should_resume {
                    available_slots = available_slots.saturating_sub(1);
                }
                to_spawn.push(run.team_run_id);
            }
        }

        if to_spawn.is_empty() {
            return Ok(());
        }

        let mut active = self.active_runs.lock().await;
        for team_run_id in to_spawn {
            if active.contains_key(&team_run_id) {
                continue;
            }
            let cancel = Arc::new(AtomicBool::new(false));
            let sessions = Arc::new(Mutex::new(HashSet::new()));
            let runner = self.clone();
            let run_id_for_task = team_run_id.clone();
            let cancel_for_task = cancel.clone();
            let sessions_for_task = sessions.clone();
            let handle = tokio::spawn(async move {
                if let Err(err) = runner
                    .execute_team_run(run_id_for_task.clone(), cancel_for_task, sessions_for_task)
                    .await
                {
                    warn!("team run worker failed for {run_id_for_task}: {err}");
                }
            });
            active.insert(
                team_run_id,
                ActiveRunControl {
                    cancel,
                    sessions,
                    handle,
                },
            );
        }
        Ok(())
    }

    async fn execute_team_run(
        self: Arc<Self>,
        team_run_id: String,
        cancel_flag: Arc<AtomicBool>,
        active_sessions: Arc<Mutex<HashSet<String>>>,
    ) -> Result<()> {
        let run = match self.user_store.get_team_run(&team_run_id)? {
            Some(record) => record,
            None => return Ok(()),
        };
        if !is_active_run_status(&run.status) {
            return Ok(());
        }

        let swarm_config = self.config_store.get().await.tools.swarm;
        let options = TeamRunOptions::from_record(&run, swarm_config.default_timeout_s as f64);
        if normalize_status(&run.status) == TEAM_RUN_STATUS_QUEUED {
            let mut updated = run.clone();
            let now = now_ts();
            updated.status = TEAM_RUN_STATUS_RUNNING.to_string();
            if updated.started_time.is_none() {
                updated.started_time = Some(now);
            }
            updated.updated_time = now;
            self.user_store.upsert_team_run(&updated)?;
            self.emit_team_event(
                &updated.parent_session_id,
                TEAM_START,
                json!({
                    "team_run_id": updated.team_run_id,
                    "hive_id": DEFAULT_HIVE_ID,
                    "status": updated.status,
                    "task_total": updated.task_total,
                    "strategy": updated.strategy,
                }),
            );
        }

        let question = self.resolve_parent_question(&run);
        let mut tasks = self.load_ready_tasks(&run).await?;
        tasks.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.updated_time.total_cmp(&b.updated_time))
        });
        let mut pending = tasks
            .into_iter()
            .filter(|task| !is_terminal_task_status(&task.status))
            .collect::<VecDeque<_>>();

        let max_parallel = swarm_config.max_parallel_tasks_per_team.max(1);
        let mut join_set = JoinSet::new();
        let mut runner_error_count = 0usize;
        while !pending.is_empty() || !join_set.is_empty() {
            let cancelled =
                cancel_flag.load(Ordering::Relaxed) || self.is_run_cancelled(&run.team_run_id)?;
            if cancelled {
                self.cancel_pending_tasks(&run, &mut pending)?;
            }

            while !cancelled && join_set.len() < max_parallel && !pending.is_empty() {
                let Some(task) = pending.pop_front() else {
                    break;
                };
                let task_id = task.task_id.clone();
                let runner = self.clone();
                let run_snapshot = run.clone();
                let question_snapshot = question.clone();
                let cancel_for_task = cancel_flag.clone();
                let sessions_for_task = active_sessions.clone();
                let timeout_s = options.timeout_s;
                let max_retry = swarm_config.max_retry;
                join_set.spawn(async move {
                    let result = runner
                        .execute_team_task(
                            run_snapshot,
                            task,
                            question_snapshot,
                            timeout_s,
                            max_retry,
                            cancel_for_task,
                            sessions_for_task,
                        )
                        .await;
                    (task_id, result)
                });
            }

            if join_set.is_empty() {
                if pending.is_empty() {
                    break;
                }
                sleep(Duration::from_millis(120)).await;
                continue;
            }

            if let Some(result) = join_set.join_next().await {
                match result {
                    Ok((_, Ok(()))) => {}
                    Ok((task_id, Err(err))) => {
                        runner_error_count += 1;
                        warn!(
                            "team task execution failed in run {}: {err}",
                            run.team_run_id
                        );
                        if let Err(mark_err) = self.mark_task_failed_from_runner_error(
                            &run,
                            &task_id,
                            &err.to_string(),
                        ) {
                            warn!(
                                "team task failure mark failed in run {} task {}: {mark_err}",
                                run.team_run_id, task_id
                            );
                        }
                    }
                    Err(err) => {
                        runner_error_count += 1;
                        warn!("team task join failed in run {}: {err}", run.team_run_id);
                    }
                }
                self.refresh_team_run_progress(&run.team_run_id, false)?;
            }
        }

        let runner_error = if runner_error_count == 0 {
            None
        } else {
            Some(format!("runner_error_count={runner_error_count}"))
        };
        self.finalize_team_run(
            &run.team_run_id,
            &options,
            cancel_flag.load(Ordering::Relaxed),
            runner_error.as_deref(),
        )?;
        Ok(())
    }

    async fn load_ready_tasks(&self, run: &TeamRunRecord) -> Result<Vec<TeamTaskRecord>> {
        let expected_total = run.task_total.max(0) as usize;
        let mut tasks = self.user_store.list_team_tasks(&run.team_run_id)?;
        if expected_total == 0 || tasks.len() >= expected_total {
            return Ok(tasks);
        }

        let deadline = Instant::now() + Duration::from_millis(TEAM_TASK_READY_WAIT_MS);
        while tasks.len() < expected_total && Instant::now() < deadline {
            sleep(Duration::from_millis(TEAM_TASK_READY_POLL_MS)).await;
            tasks = self.user_store.list_team_tasks(&run.team_run_id)?;
        }

        if tasks.len() < expected_total {
            warn!(
                "team run {} tasks not fully ready before execution, expected={}, loaded={}",
                run.team_run_id,
                expected_total,
                tasks.len()
            );
        }
        Ok(tasks)
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_team_task(
        self: Arc<Self>,
        run: TeamRunRecord,
        mut task: TeamTaskRecord,
        question: String,
        timeout_s: f64,
        max_retry: u32,
        cancel_flag: Arc<AtomicBool>,
        active_sessions: Arc<Mutex<HashSet<String>>>,
    ) -> Result<()> {
        if cancel_flag.load(Ordering::Relaxed) || self.is_run_cancelled(&run.team_run_id)? {
            self.mark_task_cancelled(&run, &mut task)?;
            return Ok(());
        }

        let now = now_ts();
        task.status = TEAM_TASK_STATUS_RUNNING.to_string();
        task.started_time = Some(now);
        task.updated_time = now;
        self.user_store.upsert_team_task(&task)?;
        self.emit_team_event(
            &run.parent_session_id,
            TEAM_TASK_UPDATE,
            json!({
                "team_run_id": task.team_run_id,
                "task_id": task.task_id,
                "hive_id": DEFAULT_HIVE_ID,
                "agent_id": task.agent_id,
                "status": task.status,
            }),
        );

        let agent = self
            .user_store
            .get_user_agent(&run.user_id, &task.agent_id)?
            .ok_or_else(|| anyhow::anyhow!("agent {} not found", task.agent_id))?;
        let (session_id, created_session) = self.resolve_task_session(&run, &task, &agent)?;
        if task.spawned_session_id.as_deref() != Some(session_id.as_str()) {
            task.spawned_session_id = Some(session_id.clone());
            task.updated_time = now_ts();
            self.user_store.upsert_team_task(&task)?;
        }

        let mut retry_count = task.retry_count.max(0) as u32;
        let (final_status, final_answer, final_error, final_elapsed_s) = loop {
            if cancel_flag.load(Ordering::Relaxed) || self.is_run_cancelled(&run.team_run_id)? {
                let _ = self.monitor.cancel(&session_id);
                break (
                    TEAM_TASK_STATUS_CANCELLED.to_string(),
                    None,
                    Some("cancelled".to_string()),
                    0.0,
                );
            }

            let run_id = format!("run_{}", Uuid::new_v4().simple());
            let request = build_task_request(&run, &agent, &session_id, question.clone());
            let outcome = self
                .execute_session_run(
                    &run,
                    &session_id,
                    &run_id,
                    request,
                    timeout_s,
                    active_sessions.clone(),
                )
                .await;

            match outcome {
                SessionExecutionOutcome::Success { answer, elapsed_s } => {
                    break (
                        TEAM_TASK_STATUS_SUCCESS.to_string(),
                        Some(answer),
                        None,
                        elapsed_s,
                    )
                }
                SessionExecutionOutcome::Cancelled { elapsed_s } => {
                    break (
                        TEAM_TASK_STATUS_CANCELLED.to_string(),
                        None,
                        Some("cancelled".to_string()),
                        elapsed_s,
                    )
                }
                SessionExecutionOutcome::Timeout { elapsed_s } => {
                    if retry_count < max_retry {
                        retry_count += 1;
                        task.retry_count = retry_count as i64;
                        task.updated_time = now_ts();
                        self.user_store.upsert_team_task(&task)?;
                        continue;
                    }
                    break (
                        TEAM_TASK_STATUS_TIMEOUT.to_string(),
                        None,
                        Some("timeout".to_string()),
                        elapsed_s,
                    );
                }
                SessionExecutionOutcome::Error { message, elapsed_s } => {
                    if retry_count < max_retry {
                        retry_count += 1;
                        task.retry_count = retry_count as i64;
                        task.updated_time = now_ts();
                        self.user_store.upsert_team_task(&task)?;
                        continue;
                    }
                    break (
                        TEAM_TASK_STATUS_FAILED.to_string(),
                        None,
                        Some(message),
                        elapsed_s,
                    );
                }
            }
        };

        let finished = now_ts();
        task.retry_count = retry_count as i64;
        task.status = final_status;
        task.finished_time = Some(finished);
        task.elapsed_s = Some(
            task.started_time
                .map(|started| (finished - started).max(0.0))
                .unwrap_or(final_elapsed_s),
        );
        task.updated_time = finished;
        task.result_summary = final_answer
            .as_deref()
            .map(|value| truncate_text(value, TEAM_TASK_RESULT_MAX_CHARS));
        task.error = final_error.clone();
        self.user_store.upsert_team_task(&task)?;

        self.emit_team_event(
            &run.parent_session_id,
            TEAM_TASK_RESULT,
            json!({
                "team_run_id": task.team_run_id,
                "task_id": task.task_id,
                "hive_id": DEFAULT_HIVE_ID,
                "agent_id": task.agent_id,
                "session_id": session_id,
                "created_session": created_session,
                "status": task.status,
                "retry_count": task.retry_count,
                "elapsed_s": task.elapsed_s,
                "result_summary": task.result_summary,
                "error": task.error,
            }),
        );
        Ok(())
    }

    async fn execute_session_run(
        &self,
        run: &TeamRunRecord,
        session_id: &str,
        run_id: &str,
        request: WunderRequest,
        timeout_s: f64,
        active_sessions: Arc<Mutex<HashSet<String>>>,
    ) -> SessionExecutionOutcome {
        let queued = now_ts();
        let record = SessionRunRecord {
            run_id: run_id.to_string(),
            session_id: session_id.to_string(),
            parent_session_id: Some(run.parent_session_id.clone()),
            user_id: run.user_id.clone(),
            agent_id: request.agent_id.clone(),
            model_name: request.model_name.clone(),
            status: TEAM_RUN_STATUS_QUEUED.to_string(),
            queued_time: queued,
            started_time: 0.0,
            finished_time: 0.0,
            elapsed_s: 0.0,
            result: None,
            error: None,
            updated_time: queued,
        };
        let _ = self.user_store.upsert_session_run(&record);

        let started = now_ts();
        let running = SessionRunRecord {
            status: TEAM_RUN_STATUS_RUNNING.to_string(),
            started_time: started,
            updated_time: started,
            ..record.clone()
        };
        let _ = self.user_store.upsert_session_run(&running);

        {
            let mut guard = active_sessions.lock().await;
            guard.insert(session_id.to_string());
        }

        let run_result = if timeout_s > 0.0 {
            match timeout(
                Duration::from_secs_f64(timeout_s),
                self.orchestrator.run(request),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => {
                    let _ = self.monitor.cancel(session_id);
                    Err(anyhow::anyhow!("timeout"))
                }
            }
        } else {
            self.orchestrator.run(request).await
        };

        {
            let mut guard = active_sessions.lock().await;
            guard.remove(session_id);
        }

        let finished = now_ts();
        let elapsed_s = (finished - started).max(0.0);
        let (status, answer, error, outcome) = match run_result {
            Ok(response) => {
                let answer = truncate_text(&response.answer, TEAM_TASK_RESULT_MAX_CHARS);
                (
                    TEAM_RUN_STATUS_SUCCESS.to_string(),
                    Some(answer.clone()),
                    None,
                    SessionExecutionOutcome::Success { answer, elapsed_s },
                )
            }
            Err(err) => {
                let cancelled = err
                    .downcast_ref::<OrchestratorError>()
                    .map(|inner| inner.code() == "CANCELLED")
                    .unwrap_or(false);
                if cancelled {
                    (
                        TEAM_TASK_STATUS_CANCELLED.to_string(),
                        None,
                        Some("cancelled".to_string()),
                        SessionExecutionOutcome::Cancelled { elapsed_s },
                    )
                } else if err.to_string().to_ascii_lowercase().contains("timeout") {
                    (
                        TEAM_TASK_STATUS_TIMEOUT.to_string(),
                        None,
                        Some("timeout".to_string()),
                        SessionExecutionOutcome::Timeout { elapsed_s },
                    )
                } else {
                    let message = err.to_string();
                    (
                        TEAM_TASK_STATUS_FAILED.to_string(),
                        None,
                        Some(message.clone()),
                        SessionExecutionOutcome::Error { message, elapsed_s },
                    )
                }
            }
        };

        let finished_record = SessionRunRecord {
            status,
            finished_time: finished,
            elapsed_s,
            result: answer,
            error,
            updated_time: finished,
            ..running
        };
        let _ = self.user_store.upsert_session_run(&finished_record);
        outcome
    }

    fn resolve_task_session(
        &self,
        run: &TeamRunRecord,
        task: &TeamTaskRecord,
        agent: &UserAgentRecord,
    ) -> Result<(String, bool)> {
        let now = now_ts();
        if let Some(target) = task
            .target_session_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let record = self
                .user_store
                .get_chat_session(&run.user_id, target)?
                .ok_or_else(|| anyhow::anyhow!("target session {target} not found"))?;
            if let Some(record_agent_id) = record.agent_id.as_deref() {
                if record_agent_id.trim() != task.agent_id.trim() {
                    return Err(anyhow::anyhow!(
                        "target session {target} belongs to agent {}",
                        record_agent_id
                    ));
                }
            }
            return Ok((record.session_id, false));
        }

        let session_id = format!("sess_{}", Uuid::new_v4().simple());
        let title = format!("{TEAM_TASK_SESSION_TITLE}-{}", agent.name.trim());
        let record = ChatSessionRecord {
            session_id: session_id.clone(),
            user_id: run.user_id.clone(),
            title,
            created_at: now,
            updated_at: now,
            last_message_at: now,
            agent_id: Some(task.agent_id.clone()),
            tool_overrides: agent.tool_names.clone(),
            parent_session_id: Some(run.parent_session_id.clone()),
            parent_message_id: None,
            spawn_label: Some(format!("team:{}", run.team_run_id)),
            spawned_by: Some("team_runner".to_string()),
        };
        self.user_store.upsert_chat_session(&record)?;
        Ok((session_id, true))
    }

    fn mark_task_cancelled(&self, run: &TeamRunRecord, task: &mut TeamTaskRecord) -> Result<()> {
        let now = now_ts();
        task.status = TEAM_TASK_STATUS_CANCELLED.to_string();
        task.finished_time = Some(now);
        let started = task.started_time.unwrap_or(now);
        task.elapsed_s = Some((now - started).max(0.0));
        task.updated_time = now;
        task.error = Some("cancelled".to_string());
        self.user_store.upsert_team_task(task)?;
        self.emit_team_event(
            &run.parent_session_id,
            TEAM_TASK_UPDATE,
            json!({
                "team_run_id": task.team_run_id,
                "task_id": task.task_id,
                "hive_id": DEFAULT_HIVE_ID,
                "agent_id": task.agent_id,
                "status": task.status,
            }),
        );
        Ok(())
    }

    fn mark_task_failed_from_runner_error(
        &self,
        run: &TeamRunRecord,
        task_id: &str,
        message: &str,
    ) -> Result<()> {
        let Some(mut task) = self.user_store.get_team_task(task_id)? else {
            return Ok(());
        };
        if task.team_run_id.trim() != run.team_run_id.trim() {
            return Ok(());
        }
        if is_terminal_task_status(&task.status) {
            return Ok(());
        }
        let now = now_ts();
        task.status = TEAM_TASK_STATUS_FAILED.to_string();
        if task.started_time.is_none() {
            task.started_time = Some(now);
        }
        task.finished_time = Some(now);
        let started = task.started_time.unwrap_or(now);
        task.elapsed_s = Some((now - started).max(0.0));
        task.updated_time = now;
        let cleaned = message.trim();
        if cleaned.is_empty() {
            task.error = Some("runner_error".to_string());
        } else {
            task.error = Some(truncate_text(cleaned, TEAM_TASK_RESULT_MAX_CHARS));
        }
        self.user_store.upsert_team_task(&task)?;
        self.emit_team_event(
            &run.parent_session_id,
            TEAM_TASK_UPDATE,
            json!({
                "team_run_id": task.team_run_id,
                "task_id": task.task_id,
                "hive_id": DEFAULT_HIVE_ID,
                "agent_id": task.agent_id,
                "status": task.status,
            }),
        );
        Ok(())
    }

    fn cancel_pending_tasks(
        &self,
        run: &TeamRunRecord,
        pending: &mut VecDeque<TeamTaskRecord>,
    ) -> Result<()> {
        while let Some(mut task) = pending.pop_front() {
            self.mark_task_cancelled(run, &mut task)?;
        }
        Ok(())
    }

    fn reconcile_unfinished_tasks(
        &self,
        run: &TeamRunRecord,
        cancelled: bool,
        runner_error: Option<&str>,
    ) -> Result<()> {
        let tasks = self.user_store.list_team_tasks(&run.team_run_id)?;
        let fallback = if cancelled {
            "cancelled"
        } else {
            "runner_incomplete"
        };
        let error = runner_error
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(fallback);

        for mut task in tasks {
            if is_terminal_task_status(&task.status) {
                continue;
            }
            let now = now_ts();
            task.status = if cancelled {
                TEAM_TASK_STATUS_CANCELLED.to_string()
            } else {
                TEAM_TASK_STATUS_FAILED.to_string()
            };
            let started = task.started_time.unwrap_or(now);
            task.started_time = Some(started);
            task.finished_time = Some(now);
            task.elapsed_s = Some((now - started).max(0.0));
            task.updated_time = now;
            task.error = Some(truncate_text(error, TEAM_TASK_RESULT_MAX_CHARS));
            self.user_store.upsert_team_task(&task)?;
            self.emit_team_event(
                &run.parent_session_id,
                TEAM_TASK_UPDATE,
                json!({
                    "team_run_id": task.team_run_id,
                    "task_id": task.task_id,
                    "hive_id": DEFAULT_HIVE_ID,
                    "agent_id": task.agent_id,
                    "status": task.status,
                }),
            );
        }
        Ok(())
    }

    fn refresh_team_run_progress(&self, team_run_id: &str, include_metrics: bool) -> Result<()> {
        let Some(mut run) = self.user_store.get_team_run(team_run_id)? else {
            return Ok(());
        };
        let tasks = self.user_store.list_team_tasks(team_run_id)?;
        let progress = TeamProgress::from_tasks(&tasks);
        run.task_total = tasks.len() as i64;
        run.task_success = progress.success;
        run.task_failed = progress.failed;

        if include_metrics {
            let metrics = self.collect_team_metrics(&tasks);
            run.context_tokens_total = metrics.context_tokens_total;
            run.context_tokens_peak = metrics.context_tokens_peak;
            run.model_round_total = metrics.model_round_total;
        }

        let now = now_ts();
        let status = normalize_status(&run.status);
        if progress.done >= run.task_total.max(0)
            && matches!(
                status.as_str(),
                TEAM_RUN_STATUS_QUEUED | TEAM_RUN_STATUS_RUNNING
            )
        {
            run.status = TEAM_RUN_STATUS_MERGING.to_string();
        }
        run.updated_time = now;
        self.user_store.upsert_team_run(&run)?;
        Ok(())
    }

    fn finalize_team_run(
        &self,
        team_run_id: &str,
        options: &TeamRunOptions,
        cancelled_flag: bool,
        runner_error: Option<&str>,
    ) -> Result<()> {
        let Some(snapshot) = self.user_store.get_team_run(team_run_id)? else {
            return Ok(());
        };
        let cancelled =
            cancelled_flag || normalize_status(&snapshot.status) == TEAM_RUN_STATUS_CANCELLED;
        self.reconcile_unfinished_tasks(&snapshot, cancelled, runner_error)?;
        self.refresh_team_run_progress(team_run_id, true)?;

        let Some(mut run) = self.user_store.get_team_run(team_run_id)? else {
            return Ok(());
        };
        let tasks = self.user_store.list_team_tasks(team_run_id)?;
        let progress = TeamProgress::from_tasks(&tasks);
        let all_done = progress.done >= run.task_total.max(0);
        let merge_summary = build_merge_summary(options.merge_policy.as_str(), &tasks);

        if !merge_summary.is_empty() {
            self.emit_team_event(
                &run.parent_session_id,
                TEAM_MERGE,
                json!({
                    "team_run_id": run.team_run_id,
                    "hive_id": DEFAULT_HIVE_ID,
                    "merge_policy": options.merge_policy,
                    "summary": truncate_text(&merge_summary, TEAM_RUN_SUMMARY_MAX_CHARS),
                }),
            );
        }

        let now = now_ts();
        run.status = if cancelled {
            TEAM_RUN_STATUS_CANCELLED.to_string()
        } else if !all_done {
            TEAM_RUN_STATUS_FAILED.to_string()
        } else if progress.timeout > 0 && progress.success <= 0 {
            TEAM_RUN_STATUS_TIMEOUT.to_string()
        } else if progress.failed > 0 {
            TEAM_RUN_STATUS_FAILED.to_string()
        } else {
            TEAM_RUN_STATUS_SUCCESS.to_string()
        };
        run.finished_time = Some(now);
        run.elapsed_s = run.started_time.map(|started| (now - started).max(0.0));
        run.summary = if merge_summary.is_empty() {
            Some(format!(
                "merge_policy={}; timeout_s={:.0}",
                options.merge_policy, options.timeout_s
            ))
        } else {
            Some(truncate_text(&merge_summary, TEAM_RUN_SUMMARY_MAX_CHARS))
        };
        run.error = match normalize_status(&run.status).as_str() {
            TEAM_RUN_STATUS_SUCCESS => None,
            TEAM_RUN_STATUS_CANCELLED => Some("cancelled".to_string()),
            TEAM_RUN_STATUS_TIMEOUT => Some("timeout".to_string()),
            _ => {
                let detail = if !all_done {
                    format!(
                        "incomplete_tasks={}/{}",
                        progress.done,
                        run.task_total.max(0)
                    )
                } else {
                    format!("task_failed={}", run.task_failed.max(0))
                };
                let message = runner_error
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(|value| format!("{value}; {detail}"))
                    .unwrap_or(detail);
                Some(truncate_text(&message, TEAM_TASK_RESULT_MAX_CHARS))
            }
        };
        run.updated_time = now;
        self.user_store.upsert_team_run(&run)?;

        if matches!(
            run.status.as_str(),
            TEAM_RUN_STATUS_FAILED | TEAM_RUN_STATUS_TIMEOUT
        ) {
            self.emit_team_event(
                &run.parent_session_id,
                TEAM_ERROR,
                json!({
                    "team_run_id": run.team_run_id,
                    "hive_id": DEFAULT_HIVE_ID,
                    "status": run.status,
                    "task_success": run.task_success,
                    "task_failed": run.task_failed,
                }),
            );
        }

        self.emit_team_event(
            &run.parent_session_id,
            TEAM_FINISH,
            json!({
                "team_run_id": run.team_run_id,
                "hive_id": DEFAULT_HIVE_ID,
                "status": run.status,
                "task_total": run.task_total,
                "task_success": run.task_success,
                "task_failed": run.task_failed,
                "context_tokens_total": run.context_tokens_total,
                "context_tokens_peak": run.context_tokens_peak,
                "model_round_total": run.model_round_total,
                "elapsed_s": run.elapsed_s,
                "updated_time": run.updated_time,
            }),
        );

        Ok(())
    }

    fn collect_team_metrics(&self, tasks: &[TeamTaskRecord]) -> TeamMetrics {
        let mut total = 0i64;
        let mut peak = 0i64;
        let mut model_round_total = 0i64;

        for task in tasks {
            let Some(session_id) = task
                .spawned_session_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            let Some(record) = self.monitor.get_record(session_id) else {
                continue;
            };
            let context = parse_i64_value(record.get("context_tokens"))
                .max(parse_i64_value(record.get("context_tokens_peak")));
            total += context;
            if context > peak {
                peak = context;
            }
            model_round_total += parse_model_round_total(&record);
        }

        TeamMetrics {
            context_tokens_total: total,
            context_tokens_peak: peak,
            model_round_total,
        }
    }

    fn is_run_cancelled(&self, team_run_id: &str) -> Result<bool> {
        let Some(record) = self.user_store.get_team_run(team_run_id)? else {
            return Ok(true);
        };
        Ok(normalize_status(&record.status) == TEAM_RUN_STATUS_CANCELLED)
    }

    fn resolve_parent_question(&self, run: &TeamRunRecord) -> String {
        if let Ok(history) =
            self.workspace
                .load_history(&run.user_id, &run.parent_session_id, TEAM_HISTORY_LOOKBACK)
        {
            for item in history.iter().rev() {
                let role = item.get("role").and_then(Value::as_str).unwrap_or("");
                if role != "user" {
                    continue;
                }
                if let Some(content) = extract_text_field(item.get("content")) {
                    let text = truncate_text(content.trim(), TEAM_QUESTION_MAX_CHARS);
                    if !text.is_empty() {
                        return text;
                    }
                }
            }
        }

        if let Some(record) = self.monitor.get_record(&run.parent_session_id) {
            if let Some(question) = extract_text_field(record.get("question")) {
                let text = truncate_text(question.trim(), TEAM_QUESTION_MAX_CHARS);
                if !text.is_empty() {
                    return text;
                }
            }
            if let Some(session) = record.get("session") {
                if let Some(question) = extract_text_field(session.get("question")) {
                    let text = truncate_text(question.trim(), TEAM_QUESTION_MAX_CHARS);
                    if !text.is_empty() {
                        return text;
                    }
                }
            }
        }

        i18n::t("monitor.summary.received")
    }

    fn emit_team_event(&self, session_id: &str, event_type: &str, payload: Value) {
        let cleaned = session_id.trim();
        if cleaned.is_empty() {
            return;
        }
        self.monitor.record_event(cleaned, event_type, &payload);
    }
}

fn build_task_request(
    run: &TeamRunRecord,
    agent: &UserAgentRecord,
    session_id: &str,
    question: String,
) -> WunderRequest {
    let tool_names = if agent.tool_names.is_empty() {
        vec!["__no_tools__".to_string()]
    } else {
        agent.tool_names.clone()
    };
    let agent_prompt = {
        let prompt = agent.system_prompt.trim();
        if prompt.is_empty() {
            None
        } else {
            Some(prompt.to_string())
        }
    };

    WunderRequest {
        user_id: run.user_id.clone(),
        question,
        tool_names,
        skip_tool_calls: false,
        stream: false,
        debug_payload: false,
        session_id: Some(session_id.to_string()),
        agent_id: Some(agent.agent_id.clone()),
        model_name: None,
        language: Some(i18n::get_language()),
        config_overrides: None,
        agent_prompt,
        attachments: None,
        allow_queue: true,
        is_admin: false,
        approval_tx: None,
    }
}

fn normalize_status(status: &str) -> String {
    status.trim().to_ascii_lowercase()
}

fn is_active_run_status(status: &str) -> bool {
    matches!(
        normalize_status(status).as_str(),
        TEAM_RUN_STATUS_QUEUED | TEAM_RUN_STATUS_RUNNING | TEAM_RUN_STATUS_MERGING
    )
}

fn is_terminal_task_status(status: &str) -> bool {
    matches!(
        normalize_status(status).as_str(),
        TEAM_TASK_STATUS_SUCCESS
            | TEAM_TASK_STATUS_FAILED
            | TEAM_TASK_STATUS_TIMEOUT
            | TEAM_TASK_STATUS_CANCELLED
    )
}

fn parse_i64_value(value: Option<&Value>) -> i64 {
    let Some(value) = value else {
        return 0;
    };
    if let Some(number) = value.as_i64() {
        return number;
    }
    if let Some(number) = value.as_u64() {
        return number.min(i64::MAX as u64) as i64;
    }
    value
        .as_f64()
        .map(|number| number as i64)
        .unwrap_or_default()
}

fn parse_model_round_total(record: &Value) -> i64 {
    let mut max_round = 0i64;
    if let Some(events) = record.get("events").and_then(Value::as_array) {
        for event in events {
            let data = event.get("data").unwrap_or(event);
            let round = parse_i64_value(data.get("model_round"))
                .max(parse_i64_value(data.get("round")))
                .max(parse_i64_value(data.get("modelRound")));
            if round > max_round {
                max_round = round;
            }
        }
    }
    max_round.max(parse_i64_value(record.get("model_round")))
}

fn extract_text_field(value: Option<&Value>) -> Option<&str> {
    let value = value?;
    match value {
        Value::String(text) => Some(text),
        _ => None,
    }
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let content = text.trim();
    if content.chars().count() <= max_chars {
        return content.to_string();
    }
    let mut output = content.chars().take(max_chars).collect::<String>();
    output.push_str("...");
    output
}

fn build_merge_summary(merge_policy: &str, tasks: &[TeamTaskRecord]) -> String {
    let mut ordered = tasks.to_vec();
    ordered.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.updated_time.total_cmp(&b.updated_time))
    });

    let policy = merge_policy.trim().to_ascii_lowercase();
    if policy == "first_success" {
        if let Some(task) = ordered
            .iter()
            .find(|task| normalize_status(&task.status) == TEAM_TASK_STATUS_SUCCESS)
        {
            if let Some(summary) = task.result_summary.as_deref() {
                return summary.trim().to_string();
            }
        }
    }

    let mut lines = Vec::new();
    for task in ordered {
        let status = normalize_status(&task.status);
        let summary = task
            .result_summary
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| task.error.clone().unwrap_or_else(|| status.clone()));
        lines.push(format!("[{}][{}] {}", task.agent_id, status, summary));
    }
    lines.join("\n")
}

#[derive(Debug, Clone)]
struct TeamRunOptions {
    timeout_s: f64,
    merge_policy: String,
}

impl TeamRunOptions {
    fn from_record(record: &TeamRunRecord, default_timeout_s: f64) -> Self {
        let mut timeout_s = default_timeout_s.max(1.0);
        let mut merge_policy = "collect".to_string();

        if let Some(summary) = record.summary.as_deref() {
            for segment in summary.split(';') {
                let Some((raw_key, raw_value)) = segment.split_once('=') else {
                    continue;
                };
                let key = raw_key.trim().to_ascii_lowercase();
                let value = raw_value.trim();
                if key == "timeout_s" {
                    if let Ok(parsed) = value.parse::<f64>() {
                        timeout_s = parsed.max(1.0);
                    }
                    continue;
                }
                if key == "merge_policy" && !value.is_empty() {
                    merge_policy = value.to_string();
                }
            }
        }

        Self {
            timeout_s,
            merge_policy,
        }
    }
}

#[derive(Debug, Default)]
struct TeamProgress {
    success: i64,
    failed: i64,
    timeout: i64,
    done: i64,
}

impl TeamProgress {
    fn from_tasks(tasks: &[TeamTaskRecord]) -> Self {
        let mut progress = Self::default();
        for task in tasks {
            match normalize_status(&task.status).as_str() {
                TEAM_TASK_STATUS_SUCCESS => {
                    progress.success += 1;
                    progress.done += 1;
                }
                TEAM_TASK_STATUS_TIMEOUT => {
                    progress.failed += 1;
                    progress.timeout += 1;
                    progress.done += 1;
                }
                TEAM_TASK_STATUS_FAILED | TEAM_TASK_STATUS_CANCELLED => {
                    progress.failed += 1;
                    progress.done += 1;
                }
                _ => {}
            }
        }
        progress
    }
}

#[derive(Debug, Default)]
struct TeamMetrics {
    context_tokens_total: i64,
    context_tokens_peak: i64,
    model_round_total: i64,
}

enum SessionExecutionOutcome {
    Success { answer: String, elapsed_s: f64 },
    Cancelled { elapsed_s: f64 },
    Timeout { elapsed_s: f64 },
    Error { message: String, elapsed_s: f64 },
}

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}
