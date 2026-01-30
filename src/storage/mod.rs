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

#[derive(Debug, Clone)]
pub struct UserAccountRecord {
    pub user_id: String,
    pub username: String,
    pub email: Option<String>,
    pub password_hash: String,
    pub roles: Vec<String>,
    pub status: String,
    pub access_level: String,
    pub unit_id: Option<String>,
    pub daily_quota: i64,
    pub daily_quota_used: i64,
    pub daily_quota_date: Option<String>,
    pub is_demo: bool,
    pub created_at: f64,
    pub updated_at: f64,
    pub last_login_at: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct OrgUnitRecord {
    pub unit_id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub level: i32,
    pub path: String,
    pub path_name: String,
    pub sort_order: i64,
    pub leader_ids: Vec<String>,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone)]
pub struct UserQuotaStatus {
    pub daily_quota: i64,
    pub used: i64,
    pub remaining: i64,
    pub date: String,
    pub allowed: bool,
}

#[derive(Debug, Clone)]
pub struct UserTokenRecord {
    pub token: String,
    pub user_id: String,
    pub expires_at: f64,
    pub created_at: f64,
    pub last_used_at: f64,
}

#[derive(Debug, Clone)]
pub struct UserToolAccessRecord {
    pub user_id: String,
    pub allowed_tools: Option<Vec<String>>,
    pub updated_at: f64,
}

#[derive(Debug, Clone)]
pub struct UserAgentRecord {
    pub agent_id: String,
    pub user_id: String,
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub tool_names: Vec<String>,
    pub access_level: String,
    pub is_shared: bool,
    pub status: String,
    pub icon: Option<String>,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone)]
pub struct UserAgentAccessRecord {
    pub user_id: String,
    pub allowed_agent_ids: Option<Vec<String>>,
    pub blocked_agent_ids: Vec<String>,
    pub updated_at: f64,
}

#[derive(Debug, Clone)]
pub struct VectorDocumentRecord {
    pub doc_id: String,
    pub owner_id: String,
    pub base_name: String,
    pub doc_name: String,
    pub embedding_model: String,
    pub chunk_size: i64,
    pub chunk_overlap: i64,
    pub chunk_count: i64,
    pub status: String,
    pub created_at: f64,
    pub updated_at: f64,
    pub content: String,
    pub chunks_json: String,
}

#[derive(Debug, Clone)]
pub struct VectorDocumentSummaryRecord {
    pub doc_id: String,
    pub doc_name: String,
    pub status: String,
    pub chunk_count: i64,
    pub embedding_model: String,
    pub updated_at: f64,
}

#[derive(Debug, Clone)]
pub struct ChatSessionRecord {
    pub session_id: String,
    pub user_id: String,
    pub title: String,
    pub created_at: f64,
    pub updated_at: f64,
    pub last_message_at: f64,
    pub agent_id: Option<String>,
    pub tool_overrides: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SessionLockRecord {
    pub session_id: String,
    pub user_id: String,
    pub agent_id: String,
    pub updated_time: f64,
    pub expires_at: f64,
}

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
    fn get_log_usage(&self) -> Result<u64>;

