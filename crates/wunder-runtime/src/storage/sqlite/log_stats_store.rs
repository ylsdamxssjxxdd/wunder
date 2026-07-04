use super::SqliteStorage;
use crate::storage::{StorageLifecycle, TOOL_LOG_EXCLUDED_NAMES, TOOL_LOG_SKILL_READ_MARKER};
use anyhow::Result;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter};
use serde_json::{json, Value};
use std::collections::HashMap;

pub(super) trait SqliteLogStatsStorage {
    fn get_user_chat_stats_impl(&self) -> Result<HashMap<String, HashMap<String, i64>>>;
    fn get_user_tool_stats_impl(&self) -> Result<HashMap<String, HashMap<String, i64>>>;
    fn get_tool_usage_stats_impl(
        &self,
        since_time: Option<f64>,
        until_time: Option<f64>,
    ) -> Result<HashMap<String, i64>>;
    fn get_tool_session_usage_impl(
        &self,
        tool: &str,
        since_time: Option<f64>,
        until_time: Option<f64>,
    ) -> Result<Vec<HashMap<String, Value>>>;
    fn get_log_usage_impl(&self) -> Result<u64>;
    fn delete_logs_by_time_range_impl(
        &self,
        start_time: f64,
        end_time: f64,
    ) -> Result<HashMap<String, i64>>;
    fn delete_chat_history_impl(&self, user_id: &str) -> Result<i64>;
    fn delete_chat_history_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn delete_tool_logs_impl(&self, user_id: &str) -> Result<i64>;
    fn delete_tool_logs_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn delete_artifact_logs_impl(&self, user_id: &str) -> Result<i64>;
    fn delete_artifact_logs_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64>;
}

fn append_tool_log_exclusions(filters: &mut Vec<String>, params_list: &mut Vec<SqlValue>) {
    if !TOOL_LOG_EXCLUDED_NAMES.is_empty() {
        let placeholders = vec!["?"; TOOL_LOG_EXCLUDED_NAMES.len()].join(", ");
        filters.push(format!("tool NOT IN ({placeholders})"));
        for name in TOOL_LOG_EXCLUDED_NAMES {
            params_list.push(SqlValue::from(name.to_string()));
        }
    }
    let marker = format!("%{TOOL_LOG_SKILL_READ_MARKER}%");
    filters.push("(data IS NULL OR data NOT LIKE ?)".to_string());
    params_list.push(SqlValue::from(marker));
}

