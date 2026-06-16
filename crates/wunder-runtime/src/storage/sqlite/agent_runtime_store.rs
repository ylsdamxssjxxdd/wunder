use super::SqliteStorage;
use crate::storage::{
    AgentTaskRecord, AgentThreadRecord, StorageLifecycle, UpdateAgentTaskStatusParams,
};
use anyhow::Result;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

pub(super) trait SqliteAgentRuntimeStorage {
    fn upsert_agent_thread_impl(&self, record: &AgentThreadRecord) -> Result<()>;
    fn get_agent_thread_impl(
        &self,
        user_id: &str,
        agent_id: &str,
    ) -> Result<Option<AgentThreadRecord>>;
    fn delete_agent_thread_impl(&self, user_id: &str, agent_id: &str) -> Result<i64>;
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
    fn delete_stream_events_before_impl(&self, before_time: f64) -> Result<i64>;
    fn delete_stream_events_by_user_impl(&self, user_id: &str) -> Result<i64>;
    fn delete_stream_events_by_session_impl(&self, session_id: &str) -> Result<i64>;
}

impl SqliteAgentRuntimeStorage for SqliteStorage {
    fn upsert_agent_thread_impl(&self, record: &AgentThreadRecord) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = record.user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(());
        }
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO agent_threads (thread_id, user_id, agent_id, session_id, status, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(user_id, agent_id) DO UPDATE SET thread_id = excluded.thread_id, session_id = excluded.session_id, \
             status = excluded.status, updated_at = excluded.updated_at",
            params![
                record.thread_id,
                cleaned_user,
                record.agent_id.trim(),
                record.session_id,
                record.status,
                record.created_at,
                record.updated_at
            ],
        )?;
        Ok(())
    }

    fn get_agent_thread_impl(
        &self,
        user_id: &str,
        agent_id: &str,
    ) -> Result<Option<AgentThreadRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(None);
        }
        let cleaned_agent = agent_id.trim();
        let conn = self.open()?;
        let record = conn
            .query_row(
                "SELECT thread_id, user_id, agent_id, session_id, status, created_at, updated_at \
                 FROM agent_threads WHERE user_id = ? AND agent_id = ? LIMIT 1",
                params![cleaned_user, cleaned_agent],
                |row| {
                    Ok(AgentThreadRecord {
                        thread_id: row.get(0)?,
                        user_id: row.get(1)?,
                        agent_id: row.get(2)?,
                        session_id: row.get(3)?,
                        status: row.get(4)?,
                        created_at: row.get(5)?,
                        updated_at: row.get(6)?,
                    })
                },
            )
            .optional()?;
        Ok(record)
    }

    fn delete_agent_thread_impl(&self, user_id: &str, agent_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(0);
        }
        let cleaned_agent = agent_id.trim();
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM agent_threads WHERE user_id = ? AND agent_id = ?",
            params![cleaned_user, cleaned_agent],
        )?;
        Ok(affected as i64)
    }

    fn insert_agent_task_impl(&self, record: &AgentTaskRecord) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = record.user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(());
        }
        let payload =
            serde_json::to_string(&record.request_payload).unwrap_or_else(|_| "{}".to_string());
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO agent_tasks (task_id, thread_id, user_id, agent_id, session_id, status, request_payload, request_id, retry_count, retry_at, created_at, updated_at, started_at, finished_at, last_error) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(task_id) DO UPDATE SET status = excluded.status, request_payload = excluded.request_payload, \
             request_id = excluded.request_id, retry_count = excluded.retry_count, retry_at = excluded.retry_at, updated_at = excluded.updated_at, \
             started_at = excluded.started_at, finished_at = excluded.finished_at, last_error = excluded.last_error",
            params![
                record.task_id,
                record.thread_id,
                cleaned_user,
                record.agent_id.trim(),
                record.session_id,
                record.status,
                payload,
                record.request_id.as_deref(),
                record.retry_count,
                record.retry_at,
                record.created_at,
                record.updated_at,
                record.started_at,
                record.finished_at,
                record.last_error.as_deref()
            ],
        )?;
        Ok(())
    }

    fn get_agent_task_impl(&self, task_id: &str) -> Result<Option<AgentTaskRecord>> {
        self.ensure_initialized()?;
        let cleaned = task_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let record = conn
            .query_row(
                "SELECT task_id, thread_id, user_id, agent_id, session_id, status, request_payload, request_id, retry_count, retry_at, created_at, updated_at, started_at, finished_at, last_error \
                 FROM agent_tasks WHERE task_id = ? LIMIT 1",
                params![cleaned],
                |row| {
                    let raw_payload: String = row.get(6)?;
                    let payload = serde_json::from_str(&raw_payload).unwrap_or(Value::Null);
                    Ok(AgentTaskRecord {
                        task_id: row.get(0)?,
                        thread_id: row.get(1)?,
                        user_id: row.get(2)?,
                        agent_id: row.get(3)?,
                        session_id: row.get(4)?,
                        status: row.get(5)?,
                        request_payload: payload,
                        request_id: row.get(7)?,
                        retry_count: row.get(8)?,
                        retry_at: row.get(9)?,
                        created_at: row.get(10)?,
                        updated_at: row.get(11)?,
                        started_at: row.get(12)?,
                        finished_at: row.get(13)?,
                        last_error: row.get(14)?,
                    })
                },
            )
            .optional()?;
        Ok(record)
    }

    fn list_pending_agent_tasks_impl(&self, limit: i64) -> Result<Vec<AgentTaskRecord>> {
        self.ensure_initialized()?;
        let now = Self::now_ts();
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT task_id, thread_id, user_id, agent_id, session_id, status, request_payload, request_id, retry_count, retry_at, created_at, updated_at, started_at, finished_at, last_error \
             FROM agent_tasks WHERE (status = 'pending' OR status = 'retry') AND retry_at <= ? \
             ORDER BY retry_at ASC, created_at ASC LIMIT ?",
        )?;
        let rows = stmt
            .query_map(params![now, limit.max(1)], |row| {
                let raw_payload: String = row.get(6)?;
                let payload = serde_json::from_str(&raw_payload).unwrap_or(Value::Null);
                Ok(AgentTaskRecord {
                    task_id: row.get(0)?,
                    thread_id: row.get(1)?,
                    user_id: row.get(2)?,
                    agent_id: row.get(3)?,
                    session_id: row.get(4)?,
                    status: row.get(5)?,
                    request_payload: payload,
                    request_id: row.get(7)?,
                    retry_count: row.get(8)?,
                    retry_at: row.get(9)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                    started_at: row.get(12)?,
                    finished_at: row.get(13)?,
                    last_error: row.get(14)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn count_pending_agent_tasks_impl(&self) -> Result<i64> {
        self.ensure_initialized()?;
        let now = Self::now_ts();
        let conn = self.open()?;
        let total = conn.query_row(
            "SELECT COUNT(*) FROM agent_tasks WHERE (status = 'pending' OR status = 'retry') AND retry_at <= ?",
            params![now],
            |row| row.get(0),
        )?;
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
        let conn = self.open()?;
        let total = conn.query_row(
            "SELECT COUNT(*) FROM agent_tasks \
             WHERE (status = 'pending' OR status = 'retry') AND retry_at <= ? \
               AND (retry_at < ? OR (retry_at = ? AND created_at < ?) OR (retry_at = ? AND created_at = ? AND task_id < ?))",
            params![now, retry_at, retry_at, created_at, retry_at, created_at, task_id],
            |row| row.get(0),
        )?;
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
        let conn = self.open()?;
        let (query, params): (String, Vec<rusqlite::types::Value>) = if let Some(status) =
            status.filter(|value| !value.trim().is_empty())
        {
            (
                "SELECT task_id, thread_id, user_id, agent_id, session_id, status, request_payload, request_id, retry_count, retry_at, created_at, updated_at, started_at, finished_at, last_error \
                 FROM agent_tasks WHERE thread_id = ? AND status = ? ORDER BY created_at DESC LIMIT ?"
                    .to_string(),
                vec![
                    rusqlite::types::Value::from(cleaned_thread.to_string()),
                    rusqlite::types::Value::from(status.trim().to_string()),
                    rusqlite::types::Value::from(limit.max(1)),
                ],
            )
        } else {
            (
                "SELECT task_id, thread_id, user_id, agent_id, session_id, status, request_payload, request_id, retry_count, retry_at, created_at, updated_at, started_at, finished_at, last_error \
                 FROM agent_tasks WHERE thread_id = ? ORDER BY created_at DESC LIMIT ?"
                    .to_string(),
                vec![
                    rusqlite::types::Value::from(cleaned_thread.to_string()),
                    rusqlite::types::Value::from(limit.max(1)),
                ],
            )
        };
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params.iter()), |row| {
                let raw_payload: String = row.get(6)?;
                let payload = serde_json::from_str(&raw_payload).unwrap_or(Value::Null);
                Ok(AgentTaskRecord {
                    task_id: row.get(0)?,
                    thread_id: row.get(1)?,
                    user_id: row.get(2)?,
                    agent_id: row.get(3)?,
                    session_id: row.get(4)?,
                    status: row.get(5)?,
                    request_payload: payload,
                    request_id: row.get(7)?,
                    retry_count: row.get(8)?,
                    retry_at: row.get(9)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                    started_at: row.get(12)?,
                    finished_at: row.get(13)?,
                    last_error: row.get(14)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn update_agent_task_status_impl(&self, params: UpdateAgentTaskStatusParams<'_>) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = params.task_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let conn = self.open()?;
        conn.execute(
            "UPDATE agent_tasks SET status = ?, retry_count = ?, retry_at = ?, started_at = ?, finished_at = ?, last_error = ?, updated_at = ? WHERE task_id = ?",
            params![
                params.status,
                params.retry_count,
                params.retry_at,
                params.started_at,
                params.finished_at,
                params.last_error,
                params.updated_at,
                cleaned
            ],
        )?;
        Ok(())
    }

    fn get_max_stream_event_id_impl(&self, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let value: Option<i64> = conn.query_row(
            "SELECT MAX(event_id) FROM stream_events WHERE session_id = ?",
            params![cleaned_session],
            |row| row.get::<_, Option<i64>>(0),
        )?;
        Ok(value.unwrap_or(0))
    }

    fn append_stream_event_impl(
        &self,
        session_id: &str,
        user_id: &str,
        event_id: i64,
        payload: &Value,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_session = session_id.trim();
        let cleaned_user = user_id.trim();
        if cleaned_session.is_empty() || cleaned_user.is_empty() {
            return Ok(());
        }
        let now = Self::now_ts();
        let payload_text = Self::json_to_string(payload);
        let conn = self.open()?;
        conn.execute(
            "INSERT OR REPLACE INTO stream_events (session_id, event_id, user_id, payload, created_time) VALUES (?, ?, ?, ?, ?)",
            params![cleaned_session, event_id, cleaned_user, payload_text, now],
        )?;
        Ok(())
    }

    fn load_stream_events_impl(
        &self,
        session_id: &str,
        after_event_id: i64,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() || limit <= 0 {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT event_id, payload FROM stream_events WHERE session_id = ? AND event_id > ? ORDER BY event_id ASC LIMIT ?",
        )?;
        let rows = stmt
            .query_map(params![cleaned_session, after_event_id, limit], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<(i64, String)>, _>>()?;
        let mut records = Vec::new();
        for (event_id, payload) in rows {
            if let Some(mut value) = Self::json_from_str(&payload) {
                if let Value::Object(ref mut map) = value {
                    map.insert("event_id".to_string(), json!(event_id));
                    map.insert("event_seq".to_string(), json!(event_id));
                    records.push(value);
                } else {
                    records.push(
                        json!({ "event_id": event_id, "event_seq": event_id, "data": value }),
                    );
                }
            }
        }
        Ok(records)
    }

    fn load_recent_stream_events_impl(&self, session_id: &str, limit: i64) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() || limit <= 0 {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT event_id, payload FROM stream_events WHERE session_id = ? ORDER BY event_id DESC LIMIT ?",
        )?;
        let mut rows = stmt
            .query_map(params![cleaned_session, limit], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<(i64, String)>, _>>()?;
        rows.reverse();
        let mut records = Vec::new();
        for (event_id, payload) in rows {
            if let Some(mut value) = Self::json_from_str(&payload) {
                if let Value::Object(ref mut map) = value {
                    map.insert("event_id".to_string(), json!(event_id));
                    map.insert("event_seq".to_string(), json!(event_id));
                    records.push(value);
                } else {
                    records.push(
                        json!({ "event_id": event_id, "event_seq": event_id, "data": value }),
                    );
                }
            }
        }
        Ok(records)
    }

    fn delete_stream_events_before_impl(&self, before_time: f64) -> Result<i64> {
        self.ensure_initialized()?;
        if before_time <= 0.0 {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM stream_events WHERE created_time < ?",
            params![before_time],
        )?;
        Ok(affected as i64)
    }

    fn delete_stream_events_by_user_impl(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM stream_events WHERE user_id = ?",
            params![cleaned_user],
        )?;
        Ok(affected as i64)
    }

    fn delete_stream_events_by_session_impl(&self, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM stream_events WHERE session_id = ?",
            params![cleaned_session],
        )?;
        Ok(affected as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteStorage;
    use crate::storage::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn build_storage() -> (SqliteStorage, tempfile::TempDir) {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("agent-runtime-store.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize sqlite");
        (storage, dir)
    }

    fn task(task_id: &str, retry_at: f64, created_at: f64) -> AgentTaskRecord {
        AgentTaskRecord {
            task_id: task_id.to_string(),
            thread_id: "thread-1".to_string(),
            user_id: "user-1".to_string(),
            agent_id: "agent-1".to_string(),
            session_id: "session-1".to_string(),
            status: "pending".to_string(),
            request_payload: json!({ "kind": "sample", "id": task_id }),
            request_id: Some(format!("request-{task_id}")),
            retry_count: 0,
            retry_at,
            created_at,
            updated_at: created_at,
            started_at: None,
            finished_at: None,
            last_error: None,
        }
    }

    #[test]
    fn agent_runtime_task_queue_roundtrip_preserves_order_and_status() {
        let (storage, _dir) = build_storage();
        let thread = AgentThreadRecord {
            thread_id: "thread-1".to_string(),
            user_id: "user-1".to_string(),
            agent_id: "agent-1".to_string(),
            session_id: "session-1".to_string(),
            status: "active".to_string(),
            created_at: 1.0,
            updated_at: 1.0,
        };

        storage.upsert_agent_thread(&thread).expect("upsert thread");
        assert_eq!(
            storage
                .get_agent_thread("user-1", "agent-1")
                .expect("get thread")
                .map(|record| record.thread_id),
            Some("thread-1".to_string())
        );

        storage
            .insert_agent_task(&task("task-2", 2.0, 2.0))
            .expect("insert second task");
        storage
            .insert_agent_task(&task("task-1", 1.0, 1.0))
            .expect("insert first task");

        let pending = storage
            .list_pending_agent_tasks(8)
            .expect("list pending tasks");
        assert_eq!(
            pending
                .iter()
                .map(|record| record.task_id.as_str())
                .collect::<Vec<_>>(),
            vec!["task-1", "task-2"]
        );
        assert_eq!(storage.count_pending_agent_tasks().expect("count"), 2);
        assert_eq!(
            storage
                .count_pending_agent_tasks_ahead(2.0, 2.0, "task-2")
                .expect("count ahead"),
            1
        );

        storage
            .update_agent_task_status(UpdateAgentTaskStatusParams {
                task_id: "task-1",
                status: "running",
                retry_count: 1,
                retry_at: 3.0,
                started_at: Some(4.0),
                finished_at: None,
                last_error: None,
                updated_at: 4.0,
            })
            .expect("update task");

        let running = storage
            .list_agent_tasks_by_thread("thread-1", Some("running"), 8)
            .expect("list by thread");
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].task_id, "task-1");
        assert_eq!(running[0].retry_count, 1);
        assert_eq!(running[0].started_at, Some(4.0));
    }
}
