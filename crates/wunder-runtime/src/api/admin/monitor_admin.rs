use crate::api::admin::{error_response, now_ts, resolve_monitor_session_agent_name};
use crate::config::Config;
use crate::core::runtime_metrics;
use crate::i18n;
use crate::performance::{
    run_sample as run_performance_sample, PerformanceSampleRequest, PerformanceSampleResponse,
};
use crate::state::AppState;
use crate::tools::{
    build_mcp_tool_alias_entries_for_names, builtin_aliases, builtin_tool_specs, resolve_tool_name,
};
use crate::user_store::UserStore;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::{routing::get, routing::post, Json, Router};
use chrono::{Local, TimeZone};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use tracing::{info, warn};

const ADMIN_MONITOR_TIMING_INFO_MS: u128 = 200;
const ADMIN_MONITOR_TIMING_WARN_MS: u128 = 1000;
const ADMIN_MONITOR_DETAIL_EVENT_PAGE_MAX_LIMIT: i64 = 100;

#[cfg(test)]
#[path = "session_catalog_tests.rs"]
mod session_catalog_tests;

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/admin/monitor", get(admin_monitor))
        .route(
            "/wunder/admin/monitor/tool_usage",
            get(admin_monitor_tool_usage),
        )
        .route(
            "/wunder/admin/monitor/logs/cleanup",
            post(admin_monitor_logs_cleanup),
        )
        .route(
            "/wunder/admin/monitor/{session_id}",
            get(admin_monitor_detail).delete(admin_monitor_delete),
        )
        .route(
            "/wunder/admin/monitor/{session_id}/thread-log/export",
            get(export_thread_log),
        )
        .route(
            "/wunder/admin/monitor/{session_id}/thread-log/turns",
            get(admin_thread_turns),
        )
        .route(
            "/wunder/admin/monitor/{session_id}/thread-log/turns/{turn_id}",
            get(admin_thread_turn),
        )
        .route(
            "/wunder/admin/monitor/{session_id}/thread-log/changes",
            get(admin_thread_changes),
        )
        .route(
            "/wunder/admin/monitor/{session_id}/thread-log/items/{item_id}/content",
            get(admin_thread_item_content),
        )
        .route(
            "/wunder/admin/monitor/{session_id}/cancel",
            post(admin_monitor_cancel),
        )
        .route(
            "/wunder/admin/monitor/{session_id}/compaction",
            post(admin_monitor_compaction),
        )
        .route(
            "/wunder/admin/performance/sample",
            post(admin_performance_sample),
        )
        .route("/wunder/admin/runtime_metrics", get(admin_runtime_metrics))
}

