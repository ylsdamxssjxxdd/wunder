use crate::config_store::ConfigStore;
use crate::core::long_task;
use crate::core::{blocking, runtime_metrics};
use crate::i18n;
use crate::monitor::MonitorState;
use crate::orchestrator::{Orchestrator, OrchestratorError};
use crate::schemas::WunderRequest;
use crate::services::goal;
use crate::services::stream_events::StreamEventService;
use crate::storage::{
    AgentTaskRecord, AgentThreadRecord, ChatSessionRecord, UpdateAgentTaskStatusParams,
};
use crate::user_store::UserStore;
use anyhow::{anyhow, Result};
use chrono::Utc;
use futures::StreamExt;
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{mpsc, Mutex};
use tokio_util::sync::CancellationToken;
use tracing::warn;
use uuid::Uuid;

const DEFAULT_SESSION_TITLE: &str = "新会话";
const THREAD_STATUS_IDLE: &str = "idle";
const THREAD_STATUS_BUSY: &str = "busy";
const THREAD_STATUS_WAITING: &str = "waiting";

const TASK_STATUS_PENDING: &str = "pending";
const TASK_STATUS_RUNNING: &str = "running";
const TASK_STATUS_SUCCESS: &str = "success";
const TASK_STATUS_FAILED: &str = "failed";
const TASK_STATUS_RETRY: &str = "retry";
const TASK_STATUS_CANCELLED: &str = "cancelled";
const TASK_STATUS_DEAD: &str = "dead";

#[derive(Debug, Clone)]
pub struct QueueInfo {
    pub task_id: String,
    pub thread_id: String,
    pub session_id: String,
    pub queue_ahead: usize,
    pub queue_total: usize,
    pub active_ahead: usize,
    pub wait_ahead: usize,
    pub queue_event_id: i64,
    pub queue_after_event_id: i64,
}

#[derive(Debug, Clone, Default)]
pub struct ThreadCancelSettlement {
    pub monitor_cancelled: bool,
    pub queued_tasks_cancelled: usize,
    pub running_tasks_marked_cancelled: usize,
    pub thread_status_reset: bool,
    pub settlement_event_id: i64,
}

#[derive(Debug)]
pub enum GoalContinuationSubmission {
    Started { session_id: String, goal_id: String },
    Queued(QueueInfo),
    Skipped,
}

#[derive(Debug)]
pub enum ThreadSubmitOutcome {
    Run(Box<WunderRequest>, Option<SessionLease>),
    Queued(QueueInfo),
}

#[derive(Debug, Clone)]
pub struct SessionLease {
    session_id: String,
    pending_sessions: Arc<StdMutex<HashSet<String>>>,
    active_runtime_sessions: Arc<StdMutex<HashSet<String>>>,
}

impl SessionLease {
    fn new(
        session_id: String,
        pending_sessions: Arc<StdMutex<HashSet<String>>>,
        active_runtime_sessions: Arc<StdMutex<HashSet<String>>>,
    ) -> Self {
        Self {
            session_id,
            pending_sessions,
            active_runtime_sessions,
        }
    }
}

impl Drop for SessionLease {
    fn drop(&mut self) {
        if self.session_id.trim().is_empty() {
            return;
        }
        if let Ok(mut guard) = self.pending_sessions.lock() {
            guard.remove(self.session_id.as_str());
        }
        if let Ok(mut guard) = self.active_runtime_sessions.lock() {
            guard.remove(self.session_id.as_str());
        }
    }
}

#[derive(Clone)]
pub struct ThreadRuntime {
    config_store: ConfigStore,
    user_store: Arc<UserStore>,
    monitor: Arc<MonitorState>,
    orchestrator: Arc<Orchestrator>,
    stream_events: StreamEventService,
    queue_tx: mpsc::Sender<()>,
    queue_rx: Arc<Mutex<Option<mpsc::Receiver<()>>>>,
    running_threads: Arc<Mutex<HashSet<String>>>,
    pending_sessions: Arc<StdMutex<HashSet<String>>>,
    active_runtime_sessions: Arc<StdMutex<HashSet<String>>>,
    pending_goal_continuations: Arc<StdMutex<std::collections::HashMap<String, CancellationToken>>>,
}

impl ThreadRuntime {
    pub fn new(
        config_store: ConfigStore,
        user_store: Arc<UserStore>,
        monitor: Arc<MonitorState>,
        orchestrator: Arc<Orchestrator>,
    ) -> Arc<Self> {
        let stream_events = StreamEventService::new(user_store.storage_backend());
        let (queue_tx, queue_rx) = mpsc::channel(64);
        Arc::new(Self {
            config_store,
            user_store,
            monitor,
            orchestrator,
            stream_events,
            queue_tx,
            queue_rx: Arc::new(Mutex::new(Some(queue_rx))),
            running_threads: Arc::new(Mutex::new(HashSet::new())),
            pending_sessions: Arc::new(StdMutex::new(HashSet::new())),
            active_runtime_sessions: Arc::new(StdMutex::new(HashSet::new())),
            pending_goal_continuations: Arc::new(StdMutex::new(std::collections::HashMap::new())),
        })
    }

    pub fn queue_waker(&self) -> mpsc::Sender<()> {
        self.queue_tx.clone()
    }

    pub fn start(self: Arc<Self>) {
        let runtime = self.clone();
        long_task::spawn("runtime.thread.queue_loop", async move {
            runtime.run_loop().await;
        });
    }

    pub async fn wake(&self) {
        let _ = self.queue_tx.try_send(());
    }

