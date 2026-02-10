use crate::config_store::ConfigStore;
use crate::i18n;
use crate::monitor::MonitorState;
use crate::orchestrator::{Orchestrator, OrchestratorError};
use crate::schemas::WunderRequest;
use crate::storage::{
    AgentTaskRecord, AgentThreadRecord, ChatSessionRecord, UpdateAgentTaskStatusParams,
};
use crate::user_store::UserStore;
use anyhow::{anyhow, Result};
use chrono::Utc;
use futures::StreamExt;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{mpsc, Mutex};
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
}

#[derive(Debug, Clone)]
pub enum AgentSubmitOutcome {
    Run(Box<WunderRequest>, Option<SessionLease>),
    Queued(QueueInfo),
}

#[derive(Debug, Clone)]
pub struct SessionLease {
    session_id: String,
    pending_sessions: Arc<StdMutex<HashSet<String>>>,
}

impl SessionLease {
    fn new(session_id: String, pending_sessions: Arc<StdMutex<HashSet<String>>>) -> Self {
        Self {
            session_id,
            pending_sessions,
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
    }
}

#[derive(Clone)]
pub struct AgentRuntime {
    config_store: ConfigStore,
    user_store: Arc<UserStore>,
    monitor: Arc<MonitorState>,
    orchestrator: Arc<Orchestrator>,
    queue_tx: mpsc::Sender<()>,
    queue_rx: Arc<Mutex<Option<mpsc::Receiver<()>>>>,
    running_threads: Arc<Mutex<HashSet<String>>>,
    pending_sessions: Arc<StdMutex<HashSet<String>>>,
}

impl AgentRuntime {
    pub fn new(
        config_store: ConfigStore,
        user_store: Arc<UserStore>,
        monitor: Arc<MonitorState>,
        orchestrator: Arc<Orchestrator>,
    ) -> Arc<Self> {
        let (queue_tx, queue_rx) = mpsc::channel(64);
        Arc::new(Self {
            config_store,
            user_store,
            monitor,
            orchestrator,
            queue_tx,
            queue_rx: Arc::new(Mutex::new(Some(queue_rx))),
            running_threads: Arc::new(Mutex::new(HashSet::new())),
            pending_sessions: Arc::new(StdMutex::new(HashSet::new())),
        })
    }

    pub fn queue_waker(&self) -> mpsc::Sender<()> {
        self.queue_tx.clone()
    }

    pub fn start(self: Arc<Self>) {
        let runtime = self.clone();
        tokio::spawn(async move {
            runtime.run_loop().await;
        });
    }

    pub async fn wake(&self) {
        let _ = self.queue_tx.try_send(());
    }

    pub async fn submit_user_request(
        &self,
        mut request: WunderRequest,
    ) -> Result<AgentSubmitOutcome> {
        let user_id = request.user_id.trim().to_string();
        if user_id.is_empty() {
            return Err(anyhow!(i18n::t("error.user_id_required")));
        }
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
                if !self.is_session_idle(&user_id, current_session_id).await {
                    // For implicit session requests, fork when main is busy so concurrent app calls stay parallel.
                    let forked = self.create_isolated_session(&user_id, &agent_id)?;
                    session_id = Some(forked);
                    request.session_id = session_id.clone();
                    set_as_main = false;
                }
            }
        }

