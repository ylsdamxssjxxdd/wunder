use super::{TOOL_LOG_EXCLUDED_NAMES, TOOL_LOG_SKILL_READ_MARKER};
use crate::i18n;
use crate::storage::{
    normalize_hive_id, normalize_sandbox_container_id, AgentTaskRecord, AgentThreadRecord,
    ChannelAccountRecord, ChannelBindingRecord, ChannelMessageRecord, ChannelOutboxRecord,
    ChannelSessionRecord, ChannelUserBindingRecord, ChatSessionRecord, CronJobRecord,
    CronRunRecord, ExternalLinkRecord, GatewayClientRecord, GatewayNodeRecord,
    GatewayNodeTokenRecord, HiveRecord, ListChannelUserBindingsQuery, MediaAssetRecord,
    OrgUnitRecord, SessionLockRecord, SessionLockStatus, SessionRunRecord, SpeechJobRecord,
    StorageBackend, TeamRunRecord, TeamTaskRecord, UpdateAgentTaskStatusParams,
    UpdateChannelOutboxStatusParams, UpsertMemoryTaskLogParams, UserAccountRecord,
    UserAgentAccessRecord, UserAgentRecord, UserQuotaStatus, UserTokenRecord, UserToolAccessRecord,
    UserWorldConversationRecord, UserWorldConversationSummaryRecord, UserWorldEventRecord,
    UserWorldMemberRecord, UserWorldMessageRecord, UserWorldReadResult, UserWorldSendMessageResult,
    VectorDocumentRecord, VectorDocumentSummaryRecord,
};
use anyhow::{anyhow, Result};
use chrono::Utc;
use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio_postgres::types::ToSql;
use tokio_postgres::NoTls;

const DEFAULT_POOL_SIZE: usize = 64;

pub struct PostgresStorage {
    pool: Pool,
    initialized: AtomicBool,
    init_guard: Mutex<()>,
    fallback_runtime: tokio::runtime::Runtime,
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
        let fallback_runtime = tokio::runtime::Runtime::new()
            .map_err(|err| anyhow!("create tokio runtime for postgres: {err}"))?;
        Ok(Self {
            pool,
            initialized: AtomicBool::new(false),
            init_guard: Mutex::new(()),
            fallback_runtime,
        })
    }

    fn block_on<F, T>(&self, fut: F) -> Result<T>
    where
        F: Future<Output = T>,
    {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => Ok(tokio::task::block_in_place(|| handle.block_on(fut))),
            Err(_) => Ok(self.fallback_runtime.block_on(fut)),
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

    fn normalize_user_world_pair(user_a: &str, user_b: &str) -> Option<(String, String)> {
        let a = user_a.trim();
        let b = user_b.trim();
        if a.is_empty() || b.is_empty() || a == b {
            return None;
        }
        if a <= b {
            Some((a.to_string(), b.to_string()))
        } else {
            Some((b.to_string(), a.to_string()))
        }
    }

    fn parse_json_column(value: Option<String>) -> Value {
        value
            .as_deref()
            .and_then(Self::json_from_str)
            .unwrap_or(Value::Null)
    }

    fn map_user_world_conversation_row(row: &tokio_postgres::Row) -> UserWorldConversationRecord {
        UserWorldConversationRecord {
            conversation_id: row.get(0),
            conversation_type: row.get(1),
            participant_a: row.get(2),
            participant_b: row.get(3),
            created_at: row.get(4),
            updated_at: row.get(5),
            last_message_at: row.get(6),
            last_message_id: row.get(7),
            last_message_preview: row.get(8),
        }
    }

    fn map_user_world_member_row(row: &tokio_postgres::Row) -> UserWorldMemberRecord {
        UserWorldMemberRecord {
            conversation_id: row.get(0),
            user_id: row.get(1),
            peer_user_id: row.get(2),
            last_read_message_id: row.get(3),
            unread_count_cache: row.get(4),
            pinned: row.get::<_, i32>(5) != 0,
            muted: row.get::<_, i32>(6) != 0,
            updated_at: row.get(7),
        }
    }

    fn map_user_world_message_row(row: &tokio_postgres::Row) -> UserWorldMessageRecord {
        UserWorldMessageRecord {
            message_id: row.get(0),
            conversation_id: row.get(1),
            sender_user_id: row.get(2),
            content: row.get(3),
            content_type: row.get(4),
            client_msg_id: row.get(5),
            created_at: row.get(6),
        }
    }

    fn conn(&self) -> Result<PgConn<'_>> {
        let client = self.block_on(self.pool.get())??;
        Ok(PgConn {
            storage: self,
            client,
        })
    }

    fn ensure_user_account_quota_columns(&self, conn: &mut PgConn<'_>) -> Result<()> {
        let rows = conn.query(
            "SELECT column_name FROM information_schema.columns WHERE table_name = 'user_accounts'",
            &[],
        )?;
        let mut columns = HashSet::new();
        for row in rows {
            let name: String = row.get(0);
            columns.insert(name);
        }
        let mut quota_added = false;
        if !columns.contains("daily_quota") {
            conn.execute(
                "ALTER TABLE user_accounts ADD COLUMN daily_quota BIGINT NOT NULL DEFAULT 10000",
                &[],
            )?;
            quota_added = true;
        }
        if !columns.contains("daily_quota_used") {
            conn.execute(
                "ALTER TABLE user_accounts ADD COLUMN daily_quota_used BIGINT NOT NULL DEFAULT 0",
                &[],
            )?;
        }
        if !columns.contains("daily_quota_date") {
            conn.execute(
                "ALTER TABLE user_accounts ADD COLUMN daily_quota_date TEXT",
                &[],
            )?;
        }
        if quota_added {
            conn.execute("UPDATE user_accounts SET daily_quota = 10000", &[])?;
        }
        Ok(())
    }

    fn ensure_user_account_unit_columns(&self, conn: &mut PgConn<'_>) -> Result<()> {
        let rows = conn.query(
            "SELECT column_name FROM information_schema.columns WHERE table_name = 'user_accounts'",
            &[],
        )?;
        let mut columns = HashSet::new();
        for row in rows {
            let name: String = row.get(0);
            columns.insert(name);
        }
        if !columns.contains("unit_id") {
            conn.execute("ALTER TABLE user_accounts ADD COLUMN unit_id TEXT", &[])?;
        }
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_user_accounts_unit ON user_accounts (unit_id)",
            &[],
        );
        Ok(())
    }

    fn ensure_user_account_list_indexes(&self, conn: &mut PgConn<'_>) -> Result<()> {
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_user_accounts_created ON user_accounts (created_at)",
            &[],
        );
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_user_accounts_unit_created ON user_accounts (unit_id, created_at)",
            &[],
        );
        Ok(())
    }

    fn ensure_user_tool_access_columns(&self, conn: &mut PgConn<'_>) -> Result<()> {
        let _ = conn;
        Ok(())
    }

    fn ensure_chat_session_columns(&self, conn: &mut PgConn<'_>) -> Result<()> {
        let rows = conn.query(
            "SELECT column_name FROM information_schema.columns WHERE table_name = 'chat_sessions'",
            &[],
        )?;
        let mut columns = HashSet::new();
        for row in rows {
            let name: String = row.get(0);
            columns.insert(name);
        }
        if !columns.contains("status") {
            conn.execute("ALTER TABLE chat_sessions ADD COLUMN status TEXT", &[])?;
        }
        if !columns.contains("agent_id") {
            conn.execute("ALTER TABLE chat_sessions ADD COLUMN agent_id TEXT", &[])?;
        }
        if !columns.contains("tool_overrides") {
            conn.execute(
                "ALTER TABLE chat_sessions ADD COLUMN tool_overrides TEXT",
                &[],
            )?;
        }
        if !columns.contains("parent_session_id") {
            conn.execute(
                "ALTER TABLE chat_sessions ADD COLUMN parent_session_id TEXT",
                &[],
            )?;
        }
        if !columns.contains("parent_message_id") {
            conn.execute(
                "ALTER TABLE chat_sessions ADD COLUMN parent_message_id TEXT",
                &[],
            )?;
        }
        if !columns.contains("spawn_label") {
            conn.execute("ALTER TABLE chat_sessions ADD COLUMN spawn_label TEXT", &[])?;
        }
        if !columns.contains("spawned_by") {
            conn.execute("ALTER TABLE chat_sessions ADD COLUMN spawned_by TEXT", &[])?;
        }
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_chat_sessions_parent \
             ON chat_sessions (user_id, parent_session_id, updated_at)",
            &[],
        );
        Ok(())
    }

    fn ensure_channel_columns(&self, conn: &mut PgConn<'_>) -> Result<()> {
        fn ensure_table_columns(
            conn: &mut PgConn<'_>,
            table: &str,
            columns: &[(&str, &str)],
        ) -> Result<()> {
            let rows = conn.query(
                "SELECT column_name FROM information_schema.columns WHERE table_name = $1",
                &[&table],
            )?;
            let mut existing = HashSet::new();
            for row in rows {
                let name: String = row.get(0);
                existing.insert(name);
            }
            for (name, ddl) in columns {
                if !existing.contains(*name) {
                    conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {ddl}"), &[])?;
                }
            }
            Ok(())
        }

        ensure_table_columns(
            conn,
            "channel_accounts",
            &[
                ("config", "config TEXT NOT NULL DEFAULT '{}'"),
                ("status", "status TEXT NOT NULL DEFAULT 'active'"),
                (
                    "created_at",
                    "created_at DOUBLE PRECISION NOT NULL DEFAULT 0",
                ),
                (
                    "updated_at",
                    "updated_at DOUBLE PRECISION NOT NULL DEFAULT 0",
                ),
            ],
        )?;
        ensure_table_columns(
            conn,
            "channel_bindings",
            &[
                ("channel", "channel TEXT"),
                ("account_id", "account_id TEXT"),
                ("peer_kind", "peer_kind TEXT"),
                ("peer_id", "peer_id TEXT"),
                ("agent_id", "agent_id TEXT"),
                ("tool_overrides", "tool_overrides TEXT"),
                ("priority", "priority BIGINT NOT NULL DEFAULT 0"),
                ("enabled", "enabled INTEGER NOT NULL DEFAULT 1"),
                (
                    "created_at",
                    "created_at DOUBLE PRECISION NOT NULL DEFAULT 0",
                ),
                (
                    "updated_at",
                    "updated_at DOUBLE PRECISION NOT NULL DEFAULT 0",
                ),
            ],
        )?;
        ensure_table_columns(
            conn,
            "channel_user_bindings",
            &[
                ("user_id", "user_id TEXT NOT NULL DEFAULT ''"),
                (
                    "created_at",
                    "created_at DOUBLE PRECISION NOT NULL DEFAULT 0",
                ),
                (
                    "updated_at",
                    "updated_at DOUBLE PRECISION NOT NULL DEFAULT 0",
                ),
            ],
        )?;
        ensure_table_columns(
            conn,
            "channel_sessions",
            &[
                ("thread_id", "thread_id TEXT NOT NULL DEFAULT ''"),
                ("session_id", "session_id TEXT NOT NULL DEFAULT ''"),
                ("agent_id", "agent_id TEXT"),
                ("user_id", "user_id TEXT NOT NULL DEFAULT ''"),
                ("tts_enabled", "tts_enabled INTEGER"),
                ("tts_voice", "tts_voice TEXT"),
                ("metadata", "metadata TEXT"),
                (
                    "last_message_at",
                    "last_message_at DOUBLE PRECISION NOT NULL DEFAULT 0",
                ),
                (
                    "created_at",
                    "created_at DOUBLE PRECISION NOT NULL DEFAULT 0",
                ),
                (
                    "updated_at",
                    "updated_at DOUBLE PRECISION NOT NULL DEFAULT 0",
                ),
            ],
        )?;
        let _ = conn.execute(
            "ALTER TABLE channel_sessions ALTER COLUMN thread_id SET DEFAULT ''",
            &[],
        );
        let _ = conn.execute(
            "UPDATE channel_sessions SET thread_id = '' WHERE thread_id IS NULL",
            &[],
        );
        ensure_table_columns(
            conn,
            "channel_messages",
            &[
                ("thread_id", "thread_id TEXT"),
                ("session_id", "session_id TEXT"),
                ("message_id", "message_id TEXT"),
                ("sender_id", "sender_id TEXT"),
                ("message_type", "message_type TEXT"),
                ("payload", "payload TEXT NOT NULL DEFAULT '{}'"),
                ("raw_payload", "raw_payload TEXT"),
                (
                    "created_at",
                    "created_at DOUBLE PRECISION NOT NULL DEFAULT 0",
                ),
            ],
        )?;
        ensure_table_columns(
            conn,
            "channel_outbox",
            &[
                ("thread_id", "thread_id TEXT"),
                ("payload", "payload TEXT NOT NULL DEFAULT '{}'"),
                ("status", "status TEXT NOT NULL DEFAULT 'pending'"),
                ("retry_count", "retry_count BIGINT NOT NULL DEFAULT 0"),
                ("retry_at", "retry_at DOUBLE PRECISION NOT NULL DEFAULT 0"),
                ("last_error", "last_error TEXT"),
                (
                    "created_at",
                    "created_at DOUBLE PRECISION NOT NULL DEFAULT 0",
                ),
                (
                    "updated_at",
                    "updated_at DOUBLE PRECISION NOT NULL DEFAULT 0",
                ),
                ("delivered_at", "delivered_at DOUBLE PRECISION"),
            ],
        )?;
        Ok(())
    }

    fn ensure_session_lock_columns(&self, conn: &mut PgConn<'_>) -> Result<()> {
        let rows = conn.query(
            "SELECT column_name FROM information_schema.columns WHERE table_name = 'session_locks'",
            &[],
        )?;
        let mut columns = HashSet::new();
        for row in rows {
            let name: String = row.get(0);
            columns.insert(name);
        }
        if !columns.contains("agent_id") {
            conn.execute(
                "ALTER TABLE session_locks ADD COLUMN agent_id TEXT NOT NULL DEFAULT ''",
                &[],
            )?;
        }
        let _ = conn.execute("DROP INDEX IF EXISTS idx_session_locks_user", &[]);
        let _ = conn.execute("DROP INDEX IF EXISTS idx_session_locks_user_agent", &[]);
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_session_locks_user_agent \
             ON session_locks (user_id, agent_id)",
            &[],
        )?;
        Ok(())
    }

    fn ensure_user_agent_columns(&self, conn: &mut PgConn<'_>) -> Result<()> {
        let rows = conn.query(
            "SELECT column_name FROM information_schema.columns WHERE table_name = 'user_agents'",
            &[],
        )?;
        let mut columns = HashSet::new();
        for row in rows {
            let name: String = row.get(0);
            columns.insert(name);
        }
        if !columns.contains("is_shared") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN is_shared INTEGER NOT NULL DEFAULT 0",
                &[],
            )?;
        }
        if !columns.contains("sandbox_container_id") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN sandbox_container_id INTEGER NOT NULL DEFAULT 1",
                &[],
            )?;
        }
        if !columns.contains("hive_id") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN hive_id TEXT NOT NULL DEFAULT 'default'",
                &[],
            )?;
        }
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_user_agents_user_hive ON user_agents (user_id, hive_id, updated_at)",
            &[],
        )?;
        Ok(())
    }

    fn ensure_monitor_defaults(&self, conn: &mut PgConn<'_>) -> Result<()> {
        conn.execute(
            "UPDATE monitor_sessions SET updated_time = 0 WHERE updated_time IS NULL",
            &[],
        )?;
        conn.execute(
            "ALTER TABLE monitor_sessions ALTER COLUMN updated_time SET DEFAULT 0",
            &[],
        )?;
        Ok(())
    }

    fn ensure_performance_indexes(&self, conn: &mut PgConn<'_>) -> Result<()> {
        let statements = [
            (
                "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_tool_logs_tool_time \
                 ON tool_logs (tool, created_time DESC)",
                "CREATE INDEX IF NOT EXISTS idx_tool_logs_tool_time \
                 ON tool_logs (tool, created_time DESC)",
            ),
            (
                "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_tool_logs_time \
                 ON tool_logs USING brin (created_time)",
                "CREATE INDEX IF NOT EXISTS idx_tool_logs_time \
                 ON tool_logs USING brin (created_time)",
            ),
            (
                "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_chat_history_time \
                 ON chat_history USING brin (created_time)",
                "CREATE INDEX IF NOT EXISTS idx_chat_history_time \
                 ON chat_history USING brin (created_time)",
            ),
            (
                "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_artifact_logs_time \
                 ON artifact_logs USING brin (created_time)",
                "CREATE INDEX IF NOT EXISTS idx_artifact_logs_time \
                 ON artifact_logs USING brin (created_time)",
            ),
            (
                "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_monitor_sessions_updated \
                 ON monitor_sessions (updated_time)",
                "CREATE INDEX IF NOT EXISTS idx_monitor_sessions_updated \
                 ON monitor_sessions (updated_time)",
            ),
            (
                "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_monitor_sessions_user \
                 ON monitor_sessions (user_id)",
                "CREATE INDEX IF NOT EXISTS idx_monitor_sessions_user \
                 ON monitor_sessions (user_id)",
            ),
        ];

        for (concurrent, fallback) in statements {
            if conn.execute(concurrent, &[]).is_err() {
                conn.execute(fallback, &[])?;
            }
        }

        if conn
            .execute(
                "DROP INDEX CONCURRENTLY IF EXISTS idx_user_accounts_username",
                &[],
            )
            .is_err()
        {
            conn.execute("DROP INDEX IF EXISTS idx_user_accounts_username", &[])?;
        }

        Ok(())
    }
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