    pub async fn submit_user_request(
        &self,
        mut request: WunderRequest,
    ) -> Result<ThreadSubmitOutcome> {
        let user_id = request.user_id.trim().to_string();
        if user_id.is_empty() {
            return Err(anyhow!(i18n::t("error.user_id_required")));
        }
        request.client_message_id =
            normalize_client_message_id(request.client_message_id.as_deref());
        request.enforce_runtime_queue = true;
        let agent_id = normalize_agent_id(request.agent_id.as_deref());
        let explicit_session = request
            .session_id
            .as_ref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        let mut session_id = request
            .session_id
            .clone()
            .filter(|value| !value.trim().is_empty());
        let mut set_as_main = !explicit_session;

        if session_id.is_none() {
            let resolved = self
                .resolve_or_create_main_session(&user_id, &agent_id)
                .await?;
            session_id = Some(resolved);
        }

        request.session_id = session_id.clone();
        request.agent_id = if agent_id.is_empty() {
            None
        } else {
            Some(agent_id.clone())
        };

        let mut lease = None;
        let config = self.config_store.get().await;
        if !explicit_session && set_as_main {
            if let Some(current_session_id) = session_id.as_deref() {
                if !self
                    .is_session_available_for_submit(&user_id, current_session_id)
                    .await
                {
                    // For implicit session requests, fork when main is busy so concurrent app calls stay parallel.
                    let forked = self.create_isolated_session(&user_id, &agent_id)?;
                    session_id = Some(forked);
                    request.session_id = session_id.clone();
                    set_as_main = false;
                }
            }
        }

        if let Some(session_id) = session_id.as_ref() {
            if !goal::is_goal_continuation(request.config_overrides.as_ref()) {
                self.cancel_pending_goal_continuation(session_id);
                self.cancel_queued_goal_continuations(session_id)?;
            }
            if set_as_main {
                let _ = self
                    .set_main_session(&user_id, &agent_id, session_id, "user_message")
                    .await;
            } else {
                let _ = self.ensure_session_record(&user_id, session_id, &agent_id)?;
            }
        }

        if config.agent_queue.enabled {
            if let Some(session_id) = session_id.as_deref() {
                if self.should_queue(&user_id, Some(session_id)).await {
                    let info = self
                        .enqueue_task(&request, &agent_id, Some(session_id))
                        .await?;
                    return Ok(ThreadSubmitOutcome::Queued(info));
                }
                lease = self.try_acquire_start_lease(session_id).await;
                if lease.is_none() {
                    let info = self
                        .enqueue_task(&request, &agent_id, Some(session_id))
                        .await?;
                    return Ok(ThreadSubmitOutcome::Queued(info));
                }
            }
        }

        Ok(ThreadSubmitOutcome::Run(Box::new(request), lease))
    }

