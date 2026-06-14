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
mod backend_impl;
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
