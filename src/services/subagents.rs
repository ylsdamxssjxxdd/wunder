use crate::monitor::MonitorState;
use crate::orchestrator::Orchestrator;
use crate::orchestrator_constants::OBSERVATION_PREFIX;
use crate::schemas::WunderRequest;
use crate::services::stream_events::StreamEventService;
use crate::storage::{ChatSessionRecord, SessionRunRecord, StorageBackend};
use anyhow::{anyhow, Result};
use chrono::Utc;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::warn;

pub const AUTO_WAKE_CONFIG_KEY: &str = "_subagent_auto_wake";
pub const HIDE_START_QUESTION_CONFIG_KEY: &str = "_subagent_hide_start_question";
pub const SKIP_STREAM_CLEAR_CONFIG_KEY: &str = "_subagent_skip_stream_clear";
pub const HIDDEN_USER_MESSAGE_CONFIG_KEY: &str = "_subagent_hidden_user_message";
pub const SKIP_AUTO_MEMORY_CONFIG_KEY: &str = "_subagent_skip_auto_memory_extract";
pub const HIDDEN_HISTORY_META_TYPE: &str = "subagent_hidden_observation";

const PARENT_TURN_REF_PREFIX: &str = "subagent_turn:";
const DEFAULT_LIST_LIMIT: i64 = 200;
const MAX_LIST_LIMIT: i64 = 500;
const AUTO_WAKE_OBSERVATION_MAX_CHARS: usize = 240;
const AUTO_WAKE_PARENT_UNLOCK_POLL_MS: u64 = 250;
const AUTO_WAKE_PARENT_UNLOCK_MAX_WAIT_MS: u64 = 15_000;

