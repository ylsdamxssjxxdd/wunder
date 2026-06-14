use super::PostgresStorage;
use crate::i18n;
use crate::services::output_quality;
use crate::storage::StorageBackend;
use anyhow::Result;
use serde_json::{json, Value};

pub(super) trait PostgresConversationLogStorage {
    fn append_chat_impl(&self, user_id: &str, payload: &Value) -> Result<()>;
    fn append_model_context_entry_impl(
        &self,
        user_id: &str,
        session_id: &str,
        payload: &Value,
    ) -> Result<()>;
    fn replace_model_context_entries_impl(
        &self,
        user_id: &str,
        session_id: &str,
        payloads: &[Value],
    ) -> Result<()>;
    fn append_tool_log_impl(&self, user_id: &str, payload: &Value) -> Result<()>;
    fn append_artifact_log_impl(&self, user_id: &str, payload: &Value) -> Result<()>;
    fn load_model_context_entries_impl(
        &self,
        user_id: &str,
        session_id: &str,
        limit: Option<i64>,
    ) -> Result<Vec<Value>>;
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

    fn append_model_context_entry_impl(
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

    fn replace_model_context_entries_impl(
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

    fn load_model_context_entries_impl(
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

    fn load_chat_history_impl(
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

    fn get_session_system_prompt_impl(
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
}
