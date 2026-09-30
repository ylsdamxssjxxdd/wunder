use super::{error_response, format_ts};
use crate::api::user_context::resolve_user;
use crate::core::blocking;
use crate::i18n;
use crate::orchestrator_constants::STREAM_EVENT_FETCH_LIMIT;
use crate::services::chat_runtime_projection::load_chat_session_activity;
use crate::services::chat_transcript::build_chat_transcript;
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
const WORKFLOW_EVENTS_PAGE_MAX_LIMIT: i64 = 100;

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/chat/sessions/{session_id}/events",
            get(get_session_events),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/thread-log/export",
            get(export_thread_log),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/thread-log/turns",
            get(list_thread_turns),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/thread-log/turns/{turn_id}",
            get(get_thread_turn),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/thread-log/changes",
            get(list_thread_changes),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/thread-log/items/{item_id}/content",
            get(get_thread_item_content),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/thread-log/items/{item_id}",
            get(get_thread_item),
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
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    page_size: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ThreadTurnsQuery {
    #[serde(default)]
    before: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}
#[derive(Debug, Deserialize)]
struct ThreadChangesQuery {
    #[serde(default)]
    after: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    item_after: Option<i64>,
    #[serde(default)]
    from_block: Option<i64>,
    #[serde(default)]
    field: Option<String>,
}

async fn require_owned_thread(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    session_id: &str,
) -> Result<String, Response> {
    let resolved = resolve_user(state, headers, None).await?;
    state
        .user_store
        .get_chat_session(&resolved.user.user_id, session_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))?;
    Ok(resolved.user.user_id)
}

async fn list_thread_turns(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<ThreadTurnsQuery>,
) -> Result<Json<Value>, Response> {
    let session_id = session_id.trim().to_string();
    let user_id = require_owned_thread(&state, &headers, &session_id).await?;
    let storage = state.storage.clone();
    let lookup_session = session_id.clone();
    let (turns, (user_round_total, item_total)) =
        blocking::run_db("api.chat.thread_log.turns", move || {
            let turns = storage.list_thread_turns(
                &user_id,
                &lookup_session,
                query.before,
                query.limit.unwrap_or(50).clamp(1, 100),
            )?;
            let counts = storage.get_thread_log_counts(&user_id, &lookup_session, false)?;
            Ok::<_, anyhow::Error>((turns, counts))
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let next_before = turns
        .last()
        .and_then(|turn| turn.get("user_turn_index"))
        .and_then(Value::as_i64);
    Ok(Json(
        json!({"data":{"session_id":session_id,"turns":turns,"user_round_total":user_round_total,"item_total":item_total,"next_before":next_before,"has_more":turns.len() >= query.limit.unwrap_or(50).clamp(1,100) as usize}}),
    ))
}

async fn get_thread_turn(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath((session_id, turn_id)): AxumPath<(String, String)>,
    Query(query): Query<ThreadChangesQuery>,
) -> Result<Json<Value>, Response> {
    let session_id = session_id.trim().to_string();
    let user_id = require_owned_thread(&state, &headers, &session_id).await?;
    let storage = state.storage.clone();
    let lookup_session = session_id.clone();
    let turn = blocking::run_db("api.chat.thread_log.turn", move || {
        storage.get_thread_turn(
            &user_id,
            &lookup_session,
            &turn_id,
            query.item_after.unwrap_or(-1),
            query.limit.unwrap_or(100),
            false,
        )
    })
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.content_not_found")))?;
    Ok(Json(json!({"data":{"session_id":session_id,"turn":turn}})))
}

async fn list_thread_changes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<ThreadChangesQuery>,
) -> Result<Json<Value>, Response> {
    let session_id = session_id.trim().to_string();
    let user_id = require_owned_thread(&state, &headers, &session_id).await?;
    let storage = state.storage.clone();
    let lookup_session = session_id.clone();
    let changes = blocking::run_db("api.chat.thread_log.changes", move || {
        storage.list_thread_changes(
            &user_id,
            &lookup_session,
            query.after.unwrap_or(0),
            query.limit.unwrap_or(100).clamp(1, 500),
        )
    })
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let snapshot_required = changes.iter().any(|change| change.get("change_type").and_then(Value::as_str) == Some("snapshot_required"));
    Ok(Json(json!({"data":{"session_id":session_id,"changes":changes,"frame":if snapshot_required {"thread_snapshot_required"} else {"thread_change"}}})))
}

async fn get_thread_item_content(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath((session_id, item_id)): AxumPath<(String, String)>,
    Query(query): Query<ThreadChangesQuery>,
) -> Result<Json<Value>, Response> {
    let session_id = session_id.trim().to_string();
    let user_id = require_owned_thread(&state, &headers, &session_id).await?;
    let storage = state.storage.clone();
    let lookup = session_id.clone();
    let item_lookup = item_id.clone();
    let field = query.field.clone();
    let field_for_query = field.clone();
    let from_block = query.from_block.or(query.item_after).unwrap_or(0);
    let blocks = blocking::run_db("api.chat.thread_log.item_content", move || {
        storage.list_thread_item_blocks_page(&user_id, &lookup, &item_lookup, field_for_query.as_deref(), from_block, query.limit.unwrap_or(50).clamp(1, 100), false)
    }).await.map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({"data":{"session_id":session_id,"item_id":item_id,"field":field,"blocks":blocks.0,"next_block":blocks.1,"has_more":blocks.2}})))
}

