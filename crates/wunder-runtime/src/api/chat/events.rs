use super::{error_response, format_ts, is_session_runtime_active, is_session_stream_active};
use crate::api::user_context::resolve_user;
use crate::core::blocking;
use crate::i18n;
use crate::orchestrator_constants::STREAM_EVENT_FETCH_LIMIT;
use crate::state::AppState;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::{routing::get, Json, Router};
use chrono::{DateTime, Local};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

const SESSION_EVENTS_MAX_LIMIT: i64 = 500;

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/chat/sessions/{session_id}/events",
            get(get_session_events),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/command-sessions",
            get(list_session_command_sessions),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/command-sessions/{command_session_id}",
            get(get_session_command_session),
        )
}

#[derive(Debug, Deserialize)]
struct SessionEventsQuery {
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    workflow_only: bool,
    #[serde(default)]
    from_user_round: Option<i64>,
    #[serde(default)]
    to_user_round: Option<i64>,
}

async fn get_session_events(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<SessionEventsQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let _record = state
        .user_store
        .get_chat_session(&resolved.user.user_id, &session_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))?;
    let requested_limit = normalize_session_events_limit(query.limit);
    let (stream_events, rounds) = if query.workflow_only {
        (
            Vec::new(),
            load_session_workflow_rounds(
                &state,
                &session_id,
                query.from_user_round,
                query.to_user_round,
            )
            .await,
        )
    } else {
        let stream_events = load_session_stream_events(&state, &session_id, requested_limit).await;
        let rounds = if stream_events.is_empty() {
            load_session_event_rounds(&state, &session_id).await
        } else {
            collect_session_event_rounds(&json!({ "events": stream_events.clone() }))
        };
        (stream_events, rounds)
    };
    let command_sessions = state
        .control
        .command_sessions
        .list_session_snapshots(&resolved.user.user_id, &session_id);
    let monitor_status = state.monitor.get_record(&session_id).and_then(|record| {
        record
            .get("status")
            .and_then(Value::as_str)
            .map(ToString::to_string)
    });
    let goal =
        crate::services::goal::get_goal(state.storage.clone(), &resolved.user.user_id, &session_id)
            .await
            .ok()
            .flatten();
    let runtime = state
        .kernel
        .orchestrator
        .get_tool_session_runtime_snapshot(&session_id);
    let queued = super::has_active_queue_task(&state.user_store, &session_id);
    let running = monitor_status
        .as_deref()
        .map(is_session_stream_active)
        .unwrap_or(false)
        || is_session_runtime_active(runtime.as_ref());
    let runtime_payload = runtime.or_else(|| {
        queued.then(|| {
            json!({
                "thread_status": "queued",
                "status": "queued",
                "loaded": true,
                "active_turn_id": null
            })
        })
    });
    let last_event_id = {
        let storage = state.storage.clone();
        let session_id = session_id.clone();
        blocking::run_db("api.chat.events.tail", move || {
            storage.get_max_stream_event_id(&session_id)
        })
        .await
        .unwrap_or(0)
    };
    Ok(Json(json!({
        "data": {
            "id": session_id,
            "events": stream_events,
            "rounds": rounds,
            "limit": requested_limit,
            "events_limited": !query.workflow_only && requested_limit > 0,
            "workflow_only": query.workflow_only,
            "running": running,
            "queued": queued,
            "last_event_id": last_event_id,
            "goal": goal.as_ref().map(crate::services::goal::goal_payload),
            "runtime": runtime_payload,
            "command_sessions": command_sessions
        }
    })))
}

async fn list_session_command_sessions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let _record = state
        .user_store
        .get_chat_session(&resolved.user.user_id, &session_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))?;
    let items = state
        .control
        .command_sessions
        .list_session_snapshots(&resolved.user.user_id, &session_id);
    Ok(Json(json!({
        "data": {
            "session_id": session_id,
            "items": items
        }
    })))
}

