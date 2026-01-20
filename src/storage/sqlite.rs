// SQLite 存储实现：参考 Python 版结构，统一持久化历史/监控/记忆数据。
use crate::i18n;
use crate::storage::{
    ChatSessionRecord, SessionLockStatus, StorageBackend, UserAccountRecord, UserTokenRecord,
};
use anyhow::Result;
use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::types::Value as SqlValue;
use rusqlite::{
    params, params_from_iter, Connection, ErrorCode, OptionalExtension, TransactionBehavior,
};
use serde_json::{json, Value};
use std::collections::HashMap;
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

    fn string_list_to_json(list: &[String]) -> String {
        serde_json::to_string(list).unwrap_or_else(|_| "[]".to_string())
    }
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
              created_time REAL NOT NULL,
              updated_time REAL NOT NULL,
              expires_at REAL NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_session_locks_user
              ON session_locks (user_id);
            CREATE INDEX IF NOT EXISTS idx_session_locks_expires
              ON session_locks (expires_at);
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
              is_demo INTEGER NOT NULL DEFAULT 0,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              last_login_at REAL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_user_accounts_username
              ON user_accounts (username);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_user_accounts_email
              ON user_accounts (email);
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
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              last_message_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_chat_sessions_user
              ON chat_sessions (user_id);
            CREATE INDEX IF NOT EXISTS idx_chat_sessions_updated
              ON chat_sessions (user_id, updated_at);
            "#,
        )?;
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
        let payload_text = Self::json_to_string(payload);
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
        let mut stmt = conn.prepare(
            "SELECT user_id, COUNT(*) as tool_records, MAX(created_time) as last_time FROM tool_logs GROUP BY user_id",
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
        ttl_s: f64,
        max_sessions: i64,
    ) -> Result<SessionLockStatus> {
        self.ensure_initialized()?;
        let cleaned_session = session_id.trim();
        let cleaned_user = user_id.trim();
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
        let existing: Option<String> = tx
            .query_row(
                "SELECT session_id FROM session_locks WHERE user_id = ? LIMIT 1",
                params![cleaned_user],
                |row| row.get(0),
            )
            .optional()?;
        if existing.is_some() {
            tx.commit()?;
            return Ok(SessionLockStatus::UserBusy);
        }
        let total: i64 =
            tx.query_row("SELECT COUNT(*) FROM session_locks", [], |row| row.get(0))?;
        if total >= max_sessions {
            tx.commit()?;
            return Ok(SessionLockStatus::SystemBusy);
        }
        let insert = tx.execute(
            "INSERT INTO session_locks (session_id, user_id, created_time, updated_time, expires_at) VALUES (?, ?, ?, ?, ?)",
            params![cleaned_session, cleaned_user, now, now, expires_at],
        );
        match insert {
            Ok(_) => {
                tx.commit()?;
                Ok(SessionLockStatus::Acquired)
            }
            Err(rusqlite::Error::SqliteFailure(err, _))
                if matches!(err.code, ErrorCode::ConstraintViolation) =>
            {
                tx.commit()?;
                Ok(SessionLockStatus::SystemBusy)
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
        let safe_limit = max_records.max(1);
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO memory_records (user_id, session_id, summary, created_time, updated_time) VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT(user_id, session_id) DO UPDATE SET summary = excluded.summary, updated_time = excluded.updated_time",
            params![cleaned_user, cleaned_session, cleaned_summary, now_ts, now_ts],
        )?;
        conn.execute(
            "DELETE FROM memory_records WHERE user_id = ? AND id NOT IN (\
                SELECT id FROM memory_records WHERE user_id = ? ORDER BY updated_time DESC, id DESC LIMIT ?\
             )",
            params![cleaned_user, cleaned_user, safe_limit],
        )?;
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
        if cleaned.is_empty() || limit <= 0 {
            return Ok(Vec::new());
        }
        let direction = if order_desc { "DESC" } else { "ASC" };
        let query = format!(
            "SELECT session_id, summary, created_time, updated_time FROM memory_records WHERE user_id = ? ORDER BY updated_time {direction}, id {direction} LIMIT ?"
        );
        let conn = self.open()?;
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt
            .query_map(params![cleaned, limit], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2).unwrap_or(0.0),
                    row.get::<_, f64>(3).unwrap_or(0.0),
                ))
            })?
            .collect::<std::result::Result<Vec<(String, String, f64, f64)>, _>>()?;
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

    fn upsert_memory_task_log(
        &self,
        user_id: &str,
        session_id: &str,
        task_id: &str,
        status: &str,
        queued_time: f64,
        started_time: f64,
        finished_time: f64,
        elapsed_s: f64,
        request_payload: Option<&Value>,
        result: &str,
        error: &str,
        updated_time: Option<f64>,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        let cleaned_task = task_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() || cleaned_task.is_empty() {
            return Ok(());
        }
        let status_text = status.trim();
        let payload_text = request_payload
            .map(Self::json_to_string)
            .unwrap_or_default();
        let now = updated_time.unwrap_or_else(Self::now_ts);
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO memory_task_logs (task_id, user_id, session_id, status, queued_time, started_time, finished_time, elapsed_s, request_payload, result, error, updated_time) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(user_id, session_id) DO UPDATE SET \
               task_id = excluded.task_id, status = excluded.status, queued_time = excluded.queued_time, started_time = excluded.started_time, \
               finished_time = excluded.finished_time, elapsed_s = excluded.elapsed_s, request_payload = excluded.request_payload, result = excluded.result, \
               error = excluded.error, updated_time = excluded.updated_time",
            params![
                cleaned_task,
                cleaned_user,
                cleaned_session,
                status_text,
                queued_time,
                started_time,
                finished_time,
                elapsed_s,
                payload_text,
                result,
                error,
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
        let chat = conn.execute(
            "DELETE FROM chat_history WHERE created_time < ?",
            params![cutoff],
        )?;
        results.insert("chat_history".to_string(), chat as i64);
        let tool = conn.execute(
            "DELETE FROM tool_logs WHERE created_time < ?",
            params![cutoff],
        )?;
        results.insert("tool_logs".to_string(), tool as i64);
        let artifact = conn.execute(
            "DELETE FROM artifact_logs WHERE created_time < ?",
            params![cutoff],
        )?;
        results.insert("artifact_logs".to_string(), artifact as i64);
        let monitor = conn.execute(
            "DELETE FROM monitor_sessions WHERE COALESCE(updated_time, 0) < ?",
            params![cutoff],
        )?;
        results.insert("monitor_sessions".to_string(), monitor as i64);
        let stream = conn.execute(
            "DELETE FROM stream_events WHERE created_time < ?",
            params![cutoff],
        )?;
        results.insert("stream_events".to_string(), stream as i64);
        Ok(results)
    }

    fn upsert_user_account(&self, record: &UserAccountRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let roles = Self::string_list_to_json(&record.roles);
        conn.execute(
            "INSERT INTO user_accounts (user_id, username, email, password_hash, roles, status, access_level, is_demo, created_at, updated_at, last_login_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(user_id) DO UPDATE SET username = excluded.username, email = excluded.email, password_hash = excluded.password_hash, \
             roles = excluded.roles, status = excluded.status, access_level = excluded.access_level, is_demo = excluded.is_demo, \
             created_at = excluded.created_at, updated_at = excluded.updated_at, last_login_at = excluded.last_login_at",
            params![
                record.user_id,
                record.username,
                record.email,
                record.password_hash,
                roles,
                record.status,
                record.access_level,
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
                "SELECT user_id, username, email, password_hash, roles, status, access_level, is_demo, created_at, updated_at, last_login_at \
                 FROM user_accounts WHERE user_id = ?",
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
                        is_demo: row.get::<_, i64>(7)? != 0,
                        created_at: row.get(8)?,
                        updated_at: row.get(9)?,
                        last_login_at: row.get(10)?,
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
                "SELECT user_id, username, email, password_hash, roles, status, access_level, is_demo, created_at, updated_at, last_login_at \
                 FROM user_accounts WHERE username = ?",
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
                        is_demo: row.get::<_, i64>(7)? != 0,
                        created_at: row.get(8)?,
                        updated_at: row.get(9)?,
                        last_login_at: row.get(10)?,
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
                "SELECT user_id, username, email, password_hash, roles, status, access_level, is_demo, created_at, updated_at, last_login_at \
                 FROM user_accounts WHERE email = ?",
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
                        is_demo: row.get::<_, i64>(7)? != 0,
                        created_at: row.get(8)?,
                        updated_at: row.get(9)?,
                        last_login_at: row.get(10)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn list_user_accounts(
        &self,
        keyword: Option<&str>,
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
            "SELECT user_id, username, email, password_hash, roles, status, access_level, is_demo, created_at, updated_at, last_login_at \
             FROM user_accounts",
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
                    is_demo: row.get::<_, i64>(7)? != 0,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                    last_login_at: row.get(10)?,
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
        conn.execute(
            "INSERT INTO chat_sessions (session_id, user_id, title, status, created_at, updated_at, last_message_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(session_id) DO UPDATE SET user_id = excluded.user_id, title = excluded.title, \
             status = excluded.status, created_at = excluded.created_at, updated_at = excluded.updated_at, \
             last_message_at = excluded.last_message_at",
            params![
                record.session_id,
                record.user_id,
                record.title,
                "active",
                record.created_at,
                record.updated_at,
                record.last_message_at
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
                "SELECT session_id, user_id, title, created_at, updated_at, last_message_at \
                 FROM chat_sessions WHERE user_id = ? AND session_id = ?",
                params![cleaned_user, cleaned_session],
                |row| {
                    Ok(ChatSessionRecord {
                        session_id: row.get(0)?,
                        user_id: row.get(1)?,
                        title: row.get(2)?,
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                        last_message_at: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn list_chat_sessions(
        &self,
        user_id: &str,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChatSessionRecord>, i64)> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok((Vec::new(), 0));
        }
        let conn = self.open()?;
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM chat_sessions WHERE user_id = ?",
            params![cleaned_user],
            |row| row.get(0),
        )?;
        let mut sql = String::from(
            "SELECT session_id, user_id, title, created_at, updated_at, last_message_at \
             FROM chat_sessions WHERE user_id = ? ORDER BY updated_at DESC",
        );
        let mut params_list: Vec<SqlValue> = vec![SqlValue::from(cleaned_user.to_string())];
        if limit > 0 {
            sql.push_str(" LIMIT ? OFFSET ?");
            params_list.push(SqlValue::from(limit));
            params_list.push(SqlValue::from(offset.max(0)));
        }
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(params_list.iter()), |row| {
                Ok(ChatSessionRecord {
                    session_id: row.get(0)?,
                    user_id: row.get(1)?,
                    title: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    last_message_at: row.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<ChatSessionRecord>, _>>()?;
        Ok((rows, total))
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

    fn get_user_tool_access(&self, user_id: &str) -> Result<Option<Vec<String>>> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row: Option<String> = conn
            .query_row(
                "SELECT allowed_tools FROM user_tool_access WHERE user_id = ?",
                params![cleaned],
                |row| row.get(0),
            )
            .optional()?;
        let Some(raw) = row else {
            return Ok(None);
        };
        Ok(Some(Self::parse_string_list(Some(raw))))
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
        if let Some(list) = allowed_tools {
            let payload = Self::string_list_to_json(list);
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
}