async fn get_thread_item(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath((session_id, item_id)): AxumPath<(String, String)>,
) -> Result<Json<Value>, Response> {
    let session_id = session_id.trim().to_string();
    let user_id = require_owned_thread(&state, &headers, &session_id).await?;
    let storage = state.storage.clone();
    let lookup_session = session_id.clone();
    let lookup_item = item_id.clone();
    let item = blocking::run_db("api.chat.thread_log.item", move || {
        storage.get_thread_item(&user_id, &lookup_session, &lookup_item, false)
    })
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.content_not_found")))?;
    let mut payload = item
        .get("payload")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if let Value::Object(map) = &mut payload {
            for key in ["item_id", "turn_id", "kind", "visibility", "revision", "status"] {
                if let Some(value) = item.get(key) {
                    map.insert(key.to_string(), value.clone());
                }
            }
    }
    let message = build_chat_transcript(&session_id, vec![payload], &HashMap::new())
        .into_iter()
        .next()
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.content_not_found")))?;
    Ok(Json(json!({"data":{"id":session_id,"item_id":item_id,"message":message}})))
}

#[derive(Debug, Clone, Copy)]
struct WorkflowEventsPage {
    offset: i64,
    limit: i64,
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
    let workflow_page = normalize_workflow_events_page(query.offset, query.page_size);
    let (stream_events, rounds, events_has_more, event_total) = if query.workflow_only {
        let (rounds, has_more, total) = load_session_workflow_rounds(
            &state,
            &resolved.user.user_id,
            &session_id,
            query.from_user_round,
            query.to_user_round,
            workflow_page,
        )
        .await;
        (Vec::new(), rounds, has_more, total)
    } else {
        let stream_events = load_session_stream_events(&state, &session_id, requested_limit).await;
        let rounds = if stream_events.is_empty() {
            load_session_event_rounds(&state, &session_id).await
        } else {
            collect_session_event_rounds(&json!({ "events": stream_events.clone() }))
        };
        (stream_events, rounds, false, None)
    };
    let command_sessions = state
        .control
        .command_sessions
        .list_session_snapshots(&resolved.user.user_id, &session_id);
    let monitor_record = state.monitor.get_record(&session_id);
    let goal =
        crate::services::goal::get_goal(state.storage.clone(), &resolved.user.user_id, &session_id)
            .await
            .ok()
            .flatten();
    let activity = load_chat_session_activity(&state, &session_id, monitor_record.as_ref()).await;
    let runtime = activity.runtime;
    let queued = super::has_active_queue_task(&state.user_store, &session_id);
    let running = activity.running;
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
            "event_offset": workflow_page.map(|page| page.offset),
            "event_limit": workflow_page.map(|page| page.limit),
            "event_total": event_total,
            "events_has_more": workflow_page.is_some() && events_has_more,
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
    user_id: &str,
    session_id: &str,
    from_user_round: Option<i64>,
    to_user_round: Option<i64>,
    page: Option<WorkflowEventsPage>,
) -> (Vec<Value>, bool, Option<i64>) {
    let Some((from_user_round, to_user_round)) =
        normalize_workflow_round_range(from_user_round, to_user_round)
    else {
        return (Vec::new(), false, page.map(|_| 0));
    };
    let storage = state.storage.clone();
    let user_id = user_id.to_string();
    let session_id = session_id.trim().to_string();
    blocking::run_db("api.chat.events.load_workflow", move || {
        // This temporary adapter returns a bounded turn page. Seek directly to
        // the requested numeric round; never scan/materialize the whole thread.
        let limit = page.map_or(WORKFLOW_EVENTS_PAGE_MAX_LIMIT, |page| page.limit);
        let offset = page.map_or(0, |page| page.offset);
        let latest = storage.list_thread_turns(&user_id, &session_id, None, 1)?;
        let latest_index = latest.first().and_then(|turn| turn["user_turn_index"].as_i64()).unwrap_or(0);
        let upper = to_user_round.min(latest_index);
        let total = (upper - from_user_round + 1).max(0);
        let before = upper.saturating_add(1).saturating_sub(offset);
        let turns = storage.list_thread_turns(&user_id, &session_id, Some(before), limit + 1)?;
        let mut turns: Vec<_> = turns.into_iter().filter(|turn| {
            turn["user_turn_index"].as_i64().is_some_and(|index| index >= from_user_round)
        }).collect();
        let has_more = turns.len() > limit as usize;
        turns.truncate(limit as usize);
        let rounds = turns.iter().map(|turn| {
            thread_turn_to_workflow_round(&*storage, &user_id, &session_id, turn)
        }).collect::<anyhow::Result<Vec<_>>>()?;
        // These are already round projections, not raw events to regroup.
        Ok((rounds, has_more, Some(total)))
    })
    .await
    .unwrap_or_default()
}

