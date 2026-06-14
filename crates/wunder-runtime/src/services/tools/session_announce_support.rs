use super::{normalize_optional_string, now_ts};
use crate::storage::StorageBackend;
use crate::workspace::WorkspaceManager;
use chrono::Local;
use serde_json::{json, Value};
use std::sync::Arc;

const ANNOUNCE_SKIP: &str = "ANNOUNCE_SKIP";

#[derive(Debug, Clone)]
pub(crate) struct AnnounceConfig {
    pub(crate) parent_session_id: String,
    pub(crate) label: Option<String>,
    pub(crate) dispatch_id: Option<String>,
    pub(crate) strategy: Option<String>,
    pub(crate) completion_mode: Option<String>,
    pub(crate) remaining_action: Option<String>,
    pub(crate) parent_turn_ref: Option<String>,
    pub(crate) parent_user_round: Option<i64>,
    pub(crate) parent_model_round: Option<i64>,
    pub(crate) emit_parent_events: bool,
    pub(crate) auto_wake: bool,
    pub(crate) persist_history_message: bool,
}

pub(crate) fn should_auto_wake_parent_after_child_run(
    wait_forever: bool,
    timeout_seconds: f64,
) -> bool {
    !wait_forever && timeout_seconds <= 0.0
}

pub(crate) fn should_auto_wake_parent_follow_up(
    is_swarm_task: bool,
    wait_forever: bool,
    timeout_seconds: f64,
) -> bool {
    !is_swarm_task && should_auto_wake_parent_after_child_run(wait_forever, timeout_seconds)
}

pub(crate) fn sync_announce_auto_wake(
    announce: &mut AnnounceConfig,
    run_metadata: Option<&mut Value>,
    auto_wake: bool,
) {
    announce.auto_wake = auto_wake;
    if let Some(metadata) = run_metadata {
        insert_run_metadata_field(metadata, "auto_wake", json!(auto_wake));
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_parent_follow_up_announce(
    parent_session_id: Option<String>,
    child_session_id: &str,
    label: Option<String>,
    emit_parent_events: bool,
    persist_history_message: bool,
    auto_wake: bool,
    parent_turn_ref: Option<String>,
    parent_user_round: Option<i64>,
    parent_model_round: Option<i64>,
) -> Option<AnnounceConfig> {
    let child_session_id = child_session_id.trim();
    let parent_session_id = normalize_optional_string(parent_session_id)
        .filter(|parent_session_id| parent_session_id != child_session_id)?;
    Some(AnnounceConfig {
        parent_session_id,
        label,
        dispatch_id: None,
        strategy: None,
        completion_mode: None,
        remaining_action: None,
        parent_turn_ref,
        parent_user_round,
        parent_model_round,
        emit_parent_events,
        auto_wake,
        persist_history_message,
    })
}

pub(crate) fn insert_run_metadata_field(target: &mut Value, key: &str, value: Value) {
    let Some(object) = target.as_object_mut() else {
        return;
    };
    object.insert(key.to_string(), value);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn append_child_announce(
    workspace: &WorkspaceManager,
    storage: &Arc<dyn StorageBackend>,
    user_id: &str,
    parent_session_id: &str,
    child_session_id: &str,
    run_id: &str,
    status: &str,
    answer: Option<&str>,
    error: Option<&str>,
    elapsed_s: f64,
    model_name: Option<&str>,
    label: Option<&str>,
) {
    let result_text = if status == "success" {
        answer.unwrap_or("ok").trim()
    } else {
        error.unwrap_or("error").trim()
    };
    let mut notes = vec![
        format!("run_id={run_id}"),
        format!("session_id={child_session_id}"),
        format!("elapsed_s={elapsed_s:.2}"),
    ];
    if let Some(model) = model_name {
        if !model.trim().is_empty() {
            notes.push(format!("model={}", model.trim()));
        }
    }
    if let Some(label) = label {
        if !label.trim().is_empty() {
            notes.push(format!("label={}", label.trim()));
        }
    }
    let content = format!(
        "Status: {status}\nResult: {result}\nNotes: {notes}",
        status = status,
        result = result_text,
        notes = notes.join(", ")
    );
    let timestamp = Local::now().to_rfc3339();
    let meta = json!({
        "type": "subagent_announce",
        "run_id": run_id,
        "child_session_id": child_session_id,
        "status": status,
        "elapsed_s": elapsed_s
    });
    let payload = json!({
        "role": "assistant",
        "content": content,
        "session_id": parent_session_id,
        "timestamp": timestamp,
        "meta": meta,
    });
    let _ = workspace.append_chat(user_id, &payload);
    let now = now_ts();
    let _ = storage.touch_chat_session(user_id, parent_session_id, now, now);
}

pub(crate) fn should_skip_announce(answer: Option<&str>) -> bool {
    answer
        .map(|value| value.trim() == ANNOUNCE_SKIP)
        .unwrap_or(false)
}