async fn get_session_command_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath((session_id, command_session_id)): AxumPath<(String, String)>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = session_id.trim().to_string();
    let command_session_id = command_session_id.trim().to_string();
    if session_id.is_empty() || command_session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let _record = state
        .user_store
        .get_chat_session(&resolved.user.user_id, &session_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))?;
    let snapshot = state
        .control
        .command_sessions
        .snapshot_for_scope(&resolved.user.user_id, &session_id, &command_session_id)
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.content_not_found")))?;
    Ok(Json(json!({
        "data": {
            "session_id": session_id,
            "item": snapshot
        }
    })))
}

async fn load_session_event_rounds(state: &Arc<AppState>, session_id: &str) -> Vec<Value> {
    let stream_events = load_session_stream_events(state, session_id, 0).await;
    if !stream_events.is_empty() {
        return collect_session_event_rounds(&json!({ "events": stream_events }));
    }
    state
        .monitor
        .get_record(session_id)
        .map(|record| collect_session_event_rounds(&record))
        .unwrap_or_default()
}

async fn load_session_stream_events(
    state: &Arc<AppState>,
    session_id: &str,
    limit: i64,
) -> Vec<Value> {
    let cleaned_session_id = session_id.trim().to_string();
    if cleaned_session_id.is_empty() {
        return Vec::new();
    }
    let workspace = state.workspace.clone();
    let normalized_limit = normalize_session_events_limit(Some(limit));
    blocking::run_fs("api.chat.events.load_stream", move || {
        let records = if normalized_limit <= 0 {
            let mut after_event_id = 0;
            let mut records = Vec::new();
            let batch_limit = STREAM_EVENT_FETCH_LIMIT.max(1);
            loop {
                let batch =
                    workspace.load_stream_events(&cleaned_session_id, after_event_id, batch_limit);
                if batch.is_empty() {
                    break;
                }
                let batch_len = batch.len();
                let mut last_event_id = after_event_id;
                for record in &batch {
                    if let Some(event_id) = record.get("event_id").and_then(Value::as_i64) {
                        last_event_id = last_event_id.max(event_id);
                    }
                }
                records.extend(batch);
                if last_event_id <= after_event_id {
                    break;
                }
                after_event_id = last_event_id;
                if batch_len < batch_limit as usize {
                    break;
                }
            }
            records
        } else {
            workspace.load_recent_stream_events(&cleaned_session_id, normalized_limit)
        };
        Ok(records)
    })
    .await
    .unwrap_or_default()
}

fn normalize_session_events_limit(raw: Option<i64>) -> i64 {
    let value = raw.unwrap_or(0);
    if value <= 0 {
        0
    } else {
        value.min(SESSION_EVENTS_MAX_LIMIT)
    }
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

fn unwrap_session_event_data(value: &Value) -> Value {
    let Some(map) = value.as_object() else {
        return value.clone();
    };
    let Some(inner) = map.get("data") else {
        return value.clone();
    };
    if map
        .get("session_id")
        .and_then(Value::as_str)
        .is_some_and(|session_id| !session_id.trim().is_empty())
        && map
            .get("timestamp")
            .and_then(Value::as_str)
            .is_some_and(|timestamp| !timestamp.trim().is_empty())
    {
        return inner.clone();
    }
    value.clone()
}

fn extract_session_event_round(data: &Value) -> Option<i64> {
    data.get("user_round")
        .and_then(Value::as_i64)
        .or_else(|| {
            data.get("user_round")
                .and_then(Value::as_str)
                .and_then(|value| value.trim().parse::<i64>().ok())
        })
        .or_else(|| data.get("round").and_then(Value::as_i64))
        .or_else(|| {
            data.get("round")
                .and_then(Value::as_str)
                .and_then(|value| value.trim().parse::<i64>().ok())
        })
}

fn extract_session_event_type(event: &Value) -> &str {
    event
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| event.get("event").and_then(Value::as_str))
        .unwrap_or("")
}

fn format_session_event_timestamp(event: &Value) -> String {
    if let Some(timestamp) = event.get("timestamp").and_then(Value::as_f64) {
        if timestamp > 0.0 {
            return format_ts(timestamp);
        }
    }
    if let Some(timestamp) = event.get("timestamp").and_then(Value::as_str) {
        return format_ts_text(timestamp);
    }
    String::new()
}