async fn admin_monitor(
    State(state): State<Arc<AppState>>,
    Query(query): Query<MonitorQuery>,
) -> Result<Json<Value>, Response> {
    runtime_metrics::record_loop_tick("api.admin.monitor", "request");
    let request_started_at = Instant::now();
    let mut stage_started_at = Instant::now();
    state.monitor.warm_history(true);
    let warm_history_ms = stage_started_at.elapsed().as_millis();
    let active_only = query.active_only.unwrap_or(true);
    stage_started_at = Instant::now();
    let system = state.monitor.get_system_metrics();
    let system_ms = stage_started_at.elapsed().as_millis();
    stage_started_at = Instant::now();
    let sessions = state.monitor.list_sessions(active_only);
    let sessions_ms = stage_started_at.elapsed().as_millis();

    stage_started_at = Instant::now();
    let mut since_time = None;
    let mut until_time = None;
    let mut recent_window_s = None;
    let mut service_now = None;
    let mut start_ts = normalize_ts(query.start_time);
    let mut end_ts = normalize_ts(query.end_time);
    if let (Some(start), Some(end)) = (start_ts, end_ts) {
        if end < start {
            start_ts = Some(end);
            end_ts = Some(start);
        }
    }
    if start_ts.is_some() || end_ts.is_some() {
        since_time = start_ts;
        until_time = end_ts;
        let now = end_ts.unwrap_or_else(now_ts);
        service_now = Some(now);
        if let Some(start) = start_ts {
            recent_window_s = Some((now - start).max(0.0));
        }
    } else if let Some(hours) = query.tool_hours.filter(|value| *value > 0.0) {
        let window = hours * 3600.0;
        recent_window_s = Some(window);
        since_time = Some(now_ts() - window);
    }
    let query_window_ms = stage_started_at.elapsed().as_millis();

    stage_started_at = Instant::now();
    let service = state
        .monitor
        .get_service_metrics(recent_window_s, service_now);
    let service_ms = stage_started_at.elapsed().as_millis();
    stage_started_at = Instant::now();
    let config = state.config_store.get().await;
    let tool_stats = normalize_tool_stats(
        state.workspace.get_tool_usage_stats(since_time, until_time),
        &config,
    );
    let tool_stats_ms = stage_started_at.elapsed().as_millis();
    stage_started_at = Instant::now();
    let sandbox = state.monitor.get_sandbox_metrics(since_time, until_time);
    let sandbox_ms = stage_started_at.elapsed().as_millis();
    stage_started_at = Instant::now();
    let response = Json(json!({
        "system": system,
        "service": service,
        "sandbox": sandbox,
        "sessions": sessions,
        "tool_stats": tool_stats
    }));
    let payload_ms = stage_started_at.elapsed().as_millis();
    let total_ms = request_started_at.elapsed().as_millis();
    log_admin_monitor_timing(
        &query,
        active_only,
        recent_window_s,
        response.0["sessions"]
            .as_array()
            .map(|items| items.len())
            .unwrap_or(0),
        response.0["tool_stats"]
            .as_array()
            .map(|items| items.len())
            .unwrap_or(0),
        warm_history_ms,
        query_window_ms,
        system_ms,
        sessions_ms,
        service_ms,
        tool_stats_ms,
        sandbox_ms,
        payload_ms,
        total_ms,
    );
    Ok(response)
}

async fn admin_runtime_metrics() -> Json<Value> {
    runtime_metrics::record_loop_tick("api.admin.runtime_metrics", "request");
    Json(json!({
        "runtime": runtime_metrics::snapshot(),
    }))
}

#[allow(clippy::too_many_arguments)]
fn log_admin_monitor_timing(
    query: &MonitorQuery,
    active_only: bool,
    recent_window_s: Option<f64>,
    session_count: usize,
    tool_stats_count: usize,
    warm_history_ms: u128,
    query_window_ms: u128,
    system_ms: u128,
    sessions_ms: u128,
    service_ms: u128,
    tool_stats_ms: u128,
    sandbox_ms: u128,
    payload_ms: u128,
    total_ms: u128,
) {
    let max_stage_ms = [
        warm_history_ms,
        query_window_ms,
        system_ms,
        sessions_ms,
        service_ms,
        tool_stats_ms,
        sandbox_ms,
        payload_ms,
    ]
    .into_iter()
    .max()
    .unwrap_or(0);
    if total_ms < ADMIN_MONITOR_TIMING_INFO_MS && max_stage_ms < ADMIN_MONITOR_TIMING_INFO_MS {
        return;
    }
    let has_range = query.start_time.is_some() || query.end_time.is_some();
    let recent_window_s = recent_window_s.unwrap_or_default();
    if total_ms >= ADMIN_MONITOR_TIMING_WARN_MS || max_stage_ms >= ADMIN_MONITOR_TIMING_WARN_MS {
        warn!(
            endpoint = "/wunder/admin/monitor",
            active_only,
            has_range,
            tool_hours = query.tool_hours.unwrap_or_default(),
            recent_window_s,
            session_count,
            tool_stats_count,
            warm_history_ms,
            query_window_ms,
            system_ms,
            sessions_ms,
            service_ms,
            tool_stats_ms,
            sandbox_ms,
            payload_ms,
            max_stage_ms,
            total_ms,
            "admin monitor timing"
        );
    } else {
        info!(
            endpoint = "/wunder/admin/monitor",
            active_only,
            has_range,
            tool_hours = query.tool_hours.unwrap_or_default(),
            recent_window_s,
            session_count,
            tool_stats_count,
            warm_history_ms,
            query_window_ms,
            system_ms,
            sessions_ms,
            service_ms,
            tool_stats_ms,
            sandbox_ms,
            payload_ms,
            max_stage_ms,
            total_ms,
            "admin monitor timing"
        );
    }
}