    pub async fn submit_goal_continuation(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<GoalContinuationSubmission> {
        let session = self
            .user_store
            .get_chat_session(user_id, session_id)?
            .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
        let user = self
            .user_store
            .get_user_by_id(user_id)?
            .ok_or_else(|| anyhow!(i18n::t("error.permission_denied")))?;
        let tool_names = self
            .orchestrator
            .resolve_session_effective_tool_names(&user, &session)
            .await;
        if !goal::tool_names_contain_goal_tool(&tool_names) {
            return Ok(GoalContinuationSubmission::Skipped);
        }
        let Some(mut continuation) = goal::build_continuation_request_from_session(
            self.user_store.storage_backend(),
            user_id,
            &session,
            tool_names,
        )
        .await?
        else {
            return Ok(GoalContinuationSubmission::Skipped);
        };
        let Some(goal_record) = goal::mark_goal_continuation_started(
            self.user_store.storage_backend(),
            user_id,
            &session.session_id,
        )
        .await?
        else {
            return Ok(GoalContinuationSubmission::Skipped);
        };
        continuation.goal = goal_record;
        match self.submit_user_request(continuation.request).await? {
            ThreadSubmitOutcome::Queued(info) => Ok(GoalContinuationSubmission::Queued(info)),
            ThreadSubmitOutcome::Run(request, lease) => {
                let orchestrator = self.orchestrator.clone();
                let runtime = self.clone();
                let user_id = user_id.trim().to_string();
                let session_id = session.session_id.clone();
                long_task::spawn("runtime.thread.goal_continuation.run", async move {
                    let _lease = lease;
                    match orchestrator.stream(*request).await {
                        Ok(stream) => {
                            tokio::pin!(stream);
                            let mut goal_continue_ready = false;
                            while let Some(item) = stream.next().await {
                                match item {
                                    Ok(event) => {
                                        if event.event == "goal_continuation_ready" {
                                            goal_continue_ready = true;
                                        }
                                    }
                                    Err(err) => match err {},
                                }
                                // Drain the background goal continuation stream; clients can resume
                                // persisted stream events through the normal session event channel.
                            }
                            if goal_continue_ready {
                                runtime.spawn_goal_continuation_after_cooldown(user_id, session_id);
                            }
                        }
                        Err(err) => {
                            warn!("goal continuation stream failed: {err}");
                        }
                    }
                });
                Ok(GoalContinuationSubmission::Started {
                    session_id: session.session_id,
                    goal_id: continuation.goal.goal_id,
                })
            }
        }
    }

    pub fn spawn_goal_continuation_after_cooldown(&self, user_id: String, session_id: String) {
        let runtime = self.clone();
        let token = CancellationToken::new();
        {
            let mut guard = match runtime.pending_goal_continuations.lock() {
                Ok(guard) => guard,
                Err(_) => return,
            };
            if let Some(existing) = guard.insert(session_id.clone(), token.clone()) {
                existing.cancel();
            }
        }
        long_task::spawn("runtime.thread.goal_continuation.cooldown", async move {
            let delay =
                match goal::get_goal(runtime.user_store.storage_backend(), &user_id, &session_id)
                    .await
                {
                    Ok(Some(record)) => goal::continuation_delay_seconds(&record, false),
                    _ => None,
                };
            let Some(delay) = delay else {
                runtime.clear_pending_goal_continuation(&session_id, &token);
                return;
            };
            if delay > f64::EPSILON {
                tokio::select! {
                    _ = token.cancelled() => {
                        runtime.clear_pending_goal_continuation(&session_id, &token);
                        return;
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_secs_f64(delay)) => {}
                }
            }
            if token.is_cancelled() {
                runtime.clear_pending_goal_continuation(&session_id, &token);
                return;
            }
            let _ = runtime
                .submit_goal_continuation(&user_id, &session_id)
                .await;
            runtime.clear_pending_goal_continuation(&session_id, &token);
        });
    }

    fn clear_pending_goal_continuation(&self, session_id: &str, token: &CancellationToken) {
        let cleaned = session_id.trim();
        if cleaned.is_empty() {
            return;
        }
        if let Ok(mut guard) = self.pending_goal_continuations.lock() {
            let should_remove = guard
                .get(cleaned)
                .map(|current| current.is_cancelled() || token.is_cancelled())
                .unwrap_or(false);
            if should_remove {
                guard.remove(cleaned);
            }
        }
    }

    fn cancel_pending_goal_continuation(&self, session_id: &str) {
        let cleaned = session_id.trim();
        if cleaned.is_empty() {
            return;
        }
        if let Ok(mut guard) = self.pending_goal_continuations.lock() {
            if let Some(token) = guard.remove(cleaned) {
                token.cancel();
            }
        }
    }

    fn cancel_queued_goal_continuations(&self, session_id: &str) -> Result<()> {
        let cleaned = session_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let thread_id = format!("thread_{cleaned}");
        let tasks = self
            .user_store
            .list_agent_tasks_by_thread(&thread_id, None, 32)?;
        for task in tasks {
            let is_pending = task.status == TASK_STATUS_PENDING || task.status == TASK_STATUS_RETRY;
            if !is_pending {
                continue;
            }
            if goal::is_goal_continuation(task.request_payload.get("config_overrides")) {
                let _ = self.cancel_task(&task.task_id);
            }
        }
        Ok(())
    }

    pub async fn resolve_main_session_id(
        &self,
        user_id: &str,
        agent_id: &str,
    ) -> Result<Option<String>> {
        let record = self.user_store.get_agent_thread(user_id, agent_id)?;
        if let Some(record) = record {
            if !record.session_id.trim().is_empty() {
                return Ok(Some(record.session_id));
            }
        }
        let (sessions, _) =
            self.user_store
                .list_chat_sessions(user_id, Some(agent_id), None, 0, 1)?;
        if let Some(session) = sessions.first() {
            let _ = self
                .set_main_session(user_id, agent_id, &session.session_id, "fallback")
                .await;
            return Ok(Some(session.session_id.clone()));
        }
        Ok(None)
    }

    pub async fn resolve_or_create_main_session_id(
        &self,
        user_id: &str,
        agent_id: &str,
    ) -> Result<String> {
        self.resolve_or_create_main_session(user_id, agent_id).await
    }

    pub async fn create_fresh_main_session_id(
        &self,
        user_id: &str,
        agent_id: &str,
        reason: &str,
    ) -> Result<String> {
        let session_id = self.create_isolated_session(user_id, agent_id)?;
        let _ = self
            .set_main_session(user_id, agent_id, &session_id, reason)
            .await?;
        Ok(session_id)
    }

    pub async fn set_main_session(
        &self,
        user_id: &str,
        agent_id: &str,
        session_id: &str,
        reason: &str,
    ) -> Result<AgentThreadRecord> {
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Err(anyhow!(i18n::t("error.content_required")));
        }
        let cleaned_agent = agent_id.trim();
        let session_record =
            self.ensure_session_record(cleaned_user, cleaned_session, cleaned_agent)?;
        if !cleaned_agent.is_empty() {
            let record_agent = session_record.agent_id.as_deref().unwrap_or("").trim();
            if !record_agent.is_empty() && record_agent != cleaned_agent {
                return Err(anyhow!(i18n::t("error.permission_denied")));
            }
        }

        let existing = self
            .user_store
            .get_agent_thread(cleaned_user, cleaned_agent)?;
        let now = now_ts();
        let thread_id = format!("thread_{cleaned_session}");
        let (created_at, status) = if let Some(record) = existing.as_ref() {
            (record.created_at, record.status.clone())
        } else {
            (now, THREAD_STATUS_IDLE.to_string())
        };
        let next_status = if status.trim().is_empty() {
            THREAD_STATUS_IDLE.to_string()
        } else {
            status
        };
        let record = AgentThreadRecord {
            thread_id,
            user_id: cleaned_user.to_string(),
            agent_id: cleaned_agent.to_string(),
            session_id: cleaned_session.to_string(),
            status: next_status,
            created_at,
            updated_at: now,
        };
        self.user_store.upsert_agent_thread(&record)?;
        self.monitor.record_event(
            cleaned_session,
            "main_thread_changed",
            &json!({
                "session_id": cleaned_session,
                "agent_id": cleaned_agent,
                "user_id": cleaned_user,
                "reason": reason,
            }),
        );
        Ok(record)
    }

    pub async fn set_main_session_by_thread(
        &self,
        user_id: &str,
        agent_id: &str,
        session_id: &str,
        reason: &str,
    ) -> Result<()> {
        let _ = self
            .set_main_session(user_id, agent_id, session_id, reason)
            .await?;
        Ok(())
    }

    pub fn update_thread_status(
        &self,
        user_id: &str,
        agent_id: &str,
        session_id: &str,
        status: &str,
    ) -> Result<()> {
        let existing = self.user_store.get_agent_thread(user_id, agent_id)?;
        let now = now_ts();
        if let Some(record) = existing {
            let cleaned_session = session_id.trim();
            if !cleaned_session.is_empty() && record.session_id.trim() != cleaned_session {
                return Ok(());
            }
            let updated = AgentThreadRecord {
                status: status.to_string(),
                updated_at: now,
                ..record
            };
            self.user_store.upsert_agent_thread(&updated)?;
        }
        Ok(())
    }

    pub async fn list_thread_tasks(
        &self,
        thread_id: &str,
        status: Option<&str>,
        limit: i64,
    ) -> Result<Vec<AgentTaskRecord>> {
        self.user_store
            .list_agent_tasks_by_thread(thread_id, status, limit)
    }

    pub fn cancel_task(&self, task_id: &str) -> Result<()> {
        let now = now_ts();
        self.user_store
            .update_agent_task_status(UpdateAgentTaskStatusParams {
                task_id,
                status: TASK_STATUS_CANCELLED,
                retry_count: 0,
                retry_at: now,
                started_at: None,
                finished_at: Some(now),
                last_error: Some("cancelled"),
                updated_at: now,
            })?;
        Ok(())
    }

    pub async fn cancel_session_activity(
        &self,
        user_id: &str,
        session_id: &str,
        cancel_source: &str,
    ) -> Result<ThreadCancelSettlement> {
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Err(anyhow!(i18n::t("error.content_required")));
        }
        let source = cancel_source.trim();
        let source = if source.is_empty() {
            "thread_cancel"
        } else {
            source
        };

