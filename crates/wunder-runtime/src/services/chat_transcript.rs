use crate::services::chat_cancel_marker::{
    is_tool_call_meta, is_tool_payload_text, is_tool_payload_value, normalize_message_content,
};
use crate::services::chat_payload_sanitizer::sanitize_loaded_chat_record;
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

pub fn build_chat_transcript(session_id: &str, history: Vec<Value>) -> Vec<Value> {
    let mut cursor = TranscriptCursor::default();
    // Cancellation is requested through more than one transport in the web
    // client. Older sessions can therefore contain several identical visible
    // cancellation rows after one user message. Collapse those rows before
    // assigning model turn identities; otherwise each duplicate becomes a
    // separate assistant bubble during refresh/replay.
    let history = dedupe_cancelled_markers(history);
    let page_user_rounds = collect_explicit_user_rounds(&history);
    let mut transcript = Vec::new();
    for item in history {
        if let Some(message) =
            map_transcript_message(session_id, item, &page_user_rounds, &mut cursor)
        {
            transcript.push(message);
        }
    }
    // One user turn may contain several model requests (for example a tool
    // call followed by the final answer).  ThreadLog stores each durable
    // assistant Item so that usage and workflow metadata remain addressable,
    // but the conversation surface has one assistant bubble per user turn.
    // Fold those Items before exposing the transcript; the frontend applies
    // the same rule to live snapshots.
    coalesce_assistant_messages_by_user_turn(&mut transcript);
    if transcript.iter().any(has_trusted_transcript_round) {
        sort_transcript_messages(&mut transcript);
    }
    renumber_transcript_messages(&mut transcript);
    transcript
}