async fn admin_monitor_tool_usage(
    State(state): State<Arc<AppState>>,
    Query(query): Query<MonitorToolUsageQuery>,
) -> Result<Json<Value>, Response> {
    let cleaned = query.tool.as_deref().unwrap_or("").trim().to_string();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.tool_name_required"),
        ));
    }
    if cleaned.eq_ignore_ascii_case("performance_log") {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.tool_not_found"),
        ));
    }

    let mut since_time = None;
    let mut until_time = None;
    let mut start_ts = normalize_ts(query.start_time);
    let mut end_ts = normalize_ts(query.end_time);
    if let (Some(start), Some(end)) = (start_ts, end_ts) {
        if end < start {
            start_ts = Some(end);
            end_ts = Some(start);
        }
    }
    if start_ts.is_some() || end_ts.is_some() {
        since_time = start_ts;
        until_time = end_ts;
    } else if let Some(hours) = query.tool_hours.filter(|value| *value > 0.0) {
        since_time = Some(now_ts() - hours * 3600.0);
    }

    let config = state.config_store.get().await;
    let display_map = build_builtin_tool_display_map(&config);
    let canonical = resolve_tool_name(&cleaned);
    let builtin_names = builtin_tool_names();
    let mut requested_runtime_name = display_map
        .iter()
        .find_map(|(runtime_name, display_name)| {
            if display_name == &cleaned {
                Some(runtime_name.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| cleaned.clone());
    if requested_runtime_name == cleaned {
        let mcp_alias = build_mcp_tool_alias_entries_for_names([cleaned.as_str()]);
        if let Some(entry) = mcp_alias.first() {
            requested_runtime_name = entry.runtime_name.clone();
        }
    }
    let mut tool_name = cleaned.clone();
    let usage_records = if builtin_names.contains(&canonical) {
        let mut names = vec![canonical.clone()];
        for (alias, target) in builtin_aliases() {
            if target == canonical && !names.contains(&alias) {
                names.push(alias);
            }
        }
        let mut combined = Vec::new();
        for name in names {
            combined.extend(
                state
                    .workspace
                    .get_tool_session_usage(&name, since_time, until_time),
            );
        }
        tool_name = canonical.clone();
        merge_tool_session_usage(combined)
    } else {
        state
            .workspace
            .get_tool_session_usage(&requested_runtime_name, since_time, until_time)
    };

    let display_name = display_map
        .get(&tool_name)
        .cloned()
        .or_else(|| display_map.get(&canonical).cloned())
        .unwrap_or_else(|| cleaned.clone());
    let mut session_map = HashMap::new();
    for session in state.monitor.list_sessions(false) {
        if let Some(session_id) = session.get("session_id").and_then(Value::as_str) {
            session_map.insert(session_id.to_string(), session);
        }
    }

    let mut sessions = Vec::new();
    for record in usage_records {
        let session_id = record
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if session_id.is_empty() {
            continue;
        }
        let user_id = record
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let tool_calls = record
            .get("tool_calls")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let last_time = record
            .get("last_time")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let last_time_text = format_ts(last_time);
        let session_info = session_map.get(&session_id);
        let fallback_user = session_info
            .and_then(|value| value.get("user_id").and_then(Value::as_str))
            .unwrap_or("")
            .trim()
            .to_string();
        let final_user = if user_id.is_empty() {
            fallback_user
        } else {
            user_id
        };
        let question = session_info
            .and_then(|value| value.get("question").and_then(Value::as_str))
            .unwrap_or("")
            .to_string();
        let status = session_info
            .and_then(|value| value.get("status").and_then(Value::as_str))
            .unwrap_or("unknown")
            .to_string();
        let stage = session_info
            .and_then(|value| value.get("stage").and_then(Value::as_str))
            .unwrap_or("")
            .to_string();
        let start_time = session_info
            .and_then(|value| value.get("start_time").cloned())
            .unwrap_or(Value::String(String::new()));
        let updated_time = session_info
            .and_then(|value| value.get("updated_time").cloned())
            .unwrap_or(Value::String(last_time_text.clone()));
        let elapsed_s = session_info
            .and_then(|value| value.get("elapsed_s").and_then(Value::as_f64))
            .unwrap_or(0.0);
        let context_tokens = session_info
            .and_then(|value| value.get("context_tokens").and_then(Value::as_i64))
            .unwrap_or(0);
        let context_tokens_peak = session_info
            .and_then(|value| value.get("context_tokens_peak").and_then(Value::as_i64))
            .unwrap_or(context_tokens);
        let consumed_tokens = session_info
            .and_then(|value| value.get("consumed_tokens").and_then(Value::as_i64))
            .unwrap_or(0);
        let prefill_tokens =
            session_info.and_then(|value| value.get("prefill_tokens").and_then(Value::as_i64));
        let ttft_ms = session_info.and_then(|value| value.get("ttft_ms").and_then(Value::as_u64));
        let prefill_duration_s =
            session_info.and_then(|value| value.get("prefill_duration_s").and_then(Value::as_f64));
        let prefill_speed_tps =
            session_info.and_then(|value| value.get("prefill_speed_tps").and_then(Value::as_f64));
        let prefill_speed_lower_bound = session_info.and_then(|value| {
            value
                .get("prefill_speed_lower_bound")
                .and_then(Value::as_bool)
        });
        let decode_tokens =
            session_info.and_then(|value| value.get("decode_tokens").and_then(Value::as_i64));
        let decode_duration_s =
            session_info.and_then(|value| value.get("decode_duration_s").and_then(Value::as_f64));
        let decode_speed_tps =
            session_info.and_then(|value| value.get("decode_speed_tps").and_then(Value::as_f64));
        sessions.push(json!({
            "session_id": session_id,
            "user_id": final_user,
            "question": question,
            "status": status,
            "stage": stage,
            "start_time": start_time,
            "updated_time": updated_time,
            "elapsed_s": elapsed_s,
            "context_tokens": context_tokens,
            "context_occupancy_tokens": context_tokens,
            "context_tokens_peak": context_tokens_peak,
            "context_occupancy_tokens_peak": context_tokens_peak,
            "consumed_tokens": consumed_tokens,
            "ttft_ms": ttft_ms,
            "prefill_tokens": prefill_tokens,
            "prefill_duration_s": prefill_duration_s,
            "prefill_speed_tps": prefill_speed_tps,
            "prefill_speed_lower_bound": prefill_speed_lower_bound,
            "decode_tokens": decode_tokens,
            "decode_duration_s": decode_duration_s,
            "decode_speed_tps": decode_speed_tps,
            "tool_calls": tool_calls,
            "last_time": last_time_text
        }));
    }

    Ok(Json(json!({
        "tool": display_name,
        "tool_name": tool_name,
        "runtime_name": requested_runtime_name,
        "sessions": sessions
    })))
}

async fn admin_monitor_detail(
    State(state): State<Arc<AppState>>,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<MonitorDetailQuery>,
) -> Result<Json<Value>, Response> {
    let cleaned = session_id.trim().to_string();
    let user_id = monitor_user_id(&state, &cleaned)?;
    let monitor = state.monitor.get_record(&cleaned);
    let chat = state
        .user_store
        .get_chat_session(&user_id, &cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let user_name = state
        .user_store
        .get_user_by_id(&user_id)
        .ok()
        .flatten()
        .map(|user| user.username)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| user_id.clone());
    let mut session = monitor
        .unwrap_or_else(|| json!({"session_id":cleaned,"user_id":user_id,"user_name":user_name}));
    if let Some(map) = session.as_object_mut() {
        map.entry("user_name").or_insert_with(|| json!(user_name));
    }
    if let Some(chat) = chat {
        if let Some(map) = session.as_object_mut() {
            map.entry("session_id").or_insert_with(|| json!(cleaned));
            map.entry("user_id").or_insert_with(|| json!(user_id));
            map.entry("question").or_insert_with(|| json!(chat.title));
            map.entry("agent_id")
                .or_insert_with(|| json!(chat.agent_id));
            map.entry("status").or_insert_with(|| json!("completed"));
        }
    }
    if let Some(map) = session.as_object_mut() {
        let uid = map
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or(&user_id)
            .to_string();
        let aid = map
            .get("agent_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if let Some(name) = resolve_monitor_session_agent_name(&state, &uid, &aid)? {
            map.insert("agent_name".into(), json!(name));
        }
    }
    // `get_record()` retains raw events and counters. Re-apply the canonical
    // derived timing projection so the administrator and user detail views
    // expose the same TTFT, prefill, and decode metrics.
    if let Some(Value::Object(overview)) = state.monitor.get_log_overview(&cleaned) {
        if let Some(map) = session.as_object_mut() {
            for key in [
                "elapsed_s",
                "user_rounds",
                "tool_calls",
                "model_request_count",
                "quota_used",
                "consumed_tokens",
                "ttft_ms",
                "prefill_tokens",
                "prefill_duration_s",
                "prefill_speed_tps",
                "prefill_speed_lower_bound",
                "decode_tokens",
                "decode_duration_s",
                "decode_speed_tps",
                "event_total",
            ] {
                if let Some(value) = overview.get(key) {
                    map.insert(key.to_string(), value.clone());
                }
            }
        }
    }
    let limit = query
        .limit
        .unwrap_or(ADMIN_MONITOR_DETAIL_EVENT_PAGE_MAX_LIMIT)
        .clamp(1, ADMIN_MONITOR_DETAIL_EVENT_PAGE_MAX_LIMIT);
    let storage = state.storage.clone();
    let owner = user_id.clone();
    let thread = cleaned.clone();
    let (turns, (user_round_total, item_total)) =
        crate::core::blocking::run_db("api.admin.thread_log.detail", move || {
            let turns = storage.list_thread_turns(&owner, &thread, None, limit)?;
            let counts = storage.get_thread_log_counts(&owner, &thread, true)?;
            Ok::<_, anyhow::Error>((turns, counts))
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(
        json!({"session":session,"events":[],"events_has_more":!turns.is_empty() && turns.len()>=limit as usize,"event_total":item_total,"user_round_total":user_round_total,"thread_turns":turns,"feedback":[]}),
    ))
}

#[derive(Debug, Deserialize)]
struct MonitorDetailQuery {
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    export_all: bool,
}

#[derive(Debug, Deserialize)]
struct AdminThreadTurnsQuery {
    #[serde(default)]
    before: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}
#[derive(Debug, Deserialize)]
struct AdminThreadChangesQuery {
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

fn monitor_user_id(state: &AppState, session_id: &str) -> Result<String, Response> {
    state
        .storage
        .get_chat_session_owner(session_id)
        .map_err(|err| error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))
}

async fn admin_thread_turns(
    State(state): State<Arc<AppState>>,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<AdminThreadTurnsQuery>,
) -> Result<Json<Value>, Response> {
    let session_id = session_id.trim().to_string();
    let user_id = monitor_user_id(&state, &session_id)?;
    let storage = state.storage.clone();
    let lookup = session_id.clone();
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let (turns, (user_round_total, item_total)) =
        crate::core::blocking::run_db("api.admin.thread_log.turns", move || {
            let turns = storage.list_thread_turns(&user_id, &lookup, query.before, limit)?;
            let counts = storage.get_thread_log_counts(&user_id, &lookup, true)?;
            Ok::<_, anyhow::Error>((turns, counts))
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let next_before = turns
        .last()
        .and_then(|v| v.get("user_turn_index"))
        .and_then(Value::as_i64);
    let has_more = turns.len() >= limit as usize;
    Ok(Json(
        json!({"data":{"session_id":session_id,"turns":turns,"user_round_total":user_round_total,"item_total":item_total,"next_before":next_before,"has_more":has_more}}),
    ))
}

async fn admin_thread_turn(
    State(state): State<Arc<AppState>>,
    AxumPath((session_id, turn_id)): AxumPath<(String, String)>,
    Query(query): Query<AdminThreadChangesQuery>,
) -> Result<Json<Value>, Response> {
    let session_id = session_id.trim().to_string();
    let user_id = monitor_user_id(&state, &session_id)?;
    let storage = state.storage.clone();
    let lookup = session_id.clone();
    let turn = crate::core::blocking::run_db("api.admin.thread_log.turn", move || {
        storage.get_thread_turn(
            &user_id,
            &lookup,
            &turn_id,
            query.item_after.unwrap_or(-1),
            query.limit.unwrap_or(100),
            true,
        )
    })
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.content_not_found")))?;
    Ok(Json(json!({"data":{"session_id":session_id,"turn":turn}})))
}

async fn admin_thread_changes(
    State(state): State<Arc<AppState>>,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<AdminThreadChangesQuery>,
) -> Result<Json<Value>, Response> {
    let session_id = session_id.trim().to_string();
    let user_id = monitor_user_id(&state, &session_id)?;
    let storage = state.storage.clone();
    let lookup = session_id.clone();
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let changes = crate::core::blocking::run_db("api.admin.thread_log.changes", move || {
        storage.list_thread_changes(&user_id, &lookup, query.after.unwrap_or(0), limit)
    })
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let snapshot_required = changes.iter().any(|change| {
        change.get("change_type").and_then(Value::as_str) == Some("snapshot_required")
    });
    Ok(Json(
        json!({"data":{"session_id":session_id,"changes":changes,"frame":if snapshot_required {"thread_snapshot_required"} else {"thread_change"}}}),
    ))
}

async fn admin_thread_item_content(
    State(state): State<Arc<AppState>>,
    AxumPath((session_id, item_id)): AxumPath<(String, String)>,
    Query(query): Query<AdminThreadChangesQuery>,
) -> Result<Json<Value>, Response> {
    let session_id = session_id.trim().to_string();
    let user_id = monitor_user_id(&state, &session_id)?;
    let storage = state.storage.clone();
    let lookup = session_id.clone();
    let item_lookup = item_id.clone();
    let field = query.field.clone();
    let field_for_query = field.clone();
    let from_block = query.from_block.or(query.item_after).unwrap_or(0);
    let blocks = crate::core::blocking::run_db("api.admin.thread_log.item_content", move || {
        storage.list_thread_item_blocks_page(
            &user_id,
            &lookup,
            &item_lookup,
            field_for_query.as_deref(),
            from_block,
            query.limit.unwrap_or(100).clamp(1, 100),
            true,
        )
    })
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(
        json!({"data":{"session_id":session_id,"item_id":item_id,"field":field,"blocks":blocks.0,"next_block":blocks.1,"has_more":blocks.2}}),
    ))
}

async fn admin_monitor_cancel(
    State(state): State<Arc<AppState>>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let record = state.monitor.get_record(&session_id);
    let user_id = record
        .as_ref()
        .and_then(|record| record.get("user_id"))
        .and_then(Value::as_str);
    let ok = if let Some(user_id) = user_id {
        state
            .kernel
            .thread_runtime
            .cancel_session_activity(user_id, &session_id, "admin_cancel")
            .await
            .map(|result| {
                result.monitor_cancelled
                    || result.queued_tasks_cancelled > 0
                    || result.running_tasks_marked_cancelled > 0
            })
            .unwrap_or(false)
    } else {
        false
    };
    if !ok {
        return Ok(Json(json!({
            "ok": false,
            "message": i18n::t("error.session_not_found_or_finished")
        })));
    }
    Ok(Json(
        json!({ "ok": true, "message": i18n::t("message.cancel_requested") }),
    ))
}

async fn admin_monitor_compaction(
    State(state): State<Arc<AppState>>,
    AxumPath(session_id): AxumPath<String>,
    Json(payload): Json<MonitorCompactionRequest>,
) -> Result<Json<Value>, Response> {
    let cleaned = session_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.param_required"),
        ));
    }
    state.monitor.warm_history(false);
    let record = state
        .monitor
        .get_record(cleaned)
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))?;
    let status = record.get("status").and_then(Value::as_str).unwrap_or("");
    if status == crate::monitor::MonitorState::STATUS_RUNNING
        || status == crate::monitor::MonitorState::STATUS_CANCELLING
    {
        return Err(error_response(
            StatusCode::CONFLICT,
            i18n::t("error.session_not_found_or_running"),
        ));
    }
    let user_id = record
        .get("user_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if user_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let session_record = state
        .user_store
        .get_chat_session(&user_id, cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let agent_id = session_record
        .as_ref()
        .and_then(|record| record.agent_id.clone());
    let agent_prompt = agent_id
        .as_deref()
        .and_then(|agent_id| {
            state
                .user_store
                .get_user_agent_by_id(agent_id)
                .ok()
                .flatten()
        })
        .and_then(|record| {
            let prompt = record.system_prompt.trim();
            if prompt.is_empty() {
                None
            } else {
                Some(prompt.to_string())
            }
        });
    let preview_skill = agent_id
        .as_deref()
        .and_then(|agent_id| {
            state
                .user_store
                .get_user_agent_by_id(agent_id)
                .ok()
                .flatten()
        })
        .map(|record| record.preview_skill)
        .unwrap_or(false);
    let is_admin = state
        .user_store
        .get_user_by_id(&user_id)
        .ok()
        .flatten()
        .map(|user| UserStore::is_admin(&user))
        .unwrap_or(false);
    let compaction_result = state
        .kernel
        .orchestrator
        .force_compact_session(
            &user_id,
            cleaned,
            is_admin,
            payload.model_name.as_deref(),
            agent_id.as_deref(),
            agent_prompt.as_deref(),
            Some(preview_skill),
            None,
            false,
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "ok": true, "data": compaction_result })))
}

