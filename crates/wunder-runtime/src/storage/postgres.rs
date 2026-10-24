use crate::core::runtime_tuning;
use crate::schemas::AbilityDescriptor;
use crate::storage::{
    normalize_sandbox_container_id, AgentTaskRecord, BridgeCenterAccountRecord, BridgeCenterRecord,
    BridgeDeliveryLogRecord, BridgeRouteAuditLogRecord, BridgeUserRouteRecord,
    ChannelAccountRecord, ChannelBindingRecord, ChannelMessageRecord, ChannelMessageStats,
    ChannelOutboxRecord, ChannelOutboxStats, ChannelSessionRecord, ChannelUserBindingRecord,
    ChatSessionRecord, CronJobRecord, CronRunRecord, ExternalLinkRecord, GatewayClientRecord,
    GatewayNodeRecord, GatewayNodeTokenRecord, ListBridgeCenterAccountsQuery,
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
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, File};
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio_postgres::types::ToSql;
use tokio_postgres::NoTls;

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
mod schema;
mod session_cleanup;
mod session_goal;
mod session_lock_store;
mod session_run;
mod stress_thread;
mod user_account_store;
mod user_world_store;
mod vector_document_store;
mod workspace_store;

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
use user_account_store::PostgresUserAccountStorage;
use user_world_store::PostgresUserWorldStorage;
use vector_document_store::PostgresVectorDocumentStorage;
use workspace_store::PostgresWorkspaceStorage;

const DEFAULT_POOL_SIZE: usize = 64;
/// The PostgreSQL control plane is shared by server replicas, while canonical
/// rollouts deliberately remain in a local, single-writer store.  A replica
/// pointing the same control plane at a different local root would otherwise
/// accept the same thread ids and silently create two unrelated histories.
const CANONICAL_ROOT_OWNER_META_KEY: &str = "canonical_rollout_root_owner_v1";
const CANONICAL_ROOT_MARKER_FILE: &str = ".canonical-root-id.json";
const CANONICAL_ROOT_FORMAT: u8 = 1;
const MAX_CANONICAL_ROOT_MARKER_BYTES: u64 = 1024;

/// An invalid or unavailable root is a deployment failure. `auto` must not
/// bypass it by silently switching to an independent SQLite control plane.
#[derive(Debug)]
pub(crate) struct CanonicalRootBindingError;

impl std::fmt::Display for CanonicalRootBindingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PostgreSQL canonical root validation failed; use one active executor and the original persisted root, and preserve its identity marker")
    }
}

impl std::error::Error for CanonicalRootBindingError {}

#[cfg(any(feature = "sqlite-storage", test))]
pub(crate) fn is_canonical_root_binding_failure(error: &anyhow::Error) -> bool {
    error.is::<CanonicalRootBindingError>()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CanonicalRootOwner {
    version: u8,
    /// Random identity persisted with the local rollout root. It distinguishes
    /// two local disks, including ones mounted at the same absolute path on
    /// different hosts.
    store_id: String,
}

fn decode_canonical_root_owner(bytes: &[u8]) -> Result<CanonicalRootOwner> {
    if bytes.len() as u64 > MAX_CANONICAL_ROOT_MARKER_BYTES {
        return Err(anyhow!("canonical root identity exceeds the size limit"));
    }
    let owner: CanonicalRootOwner =
        serde_json::from_slice(bytes).context("invalid canonical root identity")?;
    if owner.version != CANONICAL_ROOT_FORMAT || uuid::Uuid::parse_str(&owner.store_id).is_err() {
        return Err(anyhow!("invalid canonical root marker identity"));
    }
    Ok(owner)
}

fn read_canonical_root_owner(mut file: File) -> Result<CanonicalRootOwner> {
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_CANONICAL_ROOT_MARKER_BYTES + 1)
        .read_to_end(&mut bytes)?;
    decode_canonical_root_owner(&bytes)
}

