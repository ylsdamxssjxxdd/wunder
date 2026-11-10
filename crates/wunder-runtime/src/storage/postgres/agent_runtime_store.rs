use super::PostgresStorage;
use crate::storage::{AgentTaskRecord, StorageLifecycle, UpdateAgentTaskStatusParams};
use anyhow::Result;
use serde_json::Value;

pub(super) trait PostgresAgentRuntimeStorage {
    fn insert_agent_task_impl(&self, record: &AgentTaskRecord) -> Result<()>;
    fn get_agent_task_impl(&self, task_id: &str) -> Result<Option<AgentTaskRecord>>;
    fn list_pending_agent_tasks_impl(&self, limit: i64) -> Result<Vec<AgentTaskRecord>>;
    fn count_pending_agent_tasks_impl(&self) -> Result<i64>;
    fn count_pending_agent_tasks_ahead_impl(
        &self,
        retry_at: f64,
        created_at: f64,
        task_id: &str,
    ) -> Result<i64>;
    fn list_agent_tasks_by_thread_impl(
        &self,
        thread_id: &str,
        status: Option<&str>,
        limit: i64,
    ) -> Result<Vec<AgentTaskRecord>>;
    fn update_agent_task_status_impl(&self, params: UpdateAgentTaskStatusParams<'_>) -> Result<()>;
    fn get_max_stream_event_id_impl(&self, session_id: &str) -> Result<i64>;
    fn append_stream_event_impl(
        &self,
        session_id: &str,
        user_id: &str,
        event_id: i64,
        payload: &Value,
    ) -> Result<()>;
    fn load_stream_events_impl(
        &self,
        session_id: &str,
        after_event_id: i64,
        limit: i64,
    ) -> Result<Vec<Value>>;
    fn load_recent_stream_events_impl(&self, session_id: &str, limit: i64) -> Result<Vec<Value>>;
    fn max_session_model_round_impl(&self, session_id: &str, user_round: i64) -> Result<i64>;
    fn load_session_workflow_events_impl(
        &self,
        session_id: &str,
        from_user_round: i64,
        to_user_round: i64,
    ) -> Result<Vec<Value>>;
    fn load_session_workflow_events_page_impl(
        &self,
        session_id: &str,
        from_user_round: i64,
        to_user_round: i64,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<Value>>;
    fn count_session_workflow_events_impl(
        &self,
        session_id: &str,
        from_user_round: i64,
        to_user_round: i64,
    ) -> Result<i64>;
    fn delete_stream_events_before_impl(&self, before_time: f64) -> Result<i64>;
    fn delete_stream_events_by_user_impl(&self, user_id: &str) -> Result<i64>;
    fn delete_stream_events_by_session_impl(&self, session_id: &str) -> Result<i64>;
    fn delete_stream_events_by_round_impl(
        &self,
        session_id: &str,
        user_round: i64,
        event_types: &[&str],
    ) -> Result<i64>;
}

impl PostgresAgentRuntimeStorage for PostgresStorage {
    fn insert_agent_task_impl(&self, record: &AgentTaskRecord) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = record.user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(());
        }
        let payload =
            serde_json::to_string(&record.request_payload).unwrap_or_else(|_| "{}".to_string());
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO agent_tasks (task_id, thread_id, user_id, agent_id, session_id, status, request_payload, request_id, retry_count, retry_at, created_at, updated_at, started_at, finished_at, last_error) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15) \
             ON CONFLICT (task_id) DO UPDATE SET status = EXCLUDED.status, request_payload = EXCLUDED.request_payload, \
             request_id = EXCLUDED.request_id, retry_count = EXCLUDED.retry_count, retry_at = EXCLUDED.retry_at, updated_at = EXCLUDED.updated_at, \
             started_at = EXCLUDED.started_at, finished_at = EXCLUDED.finished_at, last_error = EXCLUDED.last_error",
            &[
                &record.task_id,
                &record.thread_id,
                &cleaned_user,
                &record.agent_id.trim(),
                &record.session_id,
                &record.status,
                &payload,
                &record.request_id,
                &record.retry_count,
                &record.retry_at,
                &record.created_at,
                &record.updated_at,
                &record.started_at,
                &record.finished_at,
                &record.last_error,
            ],
        )?;
        tx.commit()?;
        // Task persistence precedes the idempotent local admission transition.
        // The two stores do not share a transaction; a retry repairs a failed
        // transition without creating another history or accepted input copy.
        self.park_queued_admission(record)?;
        Ok(())
    }

    fn get_agent_task_impl(&self, task_id: &str) -> Result<Option<AgentTaskRecord>> {
        self.ensure_initialized()?;
        let cleaned = task_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT task_id, thread_id, user_id, agent_id, session_id, status, request_payload, request_id, retry_count, retry_at, created_at, updated_at, started_at, finished_at, last_error \
             FROM agent_tasks WHERE task_id = $1 LIMIT 1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| {
            let raw_payload: String = row.get(6);
            let payload = serde_json::from_str(&raw_payload).unwrap_or(Value::Null);
            AgentTaskRecord {
                task_id: row.get(0),
                thread_id: row.get(1),
                user_id: row.get(2),
                agent_id: row.get(3),
                session_id: row.get(4),
                status: row.get(5),
                request_payload: payload,
                request_id: row.get(7),
                retry_count: row.get(8),
                retry_at: row.get(9),
                created_at: row.get(10),
                updated_at: row.get(11),
                started_at: row.get(12),
                finished_at: row.get(13),
                last_error: row.get(14),
            }
        }))
    }

    fn list_pending_agent_tasks_impl(&self, limit: i64) -> Result<Vec<AgentTaskRecord>> {
        self.ensure_initialized()?;
        let now = Self::now_ts();
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT task_id, thread_id, user_id, agent_id, session_id, status, request_payload, request_id, retry_count, retry_at, created_at, updated_at, started_at, finished_at, last_error \
             FROM agent_tasks WHERE (status = 'pending' OR status = 'retry') AND retry_at <= $1 \
             ORDER BY priority DESC, retry_at ASC, created_at ASC, task_id ASC LIMIT $2",
            &[&now, &limit.max(1)],
        )?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let raw_payload: String = row.get(6);
                let payload = serde_json::from_str(&raw_payload).unwrap_or(Value::Null);
                AgentTaskRecord {
                    task_id: row.get(0),
                    thread_id: row.get(1),
                    user_id: row.get(2),
                    agent_id: row.get(3),
                    session_id: row.get(4),
                    status: row.get(5),
                    request_payload: payload,
                    request_id: row.get(7),
                    retry_count: row.get(8),
                    retry_at: row.get(9),
                    created_at: row.get(10),
                    updated_at: row.get(11),
                    started_at: row.get(12),
                    finished_at: row.get(13),
                    last_error: row.get(14),
                }
            })
            .collect())
    }

    fn count_pending_agent_tasks_impl(&self) -> Result<i64> {
        self.ensure_initialized()?;
        let now = Self::now_ts();
        let mut conn = self.conn()?;
        let total = conn
            .query_one(
                "SELECT COUNT(*) FROM agent_tasks WHERE (status = 'pending' OR status = 'retry') AND retry_at <= $1",
                &[&now],
            )?
            .get(0);
        Ok(total)
    }

    fn count_pending_agent_tasks_ahead_impl(
        &self,
        retry_at: f64,
        created_at: f64,
        task_id: &str,
    ) -> Result<i64> {
        self.ensure_initialized()?;
        let now = Self::now_ts();
        let mut conn = self.conn()?;
        let total = conn
            .query_one(
                "SELECT COUNT(*) FROM agent_tasks \
                 WHERE (status = 'pending' OR status = 'retry') AND retry_at <= $1 \
                   AND (priority > COALESCE((SELECT priority FROM agent_tasks WHERE task_id = $7), 0) OR (priority = COALESCE((SELECT priority FROM agent_tasks WHERE task_id = $7), 0) AND (retry_at < $2 OR (retry_at = $3 AND created_at < $4) OR (retry_at = $5 AND created_at = $6 AND task_id < $7))))",
                &[&now, &retry_at, &retry_at, &created_at, &retry_at, &created_at, &task_id],
            )?
            .get(0);
        Ok(total)
    }

    fn list_agent_tasks_by_thread_impl(
        &self,
        thread_id: &str,
        status: Option<&str>,
        limit: i64,
    ) -> Result<Vec<AgentTaskRecord>> {
        self.ensure_initialized()?;
        let cleaned_thread = thread_id.trim();
        if cleaned_thread.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let rows = if let Some(status) = status.filter(|value| !value.trim().is_empty()) {
            conn.query(
                "SELECT task_id, thread_id, user_id, agent_id, session_id, status, request_payload, request_id, retry_count, retry_at, created_at, updated_at, started_at, finished_at, last_error \
                 FROM agent_tasks WHERE thread_id = $1 AND status = $2 ORDER BY created_at DESC LIMIT $3",
                &[&cleaned_thread, &status.trim(), &limit.max(1)],
            )?
        } else {
            conn.query(
                "SELECT task_id, thread_id, user_id, agent_id, session_id, status, request_payload, request_id, retry_count, retry_at, created_at, updated_at, started_at, finished_at, last_error \
                 FROM agent_tasks WHERE thread_id = $1 ORDER BY created_at DESC LIMIT $2",
                &[&cleaned_thread, &limit.max(1)],
            )?
        };
        Ok(rows
            .into_iter()
            .map(|row| {
                let raw_payload: String = row.get(6);
                let payload = serde_json::from_str(&raw_payload).unwrap_or(Value::Null);
                AgentTaskRecord {
                    task_id: row.get(0),
                    thread_id: row.get(1),
                    user_id: row.get(2),
                    agent_id: row.get(3),
                    session_id: row.get(4),
                    status: row.get(5),
                    request_payload: payload,
                    request_id: row.get(7),
                    retry_count: row.get(8),
                    retry_at: row.get(9),
                    created_at: row.get(10),
                    updated_at: row.get(11),
                    started_at: row.get(12),
                    finished_at: row.get(13),
                    last_error: row.get(14),
                }
            })
            .collect())
    }

    fn update_agent_task_status_impl(&self, params: UpdateAgentTaskStatusParams<'_>) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = params.task_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        // Terminal states are monotonic. A late completion/error callback must not
        // resurrect a cancelled or already finished task.
        let allowed_sources = match params.status {
            "running" => "('pending','retry')",
            "retry" => "('pending','retry','running')",
            "success" | "failed" | "dead" | "cancelled" => "('pending','retry','running')",
            _ => return Ok(()),
        };
        let mut conn = self.conn()?;
        conn.execute(
            &format!("UPDATE agent_tasks SET status = $1, retry_count = $2, retry_at = $3, started_at = $4, finished_at = $5, last_error = $6, updated_at = $7 WHERE task_id = $8 AND status IN {allowed_sources}"),
            &[
                &params.status,
                &params.retry_count,
                &params.retry_at,
                &params.started_at,
                &params.finished_at,
                &params.last_error,
                &params.updated_at,
                &cleaned,
            ],
        )?;
        Ok(())
    }

    // The PostgreSQL control plane does not own the retired local execution
    // log. Chat replay, queue lifecycle and tool history live in the canonical
    // thread store, and the server schema deliberately never creates
    // `stream_events`. These legacy APIs stay callable for shared runtime
    // services but resolve to empty results instead of failing every append.
    fn get_max_stream_event_id_impl(&self, _session_id: &str) -> Result<i64> {
        Ok(0)
    }

    fn append_stream_event_impl(
        &self,
        _session_id: &str,
        _user_id: &str,
        _event_id: i64,
        _payload: &Value,
    ) -> Result<()> {
        Ok(())
    }

    fn load_stream_events_impl(
        &self,
        _session_id: &str,
        _after_event_id: i64,
        _limit: i64,
    ) -> Result<Vec<Value>> {
        Ok(Vec::new())
    }

    fn load_recent_stream_events_impl(&self, _session_id: &str, _limit: i64) -> Result<Vec<Value>> {
        Ok(Vec::new())
    }

    fn max_session_model_round_impl(&self, _session_id: &str, _user_round: i64) -> Result<i64> {
        Ok(0)
    }

    fn load_session_workflow_events_impl(
        &self,
        _session_id: &str,
        _from_user_round: i64,
        _to_user_round: i64,
    ) -> Result<Vec<Value>> {
        Ok(Vec::new())
    }

    fn load_session_workflow_events_page_impl(
        &self,
        _session_id: &str,
        _from_user_round: i64,
        _to_user_round: i64,
        _offset: i64,
        _limit: i64,
    ) -> Result<Vec<Value>> {
        Ok(Vec::new())
    }

    fn count_session_workflow_events_impl(
        &self,
        _session_id: &str,
        _from_user_round: i64,
        _to_user_round: i64,
    ) -> Result<i64> {
        Ok(0)
    }

    fn delete_stream_events_before_impl(&self, _before_time: f64) -> Result<i64> {
        Ok(0)
    }

    fn delete_stream_events_by_user_impl(&self, _user_id: &str) -> Result<i64> {
        Ok(0)
    }

    fn delete_stream_events_by_session_impl(&self, _session_id: &str) -> Result<i64> {
        Ok(0)
    }

    fn delete_stream_events_by_round_impl(
        &self,
        _session_id: &str,
        _user_round: i64,
        _event_types: &[&str],
    ) -> Result<i64> {
        Ok(0)
    }
}
