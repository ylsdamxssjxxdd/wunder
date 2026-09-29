use super::thread_log_store::SqliteThreadLogStorage;
use super::SqliteStorage;
use crate::i18n;
use crate::services::{
    chat_payload_sanitizer::{
        parse_sanitized_persisted_chat_payload, sanitize_persisted_chat_payload,
    },
    output_quality,
};
use crate::storage::StorageLifecycle;
use anyhow::Result;
use rusqlite::params;
use serde_json::{json, Value};

pub(super) trait SqliteConversationLogStorage {
    fn append_chat_impl(&self, user_id: &str, payload: &Value) -> Result<()>;
    fn append_tool_log_impl(&self, user_id: &str, payload: &Value) -> Result<()>;
    fn append_artifact_log_impl(&self, user_id: &str, payload: &Value) -> Result<()>;
    fn load_chat_history_impl(
        &self,
        user_id: &str,
        session_id: &str,
        limit: Option<i64>,
    ) -> Result<Vec<Value>>;
    fn load_chat_history_page_impl(
        &self,
        user_id: &str,
        session_id: &str,
        before_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<Value>>;
    fn load_artifact_logs_impl(
        &self,
        user_id: &str,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<Value>>;
    fn get_session_system_prompt_impl(
        &self,
        user_id: &str,
        session_id: &str,
        language: Option<&str>,
    ) -> Result<Option<String>>;
}

impl SqliteConversationLogStorage for SqliteStorage {
    fn append_chat_impl(&self, user_id: &str, payload: &Value) -> Result<()> {
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
        let payload = sanitize_persisted_chat_payload(&payload);
        let payload_text = Self::json_to_string(&payload);
        let now = Self::now_ts();
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO chat_history (user_id, session_id, role, payload, created_time) \
             VALUES (?, ?, ?, ?, ?)",
            params![user_id, session_id, role, payload_text, now],
        )?;
        if payload
            .get("user_round")
            .and_then(Value::as_i64)
            .is_some_and(|round| round > 0)
        {
            let mut timeline_payload = payload.clone();
            if let Value::Object(map) = &mut timeline_payload {
                map.insert("kind".into(), Value::String(format!("{}_message", role)));
                map.insert(
                    "status".into(),
                    Value::String(
                        if role == "user" {
                            "running"
                        } else {
                            "completed"
                        }
                        .into(),
                    ),
                );
            }
            self.append_thread_item_impl(user_id, &timeline_payload)?;
        }
        Ok(())
    }

    fn append_tool_log_impl(&self, user_id: &str, payload: &Value) -> Result<()> {
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
            .and_then(|value| serde_json::to_string(value).ok())
            .map(|text| crate::storage::constants::truncate_tool_log_column(&text));
        let data = payload
            .get("data")
            .and_then(|value| serde_json::to_string(value).ok())
            .map(|text| crate::storage::constants::truncate_tool_log_column(&text));
        let timestamp = Self::parse_string(payload.get("timestamp"));
        // The full payload column is retired: tool logs always persist the
        // bounded args/data columns plus an empty payload placeholder.
        let mut timeline_payload = payload.clone();
        if let Value::Object(map) = &mut timeline_payload {
            let has_thread_identity = map
                .get("turn_id")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
                && map
                    .get("user_round")
                    .and_then(Value::as_i64)
                    .is_some_and(|value| value > 0);
            if !has_thread_identity {
                // Diagnostics produced outside an orchestrated request do not
                // belong to an arbitrary latest user turn.
            } else {
                map.insert("kind".into(), Value::String("tool_call".into()));
                map.insert(
                    "status".into(),
                    Value::String(
                        if ok.is_some_and(|value| value != 0) {
                            "completed"
                        } else {
                            "failed"
                        }
                        .into(),
                    ),
                );
                self.append_thread_item_impl(user_id, &timeline_payload)?;
            }
        }
        let payload_text = "{}".to_string();
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

    fn append_artifact_log_impl(&self, user_id: &str, payload: &Value) -> Result<()> {
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

    fn load_chat_history_impl(
        &self,
        user_id: &str,
        session_id: &str,
        limit: Option<i64>,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let limit_value = limit.filter(|value| *value > 0);
        let conn = self.open()?;
        let mut records = Vec::new();
        let mut repairs = Vec::new();
        if let Some(limit_value) = limit_value {
            let mut stmt = conn.prepare(
                "SELECT id, payload FROM chat_history WHERE user_id = ? AND session_id = ? ORDER BY id DESC LIMIT ?",
            )?;
            let rows = stmt.query_map(params![user_id, session_id, limit_value], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (history_id, payload) = row?;
                let (value, repaired_payload) = parse_sanitized_persisted_chat_payload(&payload);
                if let Some(repaired_payload) = repaired_payload {
                    repairs.push((history_id, repaired_payload));
                }
                if let Some(value) = value {
                    records.push(value);
                }
            }
            records.reverse();
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, payload FROM chat_history WHERE user_id = ? AND session_id = ? ORDER BY id ASC",
            )?;
            let rows = stmt.query_map(params![user_id, session_id], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (history_id, payload) = row?;
                let (value, repaired_payload) = parse_sanitized_persisted_chat_payload(&payload);
                if let Some(repaired_payload) = repaired_payload {
                    repairs.push((history_id, repaired_payload));
                }
                if let Some(value) = value {
                    records.push(value);
                }
            }
        }
        repair_chat_history_payloads(&conn, repairs);
        Ok(records)
    }

    fn load_chat_history_page_impl(
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
        let mut records = Vec::new();
        let mut repairs = Vec::new();
        if let Some(before_id) = before_id {
            let mut stmt = conn.prepare(
                "SELECT id, payload FROM chat_history WHERE user_id = ? AND session_id = ? AND id < ? ORDER BY id DESC LIMIT ?",
            )?;
            let rows = stmt.query_map(params![user_id, session_id, before_id, limit], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (history_id, payload) = row?;
                let (mut value, repaired_payload) =
                    parse_sanitized_persisted_chat_payload(&payload);
                if let Some(repaired_payload) = repaired_payload {
                    repairs.push((history_id, repaired_payload));
                }
                if let Some(Value::Object(ref mut map)) = value {
                    map.insert("_history_id".to_string(), json!(history_id));
                }
                if let Some(value) = value {
                    records.push(value);
                }
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, payload FROM chat_history WHERE user_id = ? AND session_id = ? ORDER BY id DESC LIMIT ?",
            )?;
            let rows = stmt.query_map(params![user_id, session_id, limit], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (history_id, payload) = row?;
                let (mut value, repaired_payload) =
                    parse_sanitized_persisted_chat_payload(&payload);
                if let Some(repaired_payload) = repaired_payload {
                    repairs.push((history_id, repaired_payload));
                }
                if let Some(Value::Object(ref mut map)) = value {
                    map.insert("_history_id".to_string(), json!(history_id));
                }
                if let Some(value) = value {
                    records.push(value);
                }
            }
        }
        records.reverse();
        repair_chat_history_payloads(&conn, repairs);
        Ok(records)
    }

    fn load_artifact_logs_impl(
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

    fn get_session_system_prompt_impl(
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
}

impl SqliteStorage {
    fn current_thread_round(&self, user_id: &str, session_id: &str) -> Result<i64> {
        let conn = self.open()?;
        Ok(conn.query_row(
            "SELECT COALESCE(MAX(user_turn_index), 0) FROM thread_turns WHERE user_id=? AND session_id=?",
            params![user_id, session_id],
            |row| row.get(0),
        )?)
    }
}

fn repair_chat_history_payloads(conn: &rusqlite::Connection, repairs: Vec<(i64, String)>) {
    for (history_id, payload) in repairs {
        let _ = conn.execute(
            "UPDATE chat_history SET payload = ? WHERE id = ?",
            params![payload, history_id],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::constants::{
        TOOL_LOG_HEAD_CHARS, TOOL_LOG_TAIL_CHARS, TOOL_LOG_TRUNCATION_MARKER,
    };
    use tempfile::tempdir;
    use wunder_core::storage_backend::ConversationLogStore;

    fn read_tool_log_columns(
        db_path: &std::path::Path,
        session_id: &str,
    ) -> (String, String, String) {
        let conn = rusqlite::Connection::open(db_path).expect("open db");
        conn.query_row(
            "SELECT args, data, payload FROM tool_logs WHERE session_id = ?1",
            params![session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read tool log row")
    }

    #[test]
    fn tool_log_columns_are_bounded_and_payload_is_empty() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("tool-log-bounded.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize storage");
        let big_args = "a".repeat(TOOL_LOG_HEAD_CHARS + TOOL_LOG_TAIL_CHARS + 500);
        let big_data = "d".repeat(TOOL_LOG_HEAD_CHARS + TOOL_LOG_TAIL_CHARS + 500);
        let payload = json!({
            "tool": "read_file",
            "session_id": "sess-tool-log",
            "ok": true,
            "error": "",
            "args": { "content": big_args },
            "data": { "text": big_data },
            "timestamp": "2024-01-01T00:00:00Z",
        });
        storage
            .append_tool_log("user-1", &payload)
            .expect("append tool log");

        let (args, data, payload_text) = read_tool_log_columns(&db_path, "sess-tool-log");
        assert_eq!(payload_text, "{}");
        for column in [&args, &data] {
            assert!(column.contains(TOOL_LOG_TRUNCATION_MARKER));
            let budget = TOOL_LOG_HEAD_CHARS
                + TOOL_LOG_TAIL_CHARS
                + TOOL_LOG_TRUNCATION_MARKER.chars().count()
                + 256; // serialized JSON wrapper overhead
            assert!(column.chars().count() <= budget);
        }
    }

    #[test]
    fn short_tool_log_columns_are_not_truncated() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("tool-log-short.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize storage");
        let payload = json!({
            "tool": "read_file",
            "session_id": "sess-tool-log-short",
            "ok": true,
            "error": "",
            "args": { "path": "a.txt" },
            "data": { "text": "hello" },
            "timestamp": "2024-01-01T00:00:00Z",
        });
        storage
            .append_tool_log("user-1", &payload)
            .expect("append tool log");

        let (args, data, payload_text) = read_tool_log_columns(&db_path, "sess-tool-log-short");
        assert_eq!(payload_text, "{}");
        assert!(!args.contains(TOOL_LOG_TRUNCATION_MARKER));
        assert!(!data.contains(TOOL_LOG_TRUNCATION_MARKER));
        assert_eq!(args, json!({ "path": "a.txt" }).to_string());
        assert_eq!(data, json!({ "text": "hello" }).to_string());
    }
}