impl StorageBackend for PostgresStorage {
    fn ensure_initialized(&self) -> Result<()> {
        if self.initialized.load(Ordering::SeqCst) {
            return Ok(());
        }
        let _guard = self.init_guard.lock();
        if self.initialized.load(Ordering::SeqCst) {
            return Ok(());
        }
        let mut attempts = 0u32;
        loop {
            attempts += 1;
            let mut conn = match self.conn() {
                Ok(conn) => conn,
                Err(err) => {
                    if attempts >= 5 {
                        return Err(err);
                    }
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                }
            };
            let result = conn.batch_execute(
                r#"
                CREATE TABLE IF NOT EXISTS meta (
                  key TEXT PRIMARY KEY,
                  value TEXT NOT NULL,
                  updated_time DOUBLE PRECISION NOT NULL
                );
                CREATE TABLE IF NOT EXISTS chat_history (
                  id BIGSERIAL PRIMARY KEY,
                  user_id TEXT NOT NULL,
                  session_id TEXT NOT NULL,
                  role TEXT NOT NULL,
                  content TEXT,
                  timestamp TEXT,
                  meta TEXT,
                  payload TEXT NOT NULL,
                  created_time DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_chat_history_session
                  ON chat_history (user_id, session_id, id);
                CREATE TABLE IF NOT EXISTS tool_logs (
                  id BIGSERIAL PRIMARY KEY,
                  user_id TEXT NOT NULL,
                  session_id TEXT NOT NULL,
                  tool TEXT,
                  ok INTEGER,
                  error TEXT,
                  args TEXT,
                  data TEXT,
                  timestamp TEXT,
                  payload TEXT NOT NULL,
                  created_time DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_tool_logs_session
                  ON tool_logs (user_id, session_id, id);
                CREATE TABLE IF NOT EXISTS artifact_logs (
                  id BIGSERIAL PRIMARY KEY,
                  user_id TEXT NOT NULL,
                  session_id TEXT NOT NULL,
                  kind TEXT NOT NULL,
                  name TEXT,
                  payload TEXT NOT NULL,
                  created_time DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_artifact_logs_session
                  ON artifact_logs (user_id, session_id, id);
                CREATE TABLE IF NOT EXISTS monitor_sessions (
                  session_id TEXT PRIMARY KEY,
                  user_id TEXT,
                  status TEXT,
                  updated_time DOUBLE PRECISION NOT NULL DEFAULT 0,
                  payload TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_monitor_sessions_status
                  ON monitor_sessions (status);
                CREATE TABLE IF NOT EXISTS session_locks (
                  session_id TEXT PRIMARY KEY,
                  user_id TEXT NOT NULL,
                  agent_id TEXT NOT NULL DEFAULT '',
                  created_time DOUBLE PRECISION NOT NULL,
                  updated_time DOUBLE PRECISION NOT NULL,
                  expires_at DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_session_locks_user_agent
                  ON session_locks (user_id, agent_id);
                CREATE INDEX IF NOT EXISTS idx_session_locks_expires
                  ON session_locks (expires_at);
                CREATE TABLE IF NOT EXISTS agent_threads (
                  thread_id TEXT PRIMARY KEY,
                  user_id TEXT NOT NULL,
                  agent_id TEXT NOT NULL DEFAULT '',
                  session_id TEXT NOT NULL,
                  status TEXT NOT NULL,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL,
                  UNIQUE(user_id, agent_id)
                );
                CREATE INDEX IF NOT EXISTS idx_agent_threads_user
                  ON agent_threads (user_id);
                CREATE TABLE IF NOT EXISTS agent_tasks (
                  task_id TEXT PRIMARY KEY,
                  thread_id TEXT NOT NULL,
                  user_id TEXT NOT NULL,
                  agent_id TEXT NOT NULL DEFAULT '',
                  session_id TEXT NOT NULL,
                  status TEXT NOT NULL,
                  request_payload TEXT NOT NULL,
                  request_id TEXT,
                  retry_count BIGINT NOT NULL DEFAULT 0,
                  retry_at DOUBLE PRECISION NOT NULL,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL,
                  started_at DOUBLE PRECISION,
                  finished_at DOUBLE PRECISION,
                  last_error TEXT
                );
                CREATE INDEX IF NOT EXISTS idx_agent_tasks_thread_status
                  ON agent_tasks (thread_id, status, retry_at);
                CREATE INDEX IF NOT EXISTS idx_agent_tasks_status
                  ON agent_tasks (status, retry_at);
                CREATE INDEX IF NOT EXISTS idx_agent_tasks_user
                  ON agent_tasks (user_id, agent_id);
                CREATE TABLE IF NOT EXISTS stream_events (
                  session_id TEXT NOT NULL,
                  event_id BIGINT NOT NULL,
                  user_id TEXT NOT NULL,
                  payload TEXT NOT NULL,
                  created_time DOUBLE PRECISION NOT NULL,
                  PRIMARY KEY (session_id, event_id)
                );
                CREATE INDEX IF NOT EXISTS idx_stream_events_user
                  ON stream_events (user_id);
                CREATE INDEX IF NOT EXISTS idx_stream_events_time
                  ON stream_events (created_time);
                CREATE TABLE IF NOT EXISTS memory_settings (
                  user_id TEXT PRIMARY KEY,
                  enabled INTEGER NOT NULL,
                  updated_time DOUBLE PRECISION NOT NULL
                );
                CREATE TABLE IF NOT EXISTS memory_records (
                  id BIGSERIAL PRIMARY KEY,
                  user_id TEXT NOT NULL,
                  session_id TEXT NOT NULL,
                  summary TEXT NOT NULL,
                  created_time DOUBLE PRECISION NOT NULL,
                  updated_time DOUBLE PRECISION NOT NULL,
                  UNIQUE(user_id, session_id)
                );
                CREATE INDEX IF NOT EXISTS idx_memory_records_user_time
                  ON memory_records (user_id, updated_time);
                CREATE TABLE IF NOT EXISTS memory_task_logs (
                  id BIGSERIAL PRIMARY KEY,
                  task_id TEXT NOT NULL,
                  user_id TEXT NOT NULL,
                  session_id TEXT NOT NULL,
                  status TEXT,
                  queued_time DOUBLE PRECISION,
                  started_time DOUBLE PRECISION,
                  finished_time DOUBLE PRECISION,
                  elapsed_s DOUBLE PRECISION,
                  request_payload TEXT,
                  result TEXT,
                  error TEXT,
                  updated_time DOUBLE PRECISION NOT NULL,
                  UNIQUE(user_id, session_id)
                );
                CREATE INDEX IF NOT EXISTS idx_memory_task_logs_updated
                  ON memory_task_logs (updated_time);
                CREATE INDEX IF NOT EXISTS idx_memory_task_logs_task_id
                  ON memory_task_logs (task_id);
                CREATE TABLE IF NOT EXISTS evaluation_runs (
                  run_id TEXT PRIMARY KEY,
                  user_id TEXT,
                  model_name TEXT,
                  language TEXT,
                  status TEXT,
                  total_score DOUBLE PRECISION,
                  started_time DOUBLE PRECISION,
                  finished_time DOUBLE PRECISION,
                  payload TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_evaluation_runs_user
                  ON evaluation_runs (user_id);
                CREATE INDEX IF NOT EXISTS idx_evaluation_runs_status
                  ON evaluation_runs (status);
                CREATE INDEX IF NOT EXISTS idx_evaluation_runs_started
                  ON evaluation_runs (started_time);
                CREATE TABLE IF NOT EXISTS evaluation_items (
                  id BIGSERIAL PRIMARY KEY,
                  run_id TEXT NOT NULL,
                  case_id TEXT NOT NULL,
                  dimension TEXT,
                  status TEXT,
                  score DOUBLE PRECISION,
                  max_score DOUBLE PRECISION,
                  weight DOUBLE PRECISION,
                  started_time DOUBLE PRECISION,
                  finished_time DOUBLE PRECISION,
                  payload TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_evaluation_items_run
                  ON evaluation_items (run_id, id);
                CREATE TABLE IF NOT EXISTS user_accounts (
                  user_id TEXT PRIMARY KEY,
                  username TEXT NOT NULL UNIQUE,
                  email TEXT,
                  password_hash TEXT NOT NULL,
                  roles TEXT NOT NULL,
                  status TEXT NOT NULL,
                  access_level TEXT NOT NULL,
                  unit_id TEXT,
                  daily_quota BIGINT NOT NULL DEFAULT 10000,
                  daily_quota_used BIGINT NOT NULL DEFAULT 0,
                  daily_quota_date TEXT,
                  is_demo INTEGER NOT NULL DEFAULT 0,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL,
                  last_login_at DOUBLE PRECISION
                );
                CREATE UNIQUE INDEX IF NOT EXISTS idx_user_accounts_email
                  ON user_accounts (email);
                CREATE INDEX IF NOT EXISTS idx_user_accounts_unit
                  ON user_accounts (unit_id);
                CREATE INDEX IF NOT EXISTS idx_user_accounts_created
                  ON user_accounts (created_at);
                CREATE INDEX IF NOT EXISTS idx_user_accounts_unit_created
                  ON user_accounts (unit_id, created_at);
                CREATE TABLE IF NOT EXISTS org_units (
                  unit_id TEXT PRIMARY KEY,
                  parent_id TEXT,
                  name TEXT NOT NULL,
                  level INTEGER NOT NULL,
                  path TEXT NOT NULL,
                  path_name TEXT NOT NULL,
                  sort_order BIGINT NOT NULL DEFAULT 0,
                  leader_ids TEXT NOT NULL,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_org_units_parent
                  ON org_units (parent_id);
                CREATE INDEX IF NOT EXISTS idx_org_units_path
                  ON org_units (path);
                CREATE TABLE IF NOT EXISTS external_links (
                  link_id TEXT PRIMARY KEY,
                  title TEXT NOT NULL,
                  description TEXT NOT NULL,
                  url TEXT NOT NULL,
                  icon TEXT NOT NULL,
                  allowed_levels TEXT NOT NULL,
                  sort_order BIGINT NOT NULL DEFAULT 0,
                  enabled INTEGER NOT NULL DEFAULT 1,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_external_links_order
                  ON external_links (enabled, sort_order, updated_at);
                CREATE TABLE IF NOT EXISTS user_tokens (
                  token TEXT PRIMARY KEY,
                  user_id TEXT NOT NULL,
                  expires_at DOUBLE PRECISION NOT NULL,
                  created_at DOUBLE PRECISION NOT NULL,
                  last_used_at DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_user_tokens_user
                  ON user_tokens (user_id);
                CREATE INDEX IF NOT EXISTS idx_user_tokens_expires
                  ON user_tokens (expires_at);
                CREATE TABLE IF NOT EXISTS user_tool_access (
                  user_id TEXT PRIMARY KEY,
                  allowed_tools TEXT,
                  updated_at DOUBLE PRECISION NOT NULL
                );
                CREATE TABLE IF NOT EXISTS chat_sessions (
                  session_id TEXT PRIMARY KEY,
                  user_id TEXT NOT NULL,
                  title TEXT,
                  status TEXT,
                  agent_id TEXT,
                  tool_overrides TEXT,
                  parent_session_id TEXT,
                  parent_message_id TEXT,
                  spawn_label TEXT,
                  spawned_by TEXT,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL,
                  last_message_at DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_chat_sessions_user
                  ON chat_sessions (user_id);
                CREATE INDEX IF NOT EXISTS idx_chat_sessions_updated
                  ON chat_sessions (user_id, updated_at);
                CREATE INDEX IF NOT EXISTS idx_chat_sessions_parent
                  ON chat_sessions (user_id, parent_session_id, updated_at);
                CREATE TABLE IF NOT EXISTS user_world_conversations (
                  conversation_id TEXT PRIMARY KEY,
                  conversation_type TEXT NOT NULL,
                  participant_a TEXT NOT NULL,
                  participant_b TEXT NOT NULL,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL,
                  last_message_at DOUBLE PRECISION NOT NULL,
                  last_message_id BIGINT,
                  last_message_preview TEXT
                );
                CREATE UNIQUE INDEX IF NOT EXISTS idx_user_world_conversations_participants
                  ON user_world_conversations (participant_a, participant_b);
                CREATE INDEX IF NOT EXISTS idx_user_world_conversations_updated
                  ON user_world_conversations (updated_at DESC);
                CREATE INDEX IF NOT EXISTS idx_user_world_conversations_last_message
                  ON user_world_conversations (last_message_at DESC);
                CREATE TABLE IF NOT EXISTS user_world_members (
                  conversation_id TEXT NOT NULL,
                  user_id TEXT NOT NULL,
                  peer_user_id TEXT NOT NULL,
                  last_read_message_id BIGINT,
                  unread_count_cache BIGINT NOT NULL DEFAULT 0,
                  pinned INTEGER NOT NULL DEFAULT 0,
                  muted INTEGER NOT NULL DEFAULT 0,
                  updated_at DOUBLE PRECISION NOT NULL,
                  PRIMARY KEY (conversation_id, user_id)
                );
                CREATE INDEX IF NOT EXISTS idx_user_world_members_user_updated
                  ON user_world_members (user_id, updated_at DESC);
                CREATE TABLE IF NOT EXISTS user_world_messages (
                  message_id BIGSERIAL PRIMARY KEY,
                  conversation_id TEXT NOT NULL,
                  sender_user_id TEXT NOT NULL,
                  content TEXT NOT NULL,
                  content_type TEXT NOT NULL,
                  client_msg_id TEXT,
                  created_at DOUBLE PRECISION NOT NULL
                );
                CREATE UNIQUE INDEX IF NOT EXISTS idx_user_world_messages_client
                  ON user_world_messages (conversation_id, client_msg_id);
                CREATE INDEX IF NOT EXISTS idx_user_world_messages_conversation
                  ON user_world_messages (conversation_id, message_id DESC);
                CREATE TABLE IF NOT EXISTS user_world_events (
                  conversation_id TEXT NOT NULL,
                  event_id BIGINT NOT NULL,
                  event_type TEXT NOT NULL,
                  payload TEXT NOT NULL,
                  created_time DOUBLE PRECISION NOT NULL,
                  PRIMARY KEY (conversation_id, event_id)
                );
                CREATE INDEX IF NOT EXISTS idx_user_world_events_created_time
                  ON user_world_events (created_time);
                CREATE INDEX IF NOT EXISTS idx_user_world_events_conversation
                  ON user_world_events (conversation_id, event_id);
                CREATE TABLE IF NOT EXISTS session_runs (
                  run_id TEXT PRIMARY KEY,
                  session_id TEXT NOT NULL,
                  parent_session_id TEXT,
                  user_id TEXT NOT NULL,
                  agent_id TEXT,
                  model_name TEXT,
                  status TEXT NOT NULL,
                  queued_time DOUBLE PRECISION,
                  started_time DOUBLE PRECISION,
                  finished_time DOUBLE PRECISION,
                  elapsed_s DOUBLE PRECISION,
                  result TEXT,
                  error TEXT,
                  updated_time DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_session_runs_session
                  ON session_runs (session_id, updated_time);
                CREATE INDEX IF NOT EXISTS idx_session_runs_user
                  ON session_runs (user_id, updated_time);
                CREATE INDEX IF NOT EXISTS idx_session_runs_parent
                  ON session_runs (parent_session_id, updated_time);
                CREATE TABLE IF NOT EXISTS cron_jobs (
                  job_id TEXT PRIMARY KEY,
                  user_id TEXT NOT NULL,
                  session_id TEXT NOT NULL,
                  agent_id TEXT,
                  name TEXT,
                  session_target TEXT NOT NULL,
                  payload TEXT NOT NULL,
                  deliver TEXT,
                  enabled INTEGER NOT NULL,
                  delete_after_run INTEGER NOT NULL,
                  schedule_kind TEXT NOT NULL,
                  schedule_at TEXT,
                  schedule_every_ms BIGINT,
                  schedule_cron TEXT,
                  schedule_tz TEXT,
                  dedupe_key TEXT,
                  next_run_at DOUBLE PRECISION,
                  running_at DOUBLE PRECISION,
                  last_run_at DOUBLE PRECISION,
                  last_status TEXT,
                  last_error TEXT,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_cron_jobs_user
                  ON cron_jobs (user_id, updated_at);
                CREATE INDEX IF NOT EXISTS idx_cron_jobs_next
                  ON cron_jobs (enabled, next_run_at);
                CREATE INDEX IF NOT EXISTS idx_cron_jobs_dedupe
                  ON cron_jobs (user_id, dedupe_key);
                CREATE INDEX IF NOT EXISTS idx_cron_jobs_session
                  ON cron_jobs (user_id, session_id);
                CREATE TABLE IF NOT EXISTS cron_runs (
                  run_id TEXT PRIMARY KEY,
                  job_id TEXT NOT NULL,
                  user_id TEXT NOT NULL,
                  session_id TEXT,
                  agent_id TEXT,
                  trigger TEXT,
                  status TEXT,
                  summary TEXT,
                  error TEXT,
                  duration_ms BIGINT,
                  created_at DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_cron_runs_job
                  ON cron_runs (job_id, created_at);
                CREATE INDEX IF NOT EXISTS idx_cron_runs_user
                  ON cron_runs (user_id, created_at);
                CREATE TABLE IF NOT EXISTS channel_accounts (
                  channel TEXT NOT NULL,
                  account_id TEXT NOT NULL,
                  config TEXT NOT NULL,
                  status TEXT NOT NULL,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL,
                  PRIMARY KEY (channel, account_id)
                );
                CREATE INDEX IF NOT EXISTS idx_channel_accounts_status
                  ON channel_accounts (status);
                CREATE TABLE IF NOT EXISTS channel_bindings (
                  binding_id TEXT PRIMARY KEY,
                  channel TEXT,
                  account_id TEXT,
                  peer_kind TEXT,
                  peer_id TEXT,
                  agent_id TEXT,
                  tool_overrides TEXT,
                  priority BIGINT NOT NULL DEFAULT 0,
                  enabled INTEGER NOT NULL DEFAULT 1,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_channel_bindings_match
                  ON channel_bindings (channel, account_id, peer_kind, peer_id, priority);
                CREATE TABLE IF NOT EXISTS channel_user_bindings (
                  channel TEXT NOT NULL,
                  account_id TEXT NOT NULL,
                  peer_kind TEXT NOT NULL,
                  peer_id TEXT NOT NULL,
                  user_id TEXT NOT NULL,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL,
                  PRIMARY KEY (channel, account_id, peer_kind, peer_id)
                );
                CREATE INDEX IF NOT EXISTS idx_channel_user_bindings_user
                  ON channel_user_bindings (user_id);
                CREATE TABLE IF NOT EXISTS channel_sessions (
                  channel TEXT NOT NULL,
                  account_id TEXT NOT NULL,
                  peer_kind TEXT NOT NULL,
                  peer_id TEXT NOT NULL,
                  thread_id TEXT NOT NULL DEFAULT '',
                  session_id TEXT NOT NULL,
                  agent_id TEXT,
                  user_id TEXT NOT NULL,
                  tts_enabled INTEGER,
                  tts_voice TEXT,
                  metadata TEXT,
                  last_message_at DOUBLE PRECISION NOT NULL,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL,
                  PRIMARY KEY (channel, account_id, peer_kind, peer_id, thread_id)
                );
                CREATE INDEX IF NOT EXISTS idx_channel_sessions_session
                  ON channel_sessions (session_id);
                CREATE INDEX IF NOT EXISTS idx_channel_sessions_peer
                  ON channel_sessions (channel, account_id, peer_id);
                CREATE TABLE IF NOT EXISTS channel_messages (
                  id BIGSERIAL PRIMARY KEY,
                  channel TEXT NOT NULL,
                  account_id TEXT NOT NULL,
                  peer_kind TEXT NOT NULL,
                  peer_id TEXT NOT NULL,
                  thread_id TEXT,
                  session_id TEXT,
                  message_id TEXT,
                  sender_id TEXT,
                  message_type TEXT,
                  payload TEXT NOT NULL,
                  raw_payload TEXT,
                  created_at DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_channel_messages_session
                  ON channel_messages (session_id, id);
                CREATE INDEX IF NOT EXISTS idx_channel_messages_peer
                  ON channel_messages (channel, account_id, peer_id, id);
                CREATE TABLE IF NOT EXISTS channel_outbox (
                  outbox_id TEXT PRIMARY KEY,
                  channel TEXT NOT NULL,
                  account_id TEXT NOT NULL,
                  peer_kind TEXT NOT NULL,
                  peer_id TEXT NOT NULL,
                  thread_id TEXT,
                  payload TEXT NOT NULL,
                  status TEXT NOT NULL,
                  retry_count BIGINT NOT NULL,
                  retry_at DOUBLE PRECISION NOT NULL,
                  last_error TEXT,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL,
                  delivered_at DOUBLE PRECISION
                );
                CREATE INDEX IF NOT EXISTS idx_channel_outbox_status
                  ON channel_outbox (status, retry_at);
                CREATE INDEX IF NOT EXISTS idx_channel_outbox_peer
                  ON channel_outbox (channel, account_id, peer_id);
                CREATE TABLE IF NOT EXISTS gateway_clients (
                  connection_id TEXT PRIMARY KEY,
                  role TEXT NOT NULL,
                  user_id TEXT,
                  node_id TEXT,
                  scopes TEXT,
                  caps TEXT,
                  commands TEXT,
                  client_info TEXT,
                  status TEXT NOT NULL,
                  connected_at DOUBLE PRECISION NOT NULL,
                  last_seen_at DOUBLE PRECISION NOT NULL,
                  disconnected_at DOUBLE PRECISION
                );
                CREATE INDEX IF NOT EXISTS idx_gateway_clients_status
                  ON gateway_clients (status, role);
                CREATE INDEX IF NOT EXISTS idx_gateway_clients_node
                  ON gateway_clients (node_id, status);
                CREATE TABLE IF NOT EXISTS gateway_nodes (
                  node_id TEXT PRIMARY KEY,
                  name TEXT,
                  device_fingerprint TEXT,
                  status TEXT NOT NULL,
                  caps TEXT,
                  commands TEXT,
                  permissions TEXT,
                  metadata TEXT,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL,
                  last_seen_at DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_gateway_nodes_status
                  ON gateway_nodes (status);
                CREATE TABLE IF NOT EXISTS gateway_node_tokens (
                  token TEXT PRIMARY KEY,
                  node_id TEXT NOT NULL,
                  status TEXT NOT NULL,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL,
                  last_used_at DOUBLE PRECISION
                );
                CREATE INDEX IF NOT EXISTS idx_gateway_node_tokens_node
                  ON gateway_node_tokens (node_id, status);
                CREATE TABLE IF NOT EXISTS media_assets (
                  asset_id TEXT PRIMARY KEY,
                  kind TEXT NOT NULL,
                  url TEXT NOT NULL,
                  mime TEXT,
                  size BIGINT,
                  hash TEXT,
                  source TEXT,
                  created_at DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_media_assets_hash
                  ON media_assets (hash);
                CREATE TABLE IF NOT EXISTS speech_jobs (
                  job_id TEXT PRIMARY KEY,
                  job_type TEXT NOT NULL,
                  status TEXT NOT NULL,
                  input_text TEXT,
                  input_url TEXT,
                  output_text TEXT,
                  output_url TEXT,
                  model TEXT,
                  error TEXT,
                  retry_count BIGINT NOT NULL,
                  next_retry_at DOUBLE PRECISION NOT NULL,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL,
                  metadata TEXT
                );
                CREATE INDEX IF NOT EXISTS idx_speech_jobs_status
                  ON speech_jobs (job_type, status, next_retry_at);
                CREATE TABLE IF NOT EXISTS hives (
                  hive_id TEXT PRIMARY KEY,
                  user_id TEXT NOT NULL,
                  name TEXT NOT NULL,
                  description TEXT,
                  is_default INTEGER NOT NULL DEFAULT 0,
                  status TEXT NOT NULL DEFAULT 'active',
                  created_time DOUBLE PRECISION NOT NULL,
                  updated_time DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_hives_user
                  ON hives (user_id, updated_time);
                CREATE INDEX IF NOT EXISTS idx_hives_user_status
                  ON hives (user_id, status, updated_time);
                CREATE TABLE IF NOT EXISTS user_agents (
                  agent_id TEXT PRIMARY KEY,
                  user_id TEXT NOT NULL,
                  hive_id TEXT NOT NULL DEFAULT 'default',
                  name TEXT NOT NULL,
                  description TEXT,
                  system_prompt TEXT,
                  tool_names TEXT,
                  access_level TEXT NOT NULL,
                  is_shared INTEGER NOT NULL DEFAULT 0,
                  status TEXT NOT NULL,
                  icon TEXT,
                  sandbox_container_id INTEGER NOT NULL DEFAULT 1,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_user_agents_user
                  ON user_agents (user_id, updated_at);
                CREATE INDEX IF NOT EXISTS idx_user_agents_user_hive
                  ON user_agents (user_id, hive_id, updated_at);
                CREATE TABLE IF NOT EXISTS user_agent_access (
                  user_id TEXT PRIMARY KEY,
                  allowed_agent_ids TEXT,
                  blocked_agent_ids TEXT,
                  updated_at DOUBLE PRECISION NOT NULL
                );
                CREATE TABLE IF NOT EXISTS team_runs (
                  team_run_id TEXT PRIMARY KEY,
                  user_id TEXT NOT NULL,
                  hive_id TEXT NOT NULL,
                  parent_session_id TEXT NOT NULL,
                  parent_agent_id TEXT,
                  strategy TEXT NOT NULL,
                  status TEXT NOT NULL,
                  task_total BIGINT NOT NULL DEFAULT 0,
                  task_success BIGINT NOT NULL DEFAULT 0,
                  task_failed BIGINT NOT NULL DEFAULT 0,
                  context_tokens_total BIGINT NOT NULL DEFAULT 0,
                  context_tokens_peak BIGINT NOT NULL DEFAULT 0,
                  model_round_total BIGINT NOT NULL DEFAULT 0,
                  started_time DOUBLE PRECISION,
                  finished_time DOUBLE PRECISION,
                  elapsed_s DOUBLE PRECISION,
                  summary TEXT,
                  error TEXT,
                  updated_time DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_team_runs_user_hive
                  ON team_runs (user_id, hive_id, updated_time);
                CREATE INDEX IF NOT EXISTS idx_team_runs_hive_status
                  ON team_runs (hive_id, status, updated_time);
                CREATE INDEX IF NOT EXISTS idx_team_runs_hive_parent
                  ON team_runs (hive_id, parent_session_id, updated_time);
                CREATE TABLE IF NOT EXISTS team_tasks (
                  task_id TEXT PRIMARY KEY,
                  team_run_id TEXT NOT NULL REFERENCES team_runs(team_run_id) ON DELETE CASCADE,
                  user_id TEXT NOT NULL,
                  hive_id TEXT NOT NULL,
                  agent_id TEXT NOT NULL,
                  target_session_id TEXT,
                  spawned_session_id TEXT,
                  status TEXT NOT NULL,
                  retry_count BIGINT NOT NULL DEFAULT 0,
                  priority BIGINT NOT NULL DEFAULT 0,
                  started_time DOUBLE PRECISION,
                  finished_time DOUBLE PRECISION,
                  elapsed_s DOUBLE PRECISION,
                  result_summary TEXT,
                  error TEXT,
                  updated_time DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_team_tasks_hive_run
                  ON team_tasks (hive_id, team_run_id, updated_time);
                CREATE INDEX IF NOT EXISTS idx_team_tasks_user_hive_agent
                  ON team_tasks (user_id, hive_id, agent_id, updated_time);
                CREATE INDEX IF NOT EXISTS idx_team_tasks_hive_status
                  ON team_tasks (hive_id, status, updated_time);
                CREATE TABLE IF NOT EXISTS vector_documents (
                  doc_id TEXT PRIMARY KEY,
                  owner_id TEXT NOT NULL,
                  base_name TEXT NOT NULL,
                  doc_name TEXT NOT NULL,
                  embedding_model TEXT NOT NULL,
                  chunk_size BIGINT NOT NULL,
                  chunk_overlap BIGINT NOT NULL,
                  chunk_count BIGINT NOT NULL,
                  status TEXT NOT NULL,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL,
                  content TEXT NOT NULL,
                  chunks_json TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_vector_documents_owner_base
                  ON vector_documents (owner_id, base_name, updated_at);
                "#,
            );
            match result {
                Ok(_) => {
                    self.ensure_monitor_defaults(&mut conn)?;
                    self.ensure_user_account_quota_columns(&mut conn)?;
                    self.ensure_user_account_unit_columns(&mut conn)?;
                    self.ensure_user_account_list_indexes(&mut conn)?;
                    self.ensure_user_tool_access_columns(&mut conn)?;
                    self.ensure_chat_session_columns(&mut conn)?;
                    self.ensure_channel_columns(&mut conn)?;
                    self.ensure_session_lock_columns(&mut conn)?;
                    self.ensure_user_agent_columns(&mut conn)?;
                    self.ensure_performance_indexes(&mut conn)?;
                    self.initialized.store(true, Ordering::SeqCst);
                    return Ok(());
                }
                Err(err) => {
                    if attempts >= 5 {
                        return Err(err);
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        }
    }

    fn get_meta(&self, key: &str) -> Result<Option<String>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let row = conn.query_opt("SELECT value FROM meta WHERE key = $1", &[&key])?;
        Ok(row.map(|row| row.get::<_, String>(0)))
    }

    fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.ensure_initialized()?;
        let now = Self::now_ts();
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO meta (key, value, updated_time) VALUES ($1, $2, $3) \
             ON CONFLICT(key) DO UPDATE SET value = EXCLUDED.value, updated_time = EXCLUDED.updated_time",
            &[&key, &value, &now],
        )?;
        Ok(())
    }

    fn delete_meta_prefix(&self, prefix: &str) -> Result<usize> {
        self.ensure_initialized()?;
        let cleaned = prefix.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let pattern = format!("{cleaned}%");
        let mut conn = self.conn()?;
        let affected = conn.execute("DELETE FROM meta WHERE key LIKE $1", &[&pattern])?;
        Ok(affected as usize)
    }

    fn append_chat(&self, user_id: &str, payload: &Value) -> Result<()> {
        self.ensure_initialized()?;
        let session_id = payload
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if session_id.is_empty() {
            return Ok(());
        }
        let role = payload
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if role.is_empty() {
            return Ok(());
        }
        let content = Self::parse_string(payload.get("content"));
        let timestamp = Self::parse_string(payload.get("timestamp"));
        let meta = payload
            .get("meta")
            .and_then(|value| serde_json::to_string(value).ok());
        let payload_text = Self::json_to_string(payload);
        let now = Self::now_ts();
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO chat_history (user_id, session_id, role, content, timestamp, meta, payload, created_time) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            &[
                &user_id,
                &session_id,
                &role,
                &content,
                &timestamp,
                &meta,
                &payload_text,
                &now,
            ],
        )?;
        Ok(())
    }

    fn append_tool_log(&self, user_id: &str, payload: &Value) -> Result<()> {
        self.ensure_initialized()?;
        let session_id = payload
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if session_id.is_empty() {
            return Ok(());
        }
        let tool = Self::parse_string(payload.get("tool"));
        let ok = Self::parse_bool(payload.get("ok"));
        let error = Self::parse_string(payload.get("error"));
        let args = payload
            .get("args")
            .and_then(|value| serde_json::to_string(value).ok());
        let data = payload
            .get("data")
            .and_then(|value| serde_json::to_string(value).ok());
        let timestamp = Self::parse_string(payload.get("timestamp"));
        let omit_payload = payload
            .get("__omit_payload")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let payload_text = if omit_payload {
            "{}".to_string()
        } else {
            Self::json_to_string(payload)
        };
        let now = Self::now_ts();
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO tool_logs (user_id, session_id, tool, ok, error, args, data, timestamp, payload, created_time) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            &[
                &user_id,
                &session_id,
                &tool,
                &ok,
                &error,
                &args,
                &data,
                &timestamp,
                &payload_text,
                &now,
            ],
        )?;
        Ok(())
    }

    fn append_artifact_log(&self, user_id: &str, payload: &Value) -> Result<()> {
        self.ensure_initialized()?;
        let session_id = payload
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let kind = payload
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if session_id.is_empty() || kind.is_empty() {
            return Ok(());
        }
        let name = Self::parse_string(payload.get("name"));
        let payload_text = Self::json_to_string(payload);
        let now = Self::now_ts();
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO artifact_logs (user_id, session_id, kind, name, payload, created_time) \
             VALUES ($1, $2, $3, $4, $5, $6)",
            &[&user_id, &session_id, &kind, &name, &payload_text, &now],
        )?;
        Ok(())
    }

    fn load_chat_history(
        &self,
        user_id: &str,
        session_id: &str,
        limit: Option<i64>,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let limit_value = limit.filter(|value| *value > 0);
        let mut conn = self.conn()?;
        let mut rows: Vec<String> = if let Some(limit_value) = limit_value {
            conn.query(
                "SELECT payload FROM chat_history WHERE user_id = $1 AND session_id = $2 ORDER BY id DESC LIMIT $3",
                &[&user_id, &session_id, &limit_value],
            )?
            .into_iter()
            .map(|row| row.get::<_, String>(0))
            .collect()
        } else {
            conn.query(
                "SELECT payload FROM chat_history WHERE user_id = $1 AND session_id = $2 ORDER BY id ASC",
                &[&user_id, &session_id],
            )?
            .into_iter()
            .map(|row| row.get::<_, String>(0))
            .collect()
        };
        if limit_value.is_some() {
            rows.reverse();
        }
        let mut records = Vec::new();
        for payload in rows {
            if let Some(value) = Self::json_from_str(&payload) {
                records.push(value);
            }
        }
        Ok(records)
    }

    fn load_artifact_logs(
        &self,
        user_id: &str,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        if user_id.trim().is_empty() || session_id.trim().is_empty() || limit <= 0 {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let mut rows: Vec<(i64, String)> = conn
            .query(
                "SELECT id, payload FROM artifact_logs WHERE user_id = $1 AND session_id = $2 ORDER BY id DESC LIMIT $3",
                &[&user_id, &session_id, &limit],
            )?
            .into_iter()
            .map(|row| (row.get::<_, i64>(0), row.get::<_, String>(1)))
            .collect();
        rows.reverse();
        let mut records = Vec::new();
        for (artifact_id, payload) in rows {
            if let Some(mut value) = Self::json_from_str(&payload) {
                if let Value::Object(ref mut map) = value {
                    map.insert("artifact_id".to_string(), json!(artifact_id));
                }
                records.push(value);
            }
        }
        Ok(records)
    }

    fn get_session_system_prompt(
        &self,
        user_id: &str,
        session_id: &str,
        language: Option<&str>,
    ) -> Result<Option<String>> {
        self.ensure_initialized()?;
        let normalized_language = language.map(|value| i18n::normalize_language(Some(value), true));
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT payload FROM chat_history WHERE user_id = $1 AND session_id = $2 AND role = 'system' ORDER BY id ASC",
            &[&user_id, &session_id],
        )?;
        for row in rows {
            let payload: String = row.get(0);
            let Some(value) = Self::json_from_str(&payload) else {
                continue;
            };
            let meta = value.get("meta").and_then(Value::as_object);
            let Some(meta) = meta else {
                continue;
            };
            if meta.get("type").and_then(Value::as_str) != Some("system_prompt") {
                continue;
            }
            if let Some(ref normalized) = normalized_language {
                let meta_language = meta
                    .get("language")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                if !meta_language.is_empty() {
                    let meta_normalized = i18n::normalize_language(Some(meta_language), true);
                    if &meta_normalized != normalized {
                        continue;
                    }
                } else if normalized != &i18n::get_default_language() {
                    continue;
                }
            }
            if let Some(content) = value.get("content").and_then(Value::as_str) {
                let cleaned = content.trim();
                if !cleaned.is_empty() {
                    return Ok(Some(cleaned.to_string()));
                }
            }
        }
        Ok(None)
    }

    fn get_user_chat_stats(&self) -> Result<HashMap<String, HashMap<String, i64>>> {
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

    fn get_user_tool_stats(&self) -> Result<HashMap<String, HashMap<String, i64>>> {
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

    fn get_tool_usage_stats(
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

    fn get_tool_session_usage(
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

    fn get_log_usage(&self) -> Result<u64> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let row = conn.query_one(
            "SELECT \
            COALESCE(pg_total_relation_size(to_regclass('chat_history')), 0) + \
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

    fn delete_chat_history(&self, _user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = _user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute("DELETE FROM chat_history WHERE user_id = $1", &[&cleaned])?;
        Ok(affected as i64)
    }

    fn delete_chat_history_by_session(&self, _user_id: &str, _session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = _user_id.trim();
        let cleaned_session = _session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM chat_history WHERE user_id = $1 AND session_id = $2",
            &[&cleaned_user, &cleaned_session],
        )?;
        Ok(affected as i64)
    }

    fn delete_tool_logs(&self, _user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = _user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute("DELETE FROM tool_logs WHERE user_id = $1", &[&cleaned])?;
        Ok(affected as i64)
    }

    fn delete_tool_logs_by_session(&self, _user_id: &str, _session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = _user_id.trim();
        let cleaned_session = _session_id.trim();
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

    fn delete_artifact_logs(&self, _user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = _user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute("DELETE FROM artifact_logs WHERE user_id = $1", &[&cleaned])?;
        Ok(affected as i64)
    }

    fn delete_artifact_logs_by_session(&self, _user_id: &str, _session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = _user_id.trim();
        let cleaned_session = _session_id.trim();
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

    fn upsert_monitor_record(&self, _payload: &Value) -> Result<()> {
        self.ensure_initialized()?;
        let session_id = _payload
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if session_id.is_empty() {
            return Ok(());
        }
        let user_id = _payload
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let status = _payload
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let updated_time = _payload
            .get("updated_time")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let payload_text = Self::json_to_string(_payload);
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO monitor_sessions (session_id, user_id, status, updated_time, payload) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT(session_id) DO UPDATE SET user_id = EXCLUDED.user_id, status = EXCLUDED.status, updated_time = EXCLUDED.updated_time, payload = EXCLUDED.payload",
            &[&session_id, &user_id, &status, &updated_time, &payload_text],
        )?;
        Ok(())
    }

    fn get_monitor_record(&self, session_id: &str) -> Result<Option<Value>> {
        self.ensure_initialized()?;
        let cleaned = session_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT payload FROM monitor_sessions WHERE session_id = $1",
            &[&cleaned],
        )?;
        if let Some(row) = rows.first() {
            let payload: String = row.get(0);
            return Ok(Self::json_from_str(&payload));
        }
        Ok(None)
    }

    fn load_monitor_records(&self) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let rows = conn.query("SELECT payload FROM monitor_sessions", &[])?;
        let mut records = Vec::new();
        for row in rows {
            let payload: String = row.get(0);
            if let Some(value) = Self::json_from_str(&payload) {
                records.push(value);
            }
        }
        Ok(records)
    }

    fn load_recent_monitor_records(&self, limit: i64) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        if limit <= 0 {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT payload FROM monitor_sessions ORDER BY updated_time DESC LIMIT $1",
            &[&limit],
        )?;
        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            let payload: String = row.get(0);
            if let Some(value) = Self::json_from_str(&payload) {
                records.push(value);
            }
        }
        Ok(records)
    }

    fn load_monitor_records_by_user(
        &self,
        user_id: &str,
        statuses: Option<&[&str]>,
        since_time: Option<f64>,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() || limit <= 0 {
            return Ok(Vec::new());
        }
        let statuses = statuses
            .unwrap_or(&[])
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string())
            .collect::<Vec<_>>();
        let since_time = since_time.filter(|value| value.is_finite() && *value > 0.0);

        let mut conn = self.conn()?;
        let rows = match (!statuses.is_empty(), since_time.is_some()) {
            (true, true) => {
                let since = since_time.unwrap_or(0.0);
                conn.query(
                    "SELECT payload FROM monitor_sessions \
                     WHERE user_id = $1 AND status = ANY($2) AND updated_time >= $3 \
                     ORDER BY updated_time DESC LIMIT $4",
                    &[&cleaned_user, &statuses, &since, &limit],
                )?
            }
            (true, false) => conn.query(
                "SELECT payload FROM monitor_sessions \
                 WHERE user_id = $1 AND status = ANY($2) \
                 ORDER BY updated_time DESC LIMIT $3",
                &[&cleaned_user, &statuses, &limit],
            )?,
            (false, true) => {
                let since = since_time.unwrap_or(0.0);
                conn.query(
                    "SELECT payload FROM monitor_sessions \
                     WHERE user_id = $1 AND updated_time >= $2 \
                     ORDER BY updated_time DESC LIMIT $3",
                    &[&cleaned_user, &since, &limit],
                )?
            }
            (false, false) => conn.query(
                "SELECT payload FROM monitor_sessions WHERE user_id = $1 \
                 ORDER BY updated_time DESC LIMIT $2",
                &[&cleaned_user, &limit],
            )?,
        };
        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            let payload: String = row.get(0);
            if let Some(value) = Self::json_from_str(&payload) {
                records.push(value);
            }
        }
        Ok(records)
    }

    fn delete_monitor_record(&self, _session_id: &str) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = _session_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "DELETE FROM monitor_sessions WHERE session_id = $1",
            &[&cleaned],
        )?;
        Ok(())
    }

    fn delete_monitor_records_by_user(&self, _user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = _user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM monitor_sessions WHERE user_id = $1",
            &[&cleaned],
        )?;
        Ok(affected as i64)
    }

    fn try_acquire_session_lock(
        &self,
        _session_id: &str,
        _user_id: &str,
        _agent_id: &str,
        _ttl_s: f64,
        _max_sessions: i64,
    ) -> Result<SessionLockStatus> {
        self.ensure_initialized()?;
        let cleaned_session = _session_id.trim();
        let cleaned_user = _user_id.trim();
        let cleaned_agent = _agent_id.trim();
        if cleaned_session.is_empty() || cleaned_user.is_empty() {
            return Ok(SessionLockStatus::SystemBusy);
        }
        let max_sessions = _max_sessions.max(1);
        let ttl_s = _ttl_s.max(1.0);
        let now = Self::now_ts();
        let expires_at = now + ttl_s;

        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        tx.execute("DELETE FROM session_locks WHERE expires_at <= $1", &[&now])?;
        let inserted = tx.execute(
            "INSERT INTO session_locks (session_id, user_id, agent_id, created_time, updated_time, expires_at) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT DO NOTHING",
            &[
                &cleaned_session,
                &cleaned_user,
                &cleaned_agent,
                &now,
                &now,
                &expires_at,
            ],
        )?;
        if inserted == 0 {
            let session_lock = tx.query_opt(
                "SELECT session_id FROM session_locks WHERE session_id = $1 LIMIT 1",
                &[&cleaned_session],
            )?;
            tx.commit()?;
            return Ok(if session_lock.is_some() {
                SessionLockStatus::UserBusy
            } else {
                SessionLockStatus::SystemBusy
            });
        }
        let total: i64 = tx
            .query_one("SELECT COUNT(*) FROM session_locks", &[])?
            .get(0);
        if total > max_sessions {
            tx.execute(
                "DELETE FROM session_locks WHERE session_id = $1",
                &[&cleaned_session],
            )?;
            tx.commit()?;
            return Ok(SessionLockStatus::SystemBusy);
        }
        tx.commit()?;
        Ok(SessionLockStatus::Acquired)
    }

    fn touch_session_lock(&self, _session_id: &str, _ttl_s: f64) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_session = _session_id.trim();
        if cleaned_session.is_empty() {
            return Ok(());
        }
        let ttl_s = _ttl_s.max(1.0);
        let now = Self::now_ts();
        let expires_at = now + ttl_s;
        let mut conn = self.conn()?;
        conn.execute(
            "UPDATE session_locks SET updated_time = $1, expires_at = $2 WHERE session_id = $3",
            &[&now, &expires_at, &cleaned_session],
        )?;
        Ok(())
    }

    fn release_session_lock(&self, _session_id: &str) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_session = _session_id.trim();
        if cleaned_session.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "DELETE FROM session_locks WHERE session_id = $1",
            &[&cleaned_session],
        )?;
        Ok(())
    }

    fn delete_session_locks_by_user(&self, _user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = _user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute("DELETE FROM session_locks WHERE user_id = $1", &[&cleaned])?;
        Ok(affected as i64)
    }

    fn list_session_locks_by_user(&self, user_id: &str) -> Result<Vec<SessionLockRecord>> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let now = Self::now_ts();
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT session_id, user_id, agent_id, updated_time, expires_at \
             FROM session_locks WHERE user_id = $1 AND expires_at > $2",
            &[&cleaned, &now],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(SessionLockRecord {
                session_id: row.get(0),
                user_id: row.get(1),
                agent_id: row.get(2),
                updated_time: row.get(3),
                expires_at: row.get(4),
            });
        }
        Ok(output)
    }

    fn upsert_agent_thread(&self, record: &AgentThreadRecord) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = record.user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO agent_threads (thread_id, user_id, agent_id, session_id, status, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             ON CONFLICT (user_id, agent_id) DO UPDATE SET thread_id = EXCLUDED.thread_id, session_id = EXCLUDED.session_id, \
             status = EXCLUDED.status, updated_at = EXCLUDED.updated_at",
            &[
                &record.thread_id,
                &cleaned_user,
                &record.agent_id.trim(),
                &record.session_id,
                &record.status,
                &record.created_at,
                &record.updated_at,
            ],
        )?;
        Ok(())
    }

    fn get_agent_thread(&self, user_id: &str, agent_id: &str) -> Result<Option<AgentThreadRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(None);
        }
        let cleaned_agent = agent_id.trim();
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT thread_id, user_id, agent_id, session_id, status, created_at, updated_at \
             FROM agent_threads WHERE user_id = $1 AND agent_id = $2 LIMIT 1",
            &[&cleaned_user, &cleaned_agent],
        )?;
        Ok(row.map(|row| AgentThreadRecord {
            thread_id: row.get(0),
            user_id: row.get(1),
            agent_id: row.get(2),
            session_id: row.get(3),
            status: row.get(4),
            created_at: row.get(5),
            updated_at: row.get(6),
        }))
    }

    fn delete_agent_thread(&self, user_id: &str, agent_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(0);
        }
        let cleaned_agent = agent_id.trim();
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM agent_threads WHERE user_id = $1 AND agent_id = $2",
            &[&cleaned_user, &cleaned_agent],
        )?;
        Ok(affected as i64)
    }

    fn insert_agent_task(&self, record: &AgentTaskRecord) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = record.user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(());
        }
        let payload =
            serde_json::to_string(&record.request_payload).unwrap_or_else(|_| "{}".to_string());
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO agent_tasks (task_id, thread_id, user_id, agent_id, session_id, status, request_payload, request_id, retry_count, retry_at, created_at, updated_at, started_at, finished_at, last_error) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15) \
             ON CONFLICT (task_id) DO UPDATE SET status = EXCLUDED.status, request_payload = EXCLUDED.request_payload, \
             request_id = EXCLUDED.request_id, retry_count = EXCLUDED.retry_count, retry_at = EXCLUDED.retry_at, updated_at = EXCLUDED.updated_at, \
             started_at = EXCLUDED.started_at, finished_at = EXCLUDED.finished_at, last_error = EXCLUDED.last_error",
            &[
                &record.task_id,
                &record.thread_id,
                &cleaned_user,
                &record.agent_id.trim(),
                &record.session_id,
                &record.status,
                &payload,
                &record.request_id,
                &record.retry_count,
                &record.retry_at,
                &record.created_at,
                &record.updated_at,
                &record.started_at,
                &record.finished_at,
                &record.last_error,
            ],
        )?;
        Ok(())
    }

    fn get_agent_task(&self, task_id: &str) -> Result<Option<AgentTaskRecord>> {
        self.ensure_initialized()?;
        let cleaned = task_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT task_id, thread_id, user_id, agent_id, session_id, status, request_payload, request_id, retry_count, retry_at, created_at, updated_at, started_at, finished_at, last_error \
             FROM agent_tasks WHERE task_id = $1 LIMIT 1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| {
            let raw_payload: String = row.get(6);
            let payload = serde_json::from_str(&raw_payload).unwrap_or(Value::Null);
            AgentTaskRecord {
                task_id: row.get(0),
                thread_id: row.get(1),
                user_id: row.get(2),
                agent_id: row.get(3),
                session_id: row.get(4),
                status: row.get(5),
                request_payload: payload,
                request_id: row.get(7),
                retry_count: row.get(8),
                retry_at: row.get(9),
                created_at: row.get(10),
                updated_at: row.get(11),
                started_at: row.get(12),
                finished_at: row.get(13),
                last_error: row.get(14),
            }
        }))
    }

    fn list_pending_agent_tasks(&self, limit: i64) -> Result<Vec<AgentTaskRecord>> {
        self.ensure_initialized()?;
        let now = Self::now_ts();
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT task_id, thread_id, user_id, agent_id, session_id, status, request_payload, request_id, retry_count, retry_at, created_at, updated_at, started_at, finished_at, last_error \
             FROM agent_tasks WHERE (status = 'pending' OR status = 'retry') AND retry_at <= $1 \
             ORDER BY retry_at ASC, created_at ASC LIMIT $2",
            &[&now, &limit.max(1)],
        )?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let raw_payload: String = row.get(6);
                let payload = serde_json::from_str(&raw_payload).unwrap_or(Value::Null);
                AgentTaskRecord {
                    task_id: row.get(0),
                    thread_id: row.get(1),
                    user_id: row.get(2),
                    agent_id: row.get(3),
                    session_id: row.get(4),
                    status: row.get(5),
                    request_payload: payload,
                    request_id: row.get(7),
                    retry_count: row.get(8),
                    retry_at: row.get(9),
                    created_at: row.get(10),
                    updated_at: row.get(11),
                    started_at: row.get(12),
                    finished_at: row.get(13),
                    last_error: row.get(14),
                }
            })
            .collect())
    }

    fn list_agent_tasks_by_thread(
        &self,
        thread_id: &str,
        status: Option<&str>,
        limit: i64,
    ) -> Result<Vec<AgentTaskRecord>> {
        self.ensure_initialized()?;
        let cleaned_thread = thread_id.trim();
        if cleaned_thread.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let rows = if let Some(status) = status.filter(|value| !value.trim().is_empty()) {
            conn.query(
                "SELECT task_id, thread_id, user_id, agent_id, session_id, status, request_payload, request_id, retry_count, retry_at, created_at, updated_at, started_at, finished_at, last_error \
                 FROM agent_tasks WHERE thread_id = $1 AND status = $2 ORDER BY created_at DESC LIMIT $3",
                &[&cleaned_thread, &status.trim(), &limit.max(1)],
            )?
        } else {
            conn.query(
                "SELECT task_id, thread_id, user_id, agent_id, session_id, status, request_payload, request_id, retry_count, retry_at, created_at, updated_at, started_at, finished_at, last_error \
                 FROM agent_tasks WHERE thread_id = $1 ORDER BY created_at DESC LIMIT $2",
                &[&cleaned_thread, &limit.max(1)],
            )?
        };
        Ok(rows
            .into_iter()
            .map(|row| {
                let raw_payload: String = row.get(6);
                let payload = serde_json::from_str(&raw_payload).unwrap_or(Value::Null);
                AgentTaskRecord {
                    task_id: row.get(0),
                    thread_id: row.get(1),
                    user_id: row.get(2),
                    agent_id: row.get(3),
                    session_id: row.get(4),
                    status: row.get(5),
                    request_payload: payload,
                    request_id: row.get(7),
                    retry_count: row.get(8),
                    retry_at: row.get(9),
                    created_at: row.get(10),
                    updated_at: row.get(11),
                    started_at: row.get(12),
                    finished_at: row.get(13),
                    last_error: row.get(14),
                }
            })
            .collect())
    }

    fn update_agent_task_status(&self, params: UpdateAgentTaskStatusParams<'_>) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = params.task_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "UPDATE agent_tasks SET status = $1, retry_count = $2, retry_at = $3, started_at = $4, finished_at = $5, last_error = $6, updated_at = $7 WHERE task_id = $8",
            &[
                &params.status,
                &params.retry_count,
                &params.retry_at,
                &params.started_at,
                &params.finished_at,
                &params.last_error,
                &params.updated_at,
                &cleaned,
            ],
        )?;
        Ok(())
    }

    fn get_max_stream_event_id(&self, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let row = conn.query_one(
            "SELECT MAX(event_id) FROM stream_events WHERE session_id = $1",
            &[&cleaned_session],
        )?;
        let value: Option<i64> = row.get(0);
        Ok(value.unwrap_or(0))
    }

    fn append_stream_event(
        &self,
        _session_id: &str,
        _user_id: &str,
        _event_id: i64,
        _payload: &Value,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_session = _session_id.trim();
        let cleaned_user = _user_id.trim();
        if cleaned_session.is_empty() || cleaned_user.is_empty() {
            return Ok(());
        }
        let now = Self::now_ts();
        let payload_text = Self::json_to_string(_payload);
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO stream_events (session_id, event_id, user_id, payload, created_time) VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (session_id, event_id) DO UPDATE SET user_id = EXCLUDED.user_id, payload = EXCLUDED.payload, created_time = EXCLUDED.created_time",
            &[&cleaned_session, &_event_id, &cleaned_user, &payload_text, &now],
        )?;
        Ok(())
    }

    fn load_stream_events(
        &self,
        _session_id: &str,
        _after_event_id: i64,
        _limit: i64,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let cleaned_session = _session_id.trim();
        if cleaned_session.is_empty() || _limit <= 0 {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT event_id, payload FROM stream_events WHERE session_id = $1 AND event_id > $2 ORDER BY event_id ASC LIMIT $3",
            &[&cleaned_session, &_after_event_id, &_limit],
        )?;
        let mut records = Vec::new();
        for row in rows {
            let event_id: i64 = row.get(0);
            let payload: String = row.get(1);
            if let Some(mut value) = Self::json_from_str(&payload) {
                if let Value::Object(ref mut map) = value {
                    map.insert("event_id".to_string(), json!(event_id));
                    records.push(value);
                } else {
                    records.push(json!({ "event_id": event_id, "data": value }));
                }
            }
        }
        Ok(records)
    }

    fn delete_stream_events_before(&self, _before_time: f64) -> Result<i64> {
        self.ensure_initialized()?;
        if _before_time <= 0.0 {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM stream_events WHERE created_time < $1",
            &[&_before_time],
        )?;
        Ok(affected as i64)
    }

    fn delete_stream_events_by_user(&self, _user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = _user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute("DELETE FROM stream_events WHERE user_id = $1", &[&cleaned])?;
        Ok(affected as i64)
    }

    fn delete_stream_events_by_session(&self, _session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = _session_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM stream_events WHERE session_id = $1",
            &[&cleaned],
        )?;
        Ok(affected as i64)
    }

    fn get_memory_enabled(&self, _user_id: &str) -> Result<Option<bool>> {
        self.ensure_initialized()?;
        let cleaned = _user_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT enabled FROM memory_settings WHERE user_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| row.get::<_, i32>(0) != 0))
    }

    fn set_memory_enabled(&self, _user_id: &str, _enabled: bool) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = _user_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let now = Self::now_ts();
        let enabled_value: i32 = if _enabled { 1 } else { 0 };
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO memory_settings (user_id, enabled, updated_time) VALUES ($1, $2, $3) \
             ON CONFLICT(user_id) DO UPDATE SET enabled = EXCLUDED.enabled, updated_time = EXCLUDED.updated_time",
            &[&cleaned, &enabled_value, &now],
        )?;
        Ok(())
    }

    fn load_memory_settings(&self) -> Result<Vec<HashMap<String, Value>>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT user_id, enabled, updated_time FROM memory_settings",
            &[],
        )?;
        let mut output = Vec::new();
        for row in rows {
            let user_id: String = row.get(0);
            let cleaned = user_id.trim();
            if cleaned.is_empty() {
                continue;
            }
            let enabled: i32 = row.get(1);
            let updated_time: f64 = row.try_get(2).unwrap_or(0.0);
            let mut entry = HashMap::new();
            entry.insert("user_id".to_string(), json!(cleaned));
            entry.insert("enabled".to_string(), json!(enabled != 0));
            entry.insert("updated_time".to_string(), json!(updated_time));
            output.push(entry);
        }
        Ok(output)
    }

    fn upsert_memory_record(
        &self,
        _user_id: &str,
        _session_id: &str,
        _summary: &str,
        _max_records: i64,
        _now_ts: f64,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = _user_id.trim();
        let cleaned_session = _session_id.trim();
        let cleaned_summary = _summary.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() || cleaned_summary.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO memory_records (user_id, session_id, summary, created_time, updated_time) VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT(user_id, session_id) DO UPDATE SET summary = EXCLUDED.summary, updated_time = EXCLUDED.updated_time",
            &[&cleaned_user, &cleaned_session, &cleaned_summary, &_now_ts, &_now_ts],
        )?;
        if _max_records > 0 {
            let safe_limit = _max_records.max(1);
            conn.execute(
                "DELETE FROM memory_records WHERE user_id = $1 AND id NOT IN (\
                    SELECT id FROM memory_records WHERE user_id = $1 ORDER BY updated_time DESC, id DESC LIMIT $2\
                 )",
                &[&cleaned_user, &safe_limit],
            )?;
        }
        conn.execute(
            "DELETE FROM memory_task_logs WHERE user_id = $1 AND session_id NOT IN (\
                SELECT session_id FROM memory_records WHERE user_id = $1\
             )",
            &[&cleaned_user],
        )?;
        Ok(())
    }

    fn load_memory_records(
        &self,
        _user_id: &str,
        _limit: i64,
        _order_desc: bool,
    ) -> Result<Vec<HashMap<String, Value>>> {
        self.ensure_initialized()?;
        let cleaned = _user_id.trim();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let direction = if _order_desc { "DESC" } else { "ASC" };
        let query = if _limit > 0 {
            format!(
                "SELECT session_id, summary, created_time, updated_time FROM memory_records WHERE user_id = $1 ORDER BY updated_time {direction}, id {direction} LIMIT $2"
            )
        } else {
            format!(
                "SELECT session_id, summary, created_time, updated_time FROM memory_records WHERE user_id = $1 ORDER BY updated_time {direction}, id {direction}"
            )
        };
        let mut conn = self.conn()?;
        let rows = if _limit > 0 {
            conn.query(&query, &[&cleaned, &_limit])?
        } else {
            conn.query(&query, &[&cleaned])?
        };
        let mut records = Vec::new();
        for row in rows {
            let session_id: String = row.get(0);
            let summary: String = row.get(1);
            let created_time: f64 = row.try_get(2).unwrap_or(0.0);
            let updated_time: f64 = row.try_get(3).unwrap_or(0.0);
            let mut entry = HashMap::new();
            entry.insert("session_id".to_string(), json!(session_id));
            entry.insert("summary".to_string(), json!(summary));
            entry.insert("created_time".to_string(), json!(created_time));
            entry.insert("updated_time".to_string(), json!(updated_time));
            records.push(entry);
        }
        Ok(records)
    }

    fn get_memory_record_stats(&self) -> Result<Vec<HashMap<String, Value>>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT user_id, COUNT(*) as record_count, MAX(updated_time) as last_time FROM memory_records GROUP BY user_id",
            &[],
        )?;
        let mut stats = Vec::new();
        for row in rows {
            let user_id: String = row.get(0);
            let cleaned = user_id.trim();
            if cleaned.is_empty() {
                continue;
            }
            let record_count: i64 = row.get(1);
            let last_time: f64 = row.try_get(2).unwrap_or(0.0);
            let mut entry = HashMap::new();
            entry.insert("user_id".to_string(), json!(cleaned));
            entry.insert("record_count".to_string(), json!(record_count));
            entry.insert("last_time".to_string(), json!(last_time));
            stats.push(entry);
        }
        Ok(stats)
    }

    fn delete_memory_record(&self, _user_id: &str, _session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = _user_id.trim();
        let cleaned_session = _session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM memory_records WHERE user_id = $1 AND session_id = $2",
            &[&cleaned_user, &cleaned_session],
        )?;
        Ok(affected as i64)
    }

    fn delete_memory_records_by_user(&self, _user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = _user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected =
            conn.execute("DELETE FROM memory_records WHERE user_id = $1", &[&cleaned])?;
        Ok(affected as i64)
    }

    fn delete_memory_settings_by_user(&self, _user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = _user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM memory_settings WHERE user_id = $1",
            &[&cleaned],
        )?;
        Ok(affected as i64)
    }

    fn upsert_memory_task_log(&self, params: UpsertMemoryTaskLogParams<'_>) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = params.user_id.trim();
        let cleaned_session = params.session_id.trim();
        let cleaned_task = params.task_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() || cleaned_task.is_empty() {
            return Ok(());
        }
        let status_text = params.status.trim();
        let payload_text = params
            .request_payload
            .map(Self::json_to_string)
            .unwrap_or_default();
        let now = params.updated_time.unwrap_or_else(Self::now_ts);
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO memory_task_logs (task_id, user_id, session_id, status, queued_time, started_time, finished_time, elapsed_s, request_payload, result, error, updated_time)              VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)              ON CONFLICT(user_id, session_id) DO UPDATE SET                task_id = EXCLUDED.task_id, status = EXCLUDED.status, queued_time = EXCLUDED.queued_time, started_time = EXCLUDED.started_time,                finished_time = EXCLUDED.finished_time, elapsed_s = EXCLUDED.elapsed_s, request_payload = EXCLUDED.request_payload, result = EXCLUDED.result,                error = EXCLUDED.error, updated_time = EXCLUDED.updated_time",
            &[
                &cleaned_task,
                &cleaned_user,
                &cleaned_session,
                &status_text,
                &params.queued_time,
                &params.started_time,
                &params.finished_time,
                &params.elapsed_s,
                &payload_text,
                &params.result,
                &params.error,
                &now,
            ],
        )?;
        Ok(())
    }

    fn load_memory_task_logs(&self, _limit: Option<i64>) -> Result<Vec<HashMap<String, Value>>> {
        self.ensure_initialized()?;
        let mut query = String::from(
            "SELECT task_id, user_id, session_id, status, queued_time, started_time, finished_time, elapsed_s, updated_time FROM memory_task_logs ORDER BY updated_time DESC, id DESC",
        );
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        if let Some(limit) = _limit.filter(|value| *value > 0) {
            query.push_str(" LIMIT $1");
            params.push(Box::new(limit));
        }
        let mut conn = self.conn()?;
        let params_ref: Vec<&(dyn ToSql + Sync)> =
            params.iter().map(|value| value.as_ref()).collect();
        let rows = conn.query(&query, &params_ref)?;
        let mut logs = Vec::new();
        for row in rows {
            let task_id: String = row.get(0);
            let user_id: String = row.get(1);
            let session_id: String = row.get(2);
            let status: String = row.get(3);
            let queued_time: f64 = row.try_get(4).unwrap_or(0.0);
            let started_time: f64 = row.try_get(5).unwrap_or(0.0);
            let finished_time: f64 = row.try_get(6).unwrap_or(0.0);
            let elapsed_s: f64 = row.try_get(7).unwrap_or(0.0);
            let updated_time: f64 = row.try_get(8).unwrap_or(0.0);
            let mut entry = HashMap::new();
            entry.insert("task_id".to_string(), json!(task_id));
            entry.insert("user_id".to_string(), json!(user_id));
            entry.insert("session_id".to_string(), json!(session_id));
            entry.insert("status".to_string(), json!(status));
            entry.insert("queued_time".to_string(), json!(queued_time));
            entry.insert("started_time".to_string(), json!(started_time));
            entry.insert("finished_time".to_string(), json!(finished_time));
            entry.insert("elapsed_s".to_string(), json!(elapsed_s));
            entry.insert("updated_time".to_string(), json!(updated_time));
            logs.push(entry);
        }
        Ok(logs)
    }

    fn load_memory_task_log_by_task_id(
        &self,
        _task_id: &str,
    ) -> Result<Option<HashMap<String, Value>>> {
        self.ensure_initialized()?;
        let cleaned = _task_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT task_id, user_id, session_id, status, queued_time, started_time, finished_time, elapsed_s, request_payload, result, error, updated_time FROM memory_task_logs WHERE task_id = $1 ORDER BY updated_time DESC, id DESC LIMIT 1",
            &[&cleaned],
        )?;
        let Some(row) = row else {
            return Ok(None);
        };
        let task_id: String = row.get(0);
        let user_id: String = row.get(1);
        let session_id: String = row.get(2);
        let status: String = row.get(3);
        let queued_time: f64 = row.try_get(4).unwrap_or(0.0);
        let started_time: f64 = row.try_get(5).unwrap_or(0.0);
        let finished_time: f64 = row.try_get(6).unwrap_or(0.0);
        let elapsed_s: f64 = row.try_get(7).unwrap_or(0.0);
        let request_payload: String = row.get::<_, Option<String>>(8).unwrap_or_default();
        let result: String = row.get::<_, Option<String>>(9).unwrap_or_default();
        let error: String = row.get::<_, Option<String>>(10).unwrap_or_default();
        let updated_time: f64 = row.try_get(11).unwrap_or(0.0);
        let mut entry = HashMap::new();
        entry.insert("task_id".to_string(), json!(task_id));
        entry.insert("user_id".to_string(), json!(user_id));
        entry.insert("session_id".to_string(), json!(session_id));
        entry.insert("status".to_string(), json!(status));
        entry.insert("queued_time".to_string(), json!(queued_time));
        entry.insert("started_time".to_string(), json!(started_time));
        entry.insert("finished_time".to_string(), json!(finished_time));
        entry.insert("elapsed_s".to_string(), json!(elapsed_s));
        entry.insert("request_payload".to_string(), json!(request_payload));
        entry.insert("result".to_string(), json!(result));
        entry.insert("error".to_string(), json!(error));
        entry.insert("updated_time".to_string(), json!(updated_time));
        Ok(Some(entry))
    }

    fn delete_memory_task_log(&self, _user_id: &str, _session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = _user_id.trim();
        let cleaned_session = _session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM memory_task_logs WHERE user_id = $1 AND session_id = $2",
            &[&cleaned_user, &cleaned_session],
        )?;
        Ok(affected as i64)
    }

    fn delete_memory_task_logs_by_user(&self, _user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = _user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM memory_task_logs WHERE user_id = $1",
            &[&cleaned],
        )?;
        Ok(affected as i64)
    }

    fn create_evaluation_run(&self, payload: &Value) -> Result<()> {
        self.ensure_initialized()?;
        let run_id = payload
            .get("run_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if run_id.is_empty() {
            return Ok(());
        }
        let user_id = payload
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let model_name = payload
            .get("model_name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let language = payload
            .get("language")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let status = payload
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let total_score = Self::parse_f64(payload.get("total_score")).unwrap_or(0.0);
        let started_time = Self::parse_f64(payload.get("started_time")).unwrap_or(0.0);
        let finished_time = Self::parse_f64(payload.get("finished_time")).unwrap_or(0.0);
        let payload_text = Self::json_to_string(payload);
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO evaluation_runs (run_id, user_id, model_name, language, status, total_score, started_time, finished_time, payload) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
             ON CONFLICT(run_id) DO UPDATE SET user_id = EXCLUDED.user_id, model_name = EXCLUDED.model_name, \
             language = EXCLUDED.language, status = EXCLUDED.status, total_score = EXCLUDED.total_score, \
             started_time = EXCLUDED.started_time, finished_time = EXCLUDED.finished_time, payload = EXCLUDED.payload",
            &[
                &run_id,
                &user_id,
                &model_name,
                &language,
                &status,
                &total_score,
                &started_time,
                &finished_time,
                &payload_text,
            ],
        )?;
        Ok(())
    }

    fn update_evaluation_run(&self, run_id: &str, payload: &Value) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = run_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let mut merged = payload.clone();
        if let Value::Object(ref mut map) = merged {
            map.insert("run_id".to_string(), Value::String(cleaned.to_string()));
        }
        self.create_evaluation_run(&merged)
    }

    fn upsert_evaluation_item(&self, run_id: &str, payload: &Value) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = run_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let case_id = payload
            .get("case_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if case_id.is_empty() {
            return Ok(());
        }
        let dimension = payload
            .get("dimension")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let status = payload
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let score = Self::parse_f64(payload.get("score")).unwrap_or(0.0);
        let max_score = Self::parse_f64(payload.get("max_score")).unwrap_or(0.0);
        let weight = Self::parse_f64(payload.get("weight")).unwrap_or(0.0);
        let started_time = Self::parse_f64(payload.get("started_time")).unwrap_or(0.0);
        let finished_time = Self::parse_f64(payload.get("finished_time")).unwrap_or(0.0);
        let payload_text = Self::json_to_string(payload);
        let mut conn = self.conn()?;
        let updated = conn.execute(
            "UPDATE evaluation_items SET dimension = $1, status = $2, score = $3, max_score = $4, weight = $5, \
             started_time = $6, finished_time = $7, payload = $8 WHERE run_id = $9 AND case_id = $10",
            &[
                &dimension,
                &status,
                &score,
                &max_score,
                &weight,
                &started_time,
                &finished_time,
                &payload_text,
                &cleaned,
                &case_id,
            ],
        )?;
        if updated == 0 {
            conn.execute(
                "INSERT INTO evaluation_items (run_id, case_id, dimension, status, score, max_score, weight, started_time, finished_time, payload) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
                &[
                    &cleaned,
                    &case_id,
                    &dimension,
                    &status,
                    &score,
                    &max_score,
                    &weight,
                    &started_time,
                    &finished_time,
                    &payload_text,
                ],
            )?;
        }
        Ok(())
    }

    fn load_evaluation_runs(
        &self,
        user_id: Option<&str>,
        status: Option<&str>,
        model_name: Option<&str>,
        since_time: Option<f64>,
        until_time: Option<f64>,
        limit: Option<i64>,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let mut conditions = Vec::new();
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        if let Some(user_id) = user_id {
            let cleaned = user_id.trim();
            if !cleaned.is_empty() {
                conditions.push(format!("user_id = ${}", params.len() + 1));
                params.push(Box::new(cleaned.to_string()));
            }
        }
        if let Some(status) = status {
            let cleaned = status.trim();
            if !cleaned.is_empty() {
                conditions.push(format!("status = ${}", params.len() + 1));
                params.push(Box::new(cleaned.to_string()));
            }
        }
        if let Some(model_name) = model_name {
            let cleaned = model_name.trim();
            if !cleaned.is_empty() {
                conditions.push(format!("model_name = ${}", params.len() + 1));
                params.push(Box::new(cleaned.to_string()));
            }
        }
        if let Some(since) = since_time {
            conditions.push(format!("started_time >= ${}", params.len() + 1));
            params.push(Box::new(since));
        }
        if let Some(until) = until_time {
            conditions.push(format!("started_time <= ${}", params.len() + 1));
            params.push(Box::new(until));
        }
        let mut query = String::from("SELECT payload FROM evaluation_runs");
        if !conditions.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&conditions.join(" AND "));
        }
        query.push_str(" ORDER BY started_time DESC");
        if let Some(limit) = limit {
            if limit > 0 {
                query.push_str(&format!(" LIMIT ${}", params.len() + 1));
                params.push(Box::new(limit));
            }
        }
        let mut conn = self.conn()?;
        let params_ref: Vec<&(dyn ToSql + Sync)> =
            params.iter().map(|value| value.as_ref()).collect();
        let rows = conn.query(&query, &params_ref)?;
        let mut records = Vec::new();
        for row in rows {
            let payload: String = row.get(0);
            if let Some(value) = Self::json_from_str(&payload) {
                records.push(value);
            }
        }
        Ok(records)
    }

    fn load_evaluation_run(&self, run_id: &str) -> Result<Option<Value>> {
        self.ensure_initialized()?;
        let cleaned = run_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT payload FROM evaluation_runs WHERE run_id = $1",
            &[&cleaned],
        )?;
        Ok(row.and_then(|row| Self::json_from_str(&row.get::<_, String>(0))))
    }

    fn load_evaluation_items(&self, run_id: &str) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let cleaned = run_id.trim();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT payload FROM evaluation_items WHERE run_id = $1 ORDER BY id",
            &[&cleaned],
        )?;
        let mut records = Vec::new();
        for row in rows {
            let payload: String = row.get(0);
            if let Some(value) = Self::json_from_str(&payload) {
                records.push(value);
            }
        }
        Ok(records)
    }

    fn delete_evaluation_run(&self, run_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = run_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        let items_deleted = tx.execute(
            "DELETE FROM evaluation_items WHERE run_id = $1",
            &[&cleaned],
        )?;
        let runs_deleted =
            tx.execute("DELETE FROM evaluation_runs WHERE run_id = $1", &[&cleaned])?;
        tx.commit()?;
        Ok((items_deleted + runs_deleted) as i64)
    }

    fn cleanup_retention(&self, _retention_days: i64) -> Result<HashMap<String, i64>> {
        self.ensure_initialized()?;
        if _retention_days <= 0 {
            return Ok(HashMap::new());
        }
        let cutoff = Self::now_ts() - (_retention_days as f64 * 86400.0);
        if cutoff <= 0.0 {
            return Ok(HashMap::new());
        }
        let mut conn = self.conn()?;
        let rows = conn.query("SELECT user_id, roles FROM user_accounts", &[])?;
        let mut admin_ids = Vec::new();
        for row in rows {
            let user_id: String = row.get(0);
            let roles_raw: Option<String> = row.get(1);
            let roles = Self::parse_string_list(roles_raw);
            if roles
                .iter()
                .any(|role| role == "admin" || role == "super_admin")
            {
                admin_ids.push(user_id);
            }
        }
        let mut results = HashMap::new();
        let mut delete_with_filter = |base_sql: &str, allow_null_user: bool| -> Result<i64> {
            if admin_ids.is_empty() {
                return Ok(conn.execute(base_sql, &[&cutoff])? as i64);
            }
            let sql = if allow_null_user {
                format!("{base_sql} AND (user_id IS NULL OR user_id <> ALL($2))")
            } else {
                format!("{base_sql} AND user_id <> ALL($2)")
            };
            Ok(conn.execute(&sql, &[&cutoff, &admin_ids])? as i64)
        };
        let chat = delete_with_filter("DELETE FROM chat_history WHERE created_time < $1", false)?;
        results.insert("chat_history".to_string(), chat);
        let tool = delete_with_filter("DELETE FROM tool_logs WHERE created_time < $1", false)?;
        results.insert("tool_logs".to_string(), tool);
        let artifact =
            delete_with_filter("DELETE FROM artifact_logs WHERE created_time < $1", false)?;
        results.insert("artifact_logs".to_string(), artifact);
        let monitor =
            delete_with_filter("DELETE FROM monitor_sessions WHERE updated_time < $1", true)?;
        results.insert("monitor_sessions".to_string(), monitor);
        let stream =
            delete_with_filter("DELETE FROM stream_events WHERE created_time < $1", false)?;
        results.insert("stream_events".to_string(), stream);
        let session_runs =
            delete_with_filter("DELETE FROM session_runs WHERE updated_time < $1", false)?;
        results.insert("session_runs".to_string(), session_runs);
        Ok(results)
    }

    fn upsert_user_account(&self, record: &UserAccountRecord) -> Result<()> {
        self.ensure_initialized()?;
        let roles = Self::string_list_to_json(&record.roles);
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO user_accounts (user_id, username, email, password_hash, roles, status, access_level, unit_id, \
             daily_quota, daily_quota_used, daily_quota_date, is_demo, created_at, updated_at, last_login_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15) \
             ON CONFLICT(user_id) DO UPDATE SET username = EXCLUDED.username, email = EXCLUDED.email, password_hash = EXCLUDED.password_hash, \
             roles = EXCLUDED.roles, status = EXCLUDED.status, access_level = EXCLUDED.access_level, unit_id = EXCLUDED.unit_id, \
             daily_quota = EXCLUDED.daily_quota, daily_quota_used = EXCLUDED.daily_quota_used, daily_quota_date = EXCLUDED.daily_quota_date, \
             is_demo = EXCLUDED.is_demo, created_at = EXCLUDED.created_at, updated_at = EXCLUDED.updated_at, last_login_at = EXCLUDED.last_login_at",
            &[
                &record.user_id,
                &record.username,
                &record.email,
                &record.password_hash,
                &roles,
                &record.status,
                &record.access_level,
                &record.unit_id,
                &record.daily_quota,
                &record.daily_quota_used,
                &record.daily_quota_date,
                &(record.is_demo as i32),
                &record.created_at,
                &record.updated_at,
                &record.last_login_at,
            ],
        )?;
        Ok(())
    }

    fn upsert_user_accounts(&self, records: &[UserAccountRecord]) -> Result<()> {
        self.ensure_initialized()?;
        if records.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        for record in records {
            let roles = Self::string_list_to_json(&record.roles);
            tx.execute(
                "INSERT INTO user_accounts (user_id, username, email, password_hash, roles, status, access_level, unit_id, \
                 daily_quota, daily_quota_used, daily_quota_date, is_demo, created_at, updated_at, last_login_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15) \
                 ON CONFLICT(user_id) DO UPDATE SET username = EXCLUDED.username, email = EXCLUDED.email, password_hash = EXCLUDED.password_hash, \
                 roles = EXCLUDED.roles, status = EXCLUDED.status, access_level = EXCLUDED.access_level, unit_id = EXCLUDED.unit_id, \
                 daily_quota = EXCLUDED.daily_quota, daily_quota_used = EXCLUDED.daily_quota_used, daily_quota_date = EXCLUDED.daily_quota_date, \
                 is_demo = EXCLUDED.is_demo, created_at = EXCLUDED.created_at, updated_at = EXCLUDED.updated_at, last_login_at = EXCLUDED.last_login_at",
                &[
                    &record.user_id,
                    &record.username,
                    &record.email,
                    &record.password_hash,
                    &roles,
                    &record.status,
                    &record.access_level,
                    &record.unit_id,
                    &record.daily_quota,
                    &record.daily_quota_used,
                    &record.daily_quota_date,
                    &(record.is_demo as i32),
                    &record.created_at,
                    &record.updated_at,
                    &record.last_login_at,
                ],
            )?;
        }
        tx.commit()
    }

    fn get_user_account(&self, user_id: &str) -> Result<Option<UserAccountRecord>> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, daily_quota, daily_quota_used, daily_quota_date, \
             is_demo, created_at, updated_at, last_login_at FROM user_accounts WHERE user_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| UserAccountRecord {
            user_id: row.get(0),
            username: row.get(1),
            email: row.get(2),
            password_hash: row.get(3),
            roles: Self::parse_string_list(row.get::<_, Option<String>>(4)),
            status: row.get(5),
            access_level: row.get(6),
            unit_id: row.get(7),
            daily_quota: row.get::<_, Option<i64>>(8).unwrap_or(0),
            daily_quota_used: row.get::<_, Option<i64>>(9).unwrap_or(0),
            daily_quota_date: row.get(10),
            is_demo: row.get::<_, i32>(11) != 0,
            created_at: row.get(12),
            updated_at: row.get(13),
            last_login_at: row.get(14),
        }))
    }

    fn get_user_account_by_username(&self, username: &str) -> Result<Option<UserAccountRecord>> {
        self.ensure_initialized()?;
        let cleaned = username.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, daily_quota, daily_quota_used, daily_quota_date, \
             is_demo, created_at, updated_at, last_login_at FROM user_accounts WHERE username = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| UserAccountRecord {
            user_id: row.get(0),
            username: row.get(1),
            email: row.get(2),
            password_hash: row.get(3),
            roles: Self::parse_string_list(row.get::<_, Option<String>>(4)),
            status: row.get(5),
            access_level: row.get(6),
            unit_id: row.get(7),
            daily_quota: row.get::<_, Option<i64>>(8).unwrap_or(0),
            daily_quota_used: row.get::<_, Option<i64>>(9).unwrap_or(0),
            daily_quota_date: row.get(10),
            is_demo: row.get::<_, i32>(11) != 0,
            created_at: row.get(12),
            updated_at: row.get(13),
            last_login_at: row.get(14),
        }))
    }

    fn get_user_account_by_email(&self, email: &str) -> Result<Option<UserAccountRecord>> {
        self.ensure_initialized()?;
        let cleaned = email.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, daily_quota, daily_quota_used, daily_quota_date, \
             is_demo, created_at, updated_at, last_login_at FROM user_accounts WHERE email = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| UserAccountRecord {
            user_id: row.get(0),
            username: row.get(1),
            email: row.get(2),
            password_hash: row.get(3),
            roles: Self::parse_string_list(row.get::<_, Option<String>>(4)),
            status: row.get(5),
            access_level: row.get(6),
            unit_id: row.get(7),
            daily_quota: row.get::<_, Option<i64>>(8).unwrap_or(0),
            daily_quota_used: row.get::<_, Option<i64>>(9).unwrap_or(0),
            daily_quota_date: row.get(10),
            is_demo: row.get::<_, i32>(11) != 0,
            created_at: row.get(12),
            updated_at: row.get(13),
            last_login_at: row.get(14),
        }))
    }

    fn list_user_accounts(
        &self,
        keyword: Option<&str>,
        unit_ids: Option<&[String]>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserAccountRecord>, i64)> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let cleaned_keyword = keyword
            .map(|value| value.trim())
            .filter(|value| !value.is_empty());
        let unit_ids = unit_ids
            .filter(|ids| !ids.is_empty())
            .map(|ids| ids.to_vec());

        let total: i64 = match (&cleaned_keyword, unit_ids.as_ref()) {
            (Some(keyword), Some(unit_ids)) => {
                let pattern = format!("%{keyword}%");
                conn.query_one(
                    "SELECT COUNT(*) FROM user_accounts WHERE (username ILIKE $1 OR email ILIKE $1) AND unit_id = ANY($2)",
                    &[&pattern, unit_ids],
                )?
                .get(0)
            }
            (Some(keyword), None) => {
                let pattern = format!("%{keyword}%");
                conn.query_one(
                    "SELECT COUNT(*) FROM user_accounts WHERE username ILIKE $1 OR email ILIKE $1",
                    &[&pattern],
                )?
                .get(0)
            }
            (None, Some(unit_ids)) => conn
                .query_one(
                    "SELECT COUNT(*) FROM user_accounts WHERE unit_id = ANY($1)",
                    &[unit_ids],
                )?
                .get(0),
            (None, None) => conn
                .query_one("SELECT COUNT(*) FROM user_accounts", &[])?
                .get(0),
        };

        let rows = match (&cleaned_keyword, unit_ids.as_ref()) {
            (Some(keyword), Some(unit_ids)) => {
                let pattern = format!("%{keyword}%");
                if limit > 0 {
                    conn.query(
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, daily_quota, daily_quota_used, daily_quota_date, \
                         is_demo, created_at, updated_at, last_login_at FROM user_accounts \
                         WHERE (username ILIKE $1 OR email ILIKE $1) AND unit_id = ANY($2) \
                         ORDER BY created_at DESC LIMIT $3 OFFSET $4",
                        &[&pattern, unit_ids, &limit, &offset.max(0)],
                    )?
                } else {
                    conn.query(
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, daily_quota, daily_quota_used, daily_quota_date, \
                         is_demo, created_at, updated_at, last_login_at FROM user_accounts \
                         WHERE (username ILIKE $1 OR email ILIKE $1) AND unit_id = ANY($2) \
                         ORDER BY created_at DESC",
                        &[&pattern, unit_ids],
                    )?
                }
            }
            (Some(keyword), None) => {
                let pattern = format!("%{keyword}%");
                if limit > 0 {
                    conn.query(
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, daily_quota, daily_quota_used, daily_quota_date, \
                         is_demo, created_at, updated_at, last_login_at FROM user_accounts \
                         WHERE username ILIKE $1 OR email ILIKE $1 \
                         ORDER BY created_at DESC LIMIT $2 OFFSET $3",
                        &[&pattern, &limit, &offset.max(0)],
                    )?
                } else {
                    conn.query(
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, daily_quota, daily_quota_used, daily_quota_date, \
                         is_demo, created_at, updated_at, last_login_at FROM user_accounts \
                         WHERE username ILIKE $1 OR email ILIKE $1 \
                         ORDER BY created_at DESC",
                        &[&pattern],
                    )?
                }
            }
            (None, Some(unit_ids)) => {
                if limit > 0 {
                    conn.query(
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, daily_quota, daily_quota_used, daily_quota_date, \
                         is_demo, created_at, updated_at, last_login_at FROM user_accounts \
                         WHERE unit_id = ANY($1) \
                         ORDER BY created_at DESC LIMIT $2 OFFSET $3",
                        &[unit_ids, &limit, &offset.max(0)],
                    )?
                } else {
                    conn.query(
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, daily_quota, daily_quota_used, daily_quota_date, \
                         is_demo, created_at, updated_at, last_login_at FROM user_accounts \
                         WHERE unit_id = ANY($1) ORDER BY created_at DESC",
                        &[unit_ids],
                    )?
                }
            }
            (None, None) => {
                if limit > 0 {
                    conn.query(
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, daily_quota, daily_quota_used, daily_quota_date, \
                         is_demo, created_at, updated_at, last_login_at FROM user_accounts \
                         ORDER BY created_at DESC LIMIT $1 OFFSET $2",
                        &[&limit, &offset.max(0)],
                    )?
                } else {
                    conn.query(
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, daily_quota, daily_quota_used, daily_quota_date, \
                         is_demo, created_at, updated_at, last_login_at FROM user_accounts ORDER BY created_at DESC",
                        &[],
                    )?
                }
            }
        };

        let mut output = Vec::new();
        for row in rows {
            output.push(UserAccountRecord {
                user_id: row.get(0),
                username: row.get(1),
                email: row.get(2),
                password_hash: row.get(3),
                roles: Self::parse_string_list(row.get::<_, Option<String>>(4)),
                status: row.get(5),
                access_level: row.get(6),
                unit_id: row.get(7),
                daily_quota: row.get::<_, Option<i64>>(8).unwrap_or(0),
                daily_quota_used: row.get::<_, Option<i64>>(9).unwrap_or(0),
                daily_quota_date: row.get(10),
                is_demo: row.get::<_, i32>(11) != 0,
                created_at: row.get(12),
                updated_at: row.get(13),
                last_login_at: row.get(14),
            });
        }
        Ok((output, total))
    }

    fn delete_user_account(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute("DELETE FROM user_accounts WHERE user_id = $1", &[&cleaned])?;
        Ok(affected as i64)
    }

    fn list_org_units(&self) -> Result<Vec<OrgUnitRecord>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT unit_id, parent_id, name, level, path, path_name, sort_order, leader_ids, created_at, updated_at \
             FROM org_units ORDER BY path, sort_order, name",
            &[],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(OrgUnitRecord {
                unit_id: row.get(0),
                parent_id: row.get(1),
                name: row.get(2),
                level: row.get(3),
                path: row.get(4),
                path_name: row.get(5),
                sort_order: row.get(6),
                leader_ids: Self::parse_string_list(row.get::<_, Option<String>>(7)),
                created_at: row.get(8),
                updated_at: row.get(9),
            });
        }
        Ok(output)
    }

    fn get_org_unit(&self, unit_id: &str) -> Result<Option<OrgUnitRecord>> {
        self.ensure_initialized()?;
        let cleaned = unit_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT unit_id, parent_id, name, level, path, path_name, sort_order, leader_ids, created_at, updated_at \
             FROM org_units WHERE unit_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| OrgUnitRecord {
            unit_id: row.get(0),
            parent_id: row.get(1),
            name: row.get(2),
            level: row.get(3),
            path: row.get(4),
            path_name: row.get(5),
            sort_order: row.get(6),
            leader_ids: Self::parse_string_list(row.get::<_, Option<String>>(7)),
            created_at: row.get(8),
            updated_at: row.get(9),
        }))
    }

    fn upsert_org_unit(&self, record: &OrgUnitRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let leader_ids = Self::string_list_to_json(&record.leader_ids);
        conn.execute(
            "INSERT INTO org_units (unit_id, parent_id, name, level, path, path_name, sort_order, leader_ids, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
             ON CONFLICT(unit_id) DO UPDATE SET parent_id = EXCLUDED.parent_id, name = EXCLUDED.name, level = EXCLUDED.level, \
             path = EXCLUDED.path, path_name = EXCLUDED.path_name, sort_order = EXCLUDED.sort_order, leader_ids = EXCLUDED.leader_ids, \
             updated_at = EXCLUDED.updated_at",
            &[
                &record.unit_id,
                &record.parent_id,
                &record.name,
                &record.level,
                &record.path,
                &record.path_name,
                &record.sort_order,
                &leader_ids,
                &record.created_at,
                &record.updated_at,
            ],
        )?;
        Ok(())
    }

    fn delete_org_unit(&self, unit_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = unit_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute("DELETE FROM org_units WHERE unit_id = $1", &[&cleaned])?;
        Ok(affected as i64)
    }

    fn upsert_external_link(&self, record: &ExternalLinkRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let allowed_levels = Self::i32_list_to_json(&record.allowed_levels);
        conn.execute(
            "INSERT INTO external_links (link_id, title, description, url, icon, allowed_levels, sort_order, enabled, created_at, updated_at) \n             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \n             ON CONFLICT(link_id) DO UPDATE SET title = EXCLUDED.title, description = EXCLUDED.description, \n             url = EXCLUDED.url, icon = EXCLUDED.icon, allowed_levels = EXCLUDED.allowed_levels, \n             sort_order = EXCLUDED.sort_order, enabled = EXCLUDED.enabled, updated_at = EXCLUDED.updated_at",
            &[
                &record.link_id,
                &record.title,
                &record.description,
                &record.url,
                &record.icon,
                &allowed_levels,
                &record.sort_order,
                &(if record.enabled { 1_i32 } else { 0_i32 }),
                &record.created_at,
                &record.updated_at,
            ],
        )?;
        Ok(())
    }

    fn get_external_link(&self, link_id: &str) -> Result<Option<ExternalLinkRecord>> {
        self.ensure_initialized()?;
        let cleaned = link_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT link_id, title, description, url, icon, allowed_levels, sort_order, enabled, created_at, updated_at \n             FROM external_links WHERE link_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| ExternalLinkRecord {
            link_id: row.get(0),
            title: row.get(1),
            description: row.get(2),
            url: row.get(3),
            icon: row.get(4),
            allowed_levels: Self::parse_i32_list(row.get::<_, Option<String>>(5)),
            sort_order: row.get(6),
            enabled: row.get::<_, i32>(7) != 0,
            created_at: row.get(8),
            updated_at: row.get(9),
        }))
    }

    fn list_external_links(&self, include_disabled: bool) -> Result<Vec<ExternalLinkRecord>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let rows = if include_disabled {
            conn.query(
                "SELECT link_id, title, description, url, icon, allowed_levels, sort_order, enabled, created_at, updated_at \n                 FROM external_links ORDER BY sort_order ASC, updated_at DESC, link_id ASC",
                &[],
            )?
        } else {
            conn.query(
                "SELECT link_id, title, description, url, icon, allowed_levels, sort_order, enabled, created_at, updated_at \n                 FROM external_links WHERE enabled = 1 ORDER BY sort_order ASC, updated_at DESC, link_id ASC",
                &[],
            )?
        };
        let mut output = Vec::new();
        for row in rows {
            output.push(ExternalLinkRecord {
                link_id: row.get(0),
                title: row.get(1),
                description: row.get(2),
                url: row.get(3),
                icon: row.get(4),
                allowed_levels: Self::parse_i32_list(row.get::<_, Option<String>>(5)),
                sort_order: row.get(6),
                enabled: row.get::<_, i32>(7) != 0,
                created_at: row.get(8),
                updated_at: row.get(9),
            });
        }
        Ok(output)
    }

    fn delete_external_link(&self, link_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = link_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected =
            conn.execute("DELETE FROM external_links WHERE link_id = $1", &[&cleaned])?;
        Ok(affected as i64)
    }

    fn create_user_token(&self, record: &UserTokenRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO user_tokens (token, user_id, expires_at, created_at, last_used_at) VALUES ($1, $2, $3, $4, $5)",
            &[
                &record.token,
                &record.user_id,
                &record.expires_at,
                &record.created_at,
                &record.last_used_at,
            ],
        )?;
        Ok(())
    }

    fn get_user_token(&self, token: &str) -> Result<Option<UserTokenRecord>> {
        self.ensure_initialized()?;
        let cleaned = token.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT token, user_id, expires_at, created_at, last_used_at FROM user_tokens WHERE token = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| UserTokenRecord {
            token: row.get(0),
            user_id: row.get(1),
            expires_at: row.get(2),
            created_at: row.get(3),
            last_used_at: row.get(4),
        }))
    }

    fn touch_user_token(&self, token: &str, last_used_at: f64) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = token.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "UPDATE user_tokens SET last_used_at = $1 WHERE token = $2",
            &[&last_used_at, &cleaned],
        )?;
        Ok(())
    }

    fn delete_user_token(&self, token: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = token.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute("DELETE FROM user_tokens WHERE token = $1", &[&cleaned])?;
        Ok(affected as i64)
    }

    fn upsert_chat_session(&self, record: &ChatSessionRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let tool_overrides = if record.tool_overrides.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.tool_overrides))
        };
        conn.execute(
            "INSERT INTO chat_sessions (session_id, user_id, title, status, created_at, updated_at, last_message_at, agent_id, tool_overrides, \
             parent_session_id, parent_message_id, spawn_label, spawned_by) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13) \
             ON CONFLICT(session_id) DO UPDATE SET user_id = EXCLUDED.user_id, title = EXCLUDED.title, status = EXCLUDED.status, \
             created_at = EXCLUDED.created_at, updated_at = EXCLUDED.updated_at, last_message_at = EXCLUDED.last_message_at, \
             agent_id = EXCLUDED.agent_id, tool_overrides = EXCLUDED.tool_overrides, parent_session_id = EXCLUDED.parent_session_id, \
             parent_message_id = EXCLUDED.parent_message_id, spawn_label = EXCLUDED.spawn_label, spawned_by = EXCLUDED.spawned_by",
            &[
                &record.session_id,
                &record.user_id,
                &record.title,
                &"active",
                &record.created_at,
                &record.updated_at,
                &record.last_message_at,
                &record.agent_id,
                &tool_overrides,
                &record.parent_session_id,
                &record.parent_message_id,
                &record.spawn_label,
                &record.spawned_by,
            ],
        )?;
        Ok(())
    }

    fn get_chat_session(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<Option<ChatSessionRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT session_id, user_id, title, created_at, updated_at, last_message_at, agent_id, tool_overrides, \
             parent_session_id, parent_message_id, spawn_label, spawned_by \
             FROM chat_sessions WHERE user_id = $1 AND session_id = $2",
            &[&cleaned_user, &cleaned_session],
        )?;
        Ok(row.map(|row| ChatSessionRecord {
            session_id: row.get(0),
            user_id: row.get(1),
            title: row.get(2),
            created_at: row.get(3),
            updated_at: row.get(4),
            last_message_at: row.get(5),
            agent_id: row.get(6),
            tool_overrides: Self::parse_string_list(row.get(7)),
            parent_session_id: row.get(8),
            parent_message_id: row.get(9),
            spawn_label: row.get(10),
            spawned_by: row.get(11),
        }))
    }

    fn list_chat_sessions(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        parent_session_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChatSessionRecord>, i64)> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok((Vec::new(), 0));
        }
        let mut conn = self.conn()?;
        let mut conditions = Vec::new();
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        params.push(Box::new(cleaned_user.to_string()));
        conditions.push(format!("user_id = ${}", params.len()));

        let agent_id = agent_id.map(|value| value.trim());
        match agent_id {
            None => {}
            Some("") => {
                conditions.push("(agent_id IS NULL OR agent_id = '')".to_string());
            }
            Some(value) => {
                params.push(Box::new(value.to_string()));
                conditions.push(format!("agent_id = ${}", params.len()));
            }
        }

        match parent_session_id {
            None => {}
            Some(value) if value.trim().is_empty() => {
                conditions
                    .push("(parent_session_id IS NULL OR parent_session_id = '')".to_string());
            }
            Some(value) => {
                params.push(Box::new(value.trim().to_string()));
                conditions.push(format!("parent_session_id = ${}", params.len()));
            }
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };
        let count_sql = format!("SELECT COUNT(*) FROM chat_sessions{where_clause}");
        let params_ref: Vec<&(dyn ToSql + Sync)> =
            params.iter().map(|value| value.as_ref()).collect();
        let total: i64 = conn.query_one(&count_sql, &params_ref)?.get(0);

        let mut sql = format!(
            "SELECT session_id, user_id, title, created_at, updated_at, last_message_at, agent_id, tool_overrides, \
             parent_session_id, parent_message_id, spawn_label, spawned_by FROM chat_sessions{where_clause} \
             ORDER BY updated_at DESC"
        );
        let mut list_params: Vec<Box<dyn ToSql + Sync>> = params;
        if limit > 0 {
            list_params.push(Box::new(limit));
            list_params.push(Box::new(offset.max(0)));
            sql.push_str(&format!(
                " LIMIT ${} OFFSET ${}",
                list_params.len() - 1,
                list_params.len()
            ));
        }
        let list_ref: Vec<&(dyn ToSql + Sync)> =
            list_params.iter().map(|value| value.as_ref()).collect();
        let rows = conn.query(&sql, &list_ref)?;
        let mut output = Vec::new();
        for row in rows {
            output.push(ChatSessionRecord {
                session_id: row.get(0),
                user_id: row.get(1),
                title: row.get(2),
                created_at: row.get(3),
                updated_at: row.get(4),
                last_message_at: row.get(5),
                agent_id: row.get(6),
                tool_overrides: Self::parse_string_list(row.get(7)),
                parent_session_id: row.get(8),
                parent_message_id: row.get(9),
                spawn_label: row.get(10),
                spawned_by: row.get(11),
            });
        }
        Ok((output, total))
    }

    fn list_chat_session_agent_ids(&self, user_id: &str) -> Result<Vec<String>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT DISTINCT agent_id FROM chat_sessions WHERE user_id = $1",
            &[&cleaned_user],
        )?;
        let mut agent_ids = Vec::new();
        for row in rows {
            let agent_id: Option<String> = row.get(0);
            agent_ids.push(agent_id.unwrap_or_default());
        }
        Ok(agent_ids)
    }

    fn update_chat_session_title(
        &self,
        user_id: &str,
        session_id: &str,
        title: &str,
        updated_at: f64,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "UPDATE chat_sessions SET title = $1, updated_at = $2 WHERE user_id = $3 AND session_id = $4",
            &[&title, &updated_at, &cleaned_user, &cleaned_session],
        )?;
        Ok(())
    }

    fn touch_chat_session(
        &self,
        user_id: &str,
        session_id: &str,
        updated_at: f64,
        last_message_at: f64,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "UPDATE chat_sessions SET updated_at = $1, last_message_at = $2 WHERE user_id = $3 AND session_id = $4",
            &[&updated_at, &last_message_at, &cleaned_user, &cleaned_session],
        )?;
        Ok(())
    }

    fn delete_chat_session(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM chat_sessions WHERE user_id = $1 AND session_id = $2",
            &[&cleaned_user, &cleaned_session],
        )?;
        Ok(affected as i64)
    }

    fn resolve_or_create_user_world_direct_conversation(
        &self,
        user_a: &str,
        user_b: &str,
        now: f64,
    ) -> Result<UserWorldConversationRecord> {
        self.ensure_initialized()?;
        let (participant_a, participant_b) = Self::normalize_user_world_pair(user_a, user_b)
            .ok_or_else(|| anyhow!("invalid user pair"))?;
        let now = if now.is_finite() && now > 0.0 {
            now
        } else {
            Self::now_ts()
        };
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        let existing = tx.query_opt(
            "SELECT conversation_id, conversation_type, participant_a, participant_b, created_at, updated_at, \
             last_message_at, last_message_id, last_message_preview \
             FROM user_world_conversations WHERE participant_a = $1 AND participant_b = $2",
            &[&participant_a, &participant_b],
        )?;
        if let Some(row) = existing {
            let record = Self::map_user_world_conversation_row(&row);
            tx.commit()?;
            return Ok(record);
        }
        let conversation_id = format!("uwc_{}", uuid::Uuid::new_v4().simple());
        tx.execute(
            "INSERT INTO user_world_conversations (conversation_id, conversation_type, participant_a, participant_b, \
             created_at, updated_at, last_message_at, last_message_id, last_message_preview) \
             VALUES ($1, 'direct', $2, $3, $4, $5, $6, NULL, NULL)",
            &[&conversation_id, &participant_a, &participant_b, &now, &now, &now],
        )?;
        tx.execute(
            "INSERT INTO user_world_members (conversation_id, user_id, peer_user_id, last_read_message_id, unread_count_cache, \
             pinned, muted, updated_at) VALUES ($1, $2, $3, NULL, 0, 0, 0, $4)",
            &[&conversation_id, &participant_a, &participant_b, &now],
        )?;
        tx.execute(
            "INSERT INTO user_world_members (conversation_id, user_id, peer_user_id, last_read_message_id, unread_count_cache, \
             pinned, muted, updated_at) VALUES ($1, $2, $3, NULL, 0, 0, 0, $4)",
            &[&conversation_id, &participant_b, &participant_a, &now],
        )?;
        let row = tx
            .query_opt(
                "SELECT conversation_id, conversation_type, participant_a, participant_b, created_at, updated_at, \
                 last_message_at, last_message_id, last_message_preview \
                 FROM user_world_conversations WHERE conversation_id = $1",
                &[&conversation_id],
            )?
            .ok_or_else(|| anyhow!("user world conversation missing after insert"))?;
        let record = Self::map_user_world_conversation_row(&row);
        tx.commit()?;
        Ok(record)
    }

    fn get_user_world_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Option<UserWorldConversationRecord>> {
        self.ensure_initialized()?;
        let cleaned = conversation_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT conversation_id, conversation_type, participant_a, participant_b, created_at, updated_at, \
             last_message_at, last_message_id, last_message_preview \
             FROM user_world_conversations WHERE conversation_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| Self::map_user_world_conversation_row(&row)))
    }

    fn get_user_world_member(
        &self,
        conversation_id: &str,
        user_id: &str,
    ) -> Result<Option<UserWorldMemberRecord>> {
        self.ensure_initialized()?;
        let cleaned_conversation = conversation_id.trim();
        let cleaned_user = user_id.trim();
        if cleaned_conversation.is_empty() || cleaned_user.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT conversation_id, user_id, peer_user_id, last_read_message_id, unread_count_cache, pinned, muted, updated_at \
             FROM user_world_members WHERE conversation_id = $1 AND user_id = $2",
            &[&cleaned_conversation, &cleaned_user],
        )?;
        Ok(row.map(|row| Self::map_user_world_member_row(&row)))
    }

    fn list_user_world_conversations(
        &self,
        user_id: &str,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserWorldConversationSummaryRecord>, i64)> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok((Vec::new(), 0));
        }
        let mut conn = self.conn()?;
        let total_row = conn.query_one(
            "SELECT COUNT(*) FROM user_world_members WHERE user_id = $1",
            &[&cleaned_user],
        )?;
        let total: i64 = total_row.get(0);
        let rows = if limit > 0 {
            let safe_limit = limit.max(1);
            let safe_offset = offset.max(0);
            conn.query(
                "SELECT c.conversation_id, c.conversation_type, m.peer_user_id, m.last_read_message_id, \
                 m.unread_count_cache, m.pinned, m.muted, m.updated_at, \
                 c.last_message_at, c.last_message_id, c.last_message_preview \
                 FROM user_world_members m \
                 JOIN user_world_conversations c ON c.conversation_id = m.conversation_id \
                 WHERE m.user_id = $1 \
                 ORDER BY m.pinned DESC, c.last_message_at DESC, m.updated_at DESC \
                 LIMIT $2 OFFSET $3",
                &[&cleaned_user, &safe_limit, &safe_offset],
            )?
        } else {
            conn.query(
                "SELECT c.conversation_id, c.conversation_type, m.peer_user_id, m.last_read_message_id, \
                 m.unread_count_cache, m.pinned, m.muted, m.updated_at, \
                 c.last_message_at, c.last_message_id, c.last_message_preview \
                 FROM user_world_members m \
                 JOIN user_world_conversations c ON c.conversation_id = m.conversation_id \
                 WHERE m.user_id = $1 \
                 ORDER BY m.pinned DESC, c.last_message_at DESC, m.updated_at DESC",
                &[&cleaned_user],
            )?
        };
        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            output.push(UserWorldConversationSummaryRecord {
                conversation_id: row.get(0),
                conversation_type: row.get(1),
                peer_user_id: row.get(2),
                last_read_message_id: row.get(3),
                unread_count_cache: row.get(4),
                pinned: row.get::<_, i32>(5) != 0,
                muted: row.get::<_, i32>(6) != 0,
                updated_at: row.get(7),
                last_message_at: row.get(8),
                last_message_id: row.get(9),
                last_message_preview: row.get(10),
            });
        }
        Ok((output, total))
    }

    fn list_user_world_messages(
        &self,
        conversation_id: &str,
        before_message_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<UserWorldMessageRecord>> {
        self.ensure_initialized()?;
        let cleaned = conversation_id.trim();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let safe_limit = if limit <= 0 { 50 } else { limit.min(200) };
        let mut conn = self.conn()?;
        let rows = if let Some(before_id) = before_message_id.filter(|value| *value > 0) {
            conn.query(
                "SELECT message_id, conversation_id, sender_user_id, content, content_type, client_msg_id, created_at \
                 FROM user_world_messages WHERE conversation_id = $1 AND message_id < $2 \
                 ORDER BY message_id DESC LIMIT $3",
                &[&cleaned, &before_id, &safe_limit],
            )?
        } else {
            conn.query(
                "SELECT message_id, conversation_id, sender_user_id, content, content_type, client_msg_id, created_at \
                 FROM user_world_messages WHERE conversation_id = $1 \
                 ORDER BY message_id DESC LIMIT $2",
                &[&cleaned, &safe_limit],
            )?
        };
        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            output.push(Self::map_user_world_message_row(&row));
        }
        Ok(output)
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
        self.ensure_initialized()?;
        let cleaned_conversation = conversation_id.trim();
        let cleaned_sender = sender_user_id.trim();
        let cleaned_content = content.trim();
        if cleaned_conversation.is_empty()
            || cleaned_sender.is_empty()
            || cleaned_content.is_empty()
        {
            return Err(anyhow!("invalid message payload"));
        }
        let normalized_content_type = {
            let cleaned = content_type.trim();
            if cleaned.is_empty() {
                "text"
            } else {
                cleaned
            }
        };
        let cleaned_client_msg = client_msg_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let now = if now.is_finite() && now > 0.0 {
            now
        } else {
            Self::now_ts()
        };
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        let conversation_row = tx
            .query_opt(
                "SELECT participant_a, participant_b FROM user_world_conversations WHERE conversation_id = $1",
                &[&cleaned_conversation],
            )?
            .ok_or_else(|| anyhow!("conversation not found"))?;
        let participant_a: String = conversation_row.get(0);
        let participant_b: String = conversation_row.get(1);
        if cleaned_sender != participant_a && cleaned_sender != participant_b {
            return Err(anyhow!("sender is not a member of conversation"));
        }

        if let Some(client_msg_id) = cleaned_client_msg.as_deref() {
            if let Some(existing) = tx.query_opt(
                "SELECT message_id, conversation_id, sender_user_id, content, content_type, client_msg_id, created_at \
                 FROM user_world_messages WHERE conversation_id = $1 AND client_msg_id = $2",
                &[&cleaned_conversation, &client_msg_id],
            )? {
                let message = Self::map_user_world_message_row(&existing);
                tx.commit()?;
                return Ok(UserWorldSendMessageResult {
                    message,
                    inserted: false,
                    event: None,
                });
            }
        }

        let insert_row = tx.query_one(
            "INSERT INTO user_world_messages (conversation_id, sender_user_id, content, content_type, client_msg_id, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING message_id",
            &[
                &cleaned_conversation,
                &cleaned_sender,
                &cleaned_content,
                &normalized_content_type,
                &cleaned_client_msg,
                &now,
            ],
        )?;
        let message_id: i64 = insert_row.get(0);
        let preview = cleaned_content.chars().take(120).collect::<String>();
        tx.execute(
            "UPDATE user_world_conversations SET updated_at = $1, last_message_at = $2, last_message_id = $3, \
             last_message_preview = $4 WHERE conversation_id = $5",
            &[&now, &now, &message_id, &preview, &cleaned_conversation],
        )?;
        let peer_user = if cleaned_sender == participant_a {
            participant_b.as_str()
        } else {
            participant_a.as_str()
        };
        tx.execute(
            "UPDATE user_world_members SET last_read_message_id = $1, unread_count_cache = 0, updated_at = $2 \
             WHERE conversation_id = $3 AND user_id = $4",
            &[&message_id, &now, &cleaned_conversation, &cleaned_sender],
        )?;
        tx.execute(
            "UPDATE user_world_members SET unread_count_cache = COALESCE(unread_count_cache, 0) + 1, updated_at = $1 \
             WHERE conversation_id = $2 AND user_id = $3",
            &[&now, &cleaned_conversation, &peer_user],
        )?;

        let message_row = tx.query_one(
            "SELECT message_id, conversation_id, sender_user_id, content, content_type, client_msg_id, created_at \
             FROM user_world_messages WHERE message_id = $1",
            &[&message_id],
        )?;
        let message = Self::map_user_world_message_row(&message_row);
        let event_id_row = tx.query_one(
            "SELECT COALESCE(MAX(event_id), 0) + 1 FROM user_world_events WHERE conversation_id = $1",
            &[&cleaned_conversation],
        )?;
        let next_event_id: i64 = event_id_row.get(0);
        let payload = json!({
            "conversation_id": message.conversation_id,
            "message": {
                "message_id": message.message_id,
                "conversation_id": message.conversation_id,
                "sender_user_id": message.sender_user_id,
                "content": message.content,
                "content_type": message.content_type,
                "client_msg_id": message.client_msg_id,
                "created_at": message.created_at,
            }
        });
        let payload_text = Self::json_to_string(&payload);
        tx.execute(
            "INSERT INTO user_world_events (conversation_id, event_id, event_type, payload, created_time) VALUES ($1, $2, $3, $4, $5)",
            &[&cleaned_conversation, &next_event_id, &"uw.message", &payload_text, &now],
        )?;
        tx.commit()?;
        Ok(UserWorldSendMessageResult {
            message,
            inserted: true,
            event: Some(UserWorldEventRecord {
                conversation_id: cleaned_conversation.to_string(),
                event_id: next_event_id,
                event_type: "uw.message".to_string(),
                payload,
                created_time: now,
            }),
        })
    }

    fn mark_user_world_read(
        &self,
        conversation_id: &str,
        user_id: &str,
        last_read_message_id: Option<i64>,
        now: f64,
    ) -> Result<Option<UserWorldReadResult>> {
        self.ensure_initialized()?;
        let cleaned_conversation = conversation_id.trim();
        let cleaned_user = user_id.trim();
        if cleaned_conversation.is_empty() || cleaned_user.is_empty() {
            return Ok(None);
        }
        let now = if now.is_finite() && now > 0.0 {
            now
        } else {
            Self::now_ts()
        };
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        let current_member_row = tx.query_opt(
            "SELECT conversation_id, user_id, peer_user_id, last_read_message_id, unread_count_cache, pinned, muted, updated_at \
             FROM user_world_members WHERE conversation_id = $1 AND user_id = $2",
            &[&cleaned_conversation, &cleaned_user],
        )?;
        let Some(current_member_row) = current_member_row else {
            tx.commit()?;
            return Ok(None);
        };
        let mut member = Self::map_user_world_member_row(&current_member_row);
        let prev_last_read_message_id = member.last_read_message_id;
        let prev_unread_count = member.unread_count_cache;

        let max_message_row = tx.query_one(
            "SELECT MAX(message_id) FROM user_world_messages WHERE conversation_id = $1",
            &[&cleaned_conversation],
        )?;
        let max_message_id: Option<i64> = max_message_row.get(0);
        let resolved_target = match last_read_message_id.filter(|value| *value > 0) {
            Some(target) => max_message_id.map(|max_id| target.min(max_id)),
            None => max_message_id,
        };
        let current_last = member.last_read_message_id.unwrap_or(0);
        let next_last = resolved_target.unwrap_or(0).max(current_last);
        let unread_query = if next_last > 0 {
            tx.query_one(
                "SELECT COUNT(*) FROM user_world_messages \
                 WHERE conversation_id = $1 AND sender_user_id <> $2 AND message_id > $3",
                &[&cleaned_conversation, &cleaned_user, &next_last],
            )?
        } else {
            tx.query_one(
                "SELECT COUNT(*) FROM user_world_messages \
                 WHERE conversation_id = $1 AND sender_user_id <> $2",
                &[&cleaned_conversation, &cleaned_user],
            )?
        };
        let unread_count: i64 = unread_query.get(0);
        let next_last_opt = if next_last > 0 { Some(next_last) } else { None };
        tx.execute(
            "UPDATE user_world_members SET last_read_message_id = $1, unread_count_cache = $2, updated_at = $3 \
             WHERE conversation_id = $4 AND user_id = $5",
            &[
                &next_last_opt,
                &unread_count,
                &now,
                &cleaned_conversation,
                &cleaned_user,
            ],
        )?;
        member.last_read_message_id = next_last_opt;
        member.unread_count_cache = unread_count;
        member.updated_at = now;

        let changed = member.last_read_message_id != prev_last_read_message_id
            || member.unread_count_cache != prev_unread_count;
        if !changed {
            tx.commit()?;
            return Ok(Some(UserWorldReadResult {
                member,
                event: None,
            }));
        }

        let next_event_id_row = tx.query_one(
            "SELECT COALESCE(MAX(event_id), 0) + 1 FROM user_world_events WHERE conversation_id = $1",
            &[&cleaned_conversation],
        )?;
        let next_event_id: i64 = next_event_id_row.get(0);
        let payload = json!({
            "conversation_id": cleaned_conversation,
            "user_id": cleaned_user,
            "last_read_message_id": member.last_read_message_id,
            "unread_count": member.unread_count_cache,
        });
        let payload_text = Self::json_to_string(&payload);
        tx.execute(
            "INSERT INTO user_world_events (conversation_id, event_id, event_type, payload, created_time) VALUES ($1, $2, $3, $4, $5)",
            &[&cleaned_conversation, &next_event_id, &"uw.read", &payload_text, &now],
        )?;
        tx.commit()?;
        Ok(Some(UserWorldReadResult {
            member,
            event: Some(UserWorldEventRecord {
                conversation_id: cleaned_conversation.to_string(),
                event_id: next_event_id,
                event_type: "uw.read".to_string(),
                payload,
                created_time: now,
            }),
        }))
    }

    fn list_user_world_events(
        &self,
        conversation_id: &str,
        after_event_id: i64,
        limit: i64,
    ) -> Result<Vec<UserWorldEventRecord>> {
        self.ensure_initialized()?;
        let cleaned = conversation_id.trim();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let safe_limit = if limit <= 0 { 100 } else { limit.min(500) };
        let safe_after = after_event_id.max(0);
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT conversation_id, event_id, event_type, payload, created_time \
             FROM user_world_events WHERE conversation_id = $1 AND event_id > $2 \
             ORDER BY event_id ASC LIMIT $3",
            &[&cleaned, &safe_after, &safe_limit],
        )?;
        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            let payload_text: Option<String> = row.get(3);
            output.push(UserWorldEventRecord {
                conversation_id: row.get(0),
                event_id: row.get(1),
                event_type: row.get(2),
                payload: Self::parse_json_column(payload_text),
                created_time: row.get(4),
            });
        }
        Ok(output)
    }

    fn upsert_channel_account(&self, record: &ChannelAccountRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let config = Self::json_to_string(&record.config);
        conn.execute(
            "INSERT INTO channel_accounts (channel, account_id, config, status, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT(channel, account_id) DO UPDATE SET config = EXCLUDED.config, status = EXCLUDED.status, updated_at = EXCLUDED.updated_at",
            &[
                &record.channel,
                &record.account_id,
                &config,
                &record.status,
                &record.created_at,
                &record.updated_at,
            ],
        )?;
        Ok(())
    }

    fn get_channel_account(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<Option<ChannelAccountRecord>> {
        self.ensure_initialized()?;
        let cleaned_channel = channel.trim();
        let cleaned_account = account_id.trim();
        if cleaned_channel.is_empty() || cleaned_account.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT channel, account_id, config, status, created_at, updated_at FROM channel_accounts WHERE channel = $1 AND account_id = $2",
            &[&cleaned_channel, &cleaned_account],
        )?;
        Ok(row.map(|row| ChannelAccountRecord {
            channel: row.get(0),
            account_id: row.get(1),
            config: Self::json_from_str(row.get::<_, String>(2).as_str()).unwrap_or(Value::Null),
            status: row.get(3),
            created_at: row.get(4),
            updated_at: row.get(5),
        }))
    }

    fn list_channel_accounts(
        &self,
        channel: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<ChannelAccountRecord>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut filters = Vec::new();
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        if let Some(channel) = channel
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            params.push(Box::new(channel.to_string()));
            filters.push(format!("channel = ${}", params.len()));
        }
        if let Some(status) = status
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            params.push(Box::new(status.to_string()));
            filters.push(format!("status = ${}", params.len()));
        }
        let mut query = "SELECT channel, account_id, config, status, created_at, updated_at FROM channel_accounts".to_string();
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" ORDER BY updated_at DESC");
        let params_refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p.as_ref()).collect();
        let rows = conn.query(&query, &params_refs)?;
        let mut output = Vec::new();
        for row in rows {
            output.push(ChannelAccountRecord {
                channel: row.get(0),
                account_id: row.get(1),
                config: Self::json_from_str(row.get::<_, String>(2).as_str())
                    .unwrap_or(Value::Null),
                status: row.get(3),
                created_at: row.get(4),
                updated_at: row.get(5),
            });
        }
        Ok(output)
    }

    fn delete_channel_account(&self, channel: &str, account_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_channel = channel.trim();
        let cleaned_account = account_id.trim();
        if cleaned_channel.is_empty() || cleaned_account.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM channel_accounts WHERE channel = $1 AND account_id = $2",
            &[&cleaned_channel, &cleaned_account],
        )?;
        Ok(affected as i64)
    }

    fn upsert_channel_binding(&self, record: &ChannelBindingRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let tool_overrides = if record.tool_overrides.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.tool_overrides))
        };
        let enabled = if record.enabled { 1 } else { 0 };
        conn.execute(
            "INSERT INTO channel_bindings (binding_id, channel, account_id, peer_kind, peer_id, agent_id, tool_overrides, priority, enabled, created_at, updated_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) \
             ON CONFLICT(binding_id) DO UPDATE SET channel = EXCLUDED.channel, account_id = EXCLUDED.account_id, peer_kind = EXCLUDED.peer_kind, peer_id = EXCLUDED.peer_id, \
             agent_id = EXCLUDED.agent_id, tool_overrides = EXCLUDED.tool_overrides, priority = EXCLUDED.priority, enabled = EXCLUDED.enabled, updated_at = EXCLUDED.updated_at",
            &[
                &record.binding_id,
                &record.channel,
                &record.account_id,
                &record.peer_kind,
                &record.peer_id,
                &record.agent_id,
                &tool_overrides,
                &record.priority,
                &enabled,
                &record.created_at,
                &record.updated_at,
            ],
        )?;
        Ok(())
    }

    fn list_channel_bindings(&self, channel: Option<&str>) -> Result<Vec<ChannelBindingRecord>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut query = "SELECT binding_id, channel, account_id, peer_kind, peer_id, agent_id, tool_overrides, priority, enabled, created_at, updated_at FROM channel_bindings".to_string();
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        if let Some(channel) = channel
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            params.push(Box::new(channel.to_string()));
            query.push_str(&format!(" WHERE channel = ${}", params.len()));
        }
        query.push_str(" ORDER BY priority DESC, updated_at DESC");
        let params_refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p.as_ref()).collect();
        let rows = conn.query(&query, &params_refs)?;
        let mut output = Vec::new();
        for row in rows {
            let tool_overrides: Option<String> = row.get(6);
            output.push(ChannelBindingRecord {
                binding_id: row.get(0),
                channel: row.get(1),
                account_id: row.get(2),
                peer_kind: row.get(3),
                peer_id: row.get(4),
                agent_id: row.get(5),
                tool_overrides: Self::parse_string_list(tool_overrides),
                priority: row.get::<_, i64>(7),
                enabled: row.get::<_, i32>(8) != 0,
                created_at: row.get(9),
                updated_at: row.get(10),
            });
        }
        Ok(output)
    }

    fn delete_channel_binding(&self, binding_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = binding_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM channel_bindings WHERE binding_id = $1",
            &[&cleaned],
        )?;
        Ok(affected as i64)
    }

    fn upsert_channel_user_binding(&self, record: &ChannelUserBindingRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO channel_user_bindings (channel, account_id, peer_kind, peer_id, user_id, created_at, updated_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7) \
             ON CONFLICT(channel, account_id, peer_kind, peer_id) DO UPDATE SET user_id = EXCLUDED.user_id, updated_at = EXCLUDED.updated_at",
            &[
                &record.channel,
                &record.account_id,
                &record.peer_kind,
                &record.peer_id,
                &record.user_id,
                &record.created_at,
                &record.updated_at,
            ],
        )?;
        Ok(())
    }

    fn get_channel_user_binding(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
    ) -> Result<Option<ChannelUserBindingRecord>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let row = conn
            .query_opt(
                "SELECT channel, account_id, peer_kind, peer_id, user_id, created_at, updated_at \
                 FROM channel_user_bindings WHERE channel = $1 AND account_id = $2 AND peer_kind = $3 AND peer_id = $4",
                &[&channel, &account_id, &peer_kind, &peer_id],
            )?
            .map(|row| ChannelUserBindingRecord {
                channel: row.get(0),
                account_id: row.get(1),
                peer_kind: row.get(2),
                peer_id: row.get(3),
                user_id: row.get(4),
                created_at: row.get::<_, Option<f64>>(5).unwrap_or(0.0),
                updated_at: row.get::<_, Option<f64>>(6).unwrap_or(0.0),
            });
        Ok(row)
    }

    fn list_channel_user_bindings(
        &self,
        query: ListChannelUserBindingsQuery<'_>,
    ) -> Result<(Vec<ChannelUserBindingRecord>, i64)> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut filters = Vec::new();
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        if let Some(channel) = query
            .channel
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            params.push(Box::new(channel.to_string()));
            filters.push(format!("channel = ${}", params.len()));
        }
        if let Some(account_id) = query
            .account_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            params.push(Box::new(account_id.to_string()));
            filters.push(format!("account_id = ${}", params.len()));
        }
        if let Some(peer_kind) = query
            .peer_kind
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            params.push(Box::new(peer_kind.to_string()));
            filters.push(format!("peer_kind = ${}", params.len()));
        }
        if let Some(peer_id) = query
            .peer_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            params.push(Box::new(peer_id.to_string()));
            filters.push(format!("peer_id = ${}", params.len()));
        }
        if let Some(user_id) = query
            .user_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            params.push(Box::new(user_id.to_string()));
            filters.push(format!("user_id = ${}", params.len()));
        }
        let mut sql = "SELECT channel, account_id, peer_kind, peer_id, user_id, created_at, updated_at FROM channel_user_bindings".to_string();
        if !filters.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&filters.join(" AND "));
        }
        sql.push_str(" ORDER BY updated_at DESC");
        let offset_value = query.offset.max(0);
        let limit_value = if query.limit <= 0 {
            100
        } else {
            query.limit.min(500)
        };
        params.push(Box::new(offset_value));
        params.push(Box::new(limit_value));
        sql.push_str(&format!(
            " OFFSET ${} LIMIT ${}",
            params.len() - 1,
            params.len()
        ));
        let params_refs: Vec<&(dyn ToSql + Sync)> =
            params.iter().map(|item| item.as_ref()).collect();
        let rows = conn.query(&sql, &params_refs)?;
        let mut output = Vec::new();
        for row in rows {
            output.push(ChannelUserBindingRecord {
                channel: row.get(0),
                account_id: row.get(1),
                peer_kind: row.get(2),
                peer_id: row.get(3),
                user_id: row.get(4),
                created_at: row.get::<_, Option<f64>>(5).unwrap_or(0.0),
                updated_at: row.get::<_, Option<f64>>(6).unwrap_or(0.0),
            });
        }
        let mut count_sql = "SELECT COUNT(*) FROM channel_user_bindings".to_string();
        if !filters.is_empty() {
            count_sql.push_str(" WHERE ");
            count_sql.push_str(&filters.join(" AND "));
        }
        let count_params: Vec<&(dyn ToSql + Sync)> = params_refs[..params_refs.len() - 2].to_vec();
        let total_row = conn.query_one(&count_sql, &count_params)?;
        let total: i64 = total_row.get(0);
        Ok((output, total))
    }

    fn delete_channel_user_binding(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
    ) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_channel = channel.trim();
        let cleaned_account = account_id.trim();
        let cleaned_kind = peer_kind.trim();
        let cleaned_peer = peer_id.trim();
        if cleaned_channel.is_empty()
            || cleaned_account.is_empty()
            || cleaned_kind.is_empty()
            || cleaned_peer.is_empty()
        {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM channel_user_bindings WHERE channel = $1 AND account_id = $2 AND peer_kind = $3 AND peer_id = $4",
            &[&cleaned_channel, &cleaned_account, &cleaned_kind, &cleaned_peer],
        )?;
        Ok(affected as i64)
    }

    fn upsert_channel_session(&self, record: &ChannelSessionRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let thread_id = Self::normalize_channel_thread_id(record.thread_id.as_deref());
        let metadata = record.metadata.as_ref().map(Self::json_to_string);
        let tts_enabled = record.tts_enabled.map(|value| if value { 1 } else { 0 });
        conn.execute(
            "INSERT INTO channel_sessions (channel, account_id, peer_kind, peer_id, thread_id, session_id, agent_id, user_id, tts_enabled, tts_voice, metadata, last_message_at, created_at, updated_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) \
             ON CONFLICT(channel, account_id, peer_kind, peer_id, thread_id) DO UPDATE SET session_id = EXCLUDED.session_id, agent_id = EXCLUDED.agent_id, user_id = EXCLUDED.user_id, \
             tts_enabled = EXCLUDED.tts_enabled, tts_voice = EXCLUDED.tts_voice, metadata = EXCLUDED.metadata, last_message_at = EXCLUDED.last_message_at, updated_at = EXCLUDED.updated_at",
            &[
                &record.channel,
                &record.account_id,
                &record.peer_kind,
                &record.peer_id,
                &thread_id,
                &record.session_id,
                &record.agent_id,
                &record.user_id,
                &tts_enabled,
                &record.tts_voice,
                &metadata,
                &record.last_message_at,
                &record.created_at,
                &record.updated_at,
            ],
        )?;
        Ok(())
    }

    fn get_channel_session(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
        thread_id: Option<&str>,
    ) -> Result<Option<ChannelSessionRecord>> {
        self.ensure_initialized()?;
        let cleaned_channel = channel.trim();
        let cleaned_account = account_id.trim();
        let cleaned_peer_kind = peer_kind.trim();
        let cleaned_peer_id = peer_id.trim();
        if cleaned_channel.is_empty()
            || cleaned_account.is_empty()
            || cleaned_peer_kind.is_empty()
            || cleaned_peer_id.is_empty()
        {
            return Ok(None);
        }
        let thread_id = Self::normalize_channel_thread_id(thread_id);
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT channel, account_id, peer_kind, peer_id, thread_id, session_id, agent_id, user_id, tts_enabled, tts_voice, metadata, last_message_at, created_at, updated_at \
             FROM channel_sessions WHERE channel = $1 AND account_id = $2 AND peer_kind = $3 AND peer_id = $4 AND thread_id IS NOT DISTINCT FROM $5",
            &[
                &cleaned_channel,
                &cleaned_account,
                &cleaned_peer_kind,
                &cleaned_peer_id,
                &thread_id,
            ],
        )?;
        Ok(row.map(|row| ChannelSessionRecord {
            channel: row.get(0),
            account_id: row.get(1),
            peer_kind: row.get(2),
            peer_id: row.get(3),
            thread_id: Self::normalize_channel_thread_value(row.get(4)),
            session_id: row.get(5),
            agent_id: row.get(6),
            user_id: row.get(7),
            tts_enabled: row.get::<_, Option<i32>>(8).map(|value| value != 0),
            tts_voice: row.get(9),
            metadata: row
                .get::<_, Option<String>>(10)
                .and_then(|value| Self::json_from_str(&value)),
            last_message_at: row.get::<_, Option<f64>>(11).unwrap_or(0.0),
            created_at: row.get::<_, Option<f64>>(12).unwrap_or(0.0),
            updated_at: row.get::<_, Option<f64>>(13).unwrap_or(0.0),
        }))
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
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut filters = Vec::new();
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        if let Some(channel) = channel
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            params.push(Box::new(channel.to_string()));
            filters.push(format!("channel = ${}", params.len()));
        }
        if let Some(account) = account_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            params.push(Box::new(account.to_string()));
            filters.push(format!("account_id = ${}", params.len()));
        }
        if let Some(peer_id) = peer_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            params.push(Box::new(peer_id.to_string()));
            filters.push(format!("peer_id = ${}", params.len()));
        }
        if let Some(session_id) = session_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            params.push(Box::new(session_id.to_string()));
            filters.push(format!("session_id = ${}", params.len()));
        }
        let mut query =
            "SELECT channel, account_id, peer_kind, peer_id, thread_id, session_id, agent_id, user_id, tts_enabled, tts_voice, metadata, last_message_at, created_at, updated_at FROM channel_sessions"
                .to_string();
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" ORDER BY updated_at DESC");
        let offset_value = offset.max(0);
        let limit_value = if limit <= 0 { 100 } else { limit.min(500) };
        params.push(Box::new(offset_value));
        params.push(Box::new(limit_value));
        query.push_str(&format!(
            " OFFSET ${} LIMIT ${}",
            params.len() - 1,
            params.len()
        ));
        let params_refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p.as_ref()).collect();
        let rows = conn.query(&query, &params_refs)?;
        let mut output = Vec::new();
        for row in rows {
            output.push(ChannelSessionRecord {
                channel: row.get(0),
                account_id: row.get(1),
                peer_kind: row.get(2),
                peer_id: row.get(3),
                thread_id: Self::normalize_channel_thread_value(row.get(4)),
                session_id: row.get(5),
                agent_id: row.get(6),
                user_id: row.get(7),
                tts_enabled: row.get::<_, Option<i32>>(8).map(|value| value != 0),
                tts_voice: row.get(9),
                metadata: row
                    .get::<_, Option<String>>(10)
                    .and_then(|value| Self::json_from_str(&value)),
                last_message_at: row.get::<_, Option<f64>>(11).unwrap_or(0.0),
                created_at: row.get::<_, Option<f64>>(12).unwrap_or(0.0),
                updated_at: row.get::<_, Option<f64>>(13).unwrap_or(0.0),
            });
        }
        let mut count_query = "SELECT COUNT(*) FROM channel_sessions".to_string();
        if !filters.is_empty() {
            count_query.push_str(" WHERE ");
            count_query.push_str(&filters.join(" AND "));
        }
        let count_params: Vec<&(dyn ToSql + Sync)> = params_refs[..params_refs.len() - 2].to_vec();
        let total_row = conn.query_one(&count_query, &count_params)?;
        let total: i64 = total_row.get(0);
        Ok((output, total))
    }

    fn insert_channel_message(&self, record: &ChannelMessageRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let payload = Self::json_to_string(&record.payload);
        let raw_payload = record.raw_payload.as_ref().map(Self::json_to_string);
        conn.execute(
            "INSERT INTO channel_messages (channel, account_id, peer_kind, peer_id, thread_id, session_id, message_id, sender_id, message_type, payload, raw_payload, created_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
            &[
                &record.channel,
                &record.account_id,
                &record.peer_kind,
                &record.peer_id,
                &record.thread_id,
                &record.session_id,
                &record.message_id,
                &record.sender_id,
                &record.message_type,
                &payload,
                &raw_payload,
                &record.created_at,
            ],
        )?;
        Ok(())
    }

    fn list_channel_messages(
        &self,
        channel: Option<&str>,
        session_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<ChannelMessageRecord>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut filters = Vec::new();
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        if let Some(channel) = channel
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            params.push(Box::new(channel.to_string()));
            filters.push(format!("channel = ${}", params.len()));
        }
        if let Some(session_id) = session_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            params.push(Box::new(session_id.to_string()));
            filters.push(format!("session_id = ${}", params.len()));
        }
        let mut query = "SELECT channel, account_id, peer_kind, peer_id, thread_id, session_id, message_id, sender_id, message_type, payload, raw_payload, created_at \
             FROM channel_messages".to_string();
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" ORDER BY id DESC");
        let limit_value = if limit <= 0 { 50 } else { limit.min(200) };
        params.push(Box::new(limit_value));
        query.push_str(&format!(" LIMIT ${}", params.len()));
        let params_refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p.as_ref()).collect();
        let rows = conn.query(&query, &params_refs)?;
        let mut output = Vec::new();
        for row in rows {
            output.push(ChannelMessageRecord {
                channel: row.get(0),
                account_id: row.get(1),
                peer_kind: row.get(2),
                peer_id: row.get(3),
                thread_id: row.get(4),
                session_id: row.get(5),
                message_id: row.get(6),
                sender_id: row.get(7),
                message_type: row.get(8),
                payload: Self::json_from_str(row.get::<_, String>(9).as_str())
                    .unwrap_or(Value::Null),
                raw_payload: row
                    .get::<_, Option<String>>(10)
                    .and_then(|value| Self::json_from_str(&value)),
                created_at: row.get::<_, Option<f64>>(11).unwrap_or(0.0),
            });
        }
        Ok(output)
    }

    fn enqueue_channel_outbox(&self, record: &ChannelOutboxRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let payload = Self::json_to_string(&record.payload);
        conn.execute(
            "INSERT INTO channel_outbox (outbox_id, channel, account_id, peer_kind, peer_id, thread_id, payload, status, retry_count, retry_at, last_error, created_at, updated_at, delivered_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) \
             ON CONFLICT(outbox_id) DO UPDATE SET payload = EXCLUDED.payload, status = EXCLUDED.status, retry_count = EXCLUDED.retry_count, retry_at = EXCLUDED.retry_at, \
             last_error = EXCLUDED.last_error, updated_at = EXCLUDED.updated_at, delivered_at = EXCLUDED.delivered_at",
            &[
                &record.outbox_id,
                &record.channel,
                &record.account_id,
                &record.peer_kind,
                &record.peer_id,
                &record.thread_id,
                &payload,
                &record.status,
                &record.retry_count,
                &record.retry_at,
                &record.last_error,
                &record.created_at,
                &record.updated_at,
                &record.delivered_at,
            ],
        )?;
        Ok(())
    }

    fn get_channel_outbox(&self, outbox_id: &str) -> Result<Option<ChannelOutboxRecord>> {
        self.ensure_initialized()?;
        let cleaned = outbox_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT outbox_id, channel, account_id, peer_kind, peer_id, thread_id, payload, status, retry_count, retry_at, last_error, created_at, updated_at, delivered_at \
             FROM channel_outbox WHERE outbox_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| ChannelOutboxRecord {
            outbox_id: row.get(0),
            channel: row.get(1),
            account_id: row.get(2),
            peer_kind: row.get(3),
            peer_id: row.get(4),
            thread_id: row.get(5),
            payload: Self::json_from_str(row.get::<_, String>(6).as_str()).unwrap_or(Value::Null),
            status: row.get(7),
            retry_count: row.get(8),
            retry_at: row.get::<_, Option<f64>>(9).unwrap_or(0.0),
            last_error: row.get(10),
            created_at: row.get::<_, Option<f64>>(11).unwrap_or(0.0),
            updated_at: row.get::<_, Option<f64>>(12).unwrap_or(0.0),
            delivered_at: row.get(13),
        }))
    }

    fn list_pending_channel_outbox(&self, limit: i64) -> Result<Vec<ChannelOutboxRecord>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let now = Self::now_ts();
        let limit_value = if limit <= 0 { 50 } else { limit.min(200) };
        let rows = conn.query(
            "SELECT outbox_id, channel, account_id, peer_kind, peer_id, thread_id, payload, status, retry_count, retry_at, last_error, created_at, updated_at, delivered_at \
             FROM channel_outbox WHERE (status = 'pending' OR status = 'retry') AND retry_at <= $1 \
             ORDER BY retry_at ASC LIMIT $2",
            &[&now, &limit_value],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(ChannelOutboxRecord {
                outbox_id: row.get(0),
                channel: row.get(1),
                account_id: row.get(2),
                peer_kind: row.get(3),
                peer_id: row.get(4),
                thread_id: row.get(5),
                payload: Self::json_from_str(row.get::<_, String>(6).as_str())
                    .unwrap_or(Value::Null),
                status: row.get(7),
                retry_count: row.get(8),
                retry_at: row.get::<_, Option<f64>>(9).unwrap_or(0.0),
                last_error: row.get(10),
                created_at: row.get::<_, Option<f64>>(11).unwrap_or(0.0),
                updated_at: row.get::<_, Option<f64>>(12).unwrap_or(0.0),
                delivered_at: row.get(13),
            });
        }
        Ok(output)
    }

    fn update_channel_outbox_status(
        &self,
        params: UpdateChannelOutboxStatusParams<'_>,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = params.outbox_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "UPDATE channel_outbox SET status = $1, retry_count = $2, retry_at = $3, last_error = $4, updated_at = $5, delivered_at = $6 WHERE outbox_id = $7",
            &[
                &params.status,
                &params.retry_count,
                &params.retry_at,
                &params.last_error,
                &params.updated_at,
                &params.delivered_at,
                &cleaned,
            ],
        )?;
        Ok(())
    }

    fn upsert_gateway_client(&self, record: &GatewayClientRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let scopes = if record.scopes.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.scopes))
        };
        let caps = if record.caps.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.caps))
        };
        let commands = if record.commands.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.commands))
        };
        let client_info = record.client_info.as_ref().map(Self::json_to_string);
        conn.execute(
            "INSERT INTO gateway_clients (connection_id, role, user_id, node_id, scopes, caps, commands, client_info, status, connected_at, last_seen_at, disconnected_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) \
             ON CONFLICT(connection_id) DO UPDATE SET role = EXCLUDED.role, user_id = EXCLUDED.user_id, node_id = EXCLUDED.node_id, scopes = EXCLUDED.scopes, \
             caps = EXCLUDED.caps, commands = EXCLUDED.commands, client_info = EXCLUDED.client_info, status = EXCLUDED.status, last_seen_at = EXCLUDED.last_seen_at, \
             disconnected_at = EXCLUDED.disconnected_at",
            &[
                &record.connection_id,
                &record.role,
                &record.user_id,
                &record.node_id,
                &scopes,
                &caps,
                &commands,
                &client_info,
                &record.status,
                &record.connected_at,
                &record.last_seen_at,
                &record.disconnected_at,
            ],
        )?;
        Ok(())
    }

    fn list_gateway_clients(&self, status: Option<&str>) -> Result<Vec<GatewayClientRecord>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut query = "SELECT connection_id, role, user_id, node_id, scopes, caps, commands, client_info, status, connected_at, last_seen_at, disconnected_at FROM gateway_clients".to_string();
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        if let Some(status) = status
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            query.push_str(" WHERE status = $1");
            params.push(Box::new(status.to_string()));
        }
        query.push_str(" ORDER BY last_seen_at DESC");
        let params_refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p.as_ref()).collect();
        let rows = conn.query(&query, &params_refs)?;
        let mut output = Vec::new();
        for row in rows {
            let scopes: Option<String> = row.get(4);
            let caps: Option<String> = row.get(5);
            let commands: Option<String> = row.get(6);
            let client_info: Option<String> = row.get(7);
            output.push(GatewayClientRecord {
                connection_id: row.get(0),
                role: row.get(1),
                user_id: row.get(2),
                node_id: row.get(3),
                scopes: Self::parse_string_list(scopes),
                caps: Self::parse_string_list(caps),
                commands: Self::parse_string_list(commands),
                client_info: client_info.as_deref().and_then(Self::json_from_str),
                status: row.get(8),
                connected_at: row.get(9),
                last_seen_at: row.get(10),
                disconnected_at: row.get(11),
            });
        }
        Ok(output)
    }

    fn upsert_gateway_node(&self, record: &GatewayNodeRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let caps = if record.caps.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.caps))
        };
        let commands = if record.commands.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.commands))
        };
        let permissions = record.permissions.as_ref().map(Self::json_to_string);
        let metadata = record.metadata.as_ref().map(Self::json_to_string);
        conn.execute(
            "INSERT INTO gateway_nodes (node_id, name, device_fingerprint, status, caps, commands, permissions, metadata, created_at, updated_at, last_seen_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) \
             ON CONFLICT(node_id) DO UPDATE SET name = EXCLUDED.name, device_fingerprint = EXCLUDED.device_fingerprint, status = EXCLUDED.status, caps = EXCLUDED.caps, \
             commands = EXCLUDED.commands, permissions = EXCLUDED.permissions, metadata = EXCLUDED.metadata, updated_at = EXCLUDED.updated_at, last_seen_at = EXCLUDED.last_seen_at",
            &[
                &record.node_id,
                &record.name,
                &record.device_fingerprint,
                &record.status,
                &caps,
                &commands,
                &permissions,
                &metadata,
                &record.created_at,
                &record.updated_at,
                &record.last_seen_at,
            ],
        )?;
        Ok(())
    }

    fn get_gateway_node(&self, node_id: &str) -> Result<Option<GatewayNodeRecord>> {
        self.ensure_initialized()?;
        let cleaned = node_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT node_id, name, device_fingerprint, status, caps, commands, permissions, metadata, created_at, updated_at, last_seen_at FROM gateway_nodes WHERE node_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| {
            let caps: Option<String> = row.get(4);
            let commands: Option<String> = row.get(5);
            let permissions: Option<String> = row.get(6);
            let metadata: Option<String> = row.get(7);
            GatewayNodeRecord {
                node_id: row.get(0),
                name: row.get(1),
                device_fingerprint: row.get(2),
                status: row.get(3),
                caps: Self::parse_string_list(caps),
                commands: Self::parse_string_list(commands),
                permissions: permissions.as_deref().and_then(Self::json_from_str),
                metadata: metadata.as_deref().and_then(Self::json_from_str),
                created_at: row.get(8),
                updated_at: row.get(9),
                last_seen_at: row.get(10),
            }
        }))
    }

    fn list_gateway_nodes(&self, status: Option<&str>) -> Result<Vec<GatewayNodeRecord>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut query = "SELECT node_id, name, device_fingerprint, status, caps, commands, permissions, metadata, created_at, updated_at, last_seen_at FROM gateway_nodes".to_string();
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        if let Some(status) = status
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            query.push_str(" WHERE status = $1");
            params.push(Box::new(status.to_string()));
        }
        query.push_str(" ORDER BY updated_at DESC");
        let params_refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p.as_ref()).collect();
        let rows = conn.query(&query, &params_refs)?;
        let mut output = Vec::new();
        for row in rows {
            let caps: Option<String> = row.get(4);
            let commands: Option<String> = row.get(5);
            let permissions: Option<String> = row.get(6);
            let metadata: Option<String> = row.get(7);
            output.push(GatewayNodeRecord {
                node_id: row.get(0),
                name: row.get(1),
                device_fingerprint: row.get(2),
                status: row.get(3),
                caps: Self::parse_string_list(caps),
                commands: Self::parse_string_list(commands),
                permissions: permissions.as_deref().and_then(Self::json_from_str),
                metadata: metadata.as_deref().and_then(Self::json_from_str),
                created_at: row.get(8),
                updated_at: row.get(9),
                last_seen_at: row.get(10),
            });
        }
        Ok(output)
    }

    fn upsert_gateway_node_token(&self, record: &GatewayNodeTokenRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO gateway_node_tokens (token, node_id, status, created_at, updated_at, last_used_at) \
             VALUES ($1,$2,$3,$4,$5,$6) \
             ON CONFLICT(token) DO UPDATE SET node_id = EXCLUDED.node_id, status = EXCLUDED.status, updated_at = EXCLUDED.updated_at, last_used_at = EXCLUDED.last_used_at",
            &[
                &record.token,
                &record.node_id,
                &record.status,
                &record.created_at,
                &record.updated_at,
                &record.last_used_at,
            ],
        )?;
        Ok(())
    }

    fn get_gateway_node_token(&self, token: &str) -> Result<Option<GatewayNodeTokenRecord>> {
        self.ensure_initialized()?;
        let cleaned = token.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT token, node_id, status, created_at, updated_at, last_used_at FROM gateway_node_tokens WHERE token = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| GatewayNodeTokenRecord {
            token: row.get(0),
            node_id: row.get(1),
            status: row.get(2),
            created_at: row.get(3),
            updated_at: row.get(4),
            last_used_at: row.get(5),
        }))
    }

    fn list_gateway_node_tokens(
        &self,
        node_id: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<GatewayNodeTokenRecord>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut query =
            "SELECT token, node_id, status, created_at, updated_at, last_used_at FROM gateway_node_tokens"
                .to_string();
        let mut filters = Vec::new();
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        if let Some(node_id) = node_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            filters.push(format!("node_id = ${}", params.len() + 1));
            params.push(Box::new(node_id.to_string()));
        }
        if let Some(status) = status
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            filters.push(format!("status = ${}", params.len() + 1));
            params.push(Box::new(status.to_string()));
        }
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" ORDER BY updated_at DESC");
        let params_refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p.as_ref()).collect();
        let rows = conn.query(&query, &params_refs)?;
        let mut output = Vec::new();
        for row in rows {
            output.push(GatewayNodeTokenRecord {
                token: row.get(0),
                node_id: row.get(1),
                status: row.get(2),
                created_at: row.get(3),
                updated_at: row.get(4),
                last_used_at: row.get(5),
            });
        }
        Ok(output)
    }

    fn delete_gateway_node_token(&self, token: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = token.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM gateway_node_tokens WHERE token = $1",
            &[&cleaned],
        )?;
        Ok(affected as i64)
    }

    fn upsert_media_asset(&self, record: &MediaAssetRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO media_assets (asset_id, kind, url, mime, size, hash, source, created_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8) \
             ON CONFLICT(asset_id) DO UPDATE SET kind = EXCLUDED.kind, url = EXCLUDED.url, mime = EXCLUDED.mime, size = EXCLUDED.size, hash = EXCLUDED.hash, source = EXCLUDED.source",
            &[
                &record.asset_id,
                &record.kind,
                &record.url,
                &record.mime,
                &record.size,
                &record.hash,
                &record.source,
                &record.created_at,
            ],
        )?;
        Ok(())
    }

    fn get_media_asset(&self, asset_id: &str) -> Result<Option<MediaAssetRecord>> {
        self.ensure_initialized()?;
        let cleaned = asset_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT asset_id, kind, url, mime, size, hash, source, created_at FROM media_assets WHERE asset_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| MediaAssetRecord {
            asset_id: row.get(0),
            kind: row.get(1),
            url: row.get(2),
            mime: row.get(3),
            size: row.get(4),
            hash: row.get(5),
            source: row.get(6),
            created_at: row.get(7),
        }))
    }

    fn get_media_asset_by_hash(&self, hash: &str) -> Result<Option<MediaAssetRecord>> {
        self.ensure_initialized()?;
        let cleaned = hash.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT asset_id, kind, url, mime, size, hash, source, created_at FROM media_assets WHERE hash = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| MediaAssetRecord {
            asset_id: row.get(0),
            kind: row.get(1),
            url: row.get(2),
            mime: row.get(3),
            size: row.get(4),
            hash: row.get(5),
            source: row.get(6),
            created_at: row.get(7),
        }))
    }

    fn upsert_speech_job(&self, record: &SpeechJobRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let metadata = record.metadata.as_ref().map(Self::json_to_string);
        conn.execute(
            "INSERT INTO speech_jobs (job_id, job_type, status, input_text, input_url, output_text, output_url, model, error, retry_count, next_retry_at, created_at, updated_at, metadata) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) \
             ON CONFLICT(job_id) DO UPDATE SET status = EXCLUDED.status, input_text = EXCLUDED.input_text, input_url = EXCLUDED.input_url, output_text = EXCLUDED.output_text, \
             output_url = EXCLUDED.output_url, model = EXCLUDED.model, error = EXCLUDED.error, retry_count = EXCLUDED.retry_count, next_retry_at = EXCLUDED.next_retry_at, \
             updated_at = EXCLUDED.updated_at, metadata = EXCLUDED.metadata",
            &[
                &record.job_id,
                &record.job_type,
                &record.status,
                &record.input_text,
                &record.input_url,
                &record.output_text,
                &record.output_url,
                &record.model,
                &record.error,
                &record.retry_count,
                &record.next_retry_at,
                &record.created_at,
                &record.updated_at,
                &metadata,
            ],
        )?;
        Ok(())
    }

    fn list_pending_speech_jobs(&self, job_type: &str, limit: i64) -> Result<Vec<SpeechJobRecord>> {
        self.ensure_initialized()?;
        let cleaned = job_type.trim();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let now = Self::now_ts();
        let limit_value = if limit <= 0 { 50 } else { limit.min(200) };
        let rows = conn.query(
            "SELECT job_id, job_type, status, input_text, input_url, output_text, output_url, model, error, retry_count, next_retry_at, created_at, updated_at, metadata \
             FROM speech_jobs WHERE job_type = $1 AND (status = 'queued' OR status = 'retry') AND next_retry_at <= $2 ORDER BY next_retry_at ASC LIMIT $3",
            &[&cleaned, &now, &limit_value],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(SpeechJobRecord {
                job_id: row.get(0),
                job_type: row.get(1),
                status: row.get(2),
                input_text: row.get(3),
                input_url: row.get(4),
                output_text: row.get(5),
                output_url: row.get(6),
                model: row.get(7),
                error: row.get(8),
                retry_count: row.get(9),
                next_retry_at: row.get::<_, Option<f64>>(10).unwrap_or(0.0),
                created_at: row.get::<_, Option<f64>>(11).unwrap_or(0.0),
                updated_at: row.get::<_, Option<f64>>(12).unwrap_or(0.0),
                metadata: row
                    .get::<_, Option<String>>(13)
                    .and_then(|value| Self::json_from_str(&value)),
            });
        }
        Ok(output)
    }

    fn upsert_session_run(&self, record: &SessionRunRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO session_runs (run_id, session_id, parent_session_id, user_id, agent_id, model_name, status, queued_time, \
             started_time, finished_time, elapsed_s, result, error, updated_time) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14) \
             ON CONFLICT(run_id) DO UPDATE SET session_id = EXCLUDED.session_id, parent_session_id = EXCLUDED.parent_session_id, \
             user_id = EXCLUDED.user_id, agent_id = EXCLUDED.agent_id, model_name = EXCLUDED.model_name, status = EXCLUDED.status, \
             queued_time = EXCLUDED.queued_time, started_time = EXCLUDED.started_time, finished_time = EXCLUDED.finished_time, \
             elapsed_s = EXCLUDED.elapsed_s, result = EXCLUDED.result, error = EXCLUDED.error, updated_time = EXCLUDED.updated_time",
            &[
                &record.run_id,
                &record.session_id,
                &record.parent_session_id,
                &record.user_id,
                &record.agent_id,
                &record.model_name,
                &record.status,
                &record.queued_time,
                &record.started_time,
                &record.finished_time,
                &record.elapsed_s,
                &record.result,
                &record.error,
                &record.updated_time,
            ],
        )?;
        Ok(())
    }

    fn get_session_run(&self, run_id: &str) -> Result<Option<SessionRunRecord>> {
        self.ensure_initialized()?;
        let cleaned = run_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT run_id, session_id, parent_session_id, user_id, agent_id, model_name, status, queued_time, started_time, \
             finished_time, elapsed_s, result, error, updated_time FROM session_runs WHERE run_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| SessionRunRecord {
            run_id: row.get(0),
            session_id: row.get(1),
            parent_session_id: row.get(2),
            user_id: row.get(3),
            agent_id: row.get(4),
            model_name: row.get(5),
            status: row.get(6),
            queued_time: row.get::<_, Option<f64>>(7).unwrap_or(0.0),
            started_time: row.get::<_, Option<f64>>(8).unwrap_or(0.0),
            finished_time: row.get::<_, Option<f64>>(9).unwrap_or(0.0),
            elapsed_s: row.get::<_, Option<f64>>(10).unwrap_or(0.0),
            result: row.get(11),
            error: row.get(12),
            updated_time: row.get::<_, Option<f64>>(13).unwrap_or(0.0),
        }))
    }

    fn upsert_cron_job(&self, record: &CronJobRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let payload = Self::json_to_string(&record.payload);
        let deliver = record.deliver.as_ref().map(Self::json_to_string);
        let enabled = if record.enabled { 1 } else { 0 };
        let delete_after = if record.delete_after_run { 1 } else { 0 };
        conn.execute(
            "INSERT INTO cron_jobs (job_id, user_id, session_id, agent_id, name, session_target, payload, deliver, enabled, delete_after_run, \
             schedule_kind, schedule_at, schedule_every_ms, schedule_cron, schedule_tz, dedupe_key, next_run_at, running_at, last_run_at, \
             last_status, last_error, created_at, updated_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23) \
             ON CONFLICT(job_id) DO UPDATE SET user_id = EXCLUDED.user_id, session_id = EXCLUDED.session_id, agent_id = EXCLUDED.agent_id, \
             name = EXCLUDED.name, session_target = EXCLUDED.session_target, payload = EXCLUDED.payload, deliver = EXCLUDED.deliver, \
             enabled = EXCLUDED.enabled, delete_after_run = EXCLUDED.delete_after_run, schedule_kind = EXCLUDED.schedule_kind, \
             schedule_at = EXCLUDED.schedule_at, schedule_every_ms = EXCLUDED.schedule_every_ms, schedule_cron = EXCLUDED.schedule_cron, \
             schedule_tz = EXCLUDED.schedule_tz, dedupe_key = EXCLUDED.dedupe_key, next_run_at = EXCLUDED.next_run_at, \
             running_at = EXCLUDED.running_at, last_run_at = EXCLUDED.last_run_at, last_status = EXCLUDED.last_status, \
             last_error = EXCLUDED.last_error, updated_at = EXCLUDED.updated_at",
            &[
                &record.job_id,
                &record.user_id,
                &record.session_id,
                &record.agent_id,
                &record.name,
                &record.session_target,
                &payload,
                &deliver,
                &enabled,
                &delete_after,
                &record.schedule_kind,
                &record.schedule_at,
                &record.schedule_every_ms,
                &record.schedule_cron,
                &record.schedule_tz,
                &record.dedupe_key,
                &record.next_run_at,
                &record.running_at,
                &record.last_run_at,
                &record.last_status,
                &record.last_error,
                &record.created_at,
                &record.updated_at,
            ],
        )?;
        Ok(())
    }

    fn get_cron_job(&self, user_id: &str, job_id: &str) -> Result<Option<CronJobRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_job = job_id.trim();
        if cleaned_user.is_empty() || cleaned_job.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT job_id, user_id, session_id, agent_id, name, session_target, payload, deliver, enabled, delete_after_run, \
             schedule_kind, schedule_at, schedule_every_ms, schedule_cron, schedule_tz, dedupe_key, next_run_at, running_at, \
             last_run_at, last_status, last_error, created_at, updated_at FROM cron_jobs WHERE user_id = $1 AND job_id = $2",
            &[&cleaned_user, &cleaned_job],
        )?;
        Ok(row.map(|row| {
            let payload_text: Option<String> = row.get(6);
            let deliver_text: Option<String> = row.get(7);
            let enabled: Option<i32> = row.get(8);
            let delete_after: Option<i32> = row.get(9);
            CronJobRecord {
                job_id: row.get(0),
                user_id: row.get(1),
                session_id: row.get(2),
                agent_id: row.get(3),
                name: row.get(4),
                session_target: row.get(5),
                payload: Self::json_value_or_null(payload_text),
                deliver: deliver_text.and_then(|value| Self::json_from_str(&value)),
                enabled: enabled.unwrap_or(0) != 0,
                delete_after_run: delete_after.unwrap_or(0) != 0,
                schedule_kind: row.get(10),
                schedule_at: row.get(11),
                schedule_every_ms: row.get(12),
                schedule_cron: row.get(13),
                schedule_tz: row.get(14),
                dedupe_key: row.get(15),
                next_run_at: row.get(16),
                running_at: row.get(17),
                last_run_at: row.get(18),
                last_status: row.get(19),
                last_error: row.get(20),
                created_at: row.get(21),
                updated_at: row.get(22),
            }
        }))
    }

    fn get_cron_job_by_dedupe_key(
        &self,
        user_id: &str,
        dedupe_key: &str,
    ) -> Result<Option<CronJobRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_key = dedupe_key.trim();
        if cleaned_user.is_empty() || cleaned_key.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT job_id, user_id, session_id, agent_id, name, session_target, payload, deliver, enabled, delete_after_run, \
             schedule_kind, schedule_at, schedule_every_ms, schedule_cron, schedule_tz, dedupe_key, next_run_at, running_at, \
             last_run_at, last_status, last_error, created_at, updated_at FROM cron_jobs WHERE user_id = $1 AND dedupe_key = $2 LIMIT 1",
            &[&cleaned_user, &cleaned_key],
        )?;
        Ok(row.map(|row| {
            let payload_text: Option<String> = row.get(6);
            let deliver_text: Option<String> = row.get(7);
            let enabled: Option<i32> = row.get(8);
            let delete_after: Option<i32> = row.get(9);
            CronJobRecord {
                job_id: row.get(0),
                user_id: row.get(1),
                session_id: row.get(2),
                agent_id: row.get(3),
                name: row.get(4),
                session_target: row.get(5),
                payload: Self::json_value_or_null(payload_text),
                deliver: deliver_text.and_then(|value| Self::json_from_str(&value)),
                enabled: enabled.unwrap_or(0) != 0,
                delete_after_run: delete_after.unwrap_or(0) != 0,
                schedule_kind: row.get(10),
                schedule_at: row.get(11),
                schedule_every_ms: row.get(12),
                schedule_cron: row.get(13),
                schedule_tz: row.get(14),
                dedupe_key: row.get(15),
                next_run_at: row.get(16),
                running_at: row.get(17),
                last_run_at: row.get(18),
                last_status: row.get(19),
                last_error: row.get(20),
                created_at: row.get(21),
                updated_at: row.get(22),
            }
        }))
    }

    fn list_cron_jobs(&self, user_id: &str, include_disabled: bool) -> Result<Vec<CronJobRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let mut sql = String::from(
            "SELECT job_id, user_id, session_id, agent_id, name, session_target, payload, deliver, enabled, delete_after_run, \
             schedule_kind, schedule_at, schedule_every_ms, schedule_cron, schedule_tz, dedupe_key, next_run_at, running_at, \
             last_run_at, last_status, last_error, created_at, updated_at FROM cron_jobs WHERE user_id = $1",
        );
        if !include_disabled {
            sql.push_str(" AND enabled = 1");
        }
        sql.push_str(" ORDER BY updated_at DESC");
        let rows = conn.query(&sql, &[&cleaned_user])?;
        let mut output = Vec::new();
        for row in rows {
            let payload_text: Option<String> = row.get(6);
            let deliver_text: Option<String> = row.get(7);
            let enabled: Option<i32> = row.get(8);
            let delete_after: Option<i32> = row.get(9);
            output.push(CronJobRecord {
                job_id: row.get(0),
                user_id: row.get(1),
                session_id: row.get(2),
                agent_id: row.get(3),
                name: row.get(4),
                session_target: row.get(5),
                payload: Self::json_value_or_null(payload_text),
                deliver: deliver_text.and_then(|value| Self::json_from_str(&value)),
                enabled: enabled.unwrap_or(0) != 0,
                delete_after_run: delete_after.unwrap_or(0) != 0,
                schedule_kind: row.get(10),
                schedule_at: row.get(11),
                schedule_every_ms: row.get(12),
                schedule_cron: row.get(13),
                schedule_tz: row.get(14),
                dedupe_key: row.get(15),
                next_run_at: row.get(16),
                running_at: row.get(17),
                last_run_at: row.get(18),
                last_status: row.get(19),
                last_error: row.get(20),
                created_at: row.get(21),
                updated_at: row.get(22),
            });
        }
        Ok(output)
    }

    fn delete_cron_job(&self, user_id: &str, job_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_job = job_id.trim();
        if cleaned_user.is_empty() || cleaned_job.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM cron_jobs WHERE user_id = $1 AND job_id = $2",
            &[&cleaned_user, &cleaned_job],
        )?;
        Ok(affected as i64)
    }

    fn delete_cron_jobs_by_session(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM cron_jobs WHERE user_id = $1 AND session_id = $2",
            &[&cleaned_user, &cleaned_session],
        )?;
        Ok(affected as i64)
    }

    fn reset_cron_jobs_running(&self) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute(
            "UPDATE cron_jobs SET running_at = NULL WHERE running_at IS NOT NULL",
            &[],
        )?;
        Ok(())
    }

    fn count_running_cron_jobs(&self) -> Result<i64> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let total: i64 = conn
            .query_one(
                "SELECT COUNT(*) FROM cron_jobs WHERE running_at IS NOT NULL",
                &[],
            )?
            .get(0);
        Ok(total)
    }

    fn claim_due_cron_jobs(&self, now: f64, limit: i64) -> Result<Vec<CronJobRecord>> {
        self.ensure_initialized()?;
        let limit = limit.max(0);
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        let rows = tx.query(
            "SELECT job_id FROM cron_jobs WHERE enabled = 1 AND next_run_at IS NOT NULL AND next_run_at <= $1 \
             AND (running_at IS NULL) ORDER BY next_run_at ASC LIMIT $2 FOR UPDATE SKIP LOCKED",
            &[&now, &limit],
        )?;
        let ids = rows
            .iter()
            .map(|row| row.get::<_, String>(0))
            .collect::<Vec<_>>();
        if ids.is_empty() {
            tx.commit()?;
            return Ok(Vec::new());
        }
        for id in &ids {
            tx.execute(
                "UPDATE cron_jobs SET running_at = $1, updated_at = $2 WHERE job_id = $3",
                &[&now, &now, id],
            )?;
        }
        let rows = tx.query(
            "SELECT job_id, user_id, session_id, agent_id, name, session_target, payload, deliver, enabled, delete_after_run, \
             schedule_kind, schedule_at, schedule_every_ms, schedule_cron, schedule_tz, dedupe_key, next_run_at, running_at, \
             last_run_at, last_status, last_error, created_at, updated_at FROM cron_jobs WHERE job_id = ANY($1)",
            &[&ids],
        )?;
        let mut output = Vec::new();
        for row in rows {
            let payload_text: Option<String> = row.get(6);
            let deliver_text: Option<String> = row.get(7);
            let enabled: Option<i32> = row.get(8);
            let delete_after: Option<i32> = row.get(9);
            output.push(CronJobRecord {
                job_id: row.get(0),
                user_id: row.get(1),
                session_id: row.get(2),
                agent_id: row.get(3),
                name: row.get(4),
                session_target: row.get(5),
                payload: Self::json_value_or_null(payload_text),
                deliver: deliver_text.and_then(|value| Self::json_from_str(&value)),
                enabled: enabled.unwrap_or(0) != 0,
                delete_after_run: delete_after.unwrap_or(0) != 0,
                schedule_kind: row.get(10),
                schedule_at: row.get(11),
                schedule_every_ms: row.get(12),
                schedule_cron: row.get(13),
                schedule_tz: row.get(14),
                dedupe_key: row.get(15),
                next_run_at: row.get(16),
                running_at: row.get(17),
                last_run_at: row.get(18),
                last_status: row.get(19),
                last_error: row.get(20),
                created_at: row.get(21),
                updated_at: row.get(22),
            });
        }
        tx.commit()?;
        Ok(output)
    }

    fn insert_cron_run(&self, record: &CronRunRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO cron_runs (run_id, job_id, user_id, session_id, agent_id, trigger, status, summary, error, duration_ms, created_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
            &[
                &record.run_id,
                &record.job_id,
                &record.user_id,
                &record.session_id,
                &record.agent_id,
                &record.trigger,
                &record.status,
                &record.summary,
                &record.error,
                &record.duration_ms,
                &record.created_at,
            ],
        )?;
        Ok(())
    }

    fn list_cron_runs(
        &self,
        user_id: &str,
        job_id: &str,
        limit: i64,
    ) -> Result<Vec<CronRunRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_job = job_id.trim();
        if cleaned_user.is_empty() || cleaned_job.is_empty() {
            return Ok(Vec::new());
        }
        let safe_limit = limit.clamp(1, 200);
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT run_id, job_id, user_id, session_id, agent_id, trigger, status, summary, error, duration_ms, created_at \
             FROM cron_runs WHERE user_id = $1 AND job_id = $2 ORDER BY created_at DESC LIMIT $3",
            &[&cleaned_user, &cleaned_job, &safe_limit],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(CronRunRecord {
                run_id: row.get(0),
                job_id: row.get(1),
                user_id: row.get(2),
                session_id: row.get(3),
                agent_id: row.get(4),
                trigger: row.get(5),
                status: row.get(6),
                summary: row.get(7),
                error: row.get(8),
                duration_ms: row.get::<_, Option<i64>>(9).unwrap_or(0),
                created_at: row.get::<_, Option<f64>>(10).unwrap_or(0.0),
            });
        }
        Ok(output)
    }

    fn get_next_cron_run_at(&self, now: f64) -> Result<Option<f64>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT MIN(next_run_at) FROM cron_jobs WHERE enabled = 1 AND next_run_at IS NOT NULL AND next_run_at > $1",
            &[&now],
        )?;
        Ok(row.and_then(|row| row.get::<_, Option<f64>>(0)))
    }

    fn get_user_tool_access(&self, user_id: &str) -> Result<Option<UserToolAccessRecord>> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT allowed_tools, updated_at FROM user_tool_access WHERE user_id = $1",
            &[&cleaned],
        )?;
        let Some(row) = row else {
            return Ok(None);
        };
        let allowed: Option<String> = row.get(0);
        let updated_at: f64 = row.get(1);
        Ok(Some(UserToolAccessRecord {
            user_id: cleaned.to_string(),
            allowed_tools: allowed.map(|value| Self::parse_string_list(Some(value))),
            updated_at,
        }))
    }

    fn set_user_tool_access(
        &self,
        user_id: &str,
        allowed_tools: Option<&Vec<String>>,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        if allowed_tools.is_some() {
            let payload = allowed_tools
                .map(|value| Self::string_list_to_json(value))
                .unwrap_or_else(|| "[]".to_string());
            let now = Self::now_ts();
            conn.execute(
                "INSERT INTO user_tool_access (user_id, allowed_tools, updated_at) VALUES ($1, $2, $3) \
                 ON CONFLICT(user_id) DO UPDATE SET allowed_tools = EXCLUDED.allowed_tools, updated_at = EXCLUDED.updated_at",
                &[&cleaned, &payload, &now],
            )?;
        } else {
            conn.execute(
                "DELETE FROM user_tool_access WHERE user_id = $1",
                &[&cleaned],
            )?;
        }
        Ok(())
    }

    fn get_user_agent_access(&self, user_id: &str) -> Result<Option<UserAgentAccessRecord>> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT allowed_agent_ids, blocked_agent_ids, updated_at FROM user_agent_access WHERE user_id = $1",
            &[&cleaned],
        )?;
        let Some(row) = row else {
            return Ok(None);
        };
        let allowed: Option<String> = row.get(0);
        let blocked: Option<String> = row.get(1);
        let updated_at: f64 = row.get(2);
        Ok(Some(UserAgentAccessRecord {
            user_id: cleaned.to_string(),
            allowed_agent_ids: allowed.map(|value| Self::parse_string_list(Some(value))),
            blocked_agent_ids: Self::parse_string_list(blocked),
            updated_at,
        }))
    }

    fn set_user_agent_access(
        &self,
        user_id: &str,
        allowed_agent_ids: Option<&Vec<String>>,
        blocked_agent_ids: Option<&Vec<String>>,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        if allowed_agent_ids.is_some() || blocked_agent_ids.is_some() {
            let allowed_payload = allowed_agent_ids
                .map(|value| Self::string_list_to_json(value))
                .unwrap_or_else(|| "[]".to_string());
            let blocked_payload = blocked_agent_ids
                .map(|value| Self::string_list_to_json(value))
                .unwrap_or_else(|| "[]".to_string());
            let now = Self::now_ts();
            conn.execute(
                "INSERT INTO user_agent_access (user_id, allowed_agent_ids, blocked_agent_ids, updated_at) VALUES ($1, $2, $3, $4) \
                 ON CONFLICT(user_id) DO UPDATE SET allowed_agent_ids = EXCLUDED.allowed_agent_ids, blocked_agent_ids = EXCLUDED.blocked_agent_ids, updated_at = EXCLUDED.updated_at",
                &[&cleaned, &allowed_payload, &blocked_payload, &now],
            )?;
        } else {
            conn.execute(
                "DELETE FROM user_agent_access WHERE user_id = $1",
                &[&cleaned],
            )?;
        }
        Ok(())
    }

    fn upsert_user_agent(&self, record: &UserAgentRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let tool_names = if record.tool_names.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.tool_names))
        };
        let is_shared = if record.is_shared { 1 } else { 0 };
        let sandbox_container_id = normalize_sandbox_container_id(record.sandbox_container_id);
        let hive_id = normalize_hive_id(&record.hive_id);
        conn.execute(
            "INSERT INTO user_agents (agent_id, user_id, hive_id, name, description, system_prompt, tool_names, access_level, is_shared, status, icon, sandbox_container_id, created_at, updated_at)              VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)              ON CONFLICT(agent_id) DO UPDATE SET user_id = EXCLUDED.user_id, hive_id = EXCLUDED.hive_id, name = EXCLUDED.name, description = EXCLUDED.description,              system_prompt = EXCLUDED.system_prompt, tool_names = EXCLUDED.tool_names, access_level = EXCLUDED.access_level,              is_shared = EXCLUDED.is_shared, status = EXCLUDED.status, icon = EXCLUDED.icon, sandbox_container_id = EXCLUDED.sandbox_container_id, updated_at = EXCLUDED.updated_at",
            &[
                &record.agent_id,
                &record.user_id,
                &hive_id,
                &record.name,
                &record.description,
                &record.system_prompt,
                &tool_names,
                &record.access_level,
                &is_shared,
                &record.status,
                &record.icon,
                &sandbox_container_id,
                &record.created_at,
                &record.updated_at,
            ],
        )?;
        Ok(())
    }

    fn get_user_agent(&self, user_id: &str, agent_id: &str) -> Result<Option<UserAgentRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_agent = agent_id.trim();
        if cleaned_user.is_empty() || cleaned_agent.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, tool_names, access_level, is_shared, status, icon, sandbox_container_id, created_at, updated_at              FROM user_agents WHERE user_id = $1 AND agent_id = $2",
            &[&cleaned_user, &cleaned_agent],
        )?;
        Ok(row.map(|row| UserAgentRecord {
            agent_id: row.get(0),
            user_id: row.get(1),
            hive_id: normalize_hive_id(&row.get::<_, String>(2)),
            name: row.get(3),
            description: row.get(4),
            system_prompt: row.get(5),
            tool_names: Self::parse_string_list(row.get(6)),
            access_level: row.get(7),
            is_shared: row.get::<_, i32>(8) != 0,
            status: row.get(9),
            icon: row.get(10),
            sandbox_container_id: normalize_sandbox_container_id(row.get::<_, i32>(11)),
            created_at: row.get(12),
            updated_at: row.get(13),
        }))
    }

    fn get_user_agent_by_id(&self, agent_id: &str) -> Result<Option<UserAgentRecord>> {
        self.ensure_initialized()?;
        let cleaned_agent = agent_id.trim();
        if cleaned_agent.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, tool_names, access_level, is_shared, status, icon, sandbox_container_id, created_at, updated_at              FROM user_agents WHERE agent_id = $1",
            &[&cleaned_agent],
        )?;
        Ok(row.map(|row| UserAgentRecord {
            agent_id: row.get(0),
            user_id: row.get(1),
            hive_id: normalize_hive_id(&row.get::<_, String>(2)),
            name: row.get(3),
            description: row.get(4),
            system_prompt: row.get(5),
            tool_names: Self::parse_string_list(row.get(6)),
            access_level: row.get(7),
            is_shared: row.get::<_, i32>(8) != 0,
            status: row.get(9),
            icon: row.get(10),
            sandbox_container_id: normalize_sandbox_container_id(row.get::<_, i32>(11)),
            created_at: row.get(12),
            updated_at: row.get(13),
        }))
    }

    fn list_user_agents(&self, user_id: &str) -> Result<Vec<UserAgentRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, tool_names, access_level, is_shared, status, icon, sandbox_container_id, created_at, updated_at              FROM user_agents WHERE user_id = $1 ORDER BY updated_at DESC",
            &[&cleaned_user],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(UserAgentRecord {
                agent_id: row.get(0),
                user_id: row.get(1),
                hive_id: normalize_hive_id(&row.get::<_, String>(2)),
                name: row.get(3),
                description: row.get(4),
                system_prompt: row.get(5),
                tool_names: Self::parse_string_list(row.get(6)),
                access_level: row.get(7),
                is_shared: row.get::<_, i32>(8) != 0,
                status: row.get(9),
                icon: row.get(10),
                sandbox_container_id: normalize_sandbox_container_id(row.get::<_, i32>(11)),
                created_at: row.get(12),
                updated_at: row.get(13),
            });
        }
        Ok(output)
    }

    fn list_user_agents_by_hive(
        &self,
        user_id: &str,
        hive_id: &str,
    ) -> Result<Vec<UserAgentRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let normalized_hive_id = normalize_hive_id(hive_id);
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, tool_names, access_level, is_shared, status, icon, sandbox_container_id, created_at, updated_at              FROM user_agents WHERE user_id = $1 AND hive_id = $2 ORDER BY updated_at DESC",
            &[&cleaned_user, &normalized_hive_id],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(UserAgentRecord {
                agent_id: row.get(0),
                user_id: row.get(1),
                hive_id: normalize_hive_id(&row.get::<_, String>(2)),
                name: row.get(3),
                description: row.get(4),
                system_prompt: row.get(5),
                tool_names: Self::parse_string_list(row.get(6)),
                access_level: row.get(7),
                is_shared: row.get::<_, i32>(8) != 0,
                status: row.get(9),
                icon: row.get(10),
                sandbox_container_id: normalize_sandbox_container_id(row.get::<_, i32>(11)),
                created_at: row.get(12),
                updated_at: row.get(13),
            });
        }
        Ok(output)
    }

    fn list_shared_user_agents(&self, user_id: &str) -> Result<Vec<UserAgentRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, tool_names, access_level, is_shared, status, icon, sandbox_container_id, created_at, updated_at              FROM user_agents WHERE is_shared = 1 AND user_id <> $1 ORDER BY updated_at DESC",
            &[&cleaned_user],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(UserAgentRecord {
                agent_id: row.get(0),
                user_id: row.get(1),
                hive_id: normalize_hive_id(&row.get::<_, String>(2)),
                name: row.get(3),
                description: row.get(4),
                system_prompt: row.get(5),
                tool_names: Self::parse_string_list(row.get(6)),
                access_level: row.get(7),
                is_shared: row.get::<_, i32>(8) != 0,
                status: row.get(9),
                icon: row.get(10),
                sandbox_container_id: normalize_sandbox_container_id(row.get::<_, i32>(11)),
                created_at: row.get(12),
                updated_at: row.get(13),
            });
        }
        Ok(output)
    }

    fn delete_user_agent(&self, user_id: &str, agent_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_agent = agent_id.trim();
        if cleaned_user.is_empty() || cleaned_agent.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM user_agents WHERE user_id = $1 AND agent_id = $2",
            &[&cleaned_user, &cleaned_agent],
        )?;
        Ok(affected as i64)
    }

    fn upsert_hive(&self, record: &HiveRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let hive_id = normalize_hive_id(&record.hive_id);
        let is_default = if record.is_default { 1 } else { 0 };
        conn.execute(
            "INSERT INTO hives (hive_id, user_id, name, description, is_default, status, created_time, updated_time)              VALUES ($1,$2,$3,$4,$5,$6,$7,$8)              ON CONFLICT(hive_id) DO UPDATE SET user_id = EXCLUDED.user_id, name = EXCLUDED.name, description = EXCLUDED.description,              is_default = EXCLUDED.is_default, status = EXCLUDED.status, updated_time = EXCLUDED.updated_time",
            &[
                &hive_id,
                &record.user_id,
                &record.name,
                &record.description,
                &is_default,
                &record.status,
                &record.created_time,
                &record.updated_time,
            ],
        )?;
        Ok(())
    }

    fn get_hive(&self, user_id: &str, hive_id: &str) -> Result<Option<HiveRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(None);
        }
        let normalized_hive_id = normalize_hive_id(hive_id);
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT hive_id, user_id, name, description, is_default, status, created_time, updated_time FROM hives WHERE user_id = $1 AND hive_id = $2",
            &[&cleaned_user, &normalized_hive_id],
        )?;
        Ok(row.map(|row| HiveRecord {
            hive_id: normalize_hive_id(&row.get::<_, String>(0)),
            user_id: row.get(1),
            name: row.get(2),
            description: row.get(3),
            is_default: row.get::<_, i32>(4) != 0,
            status: row.get(5),
            created_time: row.get(6),
            updated_time: row.get(7),
        }))
    }

    fn list_hives(&self, user_id: &str, include_archived: bool) -> Result<Vec<HiveRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let sql = if include_archived {
            "SELECT hive_id, user_id, name, description, is_default, status, created_time, updated_time FROM hives WHERE user_id = $1 ORDER BY is_default DESC, updated_time DESC"
        } else {
            "SELECT hive_id, user_id, name, description, is_default, status, created_time, updated_time FROM hives WHERE user_id = $1 AND status <> 'archived' ORDER BY is_default DESC, updated_time DESC"
        };
        let rows = conn.query(sql, &[&cleaned_user])?;
        let mut output = Vec::new();
        for row in rows {
            output.push(HiveRecord {
                hive_id: normalize_hive_id(&row.get::<_, String>(0)),
                user_id: row.get(1),
                name: row.get(2),
                description: row.get(3),
                is_default: row.get::<_, i32>(4) != 0,
                status: row.get(5),
                created_time: row.get(6),
                updated_time: row.get(7),
            });
        }
        Ok(output)
    }

    fn move_agents_to_hive(
        &self,
        user_id: &str,
        hive_id: &str,
        agent_ids: &[String],
    ) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() || agent_ids.is_empty() {
            return Ok(0);
        }
        let mut cleaned_ids = Vec::new();
        for agent_id in agent_ids {
            let cleaned = agent_id.trim();
            if !cleaned.is_empty() {
                cleaned_ids.push(cleaned.to_string());
            }
        }
        if cleaned_ids.is_empty() {
            return Ok(0);
        }
        let normalized_hive_id = normalize_hive_id(hive_id);
        let now = Self::now_ts();
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "UPDATE user_agents SET hive_id = $1, updated_at = $2 WHERE user_id = $3 AND agent_id = ANY($4)",
            &[&normalized_hive_id, &now, &cleaned_user, &cleaned_ids],
        )?;
        Ok(affected as i64)
    }

    fn upsert_team_run(&self, record: &TeamRunRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let hive_id = normalize_hive_id(&record.hive_id);
        conn.execute(
            "INSERT INTO team_runs (team_run_id, user_id, hive_id, parent_session_id, parent_agent_id, strategy, status, task_total, task_success, task_failed, context_tokens_total, context_tokens_peak, model_round_total, started_time, finished_time, elapsed_s, summary, error, updated_time)              VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19)              ON CONFLICT(team_run_id) DO UPDATE SET user_id = EXCLUDED.user_id, hive_id = EXCLUDED.hive_id, parent_session_id = EXCLUDED.parent_session_id, parent_agent_id = EXCLUDED.parent_agent_id,              strategy = EXCLUDED.strategy, status = EXCLUDED.status, task_total = EXCLUDED.task_total, task_success = EXCLUDED.task_success, task_failed = EXCLUDED.task_failed,              context_tokens_total = EXCLUDED.context_tokens_total, context_tokens_peak = EXCLUDED.context_tokens_peak, model_round_total = EXCLUDED.model_round_total,              started_time = EXCLUDED.started_time, finished_time = EXCLUDED.finished_time, elapsed_s = EXCLUDED.elapsed_s, summary = EXCLUDED.summary, error = EXCLUDED.error, updated_time = EXCLUDED.updated_time",
            &[
                &record.team_run_id,
                &record.user_id,
                &hive_id,
                &record.parent_session_id,
                &record.parent_agent_id,
                &record.strategy,
                &record.status,
                &record.task_total,
                &record.task_success,
                &record.task_failed,
                &record.context_tokens_total,
                &record.context_tokens_peak,
                &record.model_round_total,
                &record.started_time,
                &record.finished_time,
                &record.elapsed_s,
                &record.summary,
                &record.error,
                &record.updated_time,
            ],
        )?;
        Ok(())
    }

    fn get_team_run(&self, team_run_id: &str) -> Result<Option<TeamRunRecord>> {
        self.ensure_initialized()?;
        let cleaned = team_run_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT team_run_id, user_id, hive_id, parent_session_id, parent_agent_id, strategy, status, task_total, task_success, task_failed, context_tokens_total, context_tokens_peak, model_round_total, started_time, finished_time, elapsed_s, summary, error, updated_time FROM team_runs WHERE team_run_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| TeamRunRecord {
            team_run_id: row.get(0),
            user_id: row.get(1),
            hive_id: normalize_hive_id(&row.get::<_, String>(2)),
            parent_session_id: row.get(3),
            parent_agent_id: row.get(4),
            strategy: row.get(5),
            status: row.get(6),
            task_total: row.get(7),
            task_success: row.get(8),
            task_failed: row.get(9),
            context_tokens_total: row.get(10),
            context_tokens_peak: row.get(11),
            model_round_total: row.get(12),
            started_time: row.get(13),
            finished_time: row.get(14),
            elapsed_s: row.get(15),
            summary: row.get(16),
            error: row.get(17),
            updated_time: row.get(18),
        }))
    }

    fn list_team_runs(
        &self,
        user_id: &str,
        hive_id: Option<&str>,
        parent_session_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<TeamRunRecord>, i64)> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok((Vec::new(), 0));
        }
        let hive_filter = hive_id.map(normalize_hive_id).unwrap_or_default();
        let parent_filter = parent_session_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .unwrap_or_default();
        let safe_limit = limit.max(1);
        let safe_offset = offset.max(0);
        let mut conn = self.conn()?;
        let total: i64 = conn
            .query_one(
                "SELECT COUNT(1) FROM team_runs WHERE user_id = $1 AND ($2 = '' OR hive_id = $2) AND ($3 = '' OR parent_session_id = $3)",
                &[&cleaned_user, &hive_filter, &parent_filter],
            )?
            .get(0);
        let rows = conn.query(
            "SELECT team_run_id, user_id, hive_id, parent_session_id, parent_agent_id, strategy, status, task_total, task_success, task_failed, context_tokens_total, context_tokens_peak, model_round_total, started_time, finished_time, elapsed_s, summary, error, updated_time              FROM team_runs WHERE user_id = $1 AND ($2 = '' OR hive_id = $2) AND ($3 = '' OR parent_session_id = $3)              ORDER BY updated_time DESC LIMIT $4 OFFSET $5",
            &[&cleaned_user, &hive_filter, &parent_filter, &safe_limit, &safe_offset],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(TeamRunRecord {
                team_run_id: row.get(0),
                user_id: row.get(1),
                hive_id: normalize_hive_id(&row.get::<_, String>(2)),
                parent_session_id: row.get(3),
                parent_agent_id: row.get(4),
                strategy: row.get(5),
                status: row.get(6),
                task_total: row.get(7),
                task_success: row.get(8),
                task_failed: row.get(9),
                context_tokens_total: row.get(10),
                context_tokens_peak: row.get(11),
                model_round_total: row.get(12),
                started_time: row.get(13),
                finished_time: row.get(14),
                elapsed_s: row.get(15),
                summary: row.get(16),
                error: row.get(17),
                updated_time: row.get(18),
            });
        }
        Ok((output, total))
    }

    fn list_team_runs_by_status(
        &self,
        statuses: &[&str],
        offset: i64,
        limit: i64,
    ) -> Result<Vec<TeamRunRecord>> {
        self.ensure_initialized()?;
        let mut cleaned_statuses = statuses
            .iter()
            .map(|status| status.trim().to_string())
            .filter(|status| !status.is_empty())
            .collect::<Vec<_>>();
        cleaned_statuses.sort();
        cleaned_statuses.dedup();
        if cleaned_statuses.is_empty() {
            return Ok(Vec::new());
        }

        let safe_limit = limit.max(1);
        let safe_offset = offset.max(0);
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT team_run_id, user_id, hive_id, parent_session_id, parent_agent_id, strategy, status, task_total, task_success, task_failed, context_tokens_total, context_tokens_peak, model_round_total, started_time, finished_time, elapsed_s, summary, error, updated_time              FROM team_runs WHERE status = ANY($1::text[]) ORDER BY updated_time ASC LIMIT $2 OFFSET $3",
            &[&cleaned_statuses, &safe_limit, &safe_offset],
        )?;
        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            output.push(TeamRunRecord {
                team_run_id: row.get(0),
                user_id: row.get(1),
                hive_id: normalize_hive_id(&row.get::<_, String>(2)),
                parent_session_id: row.get(3),
                parent_agent_id: row.get(4),
                strategy: row.get(5),
                status: row.get(6),
                task_total: row.get(7),
                task_success: row.get(8),
                task_failed: row.get(9),
                context_tokens_total: row.get(10),
                context_tokens_peak: row.get(11),
                model_round_total: row.get(12),
                started_time: row.get(13),
                finished_time: row.get(14),
                elapsed_s: row.get(15),
                summary: row.get(16),
                error: row.get(17),
                updated_time: row.get(18),
            });
        }
        Ok(output)
    }

    fn upsert_team_task(&self, record: &TeamTaskRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let hive_id = normalize_hive_id(&record.hive_id);
        conn.execute(
            "INSERT INTO team_tasks (task_id, team_run_id, user_id, hive_id, agent_id, target_session_id, spawned_session_id, status, retry_count, priority, started_time, finished_time, elapsed_s, result_summary, error, updated_time)              VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)              ON CONFLICT(task_id) DO UPDATE SET team_run_id = EXCLUDED.team_run_id, user_id = EXCLUDED.user_id, hive_id = EXCLUDED.hive_id, agent_id = EXCLUDED.agent_id,              target_session_id = EXCLUDED.target_session_id, spawned_session_id = EXCLUDED.spawned_session_id, status = EXCLUDED.status, retry_count = EXCLUDED.retry_count,              priority = EXCLUDED.priority, started_time = EXCLUDED.started_time, finished_time = EXCLUDED.finished_time, elapsed_s = EXCLUDED.elapsed_s,              result_summary = EXCLUDED.result_summary, error = EXCLUDED.error, updated_time = EXCLUDED.updated_time",
            &[
                &record.task_id,
                &record.team_run_id,
                &record.user_id,
                &hive_id,
                &record.agent_id,
                &record.target_session_id,
                &record.spawned_session_id,
                &record.status,
                &record.retry_count,
                &record.priority,
                &record.started_time,
                &record.finished_time,
                &record.elapsed_s,
                &record.result_summary,
                &record.error,
                &record.updated_time,
            ],
        )?;
        Ok(())
    }

    fn list_team_tasks(&self, team_run_id: &str) -> Result<Vec<TeamTaskRecord>> {
        self.ensure_initialized()?;
        let cleaned_run_id = team_run_id.trim();
        if cleaned_run_id.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT task_id, team_run_id, user_id, hive_id, agent_id, target_session_id, spawned_session_id, status, retry_count, priority, started_time, finished_time, elapsed_s, result_summary, error, updated_time              FROM team_tasks WHERE team_run_id = $1 ORDER BY updated_time DESC",
            &[&cleaned_run_id],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(TeamTaskRecord {
                task_id: row.get(0),
                team_run_id: row.get(1),
                user_id: row.get(2),
                hive_id: normalize_hive_id(&row.get::<_, String>(3)),
                agent_id: row.get(4),
                target_session_id: row.get(5),
                spawned_session_id: row.get(6),
                status: row.get(7),
                retry_count: row.get(8),
                priority: row.get(9),
                started_time: row.get(10),
                finished_time: row.get(11),
                elapsed_s: row.get(12),
                result_summary: row.get(13),
                error: row.get(14),
                updated_time: row.get(15),
            });
        }
        Ok(output)
    }

    fn get_team_task(&self, task_id: &str) -> Result<Option<TeamTaskRecord>> {
        self.ensure_initialized()?;
        let cleaned = task_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT task_id, team_run_id, user_id, hive_id, agent_id, target_session_id, spawned_session_id, status, retry_count, priority, started_time, finished_time, elapsed_s, result_summary, error, updated_time FROM team_tasks WHERE task_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| TeamTaskRecord {
            task_id: row.get(0),
            team_run_id: row.get(1),
            user_id: row.get(2),
            hive_id: normalize_hive_id(&row.get::<_, String>(3)),
            agent_id: row.get(4),
            target_session_id: row.get(5),
            spawned_session_id: row.get(6),
            status: row.get(7),
            retry_count: row.get(8),
            priority: row.get(9),
            started_time: row.get(10),
            finished_time: row.get(11),
            elapsed_s: row.get(12),
            result_summary: row.get(13),
            error: row.get(14),
            updated_time: row.get(15),
        }))
    }
    fn upsert_vector_document(&self, record: &VectorDocumentRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO vector_documents \
             (doc_id, owner_id, base_name, doc_name, embedding_model, chunk_size, chunk_overlap, chunk_count, status, created_at, updated_at, content, chunks_json) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) \
             ON CONFLICT (doc_id) DO UPDATE SET \
             owner_id = EXCLUDED.owner_id, \
             base_name = EXCLUDED.base_name, \
             doc_name = EXCLUDED.doc_name, \
             embedding_model = EXCLUDED.embedding_model, \
             chunk_size = EXCLUDED.chunk_size, \
             chunk_overlap = EXCLUDED.chunk_overlap, \
             chunk_count = EXCLUDED.chunk_count, \
             status = EXCLUDED.status, \
             created_at = EXCLUDED.created_at, \
             updated_at = EXCLUDED.updated_at, \
             content = EXCLUDED.content, \
             chunks_json = EXCLUDED.chunks_json",
            &[
                &record.doc_id,
                &record.owner_id,
                &record.base_name,
                &record.doc_name,
                &record.embedding_model,
                &record.chunk_size,
                &record.chunk_overlap,
                &record.chunk_count,
                &record.status,
                &record.created_at,
                &record.updated_at,
                &record.content,
                &record.chunks_json,
            ],
        )?;
        Ok(())
    }

    fn get_vector_document(
        &self,
        owner_id: &str,
        base_name: &str,
        doc_id: &str,
    ) -> Result<Option<VectorDocumentRecord>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT doc_id, owner_id, base_name, doc_name, embedding_model, chunk_size, chunk_overlap, chunk_count, status, created_at, updated_at, content, chunks_json \
             FROM vector_documents WHERE doc_id = $1 AND owner_id = $2 AND base_name = $3",
            &[&doc_id, &owner_id, &base_name],
        )?;
        Ok(row.map(|row| VectorDocumentRecord {
            doc_id: row.get(0),
            owner_id: row.get(1),
            base_name: row.get(2),
            doc_name: row.get(3),
            embedding_model: row.get(4),
            chunk_size: row.get::<_, i64>(5),
            chunk_overlap: row.get::<_, i64>(6),
            chunk_count: row.get::<_, i64>(7),
            status: row.get(8),
            created_at: row.get(9),
            updated_at: row.get(10),
            content: row.get(11),
            chunks_json: row.get(12),
        }))
    }

    fn list_vector_document_summaries(
        &self,
        owner_id: &str,
        base_name: &str,
    ) -> Result<Vec<VectorDocumentSummaryRecord>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT doc_id, doc_name, status, chunk_count, embedding_model, updated_at \
             FROM vector_documents WHERE owner_id = $1 AND base_name = $2 \
             ORDER BY updated_at DESC",
            &[&owner_id, &base_name],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(VectorDocumentSummaryRecord {
                doc_id: row.get(0),
                doc_name: row.get(1),
                status: row.get(2),
                chunk_count: row.get::<_, i64>(3),
                embedding_model: row.get(4),
                updated_at: row.get(5),
            });
        }
        Ok(output)
    }

    fn delete_vector_document(
        &self,
        owner_id: &str,
        base_name: &str,
        doc_id: &str,
    ) -> Result<bool> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM vector_documents WHERE doc_id = $1 AND owner_id = $2 AND base_name = $3",
            &[&doc_id, &owner_id, &base_name],
        )?;
        Ok(affected > 0)
    }

    fn delete_vector_documents_by_base(&self, owner_id: &str, base_name: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM vector_documents WHERE owner_id = $1 AND base_name = $2",
            &[&owner_id, &base_name],
        )?;
        Ok(affected as i64)
    }

    fn consume_user_quota(&self, user_id: &str, today: &str) -> Result<Option<UserQuotaStatus>> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let today = today.trim();
        if today.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        let row = tx.query_opt(
            "SELECT daily_quota, daily_quota_used, daily_quota_date \
             FROM user_accounts WHERE user_id = $1 FOR UPDATE",
            &[&cleaned],
        )?;
        let Some(row) = row else {
            tx.commit()?;
            return Ok(None);
        };
        let daily_quota: i64 = row.get(0);
        let daily_used: i64 = row.get(1);
        let daily_date: Option<String> = row.get(2);
        let safe_quota = daily_quota.max(0);
        let mut used = daily_used.max(0);
        let date_match = daily_date.as_deref() == Some(today);
        if !date_match {
            used = 0;
        }
        let mut allowed = false;
        if safe_quota > 0 && used < safe_quota {
            allowed = true;
            used += 1;
        }
        let should_update = allowed || !date_match;
        if should_update {
            tx.execute(
                "UPDATE user_accounts SET daily_quota_used = $1, daily_quota_date = $2 WHERE user_id = $3",
                &[&used, &today, &cleaned],
            )?;
        }
        tx.commit()?;
        let remaining = (safe_quota - used).max(0);
        Ok(Some(UserQuotaStatus {
            daily_quota: safe_quota,
            used,
            remaining,
            date: today.to_string(),
            allowed,
        }))
    }
}
