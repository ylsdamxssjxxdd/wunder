use super::SqliteStorage;
use crate::storage::{LogUsage, StorageLifecycle};
use anyhow::Result;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;

pub(super) trait SqliteLogStatsStorage {
    fn get_user_chat_stats_impl(&self) -> Result<HashMap<String, HashMap<String, i64>>>;
    fn get_user_tool_stats_impl(&self) -> Result<HashMap<String, HashMap<String, i64>>>;
    fn get_user_usage_summary_impl(&self) -> Result<HashMap<String, (i64, i64)>>;
    fn reconcile_user_usage_impl(&self, full: bool) -> Result<()>;
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
    fn get_log_usage_impl(&self) -> Result<LogUsage>;
    fn delete_logs_by_time_range_impl(
        &self,
        start_time: f64,
        end_time: f64,
    ) -> Result<HashMap<String, i64>>;
    fn delete_thread_logs_by_user_impl(&self, user_id: &str) -> Result<i64>;
    fn delete_tool_logs_impl(&self, user_id: &str) -> Result<i64>;
    fn delete_tool_logs_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn delete_artifact_logs_impl(&self, user_id: &str) -> Result<i64>;
    fn delete_artifact_logs_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn mark_deleted_session_log_grace_impl(
        &self,
        user_id: &str,
        session_id: &str,
        deleted_at: f64,
    ) -> Result<()>;
    fn list_expired_deleted_session_log_grace_impl(
        &self,
        cutoff: f64,
        limit: i64,
    ) -> Result<Vec<(String, String)>>;
    fn delete_deleted_session_log_grace_impl(&self, user_id: &str, session_id: &str) -> Result<()>;
}

impl SqliteLogStatsStorage for SqliteStorage {
    fn get_user_chat_stats_impl(&self) -> Result<HashMap<String, HashMap<String, i64>>> {
        self.projected_chat_stats()
    }

