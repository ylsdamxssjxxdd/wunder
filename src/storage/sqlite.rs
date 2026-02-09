// SQLite 存储实现：参考 Python 版结构，统一持久化历史/监控/记忆数据。
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
    VectorDocumentRecord, VectorDocumentSummaryRecord,
};
use anyhow::Result;
use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::types::Value as SqlValue;
use rusqlite::{
    params, params_from_iter, Connection, ErrorCode, OptionalExtension, TransactionBehavior,
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct SqliteStorage {
    db_path: PathBuf,
    initialized: AtomicBool,
    init_guard: Mutex<()>,
}

impl SqliteStorage {
    pub fn new(db_path: String) -> Self {
        let path = if db_path.trim().is_empty() {
            PathBuf::from("./data/wunder.db")
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

    fn ensure_user_account_quota_columns(&self, conn: &Connection) -> Result<()> {
        let mut stmt = conn.prepare("PRAGMA table_info(user_accounts)")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
        let mut columns = HashSet::new();
        for name in rows.flatten() {
            columns.insert(name);
        }
        let mut quota_added = false;
        if !columns.contains("daily_quota") {
            conn.execute(
                "ALTER TABLE user_accounts ADD COLUMN daily_quota INTEGER NOT NULL DEFAULT 10000",
                [],
            )?;
            quota_added = true;
        }
        if !columns.contains("daily_quota_used") {
            conn.execute(
                "ALTER TABLE user_accounts ADD COLUMN daily_quota_used INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        if !columns.contains("daily_quota_date") {
            conn.execute(
                "ALTER TABLE user_accounts ADD COLUMN daily_quota_date TEXT",
                [],
            )?;
        }
        if quota_added {
            conn.execute("UPDATE user_accounts SET daily_quota = 10000", [])?;
        }
        Ok(())
    }

    fn ensure_user_account_unit_columns(&self, conn: &Connection) -> Result<()> {
        let mut stmt = conn.prepare("PRAGMA table_info(user_accounts)")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
        let mut columns = HashSet::new();
        for name in rows.flatten() {
            columns.insert(name);
        }
        if !columns.contains("unit_id") {
            conn.execute("ALTER TABLE user_accounts ADD COLUMN unit_id TEXT", [])?;
        }
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_user_accounts_unit ON user_accounts (unit_id)",
            [],
        );
        Ok(())
    }

    fn ensure_user_account_list_indexes(&self, conn: &Connection) -> Result<()> {
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_user_accounts_created ON user_accounts (created_at)",
            [],
        );
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_user_accounts_unit_created ON user_accounts (unit_id, created_at)",
            [],
        );
        Ok(())
    }

    fn ensure_user_tool_access_columns(&self, conn: &Connection) -> Result<()> {
        let _ = conn;
        Ok(())
    }

    fn ensure_chat_session_columns(&self, conn: &Connection) -> Result<()> {
        let mut stmt = conn.prepare("PRAGMA table_info(chat_sessions)")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
        let mut columns = HashSet::new();
        for name in rows.flatten() {
            columns.insert(name);
        }
        if !columns.contains("status") {
            conn.execute("ALTER TABLE chat_sessions ADD COLUMN status TEXT", [])?;
        }
        if !columns.contains("agent_id") {
            conn.execute("ALTER TABLE chat_sessions ADD COLUMN agent_id TEXT", [])?;
        }
        if !columns.contains("tool_overrides") {
            conn.execute(
                "ALTER TABLE chat_sessions ADD COLUMN tool_overrides TEXT",
                [],
            )?;
        }
        if !columns.contains("parent_session_id") {
            conn.execute(
                "ALTER TABLE chat_sessions ADD COLUMN parent_session_id TEXT",
                [],
            )?;
        }
        if !columns.contains("parent_message_id") {
            conn.execute(
                "ALTER TABLE chat_sessions ADD COLUMN parent_message_id TEXT",
                [],
            )?;
        }
        if !columns.contains("spawn_label") {
            conn.execute("ALTER TABLE chat_sessions ADD COLUMN spawn_label TEXT", [])?;
        }
        if !columns.contains("spawned_by") {
            conn.execute("ALTER TABLE chat_sessions ADD COLUMN spawned_by TEXT", [])?;
        }
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_chat_sessions_parent \
             ON chat_sessions (user_id, parent_session_id, updated_at)",
            [],
        );
        Ok(())
    }

    fn ensure_channel_columns(&self, conn: &Connection) -> Result<()> {
        fn existing_columns(conn: &Connection, table: &str) -> Result<HashSet<String>> {
            let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            let mut columns = HashSet::new();
            for name in rows.flatten() {
                columns.insert(name);
            }
            Ok(columns)
        }

        fn add_missing(conn: &Connection, table: &str, columns: &[(&str, &str)]) -> Result<()> {
            let existing = existing_columns(conn, table)?;
            for (name, ddl) in columns {
                if !existing.contains(*name) {
                    conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {ddl}"), [])?;
                }
            }
            Ok(())
        }

        add_missing(
            conn,
            "channel_accounts",
            &[
                ("config", "config TEXT NOT NULL DEFAULT '{}'"),
                ("status", "status TEXT NOT NULL DEFAULT 'active'"),
                ("created_at", "created_at REAL NOT NULL DEFAULT 0"),
                ("updated_at", "updated_at REAL NOT NULL DEFAULT 0"),
            ],
        )?;
        add_missing(
            conn,
            "channel_bindings",
            &[
                ("channel", "channel TEXT"),
                ("account_id", "account_id TEXT"),
                ("peer_kind", "peer_kind TEXT"),
                ("peer_id", "peer_id TEXT"),
                ("agent_id", "agent_id TEXT"),
                ("tool_overrides", "tool_overrides TEXT"),
                ("priority", "priority INTEGER NOT NULL DEFAULT 0"),
                ("enabled", "enabled INTEGER NOT NULL DEFAULT 1"),
                ("created_at", "created_at REAL NOT NULL DEFAULT 0"),
                ("updated_at", "updated_at REAL NOT NULL DEFAULT 0"),
            ],
        )?;
        add_missing(
            conn,
            "channel_user_bindings",
            &[
                ("user_id", "user_id TEXT NOT NULL DEFAULT ''"),
                ("created_at", "created_at REAL NOT NULL DEFAULT 0"),
                ("updated_at", "updated_at REAL NOT NULL DEFAULT 0"),
            ],
        )?;
        add_missing(
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
                ("last_message_at", "last_message_at REAL NOT NULL DEFAULT 0"),
                ("created_at", "created_at REAL NOT NULL DEFAULT 0"),
                ("updated_at", "updated_at REAL NOT NULL DEFAULT 0"),
            ],
        )?;
        let _ = conn.execute(
            "UPDATE channel_sessions SET thread_id = '' WHERE thread_id IS NULL",
            [],
        );
        add_missing(
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
                ("created_at", "created_at REAL NOT NULL DEFAULT 0"),
            ],
        )?;
        add_missing(
            conn,
            "channel_outbox",
            &[
                ("thread_id", "thread_id TEXT"),
                ("payload", "payload TEXT NOT NULL DEFAULT '{}'"),
                ("status", "status TEXT NOT NULL DEFAULT 'pending'"),
                ("retry_count", "retry_count INTEGER NOT NULL DEFAULT 0"),
                ("retry_at", "retry_at REAL NOT NULL DEFAULT 0"),
                ("last_error", "last_error TEXT"),
                ("created_at", "created_at REAL NOT NULL DEFAULT 0"),
                ("updated_at", "updated_at REAL NOT NULL DEFAULT 0"),
                ("delivered_at", "delivered_at REAL"),
            ],
        )?;
        Ok(())
    }

    fn ensure_session_lock_columns(&self, conn: &Connection) -> Result<()> {
        let mut stmt = conn.prepare("PRAGMA table_info(session_locks)")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
        let mut columns = HashSet::new();
        for name in rows.flatten() {
            columns.insert(name);
        }
        if !columns.contains("agent_id") {
            conn.execute(
                "ALTER TABLE session_locks ADD COLUMN agent_id TEXT NOT NULL DEFAULT ''",
                [],
            )?;
        }
        conn.execute("DROP INDEX IF EXISTS idx_session_locks_user", [])?;
        conn.execute("DROP INDEX IF EXISTS idx_session_locks_user_agent", [])?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_session_locks_user_agent \
             ON session_locks (user_id, agent_id)",
            [],
        )?;
        Ok(())
    }

    fn ensure_user_agent_columns(&self, conn: &Connection) -> Result<()> {
        let mut stmt = conn.prepare("PRAGMA table_info(user_agents)")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
        let mut columns = HashSet::new();
        for name in rows.flatten() {
            columns.insert(name);
        }
        if !columns.contains("is_shared") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN is_shared INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        if !columns.contains("sandbox_container_id") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN sandbox_container_id INTEGER NOT NULL DEFAULT 1",
                [],
            )?;
        }
        if !columns.contains("hive_id") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN hive_id TEXT NOT NULL DEFAULT 'default'",
                [],
            )?;
        }
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_user_agents_user_hive ON user_agents (user_id, hive_id, updated_at)",
            [],
        )?;
        Ok(())
    }
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
            CREATE TABLE IF NOT EXISTS evaluation_runs (
              run_id TEXT PRIMARY KEY,
              user_id TEXT,
              model_name TEXT,
              language TEXT,
              status TEXT,
              total_score REAL,
              started_time REAL,
              finished_time REAL,
              payload TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_evaluation_runs_user
              ON evaluation_runs (user_id);
            CREATE INDEX IF NOT EXISTS idx_evaluation_runs_status
              ON evaluation_runs (status);
            CREATE INDEX IF NOT EXISTS idx_evaluation_runs_started
              ON evaluation_runs (started_time);
            CREATE TABLE IF NOT EXISTS evaluation_items (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              run_id TEXT NOT NULL,
              case_id TEXT NOT NULL,
              dimension TEXT,
              status TEXT,
              score REAL,
              max_score REAL,
              weight REAL,
              started_time REAL,
              finished_time REAL,
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
              daily_quota INTEGER NOT NULL DEFAULT 10000,
              daily_quota_used INTEGER NOT NULL DEFAULT 0,
              daily_quota_date TEXT,
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
              expires_at REAL NOT NULL,
              created_at REAL NOT NULL,
              last_used_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_user_tokens_user
              ON user_tokens (user_id);
            CREATE INDEX IF NOT EXISTS idx_user_tokens_expires
              ON user_tokens (expires_at);
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
            CREATE TABLE IF NOT EXISTS session_runs (
              run_id TEXT PRIMARY KEY,
              session_id TEXT NOT NULL,
              parent_session_id TEXT,
              user_id TEXT NOT NULL,
              agent_id TEXT,
              model_name TEXT,
              status TEXT NOT NULL,
              queued_time REAL,
              started_time REAL,
              finished_time REAL,
              elapsed_s REAL,
              result TEXT,
              error TEXT,
              updated_time REAL NOT NULL
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
              schedule_every_ms INTEGER,
              schedule_cron TEXT,
              schedule_tz TEXT,
              dedupe_key TEXT,
              next_run_at REAL,
              running_at REAL,
              last_run_at REAL,
              last_status TEXT,
              last_error TEXT,
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
              tool_names TEXT,
              access_level TEXT NOT NULL,
              is_shared INTEGER NOT NULL DEFAULT 0,
              status TEXT NOT NULL,
              icon TEXT,
              sandbox_container_id INTEGER NOT NULL DEFAULT 1,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
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
        self.ensure_user_account_unit_columns(&conn)?;
        self.ensure_user_account_list_indexes(&conn)?;
        self.ensure_user_tool_access_columns(&conn)?;
        self.ensure_chat_session_columns(&conn)?;
        self.ensure_channel_columns(&conn)?;
        self.ensure_session_lock_columns(&conn)?;
        self.ensure_user_agent_columns(&conn)?;
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
        let content = Self::parse_string(payload.get("content"));
        let timestamp = Self::parse_string(payload.get("timestamp"));
        let meta = payload
            .get("meta")
            .and_then(|value| serde_json::to_string(value).ok());
        let payload_text = Self::json_to_string(payload);
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
        let value: Option<i64> = conn
            .query_row(
                "SELECT MAX(event_id) FROM stream_events WHERE session_id = ?",
                params![cleaned_session],
                |row| row.get(0),
            )
            .optional()?;
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
                    records.push(value);
                } else {
                    records.push(json!({ "event_id": event_id, "data": value }));
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
        self.ensure_initialized()?;
        let conn = self.open()?;
        let value: Option<i64> = conn
            .query_row(
                "SELECT enabled FROM memory_settings WHERE user_id = ?",
                params![user_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value.map(|flag| flag != 0))
    }

    fn set_memory_enabled(&self, user_id: &str, enabled: bool) -> Result<()> {
        self.ensure_initialized()?;
        if user_id.trim().is_empty() {
            return Ok(());
        }
        let now = Self::now_ts();
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO memory_settings (user_id, enabled, updated_time) VALUES (?, ?, ?) \
             ON CONFLICT(user_id) DO UPDATE SET enabled = excluded.enabled, updated_time = excluded.updated_time",
            params![user_id, if enabled { 1 } else { 0 }, now],
        )?;
        Ok(())
    }

    fn load_memory_settings(&self) -> Result<Vec<HashMap<String, Value>>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut stmt =
            conn.prepare("SELECT user_id, enabled, updated_time FROM memory_settings")?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, f64>(2).unwrap_or(0.0),
                ))
            })?
            .collect::<std::result::Result<Vec<(String, i64, f64)>, _>>()?;
        let mut output = Vec::new();
        for (user_id, enabled, updated_time) in rows {
            let cleaned = user_id.trim();
            if cleaned.is_empty() {
                continue;
            }
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
        user_id: &str,
        session_id: &str,
        summary: &str,
        max_records: i64,
        now_ts: f64,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        let cleaned_summary = summary.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() || cleaned_summary.is_empty() {
            return Ok(());
        }
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO memory_records (user_id, session_id, summary, created_time, updated_time) VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT(user_id, session_id) DO UPDATE SET summary = excluded.summary, updated_time = excluded.updated_time",
            params![cleaned_user, cleaned_session, cleaned_summary, now_ts, now_ts],
        )?;
        if max_records > 0 {
            let safe_limit = max_records.max(1);
            conn.execute(
                "DELETE FROM memory_records WHERE user_id = ? AND id NOT IN (\
                    SELECT id FROM memory_records WHERE user_id = ? ORDER BY updated_time DESC, id DESC LIMIT ?\
                 )",
                params![cleaned_user, cleaned_user, safe_limit],
            )?;
        }
        conn.execute(
            "DELETE FROM memory_task_logs WHERE user_id = ? AND session_id NOT IN (\
                SELECT session_id FROM memory_records WHERE user_id = ?\
             )",
            params![cleaned_user, cleaned_user],
        )?;
        Ok(())
    }

    fn load_memory_records(
        &self,
        user_id: &str,
        limit: i64,
        order_desc: bool,
    ) -> Result<Vec<HashMap<String, Value>>> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let direction = if order_desc { "DESC" } else { "ASC" };
        let query = if limit > 0 {
            format!(
                "SELECT session_id, summary, created_time, updated_time FROM memory_records WHERE user_id = ? ORDER BY updated_time {direction}, id {direction} LIMIT ?"
            )
        } else {
            format!(
                "SELECT session_id, summary, created_time, updated_time FROM memory_records WHERE user_id = ? ORDER BY updated_time {direction}, id {direction}"
            )
        };
        let conn = self.open()?;
        let mut stmt = conn.prepare(&query)?;
        let rows = if limit > 0 {
            stmt.query_map(params![cleaned, limit], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2).unwrap_or(0.0),
                    row.get::<_, f64>(3).unwrap_or(0.0),
                ))
            })?
            .collect::<std::result::Result<Vec<(String, String, f64, f64)>, _>>()?
        } else {
            stmt.query_map(params![cleaned], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2).unwrap_or(0.0),
                    row.get::<_, f64>(3).unwrap_or(0.0),
                ))
            })?
            .collect::<std::result::Result<Vec<(String, String, f64, f64)>, _>>()?
        };
        let mut records = Vec::new();
        for (session_id, summary, created_time, updated_time) in rows {
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
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT user_id, COUNT(*) as record_count, MAX(updated_time) as last_time FROM memory_records GROUP BY user_id",
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
        let mut stats = Vec::new();
        for (user_id, record_count, last_time) in rows {
            let cleaned = user_id.trim();
            if cleaned.is_empty() {
                continue;
            }
            let mut entry = HashMap::new();
            entry.insert("user_id".to_string(), json!(cleaned));
            entry.insert("record_count".to_string(), json!(record_count));
            entry.insert("last_time".to_string(), json!(last_time));
            stats.push(entry);
        }
        Ok(stats)
    }

    fn delete_memory_record(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM memory_records WHERE user_id = ? AND session_id = ?",
            params![cleaned_user, cleaned_session],
        )?;
        Ok(affected as i64)
    }

    fn delete_memory_records_by_user(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM memory_records WHERE user_id = ?",
            params![cleaned_user],
        )?;
        Ok(affected as i64)
    }

    fn delete_memory_settings_by_user(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM memory_settings WHERE user_id = ?",
            params![cleaned_user],
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
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO memory_task_logs (task_id, user_id, session_id, status, queued_time, started_time, finished_time, elapsed_s, request_payload, result, error, updated_time)              VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)              ON CONFLICT(user_id, session_id) DO UPDATE SET                task_id = excluded.task_id, status = excluded.status, queued_time = excluded.queued_time, started_time = excluded.started_time,                finished_time = excluded.finished_time, elapsed_s = excluded.elapsed_s, request_payload = excluded.request_payload, result = excluded.result,                error = excluded.error, updated_time = excluded.updated_time",
            params![
                cleaned_task,
                cleaned_user,
                cleaned_session,
                status_text,
                params.queued_time,
                params.started_time,
                params.finished_time,
                params.elapsed_s,
                payload_text,
                params.result,
                params.error,
                now
            ],
        )?;
        Ok(())
    }

    fn load_memory_task_logs(&self, limit: Option<i64>) -> Result<Vec<HashMap<String, Value>>> {
        self.ensure_initialized()?;
        let mut query = String::from(
            "SELECT task_id, user_id, session_id, status, queued_time, started_time, finished_time, elapsed_s, updated_time FROM memory_task_logs ORDER BY updated_time DESC, id DESC",
        );
        let mut params_list: Vec<SqlValue> = Vec::new();
        if let Some(limit) = limit.filter(|value| *value > 0) {
            query.push_str(" LIMIT ?");
            params_list.push(SqlValue::from(limit));
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params_list.iter()), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, f64>(4).unwrap_or(0.0),
                    row.get::<_, f64>(5).unwrap_or(0.0),
                    row.get::<_, f64>(6).unwrap_or(0.0),
                    row.get::<_, f64>(7).unwrap_or(0.0),
                    row.get::<_, f64>(8).unwrap_or(0.0),
                ))
            })?
            .collect::<std::result::Result<
                Vec<(String, String, String, String, f64, f64, f64, f64, f64)>,
                _,
            >>()?;
        let mut logs = Vec::new();
        for (
            task_id,
            user_id,
            session_id,
            status,
            queued_time,
            started_time,
            finished_time,
            elapsed_s,
            updated_time,
        ) in rows
        {
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
        task_id: &str,
    ) -> Result<Option<HashMap<String, Value>>> {
        self.ensure_initialized()?;
        let cleaned = task_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT task_id, user_id, session_id, status, queued_time, started_time, finished_time, elapsed_s, request_payload, result, error, updated_time FROM memory_task_logs WHERE task_id = ? ORDER BY updated_time DESC, id DESC LIMIT 1",
        )?;
        let row = stmt
            .query_row(params![cleaned], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, f64>(4).unwrap_or(0.0),
                    row.get::<_, f64>(5).unwrap_or(0.0),
                    row.get::<_, f64>(6).unwrap_or(0.0),
                    row.get::<_, f64>(7).unwrap_or(0.0),
                    row.get::<_, String>(8).unwrap_or_default(),
                    row.get::<_, String>(9).unwrap_or_default(),
                    row.get::<_, String>(10).unwrap_or_default(),
                    row.get::<_, f64>(11).unwrap_or(0.0),
                ))
            })
            .optional()?;
        let Some((
            task_id,
            user_id,
            session_id,
            status,
            queued_time,
            started_time,
            finished_time,
            elapsed_s,
            request_payload,
            result,
            error,
            updated_time,
        )) = row
        else {
            return Ok(None);
        };
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

    fn delete_memory_task_log(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM memory_task_logs WHERE user_id = ? AND session_id = ?",
            params![cleaned_user, cleaned_session],
        )?;
        Ok(affected as i64)
    }

    fn delete_memory_task_logs_by_user(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM memory_task_logs WHERE user_id = ?",
            params![cleaned_user],
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
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO evaluation_runs (run_id, user_id, model_name, language, status, total_score, started_time, finished_time, payload) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(run_id) DO UPDATE SET user_id = excluded.user_id, model_name = excluded.model_name, \
             language = excluded.language, status = excluded.status, total_score = excluded.total_score, \
             started_time = excluded.started_time, finished_time = excluded.finished_time, payload = excluded.payload",
            params![
                run_id,
                user_id,
                model_name,
                language,
                status,
                total_score,
                started_time,
                finished_time,
                payload_text
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
        let conn = self.open()?;
        let updated = conn.execute(
            "UPDATE evaluation_items SET dimension = ?, status = ?, score = ?, max_score = ?, weight = ?, \
             started_time = ?, finished_time = ?, payload = ? WHERE run_id = ? AND case_id = ?",
            params![
                dimension,
                status,
                score,
                max_score,
                weight,
                started_time,
                finished_time,
                payload_text,
                cleaned,
                case_id,
            ],
        )?;
        if updated == 0 {
            conn.execute(
                "INSERT INTO evaluation_items (run_id, case_id, dimension, status, score, max_score, weight, started_time, finished_time, payload) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    cleaned,
                    case_id,
                    dimension,
                    status,
                    score,
                    max_score,
                    weight,
                    started_time,
                    finished_time,
                    payload_text
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
        let conn = self.open()?;
        let mut conditions = Vec::new();
        let mut params: Vec<SqlValue> = Vec::new();
        if let Some(user_id) = user_id {
            let cleaned = user_id.trim();
            if !cleaned.is_empty() {
                conditions.push("user_id = ?".to_string());
                params.push(SqlValue::from(cleaned.to_string()));
            }
        }
        if let Some(status) = status {
            let cleaned = status.trim();
            if !cleaned.is_empty() {
                conditions.push("status = ?".to_string());
                params.push(SqlValue::from(cleaned.to_string()));
            }
        }
        if let Some(model_name) = model_name {
            let cleaned = model_name.trim();
            if !cleaned.is_empty() {
                conditions.push("model_name = ?".to_string());
                params.push(SqlValue::from(cleaned.to_string()));
            }
        }
        if let Some(since) = since_time {
            conditions.push("started_time >= ?".to_string());
            params.push(SqlValue::from(since));
        }
        if let Some(until) = until_time {
            conditions.push("started_time <= ?".to_string());
            params.push(SqlValue::from(until));
        }
        let mut sql = String::from("SELECT payload FROM evaluation_runs");
        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
        sql.push_str(" ORDER BY started_time DESC");
        if let Some(limit) = limit {
            if limit > 0 {
                sql.push_str(" LIMIT ?");
                params.push(SqlValue::from(limit));
            }
        }
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(params), |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        let mut records = Vec::new();
        for payload in rows {
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
        let conn = self.open()?;
        let mut stmt = conn.prepare("SELECT payload FROM evaluation_runs WHERE run_id = ?")?;
        let mut rows = stmt.query([cleaned])?;
        if let Some(row) = rows.next()? {
            let payload: String = row.get(0)?;
            return Ok(Self::json_from_str(&payload));
        }
        Ok(None)
    }

    fn load_evaluation_items(&self, run_id: &str) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let cleaned = run_id.trim();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut stmt =
            conn.prepare("SELECT payload FROM evaluation_items WHERE run_id = ? ORDER BY id")?;
        let rows = stmt
            .query_map([cleaned], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        let mut records = Vec::new();
        for payload in rows {
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
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let items_deleted = tx.execute(
            "DELETE FROM evaluation_items WHERE run_id = ?",
            params![cleaned],
        )?;
        let runs_deleted = tx.execute(
            "DELETE FROM evaluation_runs WHERE run_id = ?",
            params![cleaned],
        )?;
        tx.commit()?;
        Ok((items_deleted + runs_deleted) as i64)
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
             daily_quota, daily_quota_used, daily_quota_date, is_demo, created_at, updated_at, last_login_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(user_id) DO UPDATE SET username = excluded.username, email = excluded.email, password_hash = excluded.password_hash, \
             roles = excluded.roles, status = excluded.status, access_level = excluded.access_level, unit_id = excluded.unit_id, \
             daily_quota = excluded.daily_quota, daily_quota_used = excluded.daily_quota_used, daily_quota_date = excluded.daily_quota_date, \
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
                record.daily_quota,
                record.daily_quota_used,
                record.daily_quota_date,
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
                "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, daily_quota, daily_quota_used, daily_quota_date, \
                 is_demo, created_at, updated_at, last_login_at FROM user_accounts WHERE user_id = ?",
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
                        daily_quota: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                        daily_quota_used: row.get::<_, Option<i64>>(9)?.unwrap_or(0),
                        daily_quota_date: row.get(10)?,
                        is_demo: row.get::<_, i64>(11)? != 0,
                        created_at: row.get(12)?,
                        updated_at: row.get(13)?,
                        last_login_at: row.get(14)?,
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
                "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, daily_quota, daily_quota_used, daily_quota_date, \
                 is_demo, created_at, updated_at, last_login_at FROM user_accounts WHERE username = ?",
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
                        daily_quota: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                        daily_quota_used: row.get::<_, Option<i64>>(9)?.unwrap_or(0),
                        daily_quota_date: row.get(10)?,
                        is_demo: row.get::<_, i64>(11)? != 0,
                        created_at: row.get(12)?,
                        updated_at: row.get(13)?,
                        last_login_at: row.get(14)?,
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
                "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, daily_quota, daily_quota_used, daily_quota_date, \
                 is_demo, created_at, updated_at, last_login_at FROM user_accounts WHERE email = ?",
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
                        daily_quota: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                        daily_quota_used: row.get::<_, Option<i64>>(9)?.unwrap_or(0),
                        daily_quota_date: row.get(10)?,
                        is_demo: row.get::<_, i64>(11)? != 0,
                        created_at: row.get(12)?,
                        updated_at: row.get(13)?,
                        last_login_at: row.get(14)?,
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
            "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, daily_quota, daily_quota_used, daily_quota_date, \
             is_demo, created_at, updated_at, last_login_at FROM user_accounts",
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
                    daily_quota: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                    daily_quota_used: row.get::<_, Option<i64>>(9)?.unwrap_or(0),
                    daily_quota_date: row.get(10)?,
                    is_demo: row.get::<_, i64>(11)? != 0,
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
                    last_login_at: row.get(14)?,
                })
            })?
            .collect::<std::result::Result<Vec<UserAccountRecord>, _>>()?;
        Ok((rows, total))
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
                 daily_quota, daily_quota_used, daily_quota_date, is_demo, created_at, updated_at, last_login_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
                 ON CONFLICT(user_id) DO UPDATE SET username = excluded.username, email = excluded.email, password_hash = excluded.password_hash, \
                 roles = excluded.roles, status = excluded.status, access_level = excluded.access_level, unit_id = excluded.unit_id, \
                 daily_quota = excluded.daily_quota, daily_quota_used = excluded.daily_quota_used, daily_quota_date = excluded.daily_quota_date, \
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
                    record.daily_quota,
                    record.daily_quota_used,
                    record.daily_quota_date,
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
            "INSERT INTO user_tokens (token, user_id, expires_at, created_at, last_used_at) VALUES (?, ?, ?, ?, ?)",
            params![
                record.token,
                record.user_id,
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
                "SELECT token, user_id, expires_at, created_at, last_used_at FROM user_tokens WHERE token = ?",
                params![cleaned],
                |row| {
                    Ok(UserTokenRecord {
                        token: row.get(0)?,
                        user_id: row.get(1)?,
                        expires_at: row.get(2)?,
                        created_at: row.get(3)?,
                        last_used_at: row.get(4)?,
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

    fn upsert_chat_session(&self, record: &ChatSessionRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let tool_overrides = if record.tool_overrides.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.tool_overrides))
        };
        conn.execute(
            "INSERT INTO chat_sessions (session_id, user_id, title, status, created_at, updated_at, last_message_at, agent_id, tool_overrides, \
             parent_session_id, parent_message_id, spawn_label, spawned_by) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(session_id) DO UPDATE SET user_id = excluded.user_id, title = excluded.title, \
             status = excluded.status, created_at = excluded.created_at, updated_at = excluded.updated_at, \
             last_message_at = excluded.last_message_at, agent_id = excluded.agent_id, \
             tool_overrides = excluded.tool_overrides, parent_session_id = excluded.parent_session_id, \
             parent_message_id = excluded.parent_message_id, spawn_label = excluded.spawn_label, \
             spawned_by = excluded.spawned_by",
            params![
                record.session_id,
                record.user_id,
                record.title,
                "active",
                record.created_at,
                record.updated_at,
                record.last_message_at,
                record.agent_id,
                tool_overrides,
                record.parent_session_id,
                record.parent_message_id,
                record.spawn_label,
                record.spawned_by
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT session_id, user_id, title, created_at, updated_at, last_message_at, agent_id, tool_overrides, \
                 parent_session_id, parent_message_id, spawn_label, spawned_by \
                 FROM chat_sessions WHERE user_id = ? AND session_id = ?",
                params![cleaned_user, cleaned_session],
                |row| {
                    let tool_overrides: Option<String> = row.get(7)?;
                    Ok(ChatSessionRecord {
                        session_id: row.get(0)?,
                        user_id: row.get(1)?,
                        title: row.get(2)?,
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                        last_message_at: row.get(5)?,
                        agent_id: row.get(6)?,
                        tool_overrides: Self::parse_string_list(tool_overrides),
                        parent_session_id: row.get(8)?,
                        parent_message_id: row.get(9)?,
                        spawn_label: row.get(10)?,
                        spawned_by: row.get(11)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
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
        let conn = self.open()?;
        let agent_id = agent_id.map(|value| value.trim());
        let (agent_clause, agent_params) = match agent_id {
            None => ("".to_string(), Vec::new()),
            Some("") => (
                " AND (agent_id IS NULL OR agent_id = '')".to_string(),
                Vec::new(),
            ),
            Some(value) => (
                " AND agent_id = ?".to_string(),
                vec![SqlValue::from(value.to_string())],
            ),
        };
        let (parent_clause, parent_params) = match parent_session_id {
            None => ("".to_string(), Vec::new()),
            Some(value) if value.trim().is_empty() => (
                " AND (parent_session_id IS NULL OR parent_session_id = '')".to_string(),
                Vec::new(),
            ),
            Some(value) => (
                " AND parent_session_id = ?".to_string(),
                vec![SqlValue::from(value.trim().to_string())],
            ),
        };
        let total_sql = format!(
            "SELECT COUNT(*) FROM chat_sessions WHERE user_id = ?{agent_clause}{parent_clause}"
        );
        let mut total_params = Vec::with_capacity(1 + agent_params.len() + parent_params.len());
        total_params.push(SqlValue::from(cleaned_user.to_string()));
        total_params.extend(agent_params.iter().cloned());
        total_params.extend(parent_params.iter().cloned());
        let total: i64 =
            conn.query_row(&total_sql, params_from_iter(total_params.iter()), |row| {
                row.get(0)
            })?;
        let mut sql = format!(
            "SELECT session_id, user_id, title, created_at, updated_at, last_message_at, agent_id, tool_overrides, \
             parent_session_id, parent_message_id, spawn_label, spawned_by \
             FROM chat_sessions WHERE user_id = ?{agent_clause}{parent_clause} ORDER BY updated_at DESC"
        );
        let mut params_list: Vec<SqlValue> = vec![SqlValue::from(cleaned_user.to_string())];
        params_list.extend(agent_params);
        params_list.extend(parent_params);
        if limit > 0 {
            sql.push_str(" LIMIT ? OFFSET ?");
            params_list.push(SqlValue::from(limit));
            params_list.push(SqlValue::from(offset.max(0)));
        }
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(params_list.iter()), |row| {
                let tool_overrides: Option<String> = row.get(7)?;
                Ok(ChatSessionRecord {
                    session_id: row.get(0)?,
                    user_id: row.get(1)?,
                    title: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    last_message_at: row.get(5)?,
                    agent_id: row.get(6)?,
                    tool_overrides: Self::parse_string_list(tool_overrides),
                    parent_session_id: row.get(8)?,
                    parent_message_id: row.get(9)?,
                    spawn_label: row.get(10)?,
                    spawned_by: row.get(11)?,
                })
            })?
            .collect::<std::result::Result<Vec<ChatSessionRecord>, _>>()?;
        Ok((rows, total))
    }

    fn list_chat_session_agent_ids(&self, user_id: &str) -> Result<Vec<String>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut stmt =
            conn.prepare("SELECT DISTINCT agent_id FROM chat_sessions WHERE user_id = ?")?;
        let rows = stmt.query_map([cleaned_user], |row| row.get::<_, Option<String>>(0))?;
        let mut agent_ids = Vec::new();
        for row in rows {
            let agent_id = row?.unwrap_or_default();
            agent_ids.push(agent_id);
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
        let conn = self.open()?;
        conn.execute(
            "UPDATE chat_sessions SET title = ?, updated_at = ? WHERE user_id = ? AND session_id = ?",
            params![title, updated_at, cleaned_user, cleaned_session],
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
        let conn = self.open()?;
        conn.execute(
            "UPDATE chat_sessions SET updated_at = ?, last_message_at = ? WHERE user_id = ? AND session_id = ?",
            params![updated_at, last_message_at, cleaned_user, cleaned_session],
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
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM chat_sessions WHERE user_id = ? AND session_id = ?",
            params![cleaned_user, cleaned_session],
        )?;
        Ok(affected as i64)
    }

    fn upsert_channel_account(&self, record: &ChannelAccountRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let config = Self::json_to_string(&record.config);
        conn.execute(
            "INSERT INTO channel_accounts (channel, account_id, config, status, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?) \
             ON CONFLICT(channel, account_id) DO UPDATE SET config = excluded.config, status = excluded.status, updated_at = excluded.updated_at",
            params![
                record.channel,
                record.account_id,
                config,
                record.status,
                record.created_at,
                record.updated_at
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT channel, account_id, config, status, created_at, updated_at FROM channel_accounts WHERE channel = ? AND account_id = ?",
                params![cleaned_channel, cleaned_account],
                |row| {
                    let config_text: String = row.get(2)?;
                    Ok(ChannelAccountRecord {
                        channel: row.get(0)?,
                        account_id: row.get(1)?,
                        config: Self::json_from_str(&config_text).unwrap_or(Value::Null),
                        status: row.get(3)?,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn list_channel_accounts(
        &self,
        channel: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<ChannelAccountRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut filters = Vec::new();
        let mut params_list: Vec<SqlValue> = Vec::new();
        if let Some(channel) = channel
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            filters.push("channel = ?".to_string());
            params_list.push(SqlValue::from(channel.to_string()));
        }
        if let Some(status) = status
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            filters.push("status = ?".to_string());
            params_list.push(SqlValue::from(status.to_string()));
        }
        let mut query =
            "SELECT channel, account_id, config, status, created_at, updated_at FROM channel_accounts"
                .to_string();
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" ORDER BY updated_at DESC");
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(params_from_iter(params_list.iter()), |row| {
            let config_text: String = row.get(2)?;
            Ok(ChannelAccountRecord {
                channel: row.get(0)?,
                account_id: row.get(1)?,
                config: Self::json_from_str(&config_text).unwrap_or(Value::Null),
                status: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
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
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM channel_accounts WHERE channel = ? AND account_id = ?",
            params![cleaned_channel, cleaned_account],
        )?;
        Ok(affected as i64)
    }

    fn upsert_channel_binding(&self, record: &ChannelBindingRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let tool_overrides = if record.tool_overrides.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.tool_overrides))
        };
        let enabled = if record.enabled { 1 } else { 0 };
        conn.execute(
            "INSERT INTO channel_bindings (binding_id, channel, account_id, peer_kind, peer_id, agent_id, tool_overrides, priority, enabled, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(binding_id) DO UPDATE SET channel = excluded.channel, account_id = excluded.account_id, peer_kind = excluded.peer_kind, peer_id = excluded.peer_id, \
             agent_id = excluded.agent_id, tool_overrides = excluded.tool_overrides, priority = excluded.priority, enabled = excluded.enabled, updated_at = excluded.updated_at",
            params![
                record.binding_id,
                record.channel,
                record.account_id,
                record.peer_kind,
                record.peer_id,
                record.agent_id,
                tool_overrides,
                record.priority,
                enabled,
                record.created_at,
                record.updated_at
            ],
        )?;
        Ok(())
    }

    fn list_channel_bindings(&self, channel: Option<&str>) -> Result<Vec<ChannelBindingRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut query = "SELECT binding_id, channel, account_id, peer_kind, peer_id, agent_id, tool_overrides, priority, enabled, created_at, updated_at FROM channel_bindings".to_string();
        let mut params_list: Vec<SqlValue> = Vec::new();
        if let Some(channel) = channel
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            query.push_str(" WHERE channel = ?");
            params_list.push(SqlValue::from(channel.to_string()));
        }
        query.push_str(" ORDER BY priority DESC, updated_at DESC");
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(params_from_iter(params_list.iter()), |row| {
            let tool_overrides: Option<String> = row.get(6)?;
            Ok(ChannelBindingRecord {
                binding_id: row.get(0)?,
                channel: row.get(1)?,
                account_id: row.get(2)?,
                peer_kind: row.get(3)?,
                peer_id: row.get(4)?,
                agent_id: row.get(5)?,
                tool_overrides: Self::parse_string_list(tool_overrides),
                priority: row.get(7)?,
                enabled: row.get::<_, i64>(8)? != 0,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            })
        })?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        Ok(output)
    }

    fn delete_channel_binding(&self, binding_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = binding_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM channel_bindings WHERE binding_id = ?",
            params![cleaned],
        )?;
        Ok(affected as i64)
    }

    fn upsert_channel_user_binding(&self, record: &ChannelUserBindingRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO channel_user_bindings (channel, account_id, peer_kind, peer_id, user_id, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(channel, account_id, peer_kind, peer_id) DO UPDATE SET user_id = excluded.user_id, updated_at = excluded.updated_at",
            params![
                record.channel,
                record.account_id,
                record.peer_kind,
                record.peer_id,
                record.user_id,
                record.created_at,
                record.updated_at
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT channel, account_id, peer_kind, peer_id, user_id, created_at, updated_at \
                 FROM channel_user_bindings WHERE channel = ? AND account_id = ? AND peer_kind = ? AND peer_id = ?",
                params![channel, account_id, peer_kind, peer_id],
                |row| {
                    Ok(ChannelUserBindingRecord {
                        channel: row.get(0)?,
                        account_id: row.get(1)?,
                        peer_kind: row.get(2)?,
                        peer_id: row.get(3)?,
                        user_id: row.get(4)?,
                        created_at: row.get(5)?,
                        updated_at: row.get(6)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn list_channel_user_bindings(
        &self,
        query: ListChannelUserBindingsQuery<'_>,
    ) -> Result<(Vec<ChannelUserBindingRecord>, i64)> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut filters = Vec::new();
        let mut params_list: Vec<SqlValue> = Vec::new();
        if let Some(channel) = query
            .channel
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            filters.push("channel = ?".to_string());
            params_list.push(SqlValue::from(channel.to_string()));
        }
        if let Some(account_id) = query
            .account_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            filters.push("account_id = ?".to_string());
            params_list.push(SqlValue::from(account_id.to_string()));
        }
        if let Some(peer_kind) = query
            .peer_kind
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            filters.push("peer_kind = ?".to_string());
            params_list.push(SqlValue::from(peer_kind.to_string()));
        }
        if let Some(peer_id) = query
            .peer_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            filters.push("peer_id = ?".to_string());
            params_list.push(SqlValue::from(peer_id.to_string()));
        }
        if let Some(user_id) = query
            .user_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            filters.push("user_id = ?".to_string());
            params_list.push(SqlValue::from(user_id.to_string()));
        }
        let mut sql =
            "SELECT channel, account_id, peer_kind, peer_id, user_id, created_at, updated_at FROM channel_user_bindings"
                .to_string();
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
        params_list.push(SqlValue::from(limit_value));
        params_list.push(SqlValue::from(offset_value));
        sql.push_str(" LIMIT ? OFFSET ?");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(params_list.iter()), |row| {
            Ok(ChannelUserBindingRecord {
                channel: row.get(0)?,
                account_id: row.get(1)?,
                peer_kind: row.get(2)?,
                peer_id: row.get(3)?,
                user_id: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        let mut count_sql = "SELECT COUNT(*) FROM channel_user_bindings".to_string();
        if !filters.is_empty() {
            count_sql.push_str(" WHERE ");
            count_sql.push_str(&filters.join(" AND "));
        }
        let count_params = params_from_iter(params_list.iter().take(params_list.len() - 2));
        let total: i64 = conn.query_row(&count_sql, count_params, |row| row.get(0))?;
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
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM channel_user_bindings WHERE channel = ? AND account_id = ? AND peer_kind = ? AND peer_id = ?",
            params![cleaned_channel, cleaned_account, cleaned_kind, cleaned_peer],
        )?;
        Ok(affected as i64)
    }

    fn upsert_channel_session(&self, record: &ChannelSessionRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let thread_id = Self::normalize_channel_thread_id(record.thread_id.as_deref());
        let metadata = record.metadata.as_ref().map(Self::json_to_string);
        let tts_enabled = record.tts_enabled.map(|value| if value { 1 } else { 0 });
        conn.execute(
            "INSERT INTO channel_sessions (channel, account_id, peer_kind, peer_id, thread_id, session_id, agent_id, user_id, tts_enabled, tts_voice, metadata, last_message_at, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(channel, account_id, peer_kind, peer_id, thread_id) DO UPDATE SET session_id = excluded.session_id, agent_id = excluded.agent_id, user_id = excluded.user_id, \
             tts_enabled = excluded.tts_enabled, tts_voice = excluded.tts_voice, metadata = excluded.metadata, last_message_at = excluded.last_message_at, updated_at = excluded.updated_at",
            params![
                record.channel,
                record.account_id,
                record.peer_kind,
                record.peer_id,
                thread_id,
                record.session_id,
                record.agent_id,
                record.user_id,
                tts_enabled,
                record.tts_voice,
                metadata,
                record.last_message_at,
                record.created_at,
                record.updated_at
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT channel, account_id, peer_kind, peer_id, thread_id, session_id, agent_id, user_id, tts_enabled, tts_voice, metadata, last_message_at, created_at, updated_at \
                 FROM channel_sessions WHERE channel = ? AND account_id = ? AND peer_kind = ? AND peer_id = ? AND (thread_id IS ? OR thread_id = ?)",
                params![
                    cleaned_channel,
                    cleaned_account,
                    cleaned_peer_kind,
                    cleaned_peer_id,
                    thread_id,
                    thread_id
                ],
                |row| {
                    let metadata_text: Option<String> = row.get(10)?;
                    Ok(ChannelSessionRecord {
                        channel: row.get(0)?,
                        account_id: row.get(1)?,
                        peer_kind: row.get(2)?,
                        peer_id: row.get(3)?,
                        thread_id: Self::normalize_channel_thread_value(row.get(4)?),
                        session_id: row.get(5)?,
                        agent_id: row.get(6)?,
                        user_id: row.get(7)?,
                        tts_enabled: row
                            .get::<_, Option<i64>>(8)?
                            .map(|value| value != 0),
                        tts_voice: row.get(9)?,
                        metadata: metadata_text.and_then(|value| Self::json_from_str(&value)),
                        last_message_at: row.get(11)?,
                        created_at: row.get(12)?,
                        updated_at: row.get(13)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
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
        let conn = self.open()?;
        let mut filters = Vec::new();
        let mut params_list: Vec<SqlValue> = Vec::new();
        if let Some(channel) = channel
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            filters.push("channel = ?".to_string());
            params_list.push(SqlValue::from(channel.to_string()));
        }
        if let Some(account) = account_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            filters.push("account_id = ?".to_string());
            params_list.push(SqlValue::from(account.to_string()));
        }
        if let Some(peer_id) = peer_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            filters.push("peer_id = ?".to_string());
            params_list.push(SqlValue::from(peer_id.to_string()));
        }
        if let Some(session_id) = session_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            filters.push("session_id = ?".to_string());
            params_list.push(SqlValue::from(session_id.to_string()));
        }
        let mut query = "SELECT channel, account_id, peer_kind, peer_id, thread_id, session_id, agent_id, user_id, tts_enabled, tts_voice, metadata, last_message_at, created_at, updated_at FROM channel_sessions".to_string();
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" ORDER BY updated_at DESC");
        let offset_value = offset.max(0);
        let limit_value = if limit <= 0 { 100 } else { limit.min(500) };
        params_list.push(SqlValue::from(limit_value));
        params_list.push(SqlValue::from(offset_value));
        query.push_str(" LIMIT ? OFFSET ?");
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(params_from_iter(params_list.iter()), |row| {
            let metadata_text: Option<String> = row.get(10)?;
            Ok(ChannelSessionRecord {
                channel: row.get(0)?,
                account_id: row.get(1)?,
                peer_kind: row.get(2)?,
                peer_id: row.get(3)?,
                thread_id: Self::normalize_channel_thread_value(row.get(4)?),
                session_id: row.get(5)?,
                agent_id: row.get(6)?,
                user_id: row.get(7)?,
                tts_enabled: row.get::<_, Option<i64>>(8)?.map(|value| value != 0),
                tts_voice: row.get(9)?,
                metadata: metadata_text.and_then(|value| Self::json_from_str(&value)),
                last_message_at: row.get(11)?,
                created_at: row.get(12)?,
                updated_at: row.get(13)?,
            })
        })?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }

        let mut count_query = "SELECT COUNT(*) FROM channel_sessions".to_string();
        if !filters.is_empty() {
            count_query.push_str(" WHERE ");
            count_query.push_str(&filters.join(" AND "));
        }
        let count_params = params_from_iter(params_list.iter().take(params_list.len() - 2));
        let total: i64 = conn.query_row(&count_query, count_params, |row| row.get(0))?;
        Ok((output, total))
    }

    fn insert_channel_message(&self, record: &ChannelMessageRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let payload = Self::json_to_string(&record.payload);
        let raw_payload = record.raw_payload.as_ref().map(Self::json_to_string);
        conn.execute(
            "INSERT INTO channel_messages (channel, account_id, peer_kind, peer_id, thread_id, session_id, message_id, sender_id, message_type, payload, raw_payload, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                record.channel,
                record.account_id,
                record.peer_kind,
                record.peer_id,
                record.thread_id,
                record.session_id,
                record.message_id,
                record.sender_id,
                record.message_type,
                payload,
                raw_payload,
                record.created_at
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
        let conn = self.open()?;
        let mut filters = Vec::new();
        let mut params_list: Vec<SqlValue> = Vec::new();
        if let Some(channel) = channel
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            filters.push("channel = ?".to_string());
            params_list.push(SqlValue::from(channel.to_string()));
        }
        if let Some(session_id) = session_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            filters.push("session_id = ?".to_string());
            params_list.push(SqlValue::from(session_id.to_string()));
        }
        let mut query = "SELECT channel, account_id, peer_kind, peer_id, thread_id, session_id, message_id, sender_id, message_type, payload, raw_payload, created_at FROM channel_messages".to_string();
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" ORDER BY id DESC");
        let limit_value = if limit <= 0 { 50 } else { limit.min(200) };
        params_list.push(SqlValue::from(limit_value));
        query.push_str(" LIMIT ?");
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(params_from_iter(params_list.iter()), |row| {
            let payload_text: String = row.get(9)?;
            let raw_text: Option<String> = row.get(10)?;
            Ok(ChannelMessageRecord {
                channel: row.get(0)?,
                account_id: row.get(1)?,
                peer_kind: row.get(2)?,
                peer_id: row.get(3)?,
                thread_id: row.get(4)?,
                session_id: row.get(5)?,
                message_id: row.get(6)?,
                sender_id: row.get(7)?,
                message_type: row.get(8)?,
                payload: Self::json_from_str(&payload_text).unwrap_or(Value::Null),
                raw_payload: raw_text.and_then(|value| Self::json_from_str(&value)),
                created_at: row.get(11)?,
            })
        })?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        Ok(output)
    }

    fn enqueue_channel_outbox(&self, record: &ChannelOutboxRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let payload = Self::json_to_string(&record.payload);
        conn.execute(
            "INSERT INTO channel_outbox (outbox_id, channel, account_id, peer_kind, peer_id, thread_id, payload, status, retry_count, retry_at, last_error, created_at, updated_at, delivered_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(outbox_id) DO UPDATE SET payload = excluded.payload, status = excluded.status, retry_count = excluded.retry_count, retry_at = excluded.retry_at, \
             last_error = excluded.last_error, updated_at = excluded.updated_at, delivered_at = excluded.delivered_at",
            params![
                record.outbox_id,
                record.channel,
                record.account_id,
                record.peer_kind,
                record.peer_id,
                record.thread_id,
                payload,
                record.status,
                record.retry_count,
                record.retry_at,
                record.last_error,
                record.created_at,
                record.updated_at,
                record.delivered_at
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT outbox_id, channel, account_id, peer_kind, peer_id, thread_id, payload, status, retry_count, retry_at, last_error, created_at, updated_at, delivered_at \
                 FROM channel_outbox WHERE outbox_id = ?",
                params![cleaned],
                |row| {
                    let payload_text: String = row.get(6)?;
                    Ok(ChannelOutboxRecord {
                        outbox_id: row.get(0)?,
                        channel: row.get(1)?,
                        account_id: row.get(2)?,
                        peer_kind: row.get(3)?,
                        peer_id: row.get(4)?,
                        thread_id: row.get(5)?,
                        payload: Self::json_from_str(&payload_text).unwrap_or(Value::Null),
                        status: row.get(7)?,
                        retry_count: row.get(8)?,
                        retry_at: row.get(9)?,
                        last_error: row.get(10)?,
                        created_at: row.get(11)?,
                        updated_at: row.get(12)?,
                        delivered_at: row.get(13)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn list_pending_channel_outbox(&self, limit: i64) -> Result<Vec<ChannelOutboxRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let now = Self::now_ts();
        let limit_value = if limit <= 0 { 50 } else { limit.min(200) };
        let mut stmt = conn.prepare(
            "SELECT outbox_id, channel, account_id, peer_kind, peer_id, thread_id, payload, status, retry_count, retry_at, last_error, created_at, updated_at, delivered_at \
             FROM channel_outbox WHERE (status = 'pending' OR status = 'retry') AND retry_at <= ? ORDER BY retry_at ASC LIMIT ?",
        )?;
        let rows = stmt.query_map(params![now, limit_value], |row| {
            let payload_text: String = row.get(6)?;
            Ok(ChannelOutboxRecord {
                outbox_id: row.get(0)?,
                channel: row.get(1)?,
                account_id: row.get(2)?,
                peer_kind: row.get(3)?,
                peer_id: row.get(4)?,
                thread_id: row.get(5)?,
                payload: Self::json_from_str(&payload_text).unwrap_or(Value::Null),
                status: row.get(7)?,
                retry_count: row.get(8)?,
                retry_at: row.get(9)?,
                last_error: row.get(10)?,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
                delivered_at: row.get(13)?,
            })
        })?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
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
        let conn = self.open()?;
        conn.execute(
            "UPDATE channel_outbox SET status = ?, retry_count = ?, retry_at = ?, last_error = ?, updated_at = ?, delivered_at = ? WHERE outbox_id = ?",
            params![
                params.status,
                params.retry_count,
                params.retry_at,
                params.last_error,
                params.updated_at,
                params.delivered_at,
                cleaned
            ],
        )?;
        Ok(())
    }

    fn upsert_gateway_client(&self, record: &GatewayClientRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
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
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(connection_id) DO UPDATE SET role = excluded.role, user_id = excluded.user_id, node_id = excluded.node_id, scopes = excluded.scopes, \
             caps = excluded.caps, commands = excluded.commands, client_info = excluded.client_info, status = excluded.status, last_seen_at = excluded.last_seen_at, \
             disconnected_at = excluded.disconnected_at",
            params![
                record.connection_id,
                record.role,
                record.user_id,
                record.node_id,
                scopes,
                caps,
                commands,
                client_info,
                record.status,
                record.connected_at,
                record.last_seen_at,
                record.disconnected_at
            ],
        )?;
        Ok(())
    }

    fn list_gateway_clients(&self, status: Option<&str>) -> Result<Vec<GatewayClientRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut query = "SELECT connection_id, role, user_id, node_id, scopes, caps, commands, client_info, status, connected_at, last_seen_at, disconnected_at FROM gateway_clients".to_string();
        let mut params_list: Vec<SqlValue> = Vec::new();
        if let Some(status) = status
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            query.push_str(" WHERE status = ?");
            params_list.push(SqlValue::from(status.to_string()));
        }
        query.push_str(" ORDER BY last_seen_at DESC");
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(params_from_iter(params_list.iter()), |row| {
            let scopes: Option<String> = row.get(4)?;
            let caps: Option<String> = row.get(5)?;
            let commands: Option<String> = row.get(6)?;
            let client_info: Option<String> = row.get(7)?;
            Ok(GatewayClientRecord {
                connection_id: row.get(0)?,
                role: row.get(1)?,
                user_id: row.get(2)?,
                node_id: row.get(3)?,
                scopes: Self::parse_string_list(scopes),
                caps: Self::parse_string_list(caps),
                commands: Self::parse_string_list(commands),
                client_info: client_info.as_deref().and_then(Self::json_from_str),
                status: row.get(8)?,
                connected_at: row.get(9)?,
                last_seen_at: row.get(10)?,
                disconnected_at: row.get(11)?,
            })
        })?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        Ok(output)
    }

    fn upsert_gateway_node(&self, record: &GatewayNodeRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
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
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(node_id) DO UPDATE SET name = excluded.name, device_fingerprint = excluded.device_fingerprint, status = excluded.status, caps = excluded.caps, \
             commands = excluded.commands, permissions = excluded.permissions, metadata = excluded.metadata, updated_at = excluded.updated_at, last_seen_at = excluded.last_seen_at",
            params![
                record.node_id,
                record.name,
                record.device_fingerprint,
                record.status,
                caps,
                commands,
                permissions,
                metadata,
                record.created_at,
                record.updated_at,
                record.last_seen_at
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT node_id, name, device_fingerprint, status, caps, commands, permissions, metadata, created_at, updated_at, last_seen_at FROM gateway_nodes WHERE node_id = ?",
                params![cleaned],
                |row| {
                    let caps: Option<String> = row.get(4)?;
                    let commands: Option<String> = row.get(5)?;
                    let permissions: Option<String> = row.get(6)?;
                    let metadata: Option<String> = row.get(7)?;
                    Ok(GatewayNodeRecord {
                        node_id: row.get(0)?,
                        name: row.get(1)?,
                        device_fingerprint: row.get(2)?,
                        status: row.get(3)?,
                        caps: Self::parse_string_list(caps),
                        commands: Self::parse_string_list(commands),
                        permissions: permissions
                            .as_deref()
                            .and_then(Self::json_from_str),
                        metadata: metadata
                            .as_deref()
                            .and_then(Self::json_from_str),
                        created_at: row.get(8)?,
                        updated_at: row.get(9)?,
                        last_seen_at: row.get(10)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn list_gateway_nodes(&self, status: Option<&str>) -> Result<Vec<GatewayNodeRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut query = "SELECT node_id, name, device_fingerprint, status, caps, commands, permissions, metadata, created_at, updated_at, last_seen_at FROM gateway_nodes".to_string();
        let mut params_list: Vec<SqlValue> = Vec::new();
        if let Some(status) = status
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            query.push_str(" WHERE status = ?");
            params_list.push(SqlValue::from(status.to_string()));
        }
        query.push_str(" ORDER BY updated_at DESC");
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(params_from_iter(params_list.iter()), |row| {
            let caps: Option<String> = row.get(4)?;
            let commands: Option<String> = row.get(5)?;
            let permissions: Option<String> = row.get(6)?;
            let metadata: Option<String> = row.get(7)?;
            Ok(GatewayNodeRecord {
                node_id: row.get(0)?,
                name: row.get(1)?,
                device_fingerprint: row.get(2)?,
                status: row.get(3)?,
                caps: Self::parse_string_list(caps),
                commands: Self::parse_string_list(commands),
                permissions: permissions.as_deref().and_then(Self::json_from_str),
                metadata: metadata.as_deref().and_then(Self::json_from_str),
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
                last_seen_at: row.get(10)?,
            })
        })?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        Ok(output)
    }

    fn upsert_gateway_node_token(&self, record: &GatewayNodeTokenRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO gateway_node_tokens (token, node_id, status, created_at, updated_at, last_used_at) \
             VALUES (?, ?, ?, ?, ?, ?) \
             ON CONFLICT(token) DO UPDATE SET node_id = excluded.node_id, status = excluded.status, updated_at = excluded.updated_at, last_used_at = excluded.last_used_at",
            params![
                record.token,
                record.node_id,
                record.status,
                record.created_at,
                record.updated_at,
                record.last_used_at
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT token, node_id, status, created_at, updated_at, last_used_at FROM gateway_node_tokens WHERE token = ?",
                params![cleaned],
                |row| {
                    Ok(GatewayNodeTokenRecord {
                        token: row.get(0)?,
                        node_id: row.get(1)?,
                        status: row.get(2)?,
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                        last_used_at: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn list_gateway_node_tokens(
        &self,
        node_id: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<GatewayNodeTokenRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut query =
            "SELECT token, node_id, status, created_at, updated_at, last_used_at FROM gateway_node_tokens"
                .to_string();
        let mut filters = Vec::new();
        let mut params_list: Vec<SqlValue> = Vec::new();
        if let Some(node_id) = node_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            filters.push("node_id = ?".to_string());
            params_list.push(SqlValue::from(node_id.to_string()));
        }
        if let Some(status) = status
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            filters.push("status = ?".to_string());
            params_list.push(SqlValue::from(status.to_string()));
        }
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" ORDER BY updated_at DESC");
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(params_from_iter(params_list.iter()), |row| {
            Ok(GatewayNodeTokenRecord {
                token: row.get(0)?,
                node_id: row.get(1)?,
                status: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                last_used_at: row.get(5)?,
            })
        })?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        Ok(output)
    }

    fn delete_gateway_node_token(&self, token: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = token.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM gateway_node_tokens WHERE token = ?",
            params![cleaned],
        )?;
        Ok(affected as i64)
    }

    fn upsert_media_asset(&self, record: &MediaAssetRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO media_assets (asset_id, kind, url, mime, size, hash, source, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(asset_id) DO UPDATE SET kind = excluded.kind, url = excluded.url, mime = excluded.mime, size = excluded.size, hash = excluded.hash, source = excluded.source",
            params![
                record.asset_id,
                record.kind,
                record.url,
                record.mime,
                record.size,
                record.hash,
                record.source,
                record.created_at
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT asset_id, kind, url, mime, size, hash, source, created_at FROM media_assets WHERE asset_id = ?",
                params![cleaned],
                |row| {
                    Ok(MediaAssetRecord {
                        asset_id: row.get(0)?,
                        kind: row.get(1)?,
                        url: row.get(2)?,
                        mime: row.get(3)?,
                        size: row.get(4)?,
                        hash: row.get(5)?,
                        source: row.get(6)?,
                        created_at: row.get(7)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn get_media_asset_by_hash(&self, hash: &str) -> Result<Option<MediaAssetRecord>> {
        self.ensure_initialized()?;
        let cleaned = hash.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT asset_id, kind, url, mime, size, hash, source, created_at FROM media_assets WHERE hash = ?",
                params![cleaned],
                |row| {
                    Ok(MediaAssetRecord {
                        asset_id: row.get(0)?,
                        kind: row.get(1)?,
                        url: row.get(2)?,
                        mime: row.get(3)?,
                        size: row.get(4)?,
                        hash: row.get(5)?,
                        source: row.get(6)?,
                        created_at: row.get(7)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn upsert_speech_job(&self, record: &SpeechJobRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let metadata = record.metadata.as_ref().map(Self::json_to_string);
        conn.execute(
            "INSERT INTO speech_jobs (job_id, job_type, status, input_text, input_url, output_text, output_url, model, error, retry_count, next_retry_at, created_at, updated_at, metadata) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(job_id) DO UPDATE SET status = excluded.status, input_text = excluded.input_text, input_url = excluded.input_url, output_text = excluded.output_text, \
             output_url = excluded.output_url, model = excluded.model, error = excluded.error, retry_count = excluded.retry_count, next_retry_at = excluded.next_retry_at, \
             updated_at = excluded.updated_at, metadata = excluded.metadata",
            params![
                record.job_id,
                record.job_type,
                record.status,
                record.input_text,
                record.input_url,
                record.output_text,
                record.output_url,
                record.model,
                record.error,
                record.retry_count,
                record.next_retry_at,
                record.created_at,
                record.updated_at,
                metadata
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
        let conn = self.open()?;
        let now = Self::now_ts();
        let limit_value = if limit <= 0 { 50 } else { limit.min(200) };
        let mut stmt = conn.prepare(
            "SELECT job_id, job_type, status, input_text, input_url, output_text, output_url, model, error, retry_count, next_retry_at, created_at, updated_at, metadata \
             FROM speech_jobs WHERE job_type = ? AND (status = 'queued' OR status = 'retry') AND next_retry_at <= ? ORDER BY next_retry_at ASC LIMIT ?",
        )?;
        let rows = stmt.query_map(params![cleaned, now, limit_value], |row| {
            let metadata_text: Option<String> = row.get(13)?;
            Ok(SpeechJobRecord {
                job_id: row.get(0)?,
                job_type: row.get(1)?,
                status: row.get(2)?,
                input_text: row.get(3)?,
                input_url: row.get(4)?,
                output_text: row.get(5)?,
                output_url: row.get(6)?,
                model: row.get(7)?,
                error: row.get(8)?,
                retry_count: row.get(9)?,
                next_retry_at: row.get(10)?,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
                metadata: metadata_text.and_then(|value| Self::json_from_str(&value)),
            })
        })?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        Ok(output)
    }

    fn upsert_session_run(&self, record: &SessionRunRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO session_runs (run_id, session_id, parent_session_id, user_id, agent_id, model_name, status, queued_time, \
             started_time, finished_time, elapsed_s, result, error, updated_time) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(run_id) DO UPDATE SET session_id = excluded.session_id, parent_session_id = excluded.parent_session_id, \
             user_id = excluded.user_id, agent_id = excluded.agent_id, model_name = excluded.model_name, status = excluded.status, \
             queued_time = excluded.queued_time, started_time = excluded.started_time, finished_time = excluded.finished_time, \
             elapsed_s = excluded.elapsed_s, result = excluded.result, error = excluded.error, updated_time = excluded.updated_time",
            params![
                record.run_id,
                record.session_id,
                record.parent_session_id,
                record.user_id,
                record.agent_id,
                record.model_name,
                record.status,
                record.queued_time,
                record.started_time,
                record.finished_time,
                record.elapsed_s,
                record.result,
                record.error,
                record.updated_time
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT run_id, session_id, parent_session_id, user_id, agent_id, model_name, status, queued_time, started_time, \
                 finished_time, elapsed_s, result, error, updated_time FROM session_runs WHERE run_id = ?",
                params![cleaned],
                |row| {
                    Ok(SessionRunRecord {
                        run_id: row.get(0)?,
                        session_id: row.get(1)?,
                        parent_session_id: row.get(2)?,
                        user_id: row.get(3)?,
                        agent_id: row.get(4)?,
                        model_name: row.get(5)?,
                        status: row.get(6)?,
                        queued_time: row.get::<_, Option<f64>>(7)?.unwrap_or(0.0),
                        started_time: row.get::<_, Option<f64>>(8)?.unwrap_or(0.0),
                        finished_time: row.get::<_, Option<f64>>(9)?.unwrap_or(0.0),
                        elapsed_s: row.get::<_, Option<f64>>(10)?.unwrap_or(0.0),
                        result: row.get(11)?,
                        error: row.get(12)?,
                        updated_time: row.get::<_, Option<f64>>(13)?.unwrap_or(0.0),
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn upsert_cron_job(&self, record: &CronJobRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let payload = Self::json_to_string(&record.payload);
        let deliver = record.deliver.as_ref().map(Self::json_to_string);
        conn.execute(
            "INSERT INTO cron_jobs (job_id, user_id, session_id, agent_id, name, session_target, payload, deliver, enabled, delete_after_run, \
             schedule_kind, schedule_at, schedule_every_ms, schedule_cron, schedule_tz, dedupe_key, next_run_at, running_at, last_run_at, \
             last_status, last_error, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(job_id) DO UPDATE SET user_id = excluded.user_id, session_id = excluded.session_id, agent_id = excluded.agent_id, \
             name = excluded.name, session_target = excluded.session_target, payload = excluded.payload, deliver = excluded.deliver, \
             enabled = excluded.enabled, delete_after_run = excluded.delete_after_run, schedule_kind = excluded.schedule_kind, \
             schedule_at = excluded.schedule_at, schedule_every_ms = excluded.schedule_every_ms, schedule_cron = excluded.schedule_cron, \
             schedule_tz = excluded.schedule_tz, dedupe_key = excluded.dedupe_key, next_run_at = excluded.next_run_at, \
             running_at = excluded.running_at, last_run_at = excluded.last_run_at, last_status = excluded.last_status, \
             last_error = excluded.last_error, updated_at = excluded.updated_at",
            params![
                record.job_id,
                record.user_id,
                record.session_id,
                record.agent_id,
                record.name,
                record.session_target,
                payload,
                deliver,
                if record.enabled { 1 } else { 0 },
                if record.delete_after_run { 1 } else { 0 },
                record.schedule_kind,
                record.schedule_at,
                record.schedule_every_ms,
                record.schedule_cron,
                record.schedule_tz,
                record.dedupe_key,
                record.next_run_at,
                record.running_at,
                record.last_run_at,
                record.last_status,
                record.last_error,
                record.created_at,
                record.updated_at
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT job_id, user_id, session_id, agent_id, name, session_target, payload, deliver, enabled, delete_after_run, \
                 schedule_kind, schedule_at, schedule_every_ms, schedule_cron, schedule_tz, dedupe_key, next_run_at, running_at, \
                 last_run_at, last_status, last_error, created_at, updated_at FROM cron_jobs WHERE user_id = ? AND job_id = ?",
                params![cleaned_user, cleaned_job],
                |row| {
                    let payload_text: Option<String> = row.get(6)?;
                    let deliver_text: Option<String> = row.get(7)?;
                    let enabled: Option<i64> = row.get(8)?;
                    let delete_after: Option<i64> = row.get(9)?;
                    Ok(CronJobRecord {
                        job_id: row.get(0)?,
                        user_id: row.get(1)?,
                        session_id: row.get(2)?,
                        agent_id: row.get(3)?,
                        name: row.get(4)?,
                        session_target: row.get(5)?,
                        payload: Self::json_value_or_null(payload_text),
                        deliver: match deliver_text {
                            Some(text) => Self::json_from_str(&text),
                            None => None,
                        },
                        enabled: enabled.unwrap_or(0) != 0,
                        delete_after_run: delete_after.unwrap_or(0) != 0,
                        schedule_kind: row.get(10)?,
                        schedule_at: row.get(11)?,
                        schedule_every_ms: row.get(12)?,
                        schedule_cron: row.get(13)?,
                        schedule_tz: row.get(14)?,
                        dedupe_key: row.get(15)?,
                        next_run_at: row.get(16)?,
                        running_at: row.get(17)?,
                        last_run_at: row.get(18)?,
                        last_status: row.get(19)?,
                        last_error: row.get(20)?,
                        created_at: row.get(21)?,
                        updated_at: row.get(22)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT job_id, user_id, session_id, agent_id, name, session_target, payload, deliver, enabled, delete_after_run, \
                 schedule_kind, schedule_at, schedule_every_ms, schedule_cron, schedule_tz, dedupe_key, next_run_at, running_at, \
                 last_run_at, last_status, last_error, created_at, updated_at FROM cron_jobs WHERE user_id = ? AND dedupe_key = ? LIMIT 1",
                params![cleaned_user, cleaned_key],
                |row| {
                    let payload_text: Option<String> = row.get(6)?;
                    let deliver_text: Option<String> = row.get(7)?;
                    let enabled: Option<i64> = row.get(8)?;
                    let delete_after: Option<i64> = row.get(9)?;
                    Ok(CronJobRecord {
                        job_id: row.get(0)?,
                        user_id: row.get(1)?,
                        session_id: row.get(2)?,
                        agent_id: row.get(3)?,
                        name: row.get(4)?,
                        session_target: row.get(5)?,
                        payload: Self::json_value_or_null(payload_text),
                        deliver: match deliver_text {
                            Some(text) => Self::json_from_str(&text),
                            None => None,
                        },
                        enabled: enabled.unwrap_or(0) != 0,
                        delete_after_run: delete_after.unwrap_or(0) != 0,
                        schedule_kind: row.get(10)?,
                        schedule_at: row.get(11)?,
                        schedule_every_ms: row.get(12)?,
                        schedule_cron: row.get(13)?,
                        schedule_tz: row.get(14)?,
                        dedupe_key: row.get(15)?,
                        next_run_at: row.get(16)?,
                        running_at: row.get(17)?,
                        last_run_at: row.get(18)?,
                        last_status: row.get(19)?,
                        last_error: row.get(20)?,
                        created_at: row.get(21)?,
                        updated_at: row.get(22)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn list_cron_jobs(&self, user_id: &str, include_disabled: bool) -> Result<Vec<CronJobRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut sql = String::from(
            "SELECT job_id, user_id, session_id, agent_id, name, session_target, payload, deliver, enabled, delete_after_run, \
             schedule_kind, schedule_at, schedule_every_ms, schedule_cron, schedule_tz, dedupe_key, next_run_at, running_at, \
             last_run_at, last_status, last_error, created_at, updated_at FROM cron_jobs WHERE user_id = ?",
        );
        if !include_disabled {
            sql.push_str(" AND enabled = 1");
        }
        sql.push_str(" ORDER BY updated_at DESC");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![cleaned_user], |row| {
            let payload_text: Option<String> = row.get(6)?;
            let deliver_text: Option<String> = row.get(7)?;
            let enabled: Option<i64> = row.get(8)?;
            let delete_after: Option<i64> = row.get(9)?;
            Ok(CronJobRecord {
                job_id: row.get(0)?,
                user_id: row.get(1)?,
                session_id: row.get(2)?,
                agent_id: row.get(3)?,
                name: row.get(4)?,
                session_target: row.get(5)?,
                payload: Self::json_value_or_null(payload_text),
                deliver: match deliver_text {
                    Some(text) => Self::json_from_str(&text),
                    None => None,
                },
                enabled: enabled.unwrap_or(0) != 0,
                delete_after_run: delete_after.unwrap_or(0) != 0,
                schedule_kind: row.get(10)?,
                schedule_at: row.get(11)?,
                schedule_every_ms: row.get(12)?,
                schedule_cron: row.get(13)?,
                schedule_tz: row.get(14)?,
                dedupe_key: row.get(15)?,
                next_run_at: row.get(16)?,
                running_at: row.get(17)?,
                last_run_at: row.get(18)?,
                last_status: row.get(19)?,
                last_error: row.get(20)?,
                created_at: row.get(21)?,
                updated_at: row.get(22)?,
            })
        })?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
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
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM cron_jobs WHERE user_id = ? AND job_id = ?",
            params![cleaned_user, cleaned_job],
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
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM cron_jobs WHERE user_id = ? AND session_id = ?",
            params![cleaned_user, cleaned_session],
        )?;
        Ok(affected as i64)
    }

    fn reset_cron_jobs_running(&self) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "UPDATE cron_jobs SET running_at = NULL WHERE running_at IS NOT NULL",
            [],
        )?;
        Ok(())
    }

    fn count_running_cron_jobs(&self) -> Result<i64> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM cron_jobs WHERE running_at IS NOT NULL",
            [],
            |row| row.get(0),
        )?;
        Ok(total)
    }

    fn claim_due_cron_jobs(&self, now: f64, limit: i64) -> Result<Vec<CronJobRecord>> {
        self.ensure_initialized()?;
        let limit = limit.max(0);
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let ids = {
            let mut stmt = tx.prepare(
                "SELECT job_id FROM cron_jobs WHERE enabled = 1 AND next_run_at IS NOT NULL AND next_run_at <= ? \
                 AND (running_at IS NULL) ORDER BY next_run_at ASC LIMIT ?",
            )?;
            let ids = stmt
                .query_map(params![now, limit], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<String>, _>>()?;
            ids
        };
        if ids.is_empty() {
            tx.commit()?;
            return Ok(Vec::new());
        }
        for id in &ids {
            tx.execute(
                "UPDATE cron_jobs SET running_at = ?, updated_at = ? WHERE job_id = ?",
                params![now, now, id],
            )?;
        }
        let placeholders = vec!["?"; ids.len()].join(", ");
        let sql = format!(
            "SELECT job_id, user_id, session_id, agent_id, name, session_target, payload, deliver, enabled, delete_after_run, \
             schedule_kind, schedule_at, schedule_every_ms, schedule_cron, schedule_tz, dedupe_key, next_run_at, running_at, \
             last_run_at, last_status, last_error, created_at, updated_at FROM cron_jobs WHERE job_id IN ({placeholders})"
        );
        let mut output = Vec::new();
        {
            let mut stmt = tx.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(ids.iter()), |row| {
                let payload_text: Option<String> = row.get(6)?;
                let deliver_text: Option<String> = row.get(7)?;
                let enabled: Option<i64> = row.get(8)?;
                let delete_after: Option<i64> = row.get(9)?;
                Ok(CronJobRecord {
                    job_id: row.get(0)?,
                    user_id: row.get(1)?,
                    session_id: row.get(2)?,
                    agent_id: row.get(3)?,
                    name: row.get(4)?,
                    session_target: row.get(5)?,
                    payload: Self::json_value_or_null(payload_text),
                    deliver: match deliver_text {
                        Some(text) => Self::json_from_str(&text),
                        None => None,
                    },
                    enabled: enabled.unwrap_or(0) != 0,
                    delete_after_run: delete_after.unwrap_or(0) != 0,
                    schedule_kind: row.get(10)?,
                    schedule_at: row.get(11)?,
                    schedule_every_ms: row.get(12)?,
                    schedule_cron: row.get(13)?,
                    schedule_tz: row.get(14)?,
                    dedupe_key: row.get(15)?,
                    next_run_at: row.get(16)?,
                    running_at: row.get(17)?,
                    last_run_at: row.get(18)?,
                    last_status: row.get(19)?,
                    last_error: row.get(20)?,
                    created_at: row.get(21)?,
                    updated_at: row.get(22)?,
                })
            })?;
            for record in rows.flatten() {
                output.push(record);
            }
        }
        tx.commit()?;
        Ok(output)
    }

    fn insert_cron_run(&self, record: &CronRunRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO cron_runs (run_id, job_id, user_id, session_id, agent_id, trigger, status, summary, error, duration_ms, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                record.run_id,
                record.job_id,
                record.user_id,
                record.session_id,
                record.agent_id,
                record.trigger,
                record.status,
                record.summary,
                record.error,
                record.duration_ms,
                record.created_at
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
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT run_id, job_id, user_id, session_id, agent_id, trigger, status, summary, error, duration_ms, created_at \
             FROM cron_runs WHERE user_id = ? AND job_id = ? ORDER BY created_at DESC LIMIT ?",
        )?;
        let rows = stmt.query_map(params![cleaned_user, cleaned_job, safe_limit], |row| {
            Ok(CronRunRecord {
                run_id: row.get(0)?,
                job_id: row.get(1)?,
                user_id: row.get(2)?,
                session_id: row.get(3)?,
                agent_id: row.get(4)?,
                trigger: row.get(5)?,
                status: row.get(6)?,
                summary: row.get(7)?,
                error: row.get(8)?,
                duration_ms: row.get::<_, Option<i64>>(9)?.unwrap_or(0),
                created_at: row.get::<_, Option<f64>>(10)?.unwrap_or(0.0),
            })
        })?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        Ok(output)
    }

    fn get_next_cron_run_at(&self, now: f64) -> Result<Option<f64>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let value: Option<f64> = conn
            .query_row(
                "SELECT MIN(next_run_at) FROM cron_jobs WHERE enabled = 1 AND next_run_at IS NOT NULL AND next_run_at > ?",
                params![now],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value)
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
        Ok(Some(UserToolAccessRecord {
            user_id: cleaned.to_string(),
            allowed_tools: raw.0.map(|value| Self::parse_string_list(Some(value))),
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
        if allowed_tools.is_some() {
            let payload = allowed_tools
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
        let hive_id = normalize_hive_id(&record.hive_id);
        conn.execute(
            "INSERT INTO user_agents (agent_id, user_id, hive_id, name, description, system_prompt, tool_names, access_level, is_shared, status, icon, sandbox_container_id, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(agent_id) DO UPDATE SET user_id = excluded.user_id, hive_id = excluded.hive_id, name = excluded.name, description = excluded.description, \
             system_prompt = excluded.system_prompt, tool_names = excluded.tool_names, access_level = excluded.access_level, \
             is_shared = excluded.is_shared, status = excluded.status, icon = excluded.icon, sandbox_container_id = excluded.sandbox_container_id, updated_at = excluded.updated_at",
            params![
                record.agent_id,
                record.user_id,
                hive_id,
                record.name,
                record.description,
                record.system_prompt,
                tool_names,
                record.access_level,
                if record.is_shared { 1 } else { 0 },
                record.status,
                record.icon,
                normalize_sandbox_container_id(record.sandbox_container_id),
                record.created_at,
                record.updated_at
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
                "SELECT agent_id, user_id, hive_id, name, description, system_prompt, tool_names, access_level, is_shared, status, icon, sandbox_container_id, created_at, updated_at                  FROM user_agents WHERE user_id = ? AND agent_id = ?",
                params![cleaned_user, cleaned_agent],
                |row| {
                    let tool_names: Option<String> = row.get(6)?;
                    let is_shared: Option<i64> = row.get(8)?;
                    let sandbox_container_id: Option<i64> = row.get(11)?;
                    Ok(UserAgentRecord {
                        agent_id: row.get(0)?,
                        user_id: row.get(1)?,
                        hive_id: normalize_hive_id(&row.get::<_, String>(2)?),
                        name: row.get(3)?,
                        description: row.get(4)?,
                        system_prompt: row.get(5)?,
                        tool_names: Self::parse_string_list(tool_names),
                        access_level: row.get(7)?,
                        is_shared: is_shared.unwrap_or(0) != 0,
                        status: row.get(9)?,
                        icon: row.get(10)?,
                        sandbox_container_id: normalize_sandbox_container_id(
                            sandbox_container_id.unwrap_or(1) as i32,
                        ),
                        created_at: row.get(12)?,
                        updated_at: row.get(13)?,
                    })
                },
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
                "SELECT agent_id, user_id, hive_id, name, description, system_prompt, tool_names, access_level, is_shared, status, icon, sandbox_container_id, created_at, updated_at                  FROM user_agents WHERE agent_id = ?",
                params![cleaned_agent],
                |row| {
                    let tool_names: Option<String> = row.get(6)?;
                    let is_shared: Option<i64> = row.get(8)?;
                    let sandbox_container_id: Option<i64> = row.get(11)?;
                    Ok(UserAgentRecord {
                        agent_id: row.get(0)?,
                        user_id: row.get(1)?,
                        hive_id: normalize_hive_id(&row.get::<_, String>(2)?),
                        name: row.get(3)?,
                        description: row.get(4)?,
                        system_prompt: row.get(5)?,
                        tool_names: Self::parse_string_list(tool_names),
                        access_level: row.get(7)?,
                        is_shared: is_shared.unwrap_or(0) != 0,
                        status: row.get(9)?,
                        icon: row.get(10)?,
                        sandbox_container_id: normalize_sandbox_container_id(
                            sandbox_container_id.unwrap_or(1) as i32,
                        ),
                        created_at: row.get(12)?,
                        updated_at: row.get(13)?,
                    })
                },
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
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, tool_names, access_level, is_shared, status, icon, sandbox_container_id, created_at, updated_at              FROM user_agents WHERE user_id = ? ORDER BY updated_at DESC",
        )?;
        let rows = stmt
            .query_map(params![cleaned_user], |row| {
                let tool_names: Option<String> = row.get(6)?;
                let is_shared: Option<i64> = row.get(8)?;
                let sandbox_container_id: Option<i64> = row.get(11)?;
                Ok(UserAgentRecord {
                    agent_id: row.get(0)?,
                    user_id: row.get(1)?,
                    hive_id: normalize_hive_id(&row.get::<_, String>(2)?),
                    name: row.get(3)?,
                    description: row.get(4)?,
                    system_prompt: row.get(5)?,
                    tool_names: Self::parse_string_list(tool_names),
                    access_level: row.get(7)?,
                    is_shared: is_shared.unwrap_or(0) != 0,
                    status: row.get(9)?,
                    icon: row.get(10)?,
                    sandbox_container_id: normalize_sandbox_container_id(
                        sandbox_container_id.unwrap_or(1) as i32,
                    ),
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
                })
            })?
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
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, tool_names, access_level, is_shared, status, icon, sandbox_container_id, created_at, updated_at              FROM user_agents WHERE user_id = ? AND hive_id = ? ORDER BY updated_at DESC",
        )?;
        let rows = stmt
            .query_map(params![cleaned_user, normalized_hive_id], |row| {
                let tool_names: Option<String> = row.get(6)?;
                let is_shared: Option<i64> = row.get(8)?;
                let sandbox_container_id: Option<i64> = row.get(11)?;
                Ok(UserAgentRecord {
                    agent_id: row.get(0)?,
                    user_id: row.get(1)?,
                    hive_id: normalize_hive_id(&row.get::<_, String>(2)?),
                    name: row.get(3)?,
                    description: row.get(4)?,
                    system_prompt: row.get(5)?,
                    tool_names: Self::parse_string_list(tool_names),
                    access_level: row.get(7)?,
                    is_shared: is_shared.unwrap_or(0) != 0,
                    status: row.get(9)?,
                    icon: row.get(10)?,
                    sandbox_container_id: normalize_sandbox_container_id(
                        sandbox_container_id.unwrap_or(1) as i32,
                    ),
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
                })
            })?
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
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, tool_names, access_level, is_shared, status, icon, sandbox_container_id, created_at, updated_at              FROM user_agents WHERE is_shared = 1 AND user_id <> ? ORDER BY updated_at DESC",
        )?;
        let rows = stmt
            .query_map(params![cleaned_user], |row| {
                let tool_names: Option<String> = row.get(6)?;
                let is_shared: Option<i64> = row.get(8)?;
                let sandbox_container_id: Option<i64> = row.get(11)?;
                Ok(UserAgentRecord {
                    agent_id: row.get(0)?,
                    user_id: row.get(1)?,
                    hive_id: normalize_hive_id(&row.get::<_, String>(2)?),
                    name: row.get(3)?,
                    description: row.get(4)?,
                    system_prompt: row.get(5)?,
                    tool_names: Self::parse_string_list(tool_names),
                    access_level: row.get(7)?,
                    is_shared: is_shared.unwrap_or(0) != 0,
                    status: row.get(9)?,
                    icon: row.get(10)?,
                    sandbox_container_id: normalize_sandbox_container_id(
                        sandbox_container_id.unwrap_or(1) as i32,
                    ),
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
                })
            })?
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
            "INSERT INTO team_runs (team_run_id, user_id, hive_id, parent_session_id, parent_agent_id, strategy, status, task_total, task_success, task_failed, context_tokens_total, context_tokens_peak, model_round_total, started_time, finished_time, elapsed_s, summary, error, updated_time)              VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)              ON CONFLICT(team_run_id) DO UPDATE SET user_id = excluded.user_id, hive_id = excluded.hive_id, parent_session_id = excluded.parent_session_id, parent_agent_id = excluded.parent_agent_id,              strategy = excluded.strategy, status = excluded.status, task_total = excluded.task_total, task_success = excluded.task_success, task_failed = excluded.task_failed,              context_tokens_total = excluded.context_tokens_total, context_tokens_peak = excluded.context_tokens_peak, model_round_total = excluded.model_round_total,              started_time = excluded.started_time, finished_time = excluded.finished_time, elapsed_s = excluded.elapsed_s, summary = excluded.summary, error = excluded.error, updated_time = excluded.updated_time",
            params![
                record.team_run_id,
                record.user_id,
                normalize_hive_id(&record.hive_id),
                record.parent_session_id,
                record.parent_agent_id,
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

    fn get_team_run(&self, team_run_id: &str) -> Result<Option<TeamRunRecord>> {
        self.ensure_initialized()?;
        let cleaned = team_run_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT team_run_id, user_id, hive_id, parent_session_id, parent_agent_id, strategy, status, task_total, task_success, task_failed, context_tokens_total, context_tokens_peak, model_round_total, started_time, finished_time, elapsed_s, summary, error, updated_time FROM team_runs WHERE team_run_id = ?",
                params![cleaned],
                |row| {
                    Ok(TeamRunRecord {
                        team_run_id: row.get(0)?,
                        user_id: row.get(1)?,
                        hive_id: normalize_hive_id(&row.get::<_, String>(2)?),
                        parent_session_id: row.get(3)?,
                        parent_agent_id: row.get(4)?,
                        strategy: row.get(5)?,
                        status: row.get(6)?,
                        task_total: row.get(7)?,
                        task_success: row.get(8)?,
                        task_failed: row.get(9)?,
                        context_tokens_total: row.get(10)?,
                        context_tokens_peak: row.get(11)?,
                        model_round_total: row.get(12)?,
                        started_time: row.get(13)?,
                        finished_time: row.get(14)?,
                        elapsed_s: row.get(15)?,
                        summary: row.get(16)?,
                        error: row.get(17)?,
                        updated_time: row.get(18)?,
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
            "SELECT team_run_id, user_id, hive_id, parent_session_id, parent_agent_id, strategy, status, task_total, task_success, task_failed, context_tokens_total, context_tokens_peak, model_round_total, started_time, finished_time, elapsed_s, summary, error, updated_time              FROM team_runs WHERE {where_clause} ORDER BY updated_time DESC LIMIT ? OFFSET ?"
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
                    strategy: row.get(5)?,
                    status: row.get(6)?,
                    task_total: row.get(7)?,
                    task_success: row.get(8)?,
                    task_failed: row.get(9)?,
                    context_tokens_total: row.get(10)?,
                    context_tokens_peak: row.get(11)?,
                    model_round_total: row.get(12)?,
                    started_time: row.get(13)?,
                    finished_time: row.get(14)?,
                    elapsed_s: row.get(15)?,
                    summary: row.get(16)?,
                    error: row.get(17)?,
                    updated_time: row.get(18)?,
                })
            })?
            .collect::<std::result::Result<Vec<TeamRunRecord>, _>>()?;
        Ok((rows, total))
    }

    fn upsert_team_task(&self, record: &TeamTaskRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO team_tasks (task_id, team_run_id, user_id, hive_id, agent_id, target_session_id, spawned_session_id, status, retry_count, priority, started_time, finished_time, elapsed_s, result_summary, error, updated_time)              VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)              ON CONFLICT(task_id) DO UPDATE SET team_run_id = excluded.team_run_id, user_id = excluded.user_id, hive_id = excluded.hive_id, agent_id = excluded.agent_id,              target_session_id = excluded.target_session_id, spawned_session_id = excluded.spawned_session_id, status = excluded.status, retry_count = excluded.retry_count,              priority = excluded.priority, started_time = excluded.started_time, finished_time = excluded.finished_time, elapsed_s = excluded.elapsed_s,              result_summary = excluded.result_summary, error = excluded.error, updated_time = excluded.updated_time",
            params![
                record.task_id,
                record.team_run_id,
                record.user_id,
                normalize_hive_id(&record.hive_id),
                record.agent_id,
                record.target_session_id,
                record.spawned_session_id,
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
            "SELECT task_id, team_run_id, user_id, hive_id, agent_id, target_session_id, spawned_session_id, status, retry_count, priority, started_time, finished_time, elapsed_s, result_summary, error, updated_time              FROM team_tasks WHERE team_run_id = ? ORDER BY updated_time DESC",
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
                    status: row.get(7)?,
                    retry_count: row.get(8)?,
                    priority: row.get(9)?,
                    started_time: row.get(10)?,
                    finished_time: row.get(11)?,
                    elapsed_s: row.get(12)?,
                    result_summary: row.get(13)?,
                    error: row.get(14)?,
                    updated_time: row.get(15)?,
                })
            })?
            .collect::<std::result::Result<Vec<TeamTaskRecord>, _>>()?;
        Ok(rows)
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
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row = tx
            .query_row(
                "SELECT daily_quota, daily_quota_used, daily_quota_date FROM user_accounts WHERE user_id = ?",
                params![cleaned],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((daily_quota, daily_used, daily_date)) = row else {
            tx.commit()?;
            return Ok(None);
        };
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
                "UPDATE user_accounts SET daily_quota_used = ?, daily_quota_date = ? WHERE user_id = ?",
                params![used, today, cleaned],
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
