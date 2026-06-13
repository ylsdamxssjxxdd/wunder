use crate::schemas::AbilityDescriptor;
use crate::storage::{
    normalize_hive_id, normalize_sandbox_container_id, AgentTaskRecord, AgentThreadRecord,
    BeeroomChatMessageRecord, BridgeCenterAccountRecord, BridgeCenterRecord,
    BridgeDeliveryLogRecord, BridgeRouteAuditLogRecord, BridgeUserRouteRecord,
    ChannelAccountRecord, ChannelBindingRecord, ChannelMessageRecord, ChannelMessageStats,
    ChannelOutboxRecord, ChannelOutboxStats, ChannelSessionRecord, ChannelUserBindingRecord,
    ChatSessionRecord, CronJobRecord, CronRunRecord, ExternalLinkRecord, GatewayClientRecord,
    GatewayNodeRecord, GatewayNodeTokenRecord, HiveRecord, ListBridgeCenterAccountsQuery,
    ListBridgeCentersQuery, ListBridgeDeliveryLogsQuery, ListBridgeRouteAuditLogsQuery,
    ListBridgeUserRoutesQuery, ListChannelUserBindingsQuery, MediaAssetRecord,
    MemoryFragmentEmbeddingRecord, MemoryFragmentRecord, MemoryHitRecord, MemoryJobRecord,
    OrgUnitRecord, SessionGoalRecord, SessionLockRecord, SessionLockStatus, SessionRunRecord,
    SpeechJobRecord, StorageBackend, TeamRunRecord, TeamTaskRecord, UpdateAgentTaskStatusParams,
    UpdateChannelOutboxStatusParams, UpsertMemoryTaskLogParams, UserAccountRecord,
    UserAgentAccessRecord, UserAgentPresetBinding, UserAgentRecord, UserExperienceUpdateResult,
    UserSessionScopeRecord, UserTokenBalanceStatus, UserTokenRecord, UserToolAccessRecord,
    UserWorldConversationRecord, UserWorldConversationSummaryRecord, UserWorldEventRecord,
    UserWorldGroupRecord, UserWorldMemberRecord, UserWorldMessageRecord, UserWorldReadResult,
    UserWorldSendMessageResult, VectorDocumentRecord, VectorDocumentSummaryRecord,
};
use anyhow::{anyhow, Result};
use chrono::Utc;
use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod};
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::AtomicBool;
use std::sync::OnceLock;
use std::time::Duration;
use tokio_postgres::types::ToSql;
use tokio_postgres::NoTls;

mod agent_directory_store;
mod agent_runtime_store;
mod benchmark_store;
mod bridge_store;
mod channel_directory;
mod channel_runtime;
mod chat_session;
mod conversation_log_store;
mod cron;
mod gateway_store;
mod log_stats_store;
mod media_store;
mod memory_store;
mod meta_store;
mod monitor_store;
mod retention_store;
mod schema;
mod session_goal;
mod session_lock_store;
mod session_run;
mod token_balance_store;
mod user_account_store;
mod user_world_store;
mod vector_document_store;

use agent_directory_store::PostgresAgentDirectoryStorage;
use agent_runtime_store::PostgresAgentRuntimeStorage;
use benchmark_store::PostgresBenchmarkStorage;
use bridge_store::PostgresBridgeStorage;
use channel_directory::PostgresChannelDirectoryStorage;
use channel_runtime::PostgresChannelRuntimeStorage;
use chat_session::PostgresChatSessionStorage;
use conversation_log_store::PostgresConversationLogStorage;
use cron::PostgresCronStorage;
use gateway_store::PostgresGatewayStorage;
use log_stats_store::PostgresLogStatsStorage;
use media_store::PostgresMediaStorage;
use memory_store::PostgresMemoryStorage;
use meta_store::PostgresMetaStorage;
use monitor_store::PostgresMonitorStorage;
use retention_store::PostgresRetentionStorage;
use schema::PostgresSchemaStorage;
use session_goal::PostgresSessionGoalStorage;
use session_lock_store::PostgresSessionLockStorage;
use session_run::PostgresSessionRunStorage;
use token_balance_store::PostgresTokenBalanceStorage;
use user_account_store::PostgresUserAccountStorage;
use user_world_store::PostgresUserWorldStorage;
use vector_document_store::PostgresVectorDocumentStorage;

const DEFAULT_POOL_SIZE: usize = 64;

fn postgres_fallback_runtime() -> Result<&'static tokio::runtime::Runtime> {
    static RUNTIME: OnceLock<Result<tokio::runtime::Runtime, String>> = OnceLock::new();
    match RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("postgres-storage-fallback")
            .build()
            .map_err(|err| format!("create tokio runtime for postgres: {err}"))
    }) {
        Ok(runtime) => Ok(runtime),
        Err(err) => Err(anyhow!(err.clone())),
    }
}

pub struct PostgresStorage {
    pool: Pool,
    initialized: AtomicBool,
    init_guard: Mutex<()>,
}

struct PgConn<'a> {
    storage: &'a PostgresStorage,
    client: deadpool_postgres::Client,
}

impl PgConn<'_> {
    fn batch_execute(&mut self, query: &str) -> Result<()> {
        self.storage.block_on(self.client.batch_execute(query))??;
        Ok(())
    }

    fn execute(&mut self, query: &str, params: &[&(dyn ToSql + Sync)]) -> Result<u64> {
        Ok(self
            .storage
            .block_on(self.client.execute(query, params))??)
    }

    fn query(
        &mut self,
        query: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Vec<tokio_postgres::Row>> {
        Ok(self.storage.block_on(self.client.query(query, params))??)
    }

    fn query_one(
        &mut self,
        query: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<tokio_postgres::Row> {
        Ok(self
            .storage
            .block_on(self.client.query_one(query, params))??)
    }

    fn query_opt(
        &mut self,
        query: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Option<tokio_postgres::Row>> {
        Ok(self
            .storage
            .block_on(self.client.query_opt(query, params))??)
    }

    fn transaction<'a>(&'a mut self) -> Result<PgTx<'a>> {
        let tx = self.storage.block_on(self.client.transaction())??;
        Ok(PgTx {
            storage: self.storage,
            tx,
        })
    }
}

struct PgTx<'a> {
    storage: &'a PostgresStorage,
    tx: deadpool_postgres::Transaction<'a>,
}