fn canonical_root_owner(root: &std::path::Path) -> Result<CanonicalRootOwner> {
    (|| {
        fs::create_dir_all(root)?;
        let marker_path = root.join(CANONICAL_ROOT_MARKER_FILE);
        match File::open(&marker_path) {
            Ok(file) => return read_canonical_root_owner(file),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let owner = CanonicalRootOwner {
            version: CANONICAL_ROOT_FORMAT,
            store_id: uuid::Uuid::new_v4().to_string(),
        };
        // Publish only a completely written, synced marker. Concurrent openers
        // must observe either no marker or the same valid, immutable identity.
        let mut temporary = tempfile::Builder::new()
            .prefix(".canonical-root-id-")
            .tempfile_in(root)?;
        temporary.write_all(&serde_json::to_vec(&owner)?)?;
        temporary.as_file().sync_all()?;
        match temporary.persist_noclobber(&marker_path) {
            Ok(file) => {
                file.sync_all()?;
                #[cfg(unix)]
                File::open(root)?.sync_all()?;
                Ok(owner)
            }
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                read_canonical_root_owner(File::open(&marker_path)?)
            }
            Err(error) => Err(error.error.into()),
        }
    })()
    .context(CanonicalRootBindingError)
}

fn validate_canonical_root_binding(owner: &CanonicalRootOwner, stored: &[u8]) -> Result<()> {
    (|| {
        let stored = decode_canonical_root_owner(stored)?;
        if stored != *owner {
            return Err(anyhow!(
                "canonical rollout root ownership conflict: this PostgreSQL schema is already bound to another persisted root; use its original root and a single active executor"
            ));
        }
        Ok(())
    })()
    .context(CanonicalRootBindingError)
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

pub struct PostgresStorage {
    pool: Pool,
    initialized: AtomicBool,
    init_guard: Mutex<()>,
    // PostgreSQL is the distributed control plane. Canonical thread rollout
    // and bounded diagnostic facts live below this explicitly configured root.
    data_root: PathBuf,
    canonical_threads: OnceLock<Arc<super::thread_store::LocalThreadStore>>,
    runtime_logs: OnceLock<Arc<super::runtime_logs::RuntimeLogs>>,
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
    fn batch_execute(&mut self, query: &str) -> Result<()> {
        self.storage.block_on(self.tx.batch_execute(query))??;
        Ok(())
    }

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
    /// Binds a PostgreSQL control-plane schema to one persisted canonical
    /// rollout root. This is intentionally a deployment guard, not a lease:
    /// a request must still be routed to the process holding that root's
    /// writer lock. The guard prevents the more damaging case where replicas
    /// with independent local disks silently accept the same history ids.
    fn ensure_canonical_root_ownership(&self, conn: &mut PgConn<'_>) -> Result<()> {
        // Validate local writer ownership before announcing a usable server.
        self.canonical_threads()
            .context(CanonicalRootBindingError)?;
        let owner = canonical_root_owner(&self.data_root)?;
        let desired = serde_json::to_string(&owner)?;
        let now = Self::now_ts();
        let mut tx = conn.transaction()?;
        // `meta` is already initialized when this method is called. The
        // insert is race-safe for two startup processes; the row lock then
        // makes the equality check authoritative for this control plane.
        tx.execute(
            "INSERT INTO meta(key,value,updated_time) VALUES($1,$2,$3) ON CONFLICT(key) DO NOTHING",
            &[&CANONICAL_ROOT_OWNER_META_KEY, &desired, &now],
        )?;
        let stored = tx
            .query_one(
                "SELECT value FROM meta WHERE key=$1 FOR UPDATE",
                &[&CANONICAL_ROOT_OWNER_META_KEY],
            )?
            .get::<_, String>(0);
        validate_canonical_root_binding(&owner, stored.as_bytes())?;
        tx.commit()?;
        Ok(())
    }

    /// Compatibility constructor for direct callers and isolated control-plane
    /// tests. Application startup must use `new_with_data_root` so rollout
    /// files are co-located with the configured storage root.
    pub fn new(dsn: String, connect_timeout_s: u64, pool_size: usize) -> Result<Self> {
        // Direct test callers can connect to independent PostgreSQL schemas.
        // Keep their local roots independent without placing DSNs on disk.
        let mut fingerprint = std::collections::hash_map::DefaultHasher::new();
        dsn.hash(&mut fingerprint);
        let root = std::env::temp_dir()
            .join("wunder-postgres-local")
            .join(std::process::id().to_string())
            .join(format!("{:016x}", fingerprint.finish()));
        Self::new_with_data_root(dsn, connect_timeout_s, pool_size, root)
    }

    pub fn new_with_data_root(
        dsn: String,
        connect_timeout_s: u64,
        pool_size: usize,
        data_root: PathBuf,
    ) -> Result<Self> {
        let cleaned = dsn.trim().to_string();
        if cleaned.is_empty() {
            return Err(anyhow!("postgres dsn is empty"));
        }
        let data_root = if data_root.as_os_str().is_empty() {
            PathBuf::from("./config/data")
        } else {
            data_root
        };
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
            data_root,
            canonical_threads: OnceLock::new(),
            runtime_logs: OnceLock::new(),
        })
    }

    pub(crate) fn canonical_threads(&self) -> Result<&Arc<super::thread_store::LocalThreadStore>> {
        if let Some(store) = self.canonical_threads.get() {
            return Ok(store);
        }
        let store = Arc::new(super::thread_store::LocalThreadStore::open(
            &self.data_root,
        )?);
        let _ = self.canonical_threads.set(store);
        Ok(self
            .canonical_threads
            .get()
            .expect("initialized canonical thread store"))
    }

    pub(crate) fn runtime_logs(&self) -> Result<&Arc<super::runtime_logs::RuntimeLogs>> {
        if let Some(logs) = self.runtime_logs.get() {
            return Ok(logs);
        }
        let logs = Arc::new(super::runtime_logs::RuntimeLogs::open(&self.data_root)?);
        let _ = self.runtime_logs.set(logs);
        Ok(self
            .runtime_logs
            .get()
            .expect("initialized runtime log store"))
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
    fn canonical_root_identity_is_stable_without_rewriting_marker() {
        let root = tempfile::tempdir().unwrap();
        let first = canonical_root_owner(root.path()).unwrap();
        let path = root.path().join(CANONICAL_ROOT_MARKER_FILE);
        let bytes = fs::read(&path).unwrap();
        let metadata = fs::metadata(&path).unwrap();
        let old_time = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(old_time)
            .unwrap();
        assert_eq!(canonical_root_owner(root.path()).unwrap(), first);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), old_time);
        assert!(metadata.len() < MAX_CANONICAL_ROOT_MARKER_BYTES);
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 2);
    }

    #[test]
    fn canonical_root_identity_follows_persisted_root_after_move() {
        let parent = tempfile::tempdir().unwrap();
        let original = parent.path().join("original");
        let owner = canonical_root_owner(&original).unwrap();
        let moved = parent.path().join("moved");
        fs::rename(original, &moved).unwrap();
        assert_eq!(canonical_root_owner(&moved).unwrap(), owner);
        assert_ne!(
            canonical_root_owner(&parent.path().join("other")).unwrap(),
            owner
        );
    }

    #[test]
    fn canonical_root_identity_concurrent_creation_observes_one_complete_marker() {
        let root = tempfile::tempdir().unwrap();
        let barrier = std::sync::Barrier::new(8);
        let owners = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        canonical_root_owner(root.path()).unwrap()
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert!(owners.iter().all(|owner| owner == &owners[0]));
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn canonical_root_invalid_marker_fails_closed_without_overwriting() {
        let valid_id = uuid::Uuid::new_v4();
        for bytes in [
            Vec::new(),
            b"{".to_vec(),
            vec![b' '; MAX_CANONICAL_ROOT_MARKER_BYTES as usize + 1],
            format!(r#"{{"version":2,"store_id":"{valid_id}"}}"#).into_bytes(),
            br#"{"version":1,"store_id":"invalid"}"#.to_vec(),
        ] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join(CANONICAL_ROOT_MARKER_FILE);
            fs::write(&path, &bytes).unwrap();
            let error = canonical_root_owner(root.path()).unwrap_err();
            assert!(is_canonical_root_binding_failure(&error));
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
    }

    #[test]
    fn canonical_root_unavailable_path_is_fatal_deployment_failure() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("regular-file");
        fs::write(&path, b"fixture").unwrap();
        let error = canonical_root_owner(&path).unwrap_err();
        assert!(is_canonical_root_binding_failure(&error));
    }

    #[test]
    fn canonical_root_binding_accepts_same_owner_and_rejects_other_or_invalid_owner() {
        let root = tempfile::tempdir().unwrap();
        let owner = canonical_root_owner(root.path()).unwrap();
        validate_canonical_root_binding(&owner, &serde_json::to_vec(&owner).unwrap()).unwrap();
        let other = CanonicalRootOwner {
            version: CANONICAL_ROOT_FORMAT,
            store_id: uuid::Uuid::new_v4().to_string(),
        };
        for bytes in [serde_json::to_vec(&other).unwrap(), b"invalid".to_vec()] {
            let error = validate_canonical_root_binding(&owner, &bytes).unwrap_err();
            assert!(is_canonical_root_binding_failure(&error));
            assert!(is_canonical_root_binding_failure(&error.context("startup")));
        }
        assert!(!is_canonical_root_binding_failure(&anyhow!(
            "canonical rollout root ownership conflict"
        )));
        assert!(!is_canonical_root_binding_failure(&anyhow!(
            "connection unavailable"
        )));
    }

    #[test]
    #[ignore = "requires WUNDER_TEST_POSTGRES_DSN in key/value format and schema privileges"]
    fn postgres_canonical_root_binding_survives_restart_and_rejects_independent_root() {
        let dsn = std::env::var("WUNDER_TEST_POSTGRES_DSN").expect("isolated test DSN required");
        assert!(!dsn.contains("://"), "use a PostgreSQL key/value DSN");
        let schema = format!("fixture_root_{}", uuid::Uuid::new_v4().simple());
        let admin = PostgresStorage::new(dsn.clone(), 5, 2).unwrap();
        let mut admin_conn = admin.conn().unwrap();
        admin_conn
            .batch_execute(&format!("CREATE SCHEMA {schema}"))
            .unwrap();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let dsn = format!("{dsn} options='-c search_path={schema}'");
            let first_root = tempfile::tempdir().unwrap();
            let other_root = tempfile::tempdir().unwrap();
            let first =
                PostgresStorage::new_with_data_root(dsn.clone(), 5, 2, first_root.path().into())
                    .unwrap();
            first.ensure_initialized_impl().unwrap();
            let mut conn = first.conn().unwrap();
            let before = conn
                .query_one(
                    "SELECT value,xmin::text FROM meta WHERE key=$1",
                    &[&CANONICAL_ROOT_OWNER_META_KEY],
                )
                .unwrap();
            let identity: String = before.get(0);
            let version: String = before.get(1);
            let reopen =
                PostgresStorage::new_with_data_root(dsn.clone(), 5, 2, first_root.path().into())
                    .unwrap();
            reopen.ensure_initialized_impl().unwrap();
            let other =
                PostgresStorage::new_with_data_root(dsn, 5, 2, other_root.path().into()).unwrap();
            let error = other.ensure_initialized_impl().unwrap_err();
            assert!(is_canonical_root_binding_failure(&error));
            let after = conn
                .query_one(
                    "SELECT value,xmin::text FROM meta WHERE key=$1",
                    &[&CANONICAL_ROOT_OWNER_META_KEY],
                )
                .unwrap();
            assert_eq!(after.get::<_, String>(0), identity);
            assert_eq!(
                after.get::<_, String>(1),
                version,
                "reopen or rejection must not rewrite the binding"
            );
        }));
        admin_conn
            .batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
            .unwrap();
        if let Err(payload) = outcome {
            std::panic::resume_unwind(payload);
        }
    }

    #[test]
    fn postgres_fallback_runtime_reuses_static_instance() {
        let first = postgres_fallback_runtime().expect("fallback runtime should initialize");
        let second = postgres_fallback_runtime().expect("fallback runtime should be reusable");
        assert!(std::ptr::eq(first, second));
        assert_eq!(first.block_on(async { 7 }), 7);
    }
}