    fn get_user_tool_stats_impl(&self) -> Result<HashMap<String, HashMap<String, i64>>> {
        self.ensure_initialized()?;
        let _ = self.runtime_logs()?;
        let conn = self.open_logs()?;
        let mut query = String::from(
            "SELECT user_id, COUNT(*) as tool_records, MAX(created_time) as last_time FROM runtime_tool_metrics",
        );
        let mut filters = Vec::new();
        filters.push("excluded = 0".to_owned());
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" GROUP BY user_id");
        let mut stmt = conn.prepare(&query)?;
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
            entry.insert("tool_records".to_string(), count);
            entry.insert("last_time".to_string(), last_time.floor() as i64);
            stats.insert(cleaned.to_string(), entry);
        }
        Ok(stats)
    }

    fn get_user_usage_summary_impl(&self) -> Result<HashMap<String, (i64, i64)>> {
        let mut output = HashMap::new();
        for (user, stats) in self.get_user_chat_stats_impl()? {
            output.insert(user, (*stats.get("chat_records").unwrap_or(&0), 0));
        }
        for (user, stats) in self.get_user_tool_stats_impl()? {
            output.entry(user).or_insert((0, 0)).1 = *stats.get("tool_records").unwrap_or(&0);
        }
        Ok(output)
    }

    fn reconcile_user_usage_impl(&self, _full: bool) -> Result<()> {
        // Canonical history and bounded metrics are queried directly. There
        // is no secondary counter table to reconcile or rewrite while idle.
        Ok(())
    }

    fn get_tool_usage_stats_impl(
        &self,
        since_time: Option<f64>,
        until_time: Option<f64>,
    ) -> Result<HashMap<String, i64>> {
        self.ensure_initialized()?;
        let mut query =
            String::from("SELECT tool, COUNT(*) as tool_records FROM runtime_tool_metrics");
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
        filters.push("excluded = 0".to_owned());
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" GROUP BY tool ORDER BY tool_records DESC");
        let _ = self.runtime_logs()?;
        let conn = self.open_logs()?;
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
            "SELECT session_id, user_id, COUNT(*) as tool_calls, MAX(created_time) as last_time FROM runtime_tool_metrics WHERE tool = ?",
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
        filters.push("excluded = 0".to_owned());
        if !filters.is_empty() {
            query.push_str(" AND ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" GROUP BY session_id, user_id ORDER BY last_time DESC");
        let _ = self.runtime_logs()?;
        let conn = self.open_logs()?;
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

    fn get_log_usage_impl(&self) -> Result<LogUsage> {
        // Report the on-disk footprint of both log stores. Reading SQLite page
        // counts used to hide the real volume twice over: it skipped the WAL
        // (which holds most recent bytes) and ignored the thread history store
        // entirely, so the admin monitor showed a few megabytes for a
        // multi-gigabyte transcript.
        let root = self.db_path.parent().unwrap_or_else(|| Path::new("."));
        let mut thread_bytes = 0;
        for name in [
            "thread_history_1.sqlite",
            "thread_history_1.sqlite-wal",
            "thread_history_1.sqlite-shm",
        ] {
            thread_bytes += regular_file_bytes(&root.join(name));
        }
        for name in ["sessions", "archived_sessions"] {
            thread_bytes += log_directory_bytes(&root.join(name), 0);
        }
        let mut diagnostic_bytes = 0;
        for name in ["logs_1.sqlite", "logs_1.sqlite-wal", "logs_1.sqlite-shm"] {
            diagnostic_bytes += regular_file_bytes(&root.join(name));
        }
        Ok(LogUsage {
            thread_bytes,
            diagnostic_bytes,
        })
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
        let _ = self.runtime_logs()?;
        let mut conn = self.open_logs()?;
        self.attach_state_and_queue(&conn)?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let now = Self::now_ts();
        // Keep one protection snapshot. Only the logs file is mutated; state
        // and queue are attached as read sources for this explicit action.
        tx.execute(
            "CREATE TEMP TABLE protected_chat_sessions AS
             SELECT session_id FROM runtime_queue.session_locks WHERE expires_at>?1 OR suspended<>0
             UNION SELECT session_id FROM runtime_queue.agent_tasks WHERE status IN ('pending','running','retry','queued','waiting')
             UNION SELECT session_id FROM runtime_queue.session_runs WHERE status IN ('pending','running','queued','waiting','cancelling')
             UNION SELECT session_id FROM monitor_sessions WHERE status IN ('running','queued','waiting','cancelling')",
            [now],
        )?;
        let mut output = HashMap::new();
        for (table, time) in [
            ("runtime_tool_metrics", "created_time"),
            ("monitor_sessions", "COALESCE(updated_time,0)"),
            ("stream_events", "created_time"),
        ] {
            let deleted = tx.execute(&format!(
                "DELETE FROM {table} WHERE {time}>=?1 AND {time}<=?2
                 AND NOT EXISTS(SELECT 1 FROM protected_chat_sessions p WHERE p.session_id={table}.session_id)"
            ), params![start,end])? as i64;
            output.insert(table.to_owned(), deleted);
        }
        tx.commit()?;
        Ok(output)
    }

    fn delete_thread_logs_by_user_impl(&self, user_id: &str) -> Result<i64> {
        super::super::canonical_thread_log::wait(
            self.canonical_threads()?.delete_user_threads(user_id),
        )?
    }

    fn delete_tool_logs_impl(&self, user_id: &str) -> Result<i64> {
        if user_id.trim().is_empty() {
            return Ok(0);
        }
        let _ = self.runtime_logs()?;
        Ok(self.open_logs()?.execute(
            "DELETE FROM runtime_tool_metrics WHERE user_id=?1",
            [user_id],
        )? as i64)
    }

    fn delete_tool_logs_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64> {
        if user_id.trim().is_empty() || session_id.trim().is_empty() {
            return Ok(0);
        }
        let _ = self.runtime_logs()?;
        Ok(self.open_logs()?.execute(
            "DELETE FROM runtime_tool_metrics WHERE user_id=?1 AND session_id=?2",
            params![user_id, session_id],
        )? as i64)
    }

    fn delete_artifact_logs_impl(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let _ = self.runtime_logs()?;
        let conn = self.open_logs()?;
        let affected = conn.execute(
            "DELETE FROM runtime_artifact_resources WHERE user_id = ?",
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
        let _ = self.runtime_logs()?;
        let conn = self.open_logs()?;
        let affected = conn.execute(
            "DELETE FROM runtime_artifact_resources WHERE user_id = ? AND session_id = ?",
            params![cleaned_user, cleaned_session],
        )?;
        Ok(affected as i64)
    }

    fn mark_deleted_session_log_grace_impl(
        &self,
        user_id: &str,
        session_id: &str,
        deleted_at: f64,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() || !deleted_at.is_finite() {
            return Ok(());
        }
        let _ = self.runtime_logs()?;
        let conn = self.open_logs()?;
        conn.execute(
            "INSERT INTO deleted_session_log_grace (user_id, session_id, deleted_at) \
             VALUES (?, ?, ?) \
             ON CONFLICT (user_id, session_id) DO UPDATE SET deleted_at = excluded.deleted_at",
            params![cleaned_user, cleaned_session, deleted_at],
        )?;
        Ok(())
    }

    fn list_expired_deleted_session_log_grace_impl(
        &self,
        cutoff: f64,
        limit: i64,
    ) -> Result<Vec<(String, String)>> {
        self.ensure_initialized()?;
        if !cutoff.is_finite() || limit <= 0 {
            return Ok(Vec::new());
        }
        let _ = self.runtime_logs()?;
        let conn = self.open_logs()?;
        let mut stmt = conn.prepare(
            "SELECT user_id, session_id FROM deleted_session_log_grace \
             WHERE deleted_at <= ? ORDER BY deleted_at ASC LIMIT ?",
        )?;
        let rows = stmt
            .query_map(params![cutoff, limit], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<(String, String)>, _>>()?;
        Ok(rows)
    }

    fn delete_deleted_session_log_grace_impl(&self, user_id: &str, session_id: &str) -> Result<()> {
        self.ensure_initialized()?;
        let _ = self.runtime_logs()?;
        let conn = self.open_logs()?;
        conn.execute(
            "DELETE FROM deleted_session_log_grace WHERE user_id = ? AND session_id = ?",
            params![user_id, session_id],
        )?;
        Ok(())
    }
}

/// Missing files count as zero; symlinks are never followed so a log footprint
/// report cannot escape the storage root.
fn regular_file_bytes(path: &Path) -> u64 {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => metadata.len(),
        _ => 0,
    }
}

/// Bounded recursive size of a log directory (rollout transcripts). The depth
/// limit only guards against pathological trees; normal `YYYY/MM/DD` nesting is
/// three levels deep.
fn log_directory_bytes(path: &Path, depth: u8) -> u64 {
    const MAX_DEPTH: u8 = 6;
    if depth >= MAX_DEPTH {
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0_u64;
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            total = total.saturating_add(log_directory_bytes(&entry.path(), depth + 1));
        } else if file_type.is_file() {
            total = total.saturating_add(entry.metadata().map(|meta| meta.len()).unwrap_or(0));
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{ConversationLogStore, LogStatsStore};

    #[test]
    fn log_usage_separates_thread_and_diagnostic_footprint() {
        let dir = tempfile::tempdir().unwrap();
        let storage =
            SqliteStorage::new(dir.path().join("state_1.sqlite").to_string_lossy().into());
        storage.ensure_initialized().unwrap();
        storage
            .append_tool_log(
                "sample-user",
                &json!({"session_id":"sample-thread","tool":"sample-tool","ok":true}),
            )
            .unwrap();
        let rollout_dir = dir.path().join("sessions/2026/01/01");
        std::fs::create_dir_all(&rollout_dir).unwrap();
        std::fs::write(rollout_dir.join("rollout-sample.jsonl"), vec![b'x'; 8192]).unwrap();

        let usage = storage.get_log_usage().unwrap();
        assert!(
            usage.diagnostic_bytes > 0,
            "diagnostic log store should be counted"
        );
        assert!(
            usage.thread_bytes >= 8192,
            "thread transcripts should be counted, got {}",
            usage.thread_bytes
        );
        assert_eq!(usage.total(), usage.thread_bytes + usage.diagnostic_bytes);
    }
}