fn collect_session_event_rounds(record: &Value) -> Vec<Value> {
    let Some(events) = record.get("events").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut order = Vec::new();
    let mut grouped: HashMap<i64, Vec<Value>> = HashMap::new();
    let mut current_round: Option<i64> = None;
    let mut has_round_start = false;
    let register_round = |round: i64,
                          order: &mut Vec<i64>,
                          grouped: &mut HashMap<i64, Vec<Value>>,
                          current_round: &mut Option<i64>| {
        if round <= 0 {
            return;
        }
        grouped.entry(round).or_insert_with(|| {
            order.push(round);
            Vec::new()
        });
        *current_round = Some(round);
    };
    for event in events {
        let event_type = extract_session_event_type(event);
        let data = unwrap_session_event_data(&event.get("data").cloned().unwrap_or(Value::Null));
        let data_round = extract_session_event_round(&data);
        if event_type == "round_start" {
            let round = data_round
                .or_else(|| current_round.map(|value| value + 1))
                .unwrap_or(1);
            register_round(round, &mut order, &mut grouped, &mut current_round);
            has_round_start = true;
            continue;
        }
        if current_round.is_none() {
            if let Some(round) = data_round {
                register_round(round, &mut order, &mut grouped, &mut current_round);
            } else if is_workflow_event(event_type) {
                register_round(1, &mut order, &mut grouped, &mut current_round);
            }
        } else if !has_round_start {
            if let Some(round) = data_round {
                register_round(round, &mut order, &mut grouped, &mut current_round);
            }
        }
        let Some(round) = current_round else {
            continue;
        };
        if !is_workflow_event(event_type) {
            continue;
        }
        let entry = json!({
            "event": event_type,
            "data": data,
            "timestamp": format_session_event_timestamp(event),
            "event_id": event.get("event_id").cloned().unwrap_or(Value::Null),
            "event_seq": event
                .get("event_seq")
                .cloned()
                .or_else(|| event.get("event_id").cloned())
                .unwrap_or(Value::Null),
        });
        let round_events = grouped.entry(round).or_default();
        if let Some(previous) = round_events.last_mut() {
            if should_merge_round_event(previous, &entry) {
                if round_event_detail_score(&entry) > round_event_detail_score(previous) {
                    *previous = entry;
                }
                continue;
            }
        }
        round_events.push(entry);
    }
    order
        .into_iter()
        .filter_map(|round| {
            let events = grouped.remove(&round).unwrap_or_default();
            if events.is_empty() {
                None
            } else {
                Some(json!({ "user_round": round, "events": events }))
            }
        })
        .collect()
}

async fn load_session_workflow_rounds(
    state: &Arc<AppState>,
    session_id: &str,
    from_user_round: Option<i64>,
    to_user_round: Option<i64>,
) -> Vec<Value> {
    let Some((from_user_round, to_user_round)) =
        normalize_workflow_round_range(from_user_round, to_user_round)
    else {
        return Vec::new();
    };
    let storage = state.storage.clone();
    let session_id = session_id.trim().to_string();
    blocking::run_db("api.chat.events.load_workflow", move || {
        storage.load_session_workflow_events(&session_id, from_user_round, to_user_round)
    })
    .await
    .map(|events| collect_session_event_rounds(&json!({ "events": events })))
    .unwrap_or_default()
}

fn normalize_workflow_round_range(
    from_user_round: Option<i64>,
    to_user_round: Option<i64>,
) -> Option<(i64, i64)> {
    let from = from_user_round.filter(|value| *value > 0)?;
    let to = to_user_round.filter(|value| *value >= from)?;
    Some((from, to))
}