fn map_transcript_message(
    session_id: &str,
    item: Value,
    page_user_rounds: &HashSet<i64>,
    cursor: &mut TranscriptCursor,
) -> Option<Value> {
    let role = item
        .get("role")
        .and_then(Value::as_str)
        .or_else(|| match item.get("kind").and_then(Value::as_str) {
            Some("user_message") => Some("user"),
            Some("assistant_message") => Some("assistant"),
            _ => None,
        })?
        .to_string();
    if role == "system" || role == "tool" {
        return None;
    }
    let hidden_internal = is_hidden_internal_history_message(&item);
    if hidden_internal {
        return None;
    }
    let item = sanitize_loaded_chat_record(item);
    let raw_content = item.get("content").cloned().unwrap_or(Value::Null);
    let content = normalize_message_content(&raw_content);
    let reasoning = if role == "assistant" {
        item.get("reasoning_content")
            .or_else(|| item.get("reasoning"))
            .and_then(Value::as_str)
            .unwrap_or("")
    } else {
        ""
    };
    if role == "assistant"
        && should_hide_assistant_history_item(&item, &raw_content, &content, reasoning)
    {
        return None;
    }

    let item_id = item.get("item_id").and_then(Value::as_str);
    let created_seq = item.get("created_seq").and_then(Value::as_i64);
    let created_at = item
        .get("timestamp")
        .and_then(Value::as_str)
        .map(format_ts_text)
        .unwrap_or_default();
    let raw_user_round = resolve_history_user_round(&item);
    let raw_model_round =
        positive_i64(item.get("model_round")).or_else(|| positive_i64(item.get("modelRound")));
    let trusted_user_round = is_trusted_history_user_round(
        role.as_str(),
        &item,
        raw_user_round,
        hidden_internal,
        page_user_rounds,
        cursor,
    );
    let (user_turn_index, model_turn_index) = resolve_transcript_turn_indexes(
        role.as_str(),
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
    let message_id = item_id
        .map(|id| format!("item:{id}"))
        .unwrap_or_else(|| resolve_message_id(session_id, role.as_str(), turn_index));
    let status = resolve_message_status(role.as_str(), &item);
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
        // Preserve the durable ThreadLog identity in the UI projection.  The
        // message id remains a compatibility/rendering key, while these
        // fields let realtime reconciliation target one Item revision without
        // rebuilding the whole transcript.
        for key in ["item_id", "turn_id", "kind", "visibility", "revision"] {
            if let Some(value) = item.get(key) {
                map.insert(key.to_string(), value.clone());
            }
        }
        if let Some(created_seq) = created_seq {
            map.insert("created_seq".to_string(), json!(created_seq));
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
        if role == "assistant" {
            if let Some(stats) = extract_persisted_message_stats(&item) {
                map.insert("stats".to_string(), stats);
            }
        }
        if let Some(panel) = extract_question_panel(&item) {
            map.insert("questionPanel".to_string(), panel);
        }
        if let Some(attachments) = normalized_attachments(&item) {
            map.insert("attachments".to_string(), attachments);
        }
        if role == "user" {
            if let Some(meta) = item.get("meta").and_then(Value::as_object) {
                if meta.get("type").and_then(Value::as_str) == Some("manual_compaction_command") {
                    map.insert("manual_compaction_command".to_string(), Value::Bool(true));
                }
                if meta.get("type").and_then(Value::as_str) == Some("goal_command") {
                    map.insert("goal_command".to_string(), Value::Bool(true));
                }
            }
        }
        if role == "assistant" {
            if let Some(meta) = item.get("meta").and_then(Value::as_object) {
                if meta.get("type").and_then(Value::as_str) == Some("manual_compaction_marker") {
                    map.insert("manual_compaction_marker".to_string(), Value::Bool(true));
                    let running = meta.get("status").and_then(Value::as_str) == Some("running");
                    map.insert("workflowStreaming".to_string(), Value::Bool(running));
                    map.insert("stream_incomplete".to_string(), Value::Bool(running));
                    let detail = Value::Object(meta.clone());
                    map.insert(
                        "workflowItems".to_string(),
                        json!([{
                            "id": format!("compaction:{}", item_id.unwrap_or("turn")),
                            "eventType": "compaction",
                            "toolName": "context_compaction",
                            "status": meta.get("status").and_then(Value::as_str).unwrap_or("completed"),
                            "detail": detail.to_string(),
                            "toolCallId": meta.get("compaction_id").and_then(Value::as_str).unwrap_or("")
                        }]),
                    );
                }
            }
        }
        if role == "assistant" {
            if let Some(feedback) = item.get("feedback") {
                map.insert("feedback".to_string(), feedback.clone());
            }
        }
    }
    Some(message)
}

fn dedupe_cancelled_markers(history: Vec<Value>) -> Vec<Value> {
    let mut result = Vec::with_capacity(history.len());
    let mut has_visible_user = false;
    let mut marker_seen_for_user = false;
    for item in history {
        let role = item.get("role").and_then(Value::as_str).unwrap_or("");
        if role == "user" && !is_hidden_internal_history_message(&item) {
            has_visible_user = true;
            marker_seen_for_user = false;
        }
        if role == "assistant" && has_visible_user && is_cancelled_history_message(&item) {
            if marker_seen_for_user {
                continue;
            }
            marker_seen_for_user = true;
        }
        result.push(item);
    }
    result
}

fn coalesce_assistant_messages_by_user_turn(messages: &mut Vec<Value>) {
    let mut result = Vec::with_capacity(messages.len());
    let mut assistant_index_by_turn = HashMap::<String, usize>::new();
    for message in messages.drain(..) {
        if message.get("role").and_then(Value::as_str) != Some("assistant")
            || is_special_assistant_transcript_message(&message)
        {
            result.push(message);
            continue;
        }
        // A persisted root `turn_id` is the only safe durable coalescing key.
        // Legacy rows can reuse a stale user_round across separate user
        // inputs, so historical hydration must not infer one bubble from the
        // synthesized round key.
        let turn_key = message
            .get("turn_id")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!("turn:{value}"));
        let Some(turn_key) = turn_key else {
            result.push(message);
            continue;
        };
        let Some(&index) = assistant_index_by_turn.get(&turn_key) else {
            assistant_index_by_turn.insert(turn_key, result.len());
            result.push(message);
            continue;
        };
        let existing = &mut result[index];
        merge_transcript_assistant_message(existing, &message);
    }
    *messages = result;
}

fn is_special_assistant_transcript_message(message: &Value) -> bool {
    message
        .get("manual_compaction_marker")
        .and_then(Value::as_bool)
        == Some(true)
        || message
            .get("meta")
            .and_then(Value::as_object)
            .and_then(|meta| meta.get("type"))
            .and_then(Value::as_str)
            .is_some_and(|kind| kind == "manual_compaction_marker")
}

