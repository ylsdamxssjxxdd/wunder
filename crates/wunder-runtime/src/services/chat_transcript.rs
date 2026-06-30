use crate::services::chat_cancel_marker::{
    is_tool_call_meta, is_tool_payload_text, is_tool_payload_value, normalize_message_content,
};
use chrono::{DateTime, Local};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

const CANCEL_STOP_REASONS: &[&str] = &["user_stop", "cancelled", "canceled", "aborted"];

#[derive(Clone, Debug)]
struct TranscriptCursor {
    user_turn_index: i64,
    model_turn_index: i64,
    message_index: i64,
    current_user_turn_index: i64,
    max_user_turn_index: i64,
    persisted_user_rounds: HashSet<i64>,
    model_turn_indexes: HashMap<i64, i64>,
}

impl Default for TranscriptCursor {
    fn default() -> Self {
        Self {
            user_turn_index: 0,
            model_turn_index: 0,
            message_index: 0,
            current_user_turn_index: 0,
            max_user_turn_index: 0,
            persisted_user_rounds: HashSet::new(),
            model_turn_indexes: HashMap::new(),
        }
    }
}

pub fn build_chat_transcript(
    session_id: &str,
    history: Vec<Value>,
    message_feedback: &HashMap<i64, Value>,
) -> Vec<Value> {
    let mut cursor = TranscriptCursor::default();
    let page_user_rounds = collect_explicit_user_rounds(&history);
    let mut transcript = Vec::new();
    for item in history {
        if let Some(message) = map_transcript_message(
            session_id,
            item,
            message_feedback,
            &page_user_rounds,
            &mut cursor,
        ) {
            transcript.push(message);
        }
    }
    if transcript.iter().any(has_trusted_transcript_round) {
        sort_transcript_messages(&mut transcript);
    }
    renumber_transcript_messages(&mut transcript);
    transcript
}

