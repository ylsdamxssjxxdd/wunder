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
use anyhow::Result;
use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tracing::warn;

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
mod log_stats_store;
mod media_store;
mod memory_store;
mod meta_store;
mod monitor_store;
mod queue_control;
mod quota_balance_store;
mod read_connection;
mod retention_store;
mod runtime_state;
mod schema;
mod session_cleanup;
mod session_goal;
mod session_lock_store;
mod session_run;
pub(crate) mod stress_thread;
mod thread_open_lease_store;
mod user_account_store;
mod user_world_store;
mod vector_document_store;
mod workspace_store;

use agent_directory_store::SqliteAgentDirectoryStorage;
use agent_runtime_store::SqliteAgentRuntimeStorage;
use benchmark_store::SqliteBenchmarkStorage;
use bridge_store::SqliteBridgeStorage;
use channel_directory::SqliteChannelDirectoryStorage;
use channel_runtime::SqliteChannelRuntimeStorage;
use chat_session::SqliteChatSessionStorage;
use cloud_store::SqliteCloudStorage;
use conversation_log_store::SqliteConversationLogStorage;
use cron::SqliteCronStorage;
use gateway_store::SqliteGatewayStorage;
use interlink_store::SqliteInterlinkStorage;
use log_stats_store::SqliteLogStatsStorage;
use media_store::SqliteMediaStorage;
use memory_store::SqliteMemoryStorage;
use meta_store::SqliteMetaStorage;
use monitor_store::SqliteMonitorStorage;
use quota_balance_store::SqliteQuotaBalanceStorage;
use retention_store::SqliteRetentionStorage;
use schema::SqliteSchemaStorage;
use session_goal::SqliteSessionGoalStorage;
use session_lock_store::SqliteSessionLockStorage;
use session_run::SqliteSessionRunStorage;
use thread_open_lease_store::SqliteThreadOpenLeaseStorage;
use user_account_store::SqliteUserAccountStorage;
use user_world_store::SqliteUserWorldStorage;
use vector_document_store::SqliteVectorDocumentStorage;
use workspace_store::SqliteWorkspaceStorage;

pub struct SqliteStorage {
    db_path: PathBuf,
    initialized: AtomicBool,
    init_guard: Mutex<()>,
    // Readers are checked out without holding this mutex during any SQL.
    read_connections: Mutex<Vec<Connection>>,
    // SQLite has one writer. Reuse one bounded connection for message admission
    // instead of reopening/checkpointing the WAL for every small queue write.
    agent_message_connection: Mutex<Option<Connection>>,
    // Keep the WAL open between short operations instead of checkpointing and
    // deleting its sidecars each time. Close after readers at storage shutdown.
    wal_anchor: Mutex<Option<Connection>>,
    canonical_threads: OnceLock<Arc<super::thread_store::LocalThreadStore>>,
    runtime_logs: OnceLock<Arc<super::runtime_logs::RuntimeLogs>>,
    database_role: runtime_state::DatabaseRole,
    runtime_databases: [OnceLock<Box<SqliteStorage>>; 4],
}

impl SqliteStorage {
    pub fn new(db_path: String) -> Self {
        let path = if db_path.trim().is_empty() {
            PathBuf::from("./config/data/state_1.sqlite")
        } else {
            PathBuf::from(db_path)
        };
        Self::with_role(path, runtime_state::DatabaseRole::State)
    }

    fn with_role(path: PathBuf, database_role: runtime_state::DatabaseRole) -> Self {
        Self {
            db_path: path,
            initialized: AtomicBool::new(false),
            init_guard: Mutex::new(()),
            wal_anchor: Mutex::new(None),
            read_connections: Mutex::new(Vec::new()),
            agent_message_connection: Mutex::new(None),
            canonical_threads: OnceLock::new(),
            runtime_logs: OnceLock::new(),
            database_role,
            runtime_databases: std::array::from_fn(|_| OnceLock::new()),
        }
    }

    pub(crate) fn canonical_threads(&self) -> Result<&Arc<super::thread_store::LocalThreadStore>> {
        if let Some(store) = self.canonical_threads.get() {
            return Ok(store);
        }
        let root = self
            .db_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let store = Arc::new(super::thread_store::LocalThreadStore::open(root)?);
        let _ = self.canonical_threads.set(store);
        Ok(self
            .canonical_threads
            .get()
            .expect("initialized canonical store"))
    }

    pub(crate) fn runtime_logs(&self) -> Result<&Arc<super::runtime_logs::RuntimeLogs>> {
        if let Some(logs) = self.runtime_logs.get() {
            return Ok(logs);
        }
        let root = self
            .db_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let logs = Arc::new(super::runtime_logs::RuntimeLogs::open(root)?);
        let _ = self.runtime_logs.set(logs);
        Ok(self
            .runtime_logs
            .get()
            .expect("initialized diagnostic store"))
    }