fn merge_transcript_assistant_message(target: &mut Value, source: &Value) {
    let (Some(target_map), Some(source_map)) = (target.as_object_mut(), source.as_object()) else {
        return;
    };
    if source_map.get("cancelled").and_then(Value::as_bool) == Some(true) {
        target_map.insert("status".into(), json!("cancelled"));
        target_map.insert("cancelled".into(), json!(true));
        if let Some(reason) = source_map.get("stop_reason") {
            target_map.insert("stop_reason".into(), reason.clone());
        }
        if target_map
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .is_empty()
        {
            target_map.insert(
                "content".into(),
                source_map.get("content").cloned().unwrap_or(json!("")),
            );
        }
        return;
    }
    if should_replace_transcript_assistant_target(target_map, source_map) {
        let old = std::mem::take(target_map);
        *target_map = source_map.clone();
        // `target_map` is the newer/richer snapshot here.  Keep its text when
        // the older snapshot contains a different model round (for example,
        // tool-call reasoning followed by the final answer).
        merge_transcript_assistant_fields(target_map, &old, false);
    } else {
        merge_transcript_assistant_fields(target_map, source_map, false);
    }
}

fn should_replace_transcript_assistant_target(
    target: &serde_json::Map<String, Value>,
    source: &serde_json::Map<String, Value>,
) -> bool {
    let round = |map: &serde_json::Map<String, Value>| {
        map.get("model_round")
            .or_else(|| map.get("model_turn_index"))
            .and_then(Value::as_i64)
            .unwrap_or(0)
    };
    if round(source) != round(target) {
        return round(source) > round(target);
    }
    if target.get("item_id") == source.get("item_id") {
        let revision = |map: &serde_json::Map<String, Value>| {
            map.get("revision").and_then(Value::as_i64).unwrap_or(0)
        };
        if revision(source) != revision(target) {
            return revision(source) > revision(target);
        }
    }
    transcript_assistant_score(source) > transcript_assistant_score(target)
}

fn transcript_assistant_score(message: &serde_json::Map<String, Value>) -> i32 {
    let mut score = 0;
    if message
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        score += 8;
    }
    if message
        .get("reasoning")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        score += 2;
    }
    if message.get("stats").is_some() {
        score += 3;
    }
    if message
        .get("item_id")
        .and_then(Value::as_str)
        .is_some_and(|value| value.contains(":text-"))
    {
        score += 3;
    }
    if message.get("status").and_then(Value::as_str) == Some("final") {
        score += 2;
    }
    score
}

fn merge_transcript_assistant_fields(
    target: &mut serde_json::Map<String, Value>,
    source: &serde_json::Map<String, Value>,
    prefer_source_text: bool,
) {
    for (key, value) in source {
        if key == "content" || key == "reasoning" {
            let current = target.get(key).and_then(Value::as_str).unwrap_or("");
            let incoming = value.as_str().unwrap_or("");
            if !incoming.is_empty() {
                let merged = merge_transcript_text(current, incoming, prefer_source_text);
                if merged != current {
                    target.insert(key.clone(), Value::String(merged));
                }
            }
            continue;
        }
        if key == "stats" {
            if let (Some(current), Some(incoming)) = (target.get_mut(key), value.as_object()) {
                merge_transcript_object(current, incoming);
            } else if !target.contains_key(key) {
                target.insert(key.clone(), value.clone());
            }
            continue;
        }
        if key == "revision" {
            continue;
        }
        if key == "created_seq" {
            let current = target.get(key).and_then(Value::as_i64).unwrap_or(0);
            let incoming = value.as_i64().unwrap_or(0);
            if key == "revision" {
                target.insert(key.clone(), json!(current.max(incoming)));
            } else if current == 0 || (incoming > 0 && incoming < current) {
                target.insert(key.clone(), value.clone());
            }
            continue;
        }
        if !target.contains_key(key)
            || target
                .get(key)
                .is_some_and(|current| current.is_null() || current.as_str() == Some(""))
        {
            target.insert(key.clone(), value.clone());
        }
    }
}

fn merge_transcript_text(current: &str, incoming: &str, prefer_incoming: bool) -> String {
    if current.is_empty() || current == incoming {
        return incoming.to_string();
    }
    if incoming.is_empty() || current.starts_with(incoming) {
        return current.to_string();
    }
    if !prefer_incoming {
        return current.to_string();
    }
    if incoming.starts_with(current) {
        return incoming.to_string();
    }
    if prefer_incoming {
        incoming.to_string()
    } else {
        current.to_string()
    }
}

