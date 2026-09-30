use super::thread_log_store::PostgresThreadLogStorage;
use super::PostgresStorage;
use crate::services::{chat_payload_sanitizer::sanitize_persisted_chat_payload, output_quality};
use crate::storage::StorageLifecycle;
use anyhow::Result;
use serde_json::{json, Value};

pub(super) trait PostgresConversationLogStorage {
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

impl PostgresConversationLogStorage for PostgresStorage {
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
                        if ok.unwrap_or(0) != 0 {
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
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO artifact_logs (user_id, session_id, kind, name, payload, created_time) \
             VALUES ($1, $2, $3, $4, $5, $6)",
            &[&user_id, &session_id, &kind, &name, &payload_text, &now],
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
}
