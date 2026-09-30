use super::thread_log_store::SqliteThreadLogStorage;
use super::SqliteStorage;
use crate::services::{chat_payload_sanitizer::sanitize_persisted_chat_payload, output_quality};
use crate::storage::StorageLifecycle;
use anyhow::Result;
use rusqlite::params;
use serde_json::{json, Value};

pub(super) trait SqliteConversationLogStorage {
    fn append_chat_impl(&self, user_id: &str, payload: &Value) -> Result<()>;
    fn append_tool_log_impl(&self, user_id: &str, payload: &Value) -> Result<()>;
    fn append_artifact_log_impl(&self, user_id: &str, payload: &Value) -> Result<()>;
    fn load_artifact_logs_impl(
        &self,
        user_id: &str,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<Value>>;
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
        // ThreadLog is authoritative even for legacy callers that did not yet
        // attach a Turn identity. Admit a synthetic root once, then route all
        // following messages in this session to that durable Turn.
        let explicit_turn = payload
            .get("turn_id")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
            .map(str::to_owned);
        let mut admitted_user = false;
        let turn_id = if let Some(turn_id) = explicit_turn {
            Some(turn_id)
        } else if let Some(round) = payload
            .get("user_round")
            .and_then(Value::as_i64)
            .filter(|round| *round > 0)
        {
            self.find_thread_turn_id_impl(user_id, &session_id, round)?
        } else if role == "user" {
            let accepted = self.accept_thread_turn_impl(user_id, &session_id, &payload)?;
            admitted_user = accepted.get("created").and_then(Value::as_bool) == Some(true);
            accepted
                .get("turn_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
        } else {
            self.list_thread_turns_impl(user_id, &session_id, None, 1)?
                .into_iter()
                .next()
                .and_then(|turn| {
                    turn.get("turn_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
        };
        if let Some(turn_id) = turn_id {
            if !admitted_user {
                let mut timeline_payload = payload.clone();
                if let Value::Object(map) = &mut timeline_payload {
                    map.insert("turn_id".into(), Value::String(turn_id.clone()));
                    map.insert(
                        "item_id".into(),
                        Value::String(
                            payload
                                .get("item_id")
                                .and_then(Value::as_str)
                                .map(str::to_owned)
                                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                        ),
                    );
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
        }
        // ThreadLog is the only durable chat history.
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