fn merge_transcript_object(target: &mut Value, source: &serde_json::Map<String, Value>) {
    let Some(target_map) = target.as_object_mut() else {
        *target = Value::Object(source.clone());
        return;
    };
    for (key, value) in source {
        if let (Some(Value::Object(_)), Value::Object(incoming)) = (target_map.get(key), value) {
            let mut merged = target_map.remove(key).unwrap_or(Value::Null);
            merge_transcript_object(&mut merged, incoming);
            target_map.insert(key.clone(), merged);
            continue;
        }
        if let (Some(Value::Number(current)), Value::Number(incoming)) =
            (target_map.get(key), value)
        {
            if incoming.as_f64().unwrap_or(0.0) > current.as_f64().unwrap_or(0.0) {
                target_map.insert(key.clone(), value.clone());
            }
            continue;
        }
        if target_map
            .get(key)
            .is_none_or(|current| current.is_null() || current.as_str() == Some(""))
        {
            target_map.insert(key.clone(), value.clone());
        }
    }
}

fn extract_persisted_message_stats(item: &Value) -> Option<Value> {
    let mut stats = item
        .get("meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("message_stats"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for key in [
        "decode_output_tokens",
        "decode_tokens",
        "decode_duration_s",
        "decode_speed_tps",
        "visible_decode_tokens",
        "visible_decode_duration_s",
        "visible_decode_speed_tps",
        "prefill_duration_s",
        "prefill_speed_tps",
        "stream_timing",
        "usage",
        "round_usage",
        "context_occupancy_tokens",
    ] {
        if let Some(value) = item.get(key) {
            stats
                .entry(key.to_string())
                .or_insert_with(|| value.clone());
        }
    }
    if stats.get("visible_decode_tokens").is_none() {
        if let Some(tokens) = item
            .get("decode_output_tokens")
            .or_else(|| item.get("decode_tokens"))
        {
            stats.insert("visible_decode_tokens".into(), tokens.clone());
        }
    }
    let timing_ms = item
        .get("stream_timing")
        .and_then(Value::as_object)
        .and_then(|timing| timing.get("content_decode_ms"))
        .and_then(Value::as_f64)
        .filter(|value| *value > 0.0);
    if stats.get("visible_decode_duration_s").is_none() {
        if let Some(ms) = timing_ms {
            stats.insert("visible_decode_duration_s".into(), json!(ms / 1000.0));
        }
    }
    if stats.get("visible_decode_speed_tps").is_none() {
        let tokens = stats.get("visible_decode_tokens").and_then(Value::as_f64);
        let duration = stats
            .get("visible_decode_duration_s")
            .and_then(Value::as_f64);
        if let (Some(tokens), Some(duration)) = (tokens, duration) {
            if tokens > 0.0 && duration > 0.0 {
                stats.insert("visible_decode_speed_tps".into(), json!(tokens / duration));
                stats.insert("visible_decode_measured".into(), json!(true));
            }
        }
    }
    (!stats.is_empty()).then_some(Value::Object(stats))
}

fn resolve_message_id(session_id: &str, role: &str, turn_index: i64) -> String {
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
    if matches!(status.as_str(), "running" | "streaming" | "waiting_input") {
        return "streaming";
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
                positive_i64(left.get("created_seq"))
                    .unwrap_or(i64::MAX)
                    .cmp(&positive_i64(right.get("created_seq")).unwrap_or(i64::MAX))
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
                .map(|value| {
                    value == crate::services::subagents::HIDDEN_HISTORY_META_TYPE
                        || value == "model_context_internal"
                })
                .unwrap_or(false)
                || meta.get("hidden").and_then(Value::as_bool).unwrap_or(false)
                || meta
                    .get("internal_user")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
        })
        .unwrap_or(false)
}

