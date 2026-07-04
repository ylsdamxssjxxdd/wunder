use super::PostgresStorage;
use crate::storage::{StorageLifecycle, TOOL_LOG_EXCLUDED_NAMES, TOOL_LOG_SKILL_READ_MARKER};
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio_postgres::types::ToSql;

pub(super) trait PostgresLogStatsStorage {
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

fn append_tool_log_exclusions(filters: &mut Vec<String>, params: &mut Vec<Box<dyn ToSql + Sync>>) {
    if !TOOL_LOG_EXCLUDED_NAMES.is_empty() {
        let start = params.len() + 1;
        let placeholders = (0..TOOL_LOG_EXCLUDED_NAMES.len())
            .map(|index| format!("${}", start + index))
            .collect::<Vec<_>>()
            .join(", ");
        filters.push(format!("tool NOT IN ({placeholders})"));
        for name in TOOL_LOG_EXCLUDED_NAMES {
            params.push(Box::new(name.to_string()));
        }
    }
    let marker = format!("%{TOOL_LOG_SKILL_READ_MARKER}%");
    params.push(Box::new(marker));
    filters.push(format!("(data IS NULL OR data NOT LIKE ${})", params.len()));
}

impl PostgresLogStatsStorage for PostgresStorage {
    fn get_user_chat_stats_impl(&self) -> Result<HashMap<String, HashMap<String, i64>>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT user_id, COUNT(*) as chat_records, MAX(created_time) as last_time FROM chat_history GROUP BY user_id",
            &[],
        )?;
        let mut stats = HashMap::new();
        for row in rows {
            let user_id: String = row.get(0);
            let cleaned = user_id.trim();
            if cleaned.is_empty() {
                continue;
            }
            let count: i64 = row.get(1);
            let last_time: f64 = row.try_get(2).unwrap_or(0.0);
            let mut entry = HashMap::new();
            entry.insert("chat_records".to_string(), count);
            entry.insert("last_time".to_string(), last_time.floor() as i64);
            stats.insert(cleaned.to_string(), entry);
        }
        Ok(stats)
    }

    fn get_user_tool_stats_impl(&self) -> Result<HashMap<String, HashMap<String, i64>>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut query = String::from(
            "SELECT user_id, COUNT(*) as tool_records, MAX(created_time) as last_time FROM tool_logs",
        );
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        let mut filters = Vec::new();
        append_tool_log_exclusions(&mut filters, &mut params);
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" GROUP BY user_id");
        let params_ref: Vec<&(dyn ToSql + Sync)> =
            params.iter().map(|value| value.as_ref()).collect();
        let rows = conn.query(&query, &params_ref)?;
        let mut stats = HashMap::new();
        for row in rows {
            let user_id: String = row.get(0);
            let cleaned = user_id.trim();
            if cleaned.is_empty() {
                continue;
            }
            let count: i64 = row.get(1);
            let last_time: f64 = row.try_get(2).unwrap_or(0.0);
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
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        let mut filters = Vec::new();
        if let Some(since) = since_time.filter(|value| *value > 0.0) {
            params.push(Box::new(since));
            filters.push(format!("created_time >= ${}", params.len()));
        }
        if let Some(until) = until_time.filter(|value| *value > 0.0) {
            params.push(Box::new(until));
            filters.push(format!("created_time <= ${}", params.len()));
        }
        append_tool_log_exclusions(&mut filters, &mut params);
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" GROUP BY tool ORDER BY tool_records DESC");

        let mut conn = self.conn()?;
        let params_ref: Vec<&(dyn ToSql + Sync)> =
            params.iter().map(|value| value.as_ref()).collect();
        let rows = conn.query(&query, &params_ref)?;
        let mut stats = HashMap::new();
        for row in rows {
            let tool: Option<String> = row.try_get(0).ok();
            let Some(tool) = tool else {
                continue;
            };
            let cleaned = tool.trim();
            if cleaned.is_empty() {
                continue;
            }
            let count: i64 = row.get(1);
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
            "SELECT session_id, user_id, COUNT(*) as tool_calls, MAX(created_time) as last_time FROM tool_logs WHERE tool = $1",
        );
        let mut params: Vec<Box<dyn ToSql + Sync>> = vec![Box::new(cleaned.to_string())];
        let mut filters = Vec::new();
        if let Some(since) = since_time.filter(|value| *value > 0.0) {
            params.push(Box::new(since));
            filters.push(format!("created_time >= ${}", params.len()));
        }
        if let Some(until) = until_time.filter(|value| *value > 0.0) {
            params.push(Box::new(until));
            filters.push(format!("created_time <= ${}", params.len()));
        }
        append_tool_log_exclusions(&mut filters, &mut params);
        if !filters.is_empty() {
            query.push_str(" AND ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" GROUP BY session_id, user_id ORDER BY last_time DESC");

        let mut conn = self.conn()?;
        let params_ref: Vec<&(dyn ToSql + Sync)> =
            params.iter().map(|value| value.as_ref()).collect();
        let rows = conn.query(&query, &params_ref)?;
        let mut sessions = Vec::new();
        for row in rows {
            let session_id: String = row.get(0);
            let cleaned_session = session_id.trim();
            if cleaned_session.is_empty() {
                continue;
            }
            let user_id: String = row.get(1);
            let tool_calls: i64 = row.get(2);
            let last_time: f64 = row.try_get(3).unwrap_or(0.0);
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
        let mut conn = self.conn()?;
        let row = conn.query_one(
            "SELECT \
            COALESCE(pg_total_relation_size(to_regclass('chat_history')), 0) + \
            COALESCE(pg_total_relation_size(to_regclass('model_context_entries')), 0) + \
            COALESCE(pg_total_relation_size(to_regclass('tool_logs')), 0) + \
            COALESCE(pg_total_relation_size(to_regclass('artifact_logs')), 0) + \
            COALESCE(pg_total_relation_size(to_regclass('monitor_sessions')), 0) + \
            COALESCE(pg_total_relation_size(to_regclass('stream_events')), 0) + \
            COALESCE(pg_total_relation_size(to_regclass('memory_task_logs')), 0)",
            &[],
        )?;
        let total: i64 = row.get(0);
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
        let mut conn = self.conn()?;
        let mut results = HashMap::new();
        let mut delete_range = |table: &str, time_field: &str| -> Result<i64> {
            let sql =
                format!("DELETE FROM {table} WHERE {time_field} >= $1 AND {time_field} <= $2");
            Ok(conn.execute(&sql, &[&start, &end])? as i64)
        };
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
            delete_range("monitor_sessions", "updated_time")?,
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
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute("DELETE FROM chat_history WHERE user_id = $1", &[&cleaned])?;
        let _ = conn.execute(
            "DELETE FROM model_context_entries WHERE user_id = $1",
            &[&cleaned],
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
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM chat_history WHERE user_id = $1 AND session_id = $2",
            &[&cleaned_user, &cleaned_session],
        )?;
        let _ = conn.execute(
            "DELETE FROM model_context_entries WHERE user_id = $1 AND session_id = $2",
            &[&cleaned_user, &cleaned_session],
        );
        Ok(affected as i64)
    }

    fn delete_tool_logs_impl(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute("DELETE FROM tool_logs WHERE user_id = $1", &[&cleaned])?;
        Ok(affected as i64)
    }

    fn delete_tool_logs_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM tool_logs WHERE user_id = $1 AND session_id = $2",
            &[&cleaned_user, &cleaned_session],
        )?;
        Ok(affected as i64)
    }

    fn delete_artifact_logs_impl(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute("DELETE FROM artifact_logs WHERE user_id = $1", &[&cleaned])?;
        Ok(affected as i64)
    }

    fn delete_artifact_logs_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM artifact_logs WHERE user_id = $1 AND session_id = $2",
            &[&cleaned_user, &cleaned_session],
        )?;
        Ok(affected as i64)
    }
}
