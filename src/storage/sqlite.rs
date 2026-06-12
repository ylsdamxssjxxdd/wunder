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
use anyhow::Result;
use chrono::{Local, Utc};
use parking_lot::Mutex;
use rusqlite::types::Value as SqlValue;
use rusqlite::{
    params, params_from_iter, Connection, ErrorCode, OptionalExtension, TransactionBehavior,
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

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

use benchmark_store::SqliteBenchmarkStorage;
use bridge_store::SqliteBridgeStorage;
use channel_directory::SqliteChannelDirectoryStorage;
use channel_runtime::SqliteChannelRuntimeStorage;
use chat_session::SqliteChatSessionStorage;
use cron::SqliteCronStorage;
use gateway_store::SqliteGatewayStorage;
use media_store::SqliteMediaStorage;
use memory_store::SqliteMemoryStorage;
use session_goal::SqliteSessionGoalStorage;
use session_run::SqliteSessionRunStorage;
use user_world_store::SqliteUserWorldStorage;

pub struct SqliteStorage {
    db_path: PathBuf,
    initialized: AtomicBool,
    init_guard: Mutex<()>,
}

impl SqliteStorage {
    pub fn new(db_path: String) -> Self {
        let path = if db_path.trim().is_empty() {
            PathBuf::from("./config/data/wunder.db")
        } else {
            PathBuf::from(db_path)
        };
        Self {
            db_path: path,
            initialized: AtomicBool::new(false),
            init_guard: Mutex::new(()),
        }
    }

    fn ensure_db_dir(&self) -> Result<()> {
        if let Some(parent) = self.db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    fn open(&self) -> Result<Connection> {
        self.ensure_db_dir()?;
        let conn = Connection::open(&self.db_path)?;
        // Parallel swarm workers can briefly contend on SQLite writes.
        conn.busy_timeout(Duration::from_secs(5)).ok();
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        conn.pragma_update(None, "synchronous", "NORMAL").ok();
        Ok(conn)
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
        let tool_names = Self::parse_string_list(row.get(7)?);
        Ok(UserAgentRecord {
            agent_id: row.get(0)?,
            user_id: row.get(1)?,
            hive_id: normalize_hive_id(&row.get::<_, String>(2)?),
            name: row.get(3)?,
            description: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
            system_prompt: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
            preview_skill: row.get::<_, Option<i64>>(23)?.unwrap_or(0) != 0,
            model_name: row.get::<_, Option<String>>(6)?,
            ability_items: Self::parse_ability_items(row.get(10)?),
            tool_names: tool_names.clone(),
            declared_tool_names: Self::parse_declared_tool_names(row.get(8)?),
            declared_skill_names: Self::parse_string_list(row.get(9)?),
            visible_unit_ids: Self::parse_string_list(row.get(24)?),
            preset_questions: Self::parse_string_list(row.get(19)?),
            access_level: row.get(11)?,
            approval_mode: row.get(12)?,
            is_shared: row.get::<_, Option<i64>>(13)?.unwrap_or(0) != 0,
            status: row.get(14)?,
            icon: row.get(15)?,
            sandbox_container_id: normalize_sandbox_container_id(
                row.get::<_, Option<i64>>(16)?.unwrap_or(1) as i32,
            ),
            created_at: row.get(17)?,
            updated_at: row.get(18)?,
            preset_binding: Self::parse_preset_binding(row.get(20)?),
            silent: row.get::<_, Option<i64>>(21)?.unwrap_or(0) != 0,
            prefer_mother: row.get::<_, Option<i64>>(22)?.unwrap_or(0) != 0,
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

    fn ensure_user_account_quota_columns(&self, conn: &Connection) -> Result<()> {
        let columns = load_table_columns(conn, "user_accounts")?;
        if columns.is_empty() {
            return Ok(());
        }
        let has_legacy_daily_quota = columns.contains("daily_quota");
        let has_legacy_daily_quota_used = columns.contains("daily_quota_used");
        let has_legacy_daily_quota_date = columns.contains("daily_quota_date");
        if !columns.contains("token_balance") {
            conn.execute(
                "ALTER TABLE user_accounts ADD COLUMN token_balance INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        if !columns.contains("token_granted_total") {
            conn.execute(
                "ALTER TABLE user_accounts ADD COLUMN token_granted_total INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        if !columns.contains("token_used_total") {
            conn.execute(
                "ALTER TABLE user_accounts ADD COLUMN token_used_total INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        if !columns.contains("last_token_grant_date") {
            conn.execute(
                "ALTER TABLE user_accounts ADD COLUMN last_token_grant_date TEXT",
                [],
            )?;
        }
        if !(has_legacy_daily_quota && has_legacy_daily_quota_used && has_legacy_daily_quota_date) {
            return Ok(());
        }
        let today = Local::now().format("%Y-%m-%d").to_string();
        let today_ref = today.as_str();
        conn.execute(
            "UPDATE user_accounts
             SET token_balance = CASE
                     WHEN COALESCE(token_balance, 0) > 0 THEN token_balance
                     WHEN COALESCE(daily_quota_date, '') = ? THEN MAX(COALESCE(daily_quota, 0) - COALESCE(daily_quota_used, 0), 0)
                     ELSE MAX(COALESCE(daily_quota, 0), 0)
                 END,
                 token_granted_total = CASE
                     WHEN COALESCE(token_granted_total, 0) > 0 THEN token_granted_total
                     ELSE MAX(COALESCE(daily_quota, 0), 0)
                 END,
                 token_used_total = CASE
                     WHEN COALESCE(token_used_total, 0) > 0 THEN token_used_total
                     WHEN COALESCE(daily_quota_date, '') = ? THEN MAX(COALESCE(daily_quota_used, 0), 0)
                     ELSE 0
                 END,
                 last_token_grant_date = COALESCE(last_token_grant_date, daily_quota_date)
             WHERE token_balance = 0
                OR token_granted_total = 0
                OR token_used_total = 0
                OR last_token_grant_date IS NULL",
            params![today_ref, today_ref],
        )?;
        Ok(())
    }

    fn ensure_user_account_level_columns(&self, conn: &Connection) -> Result<()> {
        let columns = load_table_columns(conn, "user_accounts")?;
        if columns.is_empty() {
            return Ok(());
        }
        if !columns.contains("experience_total") {
            conn.execute(
                "ALTER TABLE user_accounts ADD COLUMN experience_total INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        Ok(())
    }

    fn ensure_user_account_unit_columns(&self, _conn: &Connection) -> Result<()> {
        Ok(())
    }

    fn ensure_user_account_list_indexes(&self, _conn: &Connection) -> Result<()> {
        Ok(())
    }

    fn ensure_user_token_columns(&self, conn: &Connection) -> Result<()> {
        let columns = load_table_columns(conn, "user_tokens")?;
        if columns.is_empty() {
            return Ok(());
        }
        if !columns.contains("session_scope") {
            conn.execute(
                "ALTER TABLE user_tokens ADD COLUMN session_scope TEXT NOT NULL DEFAULT 'default'",
                [],
            )?;
        }
        conn.execute(
            "UPDATE user_tokens SET session_scope = 'default' WHERE session_scope IS NULL OR trim(session_scope) = ''",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_user_tokens_user_scope_created ON user_tokens (user_id, session_scope, created_at)",
            [],
        )?;
        Ok(())
    }

    fn ensure_user_tool_access_columns(&self, _conn: &Connection) -> Result<()> {
        Ok(())
    }

    fn ensure_chat_session_columns(&self, conn: &Connection) -> Result<()> {
        let columns = load_table_columns(conn, "chat_sessions")?;
        if !columns.contains("status") {
            conn.execute("ALTER TABLE chat_sessions ADD COLUMN status TEXT", [])?;
        }
        Ok(())
    }

    fn ensure_channel_columns(&self, _conn: &Connection) -> Result<()> {
        Ok(())
    }

    fn ensure_session_lock_columns(&self, _conn: &Connection) -> Result<()> {
        Ok(())
    }

    fn ensure_session_run_columns(&self, conn: &Connection) -> Result<()> {
        let columns = load_table_columns(conn, "session_runs")?;
        if columns.is_empty() {
            return Ok(());
        }
        if !columns.contains("dispatch_id") {
            conn.execute("ALTER TABLE session_runs ADD COLUMN dispatch_id TEXT", [])?;
        }
        if !columns.contains("run_kind") {
            conn.execute("ALTER TABLE session_runs ADD COLUMN run_kind TEXT", [])?;
        }
        if !columns.contains("requested_by") {
            conn.execute("ALTER TABLE session_runs ADD COLUMN requested_by TEXT", [])?;
        }
        if !columns.contains("metadata") {
            conn.execute("ALTER TABLE session_runs ADD COLUMN metadata TEXT", [])?;
        }
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_session_runs_dispatch \
             ON session_runs (user_id, dispatch_id, updated_time)",
            [],
        );
        Ok(())
    }

    fn ensure_user_agent_columns(&self, conn: &Connection) -> Result<()> {
        let columns = load_table_columns(conn, "user_agents")?;
        if !columns.contains("model_name") {
            conn.execute("ALTER TABLE user_agents ADD COLUMN model_name TEXT", [])?;
        }
        if !columns.contains("preset_questions") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN preset_questions TEXT",
                [],
            )?;
        }
        if !columns.contains("declared_tool_names") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN declared_tool_names TEXT",
                [],
            )?;
        }
        if !columns.contains("declared_skill_names") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN declared_skill_names TEXT",
                [],
            )?;
        }
        if !columns.contains("visible_unit_ids") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN visible_unit_ids TEXT",
                [],
            )?;
        }
        if !columns.contains("ability_items") {
            conn.execute("ALTER TABLE user_agents ADD COLUMN ability_items TEXT", [])?;
        }
        if !columns.contains("preset_binding") {
            conn.execute("ALTER TABLE user_agents ADD COLUMN preset_binding TEXT", [])?;
        }
        if !columns.contains("silent") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN silent INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        if !columns.contains("prefer_mother") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN prefer_mother INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        if !columns.contains("preview_skill") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN preview_skill INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        Ok(())
    }

    fn ensure_team_run_columns(&self, conn: &Connection) -> Result<()> {
        let columns = load_table_columns(conn, "team_runs")?;
        if !columns.contains("mother_agent_id") {
            conn.execute("ALTER TABLE team_runs ADD COLUMN mother_agent_id TEXT", [])?;
        }
        Ok(())
    }

    fn ensure_team_task_columns(&self, conn: &Connection) -> Result<()> {
        let columns = load_table_columns(conn, "team_tasks")?;
        if !columns.contains("session_run_id") {
            conn.execute("ALTER TABLE team_tasks ADD COLUMN session_run_id TEXT", [])?;
        }
        Ok(())
    }

    fn ensure_user_world_group_columns(&self, _conn: &Connection) -> Result<()> {
        Ok(())
    }

    fn ensure_cron_columns(&self, conn: &Connection) -> Result<()> {
        let columns = load_table_columns(conn, "cron_jobs")?;
        if !columns.contains("consecutive_failures") {
            conn.execute(
                "ALTER TABLE cron_jobs ADD COLUMN consecutive_failures INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        if !columns.contains("auto_disabled_reason") {
            conn.execute(
                "ALTER TABLE cron_jobs ADD COLUMN auto_disabled_reason TEXT",
                [],
            )?;
        }
        if !columns.contains("runner_id") {
            conn.execute("ALTER TABLE cron_jobs ADD COLUMN runner_id TEXT", [])?;
        }
        if !columns.contains("run_token") {
            conn.execute("ALTER TABLE cron_jobs ADD COLUMN run_token TEXT", [])?;
        }
        if !columns.contains("heartbeat_at") {
            conn.execute("ALTER TABLE cron_jobs ADD COLUMN heartbeat_at REAL", [])?;
        }
        if !columns.contains("lease_expires_at") {
            conn.execute("ALTER TABLE cron_jobs ADD COLUMN lease_expires_at REAL", [])?;
        }
        Ok(())
    }

    fn ensure_memory_fragment_columns(&self, conn: &Connection) -> Result<()> {
        let columns = load_table_columns(conn, "memory_fragments")?;
        if columns.is_empty() {
            return Ok(());
        }

        let ensure_column = |name: &str, ddl: &str| -> Result<()> {
            if !columns.contains(name) {
                conn.execute(ddl, [])?;
            }
            Ok(())
        };

        ensure_column(
            "source_round_id",
            "ALTER TABLE memory_fragments ADD COLUMN source_round_id TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(
            "tags",
            "ALTER TABLE memory_fragments ADD COLUMN tags TEXT NOT NULL DEFAULT '[]'",
        )?;
        ensure_column(
            "entities",
            "ALTER TABLE memory_fragments ADD COLUMN entities TEXT NOT NULL DEFAULT '[]'",
        )?;
        ensure_column(
            "importance",
            "ALTER TABLE memory_fragments ADD COLUMN importance REAL NOT NULL DEFAULT 0.6",
        )?;
        ensure_column(
            "confidence",
            "ALTER TABLE memory_fragments ADD COLUMN confidence REAL NOT NULL DEFAULT 0.7",
        )?;
        ensure_column(
            "tier",
            "ALTER TABLE memory_fragments ADD COLUMN tier TEXT NOT NULL DEFAULT 'working'",
        )?;
        ensure_column(
            "pinned",
            "ALTER TABLE memory_fragments ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            "confirmed_by_user",
            "ALTER TABLE memory_fragments ADD COLUMN confirmed_by_user INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            "access_count",
            "ALTER TABLE memory_fragments ADD COLUMN access_count INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            "hit_count",
            "ALTER TABLE memory_fragments ADD COLUMN hit_count INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            "last_accessed_at",
            "ALTER TABLE memory_fragments ADD COLUMN last_accessed_at REAL NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            "valid_from",
            "ALTER TABLE memory_fragments ADD COLUMN valid_from REAL NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            "invalidated_at",
            "ALTER TABLE memory_fragments ADD COLUMN invalidated_at REAL",
        )?;
        ensure_column(
            "supersedes_memory_id",
            "ALTER TABLE memory_fragments ADD COLUMN supersedes_memory_id TEXT",
        )?;
        ensure_column(
            "superseded_by_memory_id",
            "ALTER TABLE memory_fragments ADD COLUMN superseded_by_memory_id TEXT",
        )?;
        ensure_column(
            "embedding_model",
            "ALTER TABLE memory_fragments ADD COLUMN embedding_model TEXT",
        )?;
        ensure_column(
            "vector_ref",
            "ALTER TABLE memory_fragments ADD COLUMN vector_ref TEXT",
        )?;

        let _ = conn.execute(
            "UPDATE memory_fragments SET tags = '[]' WHERE tags IS NULL OR trim(tags) = ''",
            [],
        );
        let _ = conn.execute(
            "UPDATE memory_fragments SET entities = '[]' WHERE entities IS NULL OR trim(entities) = ''",
            [],
        );
        let _ = conn.execute(
            "UPDATE memory_fragments SET valid_from = COALESCE(NULLIF(valid_from, 0), updated_at, created_at, 0)",
            [],
        );
        Ok(())
    }
}

fn load_table_columns(conn: &Connection, table: &str) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    let mut columns = HashSet::new();
    for name in rows.flatten() {
        columns.insert(name);
    }
    Ok(columns)
}

fn append_tool_log_exclusions(filters: &mut Vec<String>, params_list: &mut Vec<SqlValue>) {
    if !TOOL_LOG_EXCLUDED_NAMES.is_empty() {
        let placeholders = vec!["?"; TOOL_LOG_EXCLUDED_NAMES.len()].join(", ");
        filters.push(format!("tool NOT IN ({placeholders})"));
        for name in TOOL_LOG_EXCLUDED_NAMES {
            params_list.push(SqlValue::from(name.to_string()));
        }
    }
    let marker = format!("%{TOOL_LOG_SKILL_READ_MARKER}%");
    filters.push("(data IS NULL OR data NOT LIKE ?)".to_string());
    params_list.push(SqlValue::from(marker));
}

impl StorageBackend for SqliteStorage {
    fn ensure_initialized(&self) -> Result<()> {
        if self.initialized.load(Ordering::SeqCst) {
            return Ok(());
        }
        let _guard = self.init_guard.lock();
        if self.initialized.load(Ordering::SeqCst) {
            return Ok(());
        }
        let conn = self.open()?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS meta (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL,
              updated_time REAL NOT NULL
            );
            CREATE TABLE IF NOT EXISTS chat_history (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              user_id TEXT NOT NULL,
              session_id TEXT NOT NULL,
              role TEXT NOT NULL,
              content TEXT,
              timestamp TEXT,
              meta TEXT,
              payload TEXT NOT NULL,
              created_time REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_chat_history_session
              ON chat_history (user_id, session_id, id);
            CREATE TABLE IF NOT EXISTS model_context_entries (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              user_id TEXT NOT NULL,
              session_id TEXT NOT NULL,
              role TEXT NOT NULL,
              payload TEXT NOT NULL,
              created_time REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_model_context_entries_session
              ON model_context_entries (user_id, session_id, id);
            CREATE TABLE IF NOT EXISTS tool_logs (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              user_id TEXT NOT NULL,
              session_id TEXT NOT NULL,
              tool TEXT,
              ok INTEGER,
              error TEXT,
              args TEXT,
              data TEXT,
              timestamp TEXT,
              payload TEXT NOT NULL,
              created_time REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_tool_logs_session
              ON tool_logs (user_id, session_id, id);
            CREATE TABLE IF NOT EXISTS artifact_logs (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              user_id TEXT NOT NULL,
              session_id TEXT NOT NULL,
              kind TEXT NOT NULL,
              name TEXT,
              payload TEXT NOT NULL,
              created_time REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_artifact_logs_session
              ON artifact_logs (user_id, session_id, id);
            CREATE TABLE IF NOT EXISTS monitor_sessions (
              session_id TEXT PRIMARY KEY,
              user_id TEXT,
              status TEXT,
              updated_time REAL,
              payload TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_monitor_sessions_status
              ON monitor_sessions (status);
            CREATE TABLE IF NOT EXISTS session_locks (
              session_id TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              agent_id TEXT NOT NULL DEFAULT '',
              created_time REAL NOT NULL,
              updated_time REAL NOT NULL,
              expires_at REAL NOT NULL
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
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
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
              retry_count INTEGER NOT NULL DEFAULT 0,
              retry_at REAL NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              started_at REAL,
              finished_at REAL,
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
              event_id INTEGER NOT NULL,
              user_id TEXT NOT NULL,
              payload TEXT NOT NULL,
              created_time REAL NOT NULL,
              PRIMARY KEY (session_id, event_id)
            );
            CREATE TABLE IF NOT EXISTS memory_settings (
              user_id TEXT PRIMARY KEY,
              enabled INTEGER NOT NULL,
              updated_time REAL NOT NULL
            );
            CREATE TABLE IF NOT EXISTS memory_records (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              user_id TEXT NOT NULL,
              session_id TEXT NOT NULL,
              summary TEXT NOT NULL,
              created_time REAL NOT NULL,
              updated_time REAL NOT NULL,
              UNIQUE(user_id, session_id)
            );
            CREATE INDEX IF NOT EXISTS idx_memory_records_user_time
              ON memory_records (user_id, updated_time);
            CREATE TABLE IF NOT EXISTS memory_task_logs (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              task_id TEXT NOT NULL,
              user_id TEXT NOT NULL,
              session_id TEXT NOT NULL,
              status TEXT,
              queued_time REAL,
              started_time REAL,
              finished_time REAL,
              elapsed_s REAL,
              request_payload TEXT,
              result TEXT,
              error TEXT,
              updated_time REAL NOT NULL,
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
              importance REAL NOT NULL,
              confidence REAL NOT NULL,
              tier TEXT NOT NULL,
              status TEXT NOT NULL,
              pinned INTEGER NOT NULL DEFAULT 0,
              confirmed_by_user INTEGER NOT NULL DEFAULT 0,
              access_count INTEGER NOT NULL DEFAULT 0,
              hit_count INTEGER NOT NULL DEFAULT 0,
              last_accessed_at REAL NOT NULL DEFAULT 0,
              valid_from REAL NOT NULL,
              invalidated_at REAL,
              supersedes_memory_id TEXT,
              superseded_by_memory_id TEXT,
              embedding_model TEXT,
              vector_ref TEXT,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
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
              dimensions INTEGER NOT NULL,
              updated_at REAL NOT NULL,
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
              lexical_score REAL NOT NULL,
              semantic_score REAL NOT NULL,
              freshness_score REAL NOT NULL,
              importance_score REAL NOT NULL,
              final_score REAL NOT NULL,
              created_at REAL NOT NULL
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
              queued_at REAL NOT NULL,
              started_at REAL NOT NULL,
              finished_at REAL NOT NULL,
              updated_at REAL NOT NULL
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
              total_score REAL,
              started_time REAL,
              finished_time REAL,
              payload TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_benchmark_runs_user
              ON benchmark_runs (user_id);
            CREATE INDEX IF NOT EXISTS idx_benchmark_runs_status
              ON benchmark_runs (status);
            CREATE INDEX IF NOT EXISTS idx_benchmark_runs_started
              ON benchmark_runs (started_time);
            CREATE TABLE IF NOT EXISTS benchmark_attempts (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              run_id TEXT NOT NULL,
              task_id TEXT NOT NULL,
              attempt_no INTEGER NOT NULL,
              status TEXT,
              final_score REAL,
              started_time REAL,
              finished_time REAL,
              payload TEXT NOT NULL,
              UNIQUE(run_id, task_id, attempt_no)
            );
            CREATE INDEX IF NOT EXISTS idx_benchmark_attempts_run
              ON benchmark_attempts (run_id, task_id, attempt_no);
            CREATE INDEX IF NOT EXISTS idx_benchmark_attempts_status
              ON benchmark_attempts (status);
            CREATE TABLE IF NOT EXISTS benchmark_task_aggregates (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              run_id TEXT NOT NULL,
              task_id TEXT NOT NULL,
              status TEXT,
              mean_score REAL,
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
              token_balance INTEGER NOT NULL DEFAULT 0,
              token_granted_total INTEGER NOT NULL DEFAULT 0,
              token_used_total INTEGER NOT NULL DEFAULT 0,
              last_token_grant_date TEXT,
              experience_total INTEGER NOT NULL DEFAULT 0,
              is_demo INTEGER NOT NULL DEFAULT 0,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              last_login_at REAL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_user_accounts_username
              ON user_accounts (username);
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
              sort_order INTEGER NOT NULL DEFAULT 0,
              leader_ids TEXT NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
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
              sort_order INTEGER NOT NULL DEFAULT 0,
              enabled INTEGER NOT NULL DEFAULT 1,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_external_links_order
              ON external_links (enabled, sort_order, updated_at);
            CREATE TABLE IF NOT EXISTS user_tokens (
              token TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              session_scope TEXT NOT NULL DEFAULT 'default',
              expires_at REAL NOT NULL,
              created_at REAL NOT NULL,
              last_used_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_user_tokens_user
              ON user_tokens (user_id);
            CREATE INDEX IF NOT EXISTS idx_user_tokens_expires
              ON user_tokens (expires_at);
            CREATE TABLE IF NOT EXISTS user_session_scopes (
              user_id TEXT NOT NULL,
              session_scope TEXT NOT NULL,
              last_login_at REAL NOT NULL,
              PRIMARY KEY (user_id, session_scope)
            );
            CREATE TABLE IF NOT EXISTS user_tool_access (
              user_id TEXT PRIMARY KEY,
              allowed_tools TEXT,
              updated_at REAL NOT NULL
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
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              last_message_at REAL NOT NULL
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
              token_budget INTEGER,
              tokens_used INTEGER NOT NULL DEFAULT 0,
              time_used_seconds INTEGER NOT NULL DEFAULT 0,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              completed_at REAL,
              last_continued_at REAL,
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
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              last_message_at REAL NOT NULL,
              last_message_id INTEGER,
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
              announcement_updated_at REAL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_user_world_groups_conversation
              ON user_world_groups (conversation_id);
            CREATE INDEX IF NOT EXISTS idx_user_world_groups_owner
              ON user_world_groups (owner_user_id, updated_at DESC);
            CREATE TABLE IF NOT EXISTS user_world_members (
              conversation_id TEXT NOT NULL,
              user_id TEXT NOT NULL,
              peer_user_id TEXT NOT NULL,
              last_read_message_id INTEGER,
              unread_count_cache INTEGER NOT NULL DEFAULT 0,
              pinned INTEGER NOT NULL DEFAULT 0,
              muted INTEGER NOT NULL DEFAULT 0,
              updated_at REAL NOT NULL,
              PRIMARY KEY (conversation_id, user_id)
            );
            CREATE INDEX IF NOT EXISTS idx_user_world_members_user_updated
              ON user_world_members (user_id, updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_user_world_members_conversation
              ON user_world_members (conversation_id);
            CREATE TABLE IF NOT EXISTS user_world_messages (
              message_id INTEGER PRIMARY KEY AUTOINCREMENT,
              conversation_id TEXT NOT NULL,
              sender_user_id TEXT NOT NULL,
              content TEXT NOT NULL,
              content_type TEXT NOT NULL,
              client_msg_id TEXT,
              created_at REAL NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_user_world_messages_client
              ON user_world_messages (conversation_id, client_msg_id);
            CREATE INDEX IF NOT EXISTS idx_user_world_messages_conversation
              ON user_world_messages (conversation_id, message_id DESC);
            CREATE TABLE IF NOT EXISTS user_world_events (
              conversation_id TEXT NOT NULL,
              event_id INTEGER NOT NULL,
              event_type TEXT NOT NULL,
              payload TEXT NOT NULL,
              created_time REAL NOT NULL,
              PRIMARY KEY (conversation_id, event_id)
            );
            CREATE INDEX IF NOT EXISTS idx_user_world_events_created_time
              ON user_world_events (created_time);
            CREATE INDEX IF NOT EXISTS idx_user_world_events_conversation
              ON user_world_events (conversation_id, event_id);
            CREATE TABLE IF NOT EXISTS beeroom_chat_messages (
              message_id INTEGER PRIMARY KEY AUTOINCREMENT,
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
              created_at REAL NOT NULL
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
              queued_time REAL,
              started_time REAL,
              finished_time REAL,
              elapsed_s REAL,
              result TEXT,
              error TEXT,
              updated_time REAL NOT NULL,
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
              schedule_every_ms INTEGER,
              schedule_cron TEXT,
              schedule_tz TEXT,
              dedupe_key TEXT,
              next_run_at REAL,
              running_at REAL,
              runner_id TEXT,
              run_token TEXT,
              heartbeat_at REAL,
              lease_expires_at REAL,
              last_run_at REAL,
              last_status TEXT,
              last_error TEXT,
              consecutive_failures INTEGER NOT NULL DEFAULT 0,
              auto_disabled_reason TEXT,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
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
              duration_ms INTEGER,
              created_at REAL NOT NULL
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
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
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
              priority INTEGER NOT NULL DEFAULT 0,
              enabled INTEGER NOT NULL DEFAULT 1,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_channel_bindings_match
              ON channel_bindings (channel, account_id, peer_kind, peer_id, priority);
            CREATE TABLE IF NOT EXISTS channel_user_bindings (
              channel TEXT NOT NULL,
              account_id TEXT NOT NULL,
              peer_kind TEXT NOT NULL,
              peer_id TEXT NOT NULL,
              user_id TEXT NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
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
              last_message_at REAL NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              PRIMARY KEY (channel, account_id, peer_kind, peer_id, thread_id)
            );
            CREATE INDEX IF NOT EXISTS idx_channel_sessions_session
              ON channel_sessions (session_id);
            CREATE INDEX IF NOT EXISTS idx_channel_sessions_peer
              ON channel_sessions (channel, account_id, peer_id);
            CREATE TABLE IF NOT EXISTS channel_messages (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
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
              created_at REAL NOT NULL
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
              retry_count INTEGER NOT NULL,
              retry_at REAL NOT NULL,
              last_error TEXT,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              delivered_at REAL
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
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
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
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
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
              first_seen_at REAL NOT NULL,
              last_seen_at REAL NOT NULL,
              last_inbound_at REAL,
              last_outbound_at REAL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
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
              created_at REAL NOT NULL
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
              created_at REAL NOT NULL
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
              connected_at REAL NOT NULL,
              last_seen_at REAL NOT NULL,
              disconnected_at REAL
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
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              last_seen_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_gateway_nodes_status
              ON gateway_nodes (status);
            CREATE TABLE IF NOT EXISTS gateway_node_tokens (
              token TEXT PRIMARY KEY,
              node_id TEXT NOT NULL,
              status TEXT NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              last_used_at REAL
            );
            CREATE INDEX IF NOT EXISTS idx_gateway_node_tokens_node
              ON gateway_node_tokens (node_id, status);
            CREATE TABLE IF NOT EXISTS media_assets (
              asset_id TEXT PRIMARY KEY,
              kind TEXT NOT NULL,
              url TEXT NOT NULL,
              mime TEXT,
              size INTEGER,
              hash TEXT,
              source TEXT,
              created_at REAL NOT NULL
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
              retry_count INTEGER NOT NULL,
              next_retry_at REAL NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
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
              created_time REAL NOT NULL,
              updated_time REAL NOT NULL
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
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
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
              updated_at REAL NOT NULL
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
              task_total INTEGER NOT NULL DEFAULT 0,
              task_success INTEGER NOT NULL DEFAULT 0,
              task_failed INTEGER NOT NULL DEFAULT 0,
              context_tokens_total INTEGER NOT NULL DEFAULT 0,
              context_tokens_peak INTEGER NOT NULL DEFAULT 0,
              model_round_total INTEGER NOT NULL DEFAULT 0,
              started_time REAL,
              finished_time REAL,
              elapsed_s REAL,
              summary TEXT,
              error TEXT,
              updated_time REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_team_runs_user_hive
              ON team_runs (user_id, hive_id, updated_time);
            CREATE INDEX IF NOT EXISTS idx_team_runs_hive_status
              ON team_runs (hive_id, status, updated_time);
            CREATE INDEX IF NOT EXISTS idx_team_runs_hive_parent
              ON team_runs (hive_id, parent_session_id, updated_time);
            CREATE TABLE IF NOT EXISTS team_tasks (
              task_id TEXT PRIMARY KEY,
              team_run_id TEXT NOT NULL,
              user_id TEXT NOT NULL,
              hive_id TEXT NOT NULL,
              agent_id TEXT NOT NULL,
              target_session_id TEXT,
              spawned_session_id TEXT,
              session_run_id TEXT,
              status TEXT NOT NULL,
              retry_count INTEGER NOT NULL DEFAULT 0,
              priority INTEGER NOT NULL DEFAULT 0,
              started_time REAL,
              finished_time REAL,
              elapsed_s REAL,
              result_summary TEXT,
              error TEXT,
              updated_time REAL NOT NULL,
              FOREIGN KEY(team_run_id) REFERENCES team_runs(team_run_id) ON DELETE CASCADE
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
              chunk_size INTEGER NOT NULL,
              chunk_overlap INTEGER NOT NULL,
              chunk_count INTEGER NOT NULL,
              status TEXT NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              content TEXT NOT NULL,
              chunks_json TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_vector_documents_owner_base
              ON vector_documents (owner_id, base_name, updated_at);
            "#,
        )?;
        self.ensure_user_account_quota_columns(&conn)?;
        self.ensure_user_account_level_columns(&conn)?;
        self.ensure_user_account_unit_columns(&conn)?;
        self.ensure_user_account_list_indexes(&conn)?;
        self.ensure_user_token_columns(&conn)?;
        self.ensure_user_tool_access_columns(&conn)?;
        self.ensure_chat_session_columns(&conn)?;
        self.ensure_channel_columns(&conn)?;
        self.ensure_session_lock_columns(&conn)?;
        self.ensure_session_run_columns(&conn)?;
        self.ensure_user_agent_columns(&conn)?;
        self.ensure_team_run_columns(&conn)?;
        self.ensure_team_task_columns(&conn)?;
        self.ensure_user_world_group_columns(&conn)?;
        self.ensure_cron_columns(&conn)?;
        self.ensure_memory_fragment_columns(&conn)?;
        self.initialized.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn get_meta(&self, key: &str) -> Result<Option<String>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?",
                params![key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value)
    }

    fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let now = Self::now_ts();
        conn.execute(
            "INSERT INTO meta (key, value, updated_time) VALUES (?, ?, ?) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_time = excluded.updated_time",
            params![key, value, now],
        )?;
        Ok(())
    }

    fn list_meta_prefix(&self, prefix: &str) -> Result<Vec<(String, String)>> {
        self.ensure_initialized()?;
        let cleaned = prefix.trim();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let pattern = format!("{cleaned}%");
        let mut stmt = conn.prepare(
            "SELECT key, value FROM meta WHERE key LIKE ? ORDER BY updated_time DESC, key ASC",
        )?;
        let rows = stmt.query_map(params![pattern], |row| {
            let key: String = row.get(0)?;
            let value: String = row.get(1)?;
            Ok((key, value))
        })?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    fn delete_meta_prefix(&self, prefix: &str) -> Result<usize> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let pattern = format!("{prefix}%");
        let affected = conn.execute("DELETE FROM meta WHERE key LIKE ?", params![pattern])?;
        Ok(affected)
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
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO chat_history (user_id, session_id, role, content, timestamp, meta, payload, created_time) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                user_id,
                session_id,
                role,
                content,
                timestamp,
                meta,
                payload_text,
                now
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
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO model_context_entries (user_id, session_id, role, payload, created_time) \
             VALUES (?, ?, ?, ?, ?)",
            params![cleaned_user, cleaned_session, role, payload_text, now],
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
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM model_context_entries WHERE user_id = ? AND session_id = ?",
            params![cleaned_user, cleaned_session],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO model_context_entries (user_id, session_id, role, payload, created_time) \
                 VALUES (?, ?, ?, ?, ?)",
            )?;
            for payload in payloads {
                let role = payload
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                if role.is_empty() {
                    continue;
                }
                let payload_text = Self::json_to_string(payload);
                let now = Self::now_ts();
                stmt.execute(params![
                    cleaned_user,
                    cleaned_session,
                    role,
                    payload_text,
                    now
                ])?;
            }
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
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO tool_logs (user_id, session_id, tool, ok, error, args, data, timestamp, payload, created_time) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                user_id,
                session_id,
                tool,
                ok,
                error,
                args,
                data,
                timestamp,
                payload_text,
                now
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
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO artifact_logs (user_id, session_id, kind, name, payload, created_time) \
             VALUES (?, ?, ?, ?, ?, ?)",
            params![user_id, session_id, kind, name, payload_text, now],
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
        let conn = self.open()?;
        let mut rows = if let Some(limit_value) = limit_value {
            let mut stmt = conn.prepare(
                "SELECT payload FROM model_context_entries WHERE user_id = ? AND session_id = ? ORDER BY id DESC LIMIT ?",
            )?;
            let rows = stmt
                .query_map(params![cleaned_user, cleaned_session, limit_value], |row| {
                    row.get::<_, String>(0)
                })?
                .collect::<std::result::Result<Vec<String>, _>>()?;
            rows
        } else {
            let mut stmt = conn.prepare(
                "SELECT payload FROM model_context_entries WHERE user_id = ? AND session_id = ? ORDER BY id ASC",
            )?;
            let rows = stmt
                .query_map(params![cleaned_user, cleaned_session], |row| {
                    row.get::<_, String>(0)
                })?
                .collect::<std::result::Result<Vec<String>, _>>()?;
            rows
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
        let conn = self.open()?;
        let mut rows = if let Some(limit_value) = limit_value {
            let mut stmt = conn.prepare(
                "SELECT payload FROM chat_history WHERE user_id = ? AND session_id = ? ORDER BY id DESC LIMIT ?",
            )?;
            let rows = stmt
                .query_map(params![user_id, session_id, limit_value], |row| {
                    row.get::<_, String>(0)
                })?
                .collect::<std::result::Result<Vec<String>, _>>()?;
            rows
        } else {
            let mut stmt = conn.prepare(
                "SELECT payload FROM chat_history WHERE user_id = ? AND session_id = ? ORDER BY id ASC",
            )?;
            let rows = stmt
                .query_map(params![user_id, session_id], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<String>, _>>()?;
            rows
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
        let conn = self.open()?;
        let mut rows: Vec<(i64, String)> = if let Some(before_id) = before_id {
            let mut stmt = conn.prepare(
                "SELECT id, payload FROM chat_history WHERE user_id = ? AND session_id = ? AND id < ? ORDER BY id DESC LIMIT ?",
            )?;
            let rows = stmt
                .query_map(params![user_id, session_id, before_id, limit], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<std::result::Result<Vec<(i64, String)>, _>>()?;
            rows
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, payload FROM chat_history WHERE user_id = ? AND session_id = ? ORDER BY id DESC LIMIT ?",
            )?;
            let rows = stmt
                .query_map(params![user_id, session_id, limit], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<std::result::Result<Vec<(i64, String)>, _>>()?;
            rows
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
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT id, payload FROM artifact_logs WHERE user_id = ? AND session_id = ? ORDER BY id DESC LIMIT ?",
        )?;
        let mut rows = stmt
            .query_map(params![user_id, session_id, limit], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<(i64, String)>, _>>()?;
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
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT payload FROM chat_history WHERE user_id = ? AND session_id = ? AND role = 'system' ORDER BY id ASC",
        )?;
        let rows = stmt
            .query_map(params![user_id, session_id], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        for payload in rows {
            let Some(value) = Self::json_from_str(&payload) else {
                continue;
            };
            let meta = value.get("meta");
            let Some(meta) = meta.and_then(Value::as_object) else {
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
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT user_id, COUNT(*) as chat_records, MAX(created_time) as last_time FROM chat_history GROUP BY user_id",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, f64>(2).unwrap_or(0.0),
                ))
            })?
            .collect::<std::result::Result<Vec<(String, i64, f64)>, _>>()?;
        let mut stats = HashMap::new();
        for (user_id, count, last_time) in rows {
            let cleaned = user_id.trim();
            if cleaned.is_empty() {
                continue;
            }
            let mut entry = HashMap::new();
            entry.insert("chat_records".to_string(), count);
            entry.insert("last_time".to_string(), last_time.floor() as i64);
            stats.insert(cleaned.to_string(), entry);
        }
        Ok(stats)
    }

    fn get_user_tool_stats(&self) -> Result<HashMap<String, HashMap<String, i64>>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut query = String::from(
            "SELECT user_id, COUNT(*) as tool_records, MAX(created_time) as last_time FROM tool_logs",
        );
        let mut filters = Vec::new();
        let mut params_list: Vec<SqlValue> = Vec::new();
        append_tool_log_exclusions(&mut filters, &mut params_list);
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" GROUP BY user_id");
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params_list.iter()), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, f64>(2).unwrap_or(0.0),
                ))
            })?
            .collect::<std::result::Result<Vec<(String, i64, f64)>, _>>()?;
        let mut stats = HashMap::new();
        for (user_id, count, last_time) in rows {
            let cleaned = user_id.trim();
            if cleaned.is_empty() {
                continue;
            }
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
        let mut params_list: Vec<SqlValue> = Vec::new();
        let mut filters = Vec::new();
        if let Some(since) = since_time.filter(|value| *value > 0.0) {
            filters.push("created_time >= ?".to_string());
            params_list.push(SqlValue::from(since));
        }
        if let Some(until) = until_time.filter(|value| *value > 0.0) {
            filters.push("created_time <= ?".to_string());
            params_list.push(SqlValue::from(until));
        }
        append_tool_log_exclusions(&mut filters, &mut params_list);
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" GROUP BY tool ORDER BY tool_records DESC");
        let conn = self.open()?;
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params_list.iter()), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<Vec<(String, i64)>, _>>()?;
        let mut stats = HashMap::new();
        for (tool, count) in rows {
            let cleaned = tool.trim();
            if cleaned.is_empty() {
                continue;
            }
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
            "SELECT session_id, user_id, COUNT(*) as tool_calls, MAX(created_time) as last_time FROM tool_logs WHERE tool = ?",
        );
        let mut params_list: Vec<SqlValue> = vec![SqlValue::Text(cleaned.to_string())];
        let mut filters = Vec::new();
        if let Some(since) = since_time.filter(|value| *value > 0.0) {
            filters.push("created_time >= ?".to_string());
            params_list.push(SqlValue::from(since));
        }
        if let Some(until) = until_time.filter(|value| *value > 0.0) {
            filters.push("created_time <= ?".to_string());
            params_list.push(SqlValue::from(until));
        }
        append_tool_log_exclusions(&mut filters, &mut params_list);
        if !filters.is_empty() {
            query.push_str(" AND ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" GROUP BY session_id, user_id ORDER BY last_time DESC");
        let conn = self.open()?;
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params_list.iter()), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, f64>(3).unwrap_or(0.0),
                ))
            })?
            .collect::<std::result::Result<Vec<(String, String, i64, f64)>, _>>()?;
        let mut sessions = Vec::new();
        for (session_id, user_id, tool_calls, last_time) in rows {
            let cleaned_session = session_id.trim();
            if cleaned_session.is_empty() {
                continue;
            }
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
        let conn = self.open()?;
        let dbstat_query = "SELECT COALESCE(SUM(pgsize), 0) \
            FROM dbstat WHERE name IN ( \
                'chat_history', \
                'model_context_entries', \
                'tool_logs', \
                'artifact_logs', \
                'monitor_sessions', \
                'stream_events', \
                'memory_task_logs' \
            )";
        if let Ok(total) = conn.query_row(dbstat_query, [], |row| row.get::<_, i64>(0)) {
            return Ok(total.max(0) as u64);
        }
        let total: i64 = conn.query_row(
            "SELECT \
            (SELECT COALESCE(SUM(length(CAST(payload AS BLOB))), 0) FROM chat_history) + \
            (SELECT COALESCE(SUM(length(CAST(payload AS BLOB))), 0) FROM model_context_entries) + \
            (SELECT COALESCE(SUM(length(CAST(payload AS BLOB))), 0) FROM tool_logs) + \
            (SELECT COALESCE(SUM(length(CAST(payload AS BLOB))), 0) FROM artifact_logs) + \
            (SELECT COALESCE(SUM(length(CAST(payload AS BLOB))), 0) FROM monitor_sessions) + \
            (SELECT COALESCE(SUM(length(CAST(payload AS BLOB))), 0) FROM stream_events) + \
            (SELECT COALESCE(SUM( \
                COALESCE(length(CAST(request_payload AS BLOB)), 0) + \
                COALESCE(length(CAST(result AS BLOB)), 0) + \
                COALESCE(length(CAST(error AS BLOB)), 0) \
            ), 0) FROM memory_task_logs)",
            [],
            |row| row.get(0),
        )?;
        Ok(total.max(0) as u64)
    }

    fn delete_chat_history(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM chat_history WHERE user_id = ?",
            params![user_id],
        )?;
        let _ = conn.execute(
            "DELETE FROM model_context_entries WHERE user_id = ?",
            params![user_id],
        );
        Ok(affected as i64)
    }

    fn delete_chat_history_by_session(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM chat_history WHERE user_id = ? AND session_id = ?",
            params![cleaned_user, cleaned_session],
        )?;
        let _ = conn.execute(
            "DELETE FROM model_context_entries WHERE user_id = ? AND session_id = ?",
            params![cleaned_user, cleaned_session],
        );
        Ok(affected as i64)
    }

    fn delete_tool_logs(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let affected = conn.execute("DELETE FROM tool_logs WHERE user_id = ?", params![user_id])?;
        Ok(affected as i64)
    }

    fn delete_tool_logs_by_session(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM tool_logs WHERE user_id = ? AND session_id = ?",
            params![cleaned_user, cleaned_session],
        )?;
        Ok(affected as i64)
    }

    fn delete_artifact_logs(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM artifact_logs WHERE user_id = ?",
            params![user_id],
        )?;
        Ok(affected as i64)
    }

    fn delete_artifact_logs_by_session(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM artifact_logs WHERE user_id = ? AND session_id = ?",
            params![cleaned_user, cleaned_session],
        )?;
        Ok(affected as i64)
    }

    fn upsert_monitor_record(&self, payload: &Value) -> Result<()> {
        self.ensure_initialized()?;
        let session_id = payload
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if session_id.is_empty() {
            return Ok(());
        }
        let user_id = payload
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let status = payload
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let updated_time = payload
            .get("updated_time")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let payload_text = Self::json_to_string(payload);
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO monitor_sessions (session_id, user_id, status, updated_time, payload) VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT(session_id) DO UPDATE SET user_id = excluded.user_id, status = excluded.status, updated_time = excluded.updated_time, payload = excluded.payload",
            params![session_id, user_id, status, updated_time, payload_text],
        )?;
        Ok(())
    }

    fn get_monitor_record(&self, session_id: &str) -> Result<Option<Value>> {
        self.ensure_initialized()?;
        let cleaned = session_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare("SELECT payload FROM monitor_sessions WHERE session_id = ?")?;
        let mut rows = stmt.query([cleaned])?;
        if let Some(row) = rows.next()? {
            let payload: String = row.get(0)?;
            return Ok(Self::json_from_str(&payload));
        }
        Ok(None)
    }

    fn load_monitor_records(&self) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut stmt = conn.prepare("SELECT payload FROM monitor_sessions")?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        let mut records = Vec::new();
        for payload in rows {
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
        let conn = self.open()?;
        let mut stmt = conn
            .prepare("SELECT payload FROM monitor_sessions ORDER BY updated_time DESC LIMIT ?1")?;
        let rows = stmt
            .query_map([limit], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        let mut records = Vec::with_capacity(rows.len());
        for payload in rows {
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
            .collect::<Vec<_>>();
        let since_time = since_time.filter(|value| value.is_finite() && *value > 0.0);

        let mut clauses = vec!["user_id = ?".to_string()];
        let mut params_list: Vec<SqlValue> = vec![SqlValue::from(cleaned_user.to_string())];

        if !statuses.is_empty() {
            let placeholders = std::iter::repeat_n("?", statuses.len())
                .collect::<Vec<_>>()
                .join(", ");
            clauses.push(format!("status IN ({placeholders})"));
            params_list.extend(
                statuses
                    .iter()
                    .map(|value| SqlValue::from((*value).to_string())),
            );
        }
        if let Some(since) = since_time {
            clauses.push("updated_time >= ?".to_string());
            params_list.push(SqlValue::from(since));
        }
        let where_clause = clauses.join(" AND ");
        let sql = format!(
            "SELECT payload FROM monitor_sessions WHERE {where_clause} ORDER BY updated_time DESC LIMIT ?"
        );
        params_list.push(SqlValue::from(limit));
        let conn = self.open()?;
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(params_list.iter()), |row| {
                row.get::<_, String>(0)
            })?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        let mut records = Vec::with_capacity(rows.len());
        for payload in rows {
            if let Some(value) = Self::json_from_str(&payload) {
                records.push(value);
            }
        }
        Ok(records)
    }

    fn delete_monitor_record(&self, session_id: &str) -> Result<()> {
        self.ensure_initialized()?;
        if session_id.trim().is_empty() {
            return Ok(());
        }
        let conn = self.open()?;
        conn.execute(
            "DELETE FROM monitor_sessions WHERE session_id = ?",
            params![session_id],
        )?;
        Ok(())
    }

    fn delete_monitor_records_by_user(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        if user_id.trim().is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM monitor_sessions WHERE user_id = ?",
            params![user_id],
        )?;
        Ok(affected as i64)
    }

    fn try_acquire_session_lock(
        &self,
        session_id: &str,
        user_id: &str,
        agent_id: &str,
        ttl_s: f64,
        max_sessions: i64,
    ) -> Result<SessionLockStatus> {
        self.ensure_initialized()?;
        let cleaned_session = session_id.trim();
        let cleaned_user = user_id.trim();
        let cleaned_agent = agent_id.trim();
        if cleaned_session.is_empty() || cleaned_user.is_empty() {
            return Ok(SessionLockStatus::SystemBusy);
        }
        let max_sessions = max_sessions.max(1);
        let ttl_s = ttl_s.max(1.0);
        let now = Self::now_ts();
        let expires_at = now + ttl_s;
        let mut conn = self.open()?;
        let tx = match conn.transaction_with_behavior(TransactionBehavior::Immediate) {
            Ok(tx) => tx,
            Err(rusqlite::Error::SqliteFailure(err, _))
                if matches!(
                    err.code,
                    ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
                ) =>
            {
                return Ok(SessionLockStatus::SystemBusy)
            }
            Err(err) => return Err(err.into()),
        };
        tx.execute(
            "DELETE FROM session_locks WHERE expires_at <= ?",
            params![now],
        )?;
        let insert = tx.execute(
            "INSERT INTO session_locks (session_id, user_id, agent_id, created_time, updated_time, expires_at) VALUES (?, ?, ?, ?, ?, ?)",
            params![
                cleaned_session,
                cleaned_user,
                cleaned_agent,
                now,
                now,
                expires_at
            ],
        );
        match insert {
            Ok(_) => {
                let total: i64 =
                    tx.query_row("SELECT COUNT(*) FROM session_locks", [], |row| row.get(0))?;
                if total > max_sessions {
                    tx.execute(
                        "DELETE FROM session_locks WHERE session_id = ?",
                        params![cleaned_session],
                    )?;
                    tx.commit()?;
                    return Ok(SessionLockStatus::SystemBusy);
                }
                tx.commit()?;
                Ok(SessionLockStatus::Acquired)
            }
            Err(rusqlite::Error::SqliteFailure(err, _))
                if matches!(err.code, ErrorCode::ConstraintViolation) =>
            {
                tx.commit()?;
                Ok(SessionLockStatus::UserBusy)
            }
            Err(rusqlite::Error::SqliteFailure(err, _))
                if matches!(
                    err.code,
                    ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
                ) =>
            {
                tx.commit()?;
                Ok(SessionLockStatus::SystemBusy)
            }
            Err(err) => Err(err.into()),
        }
    }

    fn touch_session_lock(&self, session_id: &str, ttl_s: f64) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() {
            return Ok(());
        }
        let ttl_s = ttl_s.max(1.0);
        let now = Self::now_ts();
        let expires_at = now + ttl_s;
        let conn = self.open()?;
        conn.execute(
            "UPDATE session_locks SET updated_time = ?, expires_at = ? WHERE session_id = ?",
            params![now, expires_at, cleaned_session],
        )?;
        Ok(())
    }

    fn release_session_lock(&self, session_id: &str) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() {
            return Ok(());
        }
        let conn = self.open()?;
        conn.execute(
            "DELETE FROM session_locks WHERE session_id = ?",
            params![cleaned_session],
        )?;
        Ok(())
    }

    fn delete_session_locks_by_user(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM session_locks WHERE user_id = ?",
            params![cleaned_user],
        )?;
        Ok(affected as i64)
    }

    fn count_session_locks(&self) -> Result<i64> {
        self.ensure_initialized()?;
        let now = Self::now_ts();
        let conn = self.open()?;
        let total = conn.query_row(
            "SELECT COUNT(*) FROM session_locks WHERE expires_at > ?",
            params![now],
            |row| row.get(0),
        )?;
        Ok(total)
    }

    fn list_session_locks_by_user(&self, user_id: &str) -> Result<Vec<SessionLockRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let now = Self::now_ts();
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT session_id, user_id, agent_id, updated_time, expires_at \
             FROM session_locks WHERE user_id = ? AND expires_at > ?",
        )?;
        let rows = stmt
            .query_map(params![cleaned_user, now], |row| {
                Ok(SessionLockRecord {
                    session_id: row.get(0)?,
                    user_id: row.get(1)?,
                    agent_id: row.get(2)?,
                    updated_time: row.get(3)?,
                    expires_at: row.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<SessionLockRecord>, _>>()?;
        Ok(rows)
    }

    fn upsert_agent_thread(&self, record: &AgentThreadRecord) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = record.user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(());
        }
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO agent_threads (thread_id, user_id, agent_id, session_id, status, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(user_id, agent_id) DO UPDATE SET thread_id = excluded.thread_id, session_id = excluded.session_id, \
             status = excluded.status, updated_at = excluded.updated_at",
            params![
                record.thread_id,
                cleaned_user,
                record.agent_id.trim(),
                record.session_id,
                record.status,
                record.created_at,
                record.updated_at
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
        let conn = self.open()?;
        let record = conn
            .query_row(
                "SELECT thread_id, user_id, agent_id, session_id, status, created_at, updated_at \
                 FROM agent_threads WHERE user_id = ? AND agent_id = ? LIMIT 1",
                params![cleaned_user, cleaned_agent],
                |row| {
                    Ok(AgentThreadRecord {
                        thread_id: row.get(0)?,
                        user_id: row.get(1)?,
                        agent_id: row.get(2)?,
                        session_id: row.get(3)?,
                        status: row.get(4)?,
                        created_at: row.get(5)?,
                        updated_at: row.get(6)?,
                    })
                },
            )
            .optional()?;
        Ok(record)
    }

    fn delete_agent_thread(&self, user_id: &str, agent_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(0);
        }
        let cleaned_agent = agent_id.trim();
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM agent_threads WHERE user_id = ? AND agent_id = ?",
            params![cleaned_user, cleaned_agent],
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
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO agent_tasks (task_id, thread_id, user_id, agent_id, session_id, status, request_payload, request_id, retry_count, retry_at, created_at, updated_at, started_at, finished_at, last_error) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(task_id) DO UPDATE SET status = excluded.status, request_payload = excluded.request_payload, \
             request_id = excluded.request_id, retry_count = excluded.retry_count, retry_at = excluded.retry_at, updated_at = excluded.updated_at, \
             started_at = excluded.started_at, finished_at = excluded.finished_at, last_error = excluded.last_error",
            params![
                record.task_id,
                record.thread_id,
                cleaned_user,
                record.agent_id.trim(),
                record.session_id,
                record.status,
                payload,
                record.request_id.as_deref(),
                record.retry_count,
                record.retry_at,
                record.created_at,
                record.updated_at,
                record.started_at,
                record.finished_at,
                record.last_error.as_deref()
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
        let conn = self.open()?;
        let record = conn
            .query_row(
                "SELECT task_id, thread_id, user_id, agent_id, session_id, status, request_payload, request_id, retry_count, retry_at, created_at, updated_at, started_at, finished_at, last_error \
                 FROM agent_tasks WHERE task_id = ? LIMIT 1",
                params![cleaned],
                |row| {
                    let raw_payload: String = row.get(6)?;
                    let payload = serde_json::from_str(&raw_payload).unwrap_or(Value::Null);
                    Ok(AgentTaskRecord {
                        task_id: row.get(0)?,
                        thread_id: row.get(1)?,
                        user_id: row.get(2)?,
                        agent_id: row.get(3)?,
                        session_id: row.get(4)?,
                        status: row.get(5)?,
                        request_payload: payload,
                        request_id: row.get(7)?,
                        retry_count: row.get(8)?,
                        retry_at: row.get(9)?,
                        created_at: row.get(10)?,
                        updated_at: row.get(11)?,
                        started_at: row.get(12)?,
                        finished_at: row.get(13)?,
                        last_error: row.get(14)?,
                    })
                },
            )
            .optional()?;
        Ok(record)
    }

    fn list_pending_agent_tasks(&self, limit: i64) -> Result<Vec<AgentTaskRecord>> {
        self.ensure_initialized()?;
        let now = Self::now_ts();
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT task_id, thread_id, user_id, agent_id, session_id, status, request_payload, request_id, retry_count, retry_at, created_at, updated_at, started_at, finished_at, last_error \
             FROM agent_tasks WHERE (status = 'pending' OR status = 'retry') AND retry_at <= ? \
             ORDER BY retry_at ASC, created_at ASC LIMIT ?",
        )?;
        let rows = stmt
            .query_map(params![now, limit.max(1)], |row| {
                let raw_payload: String = row.get(6)?;
                let payload = serde_json::from_str(&raw_payload).unwrap_or(Value::Null);
                Ok(AgentTaskRecord {
                    task_id: row.get(0)?,
                    thread_id: row.get(1)?,
                    user_id: row.get(2)?,
                    agent_id: row.get(3)?,
                    session_id: row.get(4)?,
                    status: row.get(5)?,
                    request_payload: payload,
                    request_id: row.get(7)?,
                    retry_count: row.get(8)?,
                    retry_at: row.get(9)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                    started_at: row.get(12)?,
                    finished_at: row.get(13)?,
                    last_error: row.get(14)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn count_pending_agent_tasks(&self) -> Result<i64> {
        self.ensure_initialized()?;
        let now = Self::now_ts();
        let conn = self.open()?;
        let total = conn.query_row(
            "SELECT COUNT(*) FROM agent_tasks WHERE (status = 'pending' OR status = 'retry') AND retry_at <= ?",
            params![now],
            |row| row.get(0),
        )?;
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
        let conn = self.open()?;
        let total = conn.query_row(
            "SELECT COUNT(*) FROM agent_tasks \
             WHERE (status = 'pending' OR status = 'retry') AND retry_at <= ? \
               AND (retry_at < ? OR (retry_at = ? AND created_at < ?) OR (retry_at = ? AND created_at = ? AND task_id < ?))",
            params![now, retry_at, retry_at, created_at, retry_at, created_at, task_id],
            |row| row.get(0),
        )?;
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
        let conn = self.open()?;
        let (query, params): (String, Vec<rusqlite::types::Value>) = if let Some(status) =
            status.filter(|value| !value.trim().is_empty())
        {
            (
                "SELECT task_id, thread_id, user_id, agent_id, session_id, status, request_payload, request_id, retry_count, retry_at, created_at, updated_at, started_at, finished_at, last_error \
                 FROM agent_tasks WHERE thread_id = ? AND status = ? ORDER BY created_at DESC LIMIT ?"
                    .to_string(),
                vec![
                    rusqlite::types::Value::from(cleaned_thread.to_string()),
                    rusqlite::types::Value::from(status.trim().to_string()),
                    rusqlite::types::Value::from(limit.max(1)),
                ],
            )
        } else {
            (
                "SELECT task_id, thread_id, user_id, agent_id, session_id, status, request_payload, request_id, retry_count, retry_at, created_at, updated_at, started_at, finished_at, last_error \
                 FROM agent_tasks WHERE thread_id = ? ORDER BY created_at DESC LIMIT ?"
                    .to_string(),
                vec![
                    rusqlite::types::Value::from(cleaned_thread.to_string()),
                    rusqlite::types::Value::from(limit.max(1)),
                ],
            )
        };
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params.iter()), |row| {
                let raw_payload: String = row.get(6)?;
                let payload = serde_json::from_str(&raw_payload).unwrap_or(Value::Null);
                Ok(AgentTaskRecord {
                    task_id: row.get(0)?,
                    thread_id: row.get(1)?,
                    user_id: row.get(2)?,
                    agent_id: row.get(3)?,
                    session_id: row.get(4)?,
                    status: row.get(5)?,
                    request_payload: payload,
                    request_id: row.get(7)?,
                    retry_count: row.get(8)?,
                    retry_at: row.get(9)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                    started_at: row.get(12)?,
                    finished_at: row.get(13)?,
                    last_error: row.get(14)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn update_agent_task_status(&self, params: UpdateAgentTaskStatusParams<'_>) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = params.task_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let conn = self.open()?;
        conn.execute(
            "UPDATE agent_tasks SET status = ?, retry_count = ?, retry_at = ?, started_at = ?, finished_at = ?, last_error = ?, updated_at = ? WHERE task_id = ?",
            params![
                params.status,
                params.retry_count,
                params.retry_at,
                params.started_at,
                params.finished_at,
                params.last_error,
                params.updated_at,
                cleaned
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
        let conn = self.open()?;
        let value: Option<i64> = conn.query_row(
            "SELECT MAX(event_id) FROM stream_events WHERE session_id = ?",
            params![cleaned_session],
            |row| row.get::<_, Option<i64>>(0),
        )?;
        Ok(value.unwrap_or(0))
    }

    fn append_stream_event(
        &self,
        session_id: &str,
        user_id: &str,
        event_id: i64,
        payload: &Value,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_session = session_id.trim();
        let cleaned_user = user_id.trim();
        if cleaned_session.is_empty() || cleaned_user.is_empty() {
            return Ok(());
        }
        let now = Self::now_ts();
        let payload_text = Self::json_to_string(payload);
        let conn = self.open()?;
        conn.execute(
            "INSERT OR REPLACE INTO stream_events (session_id, event_id, user_id, payload, created_time) VALUES (?, ?, ?, ?, ?)",
            params![cleaned_session, event_id, cleaned_user, payload_text, now],
        )?;
        Ok(())
    }

    fn load_stream_events(
        &self,
        session_id: &str,
        after_event_id: i64,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() || limit <= 0 {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT event_id, payload FROM stream_events WHERE session_id = ? AND event_id > ? ORDER BY event_id ASC LIMIT ?",
        )?;
        let rows = stmt
            .query_map(params![cleaned_session, after_event_id, limit], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<(i64, String)>, _>>()?;
        let mut records = Vec::new();
        for (event_id, payload) in rows {
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

    fn load_recent_stream_events(&self, session_id: &str, limit: i64) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() || limit <= 0 {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT event_id, payload FROM stream_events WHERE session_id = ? ORDER BY event_id DESC LIMIT ?",
        )?;
        let mut rows = stmt
            .query_map(params![cleaned_session, limit], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<(i64, String)>, _>>()?;
        rows.reverse();
        let mut records = Vec::new();
        for (event_id, payload) in rows {
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

    fn delete_stream_events_before(&self, before_time: f64) -> Result<i64> {
        self.ensure_initialized()?;
        if before_time <= 0.0 {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM stream_events WHERE created_time < ?",
            params![before_time],
        )?;
        Ok(affected as i64)
    }

    fn delete_stream_events_by_user(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM stream_events WHERE user_id = ?",
            params![cleaned_user],
        )?;
        Ok(affected as i64)
    }

    fn delete_stream_events_by_session(&self, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM stream_events WHERE session_id = ?",
            params![cleaned_session],
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
    fn cleanup_retention(&self, retention_days: i64) -> Result<HashMap<String, i64>> {
        self.ensure_initialized()?;
        if retention_days <= 0 {
            return Ok(HashMap::new());
        }
        let cutoff = Self::now_ts() - (retention_days as f64 * 86400.0);
        if cutoff <= 0.0 {
            return Ok(HashMap::new());
        }
        let conn = self.open()?;
        let mut results = HashMap::new();
        let mut admin_ids = Vec::new();
        let mut stmt = conn.prepare("SELECT user_id, roles FROM user_accounts")?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })?
            .collect::<std::result::Result<Vec<(String, Option<String>)>, _>>()?;
        for (user_id, roles_raw) in rows {
            let roles = Self::parse_string_list(roles_raw);
            if roles
                .iter()
                .any(|role| role == "admin" || role == "super_admin")
            {
                admin_ids.push(user_id);
            }
        }
        let hookup_delete = |table: &str, time_field: &str, allow_null_user: bool| -> Result<i64> {
            let mut sql = format!("DELETE FROM {table} WHERE {time_field} < ?");
            let mut params: Vec<SqlValue> = vec![SqlValue::from(cutoff)];
            if !admin_ids.is_empty() {
                let placeholders = vec!["?"; admin_ids.len()].join(", ");
                if allow_null_user {
                    sql.push_str(&format!(
                        " AND (user_id IS NULL OR user_id NOT IN ({placeholders}))"
                    ));
                } else {
                    sql.push_str(&format!(" AND user_id NOT IN ({placeholders})"));
                }
                for user_id in &admin_ids {
                    params.push(SqlValue::from(user_id.clone()));
                }
            }
            Ok(conn.execute(&sql, params_from_iter(params))? as i64)
        };
        let chat = hookup_delete("chat_history", "created_time", false)?;
        results.insert("chat_history".to_string(), chat);
        let model_context = hookup_delete("model_context_entries", "created_time", false)?;
        results.insert("model_context_entries".to_string(), model_context);
        let tool = hookup_delete("tool_logs", "created_time", false)?;
        results.insert("tool_logs".to_string(), tool);
        let artifact = hookup_delete("artifact_logs", "created_time", false)?;
        results.insert("artifact_logs".to_string(), artifact);
        let monitor = hookup_delete("monitor_sessions", "COALESCE(updated_time, 0)", true)?;
        results.insert("monitor_sessions".to_string(), monitor);
        let stream = hookup_delete("stream_events", "created_time", false)?;
        results.insert("stream_events".to_string(), stream);
        let session_runs = hookup_delete("session_runs", "COALESCE(updated_time, 0)", false)?;
        results.insert("session_runs".to_string(), session_runs);
        Ok(results)
    }

    fn upsert_user_account(&self, record: &UserAccountRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let roles = Self::string_list_to_json(&record.roles);
        conn.execute(
            "INSERT INTO user_accounts (user_id, username, email, password_hash, roles, status, access_level, unit_id, \
             token_balance, token_granted_total, token_used_total, last_token_grant_date, experience_total, is_demo, created_at, updated_at, last_login_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(user_id) DO UPDATE SET username = excluded.username, email = excluded.email, password_hash = excluded.password_hash, \
             roles = excluded.roles, status = excluded.status, access_level = excluded.access_level, unit_id = excluded.unit_id, \
             token_balance = excluded.token_balance, token_granted_total = excluded.token_granted_total, token_used_total = excluded.token_used_total, \
             last_token_grant_date = excluded.last_token_grant_date, \
             experience_total = excluded.experience_total, \
             is_demo = excluded.is_demo, created_at = excluded.created_at, updated_at = excluded.updated_at, last_login_at = excluded.last_login_at",
            params![
                record.user_id,
                record.username,
                record.email,
                record.password_hash,
                roles,
                record.status,
                record.access_level,
                record.unit_id,
                record.token_balance,
                record.token_granted_total,
                record.token_used_total,
                record.last_token_grant_date,
                record.experience_total,
                if record.is_demo { 1 } else { 0 },
                record.created_at,
                record.updated_at,
                record.last_login_at
            ],
        )?;
        Ok(())
    }

    fn get_user_account(&self, user_id: &str) -> Result<Option<UserAccountRecord>> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
                 experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts WHERE user_id = ?",
                params![cleaned],
                |row| {
                    Ok(UserAccountRecord {
                        user_id: row.get(0)?,
                        username: row.get(1)?,
                        email: row.get(2)?,
                        password_hash: row.get(3)?,
                        roles: Self::parse_string_list(row.get::<_, Option<String>>(4)?),
                        status: row.get(5)?,
                        access_level: row.get(6)?,
                        unit_id: row.get(7)?,
                        token_balance: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                        token_granted_total: row.get::<_, Option<i64>>(9)?.unwrap_or(0),
                        token_used_total: row.get::<_, Option<i64>>(10)?.unwrap_or(0),
                        last_token_grant_date: row.get(11)?,
                        experience_total: row.get::<_, Option<i64>>(12)?.unwrap_or(0),
                        is_demo: row.get::<_, i64>(13)? != 0,
                        created_at: row.get(14)?,
                        updated_at: row.get(15)?,
                        last_login_at: row.get(16)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn get_user_account_by_username(&self, username: &str) -> Result<Option<UserAccountRecord>> {
        self.ensure_initialized()?;
        let cleaned = username.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
                 experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts WHERE username = ?",
                params![cleaned],
                |row| {
                    Ok(UserAccountRecord {
                        user_id: row.get(0)?,
                        username: row.get(1)?,
                        email: row.get(2)?,
                        password_hash: row.get(3)?,
                        roles: Self::parse_string_list(row.get::<_, Option<String>>(4)?),
                        status: row.get(5)?,
                        access_level: row.get(6)?,
                        unit_id: row.get(7)?,
                        token_balance: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                        token_granted_total: row.get::<_, Option<i64>>(9)?.unwrap_or(0),
                        token_used_total: row.get::<_, Option<i64>>(10)?.unwrap_or(0),
                        last_token_grant_date: row.get(11)?,
                        experience_total: row.get::<_, Option<i64>>(12)?.unwrap_or(0),
                        is_demo: row.get::<_, i64>(13)? != 0,
                        created_at: row.get(14)?,
                        updated_at: row.get(15)?,
                        last_login_at: row.get(16)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn get_user_account_by_email(&self, email: &str) -> Result<Option<UserAccountRecord>> {
        self.ensure_initialized()?;
        let cleaned = email.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
                 experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts WHERE email = ?",
                params![cleaned],
                |row| {
                    Ok(UserAccountRecord {
                        user_id: row.get(0)?,
                        username: row.get(1)?,
                        email: row.get(2)?,
                        password_hash: row.get(3)?,
                        roles: Self::parse_string_list(row.get::<_, Option<String>>(4)?),
                        status: row.get(5)?,
                        access_level: row.get(6)?,
                        unit_id: row.get(7)?,
                        token_balance: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                        token_granted_total: row.get::<_, Option<i64>>(9)?.unwrap_or(0),
                        token_used_total: row.get::<_, Option<i64>>(10)?.unwrap_or(0),
                        last_token_grant_date: row.get(11)?,
                        experience_total: row.get::<_, Option<i64>>(12)?.unwrap_or(0),
                        is_demo: row.get::<_, i64>(13)? != 0,
                        created_at: row.get(14)?,
                        updated_at: row.get(15)?,
                        last_login_at: row.get(16)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn list_user_accounts(
        &self,
        keyword: Option<&str>,
        unit_ids: Option<&[String]>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserAccountRecord>, i64)> {
        self.ensure_initialized()?;
        let mut conditions = Vec::new();
        let mut params_list: Vec<SqlValue> = Vec::new();
        if let Some(keyword) = keyword {
            let cleaned = keyword.trim();
            if !cleaned.is_empty() {
                let pattern = format!("%{cleaned}%");
                conditions.push("(username LIKE ? OR email LIKE ?)".to_string());
                params_list.push(SqlValue::from(pattern.clone()));
                params_list.push(SqlValue::from(pattern));
            }
        }
        if let Some(unit_ids) = unit_ids.filter(|ids| !ids.is_empty()) {
            let placeholders = std::iter::repeat_n("?", unit_ids.len())
                .collect::<Vec<_>>()
                .join(", ");
            conditions.push(format!("unit_id IN ({placeholders})"));
            for unit_id in unit_ids {
                params_list.push(SqlValue::from(unit_id.clone()));
            }
        }
        let mut count_sql = String::from("SELECT COUNT(*) FROM user_accounts");
        if !conditions.is_empty() {
            count_sql.push_str(" WHERE ");
            count_sql.push_str(&conditions.join(" AND "));
        }
        let conn = self.open()?;
        let total: i64 =
            conn.query_row(&count_sql, params_from_iter(params_list.iter()), |row| {
                row.get(0)
            })?;

        let mut sql = String::from(
            "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
             experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts",
        );
        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
        sql.push_str(" ORDER BY created_at DESC");
        if limit > 0 {
            sql.push_str(" LIMIT ? OFFSET ?");
            params_list.push(SqlValue::from(limit));
            params_list.push(SqlValue::from(offset.max(0)));
        }
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(params_list.iter()), |row| {
                Ok(UserAccountRecord {
                    user_id: row.get(0)?,
                    username: row.get(1)?,
                    email: row.get(2)?,
                    password_hash: row.get(3)?,
                    roles: Self::parse_string_list(row.get::<_, Option<String>>(4)?),
                    status: row.get(5)?,
                    access_level: row.get(6)?,
                    unit_id: row.get(7)?,
                    token_balance: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                    token_granted_total: row.get::<_, Option<i64>>(9)?.unwrap_or(0),
                    token_used_total: row.get::<_, Option<i64>>(10)?.unwrap_or(0),
                    last_token_grant_date: row.get(11)?,
                    experience_total: row.get::<_, Option<i64>>(12)?.unwrap_or(0),
                    is_demo: row.get::<_, i64>(13)? != 0,
                    created_at: row.get(14)?,
                    updated_at: row.get(15)?,
                    last_login_at: row.get(16)?,
                })
            })?
            .collect::<std::result::Result<Vec<UserAccountRecord>, _>>()?;
        Ok((rows, total))
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
        let conn = self.open()?;
        let previous_total = conn
            .query_row(
                "SELECT experience_total FROM user_accounts WHERE user_id = ?",
                params![cleaned],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten()
            .unwrap_or(0)
            .max(0);
        let safe_delta = delta.max(0);
        if safe_delta > 0 {
            conn.execute(
                "UPDATE user_accounts \
                 SET experience_total = COALESCE(experience_total, 0) + ?, updated_at = ? \
                 WHERE user_id = ?",
                params![safe_delta, updated_at, cleaned],
            )?;
        }
        let total = conn
            .query_row(
                "SELECT experience_total FROM user_accounts WHERE user_id = ?",
                params![cleaned],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten()
            .unwrap_or(0);
        Ok(UserExperienceUpdateResult {
            previous_total,
            current_total: total.max(0),
        })
    }

    fn delete_user_account(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM user_accounts WHERE user_id = ?",
            params![cleaned],
        )?;
        Ok(affected as i64)
    }

    fn list_org_units(&self) -> Result<Vec<OrgUnitRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT unit_id, parent_id, name, level, path, path_name, sort_order, leader_ids, created_at, updated_at \
             FROM org_units ORDER BY path, sort_order, name",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(OrgUnitRecord {
                    unit_id: row.get(0)?,
                    parent_id: row.get(1)?,
                    name: row.get(2)?,
                    level: row.get(3)?,
                    path: row.get(4)?,
                    path_name: row.get(5)?,
                    sort_order: row.get(6)?,
                    leader_ids: Self::parse_string_list(row.get::<_, Option<String>>(7)?),
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                })
            })?
            .collect::<std::result::Result<Vec<OrgUnitRecord>, _>>()?;
        Ok(rows)
    }

    fn get_org_unit(&self, unit_id: &str) -> Result<Option<OrgUnitRecord>> {
        self.ensure_initialized()?;
        let cleaned = unit_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT unit_id, parent_id, name, level, path, path_name, sort_order, leader_ids, created_at, updated_at \
                 FROM org_units WHERE unit_id = ?",
                params![cleaned],
                |row| {
                    Ok(OrgUnitRecord {
                        unit_id: row.get(0)?,
                        parent_id: row.get(1)?,
                        name: row.get(2)?,
                        level: row.get(3)?,
                        path: row.get(4)?,
                        path_name: row.get(5)?,
                        sort_order: row.get(6)?,
                        leader_ids: Self::parse_string_list(row.get::<_, Option<String>>(7)?),
                        created_at: row.get(8)?,
                        updated_at: row.get(9)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn upsert_org_unit(&self, record: &OrgUnitRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let leader_ids = Self::string_list_to_json(&record.leader_ids);
        conn.execute(
            "INSERT INTO org_units (unit_id, parent_id, name, level, path, path_name, sort_order, leader_ids, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(unit_id) DO UPDATE SET parent_id = excluded.parent_id, name = excluded.name, level = excluded.level, \
             path = excluded.path, path_name = excluded.path_name, sort_order = excluded.sort_order, leader_ids = excluded.leader_ids, \
             updated_at = excluded.updated_at",
            params![
                record.unit_id,
                record.parent_id,
                record.name,
                record.level,
                record.path,
                record.path_name,
                record.sort_order,
                leader_ids,
                record.created_at,
                record.updated_at
            ],
        )?;
        Ok(())
    }

    fn upsert_user_accounts(&self, records: &[UserAccountRecord]) -> Result<()> {
        self.ensure_initialized()?;
        if records.is_empty() {
            return Ok(());
        }
        let mut conn = self.open()?;
        let tx = conn.transaction()?;
        for record in records {
            let roles = Self::string_list_to_json(&record.roles);
            tx.execute(
                "INSERT INTO user_accounts (user_id, username, email, password_hash, roles, status, access_level, unit_id, \
                 token_balance, token_granted_total, token_used_total, last_token_grant_date, experience_total, is_demo, created_at, updated_at, last_login_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
                 ON CONFLICT(user_id) DO UPDATE SET username = excluded.username, email = excluded.email, password_hash = excluded.password_hash, \
                 roles = excluded.roles, status = excluded.status, access_level = excluded.access_level, unit_id = excluded.unit_id, \
                 token_balance = excluded.token_balance, token_granted_total = excluded.token_granted_total, token_used_total = excluded.token_used_total, \
                 last_token_grant_date = excluded.last_token_grant_date, \
                 experience_total = excluded.experience_total, \
                 is_demo = excluded.is_demo, created_at = excluded.created_at, updated_at = excluded.updated_at, last_login_at = excluded.last_login_at",
                params![
                    record.user_id,
                    record.username,
                    record.email,
                    record.password_hash,
                    roles,
                    record.status,
                    record.access_level,
                    record.unit_id,
                    record.token_balance,
                    record.token_granted_total,
                    record.token_used_total,
                    record.last_token_grant_date,
                    record.experience_total,
                    if record.is_demo { 1 } else { 0 },
                    record.created_at,
                    record.updated_at,
                    record.last_login_at
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn delete_org_unit(&self, unit_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = unit_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute("DELETE FROM org_units WHERE unit_id = ?", params![cleaned])?;
        Ok(affected as i64)
    }

    fn upsert_external_link(&self, record: &ExternalLinkRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let allowed_levels = Self::i32_list_to_json(&record.allowed_levels);
        conn.execute(
            "INSERT INTO external_links (link_id, title, description, url, icon, allowed_levels, sort_order, enabled, created_at, updated_at) \n             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \n             ON CONFLICT(link_id) DO UPDATE SET title = excluded.title, description = excluded.description, \n             url = excluded.url, icon = excluded.icon, allowed_levels = excluded.allowed_levels, \n             sort_order = excluded.sort_order, enabled = excluded.enabled, updated_at = excluded.updated_at",
            params![
                record.link_id,
                record.title,
                record.description,
                record.url,
                record.icon,
                allowed_levels,
                record.sort_order,
                if record.enabled { 1 } else { 0 },
                record.created_at,
                record.updated_at,
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT link_id, title, description, url, icon, allowed_levels, sort_order, enabled, created_at, updated_at \n                 FROM external_links WHERE link_id = ?",
                params![cleaned],
                |row| {
                    Ok(ExternalLinkRecord {
                        link_id: row.get(0)?,
                        title: row.get(1)?,
                        description: row.get(2)?,
                        url: row.get(3)?,
                        icon: row.get(4)?,
                        allowed_levels: Self::parse_i32_list(row.get::<_, Option<String>>(5)?),
                        sort_order: row.get(6)?,
                        enabled: row.get::<_, i64>(7)? != 0,
                        created_at: row.get(8)?,
                        updated_at: row.get(9)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn list_external_links(&self, include_disabled: bool) -> Result<Vec<ExternalLinkRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let sql = if include_disabled {
            "SELECT link_id, title, description, url, icon, allowed_levels, sort_order, enabled, created_at, updated_at \n             FROM external_links ORDER BY sort_order ASC, updated_at DESC, link_id ASC"
        } else {
            "SELECT link_id, title, description, url, icon, allowed_levels, sort_order, enabled, created_at, updated_at \n             FROM external_links WHERE enabled = 1 ORDER BY sort_order ASC, updated_at DESC, link_id ASC"
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ExternalLinkRecord {
                    link_id: row.get(0)?,
                    title: row.get(1)?,
                    description: row.get(2)?,
                    url: row.get(3)?,
                    icon: row.get(4)?,
                    allowed_levels: Self::parse_i32_list(row.get::<_, Option<String>>(5)?),
                    sort_order: row.get(6)?,
                    enabled: row.get::<_, i64>(7)? != 0,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                })
            })?
            .collect::<std::result::Result<Vec<ExternalLinkRecord>, _>>()?;
        Ok(rows)
    }

    fn delete_external_link(&self, link_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = link_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM external_links WHERE link_id = ?",
            params![cleaned],
        )?;
        Ok(affected as i64)
    }

    fn create_user_token(&self, record: &UserTokenRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO user_tokens (token, user_id, session_scope, expires_at, created_at, last_used_at) VALUES (?, ?, ?, ?, ?, ?)",
            params![
                record.token,
                record.user_id,
                record.session_scope,
                record.expires_at,
                record.created_at,
                record.last_used_at
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT token, user_id, session_scope, expires_at, created_at, last_used_at FROM user_tokens WHERE token = ?",
                params![cleaned],
                |row| {
                    Ok(UserTokenRecord {
                        token: row.get(0)?,
                        user_id: row.get(1)?,
                        session_scope: row.get(2)?,
                        expires_at: row.get(3)?,
                        created_at: row.get(4)?,
                        last_used_at: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn touch_user_token(&self, token: &str, last_used_at: f64) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = token.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let conn = self.open()?;
        conn.execute(
            "UPDATE user_tokens SET last_used_at = ? WHERE token = ?",
            params![last_used_at, cleaned],
        )?;
        Ok(())
    }

    fn delete_user_token(&self, token: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = token.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute("DELETE FROM user_tokens WHERE token = ?", params![cleaned])?;
        Ok(affected as i64)
    }

    fn upsert_user_session_scope(&self, record: &UserSessionScopeRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO user_session_scopes (user_id, session_scope, last_login_at) VALUES (?, ?, ?)
             ON CONFLICT(user_id, session_scope) DO UPDATE SET last_login_at = excluded.last_login_at",
            params![record.user_id, record.session_scope, record.last_login_at],
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT user_id, session_scope, last_login_at FROM user_session_scopes WHERE user_id = ? AND session_scope = ?",
                params![cleaned_user_id, cleaned_scope],
                |row| {
                    Ok(UserSessionScopeRecord {
                        user_id: row.get(0)?,
                        session_scope: row.get(1)?,
                        last_login_at: row.get(2)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
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
        let conn = self.open()?;
        let row: Option<(Option<String>, f64)> = conn
            .query_row(
                "SELECT allowed_tools, updated_at FROM user_tool_access WHERE user_id = ?",
                params![cleaned],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some(raw) = row else {
            return Ok(None);
        };
        let allowed_tools = raw
            .0
            .map(|value| Self::parse_string_list(Some(value)))
            .filter(|items| !items.is_empty());
        Ok(Some(UserToolAccessRecord {
            user_id: cleaned.to_string(),
            allowed_tools,
            updated_at: raw.1,
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
        let conn = self.open()?;
        let normalized_allowed_tools = allowed_tools.filter(|items| !items.is_empty());
        if normalized_allowed_tools.is_some() {
            let payload = normalized_allowed_tools
                .map(|value| Self::string_list_to_json(value))
                .unwrap_or_else(|| "[]".to_string());
            let now = Self::now_ts();
            conn.execute(
                "INSERT INTO user_tool_access (user_id, allowed_tools, updated_at) VALUES (?, ?, ?) \
                 ON CONFLICT(user_id) DO UPDATE SET allowed_tools = excluded.allowed_tools, updated_at = excluded.updated_at",
                params![cleaned, payload, now],
            )?;
        } else {
            conn.execute(
                "DELETE FROM user_tool_access WHERE user_id = ?",
                params![cleaned],
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
        let conn = self.open()?;
        let row: Option<(Option<String>, Option<String>, f64)> = conn
            .query_row(
                "SELECT allowed_agent_ids, blocked_agent_ids, updated_at FROM user_agent_access WHERE user_id = ?",
                params![cleaned],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some(raw) = row else {
            return Ok(None);
        };
        Ok(Some(UserAgentAccessRecord {
            user_id: cleaned.to_string(),
            allowed_agent_ids: raw.0.map(|value| Self::parse_string_list(Some(value))),
            blocked_agent_ids: Self::parse_string_list(raw.1),
            updated_at: raw.2,
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
        let conn = self.open()?;
        if allowed_agent_ids.is_some() || blocked_agent_ids.is_some() {
            let allowed_payload = allowed_agent_ids
                .map(|value| Self::string_list_to_json(value))
                .unwrap_or_else(|| "[]".to_string());
            let blocked_payload = blocked_agent_ids
                .map(|value| Self::string_list_to_json(value))
                .unwrap_or_else(|| "[]".to_string());
            let now = Self::now_ts();
            conn.execute(
                "INSERT INTO user_agent_access (user_id, allowed_agent_ids, blocked_agent_ids, updated_at) VALUES (?, ?, ?, ?) \
                 ON CONFLICT(user_id) DO UPDATE SET allowed_agent_ids = excluded.allowed_agent_ids, blocked_agent_ids = excluded.blocked_agent_ids, updated_at = excluded.updated_at",
                params![cleaned, allowed_payload, blocked_payload, now],
            )?;
        } else {
            conn.execute(
                "DELETE FROM user_agent_access WHERE user_id = ?",
                params![cleaned],
            )?;
        }
        Ok(())
    }

    fn upsert_user_agent(&self, record: &UserAgentRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
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
        let hive_id = normalize_hive_id(&record.hive_id);
        let preview_skill = if record.preview_skill { 1 } else { 0 };
        conn.execute(
            "INSERT INTO user_agents (agent_id, user_id, hive_id, name, description, system_prompt, preview_skill, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, visible_unit_ids) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(agent_id) DO UPDATE SET user_id = excluded.user_id, hive_id = excluded.hive_id, name = excluded.name, description = excluded.description, \
             system_prompt = excluded.system_prompt, preview_skill = excluded.preview_skill, model_name = excluded.model_name, tool_names = excluded.tool_names, declared_tool_names = excluded.declared_tool_names, declared_skill_names = excluded.declared_skill_names, ability_items = excluded.ability_items, access_level = excluded.access_level, approval_mode = excluded.approval_mode, \
             is_shared = excluded.is_shared, status = excluded.status, icon = excluded.icon, sandbox_container_id = excluded.sandbox_container_id, updated_at = excluded.updated_at, preset_questions = excluded.preset_questions, preset_binding = excluded.preset_binding, silent = excluded.silent, prefer_mother = excluded.prefer_mother, visible_unit_ids = excluded.visible_unit_ids",
            params![
                record.agent_id,
                record.user_id,
                hive_id,
                record.name,
                record.description,
                record.system_prompt,
                preview_skill,
                record.model_name,
                tool_names,
                declared_tool_names,
                declared_skill_names,
                ability_items,
                record.access_level,
                record.approval_mode,
                if record.is_shared { 1 } else { 0 },
                record.status,
                record.icon,
                normalize_sandbox_container_id(record.sandbox_container_id),
                record.created_at,
                record.updated_at,
                preset_questions,
                preset_binding,
                if record.silent { 1 } else { 0 },
                if record.prefer_mother { 1 } else { 0 },
                visible_unit_ids
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT agent_id, user_id, hive_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE user_id = ? AND agent_id = ?",
                params![cleaned_user, cleaned_agent],
                Self::read_user_agent_row,
            )
            .optional()?;
        Ok(row)
    }

    fn get_user_agent_by_id(&self, agent_id: &str) -> Result<Option<UserAgentRecord>> {
        self.ensure_initialized()?;
        let cleaned_agent = agent_id.trim();
        if cleaned_agent.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT agent_id, user_id, hive_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE agent_id = ?",
                params![cleaned_agent],
                Self::read_user_agent_row,
            )
            .optional()?;
        Ok(row)
    }

    fn list_user_agents(&self, user_id: &str) -> Result<Vec<UserAgentRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE user_id = ? ORDER BY updated_at DESC",
        )?;
        let rows = stmt
            .query_map(params![cleaned_user], Self::read_user_agent_row)?
            .collect::<std::result::Result<Vec<UserAgentRecord>, _>>()?;
        Ok(rows)
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
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE user_id = ? AND hive_id = ? ORDER BY updated_at DESC",
        )?;
        let rows = stmt
            .query_map(
                params![cleaned_user, normalized_hive_id],
                Self::read_user_agent_row,
            )?
            .collect::<std::result::Result<Vec<UserAgentRecord>, _>>()?;
        Ok(rows)
    }

    fn list_shared_user_agents(&self, user_id: &str) -> Result<Vec<UserAgentRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE is_shared = 1 AND user_id <> ? ORDER BY updated_at DESC",
        )?;
        let rows = stmt
            .query_map(params![cleaned_user], Self::read_user_agent_row)?
            .collect::<std::result::Result<Vec<UserAgentRecord>, _>>()?;
        Ok(rows)
    }

    fn delete_user_agent(&self, user_id: &str, agent_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_agent = agent_id.trim();
        if cleaned_user.is_empty() || cleaned_agent.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM user_agents WHERE user_id = ? AND agent_id = ?",
            params![cleaned_user, cleaned_agent],
        )?;
        Ok(affected as i64)
    }

    fn upsert_hive(&self, record: &HiveRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let hive_id = normalize_hive_id(&record.hive_id);
        conn.execute(
            "INSERT INTO hives (hive_id, user_id, name, description, is_default, status, created_time, updated_time)              VALUES (?, ?, ?, ?, ?, ?, ?, ?)              ON CONFLICT(hive_id) DO UPDATE SET user_id = excluded.user_id, name = excluded.name, description = excluded.description,              is_default = excluded.is_default, status = excluded.status, updated_time = excluded.updated_time",
            params![
                hive_id,
                record.user_id,
                record.name,
                record.description,
                if record.is_default { 1 } else { 0 },
                record.status,
                record.created_time,
                record.updated_time,
            ],
        )?;
        Ok(())
    }

    fn get_hive(&self, user_id: &str, hive_id: &str) -> Result<Option<HiveRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let normalized_hive_id = normalize_hive_id(hive_id);
        if cleaned_user.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT hive_id, user_id, name, description, is_default, status, created_time, updated_time                  FROM hives WHERE user_id = ? AND hive_id = ?",
                params![cleaned_user, normalized_hive_id],
                |row| {
                    let is_default: Option<i64> = row.get(4)?;
                    Ok(HiveRecord {
                        hive_id: normalize_hive_id(&row.get::<_, String>(0)?),
                        user_id: row.get(1)?,
                        name: row.get(2)?,
                        description: row.get(3)?,
                        is_default: is_default.unwrap_or(0) != 0,
                        status: row.get(5)?,
                        created_time: row.get(6)?,
                        updated_time: row.get(7)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn list_hives(&self, user_id: &str, include_archived: bool) -> Result<Vec<HiveRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut sql = String::from(
            "SELECT hive_id, user_id, name, description, is_default, status, created_time, updated_time              FROM hives WHERE user_id = ?",
        );
        if !include_archived {
            sql.push_str(" AND status <> 'archived'");
        }
        sql.push_str(" ORDER BY is_default DESC, updated_time DESC");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![cleaned_user], |row| {
                let is_default: Option<i64> = row.get(4)?;
                Ok(HiveRecord {
                    hive_id: normalize_hive_id(&row.get::<_, String>(0)?),
                    user_id: row.get(1)?,
                    name: row.get(2)?,
                    description: row.get(3)?,
                    is_default: is_default.unwrap_or(0) != 0,
                    status: row.get(5)?,
                    created_time: row.get(6)?,
                    updated_time: row.get(7)?,
                })
            })?
            .collect::<std::result::Result<Vec<HiveRecord>, _>>()?;
        Ok(rows)
    }

    fn delete_hive(&self, user_id: &str, hive_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let normalized_hive_id = normalize_hive_id(hive_id);
        if cleaned_user.is_empty() || normalized_hive_id == DEFAULT_HIVE_ID {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM hives WHERE user_id = ? AND hive_id = ? AND is_default = 0",
            params![cleaned_user, normalized_hive_id],
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
        let normalized_hive_id = normalize_hive_id(hive_id);
        let conn = self.open()?;
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
        let placeholders = std::iter::repeat_n("?", cleaned_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "UPDATE user_agents SET hive_id = ?, updated_at = ? WHERE user_id = ? AND agent_id IN ({placeholders})"
        );
        let now = Self::now_ts();
        let mut values: Vec<SqlValue> = Vec::with_capacity(cleaned_ids.len() + 3);
        values.push(SqlValue::from(normalized_hive_id));
        values.push(SqlValue::from(now));
        values.push(SqlValue::from(cleaned_user.to_string()));
        for agent_id in cleaned_ids {
            values.push(SqlValue::from(agent_id));
        }
        let affected = conn.execute(&sql, params_from_iter(values))?;
        Ok(affected as i64)
    }

    fn upsert_team_run(&self, record: &TeamRunRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO team_runs (team_run_id, user_id, hive_id, parent_session_id, parent_agent_id, mother_agent_id, strategy, status, task_total, task_success, task_failed, context_tokens_total, context_tokens_peak, model_round_total, started_time, finished_time, elapsed_s, summary, error, updated_time)              VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)              ON CONFLICT(team_run_id) DO UPDATE SET user_id = excluded.user_id, hive_id = excluded.hive_id, parent_session_id = excluded.parent_session_id, parent_agent_id = excluded.parent_agent_id, mother_agent_id = excluded.mother_agent_id,              strategy = excluded.strategy, status = excluded.status, task_total = excluded.task_total, task_success = excluded.task_success, task_failed = excluded.task_failed,              context_tokens_total = excluded.context_tokens_total, context_tokens_peak = excluded.context_tokens_peak, model_round_total = excluded.model_round_total,              started_time = excluded.started_time, finished_time = excluded.finished_time, elapsed_s = excluded.elapsed_s, summary = excluded.summary, error = excluded.error, updated_time = excluded.updated_time",
            params![
                record.team_run_id,
                record.user_id,
                normalize_hive_id(&record.hive_id),
                record.parent_session_id,
                record.parent_agent_id,
                record.mother_agent_id,
                record.strategy,
                record.status,
                record.task_total,
                record.task_success,
                record.task_failed,
                record.context_tokens_total,
                record.context_tokens_peak,
                record.model_round_total,
                record.started_time,
                record.finished_time,
                record.elapsed_s,
                record.summary,
                record.error,
                record.updated_time,
            ],
        )?;
        Ok(())
    }

    fn delete_team_runs_by_hive(&self, user_id: &str, hive_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let normalized_hive_id = normalize_hive_id(hive_id);
        if cleaned_user.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM team_runs WHERE user_id = ? AND hive_id = ?",
            params![cleaned_user, normalized_hive_id],
        )?;
        Ok(affected as i64)
    }

    fn get_team_run(&self, team_run_id: &str) -> Result<Option<TeamRunRecord>> {
        self.ensure_initialized()?;
        let cleaned = team_run_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT team_run_id, user_id, hive_id, parent_session_id, parent_agent_id, mother_agent_id, strategy, status, task_total, task_success, task_failed, context_tokens_total, context_tokens_peak, model_round_total, started_time, finished_time, elapsed_s, summary, error, updated_time FROM team_runs WHERE team_run_id = ?",
                params![cleaned],
                |row| {
                    Ok(TeamRunRecord {
                        team_run_id: row.get(0)?,
                        user_id: row.get(1)?,
                        hive_id: normalize_hive_id(&row.get::<_, String>(2)?),
                        parent_session_id: row.get(3)?,
                        parent_agent_id: row.get(4)?,
                        mother_agent_id: row.get(5)?,
                        strategy: row.get(6)?,
                        status: row.get(7)?,
                        task_total: row.get(8)?,
                        task_success: row.get(9)?,
                        task_failed: row.get(10)?,
                        context_tokens_total: row.get(11)?,
                        context_tokens_peak: row.get(12)?,
                        model_round_total: row.get(13)?,
                        started_time: row.get(14)?,
                        finished_time: row.get(15)?,
                        elapsed_s: row.get(16)?,
                        summary: row.get(17)?,
                        error: row.get(18)?,
                        updated_time: row.get(19)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
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
        let conn = self.open()?;
        let mut filters = vec!["user_id = ?".to_string()];
        let mut values: Vec<SqlValue> = vec![SqlValue::from(cleaned_user.to_string())];
        if let Some(hive_id) = hive_id {
            filters.push("hive_id = ?".to_string());
            values.push(SqlValue::from(normalize_hive_id(hive_id)));
        }
        if let Some(parent_session_id) = parent_session_id.map(str::trim).filter(|v| !v.is_empty())
        {
            filters.push("parent_session_id = ?".to_string());
            values.push(SqlValue::from(parent_session_id.to_string()));
        }
        let where_clause = filters.join(" AND ");
        let count_sql = format!("SELECT COUNT(1) FROM team_runs WHERE {where_clause}");
        let total = conn.query_row(&count_sql, params_from_iter(values.clone()), |row| {
            row.get::<_, i64>(0)
        })?;

        let mut query_values = values;
        query_values.push(SqlValue::from(offset.max(0)));
        query_values.push(SqlValue::from(limit.max(1)));
        let query_sql = format!(
            "SELECT team_run_id, user_id, hive_id, parent_session_id, parent_agent_id, mother_agent_id, strategy, status, task_total, task_success, task_failed, context_tokens_total, context_tokens_peak, model_round_total, started_time, finished_time, elapsed_s, summary, error, updated_time              FROM team_runs WHERE {where_clause} ORDER BY updated_time DESC LIMIT ? OFFSET ?"
        );
        let mut stmt = conn.prepare(&query_sql)?;
        let rows = stmt
            .query_map(params_from_iter(query_values), |row| {
                Ok(TeamRunRecord {
                    team_run_id: row.get(0)?,
                    user_id: row.get(1)?,
                    hive_id: normalize_hive_id(&row.get::<_, String>(2)?),
                    parent_session_id: row.get(3)?,
                    parent_agent_id: row.get(4)?,
                    mother_agent_id: row.get(5)?,
                    strategy: row.get(6)?,
                    status: row.get(7)?,
                    task_total: row.get(8)?,
                    task_success: row.get(9)?,
                    task_failed: row.get(10)?,
                    context_tokens_total: row.get(11)?,
                    context_tokens_peak: row.get(12)?,
                    model_round_total: row.get(13)?,
                    started_time: row.get(14)?,
                    finished_time: row.get(15)?,
                    elapsed_s: row.get(16)?,
                    summary: row.get(17)?,
                    error: row.get(18)?,
                    updated_time: row.get(19)?,
                })
            })?
            .collect::<std::result::Result<Vec<TeamRunRecord>, _>>()?;
        Ok((rows, total))
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

        let conn = self.open()?;
        let placeholders = vec!["?"; cleaned_statuses.len()].join(",");
        let query_sql = format!(
            "SELECT team_run_id, user_id, hive_id, parent_session_id, parent_agent_id, mother_agent_id, strategy, status, task_total, task_success, task_failed, context_tokens_total, context_tokens_peak, model_round_total, started_time, finished_time, elapsed_s, summary, error, updated_time              FROM team_runs WHERE status IN ({placeholders}) ORDER BY updated_time ASC LIMIT ? OFFSET ?"
        );
        let mut values = cleaned_statuses
            .into_iter()
            .map(SqlValue::from)
            .collect::<Vec<_>>();
        values.push(SqlValue::from(limit.max(1)));
        values.push(SqlValue::from(offset.max(0)));

        let mut stmt = conn.prepare(&query_sql)?;
        let rows = stmt
            .query_map(params_from_iter(values), |row| {
                Ok(TeamRunRecord {
                    team_run_id: row.get(0)?,
                    user_id: row.get(1)?,
                    hive_id: normalize_hive_id(&row.get::<_, String>(2)?),
                    parent_session_id: row.get(3)?,
                    parent_agent_id: row.get(4)?,
                    mother_agent_id: row.get(5)?,
                    strategy: row.get(6)?,
                    status: row.get(7)?,
                    task_total: row.get(8)?,
                    task_success: row.get(9)?,
                    task_failed: row.get(10)?,
                    context_tokens_total: row.get(11)?,
                    context_tokens_peak: row.get(12)?,
                    model_round_total: row.get(13)?,
                    started_time: row.get(14)?,
                    finished_time: row.get(15)?,
                    elapsed_s: row.get(16)?,
                    summary: row.get(17)?,
                    error: row.get(18)?,
                    updated_time: row.get(19)?,
                })
            })?
            .collect::<std::result::Result<Vec<TeamRunRecord>, _>>()?;
        Ok(rows)
    }

    fn upsert_team_task(&self, record: &TeamTaskRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO team_tasks (task_id, team_run_id, user_id, hive_id, agent_id, target_session_id, spawned_session_id, session_run_id, status, retry_count, priority, started_time, finished_time, elapsed_s, result_summary, error, updated_time)              VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)              ON CONFLICT(task_id) DO UPDATE SET team_run_id = excluded.team_run_id, user_id = excluded.user_id, hive_id = excluded.hive_id, agent_id = excluded.agent_id,              target_session_id = excluded.target_session_id, spawned_session_id = excluded.spawned_session_id, session_run_id = excluded.session_run_id, status = excluded.status, retry_count = excluded.retry_count,              priority = excluded.priority, started_time = excluded.started_time, finished_time = excluded.finished_time, elapsed_s = excluded.elapsed_s,              result_summary = excluded.result_summary, error = excluded.error, updated_time = excluded.updated_time",
            params![
                record.task_id,
                record.team_run_id,
                record.user_id,
                normalize_hive_id(&record.hive_id),
                record.agent_id,
                record.target_session_id,
                record.spawned_session_id,
                record.session_run_id,
                record.status,
                record.retry_count,
                record.priority,
                record.started_time,
                record.finished_time,
                record.elapsed_s,
                record.result_summary,
                record.error,
                record.updated_time,
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
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT task_id, team_run_id, user_id, hive_id, agent_id, target_session_id, spawned_session_id, session_run_id, status, retry_count, priority, started_time, finished_time, elapsed_s, result_summary, error, updated_time              FROM team_tasks WHERE team_run_id = ? ORDER BY updated_time DESC",
        )?;
        let rows = stmt
            .query_map(params![cleaned_run_id], |row| {
                Ok(TeamTaskRecord {
                    task_id: row.get(0)?,
                    team_run_id: row.get(1)?,
                    user_id: row.get(2)?,
                    hive_id: normalize_hive_id(&row.get::<_, String>(3)?),
                    agent_id: row.get(4)?,
                    target_session_id: row.get(5)?,
                    spawned_session_id: row.get(6)?,
                    session_run_id: row.get(7)?,
                    status: row.get(8)?,
                    retry_count: row.get(9)?,
                    priority: row.get(10)?,
                    started_time: row.get(11)?,
                    finished_time: row.get(12)?,
                    elapsed_s: row.get(13)?,
                    result_summary: row.get(14)?,
                    error: row.get(15)?,
                    updated_time: row.get(16)?,
                })
            })?
            .collect::<std::result::Result<Vec<TeamTaskRecord>, _>>()?;
        Ok(rows)
    }

    fn get_team_task(&self, task_id: &str) -> Result<Option<TeamTaskRecord>> {
        self.ensure_initialized()?;
        let cleaned = task_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT task_id, team_run_id, user_id, hive_id, agent_id, target_session_id, spawned_session_id, session_run_id, status, retry_count, priority, started_time, finished_time, elapsed_s, result_summary, error, updated_time FROM team_tasks WHERE task_id = ?",
                params![cleaned],
                |row| {
                    Ok(TeamTaskRecord {
                        task_id: row.get(0)?,
                        team_run_id: row.get(1)?,
                        user_id: row.get(2)?,
                        hive_id: normalize_hive_id(&row.get::<_, String>(3)?),
                        agent_id: row.get(4)?,
                        target_session_id: row.get(5)?,
                        spawned_session_id: row.get(6)?,
                        session_run_id: row.get(7)?,
                        status: row.get(8)?,
                        retry_count: row.get(9)?,
                        priority: row.get(10)?,
                        started_time: row.get(11)?,
                        finished_time: row.get(12)?,
                        elapsed_s: row.get(13)?,
                        result_summary: row.get(14)?,
                        error: row.get(15)?,
                        updated_time: row.get(16)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }
    fn upsert_vector_document(&self, record: &VectorDocumentRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO vector_documents \
             (doc_id, owner_id, base_name, doc_name, embedding_model, chunk_size, chunk_overlap, chunk_count, status, created_at, updated_at, content, chunks_json) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(doc_id) DO UPDATE SET \
             owner_id = excluded.owner_id, \
             base_name = excluded.base_name, \
             doc_name = excluded.doc_name, \
             embedding_model = excluded.embedding_model, \
             chunk_size = excluded.chunk_size, \
             chunk_overlap = excluded.chunk_overlap, \
             chunk_count = excluded.chunk_count, \
             status = excluded.status, \
             created_at = excluded.created_at, \
             updated_at = excluded.updated_at, \
             content = excluded.content, \
             chunks_json = excluded.chunks_json",
            params![
                record.doc_id,
                record.owner_id,
                record.base_name,
                record.doc_name,
                record.embedding_model,
                record.chunk_size,
                record.chunk_overlap,
                record.chunk_count,
                record.status,
                record.created_at,
                record.updated_at,
                record.content,
                record.chunks_json
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT doc_id, owner_id, base_name, doc_name, embedding_model, chunk_size, chunk_overlap, chunk_count, status, created_at, updated_at, content, chunks_json \
                 FROM vector_documents WHERE doc_id = ? AND owner_id = ? AND base_name = ?",
                params![doc_id, owner_id, base_name],
                |row| {
                    Ok(VectorDocumentRecord {
                        doc_id: row.get(0)?,
                        owner_id: row.get(1)?,
                        base_name: row.get(2)?,
                        doc_name: row.get(3)?,
                        embedding_model: row.get(4)?,
                        chunk_size: row.get::<_, i64>(5)?,
                        chunk_overlap: row.get::<_, i64>(6)?,
                        chunk_count: row.get::<_, i64>(7)?,
                        status: row.get(8)?,
                        created_at: row.get(9)?,
                        updated_at: row.get(10)?,
                        content: row.get(11)?,
                        chunks_json: row.get(12)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn list_vector_document_summaries(
        &self,
        owner_id: &str,
        base_name: &str,
    ) -> Result<Vec<VectorDocumentSummaryRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT doc_id, doc_name, status, chunk_count, embedding_model, updated_at \
             FROM vector_documents WHERE owner_id = ? AND base_name = ? \
             ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map(params![owner_id, base_name], |row| {
            Ok(VectorDocumentSummaryRecord {
                doc_id: row.get(0)?,
                doc_name: row.get(1)?,
                status: row.get(2)?,
                chunk_count: row.get::<_, i64>(3)?,
                embedding_model: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        let mut output = Vec::new();
        for item in rows.flatten() {
            output.push(item);
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
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM vector_documents WHERE doc_id = ? AND owner_id = ? AND base_name = ?",
            params![doc_id, owner_id, base_name],
        )?;
        Ok(affected > 0)
    }

    fn delete_vector_documents_by_base(&self, owner_id: &str, base_name: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM vector_documents WHERE owner_id = ? AND base_name = ?",
            params![owner_id, base_name],
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
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row = tx
            .query_row(
                "SELECT token_balance, token_granted_total, token_used_total, last_token_grant_date FROM user_accounts WHERE user_id = ?",
                params![cleaned],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((raw_balance, raw_granted_total, raw_used_total, raw_last_grant_date)) = row
        else {
            tx.commit()?;
            return Ok(None);
        };
        let mut balance = raw_balance.max(0);
        let mut granted_total = raw_granted_total.max(0);
        let used_total = raw_used_total.max(0);
        let mut last_grant_date = raw_last_grant_date;
        let safe_daily_grant = daily_grant.max(0);
        let should_grant = safe_daily_grant > 0 && last_grant_date.as_deref() != Some(today);
        if should_grant {
            balance = balance.saturating_add(safe_daily_grant);
            granted_total = granted_total.saturating_add(safe_daily_grant);
            last_grant_date = Some(today.to_string());
            tx.execute(
                "UPDATE user_accounts
                 SET token_balance = ?, token_granted_total = ?, last_token_grant_date = ?, updated_at = ?
                 WHERE user_id = ?",
                params![balance, granted_total, last_grant_date, Self::now_ts(), cleaned],
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
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row = tx
            .query_row(
                "SELECT token_balance, token_granted_total, token_used_total, last_token_grant_date FROM user_accounts WHERE user_id = ?",
                params![cleaned],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((raw_balance, raw_granted_total, raw_used_total, raw_last_grant_date)) = row
        else {
            tx.commit()?;
            return Ok(None);
        };
        let safe_daily_grant = daily_grant.max(0);
        let mut balance = raw_balance.max(0);
        let mut granted_total = raw_granted_total.max(0);
        let mut used_total = raw_used_total.max(0);
        let mut last_grant_date = raw_last_grant_date;
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
             SET token_balance = ?, token_granted_total = ?, token_used_total = ?, last_token_grant_date = ?, updated_at = ?
             WHERE user_id = ?",
            params![
                balance,
                granted_total,
                used_total,
                last_grant_date,
                Self::now_ts(),
                cleaned
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
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row = tx
            .query_row(
                "SELECT token_balance, token_granted_total, token_used_total, last_token_grant_date FROM user_accounts WHERE user_id = ?",
                params![cleaned],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((raw_balance, raw_granted_total, raw_used_total, raw_last_grant_date)) = row
        else {
            tx.commit()?;
            return Ok(None);
        };
        let safe_daily_grant = daily_grant.max(0);
        let mut balance = raw_balance.max(0);
        let mut granted_total = raw_granted_total.max(0);
        let used_total = raw_used_total.max(0);
        let mut last_grant_date = raw_last_grant_date;
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
                 SET token_balance = ?, token_granted_total = ?, last_token_grant_date = ?, updated_at = ?
                 WHERE user_id = ?",
                params![balance, granted_total, last_grant_date, updated_at, cleaned],
            )?;
        } else if safe_daily_grant > 0 && last_grant_date.as_deref() == Some(today) {
            tx.execute(
                "UPDATE user_accounts
                 SET token_balance = ?, token_granted_total = ?, last_token_grant_date = ?, updated_at = ?
                 WHERE user_id = ?",
                params![balance, granted_total, last_grant_date, updated_at, cleaned],
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
    use super::SqliteStorage;
    use crate::storage::{StorageBackend, UserAccountRecord};
    use chrono::Local;
    use rusqlite::{params, Connection};
    use serde_json::json;
    use tempfile::tempdir;

    fn sample_user(
        user_id: &str,
        token_balance: i64,
        token_granted_total: i64,
        token_used_total: i64,
        last_token_grant_date: Option<&str>,
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
            token_balance,
            token_granted_total,
            token_used_total,
            last_token_grant_date: last_token_grant_date.map(str::to_string),
            experience_total: 0,
            is_demo: false,
            created_at: 1.0,
            updated_at: 1.0,
            last_login_at: None,
        }
    }

    #[test]
    fn legacy_daily_quota_rows_migrate_to_token_account_fields() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("legacy-user-accounts.db");
        let conn = Connection::open(&db_path).expect("open sqlite");
        conn.execute_batch(
            "CREATE TABLE user_accounts (
                user_id TEXT PRIMARY KEY,
                username TEXT NOT NULL UNIQUE,
                email TEXT,
                password_hash TEXT NOT NULL,
                roles TEXT NOT NULL,
                status TEXT NOT NULL,
                access_level TEXT NOT NULL,
                unit_id TEXT,
                daily_quota INTEGER NOT NULL DEFAULT 0,
                daily_quota_used INTEGER NOT NULL DEFAULT 0,
                daily_quota_date TEXT,
                experience_total INTEGER NOT NULL DEFAULT 0,
                is_demo INTEGER NOT NULL DEFAULT 0,
                created_at REAL NOT NULL,
                updated_at REAL NOT NULL,
                last_login_at REAL
            );",
        )
        .expect("create legacy user_accounts");
        let today = Local::now().format("%Y-%m-%d").to_string();
        conn.execute(
            "INSERT INTO user_accounts (
                user_id, username, email, password_hash, roles, status, access_level, unit_id,
                daily_quota, daily_quota_used, daily_quota_date, experience_total, is_demo,
                created_at, updated_at, last_login_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                "alice",
                "alice",
                Option::<String>::None,
                "hash",
                "[\"user\"]",
                "active",
                "A",
                Option::<String>::None,
                10_000_i64,
                2_500_i64,
                today,
                0_i64,
                0_i64,
                1.0_f64,
                1.0_f64,
                Option::<f64>::None,
            ],
        )
        .expect("insert legacy row");
        drop(conn);

        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize storage");

        let account = storage
            .get_user_account("alice")
            .expect("load user")
            .expect("user exists");
        assert_eq!(account.token_balance, 7_500);
        assert_eq!(account.token_granted_total, 10_000);
        assert_eq!(account.token_used_total, 2_500);
        assert_eq!(
            account.last_token_grant_date.as_deref(),
            Some(today.as_str())
        );
    }

    #[test]
    fn prepare_user_token_balance_grants_once_per_day() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("prepare-user-tokens.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize storage");
        storage
            .upsert_user_account(&sample_user("alice", 5, 15, 2, Some("2026-04-09")))
            .expect("insert user");

        let first = storage
            .prepare_user_token_balance("alice", "2026-04-10", 100)
            .expect("prepare first")
            .expect("status");
        assert_eq!(first.balance, 105);
        assert_eq!(first.granted_total, 115);
        assert_eq!(first.used_total, 2);
        assert_eq!(first.last_grant_date.as_deref(), Some("2026-04-10"));
        assert!(first.allowed);

        let second = storage
            .prepare_user_token_balance("alice", "2026-04-10", 100)
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
        assert_eq!(account.token_balance, 105);
        assert_eq!(account.token_granted_total, 115);
        assert_eq!(account.token_used_total, 2);
    }

    #[test]
    fn consume_user_tokens_deducts_usage_and_reports_overspend() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("consume-user-tokens.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize storage");
        storage
            .upsert_user_account(&sample_user("alice", 50, 50, 10, Some("2026-04-09")))
            .expect("insert user");

        let status = storage
            .consume_user_tokens("alice", "2026-04-10", 100, 180)
            .expect("consume tokens")
            .expect("status");
        assert_eq!(status.balance, 0);
        assert_eq!(status.granted_total, 150);
        assert_eq!(status.used_total, 190);
        assert_eq!(status.daily_grant, 100);
        assert_eq!(status.overspent_tokens, 30);
        assert_eq!(status.last_grant_date.as_deref(), Some("2026-04-10"));
        assert!(!status.allowed);

        let account = storage
            .get_user_account("alice")
            .expect("load user")
            .expect("user exists");
        assert_eq!(account.token_balance, 0);
        assert_eq!(account.token_granted_total, 150);
        assert_eq!(account.token_used_total, 190);
        assert_eq!(account.last_token_grant_date.as_deref(), Some("2026-04-10"));
    }

    #[test]
    fn grant_user_tokens_updates_balance_and_granted_total() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("grant-user-tokens.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize storage");
        storage
            .upsert_user_account(&sample_user("alice", 7, 20, 3, Some("2026-04-09")))
            .expect("insert user");

        let status = storage
            .grant_user_tokens("alice", "2026-04-10", 100, 30, 123.0)
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
        assert_eq!(account.token_balance, 137);
        assert_eq!(account.token_granted_total, 150);
        assert_eq!(account.token_used_total, 3);
        assert_eq!(account.last_token_grant_date.as_deref(), Some("2026-04-10"));
    }

    #[test]
    fn model_context_entries_append_and_replace_in_order() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("model-context.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize storage");

        storage
            .append_model_context_entry(
                "user-a",
                "session-a",
                &json!({
                    "role": "user",
                    "content": "first"
                }),
            )
            .expect("append first");
        storage
            .append_model_context_entry(
                "user-a",
                "session-a",
                &json!({
                    "role": "assistant",
                    "content": "second"
                }),
            )
            .expect("append second");

        let loaded = storage
            .load_model_context_entries("user-a", "session-a", None)
            .expect("load appended");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0]["content"], json!("first"));
        assert_eq!(loaded[1]["content"], json!("second"));

        storage
            .replace_model_context_entries(
                "user-a",
                "session-a",
                &[
                    json!({ "role": "user", "content": "compacted" }),
                    json!({ "role": "assistant", "content": "baseline" }),
                ],
            )
            .expect("replace entries");

        let replaced = storage
            .load_model_context_entries("user-a", "session-a", None)
            .expect("load replaced");
        assert_eq!(
            replaced,
            vec![
                json!({ "role": "user", "content": "compacted" }),
                json!({ "role": "assistant", "content": "baseline" }),
            ]
        );
    }

    #[test]
    fn cleanup_retention_removes_expired_model_context_entries() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("model-context-retention.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize storage");
        storage
            .upsert_user_account(&sample_user("regular", 0, 0, 0, None))
            .expect("insert regular user");
        let mut admin = sample_user("admin", 0, 0, 0, None);
        admin.roles = vec!["admin".to_string()];
        storage
            .upsert_user_account(&admin)
            .expect("insert admin user");

        let conn = storage.open().expect("open sqlite");
        let expired = SqliteStorage::now_ts() - 3.0 * 86400.0;
        conn.execute(
            "INSERT INTO model_context_entries (user_id, session_id, role, payload, created_time)
             VALUES (?, ?, ?, ?, ?)",
            params![
                "regular",
                "session-a",
                "user",
                r#"{"role":"user","content":"expired"}"#,
                expired,
            ],
        )
        .expect("insert expired context");
        conn.execute(
            "INSERT INTO model_context_entries (user_id, session_id, role, payload, created_time)
             VALUES (?, ?, ?, ?, ?)",
            params![
                "admin",
                "session-a",
                "user",
                r#"{"role":"user","content":"admin-kept"}"#,
                expired,
            ],
        )
        .expect("insert admin context");
        drop(conn);

        let deleted = storage.cleanup_retention(1).expect("cleanup retention");
        assert_eq!(deleted.get("model_context_entries").copied(), Some(1));
        assert!(storage
            .load_model_context_entries("regular", "session-a", None)
            .expect("load regular entries")
            .is_empty());
        assert_eq!(
            storage
                .load_model_context_entries("admin", "session-a", None)
                .expect("load admin entries")
                .len(),
            1
        );
    }
}