fn should_merge_round_event(previous: &Value, current: &Value) -> bool {
    let previous_type = previous.get("event").and_then(Value::as_str).unwrap_or("");
    let current_type = current.get("event").and_then(Value::as_str).unwrap_or("");
    if previous_type != "error" || current_type != "error" {
        return false;
    }

    let previous_timestamp = previous
        .get("timestamp")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let current_timestamp = current
        .get("timestamp")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if previous_timestamp.is_empty()
        || current_timestamp.is_empty()
        || previous_timestamp != current_timestamp
    {
        return false;
    }

    let previous_data = previous.get("data").and_then(Value::as_object);
    let current_data = current.get("data").and_then(Value::as_object);
    let previous_trace = previous_data
        .and_then(|data| data.get("trace_id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let current_trace = current_data
        .and_then(|data| data.get("trace_id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if previous_trace.is_empty() || current_trace.is_empty() || previous_trace != current_trace {
        return false;
    }

    let previous_text = extract_round_event_error_text(previous_data);
    let current_text = extract_round_event_error_text(current_data);
    if previous_text.is_empty() || current_text.is_empty() {
        return true;
    }
    previous_text == current_text
        || previous_text.contains(&current_text)
        || current_text.contains(&previous_text)
}

fn extract_round_event_error_text(data: Option<&serde_json::Map<String, Value>>) -> String {
    data.and_then(|data| {
        data.get("message")
            .or_else(|| data.get("summary"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    })
    .unwrap_or_default()
}

fn round_event_detail_score(entry: &Value) -> usize {
    let Some(data) = entry.get("data").and_then(Value::as_object) else {
        return 0;
    };
    let mut score = 0usize;
    if data
        .get("message")
        .and_then(Value::as_str)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        score += 4;
    }
    if data
        .get("code")
        .and_then(Value::as_str)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        score += 2;
    }
    if data
        .get("summary")
        .and_then(Value::as_str)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        score += 1;
    }
    score
}

fn is_workflow_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "progress"
            | "llm_request"
            | "llm_response"
            | "knowledge_request"
            | "compaction"
            | "tool_call"
            | "tool_result"
            | "approval_request"
            | "approval_result"
            | "approval_resolved"
            | "plan_update"
            | "question_panel"
            | "thread_control"
            | "llm_output_delta"
            | "llm_output"
            | "context_usage"
            | "token_balance"
            | "quota_usage"
            | "round_usage"
            | "command_session_start"
            | "command_session_status"
            | "command_session_exit"
            | "command_session_summary"
            | "team_start"
            | "team_task_dispatch"
            | "team_task_update"
            | "team_task_result"
            | "team_merge"
            | "team_finish"
            | "team_error"
            | "subagent_status"
            | "subagent_interrupt"
            | "subagent_close"
            | "subagent_resume"
            | "subagent_dispatch_start"
            | "subagent_dispatch_item_update"
            | "subagent_dispatch_finish"
            | "subagent_announce"
            | "queue_enter"
            | "queue_start"
            | "queue_finish"
            | "queue_fail"
            | "final"
            | "turn_terminal"
            | "thread_status"
            | "thread_closed"
            | "error"
    )
}

#[cfg(test)]
mod tests {
    use super::{
        collect_session_event_rounds, normalize_workflow_round_range, should_merge_round_event,
    };
    use serde_json::{json, Value};

    #[test]
    fn merges_duplicate_round_error_pair_by_trace() {
        let previous = json!({
            "event": "error",
            "timestamp": "2026-03-12T15:40:41.383+08:00",
            "data": {
                "trace_id": "trace_1",
                "code": "INTERNAL_ERROR",
                "message": "模型调用失败: prompt too long"
            }
        });
        let current = json!({
            "event": "error",
            "timestamp": "2026-03-12T15:40:41.383+08:00",
            "data": {
                "trace_id": "trace_1",
                "summary": "模型调用失败: prompt too long"
            }
        });
        assert!(should_merge_round_event(&previous, &current));
    }

