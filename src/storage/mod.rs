// 存储模块：封装 SQLite/Postgres 持久化读写，提供统一的历史/监控/记忆接口。

mod postgres;
mod sqlite;

use crate::config::StorageConfig;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

pub use postgres::PostgresStorage;
pub use sqlite::SqliteStorage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionLockStatus {
    Acquired,
    UserBusy,
    SystemBusy,
}

/// 存储后端抽象，统一封装历史/监控/记忆的持久化读写。
pub trait StorageBackend: Send + Sync {
    fn ensure_initialized(&self) -> Result<()>;

    fn get_meta(&self, key: &str) -> Result<Option<String>>;
    fn set_meta(&self, key: &str, value: &str) -> Result<()>;
    fn delete_meta_prefix(&self, prefix: &str) -> Result<usize>;

    fn append_chat(&self, user_id: &str, payload: &Value) -> Result<()>;
    fn append_tool_log(&self, user_id: &str, payload: &Value) -> Result<()>;
    fn append_artifact_log(&self, user_id: &str, payload: &Value) -> Result<()>;

    fn load_chat_history(
        &self,
        user_id: &str,
        session_id: &str,
        limit: Option<i64>,
    ) -> Result<Vec<Value>>;
    fn load_artifact_logs(&self, user_id: &str, session_id: &str, limit: i64)
        -> Result<Vec<Value>>;
    fn get_session_system_prompt(
        &self,
        user_id: &str,
        session_id: &str,
        language: Option<&str>,
    ) -> Result<Option<String>>;

    fn get_user_chat_stats(&self) -> Result<HashMap<String, HashMap<String, i64>>>;
    fn get_user_tool_stats(&self) -> Result<HashMap<String, HashMap<String, i64>>>;
    fn get_tool_usage_stats(
        &self,
        since_time: Option<f64>,
        until_time: Option<f64>,
    ) -> Result<HashMap<String, i64>>;
    fn get_tool_session_usage(
        &self,
        tool: &str,
        since_time: Option<f64>,
        until_time: Option<f64>,
    ) -> Result<Vec<HashMap<String, Value>>>;

    fn delete_chat_history(&self, user_id: &str) -> Result<i64>;
    fn delete_tool_logs(&self, user_id: &str) -> Result<i64>;
    fn delete_artifact_logs(&self, user_id: &str) -> Result<i64>;

    fn upsert_monitor_record(&self, payload: &Value) -> Result<()>;
    fn get_monitor_record(&self, session_id: &str) -> Result<Option<Value>>;
    fn load_monitor_records(&self) -> Result<Vec<Value>>;
    fn delete_monitor_record(&self, session_id: &str) -> Result<()>;
    fn delete_monitor_records_by_user(&self, user_id: &str) -> Result<i64>;

    fn try_acquire_session_lock(
        &self,
        session_id: &str,
        user_id: &str,
        ttl_s: f64,
        max_sessions: i64,
    ) -> Result<SessionLockStatus>;
    fn touch_session_lock(&self, session_id: &str, ttl_s: f64) -> Result<()>;
    fn release_session_lock(&self, session_id: &str) -> Result<()>;
    fn delete_session_locks_by_user(&self, user_id: &str) -> Result<i64>;

    fn append_stream_event(
        &self,
        session_id: &str,
        user_id: &str,
        event_id: i64,
        payload: &Value,
    ) -> Result<()>;
    fn load_stream_events(
        &self,
        session_id: &str,
        after_event_id: i64,
        limit: i64,
    ) -> Result<Vec<Value>>;
    fn delete_stream_events_before(&self, before_time: f64) -> Result<i64>;
    fn delete_stream_events_by_user(&self, user_id: &str) -> Result<i64>;

    fn get_memory_enabled(&self, user_id: &str) -> Result<Option<bool>>;
    fn set_memory_enabled(&self, user_id: &str, enabled: bool) -> Result<()>;
    fn load_memory_settings(&self) -> Result<Vec<HashMap<String, Value>>>;
    fn upsert_memory_record(
        &self,
        user_id: &str,
        session_id: &str,
        summary: &str,
        max_records: i64,
        now_ts: f64,
    ) -> Result<()>;
    fn load_memory_records(
        &self,
        user_id: &str,
        limit: i64,
        order_desc: bool,
    ) -> Result<Vec<HashMap<String, Value>>>;
    fn get_memory_record_stats(&self) -> Result<Vec<HashMap<String, Value>>>;
    fn delete_memory_record(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn delete_memory_records_by_user(&self, user_id: &str) -> Result<i64>;
    fn delete_memory_settings_by_user(&self, user_id: &str) -> Result<i64>;

    fn upsert_memory_task_log(
        &self,
        user_id: &str,
        session_id: &str,
        task_id: &str,
        status: &str,
        queued_time: f64,
        started_time: f64,
        finished_time: f64,
        elapsed_s: f64,
        request_payload: Option<&Value>,
        result: &str,
        error: &str,
        updated_time: Option<f64>,
    ) -> Result<()>;
    fn load_memory_task_logs(&self, limit: Option<i64>) -> Result<Vec<HashMap<String, Value>>>;
    fn load_memory_task_log_by_task_id(
        &self,
        task_id: &str,
    ) -> Result<Option<HashMap<String, Value>>>;
    fn delete_memory_task_log(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn delete_memory_task_logs_by_user(&self, user_id: &str) -> Result<i64>;

    fn create_evaluation_run(&self, payload: &Value) -> Result<()>;
    fn update_evaluation_run(&self, run_id: &str, payload: &Value) -> Result<()>;
    fn append_evaluation_item(&self, run_id: &str, payload: &Value) -> Result<()>;
    fn load_evaluation_runs(
        &self,
        user_id: Option<&str>,
        status: Option<&str>,
        model_name: Option<&str>,
        since_time: Option<f64>,
        until_time: Option<f64>,
        limit: Option<i64>,
    ) -> Result<Vec<Value>>;
    fn load_evaluation_run(&self, run_id: &str) -> Result<Option<Value>>;
    fn load_evaluation_items(&self, run_id: &str) -> Result<Vec<Value>>;

    fn cleanup_retention(&self, retention_days: i64) -> Result<HashMap<String, i64>>;
}

/// 构建存储后端，根据 backend 配置选择 SQLite/Postgres。
pub fn build_storage(config: &StorageConfig) -> Result<Arc<dyn StorageBackend>> {
    let backend = config.backend.trim().to_lowercase();
    let backend = if backend.is_empty() {
        "sqlite".to_string()
    } else {
        backend
    };
    match backend.as_str() {
        "sqlite" | "default" => Ok(Arc::new(SqliteStorage::new(
            config.db_path.trim().to_string(),
        ))),
        "postgres" | "postgresql" | "pg" | "auto" => Ok(Arc::new(PostgresStorage::new(
            config.postgres.dsn.clone(),
            config.postgres.connect_timeout_s,
        )?)),
        other => Err(anyhow!("未知存储后端: {other}")),
    }
}