async fn admin_monitor_delete(
    State(state): State<Arc<AppState>>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let cleaned = session_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.param_required"),
        ));
    }
    let session_id = cleaned.to_string();
    let ok = crate::core::blocking::run_db(
        "api.admin.delete_session",
        move || -> anyhow::Result<bool> {
            // The durable catalog owns identity; monitor history may already be evicted.
            let monitor_user_id = state.monitor.get_record(&session_id).and_then(|record| {
                record
                    .get("user_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
            let user_id = state
                .storage
                .get_chat_session_owner(&session_id)?
                .or(monitor_user_id);
            if let Some(user_id) = user_id {
                // Never report successful deletion while the user-visible catalog survives.
                state
                    .user_store
                    .delete_chat_session(&user_id, &session_id)?;
                state.workspace.purge_session_data(&user_id, &session_id);
                state.workspace.purge_session_logs(&user_id, &session_id);
                state
                    .storage
                    .delete_cron_jobs_by_session(&user_id, &session_id)?;
                state.memory.delete_record(&user_id, &session_id);
            }
            Ok(state.monitor.purge_session(&session_id))
        },
    )
    .await
    .map_err(|err| {
        warn!(error = %err, "admin session deletion failed");
        error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            i18n::t("error.internal_error"),
        )
    })?;
    if !ok {
        return Ok(Json(json!({
            "ok": false,
            "message": i18n::t("error.session_not_found")
        })));
    }
    Ok(Json(
        json!({ "ok": true, "message": i18n::t("message.deleted") }),
    ))
}

async fn admin_monitor_logs_cleanup(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<MonitorLogsCleanupRequest>,
) -> Result<Json<Value>, Response> {
    let Some(mut start_time) = normalize_ts(payload.start_time) else {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.param_required"),
        ));
    };
    let Some(mut end_time) = normalize_ts(payload.end_time) else {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.param_required"),
        ));
    };
    if end_time < start_time {
        std::mem::swap(&mut start_time, &mut end_time);
    }
    if end_time <= start_time {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.invalid_time_range"),
        ));
    }
    let monitor = state.monitor.clone();
    let deleted = crate::core::blocking::run_db("api.admin.cleanup_logs", move || {
        monitor
            .delete_logs_by_time_range(start_time, end_time)
            .map_err(anyhow::Error::msg)
    })
    .await
    .map_err(|err| {
        warn!(error = %err, "admin log cleanup failed");
        error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            i18n::t("error.internal_error"),
        )
    })?;
    let deleted_total: i64 = deleted.values().copied().sum();
    let system = state.monitor.get_system_metrics();
    Ok(Json(json!({
        "ok": true,
        "start_time": start_time,
        "end_time": end_time,
        "deleted": deleted,
        "deleted_total": deleted_total,
        "system": system,
        "message": i18n::t("message.deleted"),
    })))
}

