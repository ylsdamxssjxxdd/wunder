use crate::i18n;
use crate::orchestrator_constants::OBSERVATION_PREFIX;
use crate::user_store::UserStore;
use crate::workspace::WorkspaceManager;
use anyhow::Result;
use chrono::{DateTime, Local, Utc};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

const CHAT_SESSION_STATUS_ACTIVE: &str = "active";
const CHAT_SESSION_STATUS_ARCHIVED: &str = "archived";
const CHAT_CANCEL_MARKER_META_TYPE: &str = "session_cancelled";
const CHAT_CANCEL_MARKER_STOP_REASON: &str = "user_stop";

pub async fn persist_user_cancelled_turn_marker(
    workspace: std::sync::Arc<WorkspaceManager>,
    user_store: std::sync::Arc<UserStore>,
    user_id: &str,
    session_id: &str,
    cancel_source: &str,
) -> Result<bool> {
    let cleaned_user_id = user_id.trim();
    let cleaned_session_id = session_id.trim();
    if cleaned_user_id.is_empty() || cleaned_session_id.is_empty() {
        return Ok(false);
    }
    let user_id = cleaned_user_id.to_string();
    let session_id = cleaned_session_id.to_string();
    let cancel_source = cancel_source.trim().to_string();
    tokio::task::spawn_blocking(move || {
        persist_user_cancelled_turn_marker_sync(
            workspace.as_ref(),
            user_store.as_ref(),
            &user_id,
            &session_id,
            &cancel_source,
        )
    })
    .await
    .map_err(|err| anyhow::anyhow!("persist cancelled turn marker failed: {err}"))?
}

pub(crate) fn persist_user_cancelled_turn_marker_sync(
    workspace: &WorkspaceManager,
    user_store: &UserStore,
    user_id: &str,
    session_id: &str,
    cancel_source: &str,
) -> Result<bool> {
    let _ = workspace.flush_writes();
    let history = workspace.load_history(user_id, session_id, 0)?;
    let Some(last_user_index) = history
        .iter()
        .rposition(|item| item.get("role").and_then(Value::as_str) == Some("user"))
    else {
        return Ok(false);
    };
    if history
        .iter()
        .skip(last_user_index + 1)
        .any(is_visible_assistant_turn_response)
    {
        return Ok(false);
    }
    if history
        .iter()
        .skip(last_user_index + 1)
        .any(is_visible_cancelled_turn_marker)
    {
        return Ok(false);
    }
    let now = now_ts();
    let marker = build_user_cancelled_turn_marker_payload(session_id, cancel_source, now);
    workspace.append_chat(user_id, &marker)?;
    let _ = workspace.flush_writes();
    touch_session_after_cancel_marker(user_store, user_id, session_id, now);
    Ok(true)
}

fn touch_session_after_cancel_marker(
    user_store: &UserStore,
    user_id: &str,
    session_id: &str,
    now: f64,
) {
    let Ok(Some(mut record)) = user_store.get_chat_session(user_id, session_id) else {
        return;
    };
    record.updated_at = record.updated_at.max(now);
    record.last_message_at = record.last_message_at.max(now);
    if !record
        .status
        .trim()
        .eq_ignore_ascii_case(CHAT_SESSION_STATUS_ARCHIVED)
    {
        record.status = CHAT_SESSION_STATUS_ACTIVE.to_string();
    }
    let _ = user_store.upsert_chat_session(&record);
}

