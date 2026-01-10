use crate::i18n;
use crate::storage::{SessionLockStatus, StorageBackend};
use anyhow::{anyhow, Result};
use chrono::Utc;
use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio_postgres::types::ToSql;
use tokio_postgres::NoTls;

const SESSION_LOCK_ADVISORY_KEY: i64 = 742815;

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
    pub fn new(dsn: String, connect_timeout_s: u64) -> Result<Self> {
        let cleaned = dsn.trim().to_string();
        if cleaned.is_empty() {
            return Err(anyhow!("postgres dsn is empty"));
        }
        let timeout = Duration::from_secs(connect_timeout_s.max(1));
        let mut config = cleaned.parse::<tokio_postgres::Config>()?;
        config.connect_timeout(timeout);
        let manager_config = ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        };
        let manager = Manager::from_config(config, NoTls, manager_config);
        let pool = Pool::builder(manager).max_size(16).build()?;
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
                  updated_time DOUBLE PRECISION,
                  payload TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_monitor_sessions_status
                  ON monitor_sessions (status);
                CREATE TABLE IF NOT EXISTS session_locks (
                  session_id TEXT PRIMARY KEY,
                  user_id TEXT NOT NULL,
                  created_time DOUBLE PRECISION NOT NULL,
                  updated_time DOUBLE PRECISION NOT NULL,
                  expires_at DOUBLE PRECISION NOT NULL
                );
                CREATE UNIQUE INDEX IF NOT EXISTS idx_session_locks_user
                  ON session_locks (user_id);
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
                "#,
            );
            match result {
                Ok(_) => {
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
        let payload_text = Self::json_to_string(payload);
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
        _ttl_s: f64,
        _max_sessions: i64,
    ) -> Result<SessionLockStatus> {
        self.ensure_initialized()?;
        let cleaned_session = _session_id.trim();
        let cleaned_user = _user_id.trim();
        if cleaned_session.is_empty() || cleaned_user.is_empty() {
            return Ok(SessionLockStatus::SystemBusy);
        }
        let max_sessions = _max_sessions.max(1);
        let ttl_s = _ttl_s.max(1.0);
        let now = Self::now_ts();
        let expires_at = now + ttl_s;

        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        let locked: bool = tx
            .query_one(
                "SELECT pg_try_advisory_xact_lock($1)",
                &[&SESSION_LOCK_ADVISORY_KEY],
            )?
            .get(0);
        if !locked {
            return Ok(SessionLockStatus::SystemBusy);
        }
        tx.execute("DELETE FROM session_locks WHERE expires_at <= $1", &[&now])?;
        let existing = tx.query_opt(
            "SELECT session_id FROM session_locks WHERE user_id = $1 LIMIT 1",
            &[&cleaned_user],
        )?;
        if existing.is_some() {
            tx.commit()?;
            return Ok(SessionLockStatus::UserBusy);
        }
        let total: i64 = tx
            .query_one("SELECT COUNT(*) FROM session_locks", &[])?
            .get(0);
        if total >= max_sessions {
            tx.commit()?;
            return Ok(SessionLockStatus::SystemBusy);
        }
        let inserted = tx.execute(
            "INSERT INTO session_locks (session_id, user_id, created_time, updated_time, expires_at) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT DO NOTHING",
            &[&cleaned_session, &cleaned_user, &now, &now, &expires_at],
        )?;
        if inserted > 0 {
            tx.commit()?;
            return Ok(SessionLockStatus::Acquired);
        }
        let user_lock = tx.query_opt(
            "SELECT session_id FROM session_locks WHERE user_id = $1 LIMIT 1",
            &[&cleaned_user],
        )?;
        tx.commit()?;
        Ok(if user_lock.is_some() {
            SessionLockStatus::UserBusy
        } else {
            SessionLockStatus::SystemBusy
        })
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

    fn append_evaluation_item(&self, run_id: &str, payload: &Value) -> Result<()> {
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
        let items_deleted =
            tx.execute("DELETE FROM evaluation_items WHERE run_id = $1", &[&cleaned])?;
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
            "DELETE FROM monitor_sessions WHERE COALESCE(updated_time, 0) < $1",
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
}