async fn admin_performance_sample(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<PerformanceSampleRequest>,
) -> Result<Json<PerformanceSampleResponse>, Response> {
    let response = run_performance_sample(state, payload)
        .await
        .map_err(|message| error_response(StatusCode::BAD_REQUEST, message))?;
    Ok(Json(response))
}

fn normalize_ts(value: Option<f64>) -> Option<f64> {
    value.filter(|ts| *ts > 0.0)
}

fn format_ts(ts: f64) -> String {
    if ts <= 0.0 {
        return String::new();
    }
    let secs = ts.trunc() as i64;
    let nanos = ((ts.fract()) * 1_000_000_000.0) as u32;
    match Local.timestamp_opt(secs, nanos).single() {
        Some(dt) => dt.to_rfc3339(),
        None => String::new(),
    }
}

fn builtin_tool_names() -> HashSet<String> {
    builtin_tool_specs()
        .into_iter()
        .map(|spec| spec.name)
        .collect()
}

fn build_builtin_tool_display_map(config: &Config) -> HashMap<String, String> {
    crate::tools::build_runtime_tool_display_map(config)
}

fn normalize_tool_stats(
    tool_stats: Vec<HashMap<String, Value>>,
    config: &Config,
) -> Vec<HashMap<String, Value>> {
    let builtin_names = builtin_tool_names();
    let mut merged: HashMap<String, i64> = HashMap::new();
    for item in tool_stats {
        let raw_name = item
            .get("tool")
            .or_else(|| item.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if raw_name.is_empty() || raw_name.eq_ignore_ascii_case("performance_log") {
            continue;
        }
        let calls = item
            .get("calls")
            .or_else(|| item.get("count"))
            .or_else(|| item.get("tool_calls"))
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .max(0);
        let canonical = resolve_tool_name(&raw_name);
        let key = if builtin_names.contains(&canonical) {
            canonical
        } else {
            raw_name
        };
        *merged.entry(key).or_insert(0) += calls;
    }
    let mut merged_list = merged.into_iter().collect::<Vec<_>>();
    merged_list.sort_by(|a, b| b.1.cmp(&a.1));
    merged_list
        .into_iter()
        .map(|(name, calls)| {
            let mut entry = HashMap::new();
            let display_name = crate::tools::resolve_runtime_tool_display_name(config, &name);
            entry.insert("tool".to_string(), json!(display_name));
            entry.insert("tool_name".to_string(), json!(name));
            entry.insert("calls".to_string(), json!(calls));
            entry
        })
        .collect()
}

fn merge_tool_session_usage(records: Vec<HashMap<String, Value>>) -> Vec<HashMap<String, Value>> {
    #[derive(Default)]
    struct UsageEntry {
        session_id: String,
        user_id: String,
        tool_calls: i64,
        last_time: f64,
    }

    let mut merged: HashMap<(String, String), UsageEntry> = HashMap::new();
    for record in records {
        let session_id = record
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if session_id.is_empty() {
            continue;
        }
        let user_id = record
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let key = (session_id.clone(), user_id.clone());
        let entry = merged.entry(key).or_insert_with(|| UsageEntry {
            session_id: session_id.clone(),
            user_id: user_id.clone(),
            tool_calls: 0,
            last_time: 0.0,
        });
        if entry.user_id.is_empty() && !user_id.is_empty() {
            entry.user_id = user_id.clone();
        }
        let calls = record
            .get("tool_calls")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        entry.tool_calls += calls.max(0);
        let last_time = record
            .get("last_time")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        if last_time > entry.last_time {
            entry.last_time = last_time;
        }
    }

    merged
        .into_values()
        .map(|entry| {
            let mut record = HashMap::new();
            record.insert("session_id".to_string(), json!(entry.session_id));
            record.insert("user_id".to_string(), json!(entry.user_id));
            record.insert("tool_calls".to_string(), json!(entry.tool_calls));
            record.insert("last_time".to_string(), json!(entry.last_time));
            record
        })
        .collect()
}

#[derive(Debug, Deserialize, Default)]
struct MonitorQuery {
    active_only: Option<bool>,
    tool_hours: Option<f64>,
    start_time: Option<f64>,
    end_time: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
struct MonitorToolUsageQuery {
    tool: Option<String>,
    tool_hours: Option<f64>,
    start_time: Option<f64>,
    end_time: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct MonitorCompactionRequest {
    #[serde(default)]
    model_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MonitorLogsCleanupRequest {
    start_time: Option<f64>,
    end_time: Option<f64>,
}

async fn export_thread_log(
    State(state): State<Arc<AppState>>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Response, Response> {
    let user_id = monitor_user_id(&state, &session_id)?;
    Ok(crate::services::thread_log::export_response(
        state.storage.clone(),
        user_id,
        session_id,
        true,
    ))
}
