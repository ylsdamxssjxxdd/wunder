use super::{TOOL_LOG_EXCLUDED_NAMES, TOOL_LOG_SKILL_READ_MARKER};
use crate::i18n;
use crate::schemas::AbilityDescriptor;
use crate::services::output_quality;
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
    UserWorldSendMessageResult, VectorDocumentRecord, VectorDocumentSummaryRecord, DEFAULT_HIVE_ID,
};
use anyhow::{anyhow, Result};
use chrono::{Local, Utc};
use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;
use tokio_postgres::types::ToSql;
use tokio_postgres::NoTls;

mod benchmark_store;
mod bridge_store;
mod channel_directory;
mod channel_runtime;
mod chat_session;
mod cron;
mod gateway_store;
mod media_store;
mod memory_store;
mod session_goal;
mod session_run;
mod user_world_store;

use benchmark_store::PostgresBenchmarkStorage;
use bridge_store::PostgresBridgeStorage;
use channel_directory::PostgresChannelDirectoryStorage;
use channel_runtime::PostgresChannelRuntimeStorage;
use chat_session::PostgresChatSessionStorage;
use cron::PostgresCronStorage;
use gateway_store::PostgresGatewayStorage;
use media_store::PostgresMediaStorage;
use memory_store::PostgresMemoryStorage;
use session_goal::PostgresSessionGoalStorage;
use session_run::PostgresSessionRunStorage;
use user_world_store::PostgresUserWorldStorage;

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
        if !columns.contains("token_balance") {
            conn.execute(
                "ALTER TABLE user_accounts ADD COLUMN token_balance BIGINT NOT NULL DEFAULT 0",
                &[],
            )?;
        }
        if !columns.contains("token_granted_total") {
            conn.execute(
                "ALTER TABLE user_accounts ADD COLUMN token_granted_total BIGINT NOT NULL DEFAULT 0",
                &[],
            )?;
        }
        if !columns.contains("token_used_total") {
            conn.execute(
                "ALTER TABLE user_accounts ADD COLUMN token_used_total BIGINT NOT NULL DEFAULT 0",
                &[],
            )?;
        }
        if !columns.contains("last_token_grant_date") {
            conn.execute(
                "ALTER TABLE user_accounts ADD COLUMN last_token_grant_date TEXT",
                &[],
            )?;
        }
        // Only migrate from legacy daily_quota columns when they exist
        let has_legacy_quota = columns.contains("daily_quota_date")
            && columns.contains("daily_quota")
            && columns.contains("daily_quota_used");
        if has_legacy_quota {
            let today = Local::now().format("%Y-%m-%d").to_string();
            conn.execute(
                "UPDATE user_accounts
                 SET token_balance = CASE
                         WHEN COALESCE(token_balance, 0) > 0 THEN token_balance
                         WHEN COALESCE(daily_quota_date, '') = $1 THEN GREATEST(COALESCE(daily_quota, 0) - COALESCE(daily_quota_used, 0), 0)
                         ELSE GREATEST(COALESCE(daily_quota, 0), 0)
                     END,
                     token_granted_total = CASE
                         WHEN COALESCE(token_granted_total, 0) > 0 THEN token_granted_total
                         ELSE GREATEST(COALESCE(daily_quota, 0), 0)
                     END,
                     token_used_total = CASE
                         WHEN COALESCE(token_used_total, 0) > 0 THEN token_used_total
                         WHEN COALESCE(daily_quota_date, '') = $1 THEN GREATEST(COALESCE(daily_quota_used, 0), 0)
                         ELSE 0
                     END,
                     last_token_grant_date = COALESCE(last_token_grant_date, daily_quota_date)
                 WHERE COALESCE(token_balance, 0) = 0
                    OR COALESCE(token_granted_total, 0) = 0
                    OR COALESCE(token_used_total, 0) = 0
                    OR last_token_grant_date IS NULL",
                &[&today],
            )?;
        }
        Ok(())
    }

    fn ensure_user_account_level_columns(&self, conn: &mut PgConn<'_>) -> Result<()> {
        let rows = conn.query(
            "SELECT column_name FROM information_schema.columns WHERE table_name = 'user_accounts'",
            &[],
        )?;
        let mut columns = HashSet::new();
        for row in rows {
            let name: String = row.get(0);
            columns.insert(name);
        }
        if !columns.contains("experience_total") {
            conn.execute(
                "ALTER TABLE user_accounts ADD COLUMN experience_total BIGINT NOT NULL DEFAULT 0",
                &[],
            )?;
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

    fn ensure_user_token_columns(&self, conn: &mut PgConn<'_>) -> Result<()> {
        conn.execute(
            "ALTER TABLE user_tokens ADD COLUMN IF NOT EXISTS session_scope TEXT NOT NULL DEFAULT 'default'",
            &[],
        )?;
        conn.execute(
            "UPDATE user_tokens SET session_scope = 'default' WHERE session_scope IS NULL OR btrim(session_scope) = ''",
            &[],
        )?;
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_user_tokens_user_scope_created ON user_tokens (user_id, session_scope, created_at)",
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

    fn ensure_session_run_columns(&self, conn: &mut PgConn<'_>) -> Result<()> {
        let rows = conn.query(
            "SELECT column_name FROM information_schema.columns WHERE table_name = 'session_runs'",
            &[],
        )?;
        let mut columns = HashSet::new();
        for row in rows {
            let name: String = row.get(0);
            columns.insert(name);
        }
        if !columns.contains("dispatch_id") {
            conn.execute("ALTER TABLE session_runs ADD COLUMN dispatch_id TEXT", &[])?;
        }
        if !columns.contains("run_kind") {
            conn.execute("ALTER TABLE session_runs ADD COLUMN run_kind TEXT", &[])?;
        }
        if !columns.contains("requested_by") {
            conn.execute("ALTER TABLE session_runs ADD COLUMN requested_by TEXT", &[])?;
        }
        if !columns.contains("metadata") {
            conn.execute("ALTER TABLE session_runs ADD COLUMN metadata TEXT", &[])?;
        }
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_session_runs_dispatch \
             ON session_runs (user_id, dispatch_id, updated_time)",
            &[],
        );
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
        if !columns.contains("approval_mode") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN approval_mode TEXT NOT NULL DEFAULT 'full_auto'",
                &[],
            )?;
        }
        if !columns.contains("model_name") {
            conn.execute("ALTER TABLE user_agents ADD COLUMN model_name TEXT", &[])?;
        }
        if !columns.contains("preset_questions") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN preset_questions TEXT",
                &[],
            )?;
        }
        if !columns.contains("declared_tool_names") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN declared_tool_names TEXT",
                &[],
            )?;
        }
        if !columns.contains("declared_skill_names") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN declared_skill_names TEXT",
                &[],
            )?;
        }
        if !columns.contains("visible_unit_ids") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN visible_unit_ids TEXT",
                &[],
            )?;
        }
        if !columns.contains("ability_items") {
            conn.execute("ALTER TABLE user_agents ADD COLUMN ability_items TEXT", &[])?;
        }
        if !columns.contains("preset_binding") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN preset_binding TEXT",
                &[],
            )?;
        }
        if !columns.contains("silent") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN silent INTEGER NOT NULL DEFAULT 0",
                &[],
            )?;
        }
        if !columns.contains("prefer_mother") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN prefer_mother INTEGER NOT NULL DEFAULT 0",
                &[],
            )?;
        }
        if !columns.contains("preview_skill") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN preview_skill INTEGER NOT NULL DEFAULT 0",
                &[],
            )?;
        }
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_user_agents_user_hive ON user_agents (user_id, hive_id, updated_at)",
            &[],
        )?;
        Ok(())
    }

    fn ensure_team_run_columns(&self, conn: &mut PgConn<'_>) -> Result<()> {
        conn.execute(
            "ALTER TABLE team_runs ADD COLUMN IF NOT EXISTS mother_agent_id TEXT",
            &[],
        )?;
        Ok(())
    }

    fn ensure_team_task_columns(&self, conn: &mut PgConn<'_>) -> Result<()> {
        conn.execute(
            "ALTER TABLE team_tasks ADD COLUMN IF NOT EXISTS session_run_id TEXT",
            &[],
        )?;
        Ok(())
    }

    fn ensure_user_world_group_columns(&self, conn: &mut PgConn<'_>) -> Result<()> {
        conn.execute(
            "ALTER TABLE user_world_groups ADD COLUMN IF NOT EXISTS announcement TEXT",
            &[],
        )?;
        conn.execute(
            "ALTER TABLE user_world_groups ADD COLUMN IF NOT EXISTS announcement_updated_at DOUBLE PRECISION",
            &[],
        )?;
        Ok(())
    }

    fn ensure_cron_columns(&self, conn: &mut PgConn<'_>) -> Result<()> {
        conn.execute(
            "ALTER TABLE cron_jobs ADD COLUMN IF NOT EXISTS consecutive_failures BIGINT NOT NULL DEFAULT 0",
            &[],
        )?;
        conn.execute(
            "ALTER TABLE cron_jobs ADD COLUMN IF NOT EXISTS auto_disabled_reason TEXT",
            &[],
        )?;
        conn.execute(
            "ALTER TABLE cron_jobs ADD COLUMN IF NOT EXISTS runner_id TEXT",
            &[],
        )?;
        conn.execute(
            "ALTER TABLE cron_jobs ADD COLUMN IF NOT EXISTS run_token TEXT",
            &[],
        )?;
        conn.execute(
            "ALTER TABLE cron_jobs ADD COLUMN IF NOT EXISTS heartbeat_at DOUBLE PRECISION",
            &[],
        )?;
        conn.execute(
            "ALTER TABLE cron_jobs ADD COLUMN IF NOT EXISTS lease_expires_at DOUBLE PRECISION",
            &[],
        )?;
        Ok(())
    }

    fn ensure_memory_fragment_columns(&self, conn: &mut PgConn<'_>) -> Result<()> {
        let rows = conn.query(
            "SELECT column_name, data_type FROM information_schema.columns WHERE table_name = 'memory_fragments'",
            &[],
        )?;
        let mut columns = HashMap::new();
        for row in rows {
            let name: String = row.get(0);
            let data_type: String = row.get(1);
            columns.insert(name, data_type);
        }
        if columns.is_empty() {
            return Ok(());
        }

        let ensure_column = |conn: &mut PgConn<'_>, name: &str, ddl: &str| -> Result<()> {
            if !columns.contains_key(name) {
                conn.execute(ddl, &[])?;
            }
            Ok(())
        };

        ensure_column(
            conn,
            "source_round_id",
            "ALTER TABLE memory_fragments ADD COLUMN IF NOT EXISTS source_round_id TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(
            conn,
            "tags",
            "ALTER TABLE memory_fragments ADD COLUMN IF NOT EXISTS tags TEXT NOT NULL DEFAULT '[]'",
        )?;
        ensure_column(
            conn,
            "entities",
            "ALTER TABLE memory_fragments ADD COLUMN IF NOT EXISTS entities TEXT NOT NULL DEFAULT '[]'",
        )?;
        ensure_column(
            conn,
            "importance",
            "ALTER TABLE memory_fragments ADD COLUMN IF NOT EXISTS importance DOUBLE PRECISION NOT NULL DEFAULT 0.6",
        )?;
        ensure_column(
            conn,
            "confidence",
            "ALTER TABLE memory_fragments ADD COLUMN IF NOT EXISTS confidence DOUBLE PRECISION NOT NULL DEFAULT 0.7",
        )?;
        ensure_column(
            conn,
            "tier",
            "ALTER TABLE memory_fragments ADD COLUMN IF NOT EXISTS tier TEXT NOT NULL DEFAULT 'working'",
        )?;
        ensure_column(
            conn,
            "pinned",
            "ALTER TABLE memory_fragments ADD COLUMN IF NOT EXISTS pinned BOOLEAN NOT NULL DEFAULT FALSE",
        )?;
        ensure_column(
            conn,
            "confirmed_by_user",
            "ALTER TABLE memory_fragments ADD COLUMN IF NOT EXISTS confirmed_by_user BOOLEAN NOT NULL DEFAULT FALSE",
        )?;
        ensure_column(
            conn,
            "access_count",
            "ALTER TABLE memory_fragments ADD COLUMN IF NOT EXISTS access_count BIGINT NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            conn,
            "hit_count",
            "ALTER TABLE memory_fragments ADD COLUMN IF NOT EXISTS hit_count BIGINT NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            conn,
            "last_accessed_at",
            "ALTER TABLE memory_fragments ADD COLUMN IF NOT EXISTS last_accessed_at DOUBLE PRECISION NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            conn,
            "valid_from",
            "ALTER TABLE memory_fragments ADD COLUMN IF NOT EXISTS valid_from DOUBLE PRECISION NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            conn,
            "invalidated_at",
            "ALTER TABLE memory_fragments ADD COLUMN IF NOT EXISTS invalidated_at DOUBLE PRECISION",
        )?;
        ensure_column(
            conn,
            "supersedes_memory_id",
            "ALTER TABLE memory_fragments ADD COLUMN IF NOT EXISTS supersedes_memory_id TEXT",
        )?;
        ensure_column(
            conn,
            "superseded_by_memory_id",
            "ALTER TABLE memory_fragments ADD COLUMN IF NOT EXISTS superseded_by_memory_id TEXT",
        )?;
        ensure_column(
            conn,
            "embedding_model",
            "ALTER TABLE memory_fragments ADD COLUMN IF NOT EXISTS embedding_model TEXT",
        )?;
        ensure_column(
            conn,
            "vector_ref",
            "ALTER TABLE memory_fragments ADD COLUMN IF NOT EXISTS vector_ref TEXT",
        )?;

        if columns
            .get("pinned")
            .map(String::as_str)
            .is_some_and(|ty| ty != "boolean")
        {
            conn.execute(
                "ALTER TABLE memory_fragments ALTER COLUMN pinned TYPE BOOLEAN USING CASE WHEN pinned::text IN ('1','t','true','TRUE') THEN TRUE ELSE FALSE END",
                &[],
            )?;
            conn.execute(
                "ALTER TABLE memory_fragments ALTER COLUMN pinned SET DEFAULT FALSE",
                &[],
            )?;
        }
        if columns
            .get("confirmed_by_user")
            .map(String::as_str)
            .is_some_and(|ty| ty != "boolean")
        {
            conn.execute(
                "ALTER TABLE memory_fragments ALTER COLUMN confirmed_by_user TYPE BOOLEAN USING CASE WHEN confirmed_by_user::text IN ('1','t','true','TRUE') THEN TRUE ELSE FALSE END",
                &[],
            )?;
            conn.execute(
                "ALTER TABLE memory_fragments ALTER COLUMN confirmed_by_user SET DEFAULT FALSE",
                &[],
            )?;
        }
        let _ = conn.execute(
            "UPDATE memory_fragments SET tags = '[]' WHERE tags IS NULL OR btrim(tags) = ''",
            &[],
        );
        let _ = conn.execute(
            "UPDATE memory_fragments SET entities = '[]' WHERE entities IS NULL OR btrim(entities) = ''",
            &[],
        );
        let _ = conn.execute(
            "UPDATE memory_fragments SET valid_from = COALESCE(NULLIF(valid_from, 0), updated_at, created_at, 0)",
            &[],
        );
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
                "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_model_context_entries_time \
                 ON model_context_entries USING brin (created_time)",
                "CREATE INDEX IF NOT EXISTS idx_model_context_entries_time \
                 ON model_context_entries USING brin (created_time)",
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
                CREATE TABLE IF NOT EXISTS model_context_entries (
                  id BIGSERIAL PRIMARY KEY,
                  user_id TEXT NOT NULL,
                  session_id TEXT NOT NULL,
                  role TEXT NOT NULL,
                  payload TEXT NOT NULL,
                  created_time DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_model_context_entries_session
                  ON model_context_entries (user_id, session_id, id);
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
                CREATE TABLE IF NOT EXISTS memory_fragments (
                  memory_id TEXT PRIMARY KEY,
                  user_id TEXT NOT NULL,
                  agent_id TEXT NOT NULL,
                  source_session_id TEXT NOT NULL,
                  source_round_id TEXT NOT NULL,
                  source_type TEXT NOT NULL,
                  category TEXT NOT NULL,
                  title_l0 TEXT NOT NULL,
                  summary_l1 TEXT NOT NULL,
                  content_l2 TEXT NOT NULL,
                  fact_key TEXT NOT NULL,
                  tags TEXT NOT NULL,
                  entities TEXT NOT NULL,
                  importance DOUBLE PRECISION NOT NULL,
                  confidence DOUBLE PRECISION NOT NULL,
                  tier TEXT NOT NULL,
                  status TEXT NOT NULL,
                  pinned BOOLEAN NOT NULL DEFAULT FALSE,
                  confirmed_by_user BOOLEAN NOT NULL DEFAULT FALSE,
                  access_count BIGINT NOT NULL DEFAULT 0,
                  hit_count BIGINT NOT NULL DEFAULT 0,
                  last_accessed_at DOUBLE PRECISION NOT NULL DEFAULT 0,
                  valid_from DOUBLE PRECISION NOT NULL,
                  invalidated_at DOUBLE PRECISION,
                  supersedes_memory_id TEXT,
                  superseded_by_memory_id TEXT,
                  embedding_model TEXT,
                  vector_ref TEXT,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_memory_fragments_user_agent
                  ON memory_fragments (user_id, agent_id, updated_at DESC);
                CREATE INDEX IF NOT EXISTS idx_memory_fragments_fact_key
                  ON memory_fragments (user_id, agent_id, fact_key);
                CREATE INDEX IF NOT EXISTS idx_memory_fragments_status
                  ON memory_fragments (user_id, agent_id, status, updated_at DESC);
                CREATE TABLE IF NOT EXISTS memory_fragment_embeddings (
                  memory_id TEXT NOT NULL,
                  user_id TEXT NOT NULL,
                  agent_id TEXT NOT NULL,
                  embedding_model TEXT NOT NULL,
                  content_hash TEXT NOT NULL,
                  vector_json TEXT NOT NULL,
                  dimensions BIGINT NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL,
                  PRIMARY KEY (memory_id, embedding_model, content_hash)
                );
                CREATE INDEX IF NOT EXISTS idx_memory_fragment_embeddings_user_agent
                  ON memory_fragment_embeddings (user_id, agent_id, updated_at DESC);
                CREATE INDEX IF NOT EXISTS idx_memory_fragment_embeddings_memory
                  ON memory_fragment_embeddings (memory_id, updated_at DESC);
                CREATE TABLE IF NOT EXISTS memory_hits (
                  hit_id TEXT PRIMARY KEY,
                  memory_id TEXT NOT NULL,
                  user_id TEXT NOT NULL,
                  agent_id TEXT NOT NULL,
                  session_id TEXT NOT NULL,
                  round_id TEXT NOT NULL,
                  query_text TEXT NOT NULL,
                  reason_json TEXT NOT NULL,
                  lexical_score DOUBLE PRECISION NOT NULL,
                  semantic_score DOUBLE PRECISION NOT NULL,
                  freshness_score DOUBLE PRECISION NOT NULL,
                  importance_score DOUBLE PRECISION NOT NULL,
                  final_score DOUBLE PRECISION NOT NULL,
                  created_at DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_memory_hits_user_agent
                  ON memory_hits (user_id, agent_id, created_at DESC);
                CREATE INDEX IF NOT EXISTS idx_memory_hits_session
                  ON memory_hits (user_id, agent_id, session_id, created_at DESC);
                CREATE INDEX IF NOT EXISTS idx_memory_hits_memory
                  ON memory_hits (memory_id, created_at DESC);
                CREATE TABLE IF NOT EXISTS memory_jobs (
                  job_id TEXT PRIMARY KEY,
                  user_id TEXT NOT NULL,
                  agent_id TEXT NOT NULL,
                  session_id TEXT NOT NULL,
                  job_type TEXT NOT NULL,
                  status TEXT NOT NULL,
                  request_payload TEXT NOT NULL,
                  result_summary TEXT NOT NULL,
                  error_message TEXT NOT NULL,
                  queued_at DOUBLE PRECISION NOT NULL,
                  started_at DOUBLE PRECISION NOT NULL,
                  finished_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_memory_jobs_user_agent
                  ON memory_jobs (user_id, agent_id, updated_at DESC);
                CREATE INDEX IF NOT EXISTS idx_memory_jobs_session
                  ON memory_jobs (session_id, updated_at DESC);
                CREATE TABLE IF NOT EXISTS benchmark_runs (
                    run_id TEXT PRIMARY KEY,
                    user_id TEXT,
                    model_name TEXT,
                    judge_model_name TEXT,
                    status TEXT,
                    total_score DOUBLE PRECISION,
                    started_time DOUBLE PRECISION,
                    finished_time DOUBLE PRECISION,
                    payload TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_benchmark_runs_user
                  ON benchmark_runs (user_id);
                CREATE INDEX IF NOT EXISTS idx_benchmark_runs_status
                  ON benchmark_runs (status);
                CREATE INDEX IF NOT EXISTS idx_benchmark_runs_started
                  ON benchmark_runs (started_time);
                CREATE TABLE IF NOT EXISTS benchmark_attempts (
                    id BIGSERIAL PRIMARY KEY,
                    run_id TEXT NOT NULL,
                    task_id TEXT NOT NULL,
                    attempt_no BIGINT NOT NULL,
                    status TEXT,
                    final_score DOUBLE PRECISION,
                    started_time DOUBLE PRECISION,
                    finished_time DOUBLE PRECISION,
                    payload TEXT NOT NULL,
                    UNIQUE(run_id, task_id, attempt_no)
                );
                CREATE INDEX IF NOT EXISTS idx_benchmark_attempts_run
                  ON benchmark_attempts (run_id, task_id, attempt_no);
                CREATE INDEX IF NOT EXISTS idx_benchmark_attempts_status
                  ON benchmark_attempts (status);
                CREATE TABLE IF NOT EXISTS benchmark_task_aggregates (
                    id BIGSERIAL PRIMARY KEY,
                    run_id TEXT NOT NULL,
                    task_id TEXT NOT NULL,
                    status TEXT,
                    mean_score DOUBLE PRECISION,
                    payload TEXT NOT NULL,
                    UNIQUE(run_id, task_id)
                );
                CREATE INDEX IF NOT EXISTS idx_benchmark_task_aggregates_run
                  ON benchmark_task_aggregates (run_id, task_id);
                CREATE TABLE IF NOT EXISTS user_accounts (
                    user_id TEXT PRIMARY KEY,
                    username TEXT NOT NULL UNIQUE,
                  email TEXT,
                  password_hash TEXT NOT NULL,
                  roles TEXT NOT NULL,
                  status TEXT NOT NULL,
                  access_level TEXT NOT NULL,
                  unit_id TEXT,
                  token_balance BIGINT NOT NULL DEFAULT 0,
                  token_granted_total BIGINT NOT NULL DEFAULT 0,
                  token_used_total BIGINT NOT NULL DEFAULT 0,
                  last_token_grant_date TEXT,
                  experience_total BIGINT NOT NULL DEFAULT 0,
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
                  session_scope TEXT NOT NULL DEFAULT 'default',
                  expires_at DOUBLE PRECISION NOT NULL,
                  created_at DOUBLE PRECISION NOT NULL,
                  last_used_at DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_user_tokens_user
                  ON user_tokens (user_id);
                CREATE INDEX IF NOT EXISTS idx_user_tokens_expires
                  ON user_tokens (expires_at);
                CREATE TABLE IF NOT EXISTS user_session_scopes (
                  user_id TEXT NOT NULL,
                  session_scope TEXT NOT NULL,
                  last_login_at DOUBLE PRECISION NOT NULL,
                  PRIMARY KEY (user_id, session_scope)
                );
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
                CREATE TABLE IF NOT EXISTS session_goals (
                  session_id TEXT PRIMARY KEY,
                  user_id TEXT NOT NULL,
                  goal_id TEXT NOT NULL,
                  objective TEXT NOT NULL,
                  status TEXT NOT NULL,
                  token_budget BIGINT,
                  tokens_used BIGINT NOT NULL DEFAULT 0,
                  time_used_seconds BIGINT NOT NULL DEFAULT 0,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL,
                  completed_at DOUBLE PRECISION,
                  last_continued_at DOUBLE PRECISION,
                  source TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_session_goals_user
                  ON session_goals (user_id, updated_at DESC);
                CREATE INDEX IF NOT EXISTS idx_session_goals_status
                  ON session_goals (user_id, status, updated_at DESC);
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
                CREATE TABLE IF NOT EXISTS user_world_groups (
                  group_id TEXT PRIMARY KEY,
                  conversation_id TEXT NOT NULL UNIQUE,
                  group_name TEXT NOT NULL,
                  owner_user_id TEXT NOT NULL,
                  announcement TEXT,
                  announcement_updated_at DOUBLE PRECISION,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_user_world_groups_conversation
                  ON user_world_groups (conversation_id);
                CREATE INDEX IF NOT EXISTS idx_user_world_groups_owner
                  ON user_world_groups (owner_user_id, updated_at DESC);
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
                CREATE INDEX IF NOT EXISTS idx_user_world_members_conversation
                  ON user_world_members (conversation_id);
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
                CREATE TABLE IF NOT EXISTS beeroom_chat_messages (
                  message_id BIGSERIAL PRIMARY KEY,
                  user_id TEXT NOT NULL,
                  group_id TEXT NOT NULL,
                  sender_kind TEXT NOT NULL,
                  sender_name TEXT NOT NULL,
                  sender_agent_id TEXT,
                  mention_name TEXT,
                  mention_agent_id TEXT,
                  body TEXT NOT NULL,
                  meta TEXT,
                  tone TEXT NOT NULL,
                  client_msg_id TEXT,
                  created_at DOUBLE PRECISION NOT NULL
                );
                CREATE UNIQUE INDEX IF NOT EXISTS idx_beeroom_chat_messages_client
                  ON beeroom_chat_messages (user_id, group_id, client_msg_id);
                CREATE INDEX IF NOT EXISTS idx_beeroom_chat_messages_group
                  ON beeroom_chat_messages (user_id, group_id, message_id DESC);
                CREATE TABLE IF NOT EXISTS session_runs (
                  run_id TEXT PRIMARY KEY,
                  session_id TEXT NOT NULL,
                  parent_session_id TEXT,
                  user_id TEXT NOT NULL,
                  dispatch_id TEXT,
                  run_kind TEXT,
                  requested_by TEXT,
                  agent_id TEXT,
                  model_name TEXT,
                  status TEXT NOT NULL,
                  queued_time DOUBLE PRECISION,
                  started_time DOUBLE PRECISION,
                  finished_time DOUBLE PRECISION,
                  elapsed_s DOUBLE PRECISION,
                  result TEXT,
                  error TEXT,
                  updated_time DOUBLE PRECISION NOT NULL,
                  metadata TEXT
                );
                CREATE INDEX IF NOT EXISTS idx_session_runs_session
                  ON session_runs (session_id, updated_time);
                CREATE INDEX IF NOT EXISTS idx_session_runs_user
                  ON session_runs (user_id, updated_time);
                CREATE INDEX IF NOT EXISTS idx_session_runs_parent
                  ON session_runs (parent_session_id, updated_time);
                CREATE INDEX IF NOT EXISTS idx_session_runs_dispatch
                  ON session_runs (user_id, dispatch_id, updated_time);
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
                  runner_id TEXT,
                  run_token TEXT,
                  heartbeat_at DOUBLE PRECISION,
                  lease_expires_at DOUBLE PRECISION,
                  last_run_at DOUBLE PRECISION,
                  last_status TEXT,
                  last_error TEXT,
                  consecutive_failures BIGINT NOT NULL DEFAULT 0,
                  auto_disabled_reason TEXT,
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
                CREATE TABLE IF NOT EXISTS bridge_centers (
                  center_id TEXT PRIMARY KEY,
                  name TEXT NOT NULL UNIQUE,
                  code TEXT NOT NULL UNIQUE,
                  description TEXT,
                  owner_user_id TEXT NOT NULL,
                  status TEXT NOT NULL,
                  default_preset_agent_name TEXT NOT NULL,
                  target_unit_id TEXT,
                  default_identity_strategy TEXT NOT NULL,
                  username_policy TEXT NOT NULL,
                  password_policy TEXT NOT NULL,
                  settings_json TEXT NOT NULL,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_bridge_centers_status
                  ON bridge_centers (status, updated_at);
                CREATE TABLE IF NOT EXISTS bridge_center_accounts (
                  center_account_id TEXT PRIMARY KEY,
                  center_id TEXT NOT NULL,
                  channel TEXT NOT NULL,
                  account_id TEXT NOT NULL,
                  enabled INTEGER NOT NULL DEFAULT 1,
                  default_preset_agent_name_override TEXT,
                  identity_strategy TEXT,
                  thread_strategy TEXT,
                  reply_strategy TEXT,
                  fallback_policy TEXT NOT NULL,
                  provider_caps_json TEXT,
                  status_reason TEXT,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL,
                  UNIQUE (channel, account_id),
                  UNIQUE (center_id, channel, account_id)
                );
                CREATE INDEX IF NOT EXISTS idx_bridge_center_accounts_center
                  ON bridge_center_accounts (center_id, updated_at);
                CREATE TABLE IF NOT EXISTS bridge_user_routes (
                  route_id TEXT PRIMARY KEY,
                  center_id TEXT NOT NULL,
                  center_account_id TEXT NOT NULL,
                  channel TEXT NOT NULL,
                  account_id TEXT NOT NULL,
                  external_identity_key TEXT NOT NULL,
                  external_user_key TEXT,
                  external_display_name TEXT,
                  external_peer_id TEXT,
                  external_sender_id TEXT,
                  external_thread_id TEXT,
                  external_profile_json TEXT,
                  wunder_user_id TEXT NOT NULL,
                  agent_id TEXT NOT NULL,
                  agent_name TEXT NOT NULL,
                  user_created INTEGER NOT NULL DEFAULT 0,
                  agent_created INTEGER NOT NULL DEFAULT 0,
                  status TEXT NOT NULL,
                  last_session_id TEXT,
                  last_error TEXT,
                  first_seen_at DOUBLE PRECISION NOT NULL,
                  last_seen_at DOUBLE PRECISION NOT NULL,
                  last_inbound_at DOUBLE PRECISION,
                  last_outbound_at DOUBLE PRECISION,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL,
                  UNIQUE (center_account_id, external_identity_key)
                );
                CREATE INDEX IF NOT EXISTS idx_bridge_user_routes_center
                  ON bridge_user_routes (center_id, status, last_seen_at);
                CREATE INDEX IF NOT EXISTS idx_bridge_user_routes_user
                  ON bridge_user_routes (wunder_user_id, updated_at);
                CREATE INDEX IF NOT EXISTS idx_bridge_user_routes_agent
                  ON bridge_user_routes (agent_id, updated_at);
                CREATE TABLE IF NOT EXISTS bridge_delivery_logs (
                  delivery_id TEXT PRIMARY KEY,
                  center_id TEXT NOT NULL,
                  center_account_id TEXT NOT NULL,
                  route_id TEXT,
                  direction TEXT NOT NULL,
                  stage TEXT NOT NULL,
                  provider_message_id TEXT,
                  session_id TEXT,
                  status TEXT NOT NULL,
                  summary TEXT NOT NULL,
                  payload_json TEXT,
                  created_at DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_bridge_delivery_logs_center
                  ON bridge_delivery_logs (center_id, created_at);
                CREATE INDEX IF NOT EXISTS idx_bridge_delivery_logs_route
                  ON bridge_delivery_logs (route_id, created_at);
                CREATE TABLE IF NOT EXISTS bridge_route_audit_logs (
                  audit_id TEXT PRIMARY KEY,
                  center_id TEXT NOT NULL,
                  route_id TEXT,
                  actor_type TEXT NOT NULL,
                  actor_id TEXT NOT NULL,
                  action TEXT NOT NULL,
                  detail_json TEXT,
                  created_at DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_bridge_route_audit_logs_center
                  ON bridge_route_audit_logs (center_id, created_at);
                CREATE INDEX IF NOT EXISTS idx_bridge_route_audit_logs_route
                  ON bridge_route_audit_logs (route_id, created_at);
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
                  model_name TEXT,
                  tool_names TEXT,
                  declared_tool_names TEXT,
                  declared_skill_names TEXT,
                  visible_unit_ids TEXT,
                  ability_items TEXT,
                  access_level TEXT NOT NULL,
                  approval_mode TEXT NOT NULL DEFAULT 'full_auto',
                  is_shared INTEGER NOT NULL DEFAULT 0,
                  status TEXT NOT NULL,
                  icon TEXT,
                  sandbox_container_id INTEGER NOT NULL DEFAULT 1,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL,
                  preset_questions TEXT,
                  preset_binding TEXT,
                  silent INTEGER NOT NULL DEFAULT 0,
                  prefer_mother INTEGER NOT NULL DEFAULT 0,
                  preview_skill INTEGER NOT NULL DEFAULT 0
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
                  mother_agent_id TEXT,
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
                  session_run_id TEXT,
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
                    self.ensure_user_account_level_columns(&mut conn)?;
                    self.ensure_user_account_unit_columns(&mut conn)?;
                    self.ensure_user_account_list_indexes(&mut conn)?;
                    self.ensure_user_token_columns(&mut conn)?;
                    self.ensure_user_tool_access_columns(&mut conn)?;
                    self.ensure_chat_session_columns(&mut conn)?;
                    self.ensure_channel_columns(&mut conn)?;
                    self.ensure_session_lock_columns(&mut conn)?;
                    self.ensure_session_run_columns(&mut conn)?;
                    self.ensure_user_agent_columns(&mut conn)?;
                    self.ensure_team_run_columns(&mut conn)?;
                    self.ensure_team_task_columns(&mut conn)?;
                    self.ensure_user_world_group_columns(&mut conn)?;
                    self.ensure_cron_columns(&mut conn)?;
                    self.ensure_memory_fragment_columns(&mut conn)?;
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

    fn list_meta_prefix(&self, prefix: &str) -> Result<Vec<(String, String)>> {
        self.ensure_initialized()?;
        let cleaned = prefix.trim();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let pattern = format!("{cleaned}%");
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT key, value FROM meta WHERE key LIKE $1 ORDER BY updated_time DESC, key ASC",
            &[&pattern],
        )?;
        Ok(rows
            .into_iter()
            .map(|row| (row.get::<_, String>(0), row.get::<_, String>(1)))
            .collect())
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
        let payload = output_quality::annotate_chat_payload(payload);
        let content = Self::parse_string(payload.get("content"));
        let timestamp = Self::parse_string(payload.get("timestamp"));
        let meta = payload
            .get("meta")
            .and_then(|value| serde_json::to_string(value).ok());
        let payload_text = Self::json_to_string(payload.as_ref());
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

    fn append_model_context_entry(
        &self,
        user_id: &str,
        session_id: &str,
        payload: &Value,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
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
        let payload_text = Self::json_to_string(payload);
        let now = Self::now_ts();
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO model_context_entries (user_id, session_id, role, payload, created_time) \
             VALUES ($1, $2, $3, $4, $5)",
            &[&cleaned_user, &cleaned_session, &role, &payload_text, &now],
        )?;
        Ok(())
    }

    fn replace_model_context_entries(
        &self,
        user_id: &str,
        session_id: &str,
        payloads: &[Value],
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM model_context_entries WHERE user_id = $1 AND session_id = $2",
            &[&cleaned_user, &cleaned_session],
        )?;
        for payload in payloads {
            let role = payload
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if role.is_empty() {
                continue;
            }
            let payload_text = Self::json_to_string(payload);
            let now = Self::now_ts();
            tx.execute(
                "INSERT INTO model_context_entries (user_id, session_id, role, payload, created_time) \
                 VALUES ($1, $2, $3, $4, $5)",
                &[&cleaned_user, &cleaned_session, &role, &payload_text, &now],
            )?;
        }
        tx.commit()?;
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

    fn load_model_context_entries(
        &self,
        user_id: &str,
        session_id: &str,
        limit: Option<i64>,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(Vec::new());
        }
        let limit_value = limit.filter(|value| *value > 0);
        let mut conn = self.conn()?;
        let mut rows: Vec<String> = if let Some(limit_value) = limit_value {
            conn.query(
                "SELECT payload FROM model_context_entries WHERE user_id = $1 AND session_id = $2 ORDER BY id DESC LIMIT $3",
                &[&cleaned_user, &cleaned_session, &limit_value],
            )?
            .into_iter()
            .map(|row| row.get::<_, String>(0))
            .collect()
        } else {
            conn.query(
                "SELECT payload FROM model_context_entries WHERE user_id = $1 AND session_id = $2 ORDER BY id ASC",
                &[&cleaned_user, &cleaned_session],
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

    fn load_chat_history_page(
        &self,
        user_id: &str,
        session_id: &str,
        before_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        if user_id.trim().is_empty() || session_id.trim().is_empty() || limit <= 0 {
            return Ok(Vec::new());
        }
        let before_id = before_id.filter(|value| *value > 0);
        let mut conn = self.conn()?;
        let mut rows: Vec<(i64, String)> = if let Some(before_id) = before_id {
            conn.query(
                "SELECT id, payload FROM chat_history WHERE user_id = $1 AND session_id = $2 AND id < $3 ORDER BY id DESC LIMIT $4",
                &[&user_id, &session_id, &before_id, &limit],
            )?
            .into_iter()
            .map(|row| (row.get::<_, i64>(0), row.get::<_, String>(1)))
            .collect()
        } else {
            conn.query(
                "SELECT id, payload FROM chat_history WHERE user_id = $1 AND session_id = $2 ORDER BY id DESC LIMIT $3",
                &[&user_id, &session_id, &limit],
            )?
            .into_iter()
            .map(|row| (row.get::<_, i64>(0), row.get::<_, String>(1)))
            .collect()
        };
        rows.reverse();
        let mut records = Vec::new();
        for (history_id, payload) in rows {
            if let Some(mut value) = Self::json_from_str(&payload) {
                if let Value::Object(ref mut map) = value {
                    map.insert("_history_id".to_string(), json!(history_id));
                }
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

    fn delete_chat_history(&self, _user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = _user_id.trim();
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
        let _ = conn.execute(
            "DELETE FROM model_context_entries WHERE user_id = $1 AND session_id = $2",
            &[&cleaned_user, &cleaned_session],
        );
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

    fn count_session_locks(&self) -> Result<i64> {
        self.ensure_initialized()?;
        let now = Self::now_ts();
        let mut conn = self.conn()?;
        let total = conn
            .query_one(
                "SELECT COUNT(*) FROM session_locks WHERE expires_at > $1",
                &[&now],
            )?
            .get(0);
        Ok(total)
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

    fn count_pending_agent_tasks(&self) -> Result<i64> {
        self.ensure_initialized()?;
        let now = Self::now_ts();
        let mut conn = self.conn()?;
        let total = conn
            .query_one(
                "SELECT COUNT(*) FROM agent_tasks WHERE (status = 'pending' OR status = 'retry') AND retry_at <= $1",
                &[&now],
            )?
            .get(0);
        Ok(total)
    }

    fn count_pending_agent_tasks_ahead(
        &self,
        retry_at: f64,
        created_at: f64,
        task_id: &str,
    ) -> Result<i64> {
        self.ensure_initialized()?;
        let now = Self::now_ts();
        let mut conn = self.conn()?;
        let total = conn
            .query_one(
                "SELECT COUNT(*) FROM agent_tasks \
                 WHERE (status = 'pending' OR status = 'retry') AND retry_at <= $1 \
                   AND (retry_at < $2 OR (retry_at = $3 AND created_at < $4) OR (retry_at = $5 AND created_at = $6 AND task_id < $7))",
                &[&now, &retry_at, &retry_at, &created_at, &retry_at, &created_at, &task_id],
            )?
            .get(0);
        Ok(total)
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
                    map.insert("event_seq".to_string(), json!(event_id));
                    records.push(value);
                } else {
                    records.push(
                        json!({ "event_id": event_id, "event_seq": event_id, "data": value }),
                    );
                }
            }
        }
        Ok(records)
    }

    fn load_recent_stream_events(&self, _session_id: &str, _limit: i64) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let cleaned_session = _session_id.trim();
        if cleaned_session.is_empty() || _limit <= 0 {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT event_id, payload FROM stream_events WHERE session_id = $1 ORDER BY event_id DESC LIMIT $2",
            &[&cleaned_session, &_limit],
        )?;
        let mut rows = rows;
        rows.reverse();
        let mut records = Vec::new();
        for row in rows {
            let event_id: i64 = row.get(0);
            let payload: String = row.get(1);
            if let Some(mut value) = Self::json_from_str(&payload) {
                if let Value::Object(ref mut map) = value {
                    map.insert("event_id".to_string(), json!(event_id));
                    map.insert("event_seq".to_string(), json!(event_id));
                    records.push(value);
                } else {
                    records.push(
                        json!({ "event_id": event_id, "event_seq": event_id, "data": value }),
                    );
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
        let model_context = delete_with_filter(
            "DELETE FROM model_context_entries WHERE created_time < $1",
            false,
        )?;
        results.insert("model_context_entries".to_string(), model_context);
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
             token_balance, token_granted_total, token_used_total, last_token_grant_date, experience_total, is_demo, created_at, updated_at, last_login_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17) \
             ON CONFLICT(user_id) DO UPDATE SET username = EXCLUDED.username, email = EXCLUDED.email, password_hash = EXCLUDED.password_hash, \
             roles = EXCLUDED.roles, status = EXCLUDED.status, access_level = EXCLUDED.access_level, unit_id = EXCLUDED.unit_id, \
             token_balance = EXCLUDED.token_balance, token_granted_total = EXCLUDED.token_granted_total, token_used_total = EXCLUDED.token_used_total, \
             last_token_grant_date = EXCLUDED.last_token_grant_date, \
             experience_total = EXCLUDED.experience_total, \
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
                &record.token_balance,
                &record.token_granted_total,
                &record.token_used_total,
                &record.last_token_grant_date,
                &record.experience_total,
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
                 token_balance, token_granted_total, token_used_total, last_token_grant_date, experience_total, is_demo, created_at, updated_at, last_login_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17) \
                 ON CONFLICT(user_id) DO UPDATE SET username = EXCLUDED.username, email = EXCLUDED.email, password_hash = EXCLUDED.password_hash, \
                 roles = EXCLUDED.roles, status = EXCLUDED.status, access_level = EXCLUDED.access_level, unit_id = EXCLUDED.unit_id, \
                 token_balance = EXCLUDED.token_balance, token_granted_total = EXCLUDED.token_granted_total, token_used_total = EXCLUDED.token_used_total, \
                 last_token_grant_date = EXCLUDED.last_token_grant_date, \
                 experience_total = EXCLUDED.experience_total, \
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
                    &record.token_balance,
                    &record.token_granted_total,
                    &record.token_used_total,
                    &record.last_token_grant_date,
                    &record.experience_total,
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
            "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
             experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts WHERE user_id = $1",
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
            token_balance: row.get::<_, Option<i64>>(8).unwrap_or(0),
            token_granted_total: row.get::<_, Option<i64>>(9).unwrap_or(0),
            token_used_total: row.get::<_, Option<i64>>(10).unwrap_or(0),
            last_token_grant_date: row.get(11),
            experience_total: row.get::<_, Option<i64>>(12).unwrap_or(0),
            is_demo: row.get::<_, i32>(13) != 0,
            created_at: row.get(14),
            updated_at: row.get(15),
            last_login_at: row.get(16),
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
            "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
             experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts WHERE username = $1",
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
            token_balance: row.get::<_, Option<i64>>(8).unwrap_or(0),
            token_granted_total: row.get::<_, Option<i64>>(9).unwrap_or(0),
            token_used_total: row.get::<_, Option<i64>>(10).unwrap_or(0),
            last_token_grant_date: row.get(11),
            experience_total: row.get::<_, Option<i64>>(12).unwrap_or(0),
            is_demo: row.get::<_, i32>(13) != 0,
            created_at: row.get(14),
            updated_at: row.get(15),
            last_login_at: row.get(16),
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
            "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
             experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts WHERE email = $1",
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
            token_balance: row.get::<_, Option<i64>>(8).unwrap_or(0),
            token_granted_total: row.get::<_, Option<i64>>(9).unwrap_or(0),
            token_used_total: row.get::<_, Option<i64>>(10).unwrap_or(0),
            last_token_grant_date: row.get(11),
            experience_total: row.get::<_, Option<i64>>(12).unwrap_or(0),
            is_demo: row.get::<_, i32>(13) != 0,
            created_at: row.get(14),
            updated_at: row.get(15),
            last_login_at: row.get(16),
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
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
                         experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts \
                         WHERE (username ILIKE $1 OR email ILIKE $1) AND unit_id = ANY($2) \
                         ORDER BY created_at DESC LIMIT $3 OFFSET $4",
                        &[&pattern, unit_ids, &limit, &offset.max(0)],
                    )?
                } else {
                    conn.query(
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
                         experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts \
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
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
                         experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts \
                         WHERE username ILIKE $1 OR email ILIKE $1 \
                         ORDER BY created_at DESC LIMIT $2 OFFSET $3",
                        &[&pattern, &limit, &offset.max(0)],
                    )?
                } else {
                    conn.query(
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
                         experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts \
                         WHERE username ILIKE $1 OR email ILIKE $1 \
                         ORDER BY created_at DESC",
                        &[&pattern],
                    )?
                }
            }
            (None, Some(unit_ids)) => {
                if limit > 0 {
                    conn.query(
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
                         experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts \
                         WHERE unit_id = ANY($1) \
                         ORDER BY created_at DESC LIMIT $2 OFFSET $3",
                        &[unit_ids, &limit, &offset.max(0)],
                    )?
                } else {
                    conn.query(
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
                         experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts \
                         WHERE unit_id = ANY($1) ORDER BY created_at DESC",
                        &[unit_ids],
                    )?
                }
            }
            (None, None) => {
                if limit > 0 {
                    conn.query(
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
                         experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts \
                         ORDER BY created_at DESC LIMIT $1 OFFSET $2",
                        &[&limit, &offset.max(0)],
                    )?
                } else {
                    conn.query(
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
                         experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts ORDER BY created_at DESC",
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
                token_balance: row.get::<_, Option<i64>>(8).unwrap_or(0),
                token_granted_total: row.get::<_, Option<i64>>(9).unwrap_or(0),
                token_used_total: row.get::<_, Option<i64>>(10).unwrap_or(0),
                last_token_grant_date: row.get(11),
                experience_total: row.get::<_, Option<i64>>(12).unwrap_or(0),
                is_demo: row.get::<_, i32>(13) != 0,
                created_at: row.get(14),
                updated_at: row.get(15),
                last_login_at: row.get(16),
            });
        }
        Ok((output, total))
    }

    fn add_user_experience(
        &self,
        user_id: &str,
        delta: i64,
        updated_at: f64,
    ) -> Result<UserExperienceUpdateResult> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(UserExperienceUpdateResult {
                previous_total: 0,
                current_total: 0,
            });
        }
        let mut conn = self.conn()?;
        let previous_total = conn
            .query_opt(
                "SELECT experience_total FROM user_accounts WHERE user_id = $1",
                &[&cleaned],
            )?
            .map(|value| value.get::<_, Option<i64>>(0).unwrap_or(0))
            .unwrap_or(0)
            .max(0);
        let safe_delta = delta.max(0);
        if safe_delta > 0 {
            let row = conn.query_one(
                "UPDATE user_accounts \
                 SET experience_total = COALESCE(experience_total, 0) + $1, updated_at = $2 \
                 WHERE user_id = $3 \
                 RETURNING experience_total",
                &[&safe_delta, &updated_at, &cleaned],
            )?;
            let total: i64 = row.get::<_, Option<i64>>(0).unwrap_or(0);
            return Ok(UserExperienceUpdateResult {
                previous_total,
                current_total: total.max(0),
            });
        }
        let row = conn.query_opt(
            "SELECT experience_total FROM user_accounts WHERE user_id = $1",
            &[&cleaned],
        )?;
        Ok(UserExperienceUpdateResult {
            previous_total,
            current_total: row
                .map(|value| value.get::<_, Option<i64>>(0).unwrap_or(0))
                .unwrap_or(0)
                .max(0),
        })
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
            "INSERT INTO user_tokens (token, user_id, session_scope, expires_at, created_at, last_used_at) VALUES ($1, $2, $3, $4, $5, $6)",
            &[
                &record.token,
                &record.user_id,
                &record.session_scope,
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
            "SELECT token, user_id, session_scope, expires_at, created_at, last_used_at FROM user_tokens WHERE token = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| UserTokenRecord {
            token: row.get(0),
            user_id: row.get(1),
            session_scope: row.get(2),
            expires_at: row.get(3),
            created_at: row.get(4),
            last_used_at: row.get(5),
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

    fn upsert_user_session_scope(&self, record: &UserSessionScopeRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO user_session_scopes (user_id, session_scope, last_login_at)
             VALUES ($1, $2, $3)
             ON CONFLICT (user_id, session_scope) DO UPDATE
             SET last_login_at = EXCLUDED.last_login_at",
            &[
                &record.user_id,
                &record.session_scope,
                &record.last_login_at,
            ],
        )?;
        Ok(())
    }

    fn get_user_session_scope(
        &self,
        user_id: &str,
        session_scope: &str,
    ) -> Result<Option<UserSessionScopeRecord>> {
        self.ensure_initialized()?;
        let cleaned_user_id = user_id.trim();
        let cleaned_scope = session_scope.trim();
        if cleaned_user_id.is_empty() || cleaned_scope.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT user_id, session_scope, last_login_at
             FROM user_session_scopes
             WHERE user_id = $1 AND session_scope = $2",
            &[&cleaned_user_id, &cleaned_scope],
        )?;
        Ok(row.map(|row| UserSessionScopeRecord {
            user_id: row.get(0),
            session_scope: row.get(1),
            last_login_at: row.get(2),
        }))
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
        let allowed_tools = allowed
            .map(|value| Self::parse_string_list(Some(value)))
            .filter(|items| !items.is_empty());
        Ok(Some(UserToolAccessRecord {
            user_id: cleaned.to_string(),
            allowed_tools,
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
        let normalized_allowed_tools = allowed_tools.filter(|items| !items.is_empty());
        if normalized_allowed_tools.is_some() {
            let payload = normalized_allowed_tools
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
        let declared_tool_names = if record.declared_tool_names.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.declared_tool_names))
        };
        let declared_skill_names = if record.declared_skill_names.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.declared_skill_names))
        };
        let visible_unit_ids = if record.visible_unit_ids.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.visible_unit_ids))
        };
        let ability_items = if record.ability_items.is_empty() {
            None
        } else {
            serde_json::to_string(&record.ability_items).ok()
        };
        let preset_questions = if record.preset_questions.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.preset_questions))
        };
        let preset_binding = record
            .preset_binding
            .as_ref()
            .and_then(|value| serde_json::to_string(value).ok());
        let is_shared = if record.is_shared { 1 } else { 0 };
        let silent = if record.silent { 1 } else { 0 };
        let prefer_mother = if record.prefer_mother { 1 } else { 0 };
        let preview_skill = if record.preview_skill { 1 } else { 0 };
        let sandbox_container_id = normalize_sandbox_container_id(record.sandbox_container_id);
        let hive_id = normalize_hive_id(&record.hive_id);
        conn.execute(
            "INSERT INTO user_agents (agent_id, user_id, hive_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids)              VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25)              ON CONFLICT(agent_id) DO UPDATE SET user_id = EXCLUDED.user_id, hive_id = EXCLUDED.hive_id, name = EXCLUDED.name, description = EXCLUDED.description,              system_prompt = EXCLUDED.system_prompt, model_name = EXCLUDED.model_name, tool_names = EXCLUDED.tool_names, declared_tool_names = EXCLUDED.declared_tool_names, declared_skill_names = EXCLUDED.declared_skill_names, ability_items = EXCLUDED.ability_items, access_level = EXCLUDED.access_level, approval_mode = EXCLUDED.approval_mode,              is_shared = EXCLUDED.is_shared, status = EXCLUDED.status, icon = EXCLUDED.icon, sandbox_container_id = EXCLUDED.sandbox_container_id, updated_at = EXCLUDED.updated_at, preset_questions = EXCLUDED.preset_questions, preset_binding = EXCLUDED.preset_binding, silent = EXCLUDED.silent, prefer_mother = EXCLUDED.prefer_mother, preview_skill = EXCLUDED.preview_skill, visible_unit_ids = EXCLUDED.visible_unit_ids",
            &[
                &record.agent_id,
                &record.user_id,
                &hive_id,
                &record.name,
                &record.description,
                &record.system_prompt,
                &record.model_name,
                &tool_names,
                &declared_tool_names,
                &declared_skill_names,
                &ability_items,
                &record.access_level,
                &record.approval_mode,
                &is_shared,
                &record.status,
                &record.icon,
                &sandbox_container_id,
                &record.created_at,
                &record.updated_at,
                &preset_questions,
                &preset_binding,
                &silent,
                &prefer_mother,
                &preview_skill,
                &visible_unit_ids,
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
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE user_id = $1 AND agent_id = $2",
            &[&cleaned_user, &cleaned_agent],
        )?;
        Ok(row.map(|row| Self::read_user_agent_row(&row)))
    }

    fn get_user_agent_by_id(&self, agent_id: &str) -> Result<Option<UserAgentRecord>> {
        self.ensure_initialized()?;
        let cleaned_agent = agent_id.trim();
        if cleaned_agent.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE agent_id = $1",
            &[&cleaned_agent],
        )?;
        Ok(row.map(|row| Self::read_user_agent_row(&row)))
    }

    fn list_user_agents(&self, user_id: &str) -> Result<Vec<UserAgentRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE user_id = $1 ORDER BY updated_at DESC",
            &[&cleaned_user],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(Self::read_user_agent_row(&row));
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
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE user_id = $1 AND hive_id = $2 ORDER BY updated_at DESC",
            &[&cleaned_user, &normalized_hive_id],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(Self::read_user_agent_row(&row));
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
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE is_shared = 1 AND user_id <> $1 ORDER BY updated_at DESC",
            &[&cleaned_user],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(Self::read_user_agent_row(&row));
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

    fn delete_hive(&self, user_id: &str, hive_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let normalized_hive_id = normalize_hive_id(hive_id);
        if cleaned_user.is_empty() || normalized_hive_id == DEFAULT_HIVE_ID {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM hives WHERE user_id = $1 AND hive_id = $2 AND is_default = 0",
            &[&cleaned_user, &normalized_hive_id],
        )?;
        Ok(affected as i64)
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
            "INSERT INTO team_runs (team_run_id, user_id, hive_id, parent_session_id, parent_agent_id, mother_agent_id, strategy, status, task_total, task_success, task_failed, context_tokens_total, context_tokens_peak, model_round_total, started_time, finished_time, elapsed_s, summary, error, updated_time)              VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20)              ON CONFLICT(team_run_id) DO UPDATE SET user_id = EXCLUDED.user_id, hive_id = EXCLUDED.hive_id, parent_session_id = EXCLUDED.parent_session_id, parent_agent_id = EXCLUDED.parent_agent_id, mother_agent_id = EXCLUDED.mother_agent_id,              strategy = EXCLUDED.strategy, status = EXCLUDED.status, task_total = EXCLUDED.task_total, task_success = EXCLUDED.task_success, task_failed = EXCLUDED.task_failed,              context_tokens_total = EXCLUDED.context_tokens_total, context_tokens_peak = EXCLUDED.context_tokens_peak, model_round_total = EXCLUDED.model_round_total,              started_time = EXCLUDED.started_time, finished_time = EXCLUDED.finished_time, elapsed_s = EXCLUDED.elapsed_s, summary = EXCLUDED.summary, error = EXCLUDED.error, updated_time = EXCLUDED.updated_time",
            &[
                &record.team_run_id,
                &record.user_id,
                &hive_id,
                &record.parent_session_id,
                &record.parent_agent_id,
                &record.mother_agent_id,
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

    fn delete_team_runs_by_hive(&self, user_id: &str, hive_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(0);
        }
        let normalized_hive_id = normalize_hive_id(hive_id);
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM team_runs WHERE user_id = $1 AND hive_id = $2",
            &[&cleaned_user, &normalized_hive_id],
        )?;
        Ok(affected as i64)
    }

    fn get_team_run(&self, team_run_id: &str) -> Result<Option<TeamRunRecord>> {
        self.ensure_initialized()?;
        let cleaned = team_run_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT team_run_id, user_id, hive_id, parent_session_id, parent_agent_id, mother_agent_id, strategy, status, task_total, task_success, task_failed, context_tokens_total, context_tokens_peak, model_round_total, started_time, finished_time, elapsed_s, summary, error, updated_time FROM team_runs WHERE team_run_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| TeamRunRecord {
            team_run_id: row.get(0),
            user_id: row.get(1),
            hive_id: normalize_hive_id(&row.get::<_, String>(2)),
            parent_session_id: row.get(3),
            parent_agent_id: row.get(4),
            mother_agent_id: row.get(5),
            strategy: row.get(6),
            status: row.get(7),
            task_total: row.get(8),
            task_success: row.get(9),
            task_failed: row.get(10),
            context_tokens_total: row.get(11),
            context_tokens_peak: row.get(12),
            model_round_total: row.get(13),
            started_time: row.get(14),
            finished_time: row.get(15),
            elapsed_s: row.get(16),
            summary: row.get(17),
            error: row.get(18),
            updated_time: row.get(19),
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
            "SELECT team_run_id, user_id, hive_id, parent_session_id, parent_agent_id, mother_agent_id, strategy, status, task_total, task_success, task_failed, context_tokens_total, context_tokens_peak, model_round_total, started_time, finished_time, elapsed_s, summary, error, updated_time              FROM team_runs WHERE user_id = $1 AND ($2 = '' OR hive_id = $2) AND ($3 = '' OR parent_session_id = $3)              ORDER BY updated_time DESC LIMIT $4 OFFSET $5",
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
                mother_agent_id: row.get(5),
                strategy: row.get(6),
                status: row.get(7),
                task_total: row.get(8),
                task_success: row.get(9),
                task_failed: row.get(10),
                context_tokens_total: row.get(11),
                context_tokens_peak: row.get(12),
                model_round_total: row.get(13),
                started_time: row.get(14),
                finished_time: row.get(15),
                elapsed_s: row.get(16),
                summary: row.get(17),
                error: row.get(18),
                updated_time: row.get(19),
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
            "SELECT team_run_id, user_id, hive_id, parent_session_id, parent_agent_id, mother_agent_id, strategy, status, task_total, task_success, task_failed, context_tokens_total, context_tokens_peak, model_round_total, started_time, finished_time, elapsed_s, summary, error, updated_time              FROM team_runs WHERE status = ANY($1::text[]) ORDER BY updated_time ASC LIMIT $2 OFFSET $3",
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
                mother_agent_id: row.get(5),
                strategy: row.get(6),
                status: row.get(7),
                task_total: row.get(8),
                task_success: row.get(9),
                task_failed: row.get(10),
                context_tokens_total: row.get(11),
                context_tokens_peak: row.get(12),
                model_round_total: row.get(13),
                started_time: row.get(14),
                finished_time: row.get(15),
                elapsed_s: row.get(16),
                summary: row.get(17),
                error: row.get(18),
                updated_time: row.get(19),
            });
        }
        Ok(output)
    }

    fn upsert_team_task(&self, record: &TeamTaskRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let hive_id = normalize_hive_id(&record.hive_id);
        conn.execute(
            "INSERT INTO team_tasks (task_id, team_run_id, user_id, hive_id, agent_id, target_session_id, spawned_session_id, session_run_id, status, retry_count, priority, started_time, finished_time, elapsed_s, result_summary, error, updated_time)              VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)              ON CONFLICT(task_id) DO UPDATE SET team_run_id = EXCLUDED.team_run_id, user_id = EXCLUDED.user_id, hive_id = EXCLUDED.hive_id, agent_id = EXCLUDED.agent_id,              target_session_id = EXCLUDED.target_session_id, spawned_session_id = EXCLUDED.spawned_session_id, session_run_id = EXCLUDED.session_run_id, status = EXCLUDED.status, retry_count = EXCLUDED.retry_count,              priority = EXCLUDED.priority, started_time = EXCLUDED.started_time, finished_time = EXCLUDED.finished_time, elapsed_s = EXCLUDED.elapsed_s,              result_summary = EXCLUDED.result_summary, error = EXCLUDED.error, updated_time = EXCLUDED.updated_time",
            &[
                &record.task_id,
                &record.team_run_id,
                &record.user_id,
                &hive_id,
                &record.agent_id,
                &record.target_session_id,
                &record.spawned_session_id,
                &record.session_run_id,
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
            "SELECT task_id, team_run_id, user_id, hive_id, agent_id, target_session_id, spawned_session_id, session_run_id, status, retry_count, priority, started_time, finished_time, elapsed_s, result_summary, error, updated_time              FROM team_tasks WHERE team_run_id = $1 ORDER BY updated_time DESC",
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
                session_run_id: row.get(7),
                status: row.get(8),
                retry_count: row.get(9),
                priority: row.get(10),
                started_time: row.get(11),
                finished_time: row.get(12),
                elapsed_s: row.get(13),
                result_summary: row.get(14),
                error: row.get(15),
                updated_time: row.get(16),
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
            "SELECT task_id, team_run_id, user_id, hive_id, agent_id, target_session_id, spawned_session_id, session_run_id, status, retry_count, priority, started_time, finished_time, elapsed_s, result_summary, error, updated_time FROM team_tasks WHERE task_id = $1",
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
            session_run_id: row.get(7),
            status: row.get(8),
            retry_count: row.get(9),
            priority: row.get(10),
            started_time: row.get(11),
            finished_time: row.get(12),
            elapsed_s: row.get(13),
            result_summary: row.get(14),
            error: row.get(15),
            updated_time: row.get(16),
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

    fn prepare_user_token_balance(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
    ) -> Result<Option<UserTokenBalanceStatus>> {
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
            "SELECT token_balance, token_granted_total, token_used_total, last_token_grant_date \
             FROM user_accounts WHERE user_id = $1 FOR UPDATE",
            &[&cleaned],
        )?;
        let Some(row) = row else {
            tx.commit()?;
            return Ok(None);
        };
        let mut balance: i64 = row.get::<_, Option<i64>>(0).unwrap_or(0).max(0);
        let mut granted_total: i64 = row.get::<_, Option<i64>>(1).unwrap_or(0).max(0);
        let used_total: i64 = row.get::<_, Option<i64>>(2).unwrap_or(0).max(0);
        let mut last_grant_date: Option<String> = row.get(3);
        let safe_daily_grant = daily_grant.max(0);
        if safe_daily_grant > 0 && last_grant_date.as_deref() != Some(today) {
            balance = balance.saturating_add(safe_daily_grant);
            granted_total = granted_total.saturating_add(safe_daily_grant);
            last_grant_date = Some(today.to_string());
            tx.execute(
                "UPDATE user_accounts
                 SET token_balance = $1, token_granted_total = $2, last_token_grant_date = $3, updated_at = $4
                 WHERE user_id = $5",
                &[&balance, &granted_total, &last_grant_date, &Self::now_ts(), &cleaned],
            )?;
        }
        tx.commit()?;
        Ok(Some(UserTokenBalanceStatus {
            balance,
            granted_total,
            used_total,
            daily_grant: safe_daily_grant,
            last_grant_date,
            allowed: balance > 0,
            overspent_tokens: 0,
        }))
    }

    fn consume_user_tokens(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
        amount: i64,
    ) -> Result<Option<UserTokenBalanceStatus>> {
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
            "SELECT token_balance, token_granted_total, token_used_total, last_token_grant_date \
             FROM user_accounts WHERE user_id = $1 FOR UPDATE",
            &[&cleaned],
        )?;
        let Some(row) = row else {
            tx.commit()?;
            return Ok(None);
        };
        let mut balance: i64 = row.get::<_, Option<i64>>(0).unwrap_or(0).max(0);
        let mut granted_total: i64 = row.get::<_, Option<i64>>(1).unwrap_or(0).max(0);
        let mut used_total: i64 = row.get::<_, Option<i64>>(2).unwrap_or(0).max(0);
        let mut last_grant_date: Option<String> = row.get(3);
        let safe_daily_grant = daily_grant.max(0);
        if safe_daily_grant > 0 && last_grant_date.as_deref() != Some(today) {
            balance = balance.saturating_add(safe_daily_grant);
            granted_total = granted_total.saturating_add(safe_daily_grant);
            last_grant_date = Some(today.to_string());
        }
        let safe_amount = amount.max(0);
        let charged = balance.min(safe_amount);
        let overspent_tokens = safe_amount.saturating_sub(charged);
        balance = balance.saturating_sub(charged);
        used_total = used_total.saturating_add(safe_amount);
        tx.execute(
            "UPDATE user_accounts
             SET token_balance = $1, token_granted_total = $2, token_used_total = $3, last_token_grant_date = $4, updated_at = $5
             WHERE user_id = $6",
            &[
                &balance,
                &granted_total,
                &used_total,
                &last_grant_date,
                &Self::now_ts(),
                &cleaned,
            ],
        )?;
        tx.commit()?;
        Ok(Some(UserTokenBalanceStatus {
            balance,
            granted_total,
            used_total,
            daily_grant: safe_daily_grant,
            last_grant_date,
            allowed: balance > 0,
            overspent_tokens,
        }))
    }

    fn grant_user_tokens(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
        amount: i64,
        updated_at: f64,
    ) -> Result<Option<UserTokenBalanceStatus>> {
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
            "SELECT token_balance, token_granted_total, token_used_total, last_token_grant_date \
             FROM user_accounts WHERE user_id = $1 FOR UPDATE",
            &[&cleaned],
        )?;
        let Some(row) = row else {
            tx.commit()?;
            return Ok(None);
        };
        let mut balance: i64 = row.get::<_, Option<i64>>(0).unwrap_or(0).max(0);
        let mut granted_total: i64 = row.get::<_, Option<i64>>(1).unwrap_or(0).max(0);
        let used_total: i64 = row.get::<_, Option<i64>>(2).unwrap_or(0).max(0);
        let mut last_grant_date: Option<String> = row.get(3);
        let safe_daily_grant = daily_grant.max(0);
        if safe_daily_grant > 0 && last_grant_date.as_deref() != Some(today) {
            balance = balance.saturating_add(safe_daily_grant);
            granted_total = granted_total.saturating_add(safe_daily_grant);
            last_grant_date = Some(today.to_string());
        }
        let safe_amount = amount.max(0);
        if safe_amount > 0 {
            balance = balance.saturating_add(safe_amount);
            granted_total = granted_total.saturating_add(safe_amount);
            tx.execute(
                "UPDATE user_accounts
                 SET token_balance = $1, token_granted_total = $2, last_token_grant_date = $3, updated_at = $4
                 WHERE user_id = $5",
                &[&balance, &granted_total, &last_grant_date, &updated_at, &cleaned],
            )?;
        } else if safe_daily_grant > 0 && last_grant_date.as_deref() == Some(today) {
            tx.execute(
                "UPDATE user_accounts
                 SET token_balance = $1, token_granted_total = $2, last_token_grant_date = $3, updated_at = $4
                 WHERE user_id = $5",
                &[&balance, &granted_total, &last_grant_date, &updated_at, &cleaned],
            )?;
        }
        tx.commit()?;
        Ok(Some(UserTokenBalanceStatus {
            balance,
            granted_total,
            used_total,
            daily_grant: safe_daily_grant,
            last_grant_date,
            allowed: balance > 0,
            overspent_tokens: 0,
        }))
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