fn thread_turn_to_workflow_round(
    storage: &dyn crate::storage::StorageBackend,
    user_id: &str,
    session_id: &str,
    turn: &Value,
) -> anyhow::Result<Value> {
    let turn_id = turn.get("turn_id").and_then(Value::as_str).unwrap_or_default();
    let detail = storage.get_thread_turn(user_id, session_id, turn_id, -1, 100, false)?
        .unwrap_or_else(|| turn.clone());
    let events = detail.get("items").and_then(Value::as_array).into_iter().flatten().map(|item| json!({
        "event": item.get("kind").cloned().unwrap_or_else(|| json!("item")),
        "data": item.get("payload").cloned().unwrap_or_else(|| json!({})),
        "item_id": item.get("item_id").cloned().unwrap_or(Value::Null),
        "event_seq": item.get("item_index").cloned().unwrap_or(Value::Null),
    })).collect::<Vec<_>>();
    Ok(json!({"user_round": turn.get("user_turn_index").cloned().unwrap_or(Value::Null), "turn_id": turn_id, "status": turn.get("status").cloned().unwrap_or(Value::Null), "events": events}))
}

fn normalize_workflow_events_page(
    offset: Option<i64>,
    page_size: Option<i64>,
) -> Option<WorkflowEventsPage> {
    if offset.is_none() && page_size.is_none() {
        return None;
    }
    Some(WorkflowEventsPage {
        offset: offset.unwrap_or(0).max(0),
        limit: page_size
            .unwrap_or(WORKFLOW_EVENTS_PAGE_MAX_LIMIT)
            .clamp(1, WORKFLOW_EVENTS_PAGE_MAX_LIMIT),
    })
}

fn normalize_workflow_round_range(
    from_user_round: Option<i64>,
    to_user_round: Option<i64>,
) -> Option<(i64, i64)> {
    // An omitted range means the complete durable workflow history. The SQL
    // query still returns only workflow events, so the response stays bounded
    // by event payload rather than chat message count.
    let from = match from_user_round {
        Some(value) if value > 0 => value,
        Some(_) => return None,
        None => 1,
    };
    let to = match to_user_round {
        Some(value) if value >= from => value,
        Some(_) => return None,
        None => i64::MAX,
    };
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
            | "quota_balance"
            | "quota_usage"
            | "model_request_usage"
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
        assert_eq!(
            normalize_workflow_round_range(Some(3), None),
            Some((3, i64::MAX))
        );
        assert_eq!(
            normalize_workflow_round_range(None, None),
            Some((1, i64::MAX))
        );
    }
}

async fn export_thread_log(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Response, Response> {
    let user_id = require_owned_thread(&state, &headers, &session_id).await?;
    Ok(crate::services::thread_log::export_response(
        state.storage.clone(),
        user_id,
        session_id,
        false,
    ))
}
