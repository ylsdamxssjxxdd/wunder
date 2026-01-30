use crate::i18n;
use crate::storage::{
    ChatSessionRecord, OrgUnitRecord, SessionLockRecord, SessionLockStatus, StorageBackend,
    UserAccountRecord, UserAgentAccessRecord, UserAgentRecord, UserQuotaStatus, UserTokenRecord,
    UserToolAccessRecord, VectorDocumentRecord, VectorDocumentSummaryRecord,
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

    fn string_list_to_json(list: &[String]) -> String {
        serde_json::to_string(list).unwrap_or_else(|_| "[]".to_string())
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
        if !columns.contains("agent_id") {
            conn.execute("ALTER TABLE chat_sessions ADD COLUMN agent_id TEXT", &[])?;
        }
        if !columns.contains("tool_overrides") {
            conn.execute(
                "ALTER TABLE chat_sessions ADD COLUMN tool_overrides TEXT",
                &[],
            )?;
        }
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
        conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_session_locks_user_agent \
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
                CREATE UNIQUE INDEX IF NOT EXISTS idx_session_locks_user_agent
                  ON session_locks (user_id, agent_id);
                CREATE INDEX IF NOT EXISTS idx_session_locks_expires
                  ON session_locks (expires_at);
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
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL,
                  last_message_at DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_chat_sessions_user
                  ON chat_sessions (user_id);
                CREATE INDEX IF NOT EXISTS idx_chat_sessions_updated
                  ON chat_sessions (user_id, updated_at);
                CREATE TABLE IF NOT EXISTS user_agents (
                  agent_id TEXT PRIMARY KEY,
                  user_id TEXT NOT NULL,
                  name TEXT NOT NULL,
                  description TEXT,
                  system_prompt TEXT,
                  tool_names TEXT,
                  access_level TEXT NOT NULL,
                  is_shared INTEGER NOT NULL DEFAULT 0,
                  status TEXT NOT NULL,
                  icon TEXT,
                  created_at DOUBLE PRECISION NOT NULL,
                  updated_at DOUBLE PRECISION NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_user_agents_user
                  ON user_agents (user_id, updated_at);
                CREATE TABLE IF NOT EXISTS user_agent_access (
                  user_id TEXT PRIMARY KEY,
                  allowed_agent_ids TEXT,
                  blocked_agent_ids TEXT,
                  updated_at DOUBLE PRECISION NOT NULL
                );
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
                    self.ensure_session_lock_columns(&mut conn)?;
                    self.ensure_user_agent_columns(&mut conn)?;
                    self.ensure_performance_indexes(&mut conn)?;
                    self.initialized.store(true, Ordering::SeqCst);
                    return Ok(());
                }
                Err(err) => {
                    if attempts >= 5 {
                        return Err(err.into());
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
        let rows = conn.query(
            "SELECT user_id, COUNT(*) as tool_records, MAX(created_time) as last_time FROM tool_logs GROUP BY user_id",
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
        let existing = tx.query_opt(
            "SELECT session_id FROM session_locks WHERE user_id = $1 AND agent_id = $2 LIMIT 1",
            &[&cleaned_user, &cleaned_agent],
        )?;
        if existing.is_some() {
            tx.commit()?;
            return Ok(SessionLockStatus::UserBusy);
        }
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
            let user_lock = tx.query_opt(
                "SELECT session_id FROM session_locks WHERE user_id = $1 AND agent_id = $2 LIMIT 1",
                &[&cleaned_user, &cleaned_agent],
            )?;
            tx.commit()?;
            return Ok(if user_lock.is_some() {
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
        let safe_limit = _max_records.max(1);
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO memory_records (user_id, session_id, summary, created_time, updated_time) VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT(user_id, session_id) DO UPDATE SET summary = EXCLUDED.summary, updated_time = EXCLUDED.updated_time",
            &[&cleaned_user, &cleaned_session, &cleaned_summary, &_now_ts, &_now_ts],
        )?;
        conn.execute(
            "DELETE FROM memory_records WHERE user_id = $1 AND id NOT IN (\
                SELECT id FROM memory_records WHERE user_id = $1 ORDER BY updated_time DESC, id DESC LIMIT $2\
             )",
            &[&cleaned_user, &safe_limit],
        )?;
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
        if cleaned.is_empty() || _limit <= 0 {
            return Ok(Vec::new());
        }
        let direction = if _order_desc { "DESC" } else { "ASC" };
        let query = format!(
            "SELECT session_id, summary, created_time, updated_time FROM memory_records WHERE user_id = $1 ORDER BY updated_time {direction}, id {direction} LIMIT $2"
        );
        let mut conn = self.conn()?;
        let rows = conn.query(&query, &[&cleaned, &_limit])?;
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

    fn upsert_memory_task_log(
        &self,
        _user_id: &str,
        _session_id: &str,
        _task_id: &str,
        _status: &str,
        _queued_time: f64,
        _started_time: f64,
        _finished_time: f64,
        _elapsed_s: f64,
        _request_payload: Option<&Value>,
        _result: &str,
        _error: &str,
        _updated_time: Option<f64>,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = _user_id.trim();
        let cleaned_session = _session_id.trim();
        let cleaned_task = _task_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() || cleaned_task.is_empty() {
            return Ok(());
        }
        let status_text = _status.trim();
        let payload_text = _request_payload
            .map(Self::json_to_string)
            .unwrap_or_default();
        let now = _updated_time.unwrap_or_else(Self::now_ts);
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO memory_task_logs (task_id, user_id, session_id, status, queued_time, started_time, finished_time, elapsed_s, request_payload, result, error, updated_time) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) \
             ON CONFLICT(user_id, session_id) DO UPDATE SET \
               task_id = EXCLUDED.task_id, status = EXCLUDED.status, queued_time = EXCLUDED.queued_time, started_time = EXCLUDED.started_time, \
               finished_time = EXCLUDED.finished_time, elapsed_s = EXCLUDED.elapsed_s, request_payload = EXCLUDED.request_payload, result = EXCLUDED.result, \
               error = EXCLUDED.error, updated_time = EXCLUDED.updated_time",
            &[
                &cleaned_task,
                &cleaned_user,
                &cleaned_session,
                &status_text,
                &_queued_time,
                &_started_time,
                &_finished_time,
                &_elapsed_s,
                &payload_text,
                &_result,
                &_error,
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
        let mut results = HashMap::new();
        let chat = conn.execute(
            "DELETE FROM chat_history WHERE created_time < $1",
            &[&cutoff],
        )?;
        results.insert("chat_history".to_string(), chat as i64);
        let tool = conn.execute("DELETE FROM tool_logs WHERE created_time < $1", &[&cutoff])?;
        results.insert("tool_logs".to_string(), tool as i64);
        let artifact = conn.execute(
            "DELETE FROM artifact_logs WHERE created_time < $1",
            &[&cutoff],
        )?;
        results.insert("artifact_logs".to_string(), artifact as i64);
        let monitor = conn.execute(
            "DELETE FROM monitor_sessions WHERE updated_time < $1",
            &[&cutoff],
        )?;
        results.insert("monitor_sessions".to_string(), monitor as i64);
        let stream = conn.execute(
            "DELETE FROM stream_events WHERE created_time < $1",
            &[&cutoff],
        )?;
        results.insert("stream_events".to_string(), stream as i64);
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
            "INSERT INTO chat_sessions (session_id, user_id, title, status, created_at, updated_at, last_message_at, agent_id, tool_overrides) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
             ON CONFLICT(session_id) DO UPDATE SET user_id = EXCLUDED.user_id, title = EXCLUDED.title, status = EXCLUDED.status, \
             created_at = EXCLUDED.created_at, updated_at = EXCLUDED.updated_at, last_message_at = EXCLUDED.last_message_at, \
             agent_id = EXCLUDED.agent_id, tool_overrides = EXCLUDED.tool_overrides",
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
            "SELECT session_id, user_id, title, created_at, updated_at, last_message_at, agent_id, tool_overrides \
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
        }))
    }

    fn list_chat_sessions(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChatSessionRecord>, i64)> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok((Vec::new(), 0));
        }
        let mut conn = self.conn()?;
        let agent_id = agent_id.map(|value| value.trim());
        let (total, rows) = match agent_id {
            None => {
                let total: i64 = conn
                    .query_one(
                        "SELECT COUNT(*) FROM chat_sessions WHERE user_id = $1",
                        &[&cleaned_user],
                    )?
                    .get(0);
                let rows = if limit > 0 {
                    conn.query(
                        "SELECT session_id, user_id, title, created_at, updated_at, last_message_at, agent_id, tool_overrides \
                         FROM chat_sessions WHERE user_id = $1 ORDER BY updated_at DESC LIMIT $2 OFFSET $3",
                        &[&cleaned_user, &limit, &offset.max(0)],
                    )?
                } else {
                    conn.query(
                        "SELECT session_id, user_id, title, created_at, updated_at, last_message_at, agent_id, tool_overrides \
                         FROM chat_sessions WHERE user_id = $1 ORDER BY updated_at DESC",
                        &[&cleaned_user],
                    )?
                };
                (total, rows)
            }
            Some(value) if value.is_empty() => {
                let total: i64 = conn
                    .query_one(
                        "SELECT COUNT(*) FROM chat_sessions WHERE user_id = $1 AND (agent_id IS NULL OR agent_id = '')",
                        &[&cleaned_user],
                    )?
                    .get(0);
                let rows = if limit > 0 {
                    conn.query(
                        "SELECT session_id, user_id, title, created_at, updated_at, last_message_at, agent_id, tool_overrides \
                         FROM chat_sessions WHERE user_id = $1 AND (agent_id IS NULL OR agent_id = '') \
                         ORDER BY updated_at DESC LIMIT $2 OFFSET $3",
                        &[&cleaned_user, &limit, &offset.max(0)],
                    )?
                } else {
                    conn.query(
                        "SELECT session_id, user_id, title, created_at, updated_at, last_message_at, agent_id, tool_overrides \
                         FROM chat_sessions WHERE user_id = $1 AND (agent_id IS NULL OR agent_id = '') \
                         ORDER BY updated_at DESC",
                        &[&cleaned_user],
                    )?
                };
                (total, rows)
            }
            Some(value) => {
                let total: i64 = conn
                    .query_one(
                        "SELECT COUNT(*) FROM chat_sessions WHERE user_id = $1 AND agent_id = $2",
                        &[&cleaned_user, &value],
                    )?
                    .get(0);
                let rows = if limit > 0 {
                    conn.query(
                        "SELECT session_id, user_id, title, created_at, updated_at, last_message_at, agent_id, tool_overrides \
                         FROM chat_sessions WHERE user_id = $1 AND agent_id = $2 \
                         ORDER BY updated_at DESC LIMIT $3 OFFSET $4",
                        &[&cleaned_user, &value, &limit, &offset.max(0)],
                    )?
                } else {
                    conn.query(
                        "SELECT session_id, user_id, title, created_at, updated_at, last_message_at, agent_id, tool_overrides \
                         FROM chat_sessions WHERE user_id = $1 AND agent_id = $2 \
                         ORDER BY updated_at DESC",
                        &[&cleaned_user, &value],
                    )?
                };
                (total, rows)
            }
        };
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
            });
        }
        Ok((output, total))
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
        conn.execute(
            "INSERT INTO user_agents (agent_id, user_id, name, description, system_prompt, tool_names, access_level, is_shared, status, icon, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) \
             ON CONFLICT(agent_id) DO UPDATE SET user_id = EXCLUDED.user_id, name = EXCLUDED.name, description = EXCLUDED.description, \
             system_prompt = EXCLUDED.system_prompt, tool_names = EXCLUDED.tool_names, access_level = EXCLUDED.access_level, \
             is_shared = EXCLUDED.is_shared, status = EXCLUDED.status, icon = EXCLUDED.icon, updated_at = EXCLUDED.updated_at",
            &[
                &record.agent_id,
                &record.user_id,
                &record.name,
                &record.description,
                &record.system_prompt,
                &tool_names,
                &record.access_level,
                &is_shared,
                &record.status,
                &record.icon,
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
            "SELECT agent_id, user_id, name, description, system_prompt, tool_names, access_level, is_shared, status, icon, created_at, updated_at \
             FROM user_agents WHERE user_id = $1 AND agent_id = $2",
            &[&cleaned_user, &cleaned_agent],
        )?;
        Ok(row.map(|row| UserAgentRecord {
            agent_id: row.get(0),
            user_id: row.get(1),
            name: row.get(2),
            description: row.get(3),
            system_prompt: row.get(4),
            tool_names: Self::parse_string_list(row.get(5)),
            access_level: row.get(6),
            is_shared: row.get::<_, i32>(7) != 0,
            status: row.get(8),
            icon: row.get(9),
            created_at: row.get(10),
            updated_at: row.get(11),
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
            "SELECT agent_id, user_id, name, description, system_prompt, tool_names, access_level, is_shared, status, icon, created_at, updated_at \
             FROM user_agents WHERE agent_id = $1",
            &[&cleaned_agent],
        )?;
        Ok(row.map(|row| UserAgentRecord {
            agent_id: row.get(0),
            user_id: row.get(1),
            name: row.get(2),
            description: row.get(3),
            system_prompt: row.get(4),
            tool_names: Self::parse_string_list(row.get(5)),
            access_level: row.get(6),
            is_shared: row.get::<_, i32>(7) != 0,
            status: row.get(8),
            icon: row.get(9),
            created_at: row.get(10),
            updated_at: row.get(11),
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
            "SELECT agent_id, user_id, name, description, system_prompt, tool_names, access_level, is_shared, status, icon, created_at, updated_at \
             FROM user_agents WHERE user_id = $1 ORDER BY updated_at DESC",
            &[&cleaned_user],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(UserAgentRecord {
                agent_id: row.get(0),
                user_id: row.get(1),
                name: row.get(2),
                description: row.get(3),
                system_prompt: row.get(4),
                tool_names: Self::parse_string_list(row.get(5)),
                access_level: row.get(6),
                is_shared: row.get::<_, i32>(7) != 0,
                status: row.get(8),
                icon: row.get(9),
                created_at: row.get(10),
                updated_at: row.get(11),
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
            "SELECT agent_id, user_id, name, description, system_prompt, tool_names, access_level, is_shared, status, icon, created_at, updated_at \
             FROM user_agents WHERE is_shared = 1 AND user_id <> $1 ORDER BY updated_at DESC",
            &[&cleaned_user],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(UserAgentRecord {
                agent_id: row.get(0),
                user_id: row.get(1),
                name: row.get(2),
                description: row.get(3),
                system_prompt: row.get(4),
                tool_names: Self::parse_string_list(row.get(5)),
                access_level: row.get(6),
                is_shared: row.get::<_, i32>(7) != 0,
                status: row.get(8),
                icon: row.get(9),
                created_at: row.get(10),
                updated_at: row.get(11),
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
