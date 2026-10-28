use crate::core::runtime_tuning;
use crate::schemas::AbilityDescriptor;
use crate::storage::{
    normalize_sandbox_container_id, AgentTaskRecord, BridgeCenterAccountRecord, BridgeCenterRecord,
    BridgeDeliveryLogRecord, BridgeRouteAuditLogRecord, BridgeUserRouteRecord,
    ChannelAccountRecord, ChannelBindingRecord, ChannelMessageRecord, ChannelMessageStats,
    ChannelOutboxRecord, ChannelOutboxStats, ChannelSessionRecord, ChannelUserBindingRecord,
    ChatSessionRecord, CronJobRecord, CronJobStats, CronRunRecord, ExternalLinkRecord,
    GatewayClientRecord, GatewayNodeRecord, GatewayNodeTokenRecord, ListBridgeCenterAccountsQuery,
    ListBridgeCentersQuery, ListBridgeDeliveryLogsQuery, ListBridgeRouteAuditLogsQuery,
    ListBridgeUserRoutesQuery, ListChannelUserBindingsQuery, MediaAssetRecord,
    MemoryFragmentEmbeddingRecord, MemoryFragmentRecord, MemoryHitRecord, MemoryJobRecord,
    OrgUnitRecord, PresetBoundAgentRecord, SessionGoalRecord, SessionLockRecord, SessionLockStatus,
    SessionRunRecord, SpeechJobRecord, UpdateAgentTaskStatusParams,
    UpdateChannelOutboxStatusParams, UpsertMemoryTaskLogParams, UserAccountRecord,
    UserAgentAccessRecord, UserAgentPresetBinding, UserAgentRecord, UserExperienceUpdateResult,
    UserQuotaStatus, UserSessionScopeRecord, UserTokenRecord, UserToolAccessRecord,
    UserWorldConversationRecord, UserWorldConversationSummaryRecord, UserWorldEventRecord,
    UserWorldGroupRecord, UserWorldMemberRecord, UserWorldMessageRecord, UserWorldReadResult,
    UserWorldSendMessageResult, VectorChunkEmbeddingRecord, VectorDocumentRecord,
    VectorDocumentSummaryRecord,
};
use anyhow::{anyhow, Result};
use chrono::Utc;
use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod};
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;
use tokio_postgres::types::ToSql;
use tokio_postgres::NoTls;

mod admission_store;
mod agent_directory_store;
mod agent_message;
mod agent_runtime_store;
mod backend_impl;
mod benchmark_store;
mod bridge_store;
mod channel_directory;
mod channel_runtime;
mod chat_session;
mod cloud_store;
mod conversation_log_store;
mod cron;
mod gateway_store;
mod interlink_store;
mod log_maintenance;
mod log_stats_store;
mod media_store;
mod memory_store;
mod meta_store;
mod monitor_store;
mod queue_control;
mod quota_balance_store;
mod retention_store;
mod runtime_log_store;
mod schema;
mod session_cleanup;
mod session_goal;
mod session_lock_store;
mod session_run;
mod stress_thread;
mod thread_display;
mod thread_fork_store;
mod thread_log_facade;
mod thread_store;
mod user_account_store;
mod user_world_store;
mod vector_document_store;
mod workspace_store;

pub(crate) use admission_store::PostgresAdmissionStore;
use agent_directory_store::PostgresAgentDirectoryStorage;
use agent_runtime_store::PostgresAgentRuntimeStorage;
use benchmark_store::PostgresBenchmarkStorage;
use bridge_store::PostgresBridgeStorage;
use channel_directory::PostgresChannelDirectoryStorage;
use channel_runtime::PostgresChannelRuntimeStorage;
use chat_session::PostgresChatSessionStorage;
use cloud_store::PostgresCloudStorage;
use conversation_log_store::PostgresConversationLogStorage;
use cron::PostgresCronStorage;
use gateway_store::PostgresGatewayStorage;
use interlink_store::PostgresInterlinkStorage;
use log_stats_store::PostgresLogStatsStorage;
use media_store::PostgresMediaStorage;
use memory_store::PostgresMemoryStorage;
use meta_store::PostgresMetaStorage;
use monitor_store::PostgresMonitorStorage;
use quota_balance_store::PostgresQuotaBalanceStorage;
use retention_store::PostgresRetentionStorage;
use schema::PostgresSchemaStorage;
use session_goal::PostgresSessionGoalStorage;
use session_lock_store::PostgresSessionLockStorage;
use session_run::PostgresSessionRunStorage;
pub(crate) use thread_store::PostgresThreadStore;
use user_account_store::PostgresUserAccountStorage;
use user_world_store::PostgresUserWorldStorage;
use vector_document_store::PostgresVectorDocumentStorage;
use workspace_store::PostgresWorkspaceStorage;