        let monitor_cancelled = self.monitor.cancel_with_source(cleaned_session, source);
        self.cancel_pending_goal_continuation(cleaned_session);

        let thread_id = format!("thread_{cleaned_session}");
        let tasks = self
            .user_store
            .list_agent_tasks_by_thread(&thread_id, None, 64)?;
        let mut queued_tasks_cancelled = 0usize;
        let mut running_tasks_marked_cancelled = 0usize;
        let mut agent_id = String::new();
        for task in tasks {
            if task.user_id.trim() != cleaned_user {
                continue;
            }
            if agent_id.is_empty() && !task.agent_id.trim().is_empty() {
                agent_id = task.agent_id.clone();
            }
            let status = task.status.trim().to_ascii_lowercase();
            if status == TASK_STATUS_PENDING || status == TASK_STATUS_RETRY {
                self.cancel_task(&task.task_id)?;
                queued_tasks_cancelled = queued_tasks_cancelled.saturating_add(1);
                self.emit_queue_event(
                    cleaned_session,
                    cleaned_user,
                    "queue_fail",
                    json!({
                        "queue_id": task.task_id,
                        "thread_id": task.thread_id,
                        "session_id": task.session_id,
                        "agent_id": task.agent_id,
                        "user_id": task.user_id,
                        "status": TASK_STATUS_CANCELLED,
                        "error": "cancelled",
                        "queue_ahead": 0,
                        "queue_total": 0,
                    }),
                )
                .await;
            } else if status == TASK_STATUS_RUNNING {
                self.cancel_task(&task.task_id)?;
                running_tasks_marked_cancelled = running_tasks_marked_cancelled.saturating_add(1);
                self.emit_queue_event(
                    cleaned_session,
                    cleaned_user,
                    "queue_fail",
                    json!({
                        "queue_id": task.task_id,
                        "thread_id": task.thread_id,
                        "session_id": task.session_id,
                        "agent_id": task.agent_id,
                        "user_id": task.user_id,
                        "status": TASK_STATUS_CANCELLED,
                        "error": "cancelled",
                        "queue_ahead": 0,
                        "queue_total": 0,
                    }),
                )
                .await;
            }
        }

        if agent_id.is_empty() {
            if let Ok(Some(session)) = self
                .user_store
                .get_chat_session(cleaned_user, cleaned_session)
            {
                agent_id = session.agent_id.unwrap_or_default();
            }
        }

        let mut thread_status_reset = false;
        if !agent_id.trim().is_empty() {
            self.update_thread_status(
                cleaned_user,
                &agent_id,
                cleaned_session,
                THREAD_STATUS_IDLE,
            )?;
            thread_status_reset = true;
        }
        let settlement_event_id = self
            .emit_thread_status_event(
                cleaned_session,
                cleaned_user,
                "cancelled",
                source,
                queued_tasks_cancelled,
                running_tasks_marked_cancelled,
            )
            .await;
        let _ = self.queue_tx.try_send(());