impl SqliteLogStatsStorage for SqliteStorage {
    fn get_user_chat_stats_impl(&self) -> Result<HashMap<String, HashMap<String, i64>>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT user_id, COUNT(*) as chat_records, MAX(created_time) as last_time FROM chat_history GROUP BY user_id",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, f64>(2).unwrap_or(0.0),
                ))
            })?
            .collect::<std::result::Result<Vec<(String, i64, f64)>, _>>()?;
        let mut stats = HashMap::new();
        for (user_id, count, last_time) in rows {
            let cleaned = user_id.trim();
            if cleaned.is_empty() {
                continue;
            }
            let mut entry = HashMap::new();
            entry.insert("chat_records".to_string(), count);
            entry.insert("last_time".to_string(), last_time.floor() as i64);
            stats.insert(cleaned.to_string(), entry);
        }
        Ok(stats)
    }

    fn get_user_tool_stats_impl(&self) -> Result<HashMap<String, HashMap<String, i64>>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut query = String::from(
            "SELECT user_id, COUNT(*) as tool_records, MAX(created_time) as last_time FROM tool_logs",
        );
        let mut filters = Vec::new();
        let mut params_list: Vec<SqlValue> = Vec::new();
        append_tool_log_exclusions(&mut filters, &mut params_list);
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" GROUP BY user_id");
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt
            .query_map(params_from_iter(params_list.iter()), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, f64>(2).unwrap_or(0.0),
                ))
            })?
            .collect::<std::result::Result<Vec<(String, i64, f64)>, _>>()?;
        let mut stats = HashMap::new();
        for (user_id, count, last_time) in rows {
            let cleaned = user_id.trim();
            if cleaned.is_empty() {
                continue;
            }
            let mut entry = HashMap::new();
            entry.insert("tool_records".to_string(), count);
            entry.insert("last_time".to_string(), last_time.floor() as i64);
            stats.insert(cleaned.to_string(), entry);
        }
        Ok(stats)
    }

    fn get_tool_usage_stats_impl(
        &self,
        since_time: Option<f64>,
        until_time: Option<f64>,
    ) -> Result<HashMap<String, i64>> {
        self.ensure_initialized()?;
        let mut query = String::from("SELECT tool, COUNT(*) as tool_records FROM tool_logs");
        let mut params_list: Vec<SqlValue> = Vec::new();
        let mut filters = Vec::new();
        if let Some(since) = since_time.filter(|value| *value > 0.0) {
            filters.push("created_time >= ?".to_string());
            params_list.push(SqlValue::from(since));
        }
        if let Some(until) = until_time.filter(|value| *value > 0.0) {
            filters.push("created_time <= ?".to_string());
            params_list.push(SqlValue::from(until));
        }
        append_tool_log_exclusions(&mut filters, &mut params_list);
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" GROUP BY tool ORDER BY tool_records DESC");
        let conn = self.open()?;
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt
            .query_map(params_from_iter(params_list.iter()), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<Vec<(String, i64)>, _>>()?;
        let mut stats = HashMap::new();
        for (tool, count) in rows {
            let cleaned = tool.trim();
            if cleaned.is_empty() {
                continue;
            }
            stats.insert(cleaned.to_string(), count);
        }
        Ok(stats)
    }

    fn get_tool_session_usage_impl(
        &self,
        tool: &str,
        since_time: Option<f64>,
        until_time: Option<f64>,
    ) -> Result<Vec<HashMap<String, Value>>> {
        self.ensure_initialized()?;
        let cleaned = tool.trim();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let mut query = String::from(
            "SELECT session_id, user_id, COUNT(*) as tool_calls, MAX(created_time) as last_time FROM tool_logs WHERE tool = ?",
        );
        let mut params_list: Vec<SqlValue> = vec![SqlValue::Text(cleaned.to_string())];
        let mut filters = Vec::new();
        if let Some(since) = since_time.filter(|value| *value > 0.0) {
            filters.push("created_time >= ?".to_string());
            params_list.push(SqlValue::from(since));
        }
        if let Some(until) = until_time.filter(|value| *value > 0.0) {
            filters.push("created_time <= ?".to_string());
            params_list.push(SqlValue::from(until));
        }
        append_tool_log_exclusions(&mut filters, &mut params_list);
        if !filters.is_empty() {
            query.push_str(" AND ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" GROUP BY session_id, user_id ORDER BY last_time DESC");
        let conn = self.open()?;
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt
            .query_map(params_from_iter(params_list.iter()), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, f64>(3).unwrap_or(0.0),
                ))
            })?
            .collect::<std::result::Result<Vec<(String, String, i64, f64)>, _>>()?;
        let mut sessions = Vec::new();
        for (session_id, user_id, tool_calls, last_time) in rows {
            let cleaned_session = session_id.trim();
            if cleaned_session.is_empty() {
                continue;
            }
            let mut entry = HashMap::new();
            entry.insert("session_id".to_string(), json!(cleaned_session));
            entry.insert("user_id".to_string(), json!(user_id.trim()));
            entry.insert("tool_calls".to_string(), json!(tool_calls));
            entry.insert("last_time".to_string(), json!(last_time));
            sessions.push(entry);
        }
        Ok(sessions)
    }

    fn get_log_usage_impl(&self) -> Result<u64> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let dbstat_query = "SELECT COALESCE(SUM(pgsize), 0) \
            FROM dbstat WHERE name IN ( \
                'chat_history', \
                'model_context_entries', \
                'tool_logs', \
                'artifact_logs', \
                'monitor_sessions', \
                'stream_events', \
                'memory_task_logs' \
            )";
        if let Ok(total) = conn.query_row(dbstat_query, [], |row| row.get::<_, i64>(0)) {
            return Ok(total.max(0) as u64);
        }
        let total: i64 = conn.query_row(
            "SELECT \
            (SELECT COALESCE(SUM(length(CAST(payload AS BLOB))), 0) FROM chat_history) + \
            (SELECT COALESCE(SUM(length(CAST(payload AS BLOB))), 0) FROM model_context_entries) + \
            (SELECT COALESCE(SUM(length(CAST(payload AS BLOB))), 0) FROM tool_logs) + \
            (SELECT COALESCE(SUM(length(CAST(payload AS BLOB))), 0) FROM artifact_logs) + \
            (SELECT COALESCE(SUM(length(CAST(payload AS BLOB))), 0) FROM monitor_sessions) + \
            (SELECT COALESCE(SUM(length(CAST(payload AS BLOB))), 0) FROM stream_events) + \
            (SELECT COALESCE(SUM( \
                COALESCE(length(CAST(request_payload AS BLOB)), 0) + \
                COALESCE(length(CAST(result AS BLOB)), 0) + \
                COALESCE(length(CAST(error AS BLOB)), 0) \
            ), 0) FROM memory_task_logs)",
            [],
            |row| row.get(0),
        )?;
        Ok(total.max(0) as u64)
    }

    fn delete_logs_by_time_range_impl(
        &self,
        start_time: f64,
        end_time: f64,
    ) -> Result<HashMap<String, i64>> {
        self.ensure_initialized()?;
        let start = start_time.min(end_time);
        let end = start_time.max(end_time);
        if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start {
            return Ok(HashMap::new());
        }
        let conn = self.open()?;
        let delete_range = |table: &str, time_field: &str| -> Result<i64> {
            let sql = format!("DELETE FROM {table} WHERE {time_field} >= ? AND {time_field} <= ?");
            Ok(conn.execute(&sql, params![start, end])? as i64)
        };
        let mut results = HashMap::new();
        results.insert(
            "chat_history".to_string(),
            delete_range("chat_history", "created_time")?,
        );
        results.insert(
            "model_context_entries".to_string(),
            delete_range("model_context_entries", "created_time")?,
        );
        results.insert(
            "tool_logs".to_string(),
            delete_range("tool_logs", "created_time")?,
        );
        results.insert(
            "artifact_logs".to_string(),
            delete_range("artifact_logs", "created_time")?,
        );
        results.insert(
            "monitor_sessions".to_string(),
            delete_range("monitor_sessions", "COALESCE(updated_time, 0)")?,
        );
        results.insert(
            "stream_events".to_string(),
            delete_range("stream_events", "created_time")?,
        );
        results.insert(
            "memory_task_logs".to_string(),
            delete_range("memory_task_logs", "updated_time")?,
        );
        Ok(results)
    }

    fn delete_chat_history_impl(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM chat_history WHERE user_id = ?",
            params![user_id],
        )?;
        let _ = conn.execute(
            "DELETE FROM model_context_entries WHERE user_id = ?",
            params![user_id],
        );
        Ok(affected as i64)
    }

    fn delete_chat_history_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM chat_history WHERE user_id = ? AND session_id = ?",
            params![cleaned_user, cleaned_session],
        )?;
        let _ = conn.execute(
            "DELETE FROM model_context_entries WHERE user_id = ? AND session_id = ?",
            params![cleaned_user, cleaned_session],
        );
        Ok(affected as i64)
    }

    fn delete_tool_logs_impl(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let affected = conn.execute("DELETE FROM tool_logs WHERE user_id = ?", params![user_id])?;
        Ok(affected as i64)
    }

    fn delete_tool_logs_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM tool_logs WHERE user_id = ? AND session_id = ?",
            params![cleaned_user, cleaned_session],
        )?;
        Ok(affected as i64)
    }

    fn delete_artifact_logs_impl(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM artifact_logs WHERE user_id = ?",
            params![user_id],
        )?;
        Ok(affected as i64)
    }

    fn delete_artifact_logs_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM artifact_logs WHERE user_id = ? AND session_id = ?",
            params![cleaned_user, cleaned_session],
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
        let db_path = dir.path().join("log-stats-store.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize sqlite");
        (storage, dir)
    }

    #[test]
    fn log_stats_store_counts_filters_usage_and_deletes() {
        let (storage, _dir) = build_storage();

        storage
            .append_chat(
                "user-a",
                &json!({
                    "session_id": "session-a",
                    "role": "user",
                    "content": "message-a"
                }),
            )
            .expect("append chat a");
        storage
            .append_chat(
                "user-a",
                &json!({
                    "session_id": "session-b",
                    "role": "assistant",
                    "content": "message-b"
                }),
            )
            .expect("append chat b");
        storage
            .append_model_context_entry(
                "user-a",
                "session-a",
                &json!({
                    "role": "user",
                    "content": "context-a"
                }),
            )
            .expect("append context");

        storage
            .append_tool_log(
                "user-a",
                &json!({
                    "session_id": "session-a",
                    "tool": "tool-a",
                    "ok": true,
                    "data": { "value": 1 }
                }),
            )
            .expect("append tool a");
        storage
            .append_tool_log(
                "user-a",
                &json!({
                    "session_id": "session-a",
                    "tool": "final_response",
                    "ok": true
                }),
            )
            .expect("append excluded tool");
        storage
            .append_tool_log(
                "user-a",
                &json!({
                    "session_id": "session-b",
                    "tool": "tool-b",
                    "ok": true,
                    "data": { "source": "skill_read" }
                }),
            )
            .expect("append skill read tool");
        storage
            .append_tool_log(
                "user-b",
                &json!({
                    "session_id": "session-c",
                    "tool": "tool-a",
                    "ok": true
                }),
            )
            .expect("append other user tool");

        storage
            .append_artifact_log(
                "user-a",
                &json!({
                    "session_id": "session-a",
                    "kind": "kind-a",
                    "name": "artifact-a"
                }),
            )
            .expect("append artifact a");
        storage
            .append_artifact_log(
                "user-a",
                &json!({
                    "session_id": "session-b",
                    "kind": "kind-b",
                    "name": "artifact-b"
                }),
            )
            .expect("append artifact b");

        let chat_stats = storage.get_user_chat_stats().expect("chat stats");
        assert_eq!(chat_stats["user-a"]["chat_records"], 2);

        let tool_stats = storage.get_user_tool_stats().expect("tool stats");
        assert_eq!(tool_stats["user-a"]["tool_records"], 1);
        assert_eq!(tool_stats["user-b"]["tool_records"], 1);

        let usage = storage
            .get_tool_usage_stats(None, None)
            .expect("tool usage stats");
        assert_eq!(usage.get("tool-a"), Some(&2));
        assert!(!usage.contains_key("tool-b"));
        assert!(!usage.contains_key("final_response"));

        let sessions = storage
            .get_tool_session_usage("tool-a", None, None)
            .expect("tool session usage");
        assert_eq!(sessions.len(), 2);
        assert!(sessions
            .iter()
            .any(|entry| entry.get("session_id") == Some(&json!("session-a"))));
        assert!(sessions
            .iter()
            .any(|entry| entry.get("session_id") == Some(&json!("session-c"))));
        assert!(storage.get_log_usage().expect("log usage") > 0);

        assert_eq!(
            storage
                .delete_chat_history_by_session("user-a", "session-a")
                .expect("delete chat by session"),
            1
        );
        assert!(storage
            .load_model_context_entries("user-a", "session-a", None)
            .expect("load context after delete")
            .is_empty());
        assert_eq!(
            storage
                .delete_chat_history("user-a")
                .expect("delete remaining chat"),
            1
        );
        assert_eq!(
            storage
                .delete_tool_logs_by_session("user-a", "session-a")
                .expect("delete tool by session"),
            2
        );
        assert_eq!(
            storage
                .delete_tool_logs("user-a")
                .expect("delete remaining tools"),
            1
        );
        assert_eq!(
            storage
                .delete_artifact_logs_by_session("user-a", "session-a")
                .expect("delete artifact by session"),
            1
        );
        assert_eq!(
            storage
                .delete_artifact_logs("user-a")
                .expect("delete remaining artifacts"),
            1
        );
    }

    #[test]
    fn log_stats_store_deletes_logs_by_time_range() {
        let (storage, _dir) = build_storage();
        let conn = storage.open().expect("open sqlite");
        conn.execute(
            "INSERT INTO chat_history (user_id, session_id, role, payload, created_time)
             VALUES (?, ?, ?, ?, ?)",
            ("user-a", "session-old", "user", "{}", 10.0),
        )
        .expect("insert old chat");
        conn.execute(
            "INSERT INTO chat_history (user_id, session_id, role, payload, created_time)
             VALUES (?, ?, ?, ?, ?)",
            ("user-a", "session-new", "user", "{}", 90.0),
        )
        .expect("insert new chat");
        conn.execute(
            "INSERT INTO tool_logs (user_id, session_id, tool, payload, created_time)
             VALUES (?, ?, ?, ?, ?)",
            ("user-a", "session-tool", "tool-a", "{}", 50.0),
        )
        .expect("insert tool");
        conn.execute(
            "INSERT INTO monitor_sessions (session_id, user_id, status, updated_time, payload)
             VALUES (?, ?, ?, ?, ?)",
            ("session-monitor", "user-a", "finished", 55.0, "{}"),
        )
        .expect("insert monitor");
        conn.execute(
            "INSERT INTO stream_events (session_id, event_id, user_id, payload, created_time)
             VALUES (?, ?, ?, ?, ?)",
            ("session-stream", 1_i64, "user-a", "{}", 60.0),
        )
        .expect("insert stream");
        conn.execute(
            "INSERT INTO memory_task_logs (task_id, user_id, session_id, status, updated_time)
             VALUES (?, ?, ?, ?, ?)",
            ("task-a", "user-a", "session-memory", "finished", 70.0),
        )
        .expect("insert memory task");
        drop(conn);

        let deleted = storage
            .delete_logs_by_time_range(40.0, 80.0)
            .expect("delete range");
        assert_eq!(deleted.get("tool_logs").copied(), Some(1));
        assert_eq!(deleted.get("monitor_sessions").copied(), Some(1));
        assert_eq!(deleted.get("stream_events").copied(), Some(1));
        assert_eq!(deleted.get("memory_task_logs").copied(), Some(1));

        let conn = storage.open().expect("reopen sqlite");
        let chat_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM chat_history", [], |row| row.get(0))
            .expect("count chat");
        assert_eq!(chat_count, 2);
        let tool_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tool_logs", [], |row| row.get(0))
            .expect("count tool");
        assert_eq!(tool_count, 0);
        let monitor_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM monitor_sessions", [], |row| {
                row.get(0)
            })
            .expect("count monitor");
        assert_eq!(monitor_count, 0);
    }
}