fn map_transcript_message(
    session_id: &str,
    item: Value,
    message_feedback: &HashMap<i64, Value>,
    page_user_rounds: &HashSet<i64>,
    cursor: &mut TranscriptCursor,
) -> Option<Value> {
    let role = item.get("role").and_then(Value::as_str)?;
    if role == "system" || role == "tool" {
        return None;
    }
    let raw_content = item.get("content").cloned().unwrap_or(Value::Null);
    let content = normalize_message_content(&raw_content);
    let reasoning = if role == "assistant" {
        item.get("reasoning_content")
            .or_else(|| item.get("reasoning"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
    } else {
        ""
    };
    if role == "assistant"
        && should_hide_assistant_history_item(&item, &raw_content, &content, reasoning)
    {
        return None;
    }

    let history_id = item.get("_history_id").and_then(Value::as_i64);
    let created_at = item
        .get("timestamp")
        .and_then(Value::as_str)
        .map(format_ts_text)
        .unwrap_or_default();
    let raw_user_round = resolve_history_user_round(&item);
    let raw_model_round =
        positive_i64(item.get("model_round")).or_else(|| positive_i64(item.get("modelRound")));
    let hidden_internal = is_hidden_internal_history_message(&item);
    let trusted_user_round = is_trusted_history_user_round(
        role,
        &item,
        raw_user_round,
        hidden_internal,
        page_user_rounds,
        cursor,
    );
    let (user_turn_index, model_turn_index) = resolve_transcript_turn_indexes(
        role,
        raw_user_round,
        trusted_user_round,
        raw_model_round,
        hidden_internal,
        cursor,
    );
    cursor.message_index = cursor.message_index.saturating_add(1);
    let turn_index = cursor.message_index;
    let user_turn_id = format!("user-turn:{session_id}:round:{user_turn_index}");
    let model_turn_id = model_turn_index
        .map(|index| format!("model-turn:{session_id}:user:{user_turn_index}:model:{index}"));
    let message_id = resolve_message_id(session_id, role, history_id, turn_index);
    let status = resolve_message_status(role, &item);
    let mut message = json!({
        "role": role,
        "content": content,
        "created_at": created_at,
        "message_id": message_id,
        "user_turn_id": user_turn_id,
        "turn_index": turn_index,
        "status": status,
    });

    if let Value::Object(ref mut map) = message {
        if let Some(history_id) = history_id {
            map.insert("history_id".to_string(), json!(history_id));
        }
        if let Some(model_turn_id) = model_turn_id {
            map.insert("model_turn_id".to_string(), json!(model_turn_id));
        }
        if let Some(model_turn_index) = model_turn_index {
            map.insert("model_turn_index".to_string(), json!(model_turn_index));
        }
        map.insert("user_turn_index".to_string(), json!(user_turn_index));
        if trusted_user_round {
            if let Some(raw_user_round) = raw_user_round {
                map.insert("user_round".to_string(), json!(raw_user_round));
            }
        }
        if let Some(raw_model_round) = raw_model_round {
            map.insert("model_round".to_string(), json!(raw_model_round));
        }
        if let Some(stop_reason) = resolve_stop_reason(&item) {
            map.insert("stop_reason".to_string(), json!(stop_reason));
        }
        if status == "cancelled" {
            map.insert("cancelled".to_string(), Value::Bool(true));
        }
        if status == "failed" {
            map.insert("failed".to_string(), Value::Bool(true));
        }
        if role == "assistant" && !reasoning.is_empty() {
            map.insert("reasoning".to_string(), json!(reasoning));
        }
        if hidden_internal {
            map.insert("hiddenInternal".to_string(), Value::Bool(true));
        }
        if let Some(panel) = extract_question_panel(&item) {
            map.insert("questionPanel".to_string(), panel);
        }
        if let Some(attachments) = normalized_attachments(&item) {
            map.insert("attachments".to_string(), attachments);
        }
        if role == "assistant" {
            if let Some(history_id) = history_id {
                if let Some(feedback) = message_feedback.get(&history_id) {
                    map.insert("feedback".to_string(), feedback.clone());
                }
            }
        }
    }
    Some(message)
}

fn resolve_message_id(
    session_id: &str,
    role: &str,
    history_id: Option<i64>,
    turn_index: i64,
) -> String {
    if let Some(history_id) = history_id {
        return format!("history:{history_id}");
    }
    format!("message:{session_id}:turn:{turn_index}:{role}")
}

fn resolve_message_status(role: &str, item: &Value) -> &'static str {
    if role != "assistant" {
        return "final";
    }
    let status = item
        .get("status")
        .or_else(|| item.get("thread_status"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_ascii_lowercase();
    if status == "failed" || status == "error" {
        return "failed";
    }
    if status == "cancelled" || status == "canceled" || is_cancelled_history_message(item) {
        return "cancelled";
    }
    "final"
}

fn sort_transcript_messages(messages: &mut [Value]) {
    messages.sort_by(|left, right| {
        let left_role = left.get("role").and_then(Value::as_str).unwrap_or("");
        let right_role = right.get("role").and_then(Value::as_str).unwrap_or("");
        non_negative_i64(left.get("user_turn_index"))
            .unwrap_or(i64::MAX)
            .cmp(&non_negative_i64(right.get("user_turn_index")).unwrap_or(i64::MAX))
            .then_with(|| role_sort_rank(left_role).cmp(&role_sort_rank(right_role)))
            .then_with(|| {
                non_negative_i64(left.get("model_turn_index"))
                    .unwrap_or(i64::MAX)
                    .cmp(&non_negative_i64(right.get("model_turn_index")).unwrap_or(i64::MAX))
            })
            .then_with(|| {
                positive_i64(left.get("history_id"))
                    .unwrap_or(i64::MAX)
                    .cmp(&positive_i64(right.get("history_id")).unwrap_or(i64::MAX))
            })
            .then_with(|| {
                positive_i64(left.get("turn_index"))
                    .unwrap_or(i64::MAX)
                    .cmp(&positive_i64(right.get("turn_index")).unwrap_or(i64::MAX))
            })
    });
}

fn has_trusted_transcript_round(message: &Value) -> bool {
    message
        .get("user_round")
        .and_then(Value::as_i64)
        .is_some_and(|value| value > 0)
}

fn role_sort_rank(role: &str) -> i32 {
    match role {
        "user" => 0,
        "assistant" => 1,
        _ => 2,
    }
}

fn renumber_transcript_messages(messages: &mut [Value]) {
    for (index, message) in messages.iter_mut().enumerate() {
        if let Value::Object(map) = message {
            map.insert("turn_index".to_string(), json!((index + 1) as i64));
        }
    }
}

fn resolve_history_user_round(item: &Value) -> Option<i64> {
    positive_i64(item.get("user_round"))
        .or_else(|| positive_i64(item.get("userRound")))
        .or_else(|| positive_i64(item.get("round")))
}

fn collect_explicit_user_rounds(history: &[Value]) -> HashSet<i64> {
    history
        .iter()
        .filter(|item| item.get("role").and_then(Value::as_str) == Some("user"))
        .filter(|item| !is_hidden_internal_history_message(item))
        .filter_map(explicit_history_user_round)
        .collect()
}

fn explicit_history_user_round(item: &Value) -> Option<i64> {
    positive_i64(item.get("user_round")).or_else(|| positive_i64(item.get("userRound")))
}

fn has_orchestrator_round_source(item: &Value) -> bool {
    item.get("round_info_source")
        .or_else(|| item.get("roundInfoSource"))
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
}

fn is_trusted_history_user_round(
    role: &str,
    item: &Value,
    raw_user_round: Option<i64>,
    hidden_internal: bool,
    page_user_rounds: &HashSet<i64>,
    cursor: &TranscriptCursor,
) -> bool {
    let Some(raw_user_round) = raw_user_round else {
        return false;
    };
    if role == "user" && !hidden_internal {
        return true;
    }
    has_orchestrator_round_source(item)
        || page_user_rounds.contains(&raw_user_round)
        || cursor.persisted_user_rounds.contains(&raw_user_round)
}

fn resolve_transcript_turn_indexes(
    role: &str,
    raw_user_round: Option<i64>,
    trusted_user_round: bool,
    raw_model_round: Option<i64>,
    hidden_internal: bool,
    cursor: &mut TranscriptCursor,
) -> (i64, Option<i64>) {
    if role == "user" {
        if hidden_internal {
            let user_turn_index = raw_user_round
                .filter(|_| trusted_user_round)
                .or_else(|| {
                    (cursor.current_user_turn_index > 0).then_some(cursor.current_user_turn_index)
                })
                .unwrap_or(cursor.max_user_turn_index);
            return (user_turn_index, None);
        }
        if let Some(raw_user_round) = raw_user_round.filter(|_| trusted_user_round) {
            cursor.persisted_user_rounds.insert(raw_user_round);
        }
        let user_turn_index = raw_user_round
            .filter(|_| trusted_user_round)
            .unwrap_or_else(|| {
                cursor
                    .max_user_turn_index
                    .max(cursor.user_turn_index)
                    .saturating_add(1)
            });
        cursor.user_turn_index = user_turn_index;
        cursor.current_user_turn_index = user_turn_index;
        cursor.max_user_turn_index = cursor.max_user_turn_index.max(user_turn_index);
        cursor.model_turn_index = 0;
        cursor
            .model_turn_indexes
            .entry(user_turn_index)
            .or_insert(0);
        return (user_turn_index, None);
    }

    let user_turn_index = raw_user_round
        .filter(|_| trusted_user_round)
        .or_else(|| (cursor.current_user_turn_index > 0).then_some(cursor.current_user_turn_index))
        .unwrap_or(0);
    if user_turn_index >= cursor.current_user_turn_index {
        cursor.current_user_turn_index = user_turn_index;
    }
    cursor.max_user_turn_index = cursor.max_user_turn_index.max(user_turn_index);
    let model_turn_index = raw_model_round.unwrap_or_else(|| {
        let entry = cursor
            .model_turn_indexes
            .entry(user_turn_index)
            .or_insert(0);
        *entry = (*entry).saturating_add(1);
        *entry
    });
    let entry = cursor
        .model_turn_indexes
        .entry(user_turn_index)
        .or_insert(0);
    *entry = (*entry).max(model_turn_index);
    cursor.model_turn_index = cursor.model_turn_index.max(model_turn_index);
    (user_turn_index, Some(model_turn_index))
}

fn resolve_stop_reason(item: &Value) -> Option<String> {
    item.get("stop_reason")
        .or_else(|| item.get("stopReason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn is_cancelled_history_message(item: &Value) -> bool {
    if item
        .get("cancelled")
        .or_else(|| item.get("canceled"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return true;
    }
    if let Some(stop_reason) = resolve_stop_reason(item) {
        if CANCEL_STOP_REASONS.contains(&stop_reason.as_str()) {
            return true;
        }
    }
    item.get("meta")
        .and_then(Value::as_object)
        .map(|meta| {
            meta.get("type")
                .and_then(Value::as_str)
                .map(|value| value == "session_cancelled")
                .unwrap_or(false)
                || meta
                    .get("cancelled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
        })
        .unwrap_or(false)
}

fn should_hide_assistant_history_item(
    item: &Value,
    raw_content: &Value,
    content: &str,
    reasoning: &str,
) -> bool {
    let keep_tool_message = item
        .get("_keep_tool_message")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if keep_tool_message || is_cancelled_history_message(item) {
        return false;
    }
    let content_trimmed = content.trim();
    is_tool_call_meta(item)
        || is_tool_payload_value(raw_content)
        || is_tool_payload_text(content_trimmed)
        || (content_trimmed.is_empty() && is_tool_payload_text(reasoning))
}

fn normalized_attachments(item: &Value) -> Option<Value> {
    let attachments = item.get("attachments")?;
    match attachments {
        Value::Array(items) if items.is_empty() => None,
        Value::Null => None,
        _ => Some(attachments.clone()),
    }
}

fn extract_question_panel(item: &Value) -> Option<Value> {
    let meta = item.get("meta").and_then(Value::as_object)?;
    let meta_type = meta.get("type").and_then(Value::as_str).unwrap_or("");
    if meta_type == "question_panel" {
        if let Some(panel) = meta.get("panel") {
            return Some(panel.clone());
        }
    }
    meta.get("question_panel")
        .or_else(|| meta.get("questionPanel"))
        .cloned()
}

fn is_hidden_internal_history_message(item: &Value) -> bool {
    item.get("meta")
        .and_then(Value::as_object)
        .map(|meta| {
            meta.get("type")
                .and_then(Value::as_str)
                .map(|value| value == crate::services::subagents::HIDDEN_HISTORY_META_TYPE)
                .unwrap_or(false)
                || meta.get("hidden").and_then(Value::as_bool).unwrap_or(false)
        })
        .unwrap_or(false)
}

fn positive_i64(value: Option<&Value>) -> Option<i64> {
    let parsed = value.and_then(|value| {
        value.as_i64().or_else(|| {
            value
                .as_str()
                .and_then(|text| text.trim().parse::<i64>().ok())
        })
    })?;
    (parsed > 0).then_some(parsed)
}

fn non_negative_i64(value: Option<&Value>) -> Option<i64> {
    let parsed = value.and_then(|value| {
        value.as_i64().or_else(|| {
            value
                .as_str()
                .and_then(|text| text.trim().parse::<i64>().ok())
        })
    })?;
    (parsed >= 0).then_some(parsed)
}

fn format_ts_text(value: &str) -> String {
    let text = value.trim();
    if text.is_empty() {
        return String::new();
    }
    if let Ok(parsed) = DateTime::parse_from_rfc3339(text) {
        return parsed.with_timezone(&Local).to_rfc3339();
    }
    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_does_not_fold_same_stream_round_assistants() {
        let history = vec![
            json!({"role": "assistant", "content": "greeting", "timestamp": "2026-04-30T02:14:01Z", "_history_id": 1}),
            json!({"role": "user", "content": "first", "timestamp": "2026-04-30T02:14:06Z", "_history_id": 2}),
            json!({"role": "assistant", "content": "first answer", "timestamp": "2026-04-30T02:14:07Z", "user_round": 1, "model_round": 1, "_history_id": 3}),
            json!({"role": "user", "content": "second", "timestamp": "2026-04-30T02:14:16Z", "_history_id": 4}),
            json!({"role": "assistant", "content": "second answer", "timestamp": "2026-04-30T02:14:18Z", "user_round": 1, "model_round": 1, "_history_id": 5}),
        ];
        let transcript = build_chat_transcript("sess", history, &HashMap::new());
        let ids = transcript
            .iter()
            .map(|item| {
                item.get("model_turn_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string()
            })
            .collect::<Vec<_>>();

        assert_eq!(transcript.len(), 5);
        assert_ne!(ids[2], ids[4]);
        assert_eq!(transcript[4]["user_turn_index"], json!(2));
    }

    #[test]
    fn transcript_preserves_cancelled_marker_after_user_turn() {
        let history = vec![
            json!({"role": "user", "content": "stop me", "timestamp": "2026-04-30T02:14:06Z", "_history_id": 10}),
            json!({"role": "assistant", "content": "cancelled", "timestamp": "2026-04-30T02:14:07Z", "stop_reason": "user_stop", "_history_id": 11}),
        ];
        let transcript = build_chat_transcript("sess", history, &HashMap::new());

        assert_eq!(transcript.len(), 2);
        assert_eq!(transcript[1]["status"], json!("cancelled"));
        assert_eq!(transcript[1]["cancelled"], json!(true));
        assert_eq!(transcript[1]["user_turn_index"], json!(1));
    }

    #[test]
    fn transcript_binds_delayed_assistant_to_persisted_user_round() {
        let history = vec![
            json!({"role": "user", "content": "first", "timestamp": "2026-04-30T02:14:06Z", "user_round": 1, "_history_id": 20}),
            json!({"role": "user", "content": "second", "timestamp": "2026-04-30T02:14:16Z", "user_round": 2, "_history_id": 21}),
            json!({"role": "assistant", "content": "first answer", "timestamp": "2026-04-30T02:14:30Z", "user_round": 1, "model_round": 1, "round_info_source": "orchestrator", "_history_id": 22}),
        ];

        let transcript = build_chat_transcript("sess", history, &HashMap::new());
        let delayed = transcript
            .iter()
            .find(|item| item.get("history_id") == Some(&json!(22)))
            .expect("delayed assistant exists");

        assert_eq!(transcript.len(), 3);
        assert_eq!(transcript[0]["content"], json!("first"));
        assert_eq!(transcript[1]["content"], json!("first answer"));
        assert_eq!(transcript[2]["content"], json!("second"));
        assert_eq!(delayed["user_turn_id"], json!("user-turn:sess:round:1"));
        assert_eq!(
            delayed["model_turn_id"],
            json!("model-turn:sess:user:1:model:1")
        );
        assert_eq!(delayed["user_turn_index"], json!(1));
        assert_eq!(delayed["model_turn_index"], json!(1));
    }

    #[test]
    fn transcript_preserves_legacy_history_order_without_trusted_rounds() {
        let history = vec![
            json!({"role": "user", "content": "first", "timestamp": "2026-04-30T02:14:06Z", "_history_id": 40}),
            json!({"role": "assistant", "content": "first answer", "timestamp": "2026-04-30T02:14:07Z", "_history_id": 41}),
            json!({"role": "user", "content": "second", "timestamp": "2026-04-30T02:14:16Z", "_history_id": 42}),
            json!({"role": "assistant", "content": "second answer", "timestamp": "2026-04-30T02:14:18Z", "_history_id": 43}),
        ];

        let transcript = build_chat_transcript("sess", history, &HashMap::new());

        assert_eq!(transcript.len(), 4);
        assert_eq!(transcript[0]["content"], json!("first"));
        assert_eq!(transcript[1]["content"], json!("first answer"));
        assert_eq!(transcript[2]["content"], json!("second"));
        assert_eq!(transcript[3]["content"], json!("second answer"));
    }

    #[test]
    fn hidden_internal_user_does_not_advance_visible_turn_binding() {
        let history = vec![
            json!({"role": "user", "content": "visible", "timestamp": "2026-04-30T02:14:06Z", "_history_id": 30}),
            json!({"role": "user", "content": "internal", "timestamp": "2026-04-30T02:14:07Z", "meta": {"type": "model_context_internal", "hidden": true, "internal_user": true}, "_history_id": 31}),
            json!({"role": "assistant", "content": "answer", "timestamp": "2026-04-30T02:14:08Z", "_history_id": 32}),
        ];

        let transcript = build_chat_transcript("sess", history, &HashMap::new());

        assert_eq!(transcript.len(), 3);
        assert_eq!(transcript[1]["hiddenInternal"], json!(true));
        assert_eq!(transcript[1]["user_turn_index"], json!(1));
        assert_eq!(transcript[2]["user_turn_index"], json!(1));
        assert_eq!(
            transcript[2]["model_turn_id"],
            json!("model-turn:sess:user:1:model:1")
        );
    }
}