    fn ensure_db_dir(&self) -> Result<()> {
        if let Some(parent) = self.db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    fn open(&self) -> Result<Connection> {
        self.ensure_db_dir()?;
        // Take a write lock right away and self-heal the one failure mode that
        // has no in-process remedy otherwise: the first connection to a WAL
        // database creates the `-shm` wal-index, and when a real-time scanner
        // holds that fresh file for a moment, `winOpen` silently reopens it
        // READONLY (`pShmNode->isReadonly = 1`). The shared node lives as long
        // as any connection to the database, so every later write fails with a
        // bare "attempt to write a readonly database" while the file itself is
        // writable. Dropping the connection releases the node (when no other
        // connection shares it), the backoff lets the scanner finish, and the
        // next open rebuilds the node read-write.
        for attempt in 0..6u32 {
            let conn = self.open_connection()?;
            match conn.execute_batch("BEGIN IMMEDIATE; ROLLBACK") {
                Ok(()) => return Ok(conn),
                Err(err) => {
                    // probe catches genuine read-only files too; those fail all
                    // six attempts and surface here with the file named.
                    if attempt == 5 {
                        return Err(anyhow::anyhow!(
                            "{} rejected a write lock right after open, 6 attempts: {err}",
                            self.db_path.display()
                        ));
                    }
                    drop(conn);
                    std::thread::sleep(Duration::from_millis(40 * u64::from(attempt + 1)));
                }
            }
        }
        unreachable!("the loop returns or errors on its last iteration")
    }

    /// One plain connection attempt: open, verify no silent read-only
    /// downgrade, then set WAL and prime the wal-index sidecar.
    fn open_connection(&self) -> Result<Connection> {
        // Ask for read/write instead of letting SQLite decide: when it cannot
        // open the file for writing, the Windows (and POSIX) VFS silently
        // downgrades the connection to read-only, and the first symptom is
        // then a misleading "attempt to write a readonly database" thrown by
        // some later, unrelated statement. SQLITE_OPEN_READ_WRITE is a
        // request, not a guarantee, so the downgrade is detected here and
        // fails with the file named - while the caller stack still tells us
        // which component was opening it.
        let conn = Connection::open_with_flags(
            &self.db_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        // 0: not read-only, 1: read-only, 2: unknown (SQLITE_READONLY detection
        // raced an exclusive lock). Only a definite downgrade is fatal here.
        let downgraded =
            unsafe { rusqlite::ffi::sqlite3_db_readonly(conn.handle(), b"main\0".as_ptr().cast()) };
        if downgraded == 1 {
            anyhow::bail!(
                "{} cannot be opened for writing; SQLite silently downgraded the \
                 connection to read-only (check file permissions or another \
                 process holding the file)",
                self.db_path.display()
            );
        }
        // Parallel child runs can briefly contend on SQLite writes.
        conn.busy_timeout(Duration::from_secs(5)).ok();
        // The header must be set before WAL creates the first database page.
        if !self.initialized.load(Ordering::Acquire) {
            self.apply_auto_vacuum(&conn);
        }
        // The WAL switch needs the -wal/-shm pair. Losing it silently leaves
        // the connection on the rollback journal and, when the file cannot be
        // written at all, turns every later write into the same opaque
        // readonly error - so it is reported where it happens.
        if let Err(err) = conn.pragma_update(None, "journal_mode", "WAL") {
            warn!(
                "could not switch {} to WAL, writes will use the rollback journal: {err}",
                self.db_path.display()
            );
        }
        conn.pragma_update(None, "synchronous", "NORMAL").ok();
        self.prime_wal_index();
        Ok(conn)
    }

    /// Open the WAL-index sidecar before SQLite does.
    ///
    /// The first connection to a WAL database creates the `-shm` file. On
    /// Windows a real-time antivirus scanner or the indexer opens a freshly
    /// created file for a moment; that makes the `CREATE|READWRITE` open of
    /// `-shm` fail, and `winOpen` silently falls back to opening it READONLY
    /// (`pShmNode->isReadonly = 1`). The shared shm node lives as long as any
    /// connection to the database, so from that unlucky open on, *every*
    /// write transaction in the process fails with a bare
    /// `SQLITE_READONLY` ("attempt to write a readonly database") while the
    /// database file itself is perfectly writable - exactly the intermittent
    /// loopback failure this used to be.
    ///
    /// Creating/opening the sidecar here means SQLite later finds an existing
    /// file instead of creating one, which is outside the "just created" scan
    /// window. If the file is locked right now, back off briefly and retry;
    /// giving up only warns - SQLite then behaves exactly as before.
    fn prime_wal_index(&self) {
        let shm_path = self.db_path.as_os_str().to_string_lossy() + "-shm";
        let shm_path = PathBuf::from(&*shm_path);
        for attempt in 0..5u32 {
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(&shm_path)
            {
                // Dropping the handle immediately: SQLite opens the same file
                // with its own locking; a lingering handle of ours is neither
                // needed nor wanted.
                Ok(_) => return,
                Err(err) if attempt < 4 => {
                    std::thread::sleep(Duration::from_millis(40 * u64::from(attempt + 1)));
                    let _ = err;
                }
                Err(err) => {
                    warn!(
                        "wal-index sidecar {} is not writable ({err}); SQLite may \
                         downgrade it to read-only for this process",
                        shm_path.display()
                    );
                    return;
                }
            }
        }
    }

    /// Enable incremental vacuum for new files only. Upgrading an existing
    /// file requires a full rewrite and belongs to explicit maintenance, never
    /// the startup/first-history-read path.
    fn apply_auto_vacuum(&self, conn: &Connection) {
        let current: i64 = conn
            .pragma_query_value(None, "auto_vacuum", |row| row.get(0))
            .unwrap_or(0);
        if current == 2 {
            return;
        }
        let populated = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table')",
                [],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(true);
        if !populated {
            conn.pragma_update(None, "auto_vacuum", 2).ok();
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

    fn parse_bool(value: Option<&Value>) -> Option<i64> {
        match value {
            Some(Value::Bool(flag)) => Some(if *flag { 1 } else { 0 }),
            Some(Value::Number(num)) => num.as_i64(),
            Some(Value::String(text)) => text.parse::<i64>().ok(),
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
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(|item| item.to_string())
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

    fn read_user_agent_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<UserAgentRecord> {
        let tool_names = Self::parse_string_list(row.get(6)?);
        Ok(UserAgentRecord {
            agent_id: row.get(0)?,
            user_id: row.get(1)?,
            name: row.get(2)?,
            description: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            system_prompt: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
            preview_skill: row.get::<_, Option<i64>>(22)?.unwrap_or(0) != 0,
            model_name: row.get::<_, Option<String>>(5)?,
            ability_items: Self::parse_ability_items(row.get(9)?),
            tool_names: tool_names.clone(),
            declared_tool_names: Self::parse_declared_tool_names(row.get(7)?),
            declared_skill_names: Self::parse_string_list(row.get(8)?),
            visible_unit_ids: Self::parse_string_list(row.get(23)?),
            preset_questions: Self::parse_string_list(row.get(18)?),
            access_level: row.get(10)?,
            approval_mode: row.get(11)?,
            is_shared: row.get::<_, Option<i64>>(12)?.unwrap_or(0) != 0,
            status: row.get(13)?,
            icon: row.get(14)?,
            sandbox_container_id: normalize_sandbox_container_id(
                row.get::<_, Option<i64>>(15)?.unwrap_or(1) as i32,
            ),
            created_at: row.get(16)?,
            updated_at: row.get(17)?,
            preset_binding: Self::parse_preset_binding(row.get(19)?),
            silent: row.get::<_, Option<i64>>(20)?.unwrap_or(0) != 0,
            prefer_mother: row.get::<_, Option<i64>>(21)?.unwrap_or(0) != 0,
        })
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
}

#[cfg(test)]
mod tests {
    use super::SqliteStorage;
    use crate::storage::*;
    use rusqlite::Connection;
    use tempfile::tempdir;

    fn sample_user(
        user_id: &str,
        quota_balance: i64,
        quota_granted_total: i64,
        quota_used_total: i64,
        last_quota_grant_date: Option<&str>,
    ) -> UserAccountRecord {
        UserAccountRecord {
            user_id: user_id.to_string(),
            username: user_id.to_string(),
            email: None,
            password_hash: "hash".to_string(),
            roles: vec!["user".to_string()],
            status: "active".to_string(),
            access_level: "A".to_string(),
            unit_id: None,
            quota_balance,
            quota_granted_total,
            quota_used_total,
            last_quota_grant_date: last_quota_grant_date.map(str::to_string),
            experience_total: 0,
            is_demo: false,
            created_at: 1.0,
            updated_at: 1.0,
            last_login_at: None,
        }
    }

    #[test]
    fn prepare_user_quota_grants_once_per_day() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("prepare-user-quota.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize storage");
        storage
            .upsert_user_account(&sample_user("alice", 5, 15, 2, Some("2026-04-09")))
            .expect("insert user");

        let first = storage
            .prepare_user_quota("alice", "2026-04-10", 100)
            .expect("prepare first")
            .expect("status");
        assert_eq!(first.balance, 105);
        assert_eq!(first.granted_total, 115);
        assert_eq!(first.used_total, 2);
        assert_eq!(first.last_grant_date.as_deref(), Some("2026-04-10"));
        assert!(first.allowed);

        let second = storage
            .prepare_user_quota("alice", "2026-04-10", 100)
            .expect("prepare second")
            .expect("status");
        assert_eq!(second.balance, 105);
        assert_eq!(second.granted_total, 115);
        assert_eq!(second.used_total, 2);
        assert_eq!(second.last_grant_date.as_deref(), Some("2026-04-10"));

        let account = storage
            .get_user_account("alice")
            .expect("load user")
            .expect("user exists");
        assert_eq!(account.quota_balance, 105);
        assert_eq!(account.quota_granted_total, 115);
        assert_eq!(account.quota_used_total, 2);
    }

    #[test]
    fn consume_user_quota_rejects_overdraft_without_spending() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("consume-user-quota.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize storage");
        storage
            .upsert_user_account(&sample_user("user_1", 50, 50, 10, Some("2026-04-09")))
            .expect("insert user");

        let status = storage
            .consume_user_quota("user_1", "2026-04-10", 100, 180)
            .expect("consume quota")
            .expect("status");
        assert_eq!(status.balance, 150);
        assert_eq!(status.granted_total, 150);
        assert_eq!(status.used_total, 10);
        assert_eq!(status.daily_grant, 100);

        assert_eq!(status.last_grant_date.as_deref(), Some("2026-04-10"));
        assert!(!status.allowed);

        let account = storage
            .get_user_account("user_1")
            .expect("load user")
            .expect("user exists");
        assert_eq!(account.quota_balance, 150);
        assert_eq!(account.quota_granted_total, 150);
        assert_eq!(account.quota_used_total, 10);
        assert_eq!(account.last_quota_grant_date.as_deref(), Some("2026-04-10"));
    }

    #[test]
    fn grant_user_quota_updates_balance_and_granted_total() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("grant-user-quota.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize storage");
        storage
            .upsert_user_account(&sample_user("alice", 7, 20, 3, Some("2026-04-09")))
            .expect("insert user");

        let status = storage
            .grant_user_quota("alice", "2026-04-10", 100, 30, 123.0)
            .expect("grant tokens")
            .expect("status");
        assert_eq!(status.balance, 137);
        assert_eq!(status.granted_total, 150);
        assert_eq!(status.used_total, 3);
        assert_eq!(status.daily_grant, 100);
        assert_eq!(status.last_grant_date.as_deref(), Some("2026-04-10"));
        assert!(status.allowed);

        let account = storage
            .get_user_account("alice")
            .expect("load user")
            .expect("user exists");
        assert_eq!(account.quota_balance, 137);
        assert_eq!(account.quota_granted_total, 150);
        assert_eq!(account.quota_used_total, 3);
        assert_eq!(account.last_quota_grant_date.as_deref(), Some("2026-04-10"));
    }

    #[test]
    fn startup_preserves_existing_database_without_full_vacuum() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("auto-vacuum-upgrade.db");
        let conn = Connection::open(&db_path).expect("open sqlite");
        conn.execute_batch(
            "CREATE TABLE placeholder (id INTEGER PRIMARY KEY);
             INSERT INTO placeholder VALUES (1);",
        )
        .expect("seed existing database");
        let before: i64 = conn
            .pragma_query_value(None, "auto_vacuum", |row| row.get(0))
            .expect("read initial auto_vacuum");
        assert_eq!(before, 0);
        drop(conn);

        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize storage");

        let conn = Connection::open(&db_path).expect("open upgraded sqlite");
        let mode: i64 = conn
            .pragma_query_value(None, "auto_vacuum", |row| row.get(0))
            .expect("read upgraded auto_vacuum");
        assert_eq!(mode, 0, "startup must not rewrite an existing database");
        let value: i64 = conn
            .query_row("SELECT id FROM placeholder", [], |r| r.get(0))
            .unwrap();
        assert_eq!(value, 1);
    }

    #[test]
    fn new_database_enables_incremental_vacuum() {
        let temp = tempdir().unwrap();
        let storage = SqliteStorage::new(temp.path().join("fixture.db").to_string_lossy().into());
        storage.ensure_initialized().unwrap();
        assert_eq!(
            storage
                .read_connection()
                .unwrap()
                .pragma_query_value(None, "auto_vacuum", |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
    }
}