        Ok(ThreadCancelSettlement {
            monitor_cancelled,
            queued_tasks_cancelled,
            running_tasks_marked_cancelled,
            thread_status_reset,
            settlement_event_id,
        })
    }

    async fn resolve_or_create_main_session(
        &self,
        user_id: &str,
        agent_id: &str,
    ) -> Result<String> {
        if let Some(existing) = self.resolve_main_session_id(user_id, agent_id).await? {
            return Ok(existing);
        }
        let now = now_ts();
        let session_id = format!("sess_{}", Uuid::new_v4().simple());
        let record = ChatSessionRecord {
            session_id: session_id.clone(),
            user_id: user_id.to_string(),
            title: DEFAULT_SESSION_TITLE.to_string(),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
            last_message_at: now,
            agent_id: if agent_id.trim().is_empty() {
                None
            } else {
                Some(agent_id.trim().to_string())
            },
            tool_overrides: Vec::new(),
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        };
        self.user_store.upsert_chat_session(&record)?;
        let _ = self
            .set_main_session(user_id, agent_id, &session_id, "auto_create")
            .await;
        Ok(session_id)
    }

    fn create_isolated_session(&self, user_id: &str, agent_id: &str) -> Result<String> {
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Err(anyhow!(i18n::t("error.user_id_required")));
        }
        let now = now_ts();
        let session_id = format!("sess_{}", Uuid::new_v4().simple());
        let record = ChatSessionRecord {
            session_id: session_id.clone(),
            user_id: cleaned_user.to_string(),
            title: DEFAULT_SESSION_TITLE.to_string(),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
            last_message_at: now,
            agent_id: if agent_id.trim().is_empty() {
                None
            } else {
                Some(agent_id.trim().to_string())
            },
            tool_overrides: Vec::new(),
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        };
        self.user_store.upsert_chat_session(&record)?;
        Ok(session_id)
    }

    fn ensure_session_record(
        &self,
        user_id: &str,
        session_id: &str,
        agent_id: &str,
    ) -> Result<ChatSessionRecord> {
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Err(anyhow!(i18n::t("error.content_required")));
        }
        let existing = self
            .user_store
            .get_chat_session(cleaned_user, cleaned_session)?;
        if let Some(mut record) = existing {
            if !agent_id.trim().is_empty() {
                let record_agent = record.agent_id.as_deref().unwrap_or("").trim();
                if record_agent.is_empty() {
                    record.agent_id = Some(agent_id.trim().to_string());
                    self.user_store.upsert_chat_session(&record)?;
                }
            }
            return Ok(record);
        }
        let now = now_ts();
        let record = ChatSessionRecord {
            session_id: cleaned_session.to_string(),
            user_id: cleaned_user.to_string(),
            title: DEFAULT_SESSION_TITLE.to_string(),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
            last_message_at: now,
            agent_id: if agent_id.trim().is_empty() {
                None
            } else {
                Some(agent_id.trim().to_string())
            },
            tool_overrides: Vec::new(),
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        };
        self.user_store.upsert_chat_session(&record)?;
        Ok(record)
    }

    async fn should_queue(&self, user_id: &str, session_id: Option<&str>) -> bool {
        let Some(session_id) = session_id else {
            return false;
        };
        let config = self.config_store.get().await;
        if !config.agent_queue.enabled {
            return false;
        }
        !self
            .is_session_available_for_submit(user_id, session_id)
            .await
    }

    async fn try_acquire_start_lease(&self, session_id: &str) -> Option<SessionLease> {
        let config = self.config_store.get().await;
        try_acquire_start_lease_with_limit(
            session_id,
            config.server.max_active_sessions.max(1),
            &self.pending_sessions,
            &self.active_runtime_sessions,
        )
    }

    async fn enqueue_task(
        &self,
        request: &WunderRequest,
        agent_id: &str,
        session_id: Option<&str>,
    ) -> Result<QueueInfo> {
        let Some(session_id) = session_id.filter(|value| !value.trim().is_empty()) else {
            return Err(anyhow!(i18n::t("error.session_not_found")));
        };
        let thread_id = format!("thread_{session_id}");
        let now = now_ts();
        let mut payload = serde_json::to_value(request).unwrap_or(Value::Null);
        if let Value::Object(ref mut map) = payload {
            map.insert("stream".to_string(), Value::Bool(true));
        }
        let record = AgentTaskRecord {
            task_id: format!("task_{}", Uuid::new_v4().simple()),
            thread_id: thread_id.clone(),
            user_id: request.user_id.clone(),
            agent_id: agent_id.to_string(),
            session_id: session_id.to_string(),
            status: TASK_STATUS_PENDING.to_string(),
            request_payload: payload,
            request_id: None,
            retry_count: 0,
            retry_at: now,
            created_at: now,
            updated_at: now,
            started_at: None,
            finished_at: None,
            last_error: None,
        };
        self.user_store.insert_agent_task(&record)?;
        let queue_stats = self.compute_queue_stats(&record).await;
        let mut request_payload = record.request_payload.clone();
        if let Value::Object(ref mut map) = request_payload {
            map.insert("queue_ahead".to_string(), json!(queue_stats.queue_ahead));
            map.insert("queue_total".to_string(), json!(queue_stats.queue_total));
            map.insert("active_ahead".to_string(), json!(queue_stats.active_ahead));
            map.insert("wait_ahead".to_string(), json!(queue_stats.wait_ahead));
        }
        self.user_store.insert_agent_task(&AgentTaskRecord {
            request_payload,
            ..record.clone()
        })?;
        let _ = self.update_thread_status(
            &record.user_id,
            &record.agent_id,
            &record.session_id,
            THREAD_STATUS_WAITING,
        );
        let queue_before_event_id = self
            .stream_events
            .tail_event_id(&record.session_id)
            .await
            .unwrap_or(0);
        let queue_event_id = self
            .emit_queue_event(&record.session_id, &record.user_id, "queue_enter", {
                let mut payload = json!({
                    "queue_id": record.task_id,
                    "thread_id": record.thread_id,
                    "session_id": record.session_id,
                    "agent_id": record.agent_id,
                    "user_id": record.user_id,
                    "queue_ahead": queue_stats.queue_ahead,
                    "queue_total": queue_stats.queue_total,
                    "active_ahead": queue_stats.active_ahead,
                    "wait_ahead": queue_stats.wait_ahead,
                });
                if let (Some(client_message_id), Value::Object(ref mut map)) =
                    (request.client_message_id.as_deref(), &mut payload)
                {
                    map.insert("client_message_id".to_string(), json!(client_message_id));
                }
                payload
            })
            .await;
        Ok(QueueInfo {
            task_id: record.task_id,
            thread_id: record.thread_id,
            session_id: record.session_id,
            queue_ahead: queue_stats.queue_ahead,
            queue_total: queue_stats.queue_total,
            active_ahead: queue_stats.active_ahead,
            wait_ahead: queue_stats.wait_ahead,
            queue_event_id,
            queue_after_event_id: if queue_event_id > 0 {
                queue_event_id.saturating_sub(1)
            } else {
                queue_before_event_id
            },
        })
    }

    async fn compute_queue_stats(&self, task: &AgentTaskRecord) -> QueueStats {
        let total = self
            .user_store
            .count_pending_agent_tasks()
            .map(|count| count.max(0) as usize)
            .unwrap_or(0);
        let ahead = self
            .user_store
            .count_pending_agent_tasks_ahead(task.retry_at, task.created_at, &task.task_id)
            .map(|count| count.max(0) as usize)
            .unwrap_or(0);
        let queue_ahead = ahead.min(total.saturating_sub(1));
        let active_ahead = self
            .compute_active_wait_ahead(&task.user_id, &task.session_id)
            .await;
        QueueStats {
            queue_ahead,
            queue_total: total,
            active_ahead,
            wait_ahead: queue_ahead.saturating_add(active_ahead),
        }
    }

    async fn compute_active_wait_ahead(&self, user_id: &str, session_id: &str) -> usize {
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return 0;
        }
        let active_sessions = self.active_runtime_session_count();
        if active_sessions == 0 {
            return 0;
        }
        let config = self.config_store.get().await;
        if active_sessions >= config.server.max_active_sessions.max(1) {
            return active_sessions;
        }
        self.session_has_active_runtime_slot(cleaned_session) as usize
    }

    fn active_runtime_session_count(&self) -> usize {
        self.active_runtime_sessions
            .lock()
            .map(|guard| guard.len())
            .unwrap_or(0)
    }

    fn session_has_active_runtime_slot(&self, session_id: &str) -> bool {
        let cleaned = session_id.trim();
        if cleaned.is_empty() {
            return false;
        }
        self.active_runtime_sessions
            .lock()
            .map(|guard| guard.contains(cleaned))
            .unwrap_or(false)
    }

    async fn emit_queue_update(&self, task: &AgentTaskRecord) {
        let stats = self.compute_queue_stats(task).await;
        let mut request_payload = task.request_payload.clone();
        if let Value::Object(ref mut map) = request_payload {
            map.insert("queue_ahead".to_string(), json!(stats.queue_ahead));
            map.insert("queue_total".to_string(), json!(stats.queue_total));
            map.insert("active_ahead".to_string(), json!(stats.active_ahead));
            map.insert("wait_ahead".to_string(), json!(stats.wait_ahead));
        }
        let _ = self.user_store.insert_agent_task(&AgentTaskRecord {
            request_payload,
            ..task.clone()
        });
        self.emit_queue_event(
            &task.session_id,
            &task.user_id,
            "queue_update",
            json!({
                "queue_id": task.task_id,
                "thread_id": task.thread_id,
                "session_id": task.session_id,
                "agent_id": task.agent_id,
                "user_id": task.user_id,
                "queue_ahead": stats.queue_ahead,
                "queue_total": stats.queue_total,
                "active_ahead": stats.active_ahead,
                "wait_ahead": stats.wait_ahead,
            }),
        )
        .await;
    }

    async fn is_session_available_for_submit(&self, user_id: &str, session_id: &str) -> bool {
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return false;
        }
        if let Some(record) = self.monitor.get_record(cleaned_session) {
            let status = record.get("status").and_then(Value::as_str).unwrap_or("");
            if status == crate::monitor::MonitorState::STATUS_RUNNING
                || status == crate::monitor::MonitorState::STATUS_CANCELLING
                || status == crate::monitor::MonitorState::STATUS_WAITING
            {
                return false;
            }
        }
        if let Ok(locks) = self.user_store.list_session_locks_by_user(cleaned_user) {
            if locks
                .iter()
                .any(|lock| lock.session_id.trim() == cleaned_session)
            {
                return false;
            }
        }
        let thread_id = format!("thread_{cleaned_session}");
        if let Ok(tasks) = self
            .user_store
            .list_agent_tasks_by_thread(&thread_id, None, 8)
        {
            if tasks.iter().any(|task| {
                task.status == TASK_STATUS_PENDING
                    || task.status == TASK_STATUS_RETRY
                    || task.status == TASK_STATUS_RUNNING
            }) {
                return false;
            }
        }
        true
    }

    async fn is_session_idle(&self, user_id: &str, session_id: &str) -> bool {
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return false;
        }
        if let Some(record) = self.monitor.get_record(cleaned_session) {
            let status = record.get("status").and_then(Value::as_str).unwrap_or("");
            if status == crate::monitor::MonitorState::STATUS_RUNNING
                || status == crate::monitor::MonitorState::STATUS_CANCELLING
            {
                return false;
            }
        }
        if let Ok(locks) = self.user_store.list_session_locks_by_user(cleaned_user) {
            if locks
                .iter()
                .any(|lock| lock.session_id.trim() == cleaned_session)
            {
                return false;
            }
        }
        true
    }

    async fn emit_queue_event(
        &self,
        session_id: &str,
        user_id: &str,
        event_type: &str,
        payload: Value,
    ) -> i64 {
        let cleaned_session = session_id.trim();
        let cleaned_user = user_id.trim();
        let cleaned_event = event_type.trim();
        if cleaned_session.is_empty() || cleaned_event.is_empty() {
            return 0;
        }
        self.monitor
            .record_event(cleaned_session, cleaned_event, &payload);
        if cleaned_user.is_empty() {
            return 0;
        }
        let stream_payload = json!({
            "event": cleaned_event,
            "data": payload,
            "timestamp": Utc::now().to_rfc3339(),
        });
        match self
            .stream_events
            .append_event(cleaned_session, cleaned_user, stream_payload)
            .await
        {
            Ok(event_id) => event_id,
            Err(err) => {
                warn!(
                    "append queue stream event failed: session_id={}, event_type={}, error={err}",
                    cleaned_session, cleaned_event
                );
                0
            }
        }
    }

    async fn emit_thread_status_event(
        &self,
        session_id: &str,
        user_id: &str,
        status: &str,
        cancel_source: &str,
        queued_tasks_cancelled: usize,
        running_tasks_marked_cancelled: usize,
    ) -> i64 {
        let cleaned_session = session_id.trim();
        let cleaned_user = user_id.trim();
        let cleaned_status = status.trim();
        if cleaned_session.is_empty() || cleaned_status.is_empty() {
            return 0;
        }

        let mut data = Map::new();
        data.insert("session_id".to_string(), json!(cleaned_session));
        data.insert(
            "thread_id".to_string(),
            json!(format!("thread_{cleaned_session}")),
        );
        data.insert("status".to_string(), json!(cleaned_status));
        data.insert("thread_status".to_string(), json!(cleaned_status));
        data.insert("loaded".to_string(), json!(true));
        data.insert("active_turn_id".to_string(), Value::Null);
        data.insert("cancel_source".to_string(), json!(cancel_source));
        data.insert(
            "queued_tasks_cancelled".to_string(),
            json!(queued_tasks_cancelled),
        );
        data.insert(
            "running_tasks_marked_cancelled".to_string(),
            json!(running_tasks_marked_cancelled),
        );
        let payload = Value::Object(data);
        self.monitor
            .record_event(cleaned_session, "thread_status", &payload);
        if cleaned_user.is_empty() {
            return 0;
        }
        let stream_payload = json!({
            "event": "thread_status",
            "data": payload,
            "timestamp": Utc::now().to_rfc3339(),
        });
        match self
            .stream_events
            .append_event(cleaned_session, cleaned_user, stream_payload)
            .await
        {
            Ok(event_id) => event_id,
            Err(err) => {
                warn!(
                    "append thread status stream event failed: session_id={}, status={}, error={err}",
                    cleaned_session, cleaned_status
                );
                0
            }
        }
    }

    async fn run_loop(self: Arc<Self>) {
        let mut rx = {
            let mut guard = self.queue_rx.lock().await;
            guard.take()
        };
        let Some(mut rx) = rx.take() else {
            warn!("agent queue loop skipped: receiver already taken");
            return;
        };
        loop {
            let config = self.config_store.get().await;
            let poll_interval = config.agent_queue.poll_interval_ms.max(200);
            tokio::select! {
                _ = rx.recv() => {
                    runtime_metrics::record_loop_tick("runtime.thread.queue_loop", "wake");
                },
                _ = tokio::time::sleep(std::time::Duration::from_millis(poll_interval)) => {
                    runtime_metrics::record_loop_tick("runtime.thread.queue_loop", "poll");
                },
            }
            if !config.agent_queue.enabled {
                continue;
            }
            if let Err(err) = self.process_pending_tasks().await {
                warn!("agent queue loop error: {err}");
            }
        }
    }

    async fn process_pending_tasks(&self) -> Result<()> {
        let pending = {
            let store = self.user_store.clone();
            match blocking::run_db("runtime.thread.list_pending_tasks", move || {
                store.list_pending_agent_tasks(50)
            })
            .await
            {
                Ok(items) => items,
                Err(err) => {
                    warn!("load pending agent tasks failed: {err}");
                    Vec::new()
                }
            }
        };
        if pending.is_empty() {
            return Ok(());
        }
        for task in &pending {
            self.emit_queue_update(task).await;
        }
        let config = self.config_store.get().await;
        let ttl_s = config.agent_queue.task_ttl_s as f64;
        for task in pending {
            if ttl_s > 0.0 {
                let age = now_ts() - task.created_at;
                if age > ttl_s {
                    let _ = self
                        .fail_task(&task, "task expired".to_string(), TASK_STATUS_DEAD)
                        .await;
                    continue;
                }
            }
            let Some(lease) = self.should_attempt_task(&task).await else {
                continue;
            };
            let task_clone = task.clone();
            let runtime = self.clone();
            long_task::spawn("runtime.thread.execute_task", async move {
                let _lease = lease;
                runtime.execute_task(task_clone).await;
            });
        }
        Ok(())
    }

    async fn should_attempt_task(&self, task: &AgentTaskRecord) -> Option<SessionLease> {
        if task.status != TASK_STATUS_PENDING && task.status != TASK_STATUS_RETRY {
            return None;
        }
        if !self.is_session_idle(&task.user_id, &task.session_id).await {
            return None;
        }
        let mut running = self.running_threads.lock().await;
        if running.contains(&task.thread_id) {
            return None;
        }
        running.insert(task.thread_id.clone());
        drop(running);
        if let Some(lease) = self.try_acquire_start_lease(&task.session_id).await {
            Some(lease)
        } else {
            self.finish_thread(&task.thread_id).await;
            None
        }
    }

    async fn execute_task(&self, task: AgentTaskRecord) {
        let started_at = now_ts();
        let task_client_message_id = task
            .request_payload
            .get("client_message_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let _ = self
            .user_store
            .update_agent_task_status(UpdateAgentTaskStatusParams {
                task_id: &task.task_id,
                status: TASK_STATUS_RUNNING,
                retry_count: task.retry_count,
                retry_at: started_at,
                started_at: Some(started_at),
                finished_at: None,
                last_error: None,
                updated_at: started_at,
            });
        let _ = self.update_thread_status(
            &task.user_id,
            &task.agent_id,
            &task.session_id,
            THREAD_STATUS_BUSY,
        );
        self.emit_queue_event(
            &task.session_id,
            &task.user_id,
            "queue_start",
            {
                let mut payload = json!({
                "queue_id": task.task_id,
                "thread_id": task.thread_id,
                "session_id": task.session_id,
                "agent_id": task.agent_id,
                "user_id": task.user_id,
                "queue_ahead": 0,
                "queue_total": 0,
                });
                if let (Some(client_message_id), Value::Object(ref mut map)) =
                    (task_client_message_id.as_deref(), &mut payload)
                {
                    map.insert("client_message_id".to_string(), json!(client_message_id));
                }
                payload
            },
        )
        .await;

        let mut request: WunderRequest = match serde_json::from_value(task.request_payload.clone())
        {
            Ok(value) => value,
            Err(err) => {
                let _ = self
                    .fail_task(
                        &task,
                        format!("payload decode failed: {err}"),
                        TASK_STATUS_FAILED,
                    )
                    .await;
                self.finish_thread(&task.thread_id).await;
                return;
            }
        };
        request.stream = true;
        request.session_id = Some(task.session_id.clone());
        if request.agent_id.is_none() && !task.agent_id.trim().is_empty() {
            request.agent_id = Some(task.agent_id.clone());
        }
        if let Ok(Some(user)) = self.user_store.get_user_by_id(&task.user_id) {
            request.is_admin = UserStore::is_admin(&user);
        }

        match self.orchestrator.stream(request).await {
            Ok(stream) => {
                let mut stream = Box::pin(stream);
                let mut goal_continue_ready = false;
                while let Some(item) = stream.next().await {
                    match item {
                        Ok(event) => {
                            if event.event == "goal_continuation_ready" {
                                goal_continue_ready = true;
                            }
                        }
                        Err(err) => match err {},
                    }
                    // drain
                }
                crate::orchestrator::flush_stream_event_persist_queue().await;
                if self.is_task_cancelled(&task.task_id) {
                    let _ = self.update_thread_status(
                        &task.user_id,
                        &task.agent_id,
                        &task.session_id,
                        THREAD_STATUS_IDLE,
                    );
                    self.finish_thread(&task.thread_id).await;
                    let _ = self.queue_tx.try_send(());
                    return;
                }
                let finished_at = now_ts();
                let _ = self
                    .user_store
                    .update_agent_task_status(UpdateAgentTaskStatusParams {
                        task_id: &task.task_id,
                        status: TASK_STATUS_SUCCESS,
                        retry_count: task.retry_count,
                        retry_at: finished_at,
                        started_at: Some(started_at),
                        finished_at: Some(finished_at),
                        last_error: None,
                        updated_at: finished_at,
                    });
                let _ = self.update_thread_status(
                    &task.user_id,
                    &task.agent_id,
                    &task.session_id,
                    THREAD_STATUS_IDLE,
                );
                self.emit_queue_event(
                    &task.session_id,
                    &task.user_id,
                    "queue_finish",
                    {
                        let mut payload = json!({
                        "queue_id": task.task_id,
                        "thread_id": task.thread_id,
                        "session_id": task.session_id,
                        "agent_id": task.agent_id,
                        "user_id": task.user_id,
                        "queue_ahead": 0,
                        "queue_total": 0,
                        });
                        if let (Some(client_message_id), Value::Object(ref mut map)) =
                            (task_client_message_id.as_deref(), &mut payload)
                        {
                            map.insert("client_message_id".to_string(), json!(client_message_id));
                        }
                        payload
                    },
                )
                .await;
                if goal_continue_ready {
                    self.spawn_goal_continuation_after_cooldown(
                        task.user_id.clone(),
                        task.session_id.clone(),
                    );
                }
            }
            Err(err) => {
                let is_busy = err
                    .downcast_ref::<OrchestratorError>()
                    .map(|inner| inner.code() == "USER_BUSY")
                    .unwrap_or(false);
                if is_busy {
                    let _ = self.retry_task(&task, err.to_string()).await;
                    let _ = self.update_thread_status(
                        &task.user_id,
                        &task.agent_id,
                        &task.session_id,
                        THREAD_STATUS_WAITING,
                    );
                } else {
                    let _ = self
                        .fail_task(&task, err.to_string(), TASK_STATUS_FAILED)
                        .await;
                }
            }
        }
        self.finish_thread(&task.thread_id).await;
        let _ = self.queue_tx.try_send(());
    }

    async fn retry_task(&self, task: &AgentTaskRecord, message: String) -> Result<()> {
        let config = self.config_store.get().await;
        let max_retries = config.agent_queue.max_retries as i64;
        let next_retry = task.retry_count.saturating_add(1);
        if next_retry > max_retries {
            return self.fail_task(task, message, TASK_STATUS_DEAD).await;
        }
        let delay = 1.5_f64.powi(next_retry as i32).clamp(1.0, 30.0);
        let now = now_ts();
        self.user_store
            .update_agent_task_status(UpdateAgentTaskStatusParams {
                task_id: &task.task_id,
                status: TASK_STATUS_RETRY,
                retry_count: next_retry,
                retry_at: now + delay,
                started_at: task.started_at.or(Some(now)),
                finished_at: None,
                last_error: Some(message.as_str()),
                updated_at: now,
            })?;
        Ok(())
    }

    async fn fail_task(&self, task: &AgentTaskRecord, message: String, status: &str) -> Result<()> {
        crate::orchestrator::flush_stream_event_persist_queue().await;
        let now = now_ts();
        self.user_store
            .update_agent_task_status(UpdateAgentTaskStatusParams {
                task_id: &task.task_id,
                status,
                retry_count: task.retry_count,
                retry_at: now,
                started_at: task.started_at.or(Some(now)),
                finished_at: Some(now),
                last_error: Some(message.as_str()),
                updated_at: now,
            })?;
        self.update_thread_status(
            &task.user_id,
            &task.agent_id,
            &task.session_id,
            THREAD_STATUS_IDLE,
        )?;
        self.emit_queue_event(
            &task.session_id,
            &task.user_id,
            "queue_fail",
            {
                let task_client_message_id = task
                    .request_payload
                    .get("client_message_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
                let mut payload = json!({
                "queue_id": task.task_id,
                "thread_id": task.thread_id,
                "session_id": task.session_id,
                "agent_id": task.agent_id,
                "user_id": task.user_id,
                "status": status,
                "error": message,
                "queue_ahead": 0,
                "queue_total": 0,
                });
                if let (Some(client_message_id), Value::Object(ref mut map)) =
                    (task_client_message_id.as_deref(), &mut payload)
                {
                    map.insert("client_message_id".to_string(), json!(client_message_id));
                }
                payload
            },
        )
        .await;
        Ok(())
    }

    fn is_task_cancelled(&self, task_id: &str) -> bool {
        let cleaned = task_id.trim();
        if cleaned.is_empty() {
            return false;
        }
        self.user_store
            .get_agent_task(cleaned)
            .map(|task| {
                task.map(|record| {
                    record
                        .status
                        .trim()
                        .eq_ignore_ascii_case(TASK_STATUS_CANCELLED)
                })
                .unwrap_or(false)
            })
            .unwrap_or(false)
    }

    async fn finish_thread(&self, thread_id: &str) {
        let mut running = self.running_threads.lock().await;
        running.remove(thread_id);
    }
}

fn normalize_agent_id(value: Option<&str>) -> String {
    value.unwrap_or("").trim().to_string()
}

fn normalize_client_message_id(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(128).collect::<String>())
}

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

