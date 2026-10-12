use super::agent_runtime_store::PostgresAgentRuntimeStorage;
use super::log_stats_store::{decrement_usage_summary, read_meta_f64};
use super::PostgresStorage;
use crate::storage::{
    StorageLifecycle, USAGE_CHAT_WATERMARK_KEY, USAGE_TOOL_WATERMARK_KEY,
};
use anyhow::Result;
use std::collections::HashMap;

pub(super) trait PostgresRetentionStorage {
    fn cleanup_retention_impl(&self, cutoff_epoch_s: f64) -> Result<HashMap<String, i64>>;
    fn cleanup_orphan_session_logs_impl(&self, cutoff_epoch_s: f64)
        -> Result<HashMap<String, i64>>;
}

impl PostgresRetentionStorage for PostgresStorage {
    fn cleanup_retention_impl(&self, cutoff_epoch_s: f64) -> Result<HashMap<String, i64>> {
        self.ensure_initialized()?;
        let mut deleted = HashMap::new();
        if cutoff_epoch_s <= 0.0 {
            return Ok(deleted);
        }
        // Only stream_events are ephemeral replay buffers. Conversation,
        // context, runtime and audit records stay durable and are removed only
        // through the administrator cleanup endpoint. PostgreSQL reclaims the
        // freed space itself via autovacuum.
        let removed = self.delete_stream_events_before_impl(cutoff_epoch_s)?;
        if removed > 0 {
            deleted.insert("stream_events".to_string(), removed);
        }
        Ok(deleted)
    }

    fn cleanup_orphan_session_logs_impl(
        &self,
        cutoff_epoch_s: f64,
    ) -> Result<HashMap<String, i64>> {
        self.ensure_initialized()?;
        let mut deleted = HashMap::new();
        if cutoff_epoch_s <= 0.0 {
            return Ok(deleted);
        }
        // Orphan sweep: log rows whose session vanished from the durable
        // catalog (interrupted deletions, older purge paths). Only rows older
        // than the cutoff are swept so in-flight sessions are never touched,
        // and deletion-grace sessions keep their evidence.
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        tx.execute("SET LOCAL statement_timeout = '300s'", &[])?;
        let now = Self::now_ts();
        let chat_wm = read_meta_f64(&mut tx, USAGE_CHAT_WATERMARK_KEY)?.unwrap_or(0.0);
        let tool_wm = read_meta_f64(&mut tx, USAGE_TOOL_WATERMARK_KEY)?.unwrap_or(0.0) as i64;
        let orphan_predicate = "t.created_time < $1 \
             AND NOT EXISTS (SELECT 1 FROM chat_sessions c WHERE c.session_id = t.session_id) \
             AND NOT EXISTS (SELECT 1 FROM deleted_session_log_grace g WHERE g.session_id = t.session_id)";
        for table in ["thread_turns", "thread_log_changes", "artifact_logs"] {
            let sql = format!("DELETE FROM {table} t WHERE {orphan_predicate}");
            let removed = tx.execute(&sql, &[&cutoff_epoch_s])? as i64;
            if removed > 0 {
                deleted.insert(table.to_string(), removed);
            }
        }
        // thread_items: rows inside the chat watermark were counted in the
        // usage summary and must decrement it; the rest was never admitted.
        let chat_counted_rows = tx.query(
            &format!(
                "WITH del AS (DELETE FROM thread_items t WHERE {orphan_predicate} \
                   AND t.visibility='user' AND t.kind IN ('user_message','assistant_message') \
                   AND t.created_time <= $2 RETURNING user_id) \
                 SELECT user_id, COUNT(*) FROM del GROUP BY user_id"
            ),
            &[&cutoff_epoch_s, &chat_wm],
        )?;
        let mut chat_counted = 0i64;
        let mut chat_deltas: Vec<(String, i64)> = Vec::new();
        for row in chat_counted_rows {
            let user_id: String = row.get(0);
            let count: i64 = row.get(1);
            chat_counted += count;
            chat_deltas.push((user_id, count));
        }
        let chat_rest = tx.execute(
            &format!("DELETE FROM thread_items t WHERE {orphan_predicate}"),
            &[&cutoff_epoch_s],
        )? as i64;
        if chat_counted + chat_rest > 0 {
            deleted.insert("thread_items".to_string(), chat_counted + chat_rest);
        }
        // tool_logs: same split on the tool watermark with the excluded flag.
        let tool_counted_rows = tx.query(
            &format!(
                "WITH del AS (DELETE FROM tool_logs t WHERE {orphan_predicate} \
                   AND t.excluded = FALSE AND t.id <= $2 RETURNING user_id) \
                 SELECT user_id, COUNT(*) FROM del GROUP BY user_id"
            ),
            &[&cutoff_epoch_s, &tool_wm],
        )?;
        let mut tool_counted = 0i64;
        let mut tool_deltas: Vec<(String, i64)> = Vec::new();
        for row in tool_counted_rows {
            let user_id: String = row.get(0);
            let count: i64 = row.get(1);
            tool_counted += count;
            tool_deltas.push((user_id, count));
        }
        let tool_rest = tx.execute(
            &format!("DELETE FROM tool_logs t WHERE {orphan_predicate}"),
            &[&cutoff_epoch_s],
        )? as i64;
        if tool_counted + tool_rest > 0 {
            deleted.insert("tool_logs".to_string(), tool_counted + tool_rest);
        }
        decrement_usage_summary(&mut tx, &chat_deltas, &tool_deltas, now)?;
        tx.commit()?;
        Ok(deleted)
    }
}
