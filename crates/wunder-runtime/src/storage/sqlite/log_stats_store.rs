use super::SqliteStorage;
use crate::storage::{
    StorageLifecycle, USAGE_CHAT_WATERMARK_KEY, USAGE_FULL_RECONCILE_INTERVAL_S,
    USAGE_FULL_RECONCILE_KEY, USAGE_TOOL_WATERMARK_KEY,
};
use anyhow::Result;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter};
use serde_json::{json, Value};
use std::collections::HashMap;

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
    fn get_log_usage_impl(&self) -> Result<u64>;
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

/// Exclusion is now a materialized column: the write path flags rows once,
/// stats queries never rescan the payload text.
fn append_tool_log_exclusions(filters: &mut Vec<String>) {
    filters.push("excluded = 0".to_string());
}

pub(super) fn read_meta_f64_tx(
    tx: &rusqlite::Transaction<'_>,
    key: &str,
) -> Result<Option<f64>> {
    let value: Option<String> = tx
        .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |row| {
            row.get(0)
        })
        .map(Some)
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;
    Ok(value.and_then(|value| value.trim().parse::<f64>().ok()))
}

pub(super) fn write_meta_f64_tx(
    tx: &rusqlite::Transaction<'_>,
    key: &str,
    value: f64,
    now: f64,
) -> Result<()> {
    tx.execute(
        "INSERT INTO meta (key, value, updated_time) VALUES (?1, ?2, ?3) \
         ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_time = excluded.updated_time",
        params![key, value.to_string(), now],
    )?;
    Ok(())
}

/// Decrement summary counters for rows that were inside the reconcile
/// watermarks when deleted. Summary values clamp at zero so a drifted
/// summary cannot go negative; the periodic full reconcile corrects drift.
pub(super) fn decrement_usage_summary_tx(
    tx: &rusqlite::Transaction<'_>,
    chat: &[(String, i64)],
    tool: &[(String, i64)],
    now: f64,
) -> Result<()> {
    for (user_id, delta) in chat {
        if *delta > 0 {
            tx.execute(
                "UPDATE user_usage_summary SET chat_records = MAX(chat_records - ?2, 0), \
                 updated_time = ?3 WHERE user_id = ?1",
                params![user_id, delta, now],
            )?;
        }
    }
    for (user_id, delta) in tool {
        if *delta > 0 {
            tx.execute(
                "UPDATE user_usage_summary SET tool_records = MAX(tool_records - ?2, 0), \
                 updated_time = ?3 WHERE user_id = ?1",
                params![user_id, delta, now],
            )?;
        }
    }
    Ok(())
}

impl SqliteLogStatsStorage for SqliteStorage {
    fn get_user_chat_stats_impl(&self) -> Result<HashMap<String, HashMap<String, i64>>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT l.user_id, COUNT(i.item_id) as chat_records, COALESCE(MAX(i.updated_time), MAX(l.updated_time)) as last_time FROM thread_logs l LEFT JOIN thread_items i ON i.session_id=l.session_id AND i.user_id=l.user_id AND i.visibility='user' AND i.kind IN ('user_message','assistant_message') GROUP BY l.user_id",
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
        append_tool_log_exclusions(&mut filters);
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