#[derive(Debug, Clone)]
pub struct ParentTurnRef {
    pub user_round: i64,
    pub model_round: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ParentDispatchConfig {
    pub parent_session_id: String,
    pub dispatch_id: Option<String>,
    pub strategy: Option<String>,
    pub completion_mode: Option<String>,
    pub remaining_action: Option<String>,
    pub label: Option<String>,
    pub parent_turn_ref: Option<String>,
    pub parent_user_round: Option<i64>,
    pub parent_model_round: Option<i64>,
    pub emit_parent_events: bool,
    pub auto_wake: bool,
}

#[derive(Debug, Clone)]
struct SubagentRuntimeItem {
    session: ChatSessionRecord,
    run: Option<SessionRunRecord>,
    status: String,
    terminal: bool,
    failed: bool,
    summary: Option<String>,
    user_message: Option<String>,
    assistant_message: Option<String>,
    error_message: Option<String>,
    updated_time: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionMode {
    All,
    Any,
    FirstSuccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemainingAction {
    Keep,
    Interrupt,
    Close,
}

#[derive(Debug, Clone, Copy)]
struct CompletionProgress {
    completion_reached: bool,
    all_finished: bool,
    completed_reason: &'static str,
}

pub fn encode_parent_turn_ref(user_round: Option<i64>, model_round: Option<i64>) -> Option<String> {
    let user_round = user_round.unwrap_or(0);
    if user_round <= 0 {
        return None;
    }
    let model_round = model_round.unwrap_or(0).max(0);
    Some(format!(
        "{PARENT_TURN_REF_PREFIX}{user_round}:{model_round}"
    ))
}

pub fn decode_parent_turn_ref(value: Option<&str>) -> Option<ParentTurnRef> {
    let raw = value?.trim();
    let tail = raw.strip_prefix(PARENT_TURN_REF_PREFIX)?;
    let mut parts = tail.split(':');
    let user_round = parts.next()?.trim().parse::<i64>().ok()?;
    if user_round <= 0 {
        return None;
    }
    let model_round = parts
        .next()
        .and_then(|part| part.trim().parse::<i64>().ok())
        .filter(|value| *value > 0);
    Some(ParentTurnRef {
        user_round,
        model_round,
    })
}

pub fn build_hidden_user_meta() -> Value {
    json!({
        "type": HIDDEN_HISTORY_META_TYPE,
        "hidden": true,
        "internal_user": true,
    })
}

pub fn build_auto_wake_request_overrides(base: Option<&Value>) -> Value {
    let mut payload = match base.cloned() {
        Some(Value::Object(map)) => Value::Object(map),
        _ => json!({}),
    };
    let Some(object) = payload.as_object_mut() else {
        return json!({
            AUTO_WAKE_CONFIG_KEY: true,
            HIDE_START_QUESTION_CONFIG_KEY: true,
            SKIP_STREAM_CLEAR_CONFIG_KEY: true,
            HIDDEN_USER_MESSAGE_CONFIG_KEY: true,
            SKIP_AUTO_MEMORY_CONFIG_KEY: true,
        });
    };
    object.insert(AUTO_WAKE_CONFIG_KEY.to_string(), Value::Bool(true));
    object.insert(
        HIDE_START_QUESTION_CONFIG_KEY.to_string(),
        Value::Bool(true),
    );
    object.insert(SKIP_STREAM_CLEAR_CONFIG_KEY.to_string(), Value::Bool(true));
    object.insert(
        HIDDEN_USER_MESSAGE_CONFIG_KEY.to_string(),
        Value::Bool(true),
    );
    object.insert(SKIP_AUTO_MEMORY_CONFIG_KEY.to_string(), Value::Bool(true));
    payload
}

pub fn config_flag(config_overrides: Option<&Value>, key: &str) -> bool {
    config_overrides
        .and_then(|value| value.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn cloned_metadata_field(metadata: Option<&Value>, key: &str) -> Option<Value> {
    metadata
        .and_then(|value| value.get(key))
        .filter(|value| !value.is_null())
        .cloned()
}

pub(crate) fn run_metadata_field(metadata: Option<&Value>, key: &str) -> Value {
    cloned_metadata_field(metadata, key).unwrap_or(Value::Null)
}

pub(crate) fn parent_turn_payload(
    metadata: Option<&Value>,
    fallback_parent_turn_ref: Option<&str>,
) -> (Value, Value, Value) {
    let fallback_turn = decode_parent_turn_ref(fallback_parent_turn_ref);
    let fallback_ref = encode_parent_turn_ref(
        fallback_turn.as_ref().map(|value| value.user_round),
        fallback_turn.as_ref().and_then(|value| value.model_round),
    );
    let parent_turn_ref = cloned_metadata_field(metadata, "parent_turn_ref")
        .unwrap_or_else(|| fallback_ref.map(Value::String).unwrap_or(Value::Null));
    let parent_user_round =
        cloned_metadata_field(metadata, "parent_user_round").unwrap_or_else(|| {
            fallback_turn
                .as_ref()
                .map(|value| json!(value.user_round))
                .unwrap_or(Value::Null)
        });
    let parent_model_round =
        cloned_metadata_field(metadata, "parent_model_round").unwrap_or_else(|| {
            fallback_turn
                .as_ref()
                .and_then(|value| value.model_round)
                .map(|value| json!(value))
                .unwrap_or(Value::Null)
        });
    (parent_turn_ref, parent_user_round, parent_model_round)
}

pub fn list_parent_subagents(
    storage: &dyn StorageBackend,
    monitor: Option<&MonitorState>,
    user_id: &str,
    parent_session_id: &str,
    limit: Option<i64>,
) -> Result<Vec<Value>> {
    let cleaned_user = user_id.trim();
    let cleaned_parent = parent_session_id.trim();
    if cleaned_user.is_empty() {
        return Err(anyhow!("user_id is required"));
    }
    if cleaned_parent.is_empty() {
        return Err(anyhow!("parent_session_id is required"));
    }
    let safe_limit = limit.unwrap_or(DEFAULT_LIST_LIMIT).clamp(1, MAX_LIST_LIMIT);
    let (sessions, _) = storage.list_chat_sessions_by_status(
        cleaned_user,
        None,
        Some(cleaned_parent),
        Some("all"),
        0,
        safe_limit,
    )?;
    let mut items = sessions
        .into_iter()
        .map(|session| build_runtime_item(storage, monitor, cleaned_user, session))
        .collect::<Result<Vec<_>>>()?;
    items.sort_by(|left, right| {
        right
            .updated_time
            .partial_cmp(&left.updated_time)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(items
        .into_iter()
        .map(runtime_item_payload)
        .collect::<Vec<_>>())
}

pub fn control_parent_subagents(
    storage: &dyn StorageBackend,
    monitor: Option<&MonitorState>,
    user_id: &str,
    parent_session_id: &str,
    action: &str,
    target_session_ids: &[String],
    dispatch_id: Option<&str>,
) -> Result<Value> {
    let normalized_action = action.trim().to_ascii_lowercase();
    let mut items = list_parent_subagents(storage, monitor, user_id, parent_session_id, None)?;
    if let Some(dispatch_id) = dispatch_id.map(str::trim).filter(|value| !value.is_empty()) {
        items.retain(|item| {
            item.get("dispatch_id")
                .and_then(Value::as_str)
                .map(str::trim)
                == Some(dispatch_id)
        });
    }
    let requested = target_session_ids
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<HashSet<_>>();
    if !requested.is_empty() {
        items.retain(|item| {
            item.get("session_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .is_some_and(|value| requested.contains(value))
        });
    }
    let mut updated = Vec::new();
    for item in items {
        let session_id = item
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        if session_id.is_empty() {
            continue;
        }
        match normalized_action.as_str() {
            "interrupt" | "cancel" | "stop" => {
                let changed = monitor.is_some_and(|entry| entry.cancel(session_id));
                updated.push(json!({
                    "session_id": session_id,
                    "run_id": item.get("run_id").cloned().unwrap_or(Value::Null),
                    "dispatch_id": item.get("dispatch_id").cloned().unwrap_or(Value::Null),
                    "updated": changed,
                    "status": if changed { "cancelling" } else { "unchanged" },
                    "action": "interrupt",
                }));
            }
            "terminate" | "close" => {
                let Some(mut record) = storage.get_chat_session(user_id, session_id)? else {
                    updated.push(json!({
                        "session_id": session_id,
                        "updated": false,
                        "status": "not_found",
                        "action": "close",
                    }));
                    continue;
                };
                if let Some(entry) = monitor {
                    let _ = entry.cancel(session_id);
                }
                let changed = record.status.trim() != "closed";
                if changed {
                    record.status = "closed".to_string();
                    record.updated_at = now_ts();
                    storage.upsert_chat_session(&record)?;
                }
                updated.push(json!({
                    "session_id": session_id,
                    "run_id": item.get("run_id").cloned().unwrap_or(Value::Null),
                    "dispatch_id": item.get("dispatch_id").cloned().unwrap_or(Value::Null),
                    "updated": changed,
                    "status": if changed { "closed" } else { "unchanged" },
                    "action": "close",
                }));
            }
            _ => return Err(anyhow!("unsupported subagent control action: {action}")),
        }
    }
    let updated_total = updated
        .iter()
        .filter(|item| {
            item.get("updated")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .count() as i64;
    Ok(json!({
        "status": if updated_total > 0 { "ok" } else { "noop" },
        "updated_total": updated_total,
        "items": updated,
    }))
}

#[allow(clippy::too_many_arguments)]
pub async fn handle_child_completion(
    storage: Arc<dyn StorageBackend>,
    monitor: Option<Arc<MonitorState>>,
    orchestrator: Arc<Orchestrator>,
    user_id: String,
    child_session_id: String,
    run_id: String,
    answer: Option<String>,
    error: Option<String>,
    config_overrides: Option<Value>,
    dispatch: ParentDispatchConfig,
) {
    let cleaned_parent = dispatch.parent_session_id.trim().to_string();
    if cleaned_parent.is_empty() {
        return;
    }

    let parent_item = match storage
        .get_chat_session(&user_id, &child_session_id)
        .ok()
        .flatten()
        .and_then(|session| {
            build_runtime_item(storage.as_ref(), monitor.as_deref(), &user_id, session).ok()
        }) {
        Some(item) => item,
        None => return,
    };

    if dispatch.emit_parent_events {
        let payload = runtime_item_payload(parent_item.clone());
        let _ = append_parent_stream_event(
            storage.clone(),
            &user_id,
            &cleaned_parent,
            "subagent_dispatch_item_update",
            payload,
        )
        .await;
    }

    let Some(dispatch_id) = dispatch
        .dispatch_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        if dispatch.auto_wake {
            let wake_payload = json!({
                "status": parent_item.status,
                "run_id": run_id,
                "session_id": child_session_id,
                "answer": answer,
                "error": error,
                "item": runtime_item_payload(parent_item),
            });
            schedule_parent_auto_wake(
                orchestrator,
                storage,
                &user_id,
                &cleaned_parent,
                config_overrides,
                dispatch,
                wake_payload,
            );
        }
        return;
    };

    let dispatch_key = dispatch_guard_key(&cleaned_parent, Some(dispatch_id), None);
    let items = match list_dispatch_runtime_items(
        storage.as_ref(),
        monitor.as_deref(),
        &user_id,
        &cleaned_parent,
        dispatch_id,
    ) {
        Ok(items) => items,
        Err(err) => {
            warn!(
                "list subagent dispatch items failed: parent_session_id={}, dispatch_id={}, error={err}",
                cleaned_parent, dispatch_id
            );
            return;
        }
    };
    if items.is_empty() {
        return;
    }

    let completion_mode = parse_completion_mode(dispatch.completion_mode.as_deref());
    let remaining_action = parse_remaining_action(dispatch.remaining_action.as_deref());
    let progress = evaluate_completion(completion_mode, &items);
    if !progress.completion_reached {
        return;
    }
    if !mark_dispatch_once(&dispatch_key) {
        return;
    }

    let mut finish_payload =
        build_dispatch_finish_payload(&dispatch, &items, completion_mode, progress);
    let settled_items = apply_remaining_action(
        storage.as_ref(),
        monitor.as_deref(),
        &user_id,
        remaining_action,
        &items,
        progress.completed_reason,
    );
    if let Some(object) = finish_payload.as_object_mut() {
        object.insert(
            "remaining_action".to_string(),
            Value::String(remaining_action.as_str().to_string()),
        );
        object.insert(
            "remaining_action_applied".to_string(),
            Value::Bool(!settled_items.is_empty()),
        );
        object.insert(
            "settled_total".to_string(),
            json!(settled_items.len() as i64),
        );
        object.insert(
            "settled_items".to_string(),
            Value::Array(settled_items.clone()),
        );
    }

    if dispatch.emit_parent_events {
        if let Err(err) = append_parent_stream_event(
            storage.clone(),
            &user_id,
            &cleaned_parent,
            "subagent_dispatch_finish",
            finish_payload.clone(),
        )
        .await
        {
            warn!(
                "append subagent dispatch finish failed: parent_session_id={}, dispatch_id={}, error={err}",
                cleaned_parent, dispatch_id
            );
        }
        for settled in &settled_items {
            let event_type = match settled
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or_default()
            {
                "close" => "subagent_close",
                _ => "subagent_interrupt",
            };
            let _ = append_parent_stream_event(
                storage.clone(),
                &user_id,
                &cleaned_parent,
                event_type,
                settled.clone(),
            )
            .await;
        }
    }

    if dispatch.auto_wake {
        schedule_parent_auto_wake(
            orchestrator,
            storage,
            &user_id,
            &cleaned_parent,
            config_overrides,
            dispatch,
            finish_payload,
        );
    }
}

pub async fn emit_child_runtime_update(
    storage: Arc<dyn StorageBackend>,
    monitor: Option<Arc<MonitorState>>,
    user_id: &str,
    parent_session_id: &str,
    child_session_id: &str,
) -> Result<()> {
    let cleaned_user = user_id.trim();
    let cleaned_parent = parent_session_id.trim();
    let cleaned_child = child_session_id.trim();
    if cleaned_user.is_empty() || cleaned_parent.is_empty() || cleaned_child.is_empty() {
        return Ok(());
    }
    let Some(session) = storage.get_chat_session(cleaned_user, cleaned_child)? else {
        return Ok(());
    };
    if session
        .parent_session_id
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        != cleaned_parent
    {
        return Ok(());
    }
    let item = build_runtime_item(storage.as_ref(), monitor.as_deref(), cleaned_user, session)?;
    let payload = runtime_item_payload(item);
    let _ = append_parent_stream_event(
        storage,
        cleaned_user,
        cleaned_parent,
        "subagent_dispatch_item_update",
        payload,
    )
    .await?;
    Ok(())
}

async fn append_parent_stream_event(
    storage: Arc<dyn StorageBackend>,
    user_id: &str,
    parent_session_id: &str,
    event_type: &str,
    data: Value,
) -> Result<i64> {
    let service = StreamEventService::new(storage);
    service
        .append_event(
            parent_session_id,
            user_id,
            json!({
                "event": event_type,
                "data": data,
                "timestamp": Utc::now().to_rfc3339(),
            }),
        )
        .await
}

fn schedule_parent_auto_wake(
    orchestrator: Arc<Orchestrator>,
    storage: Arc<dyn StorageBackend>,
    user_id: &str,
    parent_session_id: &str,
    config_overrides: Option<Value>,
    dispatch: ParentDispatchConfig,
    payload: Value,
) {
    let user_id = user_id.trim().to_string();
    let parent_session_id = parent_session_id.trim().to_string();
    if user_id.is_empty() || parent_session_id.is_empty() {
        return;
    }
    let wake_key = dispatch_guard_key(
        &parent_session_id,
        dispatch.dispatch_id.as_deref(),
        Some(dispatch.parent_turn_ref.as_deref().unwrap_or("__single__")),
    );
    if !mark_wake_once(&wake_key) {
        return;
    }
    let payload = build_auto_wake_payload(payload, &dispatch);
    let request = match build_parent_auto_wake_request(
        storage.as_ref(),
        &user_id,
        &parent_session_id,
        config_overrides.as_ref(),
        payload,
    ) {
        Ok(request) => request,
        Err(err) => {
            warn!(
                "build parent auto wake request failed: parent_session_id={}, error={err}",
                parent_session_id
            );
            unmark_wake_once(&wake_key);
            return;
        }
    };
    tokio::spawn(async move {
        wait_parent_session_unlock(storage.clone(), &user_id, &parent_session_id).await;
        if let Err(err) = run_parent_auto_wake(orchestrator, request).await {
            warn!(
                "run parent auto wake failed: parent_session_id={}, error={err}",
                parent_session_id
            );
            unmark_wake_once(&wake_key);
        }
    });
}

async fn wait_parent_session_unlock(
    storage: Arc<dyn StorageBackend>,
    user_id: &str,
    parent_session_id: &str,
) {
    let cleaned_user_id = user_id.trim();
    let cleaned_parent_session_id = parent_session_id.trim();
    if cleaned_user_id.is_empty() || cleaned_parent_session_id.is_empty() {
        return;
    }
    let deadline = Instant::now() + Duration::from_millis(AUTO_WAKE_PARENT_UNLOCK_MAX_WAIT_MS);
    loop {
        match parent_session_has_active_lock(
            storage.as_ref(),
            cleaned_user_id,
            cleaned_parent_session_id,
        ) {
            Ok(false) => return,
            Ok(true) => {
                if Instant::now() >= deadline {
                    return;
                }
            }
            Err(err) => {
                warn!(
                    "list parent session locks failed before auto wake: parent_session_id={}, error={err}",
                    cleaned_parent_session_id
                );
                return;
            }
        }
        sleep(Duration::from_millis(AUTO_WAKE_PARENT_UNLOCK_POLL_MS)).await;
    }
}

fn parent_session_has_active_lock(
    storage: &dyn StorageBackend,
    user_id: &str,
    parent_session_id: &str,
) -> Result<bool> {
    let cleaned_user_id = user_id.trim();
    let cleaned_parent_session_id = parent_session_id.trim();
    if cleaned_user_id.is_empty() || cleaned_parent_session_id.is_empty() {
        return Ok(false);
    }
    let now = now_ts();
    Ok(storage
        .list_session_locks_by_user(cleaned_user_id)?
        .into_iter()
        .any(|lock| {
            lock.session_id.trim() == cleaned_parent_session_id && lock.expires_at > now
        }))
}

fn build_parent_auto_wake_request(
    storage: &dyn StorageBackend,
    user_id: &str,
    parent_session_id: &str,
    config_overrides: Option<&Value>,
    payload: Value,
) -> Result<WunderRequest> {
    let Some(session) = storage.get_chat_session(user_id, parent_session_id)? else {
        return Err(anyhow!("parent session not found"));
    };
    Ok(WunderRequest {
        user_id: user_id.to_string(),
        question: format!("{OBSERVATION_PREFIX}{}", serde_json::to_string(&payload)?),
        tool_names: session.tool_overrides.clone(),
        skip_tool_calls: false,
        stream: true,
        debug_payload: false,
        session_id: Some(parent_session_id.to_string()),
        agent_id: session.agent_id.clone(),
        model_name: None,
        language: None,
        config_overrides: Some(build_auto_wake_request_overrides(config_overrides)),
        agent_prompt: None,
        attachments: None,
        allow_queue: true,
        is_admin: false,
        approval_tx: None,
    })
}

async fn run_parent_auto_wake(
    orchestrator: Arc<Orchestrator>,
    request: WunderRequest,
) -> Result<()> {
    let mut stream = Box::pin(orchestrator.stream(request).await?);
    use futures::StreamExt;
    while let Some(item) = stream.next().await {
        let event = item.expect("stream event should be infallible");
        let event_name = event.event.trim().to_ascii_lowercase();
        if event_name == "error" {
            let payload = event
                .data
                .get("data")
                .cloned()
                .unwrap_or(event.data.clone());
            let message = payload
                .get("message")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("subagent auto wake failed");
            return Err(anyhow!(message.to_string()));
        }
        if event_name == "final" {
            return Ok(());
        }
    }
    Err(anyhow!("subagent auto wake finished without final event"))
}

fn build_runtime_item(
    storage: &dyn StorageBackend,
    monitor: Option<&MonitorState>,
    user_id: &str,
    session: ChatSessionRecord,
) -> Result<SubagentRuntimeItem> {
    let run = storage
        .list_session_runs_by_session(user_id, &session.session_id, 1)?
        .into_iter()
        .next();
    let run_status = run
        .as_ref()
        .map(|record| record.status.as_str())
        .unwrap_or_default();
    let runtime_status = monitor
        .and_then(|entry| entry.get_record(&session.session_id))
        .and_then(|value| {
            value
                .get("status")
                .and_then(Value::as_str)
                .map(|status| status.trim().to_ascii_lowercase())
        })
        .filter(|value| !value.is_empty());
    let session_status = normalize_session_status(&session.status);
    let status = resolve_effective_status(run_status, runtime_status.as_deref(), &session_status);
    let terminal = is_terminal_status(&status);
    let failed = is_failed_status(&status);
    let updated_time = run
        .as_ref()
        .map(|record| record.updated_time)
        .unwrap_or(session.updated_at);
    let user_message = run
        .as_ref()
        .and_then(|record| record.metadata.as_ref())
        .and_then(|meta| meta.get("user_message_preview"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|text| truncate_text(text, AUTO_WAKE_OBSERVATION_MAX_CHARS));
    let assistant_message = run
        .as_ref()
        .and_then(|record| record.result.as_deref())
        .map(|text| truncate_text(text, AUTO_WAKE_OBSERVATION_MAX_CHARS));
    let error_message = run
        .as_ref()
        .and_then(|record| record.error.as_deref())
        .map(|text| truncate_text(text, AUTO_WAKE_OBSERVATION_MAX_CHARS));
    let summary = if status == "success" {
        assistant_message.clone()
    } else if failed {
        error_message.clone()
    } else {
        assistant_message.clone().or_else(|| user_message.clone())
    };
    Ok(SubagentRuntimeItem {
        session,
        run,
        status,
        terminal,
        failed,
        summary,
        user_message,
        assistant_message,
        error_message,
        updated_time,
    })
}

fn runtime_item_payload(item: SubagentRuntimeItem) -> Value {
    let run = item.run.clone();
    let metadata = run.as_ref().and_then(|record| record.metadata.clone());
    let (parent_turn_ref, parent_user_round, parent_model_round) =
        parent_turn_payload(metadata.as_ref(), item.session.parent_message_id.as_deref());
    let mut payload = json!({
        "session_id": item.session.session_id,
        "parent_session_id": item.session.parent_session_id,
        "run_id": run.as_ref().map(|record| record.run_id.clone()),
        "dispatch_id": run.as_ref().and_then(|record| record.dispatch_id.clone()),
        "run_kind": run.as_ref().and_then(|record| record.run_kind.clone()),
        "requested_by": run.as_ref().and_then(|record| record.requested_by.clone()),
        "agent_id": item.session.agent_id,
        "model_name": run.as_ref().and_then(|record| record.model_name.clone()),
        "title": item.session.title,
        "label": item.session.spawn_label,
        "spawn_label": item.session.spawn_label,
        "spawned_by": item.session.spawned_by,
        "status": item.status,
        "terminal": item.terminal,
        "failed": item.failed,
        "summary": item.summary,
        "user_message": item.user_message,
        "assistant_message": item.assistant_message,
        "error_message": item.error_message,
        "updated_time": item.updated_time,
        "queued_time": run.as_ref().map(|record| record.queued_time),
        "started_time": run.as_ref().map(|record| record.started_time),
        "finished_time": run.as_ref().map(|record| record.finished_time),
        "elapsed_s": run.as_ref().map(|record| record.elapsed_s),
        "result": run.as_ref().and_then(|record| record.result.clone()),
        "error": run.as_ref().and_then(|record| record.error.clone()),
        "parent_user_round": parent_user_round,
        "parent_model_round": parent_model_round,
        "parent_turn_ref": parent_turn_ref,
        "can_terminate": !item.terminal,
        "agent_state": {
            "status": collab_agent_status(&item.status),
            "message": item.summary,
        }
    });
    let Some(object) = payload.as_object_mut() else {
        return payload;
    };
    object.insert(
        "metadata".to_string(),
        metadata.clone().unwrap_or(Value::Null),
    );
    object.insert(
        "controller_session_id".to_string(),
        run_metadata_field(metadata.as_ref(), "controller_session_id"),
    );
    object.insert(
        "depth".to_string(),
        run_metadata_field(metadata.as_ref(), "depth"),
    );
    object.insert(
        "role".to_string(),
        run_metadata_field(metadata.as_ref(), "role"),
    );
    object.insert(
        "control_scope".to_string(),
        run_metadata_field(metadata.as_ref(), "control_scope"),
    );
    object.insert(
        "spawn_mode".to_string(),
        run_metadata_field(metadata.as_ref(), "spawn_mode"),
    );
    object.insert(
        "strategy".to_string(),
        run_metadata_field(metadata.as_ref(), "strategy"),
    );
    object.insert(
        "completion_mode".to_string(),
        run_metadata_field(metadata.as_ref(), "completion_mode"),
    );
    object.insert(
        "remaining_action".to_string(),
        run_metadata_field(metadata.as_ref(), "remaining_action"),
    );
    object.insert(
        "dispatch_label".to_string(),
        run_metadata_field(metadata.as_ref(), "dispatch_label"),
    );
    object.insert(
        "dispatch_index".to_string(),
        run_metadata_field(metadata.as_ref(), "dispatch_index"),
    );
    object.insert(
        "dispatch_size".to_string(),
        run_metadata_field(metadata.as_ref(), "dispatch_size"),
    );
    object.insert(
        "cleanup".to_string(),
        run_metadata_field(metadata.as_ref(), "cleanup"),
    );
    object.insert(
        "run_timeout_seconds".to_string(),
        run_metadata_field(metadata.as_ref(), "run_timeout_seconds"),
    );
    payload
}

fn list_dispatch_runtime_items(
    storage: &dyn StorageBackend,
    monitor: Option<&MonitorState>,
    user_id: &str,
    parent_session_id: &str,
    dispatch_id: &str,
) -> Result<Vec<SubagentRuntimeItem>> {
    let runs = storage.list_session_runs_by_dispatch(user_id, dispatch_id, DEFAULT_LIST_LIMIT)?;
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for run in runs {
        if run.parent_session_id.as_deref() != Some(parent_session_id) {
            continue;
        }
        if !seen.insert(run.session_id.clone()) {
            continue;
        }
        let Some(session) = storage.get_chat_session(user_id, &run.session_id)? else {
            continue;
        };
        items.push(build_runtime_item(storage, monitor, user_id, session)?);
    }
    items.sort_by(|left, right| {
        right
            .updated_time
            .partial_cmp(&left.updated_time)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(items)
}

fn build_dispatch_finish_payload(
    dispatch: &ParentDispatchConfig,
    items: &[SubagentRuntimeItem],
    completion_mode: CompletionMode,
    progress: CompletionProgress,
) -> Value {
    let payload_items = items
        .iter()
        .cloned()
        .map(runtime_item_payload)
        .collect::<Vec<_>>();
    let selected_items = select_completion_items(completion_mode, items);
    let success_total = items.iter().filter(|item| item.status == "success").count() as i64;
    let failed_total = items.iter().filter(|item| item.failed).count() as i64;
    let done_total = items.iter().filter(|item| item.terminal).count() as i64;
    let status = summarize_completion_status(completion_mode, items, progress);
    let mut payload = json!({
        "status": status,
        "dispatch_id": dispatch.dispatch_id.clone(),
        "strategy": dispatch.strategy.clone(),
        "completion_mode": completion_mode.as_str(),
        "completion_reached": progress.completion_reached,
        "completed_reason": progress.completed_reason,
        "all_finished": progress.all_finished,
        "total": items.len() as i64,
        "done_total": done_total,
        "success_total": success_total,
        "failed_total": failed_total,
        "selected_items": selected_items,
        "items": payload_items,
        "label": dispatch.label.clone(),
        "parent_user_round": dispatch.parent_user_round,
        "parent_model_round": dispatch.parent_model_round,
    });
    if let Some(object) = payload.as_object_mut() {
        if let Some(winner) = select_winner_item(items) {
            object.insert("winner_item".to_string(), winner.clone());
            object.insert("selected_item".to_string(), winner);
        }
        if let Some(summary) = build_dispatch_summary(items) {
            object.insert("summary".to_string(), Value::String(summary));
        }
    }
    payload
}

fn build_auto_wake_payload(payload: Value, dispatch: &ParentDispatchConfig) -> Value {
    let mut wake_payload = json!({
        "kind": "subagent_auto_wake",
        "instruction": "Background subagent work has produced new observations. Continue the parent task using the latest subagent state.",
        "dispatch": payload,
    });
    if let Some(object) = wake_payload.as_object_mut() {
        object.insert(
            "parent_turn_ref".to_string(),
            Value::String(
                dispatch
                    .parent_turn_ref
                    .clone()
                    .unwrap_or_else(|| "__none__".to_string()),
            ),
        );
        if let Some(label) = dispatch.label.clone() {
            object.insert("label".to_string(), Value::String(label));
        }
    }
    wake_payload
}

fn parse_completion_mode(value: Option<&str>) -> CompletionMode {
    match value.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "any" | "first" | "first_terminal" => CompletionMode::Any,
        "first_success" | "success" => CompletionMode::FirstSuccess,
        _ => CompletionMode::All,
    }
}

fn parse_remaining_action(value: Option<&str>) -> RemainingAction {
    match value.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "interrupt" | "cancel" | "stop" => RemainingAction::Interrupt,
        "close" | "shutdown" | "terminate" => RemainingAction::Close,
        _ => RemainingAction::Keep,
    }
}

fn evaluate_completion(mode: CompletionMode, items: &[SubagentRuntimeItem]) -> CompletionProgress {
    let all_finished = items.iter().all(|item| item.terminal);
    let has_terminal = items.iter().any(|item| item.terminal);
    let has_success = items.iter().any(|item| item.status == "success");
    match mode {
        CompletionMode::All => CompletionProgress {
            completion_reached: all_finished,
            all_finished,
            completed_reason: if all_finished {
                "all_finished"
            } else {
                "pending"
            },
        },
        CompletionMode::Any => CompletionProgress {
            completion_reached: has_terminal,
            all_finished,
            completed_reason: if has_terminal {
                "first_terminal"
            } else {
                "pending"
            },
        },
        CompletionMode::FirstSuccess => CompletionProgress {
            completion_reached: has_success || all_finished,
            all_finished,
            completed_reason: if has_success {
                "first_success"
            } else if all_finished {
                "all_finished_without_success"
            } else {
                "pending"
            },
        },
    }
}

fn select_completion_items(mode: CompletionMode, items: &[SubagentRuntimeItem]) -> Vec<Value> {
    match mode {
        CompletionMode::All | CompletionMode::Any => items
            .iter()
            .filter(|item| item.terminal)
            .cloned()
            .map(runtime_item_payload)
            .collect(),
        CompletionMode::FirstSuccess => {
            let winners = items
                .iter()
                .filter(|item| item.status == "success")
                .cloned()
                .map(runtime_item_payload)
                .collect::<Vec<_>>();
            if winners.is_empty() {
                items
                    .iter()
                    .filter(|item| item.terminal)
                    .cloned()
                    .map(runtime_item_payload)
                    .collect()
            } else {
                winners
            }
        }
    }
}

fn summarize_completion_status(
    mode: CompletionMode,
    items: &[SubagentRuntimeItem],
    progress: CompletionProgress,
) -> &'static str {
    if items.is_empty() {
        return "empty";
    }
    match mode {
        CompletionMode::All => {
            if !progress.all_finished {
                "running"
            } else if items.iter().all(|item| !item.failed) {
                "ok"
            } else {
                "partial"
            }
        }
        CompletionMode::Any => {
            if !progress.completion_reached {
                "running"
            } else if items.iter().any(|item| item.status == "success") {
                "ok"
            } else {
                "partial"
            }
        }
        CompletionMode::FirstSuccess => {
            if items.iter().any(|item| item.status == "success") {
                "ok"
            } else if progress.all_finished {
                "partial"
            } else {
                "running"
            }
        }
    }
}

fn apply_remaining_action(
    storage: &dyn StorageBackend,
    monitor: Option<&MonitorState>,
    user_id: &str,
    action: RemainingAction,
    items: &[SubagentRuntimeItem],
    completed_reason: &str,
) -> Vec<Value> {
    if !matches!(completed_reason, "first_success" | "first_terminal") {
        return Vec::new();
    }
    let mut settled = Vec::new();
    for item in items {
        if item.terminal {
            continue;
        }
        let session_id = item.session.session_id.as_str();
        match action {
            RemainingAction::Keep => {}
            RemainingAction::Interrupt => {
                let changed = monitor.is_some_and(|entry| entry.cancel(session_id));
                settled.push(json!({
                    "session_id": session_id,
                    "run_id": item.run.as_ref().map(|record| record.run_id.clone()),
                    "dispatch_id": item.run.as_ref().and_then(|record| record.dispatch_id.clone()),
                    "status": if changed { "cancelling" } else { "unchanged" },
                    "updated": changed,
                    "action": "interrupt",
                }));
            }
            RemainingAction::Close => {
                let changed = if let Some(mut record) =
                    storage.get_chat_session(user_id, session_id).ok().flatten()
                {
                    let changed = record.status.trim() != "closed";
                    if changed {
                        record.status = "closed".to_string();
                        record.updated_at = now_ts();
                        let _ = storage.upsert_chat_session(&record);
                    }
                    changed
                } else {
                    false
                };
                if let Some(entry) = monitor {
                    let _ = entry.cancel(session_id);
                }
                settled.push(json!({
                    "session_id": session_id,
                    "run_id": item.run.as_ref().map(|record| record.run_id.clone()),
                    "dispatch_id": item.run.as_ref().and_then(|record| record.dispatch_id.clone()),
                    "status": if changed { "closed" } else { "unchanged" },
                    "updated": changed,
                    "action": "close",
                }));
            }
        }
    }
    settled
}

fn select_winner_item(items: &[SubagentRuntimeItem]) -> Option<Value> {
    items
        .iter()
        .find(|item| item.status == "success")
        .cloned()
        .map(runtime_item_payload)
}

fn build_dispatch_summary(items: &[SubagentRuntimeItem]) -> Option<String> {
    let winner = items.iter().find(|item| item.status == "success");
    if let Some(item) = winner {
        let label = item
            .session
            .spawn_label
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(item.session.title.as_str());
        let summary = item.summary.as_deref().unwrap_or("completed");
        return Some(format!(
            "{label}: {}",
            truncate_text(summary, AUTO_WAKE_OBSERVATION_MAX_CHARS)
        ));
    }
    let lines = items
        .iter()
        .filter_map(|item| {
            let label = item
                .session
                .spawn_label
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(item.session.title.as_str());
            let summary = item.summary.as_deref()?;
            Some(format!(
                "{label}: {}",
                truncate_text(summary, AUTO_WAKE_OBSERVATION_MAX_CHARS)
            ))
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn normalize_session_status(status: &str) -> String {
    let normalized = status.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        "active".to_string()
    } else {
        normalized
    }
}

fn resolve_effective_status(
    run_status: &str,
    runtime_status: Option<&str>,
    session_status: &str,
) -> String {
    let run_status = run_status.trim().to_ascii_lowercase();
    if is_terminal_status(&run_status) {
        return run_status;
    }
    if session_status == "closed" {
        return "closed".to_string();
    }
    if let Some(runtime_status) = runtime_status {
        let runtime_status = runtime_status.trim().to_ascii_lowercase();
        if !runtime_status.is_empty() {
            return runtime_status;
        }
    }
    if run_status.is_empty() {
        if session_status == "active" {
            "idle".to_string()
        } else {
            session_status.to_string()
        }
    } else {
        run_status
    }
}

fn is_terminal_status(status: &str) -> bool {
    matches!(
        status,
        "success" | "error" | "timeout" | "cancelled" | "failed" | "closed" | "idle" | "not_found"
    )
}

fn is_failed_status(status: &str) -> bool {
    matches!(
        status,
        "error" | "timeout" | "cancelled" | "failed" | "closed" | "not_found"
    )
}

fn collab_agent_status(status: &str) -> &'static str {
    match status.trim().to_ascii_lowercase().as_str() {
        "queued" | "accepted" | "active" => "pending_init",
        "running" | "waiting" => "running",
        "cancelling" | "cancelled" => "interrupted",
        "success" | "idle" => "completed",
        "error" | "timeout" | "failed" => "errored",
        "closed" => "shutdown",
        "not_found" => "not_found",
        _ => "running",
    }
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let cleaned = text.trim();
    if cleaned.chars().count() <= max_chars {
        return cleaned.to_string();
    }
    let mut output = cleaned.chars().take(max_chars).collect::<String>();
    output.push_str("...");
    output
}

fn now_ts() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

impl CompletionMode {
    fn as_str(self) -> &'static str {
        match self {
            CompletionMode::All => "all",
            CompletionMode::Any => "any",
            CompletionMode::FirstSuccess => "first_success",
        }
    }
}

impl RemainingAction {
    fn as_str(self) -> &'static str {
        match self {
            RemainingAction::Keep => "keep",
            RemainingAction::Interrupt => "interrupt",
            RemainingAction::Close => "close",
        }
    }
}

fn dispatch_guard_key(
    parent_session_id: &str,
    dispatch_id: Option<&str>,
    suffix: Option<&str>,
) -> String {
    let dispatch_id = dispatch_id.unwrap_or("__single__");
    let suffix = suffix.unwrap_or("__default__");
    format!("{parent_session_id}::{dispatch_id}::{suffix}")
}

fn dispatch_once_registry() -> &'static Mutex<HashSet<String>> {
    static REGISTRY: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashSet::new()))
}

fn wake_once_registry() -> &'static Mutex<HashSet<String>> {
    static REGISTRY: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashSet::new()))
}

fn mark_dispatch_once(key: &str) -> bool {
    let registry = dispatch_once_registry();
    let mut guard = registry.lock().expect("dispatch registry poisoned");
    guard.insert(key.to_string())
}

fn mark_wake_once(key: &str) -> bool {
    let registry = wake_once_registry();
    let mut guard = registry.lock().expect("wake registry poisoned");
    guard.insert(key.to_string())
}

fn unmark_wake_once(key: &str) {
    let registry = wake_once_registry();
    let mut guard = registry.lock().expect("wake registry poisoned");
    guard.remove(key);
}
