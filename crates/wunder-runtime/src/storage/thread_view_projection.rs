//! Write-time display projection, shared by both transactional stores.
use serde_json::Value;
use wunder_core::thread_view::{preview, BodyRef, ChatViewRow, TrajectoryViewRow, ViewMessage};

pub(crate) fn body_id(item_id: &str, field: &str) -> String {
    format!("{item_id}|{field}")
}

pub(crate) fn message(item_id: &str, content: &str, revision: i64) -> ViewMessage {
    ViewMessage {
        item_id: item_id.into(),
        content: preview(content, 2048),
        body: Some(BodyRef {
            body_id: body_id(item_id, "content"),
            revision,
            bytes: content.len(),
        }),
        ..Default::default()
    }
}

pub(crate) fn stable_answer(item_id: &str, turn_id: &str, kind: &str) -> bool {
    kind == "assistant_message" && item_id.starts_with(&format!("{turn_id}:text-"))
}

pub(crate) fn is_failed(status: &str) -> i64 {
    i64::from(matches!(status, "failed" | "rejected"))
}
pub(crate) fn is_running(status: &str) -> i64 {
    i64::from(matches!(
        status,
        "running" | "queued" | "waiting_input" | "waiting_approval"
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn trajectory(
    item: &Value,
    item_id: &str,
    turn_id: &str,
    root: &str,
    round: i64,
    index: i64,
    seq: i64,
    revision: i64,
    kind: &str,
    status: &str,
    created: f64,
    updated: f64,
) -> TrajectoryViewRow {
    let content = item.get("content").and_then(Value::as_str).unwrap_or("");
    let summary = if content.is_empty() {
        item.get("summary")
            .or_else(|| item.get("tool_name"))
            .and_then(Value::as_str)
            .unwrap_or(kind)
    } else {
        content
    };
    TrajectoryViewRow {
        item_id: item_id.into(),
        turn_id: turn_id.into(),
        root_turn_id: root.into(),
        user_round: round,
        item_index: index,
        created_seq: seq,
        revision,
        kind: preview(kind, 48),
        status: preview(status, 48),
        summary: preview(summary, 256),
        tool_name: item
            .get("tool_name")
            .and_then(Value::as_str)
            .map(|v| preview(v, 96)),
        model_round: item.get("model_round").and_then(Value::as_i64).unwrap_or(0),
        started_at: item
            .get("started_at")
            .and_then(Value::as_f64)
            .unwrap_or(created),
        completed_at: (is_running(status) == 0).then_some(updated),
        detail: Some(BodyRef {
            body_id: body_id(item_id, "detail"),
            revision,
            bytes: 0,
        }),
    }
}

pub(crate) fn new_chat(
    root: &str,
    round: i64,
    client: Option<String>,
    status: &str,
    created: f64,
    updated: f64,
) -> ChatViewRow {
    ChatViewRow {
        root_turn_id: root.into(),
        user_round: round,
        client_message_id: client,
        status: status.into(),
        created_time: created,
        updated_time: updated,
        ..Default::default()
    }
}

pub(crate) fn finite_stats(item: &Value) -> Option<Value> {
    let source = item
        .get("message_stats")
        .or_else(|| item.pointer("/meta/message_stats"))?
        .as_object()?;
    let fields = source
        .iter()
        .take(24)
        .filter(|(_, v)| v.is_number() || v.is_boolean())
        .map(|(k, v)| (preview(k, 64), v.clone()))
        .collect::<serde_json::Map<_, _>>();
    Some(Value::Object(fields))
}