pub(crate) fn is_hidden_internal_history_message_for_cancel(item: &Value) -> bool {
    is_hidden_internal_history_message(item)
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
    fn latest_model_round_wins_without_borrowing_another_item_revision() {
        let transcript = build_chat_transcript(
            "thread",
            vec![
                json!({"role":"user","content":"request","user_round":1}),
                json!({"role":"assistant","content":"A longer preliminary response", "reasoning":"plan",
                "turn_id":"root","item_id":"root:text-1","revision":9,"model_round":1,"user_round":1}),
                json!({"role":"assistant","content":"Done.","turn_id":"root","item_id":"root:text-4",
                "revision":2,"model_round":4,"user_round":1}),
            ],
        );
        assert_eq!(transcript.len(), 2);
        assert_eq!(transcript[1]["content"], "Done.");
        assert_eq!(transcript[1]["item_id"], "root:text-4");
        assert_eq!(transcript[1]["revision"], 2);
    }

    #[test]
    fn cancelled_marker_settles_one_bubble_and_retains_partial_text() {
        let transcript = build_chat_transcript(
            "thread",
            vec![
                json!({"role":"user","content":"request","user_round":1}),
                json!({"role":"assistant","content":"Partial reply", "status":"cancelled",
                "turn_id":"root","item_id":"root:text-1","model_round":1,"user_round":1}),
                json!({"role":"assistant","content":"Cancelled", "turn_id":"root","item_id":"marker",
                "user_round":1,"meta":{"type":"session_cancelled","stop_reason":"user_stop"}}),
            ],
        );
        assert_eq!(transcript.len(), 2);
        assert_eq!(transcript[1]["status"], "cancelled");
        assert_eq!(transcript[1]["content"], "Partial reply");
    }

    #[test]
    fn transcript_does_not_fold_same_stream_round_assistants() {
        let history = vec![
            json!({"role": "assistant", "content": "greeting", "timestamp": "2026-04-30T02:14:01Z", "created_seq": 1}),
            json!({"role": "user", "content": "first", "timestamp": "2026-04-30T02:14:06Z", "created_seq": 2}),
            json!({"role": "assistant", "content": "first answer", "timestamp": "2026-04-30T02:14:07Z", "user_round": 1, "model_round": 1, "created_seq": 3}),
            json!({"role": "user", "content": "second", "timestamp": "2026-04-30T02:14:16Z", "created_seq": 4}),
            json!({"role": "assistant", "content": "second answer", "timestamp": "2026-04-30T02:14:18Z", "user_round": 1, "model_round": 1, "created_seq": 5}),
        ];
        let transcript = build_chat_transcript("sess", history);
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
    fn transcript_coalesces_tool_loop_assistant_items_into_one_bubble() {
        let history = vec![
            json!({
                "role": "user", "content": "hello", "user_round": 1,
                "round_info_source": "orchestrator", "created_seq": 1
            }),
            json!({
                "role": "assistant", "content": "", "reasoning": "decide to use a tool",
                "finish_reason": "tool_calls", "tool_calls": [{"id":"call-1"}],
                "user_round": 1, "model_round": 1,
                "round_info_source": "orchestrator", "created_seq": 2,
                "item_id": "turn:text-1", "turn_id": "turn-1"
            }),
            json!({
                "role": "assistant", "content": "", "reasoning_content": "decide to use a tool",
                "meta": {"type": "tool_call", "message_stats": {"contextTokens": 10}},
                "tool_calls": [{"id":"call-1"}], "user_round": 1, "model_round": 1,
                "round_info_source": "orchestrator", "created_seq": 3,
                "item_id": "duplicate-tool", "turn_id": "turn-1"
            }),
            json!({
                "role": "assistant", "content": "hello back", "reasoning": "final answer",
                "user_round": 1, "model_round": 2,
                "round_info_source": "orchestrator", "created_seq": 4,
                "item_id": "turn:text-2", "turn_id": "turn-1"
            }),
            json!({
                "role": "assistant", "content": "hello back", "reasoning_content": "final answer",
                "meta": {"message_stats": {"contextTokens": 20}},
                "user_round": 1, "model_round": 2,
                "round_info_source": "orchestrator", "created_seq": 5,
                "item_id": "duplicate-final", "turn_id": "turn-1"
            }),
        ];

        let transcript = build_chat_transcript("sess", history);
        assert_eq!(
            transcript
                .iter()
                .filter(|item| item["role"] == "assistant")
                .count(),
            1
        );
        let assistant = transcript
            .iter()
            .find(|item| item["role"] == "assistant")
            .unwrap();
        assert_eq!(assistant["content"], json!("hello back"));
        assert_eq!(assistant["reasoning"], json!("final answer"));
        assert_eq!(assistant["user_round"], json!(1));
        assert_eq!(assistant["model_round"], json!(2));
        assert_eq!(assistant["stats"]["contextTokens"], json!(20));
    }

    #[test]
    fn transcript_preserves_cancelled_marker_after_user_turn() {
        let history = vec![
            json!({"role": "user", "content": "stop me", "timestamp": "2026-04-30T02:14:06Z", "created_seq": 10}),
            json!({"role": "assistant", "content": "cancelled", "timestamp": "2026-04-30T02:14:07Z", "stop_reason": "user_stop", "created_seq": 11}),
        ];
        let transcript = build_chat_transcript("sess", history);

        assert_eq!(transcript.len(), 2);
        assert_eq!(transcript[1]["status"], json!("cancelled"));
        assert_eq!(transcript[1]["cancelled"], json!(true));
        assert_eq!(transcript[1]["user_turn_index"], json!(1));
    }

    #[test]
    fn transcript_collapses_duplicate_cancelled_markers_for_one_user_turn() {
        let history = vec![
            json!({"role": "user", "content": "stop me", "timestamp": "2026-04-30T02:14:06Z", "created_seq": 10}),
            json!({"role": "assistant", "content": "cancelled", "timestamp": "2026-04-30T02:14:07Z", "stop_reason": "user_stop", "created_seq": 11}),
            json!({"role": "assistant", "content": "cancelled", "timestamp": "2026-04-30T02:14:08Z", "meta": {"type": "session_cancelled"}, "created_seq": 12}),
            json!({"role": "assistant", "content": "cancelled", "timestamp": "2026-04-30T02:14:09Z", "cancelled": true, "created_seq": 13}),
        ];
        let transcript = build_chat_transcript("sess", history);

        assert_eq!(transcript.len(), 2);
        assert_eq!(transcript[1]["status"], json!("cancelled"));
        assert_eq!(transcript[1]["user_turn_index"], json!(1));
    }

    #[test]
    fn transcript_keeps_cancellations_for_distinct_user_turns() {
        let history = vec![
            json!({"role": "user", "content": "first", "timestamp": "2026-04-30T02:14:06Z", "created_seq": 20}),
            json!({"role": "assistant", "content": "cancelled", "timestamp": "2026-04-30T02:14:07Z", "stop_reason": "user_stop", "created_seq": 21}),
            json!({"role": "user", "content": "second", "timestamp": "2026-04-30T02:14:16Z", "created_seq": 22}),
            json!({"role": "assistant", "content": "cancelled", "timestamp": "2026-04-30T02:14:17Z", "stop_reason": "user_stop", "created_seq": 23}),
        ];
        let transcript = build_chat_transcript("sess", history);

        assert_eq!(transcript.len(), 4);
        assert_eq!(transcript[1]["user_turn_index"], json!(1));
        assert_eq!(transcript[3]["user_turn_index"], json!(2));
    }

    #[test]
    fn transcript_restores_persisted_assistant_message_stats() {
        let history = vec![json!({
            "role": "assistant",
            "content": "answer",
            "timestamp": "2026-04-30T02:14:07Z",
            "meta": {
                "message_stats": {
                    "round_usage": {"input_tokens": 12, "output_tokens": 8, "total_tokens": 20},
                    "decode_duration_total_s": 0.5,
                    "avg_model_round_speed_tps": 16.0
                }
            },
            "created_seq": 12
        })];

        let transcript = build_chat_transcript("sess", history);

        assert_eq!(
            transcript[0]["stats"]["round_usage"]["total_tokens"],
            json!(20)
        );
        assert_eq!(
            transcript[0]["stats"]["avg_model_round_speed_tps"],
            json!(16.0)
        );
    }

    #[test]
    fn transcript_binds_delayed_assistant_to_persisted_user_round() {
        let history = vec![
            json!({"role": "user", "content": "first", "timestamp": "2026-04-30T02:14:06Z", "user_round": 1, "created_seq": 20}),
            json!({"role": "user", "content": "second", "timestamp": "2026-04-30T02:14:16Z", "user_round": 2, "created_seq": 21}),
            json!({"role": "assistant", "content": "first answer", "timestamp": "2026-04-30T02:14:30Z", "user_round": 1, "model_round": 1, "round_info_source": "orchestrator", "created_seq": 22}),
        ];

        let transcript = build_chat_transcript("sess", history);
        let delayed = transcript
            .iter()
            .find(|item| item.get("created_seq") == Some(&json!(22)))
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
    fn transcript_keeps_manual_compaction_command_and_summary_in_one_round() {
        let history = vec![
            json!({
                "role": "user",
                "content": "/compact",
                "timestamp": "2026-04-30T02:14:16Z",
                "user_round": 2,
                "round_info_source": "orchestrator",
                "meta": {"type": "manual_compaction_command", "manual_compaction": true},
                "created_seq": 21
            }),
            json!({
                "role": "assistant",
                "content": "Compaction summary",
                "timestamp": "2026-04-30T02:14:17Z",
                "user_round": 2,
                "round_info_source": "orchestrator",
                "meta": {
                    "type": "manual_compaction_marker",
                    "manual_compaction": true,
                    "trigger_mode": "manual",
                    "status": "done",
                    "compaction_id": "compact-test"
                },
                "created_seq": 22
            }),
        ];

        let transcript = build_chat_transcript("sess", history);

        assert_eq!(transcript.len(), 2);
        assert_eq!(transcript[0]["content"], json!("/compact"));
        assert_eq!(transcript[0]["manual_compaction_command"], json!(true));
        assert_eq!(transcript[1]["content"], json!("Compaction summary"));
        assert_eq!(transcript[1]["manual_compaction_marker"], json!(true));
        assert_eq!(transcript[1]["user_round"], json!(2));
        assert_eq!(transcript[1]["workflowStreaming"], json!(false));
        assert_eq!(
            transcript[1]["workflowItems"][0]["toolCallId"],
            json!("compact-test")
        );
    }

    #[test]
    fn transcript_preserves_legacy_history_order_without_trusted_rounds() {
        let history = vec![
            json!({"role": "user", "content": "first", "timestamp": "2026-04-30T02:14:06Z", "created_seq": 40}),
            json!({"role": "assistant", "content": "first answer", "timestamp": "2026-04-30T02:14:07Z", "created_seq": 41}),
            json!({"role": "user", "content": "second", "timestamp": "2026-04-30T02:14:16Z", "created_seq": 42}),
            json!({"role": "assistant", "content": "second answer", "timestamp": "2026-04-30T02:14:18Z", "created_seq": 43}),
        ];

        let transcript = build_chat_transcript("sess", history);

        assert_eq!(transcript.len(), 4);
        assert_eq!(transcript[0]["content"], json!("first"));
        assert_eq!(transcript[1]["content"], json!("first answer"));
        assert_eq!(transcript[2]["content"], json!("second"));
        assert_eq!(transcript[3]["content"], json!("second answer"));
    }

    #[test]
    fn hidden_internal_user_does_not_advance_visible_turn_binding() {
        let history = vec![
            json!({"role": "user", "content": "visible", "timestamp": "2026-04-30T02:14:06Z", "created_seq": 30}),
            json!({"role": "user", "content": "internal", "timestamp": "2026-04-30T02:14:07Z", "meta": {"type": "model_context_internal", "hidden": true, "internal_user": true}, "created_seq": 31}),
            json!({"role": "assistant", "content": "answer", "timestamp": "2026-04-30T02:14:08Z", "created_seq": 32}),
        ];

        let transcript = build_chat_transcript("sess", history);

        assert_eq!(transcript.len(), 2);
        assert_eq!(transcript[0]["content"], json!("visible"));
        assert_eq!(transcript[1]["content"], json!("answer"));
        assert_eq!(transcript[1]["user_turn_index"], json!(1));
        assert_eq!(
            transcript[1]["model_turn_id"],
            json!("model-turn:sess:user:1:model:1")
        );
    }

    #[test]
    fn transcript_omits_hidden_internal_inline_image_followups() {
        let history = vec![json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "inspect"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}
            ],
            "timestamp": "2026-04-30T02:14:07Z",
            "meta": {"type": "model_context_internal", "hidden": true, "internal_user": true},
            "created_seq": 31
        })];

        let transcript = build_chat_transcript("sess", history);

        assert!(transcript.is_empty());
    }

    #[test]
    fn transcript_sanitizes_legacy_visible_inline_image_data_urls() {
        let history = vec![json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "inspect"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}
            ],
            "timestamp": "2026-04-30T02:14:07Z",
            "created_seq": 32
        })];

        let transcript = build_chat_transcript("sess", history);

        assert_eq!(transcript.len(), 1);
        assert!(!transcript[0].to_string().contains("data:image/png;base64"));
        assert!(transcript[0]
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .contains("inline image omitted"));
    }
}