const DEFAULT_POOL_SIZE: usize = 64;
const MAX_CACHED_STATEMENTS: usize = 256;

fn bound_statement_cache(cache: &deadpool_postgres::StatementCache) {
    if cache.size() >= MAX_CACHED_STATEMENTS {
        cache.clear();
    }
}

fn postgres_fallback_runtime() -> Result<&'static tokio::runtime::Runtime> {
    static RUNTIME: OnceLock<Result<tokio::runtime::Runtime, String>> = OnceLock::new();
    match RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(runtime_tuning::postgres_runtime_threads())
            .max_blocking_threads(8)
            .enable_all()
            .thread_name("postgres-storage-fallback")
            .build()
            .map_err(|err| format!("create tokio runtime for postgres: {err}"))
    }) {
        Ok(runtime) => Ok(runtime),
        Err(err) => Err(anyhow!(err.clone())),
    }
}

#[derive(Clone)]
pub struct PostgresStorage {
    pool: Pool,
    initialized: Arc<AtomicBool>,
    init_guard: Arc<Mutex<()>>,
    // The store owns a cloned pool handle; caching it weakly prevents a
    // storage -> thread store -> storage cycle from retaining connections.
    service_threads: Arc<Mutex<Weak<PostgresThreadStore>>>,
}

pub(crate) struct PgConn<'a> {
    storage: &'a PostgresStorage,
    client: deadpool_postgres::Client,
}

impl PgConn<'_> {
    pub(crate) fn batch_execute(&mut self, query: &str) -> Result<()> {
        self.storage.block_on(self.client.batch_execute(query))??;
        Ok(())
    }

    pub(crate) fn execute(&mut self, query: &str, params: &[&(dyn ToSql + Sync)]) -> Result<u64> {
        bound_statement_cache(&self.client.statement_cache);
        Ok(self.storage.block_on(async {
            let statement = self.client.prepare_cached(query).await?;
            self.client.execute(&statement, params).await
        })??)
    }

    pub(crate) fn query(
        &mut self,
        query: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Vec<tokio_postgres::Row>> {
        bound_statement_cache(&self.client.statement_cache);
        Ok(self.storage.block_on(async {
            let statement = self.client.prepare_cached(query).await?;
            self.client.query(&statement, params).await
        })??)
    }

    pub(crate) fn query_one(
        &mut self,
        query: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<tokio_postgres::Row> {
        bound_statement_cache(&self.client.statement_cache);
        Ok(self.storage.block_on(async {
            let statement = self.client.prepare_cached(query).await?;
            self.client.query_one(&statement, params).await
        })??)
    }

    pub(crate) fn query_opt(
        &mut self,
        query: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Option<tokio_postgres::Row>> {
        bound_statement_cache(&self.client.statement_cache);
        Ok(self.storage.block_on(async {
            let statement = self.client.prepare_cached(query).await?;
            self.client.query_opt(&statement, params).await
        })??)
    }

    pub(crate) fn transaction<'a>(&'a mut self) -> Result<PgTx<'a>> {
        let tx = self.storage.block_on(self.client.transaction())??;
        Ok(PgTx {
            storage: self.storage,
            tx,
        })
    }
}

pub(crate) struct PgTx<'a> {
    storage: &'a PostgresStorage,
    tx: deadpool_postgres::Transaction<'a>,
}