    #[test]
    fn collect_session_event_rounds_keeps_richer_error_once() {
        let record = json!({
            "events": [
                {
                    "type": "progress",
                    "timestamp": 1.0,
                    "data": { "user_round": 1, "stage": "start" }
                },
                {
                    "type": "error",
                    "timestamp": 2.0,
                    "data": {
                        "user_round": 1,
                        "trace_id": "trace_1",
                        "code": "INTERNAL_ERROR",
                        "message": "模型调用失败: prompt too long"
                    }
                },
                {
                    "type": "error",
                    "timestamp": 2.0,
                    "data": {
                        "user_round": 1,
                        "trace_id": "trace_1",
                        "summary": "模型调用失败: prompt too long"
                    }
                }
            ]
        });

        let rounds = collect_session_event_rounds(&record);
        assert_eq!(rounds.len(), 1);
        let events = rounds[0]
            .get("events")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let error_events = events
            .iter()
            .filter(|item| item.get("event").and_then(Value::as_str) == Some("error"))
            .collect::<Vec<_>>();
        assert_eq!(error_events.len(), 1);
        assert_eq!(
            error_events[0]
                .get("data")
                .and_then(|value| value.get("message"))
                .and_then(Value::as_str),
            Some("模型调用失败: prompt too long")
        );
    }

    #[test]
    fn collect_session_event_rounds_preserves_compaction_only_round() {
        let record = json!({
            "events": [
                {
                    "type": "compaction",
                    "timestamp": 1.0,
                    "data": {
                        "reason": "history",
                        "status": "done"
                    }
                }
            ]
        });
        let rounds = collect_session_event_rounds(&record);
        assert_eq!(rounds.len(), 1);
        assert_eq!(rounds[0].get("user_round").and_then(Value::as_i64), Some(1));
        let events = rounds[0]
            .get("events")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].get("event").and_then(Value::as_str),
            Some("compaction")
        );
        assert_eq!(
            events[0]
                .get("data")
                .and_then(|value| value.get("status"))
                .and_then(Value::as_str),
            Some("done")
        );
    }

    #[test]
    fn collect_session_event_rounds_supports_stream_event_wrappers() {
        let record = json!({
            "events": [
                {
                    "event": "progress",
                    "timestamp": "2026-04-06T15:47:50+08:00",
                    "data": {
                        "session_id": "sess_demo",
                        "timestamp": "2026-04-06T15:47:50+08:00",
                        "data": {
                            "user_round": 1,
                            "stage": "start"
                        }
                    }
                },
                {
                    "event": "tool_result",
                    "timestamp": "2026-04-06T15:47:56+08:00",
                    "data": {
                        "session_id": "sess_demo",
                        "timestamp": "2026-04-06T15:47:56+08:00",
                        "data": {
                            "user_round": 2,
                            "tool": "knowledge",
                            "ok": true
                        }
                    }
                }
            ]
        });

        let rounds = collect_session_event_rounds(&record);
        assert_eq!(rounds.len(), 2);
        assert_eq!(rounds[0].get("user_round").and_then(Value::as_i64), Some(1));
        assert_eq!(rounds[1].get("user_round").and_then(Value::as_i64), Some(2));

        let first_event = rounds[0]
            .get("events")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .cloned()
            .unwrap_or(Value::Null);
        assert_eq!(
            first_event.get("event").and_then(Value::as_str),
            Some("progress")
        );
        assert_eq!(
            first_event.get("timestamp").and_then(Value::as_str),
            Some("2026-04-06T15:47:50+08:00")
        );
        assert_eq!(
            first_event
                .get("data")
                .and_then(|value| value.get("stage"))
                .and_then(Value::as_str),
            Some("start")
        );

        let second_event = rounds[1]
            .get("events")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .cloned()
            .unwrap_or(Value::Null);
        assert_eq!(
            second_event
                .get("data")
                .and_then(|value| value.get("tool"))
                .and_then(Value::as_str),
            Some("knowledge")
        );
    }

    #[test]
    fn workflow_round_range_requires_an_ordered_positive_window() {
        assert_eq!(
            normalize_workflow_round_range(Some(3), Some(5)),
            Some((3, 5))
        );
        assert_eq!(normalize_workflow_round_range(Some(0), Some(5)), None);
        assert_eq!(normalize_workflow_round_range(Some(5), Some(3)), None);
        assert_eq!(normalize_workflow_round_range(Some(3), None), None);
    }
}