    fn delete_chat_history(&self, user_id: &str) -> Result<i64>;
    fn delete_chat_history_by_session(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn delete_tool_logs(&self, user_id: &str) -> Result<i64>;
    fn delete_tool_logs_by_session(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn delete_artifact_logs(&self, user_id: &str) -> Result<i64>;
    fn delete_artifact_logs_by_session(&self, user_id: &str, session_id: &str) -> Result<i64>;

    fn upsert_monitor_record(&self, payload: &Value) -> Result<()>;
    fn get_monitor_record(&self, session_id: &str) -> Result<Option<Value>>;
    fn load_monitor_records(&self) -> Result<Vec<Value>>;
    fn delete_monitor_record(&self, session_id: &str) -> Result<()>;
    fn delete_monitor_records_by_user(&self, user_id: &str) -> Result<i64>;

    fn try_acquire_session_lock(
        &self,
        session_id: &str,
        user_id: &str,
        agent_id: &str,
        ttl_s: f64,
        max_sessions: i64,
    ) -> Result<SessionLockStatus>;
    fn touch_session_lock(&self, session_id: &str, ttl_s: f64) -> Result<()>;
    fn release_session_lock(&self, session_id: &str) -> Result<()>;
    fn delete_session_locks_by_user(&self, user_id: &str) -> Result<i64>;
    fn list_session_locks_by_user(&self, user_id: &str) -> Result<Vec<SessionLockRecord>>;

    fn upsert_vector_document(&self, record: &VectorDocumentRecord) -> Result<()>;
    fn get_vector_document(
        &self,
        owner_id: &str,
        base_name: &str,
        doc_id: &str,
    ) -> Result<Option<VectorDocumentRecord>>;
    fn list_vector_document_summaries(
        &self,
        owner_id: &str,
        base_name: &str,
    ) -> Result<Vec<VectorDocumentSummaryRecord>>;
    fn delete_vector_document(&self, owner_id: &str, base_name: &str, doc_id: &str)
        -> Result<bool>;
    fn delete_vector_documents_by_base(&self, owner_id: &str, base_name: &str) -> Result<i64>;

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
    fn delete_stream_events_by_session(&self, session_id: &str) -> Result<i64>;

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
    fn upsert_evaluation_item(&self, run_id: &str, payload: &Value) -> Result<()>;
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
    fn delete_evaluation_run(&self, run_id: &str) -> Result<i64>;

    fn cleanup_retention(&self, retention_days: i64) -> Result<HashMap<String, i64>>;

    fn upsert_user_account(&self, record: &UserAccountRecord) -> Result<()>;
    fn upsert_user_accounts(&self, records: &[UserAccountRecord]) -> Result<()>;
    fn get_user_account(&self, user_id: &str) -> Result<Option<UserAccountRecord>>;
    fn get_user_account_by_username(&self, username: &str) -> Result<Option<UserAccountRecord>>;
    fn get_user_account_by_email(&self, email: &str) -> Result<Option<UserAccountRecord>>;
    fn list_user_accounts(
        &self,
        keyword: Option<&str>,
        unit_ids: Option<&[String]>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserAccountRecord>, i64)>;
    fn delete_user_account(&self, user_id: &str) -> Result<i64>;

    fn list_org_units(&self) -> Result<Vec<OrgUnitRecord>>;
    fn get_org_unit(&self, unit_id: &str) -> Result<Option<OrgUnitRecord>>;
    fn upsert_org_unit(&self, record: &OrgUnitRecord) -> Result<()>;
    fn delete_org_unit(&self, unit_id: &str) -> Result<i64>;

    fn create_user_token(&self, record: &UserTokenRecord) -> Result<()>;
    fn get_user_token(&self, token: &str) -> Result<Option<UserTokenRecord>>;
    fn touch_user_token(&self, token: &str, last_used_at: f64) -> Result<()>;
    fn delete_user_token(&self, token: &str) -> Result<i64>;

    fn upsert_chat_session(&self, record: &ChatSessionRecord) -> Result<()>;
    fn get_chat_session(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<Option<ChatSessionRecord>>;
    fn list_chat_sessions(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChatSessionRecord>, i64)>;
    fn update_chat_session_title(
        &self,
        user_id: &str,
        session_id: &str,
        title: &str,
        updated_at: f64,
    ) -> Result<()>;
    fn touch_chat_session(
        &self,
        user_id: &str,
        session_id: &str,
        updated_at: f64,
        last_message_at: f64,
    ) -> Result<()>;
    fn delete_chat_session(&self, user_id: &str, session_id: &str) -> Result<i64>;

    fn get_user_tool_access(&self, user_id: &str) -> Result<Option<UserToolAccessRecord>>;
    fn set_user_tool_access(
        &self,
        user_id: &str,
        allowed_tools: Option<&Vec<String>>,
    ) -> Result<()>;
    fn get_user_agent_access(&self, user_id: &str) -> Result<Option<UserAgentAccessRecord>>;
    fn set_user_agent_access(
        &self,
        user_id: &str,
        allowed_agent_ids: Option<&Vec<String>>,
        blocked_agent_ids: Option<&Vec<String>>,
    ) -> Result<()>;
    fn upsert_user_agent(&self, record: &UserAgentRecord) -> Result<()>;
    fn get_user_agent(&self, user_id: &str, agent_id: &str) -> Result<Option<UserAgentRecord>>;
    fn get_user_agent_by_id(&self, agent_id: &str) -> Result<Option<UserAgentRecord>>;
    fn list_user_agents(&self, user_id: &str) -> Result<Vec<UserAgentRecord>>;
    fn list_shared_user_agents(&self, user_id: &str) -> Result<Vec<UserAgentRecord>>;
    fn delete_user_agent(&self, user_id: &str, agent_id: &str) -> Result<i64>;

    fn consume_user_quota(&self, user_id: &str, today: &str) -> Result<Option<UserQuotaStatus>>;
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
            config.postgres.pool_size,
        )?)),
        other => Err(anyhow!("未知存储后端: {other}")),
    }
}