impl PgTx<'_> {
    fn execute(&mut self, query: &str, params: &[&(dyn ToSql + Sync)]) -> Result<u64> {
        Ok(self.storage.block_on(self.tx.execute(query, params))??)
    }

    fn query(
        &mut self,
        query: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Vec<tokio_postgres::Row>> {
        Ok(self.storage.block_on(self.tx.query(query, params))??)
    }

    fn query_one(
        &mut self,
        query: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<tokio_postgres::Row> {
        Ok(self.storage.block_on(self.tx.query_one(query, params))??)
    }

    fn query_opt(
        &mut self,
        query: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Option<tokio_postgres::Row>> {
        Ok(self.storage.block_on(self.tx.query_opt(query, params))??)
    }

    fn commit(self) -> Result<()> {
        self.storage.block_on(self.tx.commit())??;
        Ok(())
    }
}

impl PostgresStorage {
    pub fn new(dsn: String, connect_timeout_s: u64, pool_size: usize) -> Result<Self> {
        let cleaned = dsn.trim().to_string();
        if cleaned.is_empty() {
            return Err(anyhow!("postgres dsn is empty"));
        }
        let timeout = Duration::from_secs(connect_timeout_s.max(1));
        let pool_size = if pool_size == 0 {
            DEFAULT_POOL_SIZE
        } else {
            pool_size
        };
        let mut config = cleaned.parse::<tokio_postgres::Config>()?;
        config.connect_timeout(timeout);
        let manager_config = ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        };
        let manager = Manager::from_config(config, NoTls, manager_config);
        let pool = Pool::builder(manager).max_size(pool_size).build()?;
        Ok(Self {
            pool,
            initialized: AtomicBool::new(false),
            init_guard: Mutex::new(()),
        })
    }

    fn block_on<F, T>(&self, fut: F) -> Result<T>
    where
        F: Future<Output = T>,
    {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => Ok(tokio::task::block_in_place(|| handle.block_on(fut))),
            Err(_) => Ok(postgres_fallback_runtime()?.block_on(fut)),
        }
    }

    fn now_ts() -> f64 {
        Utc::now().timestamp_millis() as f64 / 1000.0
    }

    fn json_to_string(value: &Value) -> String {
        serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
    }

    fn json_from_str(text: &str) -> Option<Value> {
        if text.trim().is_empty() {
            return None;
        }
        serde_json::from_str::<Value>(text).ok()
    }

    fn parse_string(value: Option<&Value>) -> Option<String> {
        match value {
            Some(Value::String(text)) => Some(text.clone()),
            Some(other) => Some(other.to_string()),
            None => None,
        }
    }

    fn parse_bool(value: Option<&Value>) -> Option<i32> {
        match value {
            Some(Value::Bool(flag)) => Some(if *flag { 1 } else { 0 }),
            Some(Value::Number(num)) => num.as_i64().map(|value| value as i32),
            Some(Value::String(text)) => text.parse::<i32>().ok(),
            _ => None,
        }
    }

    fn parse_f64(value: Option<&Value>) -> Option<f64> {
        match value {
            Some(Value::Number(num)) => num.as_f64(),
            Some(Value::String(text)) => text.parse::<f64>().ok(),
            Some(Value::Bool(flag)) => Some(if *flag { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    fn read_compat_bool(row: &tokio_postgres::Row, index: usize) -> bool {
        if let Ok(value) = row.try_get::<_, bool>(index) {
            return value;
        }
        if let Ok(value) = row.try_get::<_, Option<bool>>(index) {
            return value.unwrap_or(false);
        }
        if let Ok(value) = row.try_get::<_, i16>(index) {
            return value != 0;
        }
        if let Ok(value) = row.try_get::<_, Option<i16>>(index) {
            return value.unwrap_or(0) != 0;
        }
        if let Ok(value) = row.try_get::<_, i32>(index) {
            return value != 0;
        }
        if let Ok(value) = row.try_get::<_, Option<i32>>(index) {
            return value.unwrap_or(0) != 0;
        }
        if let Ok(value) = row.try_get::<_, i64>(index) {
            return value != 0;
        }
        if let Ok(value) = row.try_get::<_, Option<i64>>(index) {
            return value.unwrap_or(0) != 0;
        }
        if let Ok(value) = row.try_get::<_, String>(index) {
            let lowered = value.trim().to_ascii_lowercase();
            return matches!(lowered.as_str(), "1" | "t" | "true" | "yes" | "y" | "on");
        }
        if let Ok(value) = row.try_get::<_, Option<String>>(index) {
            let lowered = value.unwrap_or_default().trim().to_ascii_lowercase();
            return matches!(lowered.as_str(), "1" | "t" | "true" | "yes" | "y" | "on");
        }
        false
    }

    fn parse_string_list(value: Option<String>) -> Vec<String> {
        let Some(raw) = value else {
            return Vec::new();
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        if let Ok(items) = serde_json::from_str::<Vec<String>>(trimmed) {
            return items
                .into_iter()
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect();
        }
        trimmed
            .split(',')
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect()
    }

    fn json_to_f32_vec(text: &str) -> Vec<f32> {
        serde_json::from_str::<Vec<f32>>(text).unwrap_or_default()
    }

    fn parse_i32_list(value: Option<String>) -> Vec<i32> {
        let Some(raw) = value else {
            return Vec::new();
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        if let Ok(items) = serde_json::from_str::<Vec<i32>>(trimmed) {
            return items.into_iter().filter(|item| *item > 0).collect();
        }
        trimmed
            .split(',')
            .filter_map(|item| item.trim().parse::<i32>().ok())
            .filter(|item| *item > 0)
            .collect()
    }

    fn string_list_to_json(list: &[String]) -> String {
        serde_json::to_string(list).unwrap_or_else(|_| "[]".to_string())
    }

    fn parse_declared_tool_names(value: Option<String>) -> Vec<String> {
        Self::parse_string_list(value)
    }

    fn parse_ability_items(value: Option<String>) -> Vec<AbilityDescriptor> {
        value
            .as_deref()
            .map(str::trim)
            .filter(|raw| !raw.is_empty())
            .and_then(|raw| serde_json::from_str::<Vec<AbilityDescriptor>>(raw).ok())
            .unwrap_or_default()
    }

    fn parse_preset_binding(value: Option<String>) -> Option<UserAgentPresetBinding> {
        value
            .as_deref()
            .map(str::trim)
            .filter(|raw| !raw.is_empty())
            .and_then(|raw| serde_json::from_str::<UserAgentPresetBinding>(raw).ok())
    }

    fn read_user_agent_row(row: &tokio_postgres::Row) -> UserAgentRecord {
        let tool_names = Self::parse_string_list(row.get(7));
        UserAgentRecord {
            agent_id: row.get(0),
            user_id: row.get(1),
            hive_id: normalize_hive_id(&row.get::<_, String>(2)),
            name: row.get(3),
            description: row.get::<_, Option<String>>(4).unwrap_or_default(),
            system_prompt: row.get::<_, Option<String>>(5).unwrap_or_default(),
            preview_skill: row.get::<_, i32>(23) != 0,
            model_name: row.get::<_, Option<String>>(6),
            ability_items: Self::parse_ability_items(row.get(10)),
            tool_names: tool_names.clone(),
            declared_tool_names: Self::parse_declared_tool_names(row.get(8)),
            declared_skill_names: Self::parse_string_list(row.get(9)),
            visible_unit_ids: Self::parse_string_list(row.get(24)),
            preset_questions: Self::parse_string_list(row.get(19)),
            access_level: row.get(11),
            approval_mode: row.get(12),
            is_shared: row.get::<_, i32>(13) != 0,
            status: row.get(14),
            icon: row.get(15),
            sandbox_container_id: normalize_sandbox_container_id(row.get::<_, i32>(16)),
            created_at: row.get(17),
            updated_at: row.get(18),
            preset_binding: Self::parse_preset_binding(row.get(20)),
            silent: row.get::<_, i32>(21) != 0,
            prefer_mother: row.get::<_, i32>(22) != 0,
        }
    }

    fn i32_list_to_json(list: &[i32]) -> String {
        serde_json::to_string(list).unwrap_or_else(|_| "[]".to_string())
    }

    fn json_value_or_null(value: Option<String>) -> Value {
        value
            .as_deref()
            .and_then(Self::json_from_str)
            .unwrap_or(Value::Null)
    }

    fn normalize_channel_thread_id(value: Option<&str>) -> String {
        value.unwrap_or("").trim().to_string()
    }

    fn normalize_channel_thread_value(value: Option<String>) -> Option<String> {
        value
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
    }

    fn conn(&self) -> Result<PgConn<'_>> {
        let client = self.block_on(self.pool.get())??;
        Ok(PgConn {
            storage: self,
            client,
        })
    }
}

impl StorageBackend for PostgresStorage {
    fn ensure_initialized(&self) -> Result<()> {
        self.ensure_initialized_impl()
    }

    fn get_meta(&self, key: &str) -> Result<Option<String>> {
        self.get_meta_impl(key)
    }

    fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.set_meta_impl(key, value)
    }

    fn list_meta_prefix(&self, prefix: &str) -> Result<Vec<(String, String)>> {
        self.list_meta_prefix_impl(prefix)
    }

    fn delete_meta_prefix(&self, prefix: &str) -> Result<usize> {
        self.delete_meta_prefix_impl(prefix)
    }

    fn append_chat(&self, user_id: &str, payload: &Value) -> Result<()> {
        self.append_chat_impl(user_id, payload)
    }

    fn append_model_context_entry(
        &self,
        user_id: &str,
        session_id: &str,
        payload: &Value,
    ) -> Result<()> {
        self.append_model_context_entry_impl(user_id, session_id, payload)
    }

    fn replace_model_context_entries(
        &self,
        user_id: &str,
        session_id: &str,
        payloads: &[Value],
    ) -> Result<()> {
        self.replace_model_context_entries_impl(user_id, session_id, payloads)
    }

    fn append_tool_log(&self, user_id: &str, payload: &Value) -> Result<()> {
        self.append_tool_log_impl(user_id, payload)
    }

    fn append_artifact_log(&self, user_id: &str, payload: &Value) -> Result<()> {
        self.append_artifact_log_impl(user_id, payload)
    }

    fn load_model_context_entries(
        &self,
        user_id: &str,
        session_id: &str,
        limit: Option<i64>,
    ) -> Result<Vec<Value>> {
        self.load_model_context_entries_impl(user_id, session_id, limit)
    }

    fn load_chat_history(
        &self,
        user_id: &str,
        session_id: &str,
        limit: Option<i64>,
    ) -> Result<Vec<Value>> {
        self.load_chat_history_impl(user_id, session_id, limit)
    }

    fn load_chat_history_page(
        &self,
        user_id: &str,
        session_id: &str,
        before_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.load_chat_history_page_impl(user_id, session_id, before_id, limit)
    }

    fn load_artifact_logs(
        &self,
        user_id: &str,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.load_artifact_logs_impl(user_id, session_id, limit)
    }

    fn get_session_system_prompt(
        &self,
        user_id: &str,
        session_id: &str,
        language: Option<&str>,
    ) -> Result<Option<String>> {
        self.get_session_system_prompt_impl(user_id, session_id, language)
    }

    fn get_user_chat_stats(&self) -> Result<HashMap<String, HashMap<String, i64>>> {
        self.get_user_chat_stats_impl()
    }

    fn get_user_tool_stats(&self) -> Result<HashMap<String, HashMap<String, i64>>> {
        self.get_user_tool_stats_impl()
    }

    fn get_tool_usage_stats(
        &self,
        since_time: Option<f64>,
        until_time: Option<f64>,
    ) -> Result<HashMap<String, i64>> {
        self.get_tool_usage_stats_impl(since_time, until_time)
    }

    fn get_tool_session_usage(
        &self,
        tool: &str,
        since_time: Option<f64>,
        until_time: Option<f64>,
    ) -> Result<Vec<HashMap<String, Value>>> {
        self.get_tool_session_usage_impl(tool, since_time, until_time)
    }

    fn get_log_usage(&self) -> Result<u64> {
        self.get_log_usage_impl()
    }

    fn delete_chat_history(&self, user_id: &str) -> Result<i64> {
        self.delete_chat_history_impl(user_id)
    }

    fn delete_chat_history_by_session(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.delete_chat_history_by_session_impl(user_id, session_id)
    }

    fn delete_tool_logs(&self, user_id: &str) -> Result<i64> {
        self.delete_tool_logs_impl(user_id)
    }

    fn delete_tool_logs_by_session(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.delete_tool_logs_by_session_impl(user_id, session_id)
    }

    fn delete_artifact_logs(&self, user_id: &str) -> Result<i64> {
        self.delete_artifact_logs_impl(user_id)
    }

    fn delete_artifact_logs_by_session(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.delete_artifact_logs_by_session_impl(user_id, session_id)
    }

    fn upsert_monitor_record(&self, payload: &Value) -> Result<()> {
        self.upsert_monitor_record_impl(payload)
    }

    fn get_monitor_record(&self, session_id: &str) -> Result<Option<Value>> {
        self.get_monitor_record_impl(session_id)
    }

    fn load_monitor_records(&self) -> Result<Vec<Value>> {
        self.load_monitor_records_impl()
    }

    fn load_recent_monitor_records(&self, limit: i64) -> Result<Vec<Value>> {
        self.load_recent_monitor_records_impl(limit)
    }

    fn load_monitor_records_by_user(
        &self,
        user_id: &str,
        statuses: Option<&[&str]>,
        since_time: Option<f64>,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.load_monitor_records_by_user_impl(user_id, statuses, since_time, limit)
    }

    fn delete_monitor_record(&self, session_id: &str) -> Result<()> {
        self.delete_monitor_record_impl(session_id)
    }

    fn delete_monitor_records_by_user(&self, user_id: &str) -> Result<i64> {
        self.delete_monitor_records_by_user_impl(user_id)
    }

    fn try_acquire_session_lock(
        &self,
        session_id: &str,
        user_id: &str,
        agent_id: &str,
        ttl_s: f64,
        max_sessions: i64,
    ) -> Result<SessionLockStatus> {
        self.try_acquire_session_lock_impl(session_id, user_id, agent_id, ttl_s, max_sessions)
    }

    fn touch_session_lock(&self, session_id: &str, ttl_s: f64) -> Result<()> {
        self.touch_session_lock_impl(session_id, ttl_s)
    }

    fn release_session_lock(&self, session_id: &str) -> Result<()> {
        self.release_session_lock_impl(session_id)
    }

    fn delete_session_locks_by_user(&self, user_id: &str) -> Result<i64> {
        self.delete_session_locks_by_user_impl(user_id)
    }

    fn count_session_locks(&self) -> Result<i64> {
        self.count_session_locks_impl()
    }

    fn list_session_locks_by_user(&self, user_id: &str) -> Result<Vec<SessionLockRecord>> {
        self.list_session_locks_by_user_impl(user_id)
    }

    fn upsert_agent_thread(&self, record: &AgentThreadRecord) -> Result<()> {
        self.upsert_agent_thread_impl(record)
    }

    fn get_agent_thread(&self, user_id: &str, agent_id: &str) -> Result<Option<AgentThreadRecord>> {
        self.get_agent_thread_impl(user_id, agent_id)
    }

    fn delete_agent_thread(&self, user_id: &str, agent_id: &str) -> Result<i64> {
        self.delete_agent_thread_impl(user_id, agent_id)
    }

    fn insert_agent_task(&self, record: &AgentTaskRecord) -> Result<()> {
        self.insert_agent_task_impl(record)
    }

    fn get_agent_task(&self, task_id: &str) -> Result<Option<AgentTaskRecord>> {
        self.get_agent_task_impl(task_id)
    }

    fn list_pending_agent_tasks(&self, limit: i64) -> Result<Vec<AgentTaskRecord>> {
        self.list_pending_agent_tasks_impl(limit)
    }

    fn count_pending_agent_tasks(&self) -> Result<i64> {
        self.count_pending_agent_tasks_impl()
    }

    fn count_pending_agent_tasks_ahead(
        &self,
        retry_at: f64,
        created_at: f64,
        task_id: &str,
    ) -> Result<i64> {
        self.count_pending_agent_tasks_ahead_impl(retry_at, created_at, task_id)
    }

    fn list_agent_tasks_by_thread(
        &self,
        thread_id: &str,
        status: Option<&str>,
        limit: i64,
    ) -> Result<Vec<AgentTaskRecord>> {
        self.list_agent_tasks_by_thread_impl(thread_id, status, limit)
    }

    fn update_agent_task_status(&self, params: UpdateAgentTaskStatusParams<'_>) -> Result<()> {
        self.update_agent_task_status_impl(params)
    }

    fn get_max_stream_event_id(&self, session_id: &str) -> Result<i64> {
        self.get_max_stream_event_id_impl(session_id)
    }

    fn append_stream_event(
        &self,
        session_id: &str,
        user_id: &str,
        event_id: i64,
        payload: &Value,
    ) -> Result<()> {
        self.append_stream_event_impl(session_id, user_id, event_id, payload)
    }

    fn load_stream_events(
        &self,
        session_id: &str,
        after_event_id: i64,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.load_stream_events_impl(session_id, after_event_id, limit)
    }

    fn load_recent_stream_events(&self, session_id: &str, limit: i64) -> Result<Vec<Value>> {
        self.load_recent_stream_events_impl(session_id, limit)
    }

    fn delete_stream_events_before(&self, before_time: f64) -> Result<i64> {
        self.delete_stream_events_before_impl(before_time)
    }

    fn delete_stream_events_by_user(&self, user_id: &str) -> Result<i64> {
        self.delete_stream_events_by_user_impl(user_id)
    }

    fn delete_stream_events_by_session(&self, session_id: &str) -> Result<i64> {
        self.delete_stream_events_by_session_impl(session_id)
    }

    fn get_memory_enabled(&self, user_id: &str) -> Result<Option<bool>> {
        self.get_memory_enabled_impl(user_id)
    }

    fn set_memory_enabled(&self, user_id: &str, enabled: bool) -> Result<()> {
        self.set_memory_enabled_impl(user_id, enabled)
    }

    fn load_memory_settings(&self) -> Result<Vec<HashMap<String, Value>>> {
        self.load_memory_settings_impl()
    }

    fn upsert_memory_record(
        &self,
        user_id: &str,
        session_id: &str,
        summary: &str,
        max_records: i64,
        now_ts: f64,
    ) -> Result<()> {
        self.upsert_memory_record_impl(user_id, session_id, summary, max_records, now_ts)
    }

    fn load_memory_records(
        &self,
        user_id: &str,
        limit: i64,
        order_desc: bool,
    ) -> Result<Vec<HashMap<String, Value>>> {
        self.load_memory_records_impl(user_id, limit, order_desc)
    }

    fn get_memory_record_stats(&self) -> Result<Vec<HashMap<String, Value>>> {
        self.get_memory_record_stats_impl()
    }

    fn delete_memory_record(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.delete_memory_record_impl(user_id, session_id)
    }

    fn delete_memory_records_by_user(&self, user_id: &str) -> Result<i64> {
        self.delete_memory_records_by_user_impl(user_id)
    }

    fn delete_memory_settings_by_user(&self, user_id: &str) -> Result<i64> {
        self.delete_memory_settings_by_user_impl(user_id)
    }

    fn upsert_memory_task_log(&self, params: UpsertMemoryTaskLogParams<'_>) -> Result<()> {
        self.upsert_memory_task_log_impl(params)
    }

    fn load_memory_task_logs(&self, limit: Option<i64>) -> Result<Vec<HashMap<String, Value>>> {
        self.load_memory_task_logs_impl(limit)
    }

    fn load_memory_task_log_by_task_id(
        &self,
        task_id: &str,
    ) -> Result<Option<HashMap<String, Value>>> {
        self.load_memory_task_log_by_task_id_impl(task_id)
    }

    fn delete_memory_task_log(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.delete_memory_task_log_impl(user_id, session_id)
    }

    fn delete_memory_task_logs_by_user(&self, user_id: &str) -> Result<i64> {
        self.delete_memory_task_logs_by_user_impl(user_id)
    }

    fn upsert_memory_fragment(&self, record: &MemoryFragmentRecord) -> Result<()> {
        self.upsert_memory_fragment_impl(record)
    }

    fn get_memory_fragment(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
    ) -> Result<Option<MemoryFragmentRecord>> {
        self.get_memory_fragment_impl(user_id, agent_id, memory_id)
    }

    fn list_memory_fragments(
        &self,
        user_id: &str,
        agent_id: &str,
    ) -> Result<Vec<MemoryFragmentRecord>> {
        self.list_memory_fragments_impl(user_id, agent_id)
    }

    fn get_memory_fragment_embedding(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
        embedding_model: &str,
        content_hash: &str,
    ) -> Result<Option<MemoryFragmentEmbeddingRecord>> {
        self.get_memory_fragment_embedding_impl(
            user_id,
            agent_id,
            memory_id,
            embedding_model,
            content_hash,
        )
    }

    fn upsert_memory_fragment_embedding(
        &self,
        record: &MemoryFragmentEmbeddingRecord,
    ) -> Result<()> {
        self.upsert_memory_fragment_embedding_impl(record)
    }

    fn delete_memory_fragment_embeddings(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
    ) -> Result<i64> {
        self.delete_memory_fragment_embeddings_impl(user_id, agent_id, memory_id)
    }

    fn delete_memory_fragment(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
    ) -> Result<i64> {
        self.delete_memory_fragment_impl(user_id, agent_id, memory_id)
    }

    fn insert_memory_hit(&self, record: &MemoryHitRecord) -> Result<()> {
        self.insert_memory_hit_impl(record)
    }

    fn list_memory_hits(
        &self,
        user_id: &str,
        agent_id: &str,
        session_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<MemoryHitRecord>> {
        self.list_memory_hits_impl(user_id, agent_id, session_id, limit)
    }

    fn list_memory_hit_counts(
        &self,
        user_id: &str,
        agent_id: &str,
    ) -> Result<HashMap<String, i64>> {
        self.list_memory_hit_counts_impl(user_id, agent_id)
    }

    fn has_memory_hit_event(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_id: &str,
        session_id: &str,
        round_id: Option<&str>,
        query_text: Option<&str>,
    ) -> Result<bool> {
        self.has_memory_hit_event_impl(
            user_id, agent_id, memory_id, session_id, round_id, query_text,
        )
    }

    fn upsert_memory_job(&self, record: &MemoryJobRecord) -> Result<()> {
        self.upsert_memory_job_impl(record)
    }

    fn list_memory_jobs(
        &self,
        user_id: &str,
        agent_id: &str,
        limit: i64,
    ) -> Result<Vec<MemoryJobRecord>> {
        self.list_memory_jobs_impl(user_id, agent_id, limit)
    }
    fn create_benchmark_run(&self, payload: &Value) -> Result<()> {
        self.create_benchmark_run_impl(payload)
    }

    fn update_benchmark_run(&self, run_id: &str, payload: &Value) -> Result<()> {
        self.update_benchmark_run_impl(run_id, payload)
    }

    fn upsert_benchmark_attempt(&self, run_id: &str, payload: &Value) -> Result<()> {
        self.upsert_benchmark_attempt_impl(run_id, payload)
    }

    fn upsert_benchmark_task_aggregate(&self, run_id: &str, payload: &Value) -> Result<()> {
        self.upsert_benchmark_task_aggregate_impl(run_id, payload)
    }

    fn load_benchmark_runs(
        &self,
        user_id: Option<&str>,
        status: Option<&str>,
        model_name: Option<&str>,
        since_time: Option<f64>,
        until_time: Option<f64>,
        limit: Option<i64>,
    ) -> Result<Vec<Value>> {
        self.load_benchmark_runs_impl(user_id, status, model_name, since_time, until_time, limit)
    }

    fn load_benchmark_run(&self, run_id: &str) -> Result<Option<Value>> {
        self.load_benchmark_run_impl(run_id)
    }

    fn load_benchmark_attempts(&self, run_id: &str) -> Result<Vec<Value>> {
        self.load_benchmark_attempts_impl(run_id)
    }

    fn load_benchmark_task_aggregates(&self, run_id: &str) -> Result<Vec<Value>> {
        self.load_benchmark_task_aggregates_impl(run_id)
    }

    fn delete_benchmark_run(&self, run_id: &str) -> Result<i64> {
        self.delete_benchmark_run_impl(run_id)
    }
    fn cleanup_retention(&self, retention_days: i64) -> Result<HashMap<String, i64>> {
        self.cleanup_retention_impl(retention_days)
    }

    fn upsert_user_account(&self, record: &UserAccountRecord) -> Result<()> {
        self.upsert_user_account_impl(record)
    }

    fn get_user_account(&self, user_id: &str) -> Result<Option<UserAccountRecord>> {
        self.get_user_account_impl(user_id)
    }

    fn get_user_account_by_username(&self, username: &str) -> Result<Option<UserAccountRecord>> {
        self.get_user_account_by_username_impl(username)
    }

    fn get_user_account_by_email(&self, email: &str) -> Result<Option<UserAccountRecord>> {
        self.get_user_account_by_email_impl(email)
    }

    fn list_user_accounts(
        &self,
        keyword: Option<&str>,
        unit_ids: Option<&[String]>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserAccountRecord>, i64)> {
        self.list_user_accounts_impl(keyword, unit_ids, offset, limit)
    }

    fn add_user_experience(
        &self,
        user_id: &str,
        delta: i64,
        updated_at: f64,
    ) -> Result<UserExperienceUpdateResult> {
        self.add_user_experience_impl(user_id, delta, updated_at)
    }

    fn delete_user_account(&self, user_id: &str) -> Result<i64> {
        self.delete_user_account_impl(user_id)
    }

    fn list_org_units(&self) -> Result<Vec<OrgUnitRecord>> {
        self.list_org_units_impl()
    }

    fn get_org_unit(&self, unit_id: &str) -> Result<Option<OrgUnitRecord>> {
        self.get_org_unit_impl(unit_id)
    }

    fn upsert_org_unit(&self, record: &OrgUnitRecord) -> Result<()> {
        self.upsert_org_unit_impl(record)
    }

    fn upsert_user_accounts(&self, records: &[UserAccountRecord]) -> Result<()> {
        self.upsert_user_accounts_impl(records)
    }

    fn delete_org_unit(&self, unit_id: &str) -> Result<i64> {
        self.delete_org_unit_impl(unit_id)
    }

    fn upsert_external_link(&self, record: &ExternalLinkRecord) -> Result<()> {
        self.upsert_external_link_impl(record)
    }

    fn get_external_link(&self, link_id: &str) -> Result<Option<ExternalLinkRecord>> {
        self.get_external_link_impl(link_id)
    }

    fn list_external_links(&self, include_disabled: bool) -> Result<Vec<ExternalLinkRecord>> {
        self.list_external_links_impl(include_disabled)
    }

    fn delete_external_link(&self, link_id: &str) -> Result<i64> {
        self.delete_external_link_impl(link_id)
    }

    fn create_user_token(&self, record: &UserTokenRecord) -> Result<()> {
        self.create_user_token_impl(record)
    }

    fn get_user_token(&self, token: &str) -> Result<Option<UserTokenRecord>> {
        self.get_user_token_impl(token)
    }

    fn touch_user_token(&self, token: &str, last_used_at: f64) -> Result<()> {
        self.touch_user_token_impl(token, last_used_at)
    }

    fn delete_user_token(&self, token: &str) -> Result<i64> {
        self.delete_user_token_impl(token)
    }

    fn upsert_user_session_scope(&self, record: &UserSessionScopeRecord) -> Result<()> {
        self.upsert_user_session_scope_impl(record)
    }

    fn get_user_session_scope(
        &self,
        user_id: &str,
        session_scope: &str,
    ) -> Result<Option<UserSessionScopeRecord>> {
        self.get_user_session_scope_impl(user_id, session_scope)
    }

    fn upsert_chat_session(&self, record: &ChatSessionRecord) -> Result<()> {
        self.upsert_chat_session_impl(record)
    }

    fn get_chat_session(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<Option<ChatSessionRecord>> {
        self.get_chat_session_impl(user_id, session_id)
    }

    fn list_chat_sessions(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        parent_session_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChatSessionRecord>, i64)> {
        self.list_chat_sessions_impl(user_id, agent_id, parent_session_id, offset, limit)
    }

    fn list_chat_sessions_by_status(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        parent_session_id: Option<&str>,
        status: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChatSessionRecord>, i64)> {
        self.list_chat_sessions_by_status_impl(
            user_id,
            agent_id,
            parent_session_id,
            status,
            offset,
            limit,
        )
    }

    fn list_chat_session_agent_ids(&self, user_id: &str) -> Result<Vec<String>> {
        self.list_chat_session_agent_ids_impl(user_id)
    }

    fn update_chat_session_title(
        &self,
        user_id: &str,
        session_id: &str,
        title: &str,
        updated_at: f64,
    ) -> Result<()> {
        self.update_chat_session_title_impl(user_id, session_id, title, updated_at)
    }

    fn touch_chat_session(
        &self,
        user_id: &str,
        session_id: &str,
        updated_at: f64,
        last_message_at: f64,
    ) -> Result<()> {
        self.touch_chat_session_impl(user_id, session_id, updated_at, last_message_at)
    }

    fn delete_chat_session(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.delete_chat_session_impl(user_id, session_id)
    }

    fn upsert_session_goal(&self, record: &SessionGoalRecord) -> Result<()> {
        self.upsert_session_goal_impl(record)
    }

    fn get_session_goal(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<Option<SessionGoalRecord>> {
        self.get_session_goal_impl(user_id, session_id)
    }

    fn list_session_goals(
        &self,
        user_id: &str,
        session_ids: &[String],
    ) -> Result<Vec<SessionGoalRecord>> {
        self.list_session_goals_impl(user_id, session_ids)
    }

    fn delete_session_goal(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.delete_session_goal_impl(user_id, session_id)
    }

    fn account_session_goal_usage(
        &self,
        user_id: &str,
        session_id: &str,
        tokens_delta: i64,
        time_delta_seconds: i64,
        updated_at: f64,
    ) -> Result<Option<SessionGoalRecord>> {
        self.account_session_goal_usage_impl(
            user_id,
            session_id,
            tokens_delta,
            time_delta_seconds,
            updated_at,
        )
    }

    fn resolve_or_create_user_world_direct_conversation(
        &self,
        user_a: &str,
        user_b: &str,
        now: f64,
    ) -> Result<UserWorldConversationRecord> {
        self.resolve_or_create_user_world_direct_conversation_impl(user_a, user_b, now)
    }

    fn create_user_world_group(
        &self,
        owner_user_id: &str,
        group_name: &str,
        member_user_ids: &[String],
        now: f64,
    ) -> Result<UserWorldConversationRecord> {
        self.create_user_world_group_impl(owner_user_id, group_name, member_user_ids, now)
    }

    fn get_user_world_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Option<UserWorldConversationRecord>> {
        self.get_user_world_conversation_impl(conversation_id)
    }

    fn get_user_world_member(
        &self,
        conversation_id: &str,
        user_id: &str,
    ) -> Result<Option<UserWorldMemberRecord>> {
        self.get_user_world_member_impl(conversation_id, user_id)
    }

    fn list_user_world_conversations(
        &self,
        user_id: &str,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserWorldConversationSummaryRecord>, i64)> {
        self.list_user_world_conversations_impl(user_id, offset, limit)
    }

    fn list_user_world_messages(
        &self,
        conversation_id: &str,
        before_message_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<UserWorldMessageRecord>> {
        self.list_user_world_messages_impl(conversation_id, before_message_id, limit)
    }

    fn send_user_world_message(
        &self,
        conversation_id: &str,
        sender_user_id: &str,
        content: &str,
        content_type: &str,
        client_msg_id: Option<&str>,
        now: f64,
    ) -> Result<UserWorldSendMessageResult> {
        self.send_user_world_message_impl(
            conversation_id,
            sender_user_id,
            content,
            content_type,
            client_msg_id,
            now,
        )
    }

    fn mark_user_world_read(
        &self,
        conversation_id: &str,
        user_id: &str,
        last_read_message_id: Option<i64>,
        now: f64,
    ) -> Result<Option<UserWorldReadResult>> {
        self.mark_user_world_read_impl(conversation_id, user_id, last_read_message_id, now)
    }

    fn list_user_world_events(
        &self,
        conversation_id: &str,
        after_event_id: i64,
        limit: i64,
    ) -> Result<Vec<UserWorldEventRecord>> {
        self.list_user_world_events_impl(conversation_id, after_event_id, limit)
    }

    fn list_user_world_groups(
        &self,
        user_id: &str,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserWorldGroupRecord>, i64)> {
        self.list_user_world_groups_impl(user_id, offset, limit)
    }

    fn get_user_world_group_by_id(&self, group_id: &str) -> Result<Option<UserWorldGroupRecord>> {
        self.get_user_world_group_by_id_impl(group_id)
    }

    fn update_user_world_group_announcement(
        &self,
        group_id: &str,
        announcement: Option<&str>,
        announcement_updated_at: Option<f64>,
        updated_at: f64,
    ) -> Result<Option<UserWorldGroupRecord>> {
        self.update_user_world_group_announcement_impl(
            group_id,
            announcement,
            announcement_updated_at,
            updated_at,
        )
    }

    fn list_user_world_member_user_ids(&self, conversation_id: &str) -> Result<Vec<String>> {
        self.list_user_world_member_user_ids_impl(conversation_id)
    }

    fn list_beeroom_chat_messages(
        &self,
        user_id: &str,
        group_id: &str,
        before_message_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<BeeroomChatMessageRecord>> {
        self.list_beeroom_chat_messages_impl(user_id, group_id, before_message_id, limit)
    }

    fn append_beeroom_chat_message(
        &self,
        user_id: &str,
        group_id: &str,
        sender_kind: &str,
        sender_name: &str,
        sender_agent_id: Option<&str>,
        mention_name: Option<&str>,
        mention_agent_id: Option<&str>,
        body: &str,
        meta: Option<&str>,
        tone: &str,
        client_msg_id: Option<&str>,
        created_at: f64,
    ) -> Result<BeeroomChatMessageRecord> {
        self.append_beeroom_chat_message_impl(
            user_id,
            group_id,
            sender_kind,
            sender_name,
            sender_agent_id,
            mention_name,
            mention_agent_id,
            body,
            meta,
            tone,
            client_msg_id,
            created_at,
        )
    }

    fn delete_beeroom_chat_messages(&self, user_id: &str, group_id: &str) -> Result<i64> {
        self.delete_beeroom_chat_messages_impl(user_id, group_id)
    }
    fn upsert_channel_account(&self, record: &ChannelAccountRecord) -> Result<()> {
        self.upsert_channel_account_impl(record)
    }

    fn get_channel_account(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<Option<ChannelAccountRecord>> {
        self.get_channel_account_impl(channel, account_id)
    }

    fn list_channel_accounts(
        &self,
        channel: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<ChannelAccountRecord>> {
        self.list_channel_accounts_impl(channel, status)
    }

    fn delete_channel_account(&self, channel: &str, account_id: &str) -> Result<i64> {
        self.delete_channel_account_impl(channel, account_id)
    }

    fn upsert_channel_binding(&self, record: &ChannelBindingRecord) -> Result<()> {
        self.upsert_channel_binding_impl(record)
    }

    fn list_channel_bindings(&self, channel: Option<&str>) -> Result<Vec<ChannelBindingRecord>> {
        self.list_channel_bindings_impl(channel)
    }

    fn delete_channel_binding(&self, binding_id: &str) -> Result<i64> {
        self.delete_channel_binding_impl(binding_id)
    }

    fn upsert_channel_user_binding(&self, record: &ChannelUserBindingRecord) -> Result<()> {
        self.upsert_channel_user_binding_impl(record)
    }

    fn get_channel_user_binding(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
    ) -> Result<Option<ChannelUserBindingRecord>> {
        self.get_channel_user_binding_impl(channel, account_id, peer_kind, peer_id)
    }

    fn list_channel_user_bindings(
        &self,
        query: ListChannelUserBindingsQuery<'_>,
    ) -> Result<(Vec<ChannelUserBindingRecord>, i64)> {
        self.list_channel_user_bindings_impl(query)
    }

    fn delete_channel_user_binding(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
    ) -> Result<i64> {
        self.delete_channel_user_binding_impl(channel, account_id, peer_kind, peer_id)
    }

    fn upsert_channel_session(&self, record: &ChannelSessionRecord) -> Result<()> {
        self.upsert_channel_session_impl(record)
    }

    fn get_channel_session(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
        thread_id: Option<&str>,
    ) -> Result<Option<ChannelSessionRecord>> {
        self.get_channel_session_impl(channel, account_id, peer_kind, peer_id, thread_id)
    }

    fn list_channel_sessions(
        &self,
        channel: Option<&str>,
        account_id: Option<&str>,
        peer_id: Option<&str>,
        session_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChannelSessionRecord>, i64)> {
        self.list_channel_sessions_impl(channel, account_id, peer_id, session_id, offset, limit)
    }

    fn insert_channel_message(&self, record: &ChannelMessageRecord) -> Result<()> {
        self.insert_channel_message_impl(record)
    }

    fn list_channel_messages(
        &self,
        channel: Option<&str>,
        session_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<ChannelMessageRecord>> {
        self.list_channel_messages_impl(channel, session_id, limit)
    }

    fn get_channel_message_stats(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<ChannelMessageStats> {
        self.get_channel_message_stats_impl(channel, account_id)
    }

    fn get_channel_outbox_stats(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<ChannelOutboxStats> {
        self.get_channel_outbox_stats_impl(channel, account_id)
    }

    fn delete_channel_sessions(&self, channel: &str, account_id: &str) -> Result<i64> {
        self.delete_channel_sessions_impl(channel, account_id)
    }

    fn delete_channel_messages(&self, channel: &str, account_id: &str) -> Result<i64> {
        self.delete_channel_messages_impl(channel, account_id)
    }

    fn delete_channel_outbox(&self, channel: &str, account_id: &str) -> Result<i64> {
        self.delete_channel_outbox_impl(channel, account_id)
    }

    fn enqueue_channel_outbox(&self, record: &ChannelOutboxRecord) -> Result<()> {
        self.enqueue_channel_outbox_impl(record)
    }

    fn get_channel_outbox(&self, outbox_id: &str) -> Result<Option<ChannelOutboxRecord>> {
        self.get_channel_outbox_impl(outbox_id)
    }

    fn list_pending_channel_outbox(&self, limit: i64) -> Result<Vec<ChannelOutboxRecord>> {
        self.list_pending_channel_outbox_impl(limit)
    }

    fn update_channel_outbox_status(
        &self,
        params: UpdateChannelOutboxStatusParams<'_>,
    ) -> Result<()> {
        self.update_channel_outbox_status_impl(params)
    }

    fn upsert_bridge_center(&self, record: &BridgeCenterRecord) -> Result<()> {
        self.upsert_bridge_center_impl(record)
    }

    fn get_bridge_center(&self, center_id: &str) -> Result<Option<BridgeCenterRecord>> {
        self.get_bridge_center_impl(center_id)
    }

    fn get_bridge_center_by_code(&self, code: &str) -> Result<Option<BridgeCenterRecord>> {
        self.get_bridge_center_by_code_impl(code)
    }

    fn list_bridge_centers(
        &self,
        query: ListBridgeCentersQuery<'_>,
    ) -> Result<(Vec<BridgeCenterRecord>, i64)> {
        self.list_bridge_centers_impl(query)
    }

    fn delete_bridge_center(&self, center_id: &str) -> Result<i64> {
        self.delete_bridge_center_impl(center_id)
    }

    fn upsert_bridge_center_account(&self, record: &BridgeCenterAccountRecord) -> Result<()> {
        self.upsert_bridge_center_account_impl(record)
    }

    fn get_bridge_center_account(
        &self,
        center_account_id: &str,
    ) -> Result<Option<BridgeCenterAccountRecord>> {
        self.get_bridge_center_account_impl(center_account_id)
    }

    fn get_bridge_center_account_by_channel_account(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<Option<BridgeCenterAccountRecord>> {
        self.get_bridge_center_account_by_channel_account_impl(channel, account_id)
    }

    fn list_bridge_center_accounts(
        &self,
        query: ListBridgeCenterAccountsQuery<'_>,
    ) -> Result<(Vec<BridgeCenterAccountRecord>, i64)> {
        self.list_bridge_center_accounts_impl(query)
    }

    fn delete_bridge_center_account(&self, center_account_id: &str) -> Result<i64> {
        self.delete_bridge_center_account_impl(center_account_id)
    }

    fn delete_bridge_center_accounts_by_center(&self, center_id: &str) -> Result<i64> {
        self.delete_bridge_center_accounts_by_center_impl(center_id)
    }

    fn upsert_bridge_user_route(&self, record: &BridgeUserRouteRecord) -> Result<()> {
        self.upsert_bridge_user_route_impl(record)
    }

    fn get_bridge_user_route(&self, route_id: &str) -> Result<Option<BridgeUserRouteRecord>> {
        self.get_bridge_user_route_impl(route_id)
    }

    fn get_bridge_user_route_by_identity(
        &self,
        center_account_id: &str,
        external_identity_key: &str,
    ) -> Result<Option<BridgeUserRouteRecord>> {
        self.get_bridge_user_route_by_identity_impl(center_account_id, external_identity_key)
    }

    fn list_bridge_user_routes(
        &self,
        query: ListBridgeUserRoutesQuery<'_>,
    ) -> Result<(Vec<BridgeUserRouteRecord>, i64)> {
        self.list_bridge_user_routes_impl(query)
    }

    fn delete_bridge_user_route(&self, route_id: &str) -> Result<i64> {
        self.delete_bridge_user_route_impl(route_id)
    }

    fn delete_bridge_user_routes_by_center(&self, center_id: &str) -> Result<i64> {
        self.delete_bridge_user_routes_by_center_impl(center_id)
    }

    fn delete_bridge_user_routes_by_center_account(&self, center_account_id: &str) -> Result<i64> {
        self.delete_bridge_user_routes_by_center_account_impl(center_account_id)
    }

    fn insert_bridge_delivery_log(&self, record: &BridgeDeliveryLogRecord) -> Result<()> {
        self.insert_bridge_delivery_log_impl(record)
    }

    fn list_bridge_delivery_logs(
        &self,
        query: ListBridgeDeliveryLogsQuery<'_>,
    ) -> Result<Vec<BridgeDeliveryLogRecord>> {
        self.list_bridge_delivery_logs_impl(query)
    }

    fn delete_bridge_delivery_logs_by_center(&self, center_id: &str) -> Result<i64> {
        self.delete_bridge_delivery_logs_by_center_impl(center_id)
    }

    fn delete_bridge_delivery_logs_by_center_account(
        &self,
        center_account_id: &str,
    ) -> Result<i64> {
        self.delete_bridge_delivery_logs_by_center_account_impl(center_account_id)
    }

    fn insert_bridge_route_audit_log(&self, record: &BridgeRouteAuditLogRecord) -> Result<()> {
        self.insert_bridge_route_audit_log_impl(record)
    }

    fn list_bridge_route_audit_logs(
        &self,
        query: ListBridgeRouteAuditLogsQuery<'_>,
    ) -> Result<Vec<BridgeRouteAuditLogRecord>> {
        self.list_bridge_route_audit_logs_impl(query)
    }

    fn delete_bridge_route_audit_logs_by_center(&self, center_id: &str) -> Result<i64> {
        self.delete_bridge_route_audit_logs_by_center_impl(center_id)
    }

    fn delete_bridge_route_audit_logs_by_center_account(
        &self,
        center_account_id: &str,
    ) -> Result<i64> {
        self.delete_bridge_route_audit_logs_by_center_account_impl(center_account_id)
    }

    fn upsert_gateway_client(&self, record: &GatewayClientRecord) -> Result<()> {
        self.upsert_gateway_client_impl(record)
    }

    fn list_gateway_clients(&self, status: Option<&str>) -> Result<Vec<GatewayClientRecord>> {
        self.list_gateway_clients_impl(status)
    }

    fn upsert_gateway_node(&self, record: &GatewayNodeRecord) -> Result<()> {
        self.upsert_gateway_node_impl(record)
    }

    fn get_gateway_node(&self, node_id: &str) -> Result<Option<GatewayNodeRecord>> {
        self.get_gateway_node_impl(node_id)
    }

    fn list_gateway_nodes(&self, status: Option<&str>) -> Result<Vec<GatewayNodeRecord>> {
        self.list_gateway_nodes_impl(status)
    }

    fn upsert_gateway_node_token(&self, record: &GatewayNodeTokenRecord) -> Result<()> {
        self.upsert_gateway_node_token_impl(record)
    }

    fn get_gateway_node_token(&self, token: &str) -> Result<Option<GatewayNodeTokenRecord>> {
        self.get_gateway_node_token_impl(token)
    }

    fn list_gateway_node_tokens(
        &self,
        node_id: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<GatewayNodeTokenRecord>> {
        self.list_gateway_node_tokens_impl(node_id, status)
    }

    fn delete_gateway_node_token(&self, token: &str) -> Result<i64> {
        self.delete_gateway_node_token_impl(token)
    }

    fn upsert_media_asset(&self, record: &MediaAssetRecord) -> Result<()> {
        self.upsert_media_asset_impl(record)
    }

    fn get_media_asset(&self, asset_id: &str) -> Result<Option<MediaAssetRecord>> {
        self.get_media_asset_impl(asset_id)
    }

    fn get_media_asset_by_hash(&self, hash: &str) -> Result<Option<MediaAssetRecord>> {
        self.get_media_asset_by_hash_impl(hash)
    }

    fn upsert_speech_job(&self, record: &SpeechJobRecord) -> Result<()> {
        self.upsert_speech_job_impl(record)
    }

    fn list_pending_speech_jobs(&self, job_type: &str, limit: i64) -> Result<Vec<SpeechJobRecord>> {
        self.list_pending_speech_jobs_impl(job_type, limit)
    }

    fn upsert_session_run(&self, record: &SessionRunRecord) -> Result<()> {
        self.upsert_session_run_impl(record)
    }

    fn get_session_run(&self, run_id: &str) -> Result<Option<SessionRunRecord>> {
        self.get_session_run_impl(run_id)
    }

    fn list_session_runs_by_session(
        &self,
        user_id: &str,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<SessionRunRecord>> {
        self.list_session_runs_by_session_impl(user_id, session_id, limit)
    }

    fn list_session_runs_by_parent(
        &self,
        user_id: &str,
        parent_session_id: &str,
        limit: i64,
    ) -> Result<Vec<SessionRunRecord>> {
        self.list_session_runs_by_parent_impl(user_id, parent_session_id, limit)
    }

    fn list_session_runs_by_dispatch(
        &self,
        user_id: &str,
        dispatch_id: &str,
        limit: i64,
    ) -> Result<Vec<SessionRunRecord>> {
        self.list_session_runs_by_dispatch_impl(user_id, dispatch_id, limit)
    }

    fn upsert_cron_job(&self, record: &CronJobRecord) -> Result<()> {
        self.upsert_cron_job_impl(record)
    }
    fn get_cron_job(&self, user_id: &str, job_id: &str) -> Result<Option<CronJobRecord>> {
        self.get_cron_job_impl(user_id, job_id)
    }
    fn get_cron_job_by_dedupe_key(
        &self,
        user_id: &str,
        dedupe_key: &str,
    ) -> Result<Option<CronJobRecord>> {
        self.get_cron_job_by_dedupe_key_impl(user_id, dedupe_key)
    }
    fn list_cron_jobs(&self, user_id: &str, include_disabled: bool) -> Result<Vec<CronJobRecord>> {
        self.list_cron_jobs_impl(user_id, include_disabled)
    }
    fn delete_cron_job(&self, user_id: &str, job_id: &str) -> Result<i64> {
        self.delete_cron_job_impl(user_id, job_id)
    }

    fn delete_cron_jobs_by_session(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.delete_cron_jobs_by_session_impl(user_id, session_id)
    }

    fn reset_cron_jobs_running(&self) -> Result<()> {
        self.reset_cron_jobs_running_impl()
    }

    fn count_running_cron_jobs(&self, now: f64) -> Result<i64> {
        self.count_running_cron_jobs_impl(now)
    }

    fn claim_due_cron_jobs(
        &self,
        now: f64,
        limit: i64,
        runner_id: &str,
        lease_expires_at: f64,
    ) -> Result<Vec<CronJobRecord>> {
        self.claim_due_cron_jobs_impl(now, limit, runner_id, lease_expires_at)
    }

    fn renew_cron_job_lease(
        &self,
        user_id: &str,
        job_id: &str,
        runner_id: &str,
        run_token: &str,
        heartbeat_at: f64,
        lease_expires_at: f64,
    ) -> Result<bool> {
        self.renew_cron_job_lease_impl(
            user_id,
            job_id,
            runner_id,
            run_token,
            heartbeat_at,
            lease_expires_at,
        )
    }
    fn insert_cron_run(&self, record: &CronRunRecord) -> Result<()> {
        self.insert_cron_run_impl(record)
    }

    fn list_cron_runs(
        &self,
        user_id: &str,
        job_id: &str,
        limit: i64,
    ) -> Result<Vec<CronRunRecord>> {
        self.list_cron_runs_impl(user_id, job_id, limit)
    }

    fn get_next_cron_run_at(&self, now: f64) -> Result<Option<f64>> {
        self.get_next_cron_run_at_impl(now)
    }

    fn get_user_tool_access(&self, user_id: &str) -> Result<Option<UserToolAccessRecord>> {
        self.get_user_tool_access_impl(user_id)
    }

    fn set_user_tool_access(
        &self,
        user_id: &str,
        allowed_tools: Option<&Vec<String>>,
    ) -> Result<()> {
        self.set_user_tool_access_impl(user_id, allowed_tools)
    }

    fn get_user_agent_access(&self, user_id: &str) -> Result<Option<UserAgentAccessRecord>> {
        self.get_user_agent_access_impl(user_id)
    }

    fn set_user_agent_access(
        &self,
        user_id: &str,
        allowed_agent_ids: Option<&Vec<String>>,
        blocked_agent_ids: Option<&Vec<String>>,
    ) -> Result<()> {
        self.set_user_agent_access_impl(user_id, allowed_agent_ids, blocked_agent_ids)
    }

    fn upsert_user_agent(&self, record: &UserAgentRecord) -> Result<()> {
        self.upsert_user_agent_impl(record)
    }

    fn get_user_agent(&self, user_id: &str, agent_id: &str) -> Result<Option<UserAgentRecord>> {
        self.get_user_agent_impl(user_id, agent_id)
    }

    fn get_user_agent_by_id(&self, agent_id: &str) -> Result<Option<UserAgentRecord>> {
        self.get_user_agent_by_id_impl(agent_id)
    }

    fn list_user_agents(&self, user_id: &str) -> Result<Vec<UserAgentRecord>> {
        self.list_user_agents_impl(user_id)
    }

    fn list_user_agents_by_hive(
        &self,
        user_id: &str,
        hive_id: &str,
    ) -> Result<Vec<UserAgentRecord>> {
        self.list_user_agents_by_hive_impl(user_id, hive_id)
    }

    fn list_shared_user_agents(&self, user_id: &str) -> Result<Vec<UserAgentRecord>> {
        self.list_shared_user_agents_impl(user_id)
    }

    fn delete_user_agent(&self, user_id: &str, agent_id: &str) -> Result<i64> {
        self.delete_user_agent_impl(user_id, agent_id)
    }

    fn upsert_hive(&self, record: &HiveRecord) -> Result<()> {
        self.upsert_hive_impl(record)
    }

    fn get_hive(&self, user_id: &str, hive_id: &str) -> Result<Option<HiveRecord>> {
        self.get_hive_impl(user_id, hive_id)
    }

    fn list_hives(&self, user_id: &str, include_archived: bool) -> Result<Vec<HiveRecord>> {
        self.list_hives_impl(user_id, include_archived)
    }

    fn delete_hive(&self, user_id: &str, hive_id: &str) -> Result<i64> {
        self.delete_hive_impl(user_id, hive_id)
    }

    fn move_agents_to_hive(
        &self,
        user_id: &str,
        hive_id: &str,
        agent_ids: &[String],
    ) -> Result<i64> {
        self.move_agents_to_hive_impl(user_id, hive_id, agent_ids)
    }

    fn upsert_team_run(&self, record: &TeamRunRecord) -> Result<()> {
        self.upsert_team_run_impl(record)
    }

    fn delete_team_runs_by_hive(&self, user_id: &str, hive_id: &str) -> Result<i64> {
        self.delete_team_runs_by_hive_impl(user_id, hive_id)
    }

    fn get_team_run(&self, team_run_id: &str) -> Result<Option<TeamRunRecord>> {
        self.get_team_run_impl(team_run_id)
    }

    fn list_team_runs(
        &self,
        user_id: &str,
        hive_id: Option<&str>,
        parent_session_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<TeamRunRecord>, i64)> {
        self.list_team_runs_impl(user_id, hive_id, parent_session_id, offset, limit)
    }

    fn list_team_runs_by_status(
        &self,
        statuses: &[&str],
        offset: i64,
        limit: i64,
    ) -> Result<Vec<TeamRunRecord>> {
        self.list_team_runs_by_status_impl(statuses, offset, limit)
    }

    fn upsert_team_task(&self, record: &TeamTaskRecord) -> Result<()> {
        self.upsert_team_task_impl(record)
    }

    fn list_team_tasks(&self, team_run_id: &str) -> Result<Vec<TeamTaskRecord>> {
        self.list_team_tasks_impl(team_run_id)
    }

    fn get_team_task(&self, task_id: &str) -> Result<Option<TeamTaskRecord>> {
        self.get_team_task_impl(task_id)
    }
    fn upsert_vector_document(&self, record: &VectorDocumentRecord) -> Result<()> {
        self.upsert_vector_document_impl(record)
    }

    fn get_vector_document(
        &self,
        owner_id: &str,
        base_name: &str,
        doc_id: &str,
    ) -> Result<Option<VectorDocumentRecord>> {
        self.get_vector_document_impl(owner_id, base_name, doc_id)
    }

    fn list_vector_document_summaries(
        &self,
        owner_id: &str,
        base_name: &str,
    ) -> Result<Vec<VectorDocumentSummaryRecord>> {
        self.list_vector_document_summaries_impl(owner_id, base_name)
    }

    fn delete_vector_document(
        &self,
        owner_id: &str,
        base_name: &str,
        doc_id: &str,
    ) -> Result<bool> {
        self.delete_vector_document_impl(owner_id, base_name, doc_id)
    }

    fn delete_vector_documents_by_base(&self, owner_id: &str, base_name: &str) -> Result<i64> {
        self.delete_vector_documents_by_base_impl(owner_id, base_name)
    }

    fn prepare_user_token_balance(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
    ) -> Result<Option<UserTokenBalanceStatus>> {
        self.prepare_user_token_balance_impl(user_id, today, daily_grant)
    }

    fn consume_user_tokens(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
        amount: i64,
    ) -> Result<Option<UserTokenBalanceStatus>> {
        self.consume_user_tokens_impl(user_id, today, daily_grant, amount)
    }

    fn grant_user_tokens(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
        amount: i64,
        updated_at: f64,
    ) -> Result<Option<UserTokenBalanceStatus>> {
        self.grant_user_tokens_impl(user_id, today, daily_grant, amount, updated_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_fallback_runtime_reuses_static_instance() {
        let first = postgres_fallback_runtime().expect("fallback runtime should initialize");
        let second = postgres_fallback_runtime().expect("fallback runtime should be reusable");
        assert!(std::ptr::eq(first, second));
        assert_eq!(first.block_on(async { 7 }), 7);
    }
}