    fn get_user_usage_summary_impl(&self) -> Result<HashMap<String, (i64, i64)>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT user_id, chat_records, tool_records FROM user_usage_summary",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<(String, i64, i64)>, _>>()?;
        let mut summary = HashMap::new();
        for (user_id, chat_records, tool_records) in rows {
            let cleaned = user_id.trim();
            if cleaned.is_empty() {
                continue;
            }
            summary.insert(cleaned.to_string(), (chat_records, tool_records));
        }
        Ok(summary)
    }

    fn reconcile_user_usage_impl(&self, full: bool) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let now = Self::now_ts();
        let last_full = read_meta_f64_tx(&tx, USAGE_FULL_RECONCILE_KEY)?;
        let needs_full = full
            || last_full.is_none()
            || now - last_full.unwrap_or(0.0) >= USAGE_FULL_RECONCILE_INTERVAL_S;
        if needs_full {
            // Full recompute: replace the summary in one transaction. Chat
            // counts follow the thread_logs join shape so both paths agree.
            tx.execute("DELETE FROM user_usage_summary", [])?;
            tx.execute(
                "INSERT INTO user_usage_summary (user_id, chat_records, tool_records, updated_time) \
                 SELECT u.user_id, COALESCE(c.chat_records, 0), COALESCE(t.tool_records, 0), ?1 \
                 FROM (SELECT user_id FROM thread_logs GROUP BY user_id \
                       UNION SELECT user_id FROM tool_logs WHERE excluded = 0 GROUP BY user_id) u \
                 LEFT JOIN (SELECT l.user_id, COUNT(i.item_id) AS chat_records \
                            FROM thread_logs l \
                            LEFT JOIN thread_items i ON i.session_id = l.session_id AND i.user_id = l.user_id \
                              AND i.visibility = 'user' AND i.kind IN ('user_message','assistant_message') \
                            GROUP BY l.user_id) c ON c.user_id = u.user_id \
                 LEFT JOIN (SELECT user_id, COUNT(*) AS tool_records FROM tool_logs \
                            WHERE excluded = 0 GROUP BY user_id) t ON t.user_id = u.user_id",
                params![now],
            )?;
            let tool_max: i64 = tx.query_row("SELECT COALESCE(MAX(id), 0) FROM tool_logs", [], |row| row.get(0))?;
            let chat_max: f64 = tx.query_row("SELECT COALESCE(MAX(created_time), 0) FROM thread_items", [], |row| row.get(0))?;
            write_meta_f64_tx(&tx, USAGE_TOOL_WATERMARK_KEY, tool_max as f64, now)?;
            write_meta_f64_tx(&tx, USAGE_CHAT_WATERMARK_KEY, chat_max, now)?;
            write_meta_f64_tx(&tx, USAGE_FULL_RECONCILE_KEY, now, now)?;
        } else {
            // Incremental: aggregate only rows past the watermarks, then
            // advance them inside the same transaction. Watermarks always
            // move forward; a weekly full pass corrects any drift.
            let tool_wm = read_meta_f64_tx(&tx, USAGE_TOOL_WATERMARK_KEY)?.unwrap_or(0.0) as i64;
            let chat_wm = read_meta_f64_tx(&tx, USAGE_CHAT_WATERMARK_KEY)?.unwrap_or(0.0);
            tx.execute(
                "INSERT INTO user_usage_summary (user_id, chat_records, tool_records, updated_time) \
                 SELECT user_id, 0, COUNT(*) AS delta_records, ?2 FROM tool_logs \
                 WHERE id > ?1 AND excluded = 0 GROUP BY user_id \
                 ON CONFLICT (user_id) DO UPDATE SET \
                   tool_records = user_usage_summary.tool_records + excluded.delta_records, \
                   updated_time = excluded.updated_time",
                params![tool_wm, now],
            )?;
            tx.execute(
                "INSERT INTO user_usage_summary (user_id, chat_records, tool_records, updated_time) \
                 SELECT user_id, COUNT(*) AS delta_records, 0, ?2 FROM thread_items \
                 WHERE created_time > ?1 AND visibility = 'user' \
                   AND kind IN ('user_message','assistant_message') GROUP BY user_id \
                 ON CONFLICT (user_id) DO UPDATE SET \
                   chat_records = user_usage_summary.chat_records + excluded.delta_records, \
                   updated_time = excluded.updated_time",
                params![chat_wm, now],
            )?;
            let tool_max: i64 = tx.query_row("SELECT COALESCE(MAX(id), 0) FROM tool_logs", [], |row| row.get(0))?;
            let chat_max: f64 = tx.query_row("SELECT COALESCE(MAX(created_time), 0) FROM thread_items", [], |row| row.get(0))?;
            if tool_max as f64 > tool_wm as f64 {
                write_meta_f64_tx(&tx, USAGE_TOOL_WATERMARK_KEY, tool_max as f64, now)?;
            }
            if chat_max > chat_wm {
                write_meta_f64_tx(&tx, USAGE_CHAT_WATERMARK_KEY, chat_max, now)?;
            }
        }
        tx.commit()?;
        Ok(())
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
        append_tool_log_exclusions(&mut filters);
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
        append_tool_log_exclusions(&mut filters);
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
                'thread_logs', 'thread_turns', 'thread_items', 'thread_item_blocks', 'thread_log_changes', 'thread_log_metrics', \
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
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let now = Self::now_ts();
        let live = crate::storage::session_cleanup::LIVE_SESSION_PREDICATE.replace(":now", "?1");
        // Keep the protection snapshot and all deletion steps in one transaction.
        tx.execute(
            &format!(
                "CREATE TEMP TABLE protected_chat_sessions AS \
            SELECT c.session_id FROM chat_sessions c WHERE {live}"
            ),
            [now],
        )?;
        let delete_range = |table: &str, time_field: &str| -> Result<i64> {
            let sql = format!("DELETE FROM {table} WHERE {time_field} >= ? AND {time_field} <= ? \
                AND NOT EXISTS (SELECT 1 FROM protected_chat_sessions p WHERE p.session_id = {table}.session_id)");
            Ok(tx.execute(&sql, params![start, end])? as i64)
        };
        let mut results = HashMap::new();
        // tool_logs: rows inside the usage watermark were counted in the
        // summary and must decrement it; rows past the watermark (or excluded)
        // were never counted and are removed without adjustment. Per-user
        // deltas are aggregated before deletion, in the same transaction.
        let tool_wm = read_meta_f64_tx(&tx, USAGE_TOOL_WATERMARK_KEY)?.unwrap_or(0.0) as i64;
        let tool_deltas: Vec<(String, i64)> = {
            let mut stmt = tx.prepare(
                "SELECT user_id, COUNT(*) FROM tool_logs WHERE created_time >= ? AND created_time <= ? \
                 AND excluded = 0 AND id <= ? \
                 AND NOT EXISTS (SELECT 1 FROM protected_chat_sessions p WHERE p.session_id = tool_logs.session_id) \
                 GROUP BY user_id",
            )?;
            let rows = stmt
                .query_map(params![start, end, tool_wm], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })?
                .collect::<std::result::Result<Vec<(String, i64)>, _>>()?;
            rows
        };
        let tool_counted_deleted = tx.execute(
            "DELETE FROM tool_logs WHERE created_time >= ? AND created_time <= ? \
             AND excluded = 0 AND id <= ? \
             AND NOT EXISTS (SELECT 1 FROM protected_chat_sessions p WHERE p.session_id = tool_logs.session_id)",
            params![start, end, tool_wm],
        )? as i64;
        let tool_rest = delete_range("tool_logs", "created_time")?;
        results.insert("tool_logs".to_string(), tool_counted_deleted + tool_rest);
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
        results.insert(
            "chat_sessions".to_string(),
            super::session_cleanup::remove_empty_catalog(&tx, start, end, now)?,
        );
        decrement_usage_summary_tx(&tx, &[], &tool_deltas, now)?;
        tx.execute("DROP TABLE protected_chat_sessions", [])?;
        tx.commit()?;
        Ok(results)
    }

    fn delete_thread_logs_by_user_impl(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        if user_id.trim().is_empty() {
            return Ok(0);
        }
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let now = Self::now_ts();
        let chat_wm = read_meta_f64_tx(&tx, USAGE_CHAT_WATERMARK_KEY)?.unwrap_or(0.0);
        let mut removed = 0i64;
        for table in [
            "thread_item_blocks",
            "thread_log_changes",
            "thread_turns",
            "thread_log_metrics",
            "thread_logs",
        ] {
            removed += tx.execute(
                &format!("DELETE FROM {table} WHERE user_id=?"),
                params![user_id],
            )? as i64;
        }
        // thread_items: only the visible-message rows inside the chat
        // watermark were counted; the remainder was never in the summary.
        let counted = tx.execute(
            "DELETE FROM thread_items WHERE user_id=? AND visibility='user' \
             AND kind IN ('user_message','assistant_message') AND created_time <= ?",
            params![user_id, chat_wm],
        )? as i64;
        let rest =
            tx.execute("DELETE FROM thread_items WHERE user_id=?", params![user_id])? as i64;
        removed += counted + rest;
        decrement_usage_summary_tx(&tx, &[(user_id.to_string(), counted)], &[], now)?;
        tx.commit()?;
        Ok(removed)
    }

    fn delete_tool_logs_impl(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let now = Self::now_ts();
        let tool_wm = read_meta_f64_tx(&tx, USAGE_TOOL_WATERMARK_KEY)?.unwrap_or(0.0) as i64;
        let counted = tx.execute(
            "DELETE FROM tool_logs WHERE user_id=? AND excluded=0 AND id <= ?",
            params![cleaned, tool_wm],
        )? as i64;
        let rest = tx.execute("DELETE FROM tool_logs WHERE user_id=?", params![cleaned])? as i64;
        decrement_usage_summary_tx(&tx, &[], &[(cleaned.to_string(), counted)], now)?;
        tx.commit()?;
        Ok(counted + rest)
    }

    fn delete_tool_logs_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let now = Self::now_ts();
        let tool_wm = read_meta_f64_tx(&tx, USAGE_TOOL_WATERMARK_KEY)?.unwrap_or(0.0) as i64;
        let counted = tx.execute(
            "DELETE FROM tool_logs WHERE user_id=? AND session_id=? AND excluded=0 AND id <= ?",
            params![cleaned_user, cleaned_session, tool_wm],
        )? as i64;
        let rest = tx.execute(
            "DELETE FROM tool_logs WHERE user_id=? AND session_id=?",
            params![cleaned_user, cleaned_session],
        )? as i64;
        decrement_usage_summary_tx(&tx, &[], &[(cleaned_user.to_string(), counted)], now)?;
        tx.commit()?;
        Ok(counted + rest)
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
        let conn = self.open()?;
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
        let conn = self.open()?;
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
        let conn = self.open()?;
        conn.execute(
            "DELETE FROM deleted_session_log_grace WHERE user_id = ? AND session_id = ?",
            params![user_id, session_id],
        )?;
        Ok(())
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
                    "role": "user",
                    "content": "message-b"
                }),
            )
            .expect("append chat b");

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

        // One admitted user turn writes 6 backing rows: thread_logs,
        // thread_turns, thread_items, 2x thread_log_changes, thread_log_metrics.
        assert_eq!(
            storage
                .delete_thread_log_by_session("user-a", "session-a")
                .expect("delete chat by session"),
            6
        );
        assert!(storage
            .load_thread_context_items("user-a", "session-a", 500, true)
            .expect("load chat after delete")
            .is_empty());
        assert_eq!(
            storage
                .delete_thread_logs_by_user("user-a")
                .expect("delete remaining chat"),
            6
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
}
