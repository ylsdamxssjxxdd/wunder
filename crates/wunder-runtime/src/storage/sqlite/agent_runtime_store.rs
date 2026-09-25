use super::SqliteStorage;
use crate::storage::{AgentTaskRecord, StorageLifecycle, UpdateAgentTaskStatusParams};
use anyhow::Result;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

pub(super) trait SqliteAgentRuntimeStorage {
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
}

impl SqliteAgentRuntimeStorage for SqliteStorage {
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
             ORDER BY priority DESC, retry_at ASC, created_at ASC, task_id ASC LIMIT ?",
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
               AND (priority > COALESCE((SELECT priority FROM agent_tasks WHERE task_id = ?), 0) OR (priority = COALESCE((SELECT priority FROM agent_tasks WHERE task_id = ?), 0) AND (retry_at < ? OR (retry_at = ? AND created_at < ?) OR (retry_at = ? AND created_at = ? AND task_id < ?))))",
            params![now, task_id, task_id, retry_at, retry_at, created_at, retry_at, created_at, task_id],
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
        // Terminal states are monotonic. A late completion/error callback must not
        // resurrect a cancelled or already finished task.
        let allowed_sources = match params.status {
            "running" => "('pending','retry')",
            "retry" => "('pending','retry','running')",
            "success" | "failed" | "dead" | "cancelled" => "('pending','retry','running')",
            _ => return Ok(()),
        };
        let conn = self.open()?;
        conn.execute(
            &format!("UPDATE agent_tasks SET status = ?, retry_count = ?, retry_at = ?, started_at = ?, finished_at = ?, last_error = ?, updated_at = ? WHERE task_id = ? AND status IN {allowed_sources}"),
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
        let event_type = stream_event_type(payload);
        let user_round = stream_event_user_round(payload);
        let conn = self.open()?;
        conn.execute(
            "INSERT OR REPLACE INTO stream_events (session_id, event_id, user_id, event_type, user_round, payload, created_time) VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![cleaned_session, event_id, cleaned_user, event_type, user_round, payload_text, now],
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

    fn load_session_workflow_events_impl(
        &self,
        session_id: &str,
        from_user_round: i64,
        to_user_round: i64,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() || from_user_round <= 0 || to_user_round < from_user_round {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT event_id, payload FROM stream_events \
             WHERE session_id = ? AND user_round BETWEEN ? AND ? \
             AND event_type NOT IN ('llm_output_delta', 'llm_output', 'final', 'thread_closed') \
             ORDER BY event_id ASC",
        )?;
        let rows = stmt
            .query_map(
                params![cleaned_session, from_user_round, to_user_round],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )?
            .collect::<std::result::Result<Vec<(i64, String)>, _>>()?;
        Ok(stream_event_rows_to_values(rows))
    }

    fn load_session_workflow_events_page_impl(
        &self,
        session_id: &str,
        from_user_round: i64,
        to_user_round: i64,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty()
            || from_user_round <= 0
            || to_user_round < from_user_round
            || offset < 0
            || limit <= 0
        {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT event_id, payload FROM stream_events \
             WHERE session_id = ? AND user_round BETWEEN ? AND ? \
             AND event_type NOT IN ('llm_output_delta', 'llm_output', 'final', 'thread_closed') \
             ORDER BY event_id ASC LIMIT ? OFFSET ?",
        )?;
        let rows = stmt
            .query_map(
                params![
                    cleaned_session,
                    from_user_round,
                    to_user_round,
                    limit,
                    offset
                ],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )?
            .collect::<std::result::Result<Vec<(i64, String)>, _>>()?;
        Ok(stream_event_rows_to_values(rows))
    }

    fn count_session_workflow_events_impl(
        &self,
        session_id: &str,
        from_user_round: i64,
        to_user_round: i64,
    ) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() || from_user_round <= 0 || to_user_round < from_user_round {
            return Ok(0);
        }
        let conn = self.open()?;
        let count = conn.query_row(
            "SELECT COUNT(*) FROM stream_events \
             WHERE session_id = ? AND user_round BETWEEN ? AND ? \
             AND event_type NOT IN ('llm_output_delta', 'llm_output', 'final', 'thread_closed')",
            params![cleaned_session, from_user_round, to_user_round],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(count)
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

fn stream_event_type(payload: &Value) -> String {
    payload
        .get("event")
        .or_else(|| payload.get("event_type"))
        .or_else(|| payload.get("type"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn stream_event_user_round(payload: &Value) -> Option<i64> {
    let outer = payload.get("data").unwrap_or(payload);
    let data = outer.get("data").unwrap_or(outer);
    data.get("user_round")
        .or_else(|| data.get("userRound"))
        .or_else(|| data.get("round"))
        .and_then(|value| match value {
            Value::Number(number) => number.as_i64(),
            Value::String(text) => text.trim().parse::<i64>().ok(),
            _ => None,
        })
        .filter(|value| *value > 0)
}

fn stream_event_rows_to_values(rows: Vec<(i64, String)>) -> Vec<Value> {
    let mut records = Vec::new();
    for (event_id, payload) in rows {
        if let Some(mut value) = SqliteStorage::json_from_str(&payload) {
            if let Value::Object(ref mut map) = value {
                map.insert("event_id".to_string(), json!(event_id));
                map.insert("event_seq".to_string(), json!(event_id));
                records.push(value);
            } else {
                records.push(json!({ "event_id": event_id, "event_seq": event_id, "data": value }));
            }
        }
    }
    records
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
    fn workflow_event_query_filters_rounds_and_output_payloads() {
        let (storage, _dir) = build_storage();
        let records = [
            (
                1,
                json!({ "event": "tool_call", "data": { "data": { "user_round": 2, "tool": "tool_a" } } }),
            ),
            (
                2,
                json!({ "event": "llm_output_delta", "data": { "data": { "user_round": 2, "delta": "hidden" } } }),
            ),
            (
                3,
                json!({ "event": "tool_result", "data": { "data": { "user_round": 3, "tool": "tool_a" } } }),
            ),
            (
                4,
                json!({ "event": "tool_result", "data": { "data": { "user_round": 4, "tool": "tool_b" } } }),
            ),
            (
                5,
                json!({ "event": "turn_terminal", "data": { "data": { "user_round": 3, "status": "completed" } } }),
            ),
            (
                6,
                json!({ "event": "thread_status", "data": { "data": { "user_round": 3, "status": "idle" } } }),
            ),
        ];
        for (event_id, payload) in records {
            storage
                .append_stream_event("session-1", "user-1", event_id, &payload)
                .expect("append stream event");
        }

        let events = storage
            .load_session_workflow_events("session-1", 2, 3)
            .expect("load workflow events");
        assert_eq!(events.len(), 4);
        assert_eq!(events[0]["event"], json!("tool_call"));
        assert_eq!(events[0]["event_id"], json!(1));
        assert_eq!(events[1]["event"], json!("tool_result"));
        assert_eq!(events[1]["event_id"], json!(3));
        assert_eq!(events[2]["event"], json!("turn_terminal"));
        assert_eq!(events[2]["event_id"], json!(5));
        assert_eq!(events[3]["event"], json!("thread_status"));
        assert_eq!(events[3]["event_id"], json!(6));
    }

    #[test]
    fn workflow_event_page_uses_stable_offset_and_limit() {
        let (storage, _dir) = build_storage();
        for event_id in 1..=4 {
            storage
                .append_stream_event(
                    "session-1",
                    "user-1",
                    event_id,
                    &json!({
                        "event": "tool_call",
                        "data": { "data": { "user_round": 1, "tool": "tool_a" } }
                    }),
                )
                .expect("append workflow event");
        }

        let events = storage
            .load_session_workflow_events_page("session-1", 1, 1, 1, 2)
            .expect("load workflow event page");

        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["event_id"], json!(2));
        assert_eq!(events[1]["event_id"], json!(3));
    }
}