pub(crate) fn is_visible_assistant_turn_response(item: &Value) -> bool {
    if item.get("role").and_then(Value::as_str) != Some("assistant") {
        return false;
    }
    if is_visible_cancelled_turn_marker(item) || is_tool_call_meta(item) {
        return false;
    }
    let raw_content = item.get("content").cloned().unwrap_or(Value::Null);
    let content = normalize_message_content(&raw_content);
    let content_trimmed = content.trim();
    let reasoning = item
        .get("reasoning_content")
        .or_else(|| item.get("reasoning"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if is_tool_payload_value(&raw_content)
        || is_tool_payload_text(content_trimmed)
        || (content_trimmed.is_empty() && is_tool_payload_text(reasoning))
    {
        return false;
    }
    !content_trimmed.is_empty() || !reasoning.is_empty()
}

pub(crate) fn build_user_cancelled_turn_marker_payload(
    session_id: &str,
    cancel_source: &str,
    now: f64,
) -> Value {
    let mut meta = json!({
        "type": CHAT_CANCEL_MARKER_META_TYPE,
        "cancelled": true,
        "user_visible": true,
        "stop_reason": CHAT_CANCEL_MARKER_STOP_REASON,
    });
    if let Value::Object(ref mut map) = meta {
        let cleaned_source = cancel_source.trim();
        if !cleaned_source.is_empty() {
            map.insert("cancel_source".to_string(), json!(cleaned_source));
        }
    }
    json!({
        "role": "assistant",
        "content": i18n::t("error.session_cancelled"),
        "session_id": session_id,
        "timestamp": format_ts(now),
        "stop_reason": CHAT_CANCEL_MARKER_STOP_REASON,
        "meta": meta,
    })
}

pub(crate) fn is_visible_cancelled_turn_marker(item: &Value) -> bool {
    if item.get("role").and_then(Value::as_str) != Some("assistant") {
        return false;
    }
    let meta_is_cancel_marker = item
        .get("meta")
        .and_then(Value::as_object)
        .map(|meta| {
            meta.get("type")
                .and_then(Value::as_str)
                .map(|value| value == CHAT_CANCEL_MARKER_META_TYPE)
                .unwrap_or(false)
                || meta
                    .get("cancelled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
        })
        .unwrap_or(false);
    if meta_is_cancel_marker {
        return true;
    }
    let stop_reason = item
        .get("stop_reason")
        .or_else(|| item.get("stopReason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    matches!(
        stop_reason,
        "user_stop" | "cancelled" | "canceled" | "aborted"
    )
}

pub(crate) fn is_tool_call_meta(item: &Value) -> bool {
    item.get("meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("type"))
        .and_then(Value::as_str)
        .map(|value| value == "tool_call")
        .unwrap_or(false)
}

pub(crate) fn is_tool_payload_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.starts_with(OBSERVATION_PREFIX) {
        return true;
    }
    if trimmed.contains("<tool_call")
        || trimmed.contains("</tool_call")
        || trimmed.contains("<tool>")
        || trimmed.contains("</tool>")
    {
        return true;
    }
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            return is_tool_payload(&value) && !has_visible_answer_fields(&value);
        }
    }
    trimmed.contains("\"tool_calls\"")
        || trimmed.contains("\"tool_call\"")
        || trimmed.contains("\"function_call\"")
        || trimmed.contains("\"tool_result\"")
}

fn has_visible_answer_fields(value: &Value) -> bool {
    let Some(map) = value.as_object() else {
        return false;
    };
    if has_non_empty_field(map.get("answer"))
        || has_non_empty_field(map.get("content"))
        || has_non_empty_field(map.get("message"))
    {
        return true;
    }
    let Some(data) = map.get("data").and_then(Value::as_object) else {
        return false;
    };
    has_non_empty_field(data.get("answer"))
        || has_non_empty_field(data.get("content"))
        || has_non_empty_field(data.get("message"))
}

fn has_non_empty_field(value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(text)) => !text.trim().is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
        Some(Value::Null) | None => false,
        Some(_) => true,
    }
}

pub(crate) fn is_tool_payload_value(value: &Value) -> bool {
    match value {
        Value::String(text) => is_tool_payload_text(text),
        Value::Array(items) => items.iter().any(is_tool_payload_value),
        Value::Object(_) => is_tool_payload(value) && !has_visible_answer_fields(value),
        _ => false,
    }
}

fn is_tool_payload(value: &Value) -> bool {
    is_tool_call_payload(value) || is_tool_result_payload(value)
}

fn is_tool_call_payload(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            if map.contains_key("tool_calls")
                || map.contains_key("tool_call")
                || map.contains_key("function_call")
            {
                return true;
            }
            if let Some(Value::String(kind)) = map.get("type") {
                let lowered = kind.to_lowercase();
                if lowered.contains("tool") || lowered.contains("function") {
                    return true;
                }
            }
            let has_tool = map.contains_key("tool") || map.contains_key("tool_name");
            let has_name = map.contains_key("name");
            let has_args = map.contains_key("arguments")
                || map.contains_key("args")
                || map.contains_key("parameters");
            if (has_tool || has_name) && has_args {
                return true;
            }
            let Some(Value::Object(function)) = map.get("function") else {
                return false;
            };
            let function_has_name = function.contains_key("name") || function.contains_key("tool");
            let function_has_args =
                function.contains_key("arguments") || function.contains_key("args");
            function_has_name && function_has_args
        }
        Value::Array(items) => items.iter().any(is_tool_call_payload),
        _ => false,
    }
}