fn try_acquire_start_lease_with_limit(
    session_id: &str,
    max_active: usize,
    pending_sessions: &Arc<StdMutex<HashSet<String>>>,
    active_runtime_sessions: &Arc<StdMutex<HashSet<String>>>,
) -> Option<SessionLease> {
    let cleaned = session_id.trim();
    if cleaned.is_empty() {
        return None;
    }
    let max_active = max_active.max(1);
    let mut pending_guard = pending_sessions.lock().ok()?;
    let mut active_guard = active_runtime_sessions.lock().ok()?;
    if pending_guard.contains(cleaned)
        || active_guard.contains(cleaned)
        || active_guard.len() >= max_active
    {
        return None;
    }
    pending_guard.insert(cleaned.to_string());
    active_guard.insert(cleaned.to_string());
    Some(SessionLease::new(
        cleaned.to_string(),
        pending_sessions.clone(),
        active_runtime_sessions.clone(),
    ))
}

#[derive(Debug, Clone, Copy, Default)]
struct QueueStats {
    queue_ahead: usize,
    queue_total: usize,
    active_ahead: usize,
    wait_ahead: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_lease_enforces_global_capacity_and_releases_on_drop() {
        let pending = Arc::new(StdMutex::new(HashSet::new()));
        let active = Arc::new(StdMutex::new(HashSet::new()));

        let first = try_acquire_start_lease_with_limit("session-a", 1, &pending, &active)
            .expect("first session should acquire slot");
        assert!(try_acquire_start_lease_with_limit("session-b", 1, &pending, &active).is_none());
        assert_eq!(active.lock().expect("active lock").len(), 1);

        drop(first);
        assert!(try_acquire_start_lease_with_limit("session-b", 1, &pending, &active).is_some());
    }

    #[test]
    fn start_lease_blocks_same_session_until_release() {
        let pending = Arc::new(StdMutex::new(HashSet::new()));
        let active = Arc::new(StdMutex::new(HashSet::new()));

        let first = try_acquire_start_lease_with_limit("session-a", 2, &pending, &active)
            .expect("first session should acquire slot");
        assert!(try_acquire_start_lease_with_limit("session-a", 2, &pending, &active).is_none());

        drop(first);
        assert!(try_acquire_start_lease_with_limit("session-a", 2, &pending, &active).is_some());
    }
}