        if let Some(session_id) = session_id.as_ref() {
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
                    let info = self.enqueue_task(&request, &agent_id, Some(session_id))?;
                    return Ok(AgentSubmitOutcome::Queued(info));
                }
                lease = self.try_acquire_session_lease(session_id).await;
                if lease.is_none() {
                    let info = self.enqueue_task(&request, &agent_id, Some(session_id))?;
                    return Ok(AgentSubmitOutcome::Queued(info));
                }
            }
        }

        Ok(AgentSubmitOutcome::Run(Box::new(request), lease))
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
        !self.is_session_idle(user_id, session_id).await
    }

    async fn try_acquire_session_lease(&self, session_id: &str) -> Option<SessionLease> {
        let cleaned = session_id.trim();
        if cleaned.is_empty() {
            return None;
        }
        let mut guard = self.pending_sessions.lock().ok()?;
        if guard.contains(cleaned) {
            return None;
        }
        guard.insert(cleaned.to_string());
        Some(SessionLease::new(
            cleaned.to_string(),
            self.pending_sessions.clone(),
        ))
    }

    fn enqueue_task(
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
        let _ = self.update_thread_status(
            &record.user_id,
            &record.agent_id,
            &record.session_id,
            THREAD_STATUS_WAITING,
        );
        self.monitor.record_event(
            &record.session_id,
            "queue_enter",
            &json!({
                "queue_id": record.task_id,
                "thread_id": record.thread_id,
                "session_id": record.session_id,
                "agent_id": record.agent_id,
                "user_id": record.user_id,
            }),
        );
        Ok(QueueInfo {
            task_id: record.task_id,
            thread_id: record.thread_id,
            session_id: record.session_id,
        })
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
        true
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
                _ = rx.recv() => {},
                _ = tokio::time::sleep(std::time::Duration::from_millis(poll_interval)) => {},
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
            match tokio::task::spawn_blocking(move || store.list_pending_agent_tasks(50)).await {
                Ok(Ok(items)) => items,
                Ok(Err(err)) => {
                    warn!("load pending agent tasks failed: {err}");
                    Vec::new()
                }
                Err(err) => {
                    warn!("load pending agent tasks join failed: {err}");
                    Vec::new()
                }
            }
        };
        if pending.is_empty() {
            return Ok(());
        }
        let config = self.config_store.get().await;
        let ttl_s = config.agent_queue.task_ttl_s as f64;
        for task in pending {
            if ttl_s > 0.0 {
                let age = now_ts() - task.created_at;
                if age > ttl_s {
                    let _ = self.fail_task(&task, "task expired".to_string(), TASK_STATUS_DEAD);
                    continue;
                }
            }
            if !self.should_attempt_task(&task).await {
                continue;
            }
            let task_clone = task.clone();
            let runtime = self.clone();
            tokio::spawn(async move {
                runtime.execute_task(task_clone).await;
            });
        }
        Ok(())
    }

    async fn should_attempt_task(&self, task: &AgentTaskRecord) -> bool {
        if task.status != TASK_STATUS_PENDING && task.status != TASK_STATUS_RETRY {
            return false;
        }
        if !self.is_session_idle(&task.user_id, &task.session_id).await {
            return false;
        }
        let mut running = self.running_threads.lock().await;
        if running.contains(&task.thread_id) {
            return false;
        }
        running.insert(task.thread_id.clone());
        true
    }

    async fn execute_task(&self, task: AgentTaskRecord) {
        let started_at = now_ts();
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
        self.monitor.record_event(
            &task.session_id,
            "queue_start",
            &json!({
                "queue_id": task.task_id,
                "thread_id": task.thread_id,
                "session_id": task.session_id,
                "agent_id": task.agent_id,
                "user_id": task.user_id,
            }),
        );

        let mut request: WunderRequest = match serde_json::from_value(task.request_payload.clone())
        {
            Ok(value) => value,
            Err(err) => {
                let _ = self.fail_task(
                    &task,
                    format!("payload decode failed: {err}"),
                    TASK_STATUS_FAILED,
                );
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
                while let Some(_item) = stream.next().await {
                    // drain
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
                self.monitor.record_event(
                    &task.session_id,
                    "queue_finish",
                    &json!({
                        "queue_id": task.task_id,
                        "thread_id": task.thread_id,
                        "session_id": task.session_id,
                        "agent_id": task.agent_id,
                        "user_id": task.user_id,
                    }),
                );
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
                    let _ = self.fail_task(&task, err.to_string(), TASK_STATUS_FAILED);
                    self.monitor.record_event(
                        &task.session_id,
                        "queue_fail",
                        &json!({
                            "queue_id": task.task_id,
                            "thread_id": task.thread_id,
                            "session_id": task.session_id,
                            "agent_id": task.agent_id,
                            "user_id": task.user_id,
                            "error": err.to_string(),
                        }),
                    );
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
            return self.fail_task(task, message, TASK_STATUS_DEAD);
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

    fn fail_task(&self, task: &AgentTaskRecord, message: String, status: &str) -> Result<()> {
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
        Ok(())
    }

    async fn finish_thread(&self, thread_id: &str) {
        let mut running = self.running_threads.lock().await;
        running.remove(thread_id);
    }
}

fn normalize_agent_id(value: Option<&str>) -> String {
    value.unwrap_or("").trim().to_string()
}

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}