impl PgTx<'_> {
    pub(crate) fn batch_execute(&mut self, query: &str) -> Result<()> {
        self.storage.block_on(self.tx.batch_execute(query))??;
        Ok(())
    }

    pub(crate) fn execute(&mut self, query: &str, params: &[&(dyn ToSql + Sync)]) -> Result<u64> {
        bound_statement_cache(&self.tx.statement_cache);
        Ok(self.storage.block_on(async {
            let statement = self.tx.prepare_cached(query).await?;
            self.tx.execute(&statement, params).await
        })??)
    }

    pub(crate) fn query(
        &mut self,
        query: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Vec<tokio_postgres::Row>> {
        bound_statement_cache(&self.tx.statement_cache);
        Ok(self.storage.block_on(async {
            let statement = self.tx.prepare_cached(query).await?;
            self.tx.query(&statement, params).await
        })??)
    }

    pub(crate) fn query_one(
        &mut self,
        query: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<tokio_postgres::Row> {
        bound_statement_cache(&self.tx.statement_cache);
        Ok(self.storage.block_on(async {
            let statement = self.tx.prepare_cached(query).await?;
            self.tx.query_one(&statement, params).await
        })??)
    }

    pub(crate) fn query_opt(
        &mut self,
        query: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Option<tokio_postgres::Row>> {
        bound_statement_cache(&self.tx.statement_cache);
        Ok(self.storage.block_on(async {
            let statement = self.tx.prepare_cached(query).await?;
            self.tx.query_opt(&statement, params).await
        })??)
    }

    pub(crate) fn commit(self) -> Result<()> {
        self.storage.block_on(self.tx.commit())??;
        Ok(())
    }
}

impl PostgresStorage {
    /// PostgreSQL owns both service state and canonical thread history. No
    /// local storage root or SQLite projection is required by this backend.
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
            initialized: Arc::new(AtomicBool::new(false)),
            init_guard: Arc::new(Mutex::new(())),
            service_threads: Arc::new(Mutex::new(Weak::new())),
        })
    }

    /// PostgreSQL-only canonical history store for the service profile. This
    /// accessor does not expose a local path and is safe to share across
    /// request handlers; the store uses PostgreSQL transaction locks for
    /// per-thread append ordering.
    pub(crate) fn service_threads(&self) -> Result<Arc<PostgresThreadStore>> {
        let mut cached = self.service_threads.lock();
        if let Some(store) = cached.upgrade() {
            return Ok(store);
        }
        let store = Arc::new(PostgresThreadStore::new(Arc::new(self.clone()))?);
        *cached = Arc::downgrade(&store);
        Ok(store)
    }

    /// Application facades can initialize the service store directly; direct
    /// backend callers need no separate Arc-based startup phase.
    pub(crate) fn service_threads_ref(&self) -> Result<Arc<PostgresThreadStore>> {
        self.service_threads()
    }

    fn block_on<F, T>(&self, fut: F) -> Result<T>
    where
        F: Future<Output = T>,
    {
        let runtime = postgres_fallback_runtime()?;
        match tokio::runtime::Handle::try_current() {
            Ok(_) => Ok(tokio::task::block_in_place(|| runtime.block_on(fut))),
            Err(_) => Ok(runtime.block_on(fut)),
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
        let tool_names = Self::parse_string_list(row.get(6));
        UserAgentRecord {
            agent_id: row.get(0),
            user_id: row.get(1),
            name: row.get(2),
            description: row.get::<_, Option<String>>(3).unwrap_or_default(),
            system_prompt: row.get::<_, Option<String>>(4).unwrap_or_default(),
            preview_skill: row.get::<_, i32>(22) != 0,
            model_name: row.get::<_, Option<String>>(5),
            ability_items: Self::parse_ability_items(row.get(9)),
            tool_names: tool_names.clone(),
            declared_tool_names: Self::parse_declared_tool_names(row.get(7)),
            declared_skill_names: Self::parse_string_list(row.get(8)),
            visible_unit_ids: Self::parse_string_list(row.get(23)),
            preset_questions: Self::parse_string_list(row.get(18)),
            access_level: row.get(10),
            approval_mode: row.get(11),
            is_shared: row.get::<_, i32>(12) != 0,
            status: row.get(13),
            icon: row.get(14),
            sandbox_container_id: normalize_sandbox_container_id(row.get::<_, i32>(15)),
            created_at: row.get(16),
            updated_at: row.get(17),
            preset_binding: Self::parse_preset_binding(row.get(19)),
            silent: row.get::<_, i32>(20) != 0,
            prefer_mother: row.get::<_, i32>(21) != 0,
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
    #[ignore = "requires an isolated WUNDER_TEST_POSTGRES_DSN"]
    fn postgres_prepared_statements_are_reused_and_bounded_across_transactions() {
        let storage =
            PostgresStorage::new(std::env::var("WUNDER_TEST_POSTGRES_DSN").unwrap(), 5, 1).unwrap();
        let mut connection = storage.conn().unwrap();
        connection.client.statement_cache.clear();
        assert_eq!(
            connection
                .query_one("SELECT $1::integer", &[&7_i32])
                .unwrap()
                .get::<_, i32>(0),
            7
        );
        let size = connection.client.statement_cache.size();
        for value in 0..4_i32 {
            let mut transaction = connection.transaction().unwrap();
            assert_eq!(
                transaction
                    .query_one("SELECT $1::integer", &[&value])
                    .unwrap()
                    .get::<_, i32>(0),
                value
            );
            transaction.commit().unwrap();
        }
        assert_eq!(connection.client.statement_cache.size(), size);
        for index in 0..(MAX_CACHED_STATEMENTS + 8) {
            let query = format!("SELECT {index}::integer");
            assert_eq!(
                connection.query_one(&query, &[]).unwrap().get::<_, i32>(0),
                index as i32
            );
            assert!(connection.client.statement_cache.size() <= MAX_CACHED_STATEMENTS);
        }
        let mut transaction = connection.transaction().unwrap();
        transaction
            .execute(
                "CREATE TEMP TABLE fixture_statement_rollback(value integer)",
                &[],
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO fixture_statement_rollback(value) VALUES($1)",
                &[&7_i32],
            )
            .unwrap();
        drop(transaction);
        assert!(connection
            .query_opt(
                "SELECT to_regclass('pg_temp.fixture_statement_rollback')::text",
                &[]
            )
            .unwrap()
            .unwrap()
            .get::<_, Option<String>>(0)
            .is_none());
    }

    #[test]
    fn postgres_clones_share_connection_and_initialization_state() {
        let storage = PostgresStorage::new(
            "host=unavailable.invalid user=fixture dbname=fixture".into(),
            1,
            2,
        )
        .unwrap();
        let cloned = storage.clone();
        assert!(Arc::ptr_eq(&storage.initialized, &cloned.initialized));
        assert!(Arc::ptr_eq(&storage.init_guard, &cloned.init_guard));
        assert!(Arc::ptr_eq(
            &storage.service_threads,
            &cloned.service_threads
        ));
        storage
            .initialized
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(cloned.initialized.load(std::sync::atomic::Ordering::SeqCst));
        assert!(cloned.service_threads.lock().upgrade().is_none());
    }

    #[test]
    #[ignore = "requires an isolated WUNDER_TEST_POSTGRES_DSN"]
    fn postgres_thread_store_initializes_lazily_and_releases_shared_pool_state() {
        let storage =
            PostgresStorage::new(std::env::var("WUNDER_TEST_POSTGRES_DSN").unwrap(), 5, 4).unwrap();
        let state = Arc::downgrade(&storage.initialized);
        // Direct backend callers do not first initialize AppState or wrap the
        // storage in an Arc. Both accessors must share the same live store.
        let first = storage.service_threads_ref().unwrap();
        let second = storage.service_threads().unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        let cached = Arc::downgrade(&first);
        drop(second);
        drop(storage);
        assert!(state.upgrade().is_some());
        drop(first);
        assert!(cached.upgrade().is_none());
        assert!(state.upgrade().is_none());
    }

    #[test]
    fn postgres_fallback_runtime_reuses_static_instance() {
        let first = postgres_fallback_runtime().expect("fallback runtime should initialize");
        let second = postgres_fallback_runtime().expect("fallback runtime should be reusable");
        assert!(std::ptr::eq(first, second));
        assert_eq!(first.block_on(async { 7 }), 7);
    }

    #[test]
    fn postgres_io_uses_dedicated_runtime_from_async_and_blocking_callers() {
        let storage = PostgresStorage::new(
            "host=unavailable.invalid user=fixture dbname=fixture".into(),
            1,
            2,
        )
        .unwrap();
        let expected = postgres_fallback_runtime().unwrap().handle().id();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .max_blocking_threads(2)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            assert_eq!(
                storage
                    .block_on(async { tokio::runtime::Handle::current().id() })
                    .unwrap(),
                expected
            );
            let actual = tokio::task::spawn_blocking(move || {
                storage.block_on(async { tokio::runtime::Handle::current().id() })
            })
            .await
            .unwrap()
            .unwrap();
            assert_eq!(actual, expected);
        });
    }

    #[test]
    #[ignore = "requires an isolated WUNDER_TEST_POSTGRES_DSN"]
    fn postgres_queries_progress_when_caller_blocking_pool_is_saturated() {
        let dsn = std::env::var("WUNDER_TEST_POSTGRES_DSN").unwrap();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .max_blocking_threads(2)
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let storage = PostgresStorage::new(dsn, 2, 4).unwrap();
                let barrier = Arc::new(tokio::sync::Barrier::new(4));
                let mut tasks = Vec::new();
                for _ in 0..4 {
                    let storage = storage.clone();
                    let barrier = barrier.clone();
                    tasks.push(tokio::spawn(async move {
                        barrier.wait().await;
                        for _ in 0..4 {
                            let mut connection = storage.conn().unwrap();
                            let mut transaction = connection.transaction().unwrap();
                            let value: i32 = transaction
                                .query_one("SELECT 7::integer, pg_sleep(0.01)", &[])
                                .unwrap()
                                .get(0);
                            assert_eq!(value, 7);
                            transaction.commit().unwrap();
                        }
                    }));
                }
                for task in tasks {
                    task.await.unwrap();
                }
                sender.send(()).unwrap();
            });
        });
        receiver
            .recv_timeout(Duration::from_secs(15))
            .expect("PostgreSQL I/O must progress independently of the caller blocking pool");
        worker.join().unwrap();
    }
}