fn is_tool_result_payload(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            let tool = map.get("tool").and_then(Value::as_str).unwrap_or("").trim();
            if tool.is_empty() {
                return false;
            }
            let has_ok = map.get("ok").and_then(Value::as_bool).is_some();
            let has_data = map.contains_key("data") || map.contains_key("result");
            let has_error = map.contains_key("error");
            let has_timestamp = map
                .get("timestamp")
                .and_then(Value::as_str)
                .map(|text| !text.trim().is_empty())
                .unwrap_or(false);
            (has_ok && (has_data || has_error)) || (has_timestamp && has_data)
        }
        Value::Array(items) => items.iter().any(is_tool_result_payload),
        _ => false,
    }
}

pub(crate) fn normalize_message_content(value: &Value) -> String {
    match value {
        Value::String(text) => text.to_string(),
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    if !text.trim().is_empty() {
                        parts.push(text.trim().to_string());
                    }
                }
            }
            if parts.is_empty() {
                String::new()
            } else {
                parts.join("\n")
            }
        }
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn format_ts(ts: f64) -> String {
    let millis = (ts * 1000.0) as i64;
    DateTime::<Utc>::from_timestamp_millis(millis)
        .map(|dt| dt.with_timezone(&Local).to_rfc3339())
        .unwrap_or_default()
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::{
        build_user_cancelled_turn_marker_payload, is_visible_assistant_turn_response,
        is_visible_cancelled_turn_marker, persist_user_cancelled_turn_marker_sync,
    };
    use crate::storage::{ChatSessionRecord, SqliteStorage, StorageBackend};
    use crate::user_store::UserStore;
    use crate::workspace::WorkspaceManager;
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::sync::Arc;

    fn build_chat_storage() -> Arc<dyn StorageBackend> {
        let db_path = std::env::temp_dir().join(format!(
            "wunder_chat_cancel_marker_{}.db",
            uuid::Uuid::new_v4().simple()
        ));
        Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()))
    }

    #[test]
    fn cancelled_turn_marker_persists_after_trailing_user_message() {
        let storage = build_chat_storage();
        storage.ensure_initialized().expect("init storage");
        let workspace = WorkspaceManager::new(
            std::env::temp_dir().to_string_lossy().as_ref(),
            storage.clone(),
            0,
            &HashMap::new(),
        );
        let user_store = UserStore::new(storage.clone());
        let now = 1_900_000_000.0;
        user_store
            .upsert_chat_session(&ChatSessionRecord {
                session_id: "sess_cancel_marker".to_string(),
                user_id: "user_cancel".to_string(),
                title: "session".to_string(),
                status: "active".to_string(),
                created_at: now,
                updated_at: now,
                last_message_at: now,
                agent_id: None,
                tool_overrides: Vec::new(),
                parent_session_id: None,
                parent_message_id: None,
                spawn_label: None,
                spawned_by: None,
            })
            .expect("upsert session");
        storage
            .append_chat(
                "user_cancel",
                &json!({
                    "role": "user",
                    "content": "hello",
                    "session_id": "sess_cancel_marker",
                    "timestamp": "2026-05-21T10:00:00+08:00"
                }),
            )
            .expect("append user");

        assert!(persist_user_cancelled_turn_marker_sync(
            &workspace,
            &user_store,
            "user_cancel",
            "sess_cancel_marker",
            "rest_cancel"
        )
        .expect("persist marker"));
        let history = storage
            .load_chat_history("user_cancel", "sess_cancel_marker", None)
            .expect("load history");

        assert_eq!(history.len(), 2);
        assert!(is_visible_cancelled_turn_marker(&history[1]));
        assert_eq!(history[1]["role"], json!("assistant"));
        assert_eq!(history[1]["stop_reason"], json!("user_stop"));
        assert!(history[1]
            .get("content")
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|value| !value.is_empty()));
    }

    #[test]
    fn cancelled_turn_marker_is_idempotent_for_same_user_turn() {
        let storage = build_chat_storage();
        storage.ensure_initialized().expect("init storage");
        let workspace = WorkspaceManager::new(
            std::env::temp_dir().to_string_lossy().as_ref(),
            storage.clone(),
            0,
            &HashMap::new(),
        );
        let user_store = UserStore::new(storage.clone());
        storage
            .append_chat(
                "user_cancel",
                &json!({
                    "role": "user",
                    "content": "hello",
                    "session_id": "sess_cancel_idempotent",
                    "timestamp": "2026-05-21T10:00:00+08:00"
                }),
            )
            .expect("append user");

        assert!(persist_user_cancelled_turn_marker_sync(
            &workspace,
            &user_store,
            "user_cancel",
            "sess_cancel_idempotent",
            "rest_cancel"
        )
        .expect("first marker"));
        assert!(!persist_user_cancelled_turn_marker_sync(
            &workspace,
            &user_store,
            "user_cancel",
            "sess_cancel_idempotent",
            "rest_cancel"
        )
        .expect("second marker"));
        let history = storage
            .load_chat_history("user_cancel", "sess_cancel_idempotent", None)
            .expect("load history");
        let marker_count = history
            .iter()
            .filter(|item| is_visible_cancelled_turn_marker(item))
            .count();

        assert_eq!(marker_count, 1);
    }

    #[test]
    fn cancelled_turn_marker_persists_after_pending_user_write_flush() {
        let storage = build_chat_storage();
        storage.ensure_initialized().expect("init storage");
        let workspace = WorkspaceManager::new(
            std::env::temp_dir().to_string_lossy().as_ref(),
            storage.clone(),
            0,
            &HashMap::new(),
        );
        let user_store = UserStore::new(storage.clone());
        workspace
            .append_chat(
                "user_cancel",
                &json!({
                    "role": "user",
                    "content": "pending hello",
                    "session_id": "sess_cancel_pending_flush",
                    "timestamp": "2026-05-21T10:00:00+08:00"
                }),
            )
            .expect("enqueue user");

        assert!(persist_user_cancelled_turn_marker_sync(
            &workspace,
            &user_store,
            "user_cancel",
            "sess_cancel_pending_flush",
            "orchestrator_cancel"
        )
        .expect("persist marker"));
        let history = storage
            .load_chat_history("user_cancel", "sess_cancel_pending_flush", None)
            .expect("load history");

        assert_eq!(history.len(), 2);
        assert_eq!(history[0]["role"], json!("user"));
        assert!(is_visible_cancelled_turn_marker(&history[1]));
    }

    #[test]
    fn cancelled_turn_marker_does_not_append_after_visible_assistant_response() {
        let storage = build_chat_storage();
        storage.ensure_initialized().expect("init storage");
        let workspace = WorkspaceManager::new(
            std::env::temp_dir().to_string_lossy().as_ref(),
            storage.clone(),
            0,
            &HashMap::new(),
        );
        let user_store = UserStore::new(storage.clone());
        storage
            .append_chat(
                "user_cancel",
                &json!({
                    "role": "user",
                    "content": "hello",
                    "session_id": "sess_cancel_answered",
                    "timestamp": "2026-05-21T10:00:00+08:00"
                }),
            )
            .expect("append user");
        storage
            .append_chat(
                "user_cancel",
                &json!({
                    "role": "assistant",
                    "content": "answer",
                    "session_id": "sess_cancel_answered",
                    "timestamp": "2026-05-21T10:00:01+08:00"
                }),
            )
            .expect("append assistant");

        assert!(!persist_user_cancelled_turn_marker_sync(
            &workspace,
            &user_store,
            "user_cancel",
            "sess_cancel_answered",
            "rest_cancel"
        )
        .expect("marker skipped"));
        let history = storage
            .load_chat_history("user_cancel", "sess_cancel_answered", None)
            .expect("load history");

        assert_eq!(history.len(), 2);
        assert!(is_visible_assistant_turn_response(&history[1]));
    }

    #[test]
    fn cancelled_turn_marker_builder_sets_visible_stop_reason() {
        let marker =
            build_user_cancelled_turn_marker_payload("sess_cancel_payload", "ws_cancel", 1.0);

        assert!(is_visible_cancelled_turn_marker(&marker));
        assert_eq!(marker["role"], json!("assistant"));
        assert_eq!(marker["stop_reason"], json!("user_stop"));
        assert_eq!(marker["meta"]["type"], json!("session_cancelled"));
        assert_eq!(marker["meta"]["cancel_source"], json!("ws_cancel"));
    }
}
