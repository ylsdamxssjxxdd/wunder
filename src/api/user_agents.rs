// 用户智能体 API：创建、管理用户自定义智能体。
use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::monitor::MonitorState;
use crate::schemas::AbilityDescriptor;
use crate::services::agent_abilities::{
    normalize_ability_items, resolve_agent_ability_selection, resolve_record_ability_items,
    resolve_record_declared_names,
};
use crate::services::default_agent_protocol::{
    default_agent_config_from_record, default_agent_meta_key, DefaultAgentConfig,
};
use crate::services::default_tool_profile::curated_default_tool_names;
use crate::services::llm::is_llm_model;
use crate::services::user_store::build_default_agent_record_from_storage;
use crate::state::AppState;
use crate::storage::{
    normalize_hive_id, normalize_sandbox_container_id, HiveRecord, DEFAULT_HIVE_ID,
    DEFAULT_SANDBOX_CONTAINER_ID,
};
use crate::user_access::{
    build_user_tool_context, compute_allowed_tool_names, filter_user_agents_by_access,
    is_agent_allowed,
};
use crate::user_tools::UserToolKind;
use anyhow::Result;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::{routing::get, Json, Router};
use chrono::{
    DateTime, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;

const DEFAULT_AGENT_ACCESS_LEVEL: &str = "A";
const DEFAULT_AGENT_APPROVAL_MODE: &str = "full_auto";
const DEFAULT_AGENT_ID_ALIAS: &str = "__default__";
const DEFAULT_AGENT_NAME: &str = "Default Agent";
const DEFAULT_AGENT_STATUS: &str = "active";
const DEFAULT_RUNTIME_WINDOW_DAYS: i64 = 14;
const MAX_RUNTIME_WINDOW_DAYS: i64 = 90;
const MAX_RUNTIME_RECORD_LIMIT: i64 = 5000;
const HEATMAP_TOOL_LIMIT: usize = 24;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/agents", get(list_agents).post(create_agent))
        .route("/wunder/agents/models", get(list_agent_models))
        .route("/wunder/agents/shared", get(list_shared_agents))
        .route("/wunder/agents/running", get(list_running_agents))
        .route("/wunder/agents/user-rounds", get(list_agent_user_rounds))
        .route(
            "/wunder/agents/{agent_id}/runtime-records",
            get(get_agent_runtime_records),
        )
        .route(
            "/wunder/agents/{agent_id}",
            get(get_agent).put(update_agent).delete(delete_agent),
        )
        .route(
            "/wunder/agents/{agent_id}/default-session",
            get(get_default_session).post(set_default_session),
        )
}

#[derive(Debug, Deserialize, Default)]
struct AgentUserQuery {
    #[serde(default)]
    user_id: Option<String>,
}

async fn list_agents(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(query): Query<AgentUserQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id.clone();
    sync_inner_visible_before_user_read(&state, &user_id).await?;
    state
        .user_store
        .ensure_default_hive(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    ensure_preset_agents(&state, &resolved.user).await?;
    let agents = state
        .user_store
        .list_user_agents(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let access = state
        .user_store
        .get_user_agent_access(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let filtered = filter_user_agents_by_access(&resolved.user, access.as_ref(), agents);
    let tool_context = build_user_tool_context(&state, &user_id).await;
    let skill_name_keys = collect_context_skill_names(&tool_context);
    let app_config = state.config_store.get().await;
    let configured_model_name = resolve_default_model_name(&app_config);
    let items = filtered
        .into_iter()
        .filter(|agent| !is_default_agent_alias_value(&agent.agent_id))
        .map(|record| agent_payload(&record, configured_model_name.as_deref(), &skill_name_keys))
        .collect::<Vec<_>>();
    Ok(Json(
        json!({ "data": { "total": items.len(), "items": items } }),
    ))
}

async fn list_shared_agents(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(query): Query<AgentUserQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id.clone();
    sync_inner_visible_before_user_read(&state, &user_id).await?;
    let agents = state
        .user_store
        .list_shared_user_agents(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let access = state
        .user_store
        .get_user_agent_access(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let filtered = filter_user_agents_by_access(&resolved.user, access.as_ref(), agents);
    let tool_context = build_user_tool_context(&state, &user_id).await;
    let skill_name_keys = collect_context_skill_names(&tool_context);
    let app_config = state.config_store.get().await;
    let configured_model_name = resolve_default_model_name(&app_config);
    let items = filtered
        .into_iter()
        .filter(|agent| !is_default_agent_alias_value(&agent.agent_id))
        .map(|record| agent_payload(&record, configured_model_name.as_deref(), &skill_name_keys))
        .collect::<Vec<_>>();
    Ok(Json(
        json!({ "data": { "total": items.len(), "items": items } }),
    ))
}

async fn list_agent_models(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(query): Query<AgentUserQuery>,
) -> Result<Json<Value>, Response> {
    let _resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let config = state.config_store.get().await;
    let items = resolve_available_model_names(&config);
    let default_model_name = resolve_default_model_name(&config);
    Ok(Json(json!({
        "data": {
            "items": items,
            "default_model_name": default_model_name,
        }
    })))
}

async fn list_running_agents(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(query): Query<AgentUserQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id.clone();

    #[derive(Debug, Clone, Default)]
    struct AgentStatusCandidate {
        state: &'static str,
        updated_time: f64,
        session_id: String,
        expires_at: Option<f64>,
        pending_question: bool,
        last_error: Option<String>,
    }

    const STATE_IDLE: &str = "idle";
    const STATE_WAITING: &str = "waiting";
    const STATE_RUNNING: &str = "running";
    const STATE_CANCELLING: &str = "cancelling";
    const STATE_DONE: &str = "done";
    const STATE_ERROR: &str = "error";

    const DONE_TTL_S: f64 = 15.0;
    const ERROR_TTL_S: f64 = 30.0;
    const RECENT_WINDOW_S: f64 = 120.0;
    const WAITING_TTL_S: f64 = 10.0 * 60.0;

    fn state_rank(state: &str) -> i32 {
        match state {
            STATE_WAITING => 50,
            STATE_CANCELLING => 40,
            STATE_RUNNING => 30,
            STATE_ERROR => 20,
            STATE_DONE => 10,
            _ => 0,
        }
    }

    fn is_waiting_state(state: &str) -> bool {
        state == STATE_WAITING
    }

    fn is_waiting_stale(candidate: &AgentStatusCandidate, now: f64) -> bool {
        if !is_waiting_state(candidate.state) {
            return false;
        }
        if candidate.updated_time <= 0.0 {
            return true;
        }
        (now - candidate.updated_time).max(0.0) > WAITING_TTL_S
    }

    fn should_replace(
        current: &AgentStatusCandidate,
        next: &AgentStatusCandidate,
        now: f64,
    ) -> bool {
        let current_waiting = is_waiting_state(current.state);
        let next_waiting = is_waiting_state(next.state);
        if next_waiting && is_waiting_stale(next, now) {
            return false;
        }
        if current_waiting && is_waiting_stale(current, now) {
            return true;
        }
        if current_waiting && !next_waiting && next.updated_time > current.updated_time {
            return true;
        }
        let current_rank = state_rank(current.state);
        let next_rank = state_rank(next.state);
        if next_rank != current_rank {
            return next_rank > current_rank;
        }
        next.updated_time > current.updated_time
    }

    fn format_optional_ts(value: f64) -> String {
        if value <= 0.0 {
            return "".to_string();
        }
        format_ts(value)
    }

    // Determine which agent apps should be included in the response.
    // Keep ordering stable: default, owned agents, shared agents.
    let access = state
        .user_store
        .get_user_agent_access(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let owned_agents = state
        .user_store
        .list_user_agents(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let owned_agents = filter_user_agents_by_access(&resolved.user, access.as_ref(), owned_agents)
        .into_iter()
        .filter(|agent| !is_default_agent_alias_value(&agent.agent_id))
        .collect::<Vec<_>>();

    let shared_agents = state
        .user_store
        .list_shared_user_agents(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let shared_agents =
        filter_user_agents_by_access(&resolved.user, access.as_ref(), shared_agents)
            .into_iter()
            .filter(|agent| !is_default_agent_alias_value(&agent.agent_id))
            .collect::<Vec<_>>();

    let mut agent_order = Vec::new();
    agent_order.push("".to_string()); // default entry

    let mut allowed_set = HashSet::new();
    allowed_set.insert("".to_string());
    for agent in &owned_agents {
        if allowed_set.insert(agent.agent_id.clone()) {
            agent_order.push(agent.agent_id.clone());
        }
    }
    for agent in &shared_agents {
        if allowed_set.insert(agent.agent_id.clone()) {
            agent_order.push(agent.agent_id.clone());
        }
    }

    let mut status_by_agent = HashMap::<String, AgentStatusCandidate>::new();
    for agent_id in &agent_order {
        status_by_agent.insert(
            agent_id.clone(),
            AgentStatusCandidate {
                state: STATE_IDLE,
                ..AgentStatusCandidate::default()
            },
        );
    }
    let now = now_ts();

    // 1) Session locks (authoritative for long-running sessions via heartbeat).
    let locks = state
        .user_store
        .list_session_locks_by_user(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    for lock in locks {
        let cleaned_agent = lock.agent_id.trim().to_string();
        if cleaned_agent.starts_with("subagent:") {
            continue;
        }
        if !allowed_set.contains(&cleaned_agent) {
            continue;
        }
        let next = AgentStatusCandidate {
            state: STATE_RUNNING,
            updated_time: lock.updated_time,
            session_id: lock.session_id,
            expires_at: Some(lock.expires_at),
            pending_question: false,
            last_error: None,
        };
        if let Some(current) = status_by_agent.get(&cleaned_agent) {
            if should_replace(current, &next, now) {
                status_by_agent.insert(cleaned_agent, next);
            }
        }
    }

    // 2) Active monitor sessions (waiting/running/cancelling), persisted in storage so they survive restarts.
    let active_records = state.monitor.load_records_by_user(
        &user_id,
        Some(&[
            MonitorState::STATUS_WAITING,
            MonitorState::STATUS_RUNNING,
            MonitorState::STATUS_CANCELLING,
        ]),
        None,
        2048,
    );
    for record in active_records {
        let session_user_id = record
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if session_user_id != user_id {
            continue;
        }
        let agent_id = record
            .get("agent_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if !allowed_set.contains(agent_id) {
            continue;
        }
        let status = record
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let state = match status {
            MonitorState::STATUS_WAITING => STATE_WAITING,
            MonitorState::STATUS_CANCELLING => STATE_CANCELLING,
            MonitorState::STATUS_RUNNING => STATE_RUNNING,
            _ => continue,
        };
        let updated_time = record
            .get("updated_time")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite())
            .unwrap_or(0.0);
        if state == STATE_WAITING {
            let waiting_age = (now - updated_time).max(0.0);
            if updated_time <= 0.0 || waiting_age > WAITING_TTL_S {
                continue;
            }
        }
        let session_id = record
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let next = AgentStatusCandidate {
            state,
            updated_time,
            session_id,
            expires_at: None,
            pending_question: state == STATE_WAITING,
            last_error: None,
        };
        if let Some(current) = status_by_agent.get(agent_id) {
            if should_replace(current, &next, now) {
                status_by_agent.insert(agent_id.to_string(), next);
            }
        }
    }

    // 3) Recently completed/error sessions, used to display a transient state without frontend inference.
    let recent_records = state.monitor.load_records_by_user(
        &user_id,
        Some(&[
            MonitorState::STATUS_FINISHED,
            MonitorState::STATUS_ERROR,
            MonitorState::STATUS_CANCELLED,
        ]),
        Some((now - RECENT_WINDOW_S).max(0.0)),
        512,
    );
    for record in recent_records {
        let session_user_id = record
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if session_user_id != user_id {
            continue;
        }
        let agent_id = record
            .get("agent_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if !allowed_set.contains(agent_id) {
            continue;
        }
        let status = record
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let updated_time = record
            .get("updated_time")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite())
            .unwrap_or(0.0);
        let ended_time = record
            .get("ended_time")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite())
            .unwrap_or(updated_time);
        let elapsed = (now - ended_time).max(0.0);
        let state = match status {
            MonitorState::STATUS_ERROR if elapsed <= ERROR_TTL_S => STATE_ERROR,
            MonitorState::STATUS_FINISHED if elapsed <= DONE_TTL_S => STATE_DONE,
            _ => continue,
        };
        let session_id = record
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let last_error = if state == STATE_ERROR {
            record
                .get("summary")
                .and_then(Value::as_str)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        } else {
            None
        };
        let next = AgentStatusCandidate {
            state,
            updated_time,
            session_id,
            expires_at: None,
            pending_question: false,
            last_error,
        };
        if let Some(current) = status_by_agent.get(agent_id) {
            if should_replace(current, &next, now) {
                status_by_agent.insert(agent_id.to_string(), next);
            }
        } else {
            status_by_agent.insert(agent_id.to_string(), next);
        }
    }

    let items = agent_order
        .into_iter()
        .map(|agent_id| {
            let candidate = status_by_agent.remove(&agent_id).unwrap_or_default();
            let is_default = agent_id.trim().is_empty();
            let mut payload = json!({
                "agent_id": if is_default { "" } else { agent_id.as_str() },
                "session_id": candidate.session_id,
                "updated_at": format_optional_ts(candidate.updated_time),
                "expires_at": candidate.expires_at.map(format_optional_ts).unwrap_or_default(),
                "state": candidate.state,
                "pending_question": candidate.pending_question,
                "is_default": is_default,
            });
            if let Some(last_error) = candidate
                .last_error
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                if let Value::Object(ref mut map) = payload {
                    map.insert(
                        "last_error".to_string(),
                        Value::String(last_error.to_string()),
                    );
                }
            }
            payload
        })
        .collect::<Vec<_>>();

    Ok(Json(
        json!({ "data": { "total": items.len(), "items": items } }),
    ))
}

async fn list_agent_user_rounds(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(query): Query<AgentUserQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id.clone();
    let records =
        state
            .monitor
            .load_records_by_user(&user_id, None, None, MAX_RUNTIME_RECORD_LIMIT);
    let mut totals: HashMap<String, i64> = HashMap::new();
    for record in records {
        let raw_agent_id = record
            .get("agent_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if raw_agent_id.starts_with("subagent:") {
            continue;
        }
        let agent_id = normalize_agent_id(raw_agent_id);
        let user_rounds =
            parse_i64_value(record.get("user_rounds").or_else(|| record.get("rounds")))
                .unwrap_or(0)
                .max(0);
        if user_rounds <= 0 {
            continue;
        }
        *totals.entry(agent_id).or_insert(0) += user_rounds;
    }
    let items = totals
        .into_iter()
        .map(|(agent_id, user_rounds)| {
            json!({
                "agent_id": agent_id,
                "user_rounds": user_rounds.max(0),
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(
        json!({ "data": { "total": items.len(), "items": items } }),
    ))
}

async fn get_agent(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(agent_id): AxumPath<String>,
    Query(query): Query<AgentUserQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id.clone();
    sync_inner_visible_before_user_read(&state, &user_id).await?;
    let cleaned = agent_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let normalized_agent_id = normalize_agent_id(cleaned);
    if normalized_agent_id.is_empty() {
        let config = resolve_default_agent_config(&state, &resolved.user).await?;
        let tool_context = build_user_tool_context(&state, &user_id).await;
        let skill_name_keys = collect_context_skill_names(&tool_context);
        let app_config = state.config_store.get().await;
        let configured_model_name = resolve_default_model_name(&app_config);
        return Ok(Json(
            json!({ "data": default_agent_payload(&config, configured_model_name.as_deref(), &skill_name_keys) }),
        ));
    }
    let record = state
        .user_store
        .get_user_agent_by_id(&normalized_agent_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.agent_not_found")))?;
    let access = state
        .user_store
        .get_user_agent_access(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if !is_agent_allowed(&resolved.user, access.as_ref(), &record) {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.agent_not_found"),
        ));
    }
    let tool_context = build_user_tool_context(&state, &user_id).await;
    let skill_name_keys = collect_context_skill_names(&tool_context);
    let app_config = state.config_store.get().await;
    let configured_model_name = resolve_default_model_name(&app_config);
    Ok(Json(
        json!({ "data": agent_payload(&record, configured_model_name.as_deref(), &skill_name_keys) }),
    ))
}

#[derive(Debug, Default, Clone)]
struct ThreadRuntimeDayStats {
    runtime_seconds: f64,
    billed_tokens: i64,
    quota_consumed: i64,
    tool_calls: i64,
}

async fn get_agent_runtime_records(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(agent_id): AxumPath<String>,
    Query(query): Query<ThreadRuntimeRecordsQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id.clone();
    let normalized_agent_id =
        normalize_agent_id(agent_id.trim().trim_matches('"').trim_matches('\''));
    let is_default_agent = normalized_agent_id.is_empty();
    if !is_default_agent {
        let record = state
            .user_store
            .get_user_agent_by_id(&normalized_agent_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
            .ok_or_else(|| {
                error_response(StatusCode::NOT_FOUND, i18n::t("error.agent_not_found"))
            })?;
        let access = state
            .user_store
            .get_user_agent_access(&user_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        if !is_agent_allowed(&resolved.user, access.as_ref(), &record) {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                i18n::t("error.agent_not_found"),
            ));
        }
    }

    let window_days = query
        .days
        .unwrap_or(DEFAULT_RUNTIME_WINDOW_DAYS)
        .clamp(1, MAX_RUNTIME_WINDOW_DAYS);
    let range_end = Local::now().date_naive();
    let selected_date = parse_runtime_date(query.date.as_deref()).unwrap_or(range_end);
    let range_start = range_end - Duration::days(window_days.saturating_sub(1));

    let (range_start_ts, _) = local_day_bounds(range_start).ok_or_else(|| {
        error_response(StatusCode::BAD_REQUEST, i18n::t("error.content_required"))
    })?;
    let (_, range_end_ts) = local_day_bounds(range_end).ok_or_else(|| {
        error_response(StatusCode::BAD_REQUEST, i18n::t("error.content_required"))
    })?;
    let (selected_start_ts, selected_end_ts) =
        local_day_bounds(selected_date).ok_or_else(|| {
            error_response(StatusCode::BAD_REQUEST, i18n::t("error.content_required"))
        })?;

    let records =
        state
            .monitor
            .load_records_by_user(&user_id, None, None, MAX_RUNTIME_RECORD_LIMIT);
    let mut daily = build_runtime_day_map(range_start, range_end);
    let mut heatmap_by_tool: HashMap<String, [i64; 24]> = HashMap::new();
    let mut summary_runtime_seconds = 0.0_f64;
    let mut summary_billed_tokens = 0_i64;
    let mut summary_quota_consumed = 0_i64;
    let mut summary_tool_calls = 0_i64;

    for record in records {
        let current_agent_id = normalize_agent_id(
            record
                .get("agent_id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
        if is_default_agent {
            if !current_agent_id.is_empty() {
                continue;
            }
        } else if current_agent_id != normalized_agent_id {
            continue;
        }

        let session_start = parse_timestamp_value(record.get("start_time")).unwrap_or(0.0);
        let mut session_end = parse_timestamp_value(record.get("ended_time"))
            .or_else(|| parse_timestamp_value(record.get("updated_time")))
            .unwrap_or(session_start);
        if session_end < session_start {
            session_end = session_start;
        }
        accumulate_runtime_seconds(
            &mut daily,
            session_start,
            session_end,
            range_start_ts,
            range_end_ts,
        );
        summary_runtime_seconds += (session_end - session_start).max(0.0);

        let mut round_usage_by_day: HashMap<String, i64> = HashMap::new();
        let mut token_usage_by_day: HashMap<String, i64> = HashMap::new();
        let mut has_round_usage = false;
        let mut has_round_usage_total = false;
        let mut session_round_usage = 0_i64;
        let mut session_token_usage = 0_i64;

        if let Some(events) = record.get("events").and_then(Value::as_array) {
            for event in events {
                let event_type = event
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                if event_type.is_empty() {
                    continue;
                }
                let data = event.get("data").unwrap_or(&Value::Null);
                let event_ts = parse_timestamp_value(event.get("timestamp")).unwrap_or(session_end);
                let Some(day_key) = runtime_day_key(event_ts) else {
                    continue;
                };
                let in_range = daily.contains_key(&day_key);

                match event_type {
                    "round_usage" => {
                        let total_tokens = parse_usage_total_tokens(data);
                        if total_tokens <= 0 {
                            continue;
                        }
                        has_round_usage_total = true;
                        session_round_usage =
                            session_round_usage.saturating_add(total_tokens.max(0));
                        if !in_range {
                            continue;
                        }
                        has_round_usage = true;
                        let entry = round_usage_by_day.entry(day_key).or_default();
                        *entry = entry.saturating_add(total_tokens);
                    }
                    "token_usage" => {
                        let total_tokens = parse_usage_total_tokens(data);
                        if total_tokens <= 0 {
                            continue;
                        }
                        session_token_usage =
                            session_token_usage.saturating_add(total_tokens.max(0));
                        if !in_range {
                            continue;
                        }
                        let entry = token_usage_by_day.entry(day_key).or_default();
                        *entry = entry.saturating_add(total_tokens);
                    }
                    "quota_usage" => {
                        let consumed = parse_i64_value(data.get("consumed")).unwrap_or(1).max(0);
                        if consumed <= 0 {
                            continue;
                        }
                        summary_quota_consumed = summary_quota_consumed.saturating_add(consumed);
                        if !in_range {
                            continue;
                        }
                        if let Some(entry) = daily.get_mut(&day_key) {
                            entry.quota_consumed = entry.quota_consumed.saturating_add(consumed);
                        }
                    }
                    "tool_call" => {
                        summary_tool_calls = summary_tool_calls.saturating_add(1);
                        if in_range {
                            if let Some(entry) = daily.get_mut(&day_key) {
                                entry.tool_calls = entry.tool_calls.saturating_add(1);
                            }
                        }
                        if event_ts < selected_start_ts || event_ts >= selected_end_ts {
                            continue;
                        }
                        let tool_name = extract_event_tool_name(data);
                        if tool_name.is_empty() {
                            continue;
                        }
                        let Some(hour) = runtime_day_hour(event_ts) else {
                            continue;
                        };
                        let bucket = heatmap_by_tool.entry(tool_name).or_insert([0; 24]);
                        bucket[hour] = bucket[hour].saturating_add(1);
                    }
                    _ => {}
                }
            }
        }

        let usage_source = if has_round_usage {
            &round_usage_by_day
        } else {
            &token_usage_by_day
        };
        let session_billed_tokens = if has_round_usage_total {
            session_round_usage
        } else {
            session_token_usage
        };
        summary_billed_tokens = summary_billed_tokens.saturating_add(session_billed_tokens.max(0));
        for (day_key, total_tokens) in usage_source {
            if let Some(entry) = daily.get_mut(day_key) {
                entry.billed_tokens = entry.billed_tokens.saturating_add((*total_tokens).max(0));
            }
        }
    }

    let daily_items = daily
        .iter()
        .map(|(date, stats)| {
            json!({
                "date": date,
                "runtime_seconds": round_f64(stats.runtime_seconds),
                "billed_tokens": stats.billed_tokens.max(0),
                "consumed_tokens": stats.billed_tokens.max(0),
                "quota_consumed": stats.quota_consumed.max(0),
                "tool_calls": stats.tool_calls.max(0),
            })
        })
        .collect::<Vec<_>>();

    let mut heatmap_items = heatmap_by_tool
        .into_iter()
        .map(|(tool, hourly)| {
            let total_calls = hourly.iter().copied().sum::<i64>();
            json!({
                "tool": tool,
                "hourly_calls": hourly.to_vec(),
                "total_calls": total_calls.max(0),
            })
        })
        .collect::<Vec<_>>();
    heatmap_items.sort_by(|left, right| {
        let left_calls = left.get("total_calls").and_then(Value::as_i64).unwrap_or(0);
        let right_calls = right
            .get("total_calls")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let left_name = left.get("tool").and_then(Value::as_str).unwrap_or("");
        let right_name = right.get("tool").and_then(Value::as_str).unwrap_or("");
        right_calls
            .cmp(&left_calls)
            .then_with(|| left_name.cmp(right_name))
    });
    if heatmap_items.len() > HEATMAP_TOOL_LIMIT {
        heatmap_items.truncate(HEATMAP_TOOL_LIMIT);
    }
    let heatmap_max_calls = heatmap_items
        .iter()
        .filter_map(|item| item.get("total_calls").and_then(Value::as_i64))
        .max()
        .unwrap_or(0);
    let response_agent_id = if is_default_agent {
        "__default__".to_string()
    } else {
        normalized_agent_id.clone()
    };

    Ok(Json(json!({
        "data": {
            "agent_id": response_agent_id,
            "range": {
                "days": window_days,
                "start_date": range_start.format("%Y-%m-%d").to_string(),
                "end_date": range_end.format("%Y-%m-%d").to_string(),
                "selected_date": selected_date.format("%Y-%m-%d").to_string(),
            },
            "summary": {
                "runtime_seconds": round_f64(summary_runtime_seconds.max(0.0)),
                "billed_tokens": summary_billed_tokens.max(0),
                "consumed_tokens": summary_billed_tokens.max(0),
                "quota_consumed": summary_quota_consumed.max(0),
                "tool_calls": summary_tool_calls.max(0),
            },
            "daily": daily_items,
            "heatmap": {
                "date": selected_date.format("%Y-%m-%d").to_string(),
                "max_calls": heatmap_max_calls.max(0),
                "items": heatmap_items,
            }
        }
    })))
}

async fn create_agent(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(query): Query<AgentUserQuery>,
    Json(payload): Json<AgentCreateRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id.clone();
    let name = payload.name.trim().to_string();
    if name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }

    state
        .user_store
        .ensure_default_hive(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let copy_from_agent_id = payload
        .copy_from_agent_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let requested_model_name = normalize_request_model_name(payload.model_name.as_deref());
    let copy_source = if let Some(copy_id) = copy_from_agent_id {
        let source = if is_default_agent_alias_value(copy_id) {
            build_default_agent_record_from_storage(
                state.user_store.storage_backend().as_ref(),
                &user_id,
            )
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        } else {
            state
                .user_store
                .get_user_agent(&user_id, copy_id)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
                .ok_or_else(|| {
                    error_response(StatusCode::NOT_FOUND, i18n::t("error.agent_not_found"))
                })?
        };
        Some(source)
    } else {
        None
    };
    let target_hive_id = resolve_agent_request_hive_id(
        state.as_ref(),
        &user_id,
        payload
            .hive_id
            .as_deref()
            .or(copy_source.as_ref().map(|item| item.hive_id.as_str())),
        payload.hive_name.as_deref(),
        payload.hive_description.as_deref(),
    )?;

    let tool_context = build_user_tool_context(&state, &user_id).await;
    let allowed_tool_names = compute_allowed_tool_names(&resolved.user, &tool_context);
    let skill_name_keys = collect_context_skill_names(&tool_context);

    let requested_tool_names = if let Some(source) = copy_source.as_ref() {
        source.tool_names.clone()
    } else {
        normalize_tool_list(payload.tool_names.clone())
    };
    let ability_selection = if let Some(source) = copy_source.as_ref() {
        resolve_agent_ability_selection(
            &requested_tool_names,
            Some(source.ability_items.clone()),
            Some(source.declared_tool_names.clone()),
            Some(source.declared_skill_names.clone()),
            &skill_name_keys,
        )
    } else {
        resolve_agent_ability_selection(
            &requested_tool_names,
            requested_create_ability_items(&payload),
            payload.declared_tool_names.clone().map(normalize_tool_list),
            payload
                .declared_skill_names
                .clone()
                .map(normalize_tool_list),
            &skill_name_keys,
        )
    };
    let mut tool_names = ability_selection.tool_names.clone();
    if !tool_names.is_empty() {
        tool_names = filter_allowed_tools(&tool_names, &allowed_tool_names);
    }

    let access_level = DEFAULT_AGENT_ACCESS_LEVEL.to_string();
    let approval_mode = if let Some(source) = copy_source.as_ref() {
        normalize_agent_approval_mode(Some(&source.approval_mode))
    } else {
        normalize_agent_approval_mode(payload.approval_mode.as_deref())
    };
    let status = normalize_agent_status(payload.status.as_deref());
    let is_shared = payload.is_shared.unwrap_or(false);
    let now = now_ts();
    let sandbox_container_id =
        normalize_sandbox_container_id(payload.sandbox_container_id.unwrap_or_else(|| {
            copy_source
                .as_ref()
                .map(|item| item.sandbox_container_id)
                .unwrap_or(DEFAULT_SANDBOX_CONTAINER_ID)
        }));

    let (description, system_prompt, icon) = if let Some(source) = copy_source.as_ref() {
        (
            source.description.clone(),
            source.system_prompt.clone(),
            source.icon.clone(),
        )
    } else {
        (
            payload.description.unwrap_or_default(),
            payload.system_prompt.unwrap_or_default(),
            payload.icon,
        )
    };
    let preset_questions = if let Some(source) = copy_source.as_ref() {
        source.preset_questions.clone()
    } else {
        normalize_preset_questions(payload.preset_questions)
    };

    let record = crate::storage::UserAgentRecord {
        agent_id: format!("agent_{}", Uuid::new_v4().simple()),
        user_id: user_id.clone(),
        hive_id: target_hive_id,
        name,
        description,
        system_prompt,
        model_name: requested_model_name.or_else(|| {
            copy_source
                .as_ref()
                .and_then(|item| normalize_request_model_name(item.model_name.as_deref()))
        }),
        ability_items: ability_selection.ability_items,
        tool_names,
        declared_tool_names: ability_selection.declared_tool_names,
        declared_skill_names: ability_selection.declared_skill_names,
        preset_questions,
        access_level,
        approval_mode,
        is_shared,
        status,
        icon,
        sandbox_container_id,
        created_at: now,
        updated_at: now,
        preset_binding: None,
    };
    state
        .user_store
        .upsert_user_agent(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    sync_inner_visible_after_user_change(&state, &user_id).await;
    let app_config = state.config_store.get().await;
    let configured_model_name = resolve_default_model_name(&app_config);
    Ok(Json(
        json!({ "data": agent_payload(&record, configured_model_name.as_deref(), &skill_name_keys) }),
    ))
}

async fn update_agent(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(agent_id): AxumPath<String>,
    Query(query): Query<AgentUserQuery>,
    Json(payload): Json<AgentUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id.clone();
    let cleaned = agent_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let normalized_agent_id = normalize_agent_id(cleaned);
    if normalized_agent_id.is_empty() {
        let requested_ability_items = requested_update_ability_items(&payload);
        let mut config = resolve_default_agent_config(&state, &resolved.user).await?;
        let tool_context = build_user_tool_context(&state, &user_id).await;
        let allowed = compute_allowed_tool_names(&resolved.user, &tool_context);
        let skill_name_keys = collect_context_skill_names(&tool_context);
        if let Some(name) = payload.name.as_deref() {
            let cleaned = name.trim();
            if !cleaned.is_empty() {
                config.name = cleaned.to_string();
            }
        }
        if let Some(description) = payload.description {
            config.description = description;
        }
        if let Some(system_prompt) = payload.system_prompt {
            config.system_prompt = system_prompt;
        }
        if payload.tool_names.is_some()
            || payload.ability_items.is_some()
            || payload.abilities.is_some()
            || payload.declared_tool_names.is_some()
            || payload.declared_skill_names.is_some()
        {
            let requested_tool_names = payload
                .tool_names
                .clone()
                .map(normalize_tool_list)
                .unwrap_or_else(|| config.tool_names.clone());
            let selection = resolve_agent_ability_selection(
                &requested_tool_names,
                requested_ability_items,
                payload.declared_tool_names.clone(),
                payload.declared_skill_names.clone(),
                &skill_name_keys,
            );
            config.tool_names = filter_allowed_tools(&selection.tool_names, &allowed);
            config.ability_items = selection.ability_items;
            config.declared_tool_names = selection.declared_tool_names;
            config.declared_skill_names = selection.declared_skill_names;
        }
        if let Some(preset_questions) = payload.preset_questions {
            config.preset_questions = normalize_preset_questions(preset_questions);
        }
        if let Some(status) = payload.status {
            config.status = normalize_agent_status(Some(&status));
        }
        if let Some(approval_mode) = payload.approval_mode {
            config.approval_mode = normalize_agent_approval_mode(Some(&approval_mode));
        }
        if payload.icon.is_some() {
            config.icon = payload.icon;
        }
        if let Some(sandbox_container_id) = payload.sandbox_container_id {
            config.sandbox_container_id = normalize_sandbox_container_id(sandbox_container_id);
        }
        config.updated_at = now_ts();
        if config.created_at <= 0.0 {
            config.created_at = config.updated_at;
        }
        save_default_agent_config(&state, &user_id, &config)?;
        sync_inner_visible_after_user_change(&state, &user_id).await;
        let app_config = state.config_store.get().await;
        let configured_model_name = resolve_default_model_name(&app_config);
        return Ok(Json(
            json!({ "data": default_agent_payload(&config, configured_model_name.as_deref(), &skill_name_keys) }),
        ));
    }
    let mut record = state
        .user_store
        .get_user_agent(&user_id, &normalized_agent_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.agent_not_found")))?;
    let requested_ability_items = requested_update_ability_items(&payload);
    if let Some(name) = payload.name.as_deref() {
        let cleaned = name.trim();
        if !cleaned.is_empty() {
            record.name = cleaned.to_string();
        }
    }
    if let Some(description) = payload.description {
        record.description = description;
    }
    if let Some(system_prompt) = payload.system_prompt {
        record.system_prompt = system_prompt;
    }
    if payload.model_name.is_some() {
        record.model_name = normalize_request_model_name(payload.model_name.as_deref());
    }
    if let Some(is_shared) = payload.is_shared {
        record.is_shared = is_shared;
    }
    if payload.tool_names.is_some()
        || payload.ability_items.is_some()
        || payload.abilities.is_some()
        || payload.declared_tool_names.is_some()
        || payload.declared_skill_names.is_some()
    {
        let requested_tool_names = payload
            .tool_names
            .clone()
            .map(normalize_tool_list)
            .unwrap_or_else(|| record.tool_names.clone());
        let context = build_user_tool_context(&state, &user_id).await;
        let allowed = compute_allowed_tool_names(&resolved.user, &context);
        let skill_name_keys = collect_context_skill_names(&context);
        let selection = resolve_agent_ability_selection(
            &requested_tool_names,
            requested_ability_items,
            payload.declared_tool_names.clone(),
            payload.declared_skill_names.clone(),
            &skill_name_keys,
        );
        record.tool_names = filter_allowed_tools(&selection.tool_names, &allowed);
        record.ability_items = selection.ability_items;
        record.declared_tool_names = selection.declared_tool_names;
        record.declared_skill_names = selection.declared_skill_names;
    }
    if let Some(preset_questions) = payload.preset_questions {
        record.preset_questions = normalize_preset_questions(preset_questions);
    }
    if let Some(status) = payload.status {
        record.status = normalize_agent_status(Some(&status));
    }
    if let Some(approval_mode) = payload.approval_mode {
        record.approval_mode = normalize_agent_approval_mode(Some(&approval_mode));
    }
    if payload.icon.is_some() {
        record.icon = payload.icon;
    }
    if let Some(sandbox_container_id) = payload.sandbox_container_id {
        record.sandbox_container_id = normalize_sandbox_container_id(sandbox_container_id);
    }
    if payload.hive_id.is_some() || payload.hive_name.is_some() {
        record.hive_id = resolve_agent_request_hive_id(
            state.as_ref(),
            &user_id,
            payload.hive_id.as_deref(),
            payload.hive_name.as_deref(),
            payload.hive_description.as_deref(),
        )?;
    }
    record.updated_at = now_ts();
    state
        .user_store
        .upsert_user_agent(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    sync_inner_visible_after_user_change(&state, &user_id).await;
    let app_config = state.config_store.get().await;
    let tool_context = build_user_tool_context(&state, &user_id).await;
    let skill_name_keys = collect_context_skill_names(&tool_context);
    let configured_model_name = resolve_default_model_name(&app_config);
    Ok(Json(
        json!({ "data": agent_payload(&record, configured_model_name.as_deref(), &skill_name_keys) }),
    ))
}

async fn delete_agent(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(agent_id): AxumPath<String>,
    Query(query): Query<AgentUserQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let cleaned = agent_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let normalized_agent_id = normalize_agent_id(cleaned);
    if normalized_agent_id.is_empty() {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            i18n::t("error.permission_denied"),
        ));
    }
    state
        .user_store
        .delete_user_agent(&resolved.user.user_id, &normalized_agent_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if let Err(err) = state
        .inner_visible
        .remove_agent_files(&resolved.user.user_id, &normalized_agent_id)
    {
        tracing::warn!(
            "failed to remove inner-visible files for {}/{}: {err}",
            resolved.user.user_id,
            normalized_agent_id
        );
    }
    let mut workspace_ids = state
        .workspace
        .scoped_user_id_variants(&resolved.user.user_id, Some(cleaned));
    workspace_ids.sort();
    workspace_ids.dedup();
    for workspace_id in workspace_ids {
        let _ = state.workspace.purge_user_data(&workspace_id);
    }
    Ok(Json(json!({ "data": { "id": cleaned } })))
}

async fn get_default_session(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(agent_id): AxumPath<String>,
    Query(query): Query<AgentUserQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id.clone();
    let normalized_agent = normalize_agent_id(&agent_id);
    if !normalized_agent.is_empty() {
        let record = state
            .user_store
            .get_user_agent_by_id(&normalized_agent)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let Some(record) = record else {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                i18n::t("error.agent_not_found"),
            ));
        };
        let access = state
            .user_store
            .get_user_agent_access(&user_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        if !is_agent_allowed(&resolved.user, access.as_ref(), &record) {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                i18n::t("error.agent_not_found"),
            ));
        }
    }
    let record = state
        .user_store
        .get_agent_thread(&user_id, &normalized_agent)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let session_id = record.as_ref().map(|item| item.session_id.clone());
    Ok(Json(json!({
        "data": {
            "agent_id": normalized_agent,
            "session_id": session_id,
        }
    })))
}

async fn set_default_session(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(agent_id): AxumPath<String>,
    Query(query): Query<AgentUserQuery>,
    Json(payload): Json<DefaultSessionRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id.clone();
    let normalized_agent = normalize_agent_id(&agent_id);
    if !normalized_agent.is_empty() {
        let record = state
            .user_store
            .get_user_agent_by_id(&normalized_agent)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let Some(record) = record else {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                i18n::t("error.agent_not_found"),
            ));
        };
        let access = state
            .user_store
            .get_user_agent_access(&user_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        if !is_agent_allowed(&resolved.user, access.as_ref(), &record) {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                i18n::t("error.agent_not_found"),
            ));
        }
    }
    let session_id = payload.session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let session_record = state
        .user_store
        .get_chat_session(&user_id, &session_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let Some(session_record) = session_record else {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.session_not_found"),
        ));
    };
    let session_agent = session_record.agent_id.clone().unwrap_or_default();
    if session_agent.trim() != normalized_agent {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.permission_denied"),
        ));
    }
    let record = state
        .kernel
        .thread_runtime
        .set_main_session(&user_id, &normalized_agent, &session_id, "manual")
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "agent_id": record.agent_id,
            "session_id": record.session_id,
            "thread_id": record.thread_id,
            "status": record.status,
            "updated_at": format_ts(record.updated_at),
        }
    })))
}

fn agent_payload(
    record: &crate::storage::UserAgentRecord,
    default_model_name: Option<&str>,
    skill_name_keys: &HashSet<String>,
) -> Value {
    let configured_model_name = normalize_request_model_name(record.model_name.as_deref());
    let effective_model_name = configured_model_name
        .clone()
        .or_else(|| normalize_request_model_name(default_model_name));
    let ability_items = resolve_record_ability_items(
        &record.ability_items,
        &record.tool_names,
        &record.declared_tool_names,
        &record.declared_skill_names,
        skill_name_keys,
    );
    json!({
        "id": record.agent_id,
        "name": record.name,
        "description": record.description,
        "system_prompt": record.system_prompt,
        "configured_model_name": configured_model_name,
        "model_name": effective_model_name,
        "ability_items": ability_items.clone(),
        "abilities": { "items": ability_items },
        "tool_names": record.tool_names,
        "declared_tool_names": record.declared_tool_names,
        "declared_skill_names": record.declared_skill_names,
        "preset_questions": record.preset_questions,
        "access_level": record.access_level,
        "approval_mode": normalize_agent_approval_mode(Some(&record.approval_mode)),
        "is_shared": record.is_shared,
        "status": record.status,
        "icon": record.icon,
        "hive_id": normalize_hive_id(&record.hive_id),
        "sandbox_container_id": normalize_sandbox_container_id(record.sandbox_container_id),
        "created_at": format_ts(record.created_at),
        "updated_at": format_ts(record.updated_at),
        "preset_binding": preset_binding_payload(record.preset_binding.as_ref()),
    })
}

fn preset_binding_payload(binding: Option<&crate::storage::UserAgentPresetBinding>) -> Value {
    let Some(binding) = binding else {
        return Value::Null;
    };
    json!({
        "preset_id": binding.preset_id,
        "preset_revision": binding.preset_revision,
    })
}

fn resolve_agent_request_hive_id(
    state: &AppState,
    user_id: &str,
    hive_id: Option<&str>,
    hive_name: Option<&str>,
    hive_description: Option<&str>,
) -> Result<String, Response> {
    let requested_hive_name = hive_name.map(str::trim).filter(|value| !value.is_empty());
    if let Some(name) = requested_hive_name {
        let mut normalized = hive_id
            .map(normalize_hive_id)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| normalize_hive_id(name));
        if normalized == DEFAULT_HIVE_ID {
            normalized = format!("beeroom-{}", Uuid::new_v4().simple());
        }
        let exists = state
            .user_store
            .get_hive(user_id, &normalized)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        if exists.is_none() {
            let now = now_ts();
            let record = HiveRecord {
                hive_id: normalized.clone(),
                user_id: user_id.to_string(),
                name: name.to_string(),
                description: hive_description.unwrap_or_default().trim().to_string(),
                is_default: false,
                status: "active".to_string(),
                created_time: now,
                updated_time: now,
            };
            state
                .user_store
                .upsert_hive(&record)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
        return Ok(normalized);
    }

    let normalized = normalize_hive_id(hive_id.unwrap_or(DEFAULT_HIVE_ID));
    if normalized == DEFAULT_HIVE_ID {
        state
            .user_store
            .ensure_default_hive(user_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        return Ok(normalized);
    }
    let exists = state
        .user_store
        .get_hive(user_id, &normalized)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if exists.is_none() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            format!("hive {normalized} not found"),
        ));
    }
    Ok(normalized)
}

fn normalize_tool_list(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for raw in values {
        let name = raw.trim().to_string();
        if name.is_empty() || seen.contains(&name) {
            continue;
        }
        seen.insert(name.clone());
        output.push(name);
    }
    output
}

fn collect_context_skill_names(context: &crate::user_access::UserToolContext) -> HashSet<String> {
    let mut output = HashSet::new();
    for spec in context.skills.list_specs() {
        let cleaned = spec.name.trim();
        if !cleaned.is_empty() {
            output.insert(cleaned.to_string());
        }
    }
    for spec in &context.bindings.skill_specs {
        let cleaned = spec.name.trim();
        if !cleaned.is_empty() {
            output.insert(cleaned.to_string());
        }
    }
    for (alias, info) in &context.bindings.alias_map {
        if !matches!(info.kind, UserToolKind::Skill) {
            continue;
        }
        let cleaned_alias = alias.trim();
        if !cleaned_alias.is_empty() {
            output.insert(cleaned_alias.to_string());
        }
        let cleaned_target = info.target.trim();
        if !cleaned_target.is_empty() {
            output.insert(cleaned_target.to_string());
        }
    }
    output
}

fn requested_create_ability_items(payload: &AgentCreateRequest) -> Option<Vec<AbilityDescriptor>> {
    payload.ability_items.clone().or_else(|| {
        payload
            .abilities
            .as_ref()
            .map(|abilities| abilities.items.clone())
    })
}

fn requested_update_ability_items(payload: &AgentUpdateRequest) -> Option<Vec<AbilityDescriptor>> {
    payload.ability_items.clone().or_else(|| {
        payload
            .abilities
            .as_ref()
            .map(|abilities| abilities.items.clone())
    })
}

fn normalize_preset_questions(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for raw in values {
        let question = raw.trim().to_string();
        if question.is_empty() || seen.contains(&question) {
            continue;
        }
        seen.insert(question.clone());
        output.push(question);
    }
    output
}

fn filter_allowed_tools(values: &[String], allowed: &HashSet<String>) -> Vec<String> {
    values
        .iter()
        .filter(|name| allowed.contains(*name))
        .cloned()
        .collect()
}

fn normalize_agent_status(raw: Option<&str>) -> String {
    let status = raw.unwrap_or("active").trim();
    if status.is_empty() {
        "active".to_string()
    } else {
        status.to_string()
    }
}

fn normalize_agent_approval_mode(raw: Option<&str>) -> String {
    let cleaned = raw.unwrap_or("").trim().to_ascii_lowercase();
    match cleaned.as_str() {
        "suggest" => "suggest".to_string(),
        "auto_edit" | "auto-edit" => "auto_edit".to_string(),
        "full_auto" | "full-auto" => "full_auto".to_string(),
        _ => DEFAULT_AGENT_APPROVAL_MODE.to_string(),
    }
}

fn format_ts(ts: f64) -> String {
    let millis = (ts * 1000.0) as i64;
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(millis)
        .map(|dt| dt.with_timezone(&chrono::Local).to_rfc3339())
        .unwrap_or_default()
}

fn now_ts() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn parse_runtime_date(raw: Option<&str>) -> Option<NaiveDate> {
    let cleaned = raw.unwrap_or("").trim();
    if cleaned.is_empty() {
        return None;
    }
    NaiveDate::parse_from_str(cleaned, "%Y-%m-%d").ok()
}

fn build_runtime_day_map(
    range_start: NaiveDate,
    range_end: NaiveDate,
) -> BTreeMap<String, ThreadRuntimeDayStats> {
    let mut result = BTreeMap::new();
    let mut cursor = range_start;
    while cursor <= range_end {
        result.insert(
            cursor.format("%Y-%m-%d").to_string(),
            ThreadRuntimeDayStats::default(),
        );
        cursor += Duration::days(1);
    }
    result
}

fn resolve_local_datetime(naive: NaiveDateTime) -> Option<DateTime<Local>> {
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(dt) => Some(dt),
        LocalResult::Ambiguous(early, _) => Some(early),
        LocalResult::None => Some(Utc.from_utc_datetime(&naive).with_timezone(&Local)),
    }
}

fn local_day_bounds(date: NaiveDate) -> Option<(f64, f64)> {
    let start_naive = date.and_hms_opt(0, 0, 0)?;
    let next_day_naive = (date + Duration::days(1)).and_hms_opt(0, 0, 0)?;
    let start = resolve_local_datetime(start_naive)?;
    let end = resolve_local_datetime(next_day_naive)?;
    Some((
        start.timestamp_millis() as f64 / 1000.0,
        end.timestamp_millis() as f64 / 1000.0,
    ))
}

fn runtime_local_datetime(ts: f64) -> Option<DateTime<Local>> {
    if !ts.is_finite() || ts <= 0.0 {
        return None;
    }
    let secs = ts.trunc() as i64;
    let fract = (ts - secs as f64).max(0.0);
    let mut nanos = (fract * 1_000_000_000.0).round() as u32;
    if nanos >= 1_000_000_000 {
        nanos = 999_999_999;
    }
    DateTime::<Utc>::from_timestamp(secs, nanos).map(|dt| dt.with_timezone(&Local))
}

fn runtime_day_key(ts: f64) -> Option<String> {
    runtime_local_datetime(ts).map(|dt| dt.format("%Y-%m-%d").to_string())
}

fn runtime_day_hour(ts: f64) -> Option<usize> {
    runtime_local_datetime(ts).map(|dt| dt.hour() as usize)
}

fn parse_timestamp_value(value: Option<&Value>) -> Option<f64> {
    let value = value?;
    if let Some(ts) = value.as_f64().filter(|ts| ts.is_finite() && *ts > 0.0) {
        return Some(ts);
    }
    let text = value.as_str()?.trim();
    if text.is_empty() {
        return None;
    }
    if let Ok(parsed) = text.parse::<f64>() {
        if parsed.is_finite() && parsed > 0.0 {
            return Some(parsed);
        }
    }
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|dt| dt.timestamp_millis() as f64 / 1000.0)
}

fn parse_i64_value(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    if let Some(parsed) = value.as_i64() {
        return Some(parsed);
    }
    if let Some(parsed) = value.as_u64() {
        return Some(parsed as i64);
    }
    if let Some(parsed) = value.as_f64() {
        if !parsed.is_finite() {
            return None;
        }
        return Some(parsed.round() as i64);
    }
    value.as_str()?.trim().parse::<i64>().ok()
}

fn parse_usage_total_tokens(data: &Value) -> i64 {
    let direct_total = parse_i64_value(data.get("total_tokens"));
    let nested_total = data
        .get("usage")
        .and_then(|usage| parse_i64_value(usage.get("total_tokens")));
    if let Some(total) = direct_total.or(nested_total) {
        return total.max(0);
    }
    let direct_input = parse_i64_value(data.get("input_tokens")).unwrap_or(0);
    let direct_output = parse_i64_value(data.get("output_tokens")).unwrap_or(0);
    if direct_input > 0 || direct_output > 0 {
        return direct_input.saturating_add(direct_output).max(0);
    }
    let nested_input = data
        .get("usage")
        .and_then(|usage| parse_i64_value(usage.get("input_tokens")))
        .unwrap_or(0);
    let nested_output = data
        .get("usage")
        .and_then(|usage| parse_i64_value(usage.get("output_tokens")))
        .unwrap_or(0);
    nested_input.saturating_add(nested_output).max(0)
}

fn extract_event_tool_name(data: &Value) -> String {
    for key in ["tool", "tool_name", "toolName", "name"] {
        let Some(value) = data.get(key).and_then(Value::as_str) else {
            continue;
        };
        let cleaned = value.trim();
        if !cleaned.is_empty() {
            return cleaned.to_string();
        }
    }
    "unknown".to_string()
}

fn accumulate_runtime_seconds(
    daily: &mut BTreeMap<String, ThreadRuntimeDayStats>,
    start_ts: f64,
    end_ts: f64,
    range_start_ts: f64,
    range_end_ts: f64,
) {
    if !start_ts.is_finite() || !end_ts.is_finite() {
        return;
    }
    let mut cursor = start_ts.max(range_start_ts);
    let end = end_ts.min(range_end_ts);
    if end <= cursor {
        return;
    }
    while cursor < end {
        let Some(dt) = runtime_local_datetime(cursor) else {
            break;
        };
        let day_key = dt.format("%Y-%m-%d").to_string();
        let Some((_, day_end_ts)) = local_day_bounds(dt.date_naive()) else {
            break;
        };
        let segment_end = end.min(day_end_ts);
        if segment_end <= cursor {
            break;
        }
        let duration = (segment_end - cursor).max(0.0);
        if duration > 0.0 {
            if let Some(entry) = daily.get_mut(&day_key) {
                entry.runtime_seconds += duration;
            }
        }
        cursor = segment_end;
    }
}

fn round_f64(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

const PRESET_META_PREFIX: &str = "user_agent_presets_v1:";
const PRESET_CONTAINER_META_PREFIX: &str = "user_agent_presets_container_v1:";

type PresetAgent = crate::services::user_agent_presets::PresetAgent;

async fn ensure_preset_agents(
    state: &AppState,
    user: &crate::storage::UserAccountRecord,
) -> Result<(), Response> {
    let meta_key = format!("{PRESET_META_PREFIX}{}", user.user_id);
    let container_meta_key = format!("{PRESET_CONTAINER_META_PREFIX}{}", user.user_id);
    let preset_agents = configured_preset_agents(state).await;
    let bootstrap_completed = state
        .user_store
        .get_meta(&meta_key)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .is_some();
    let configured_preset_ids = preset_agents
        .iter()
        .map(|preset| preset.preset_id.clone())
        .collect::<HashSet<_>>();
    let mut existing = state
        .user_store
        .list_user_agents(&user.user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let duplicate_ids = duplicate_preset_bound_agent_ids(&existing, &configured_preset_ids);
    if !duplicate_ids.is_empty() {
        for duplicate_id in &duplicate_ids {
            let _ = state
                .user_store
                .delete_user_agent(&user.user_id, duplicate_id);
        }
        existing.retain(|record| !duplicate_ids.contains(&record.agent_id));
    }

    let now = now_ts();
    let container_layout_seeded = state
        .user_store
        .get_meta(&container_meta_key)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .is_some();
    let mut target_by_preset_id = HashMap::new();
    let mut matched_preset_by_agent_id = HashMap::new();
    for preset in &preset_agents {
        if let Some(record) =
            crate::services::user_agent_presets::find_preset_agent(&existing, preset)
        {
            matched_preset_by_agent_id.insert(record.agent_id.clone(), preset.clone());
            target_by_preset_id.insert(
                preset.preset_id.clone(),
                crate::services::user_agent_presets::build_target_snapshot(state, user, preset)
                    .await,
            );
        }
    }
    let mut existing_mutated = false;
    for record in &existing {
        let mut updated = record.clone();
        let mut changed = false;
        let mut matched_preset = matched_preset_by_agent_id.get(&record.agent_id).cloned();
        if matched_preset.is_none() {
            matched_preset = preset_by_name(&preset_agents, updated.name.trim());
        }

        if !container_layout_seeded {
            if let Some(container_id) = matched_preset
                .as_ref()
                .map(|preset| preset.sandbox_container_id)
            {
                if updated.sandbox_container_id == DEFAULT_SANDBOX_CONTAINER_ID
                    && updated.sandbox_container_id != container_id
                {
                    updated.sandbox_container_id = container_id;
                    changed = true;
                }
            }
        }
        if let Some(preset) = matched_preset.as_ref() {
            // Keep preset identity stable even if the user later renames the agent.
            if updated.preset_binding.is_none() {
                let target = match target_by_preset_id.get(&preset.preset_id) {
                    Some(snapshot) => snapshot.clone(),
                    None => {
                        let snapshot = crate::services::user_agent_presets::build_target_snapshot(
                            state, user, preset,
                        )
                        .await;
                        target_by_preset_id.insert(preset.preset_id.clone(), snapshot.clone());
                        snapshot
                    }
                };
                updated.preset_binding = Some(crate::services::user_agent_presets::build_binding(
                    preset, &target,
                ));
                changed = true;
            }
        }

        if changed {
            updated.updated_at = now;
            let _ = state.user_store.upsert_user_agent(&updated);
            existing_mutated = true;
        }
    }
    if !container_layout_seeded {
        let _ = state.user_store.set_meta(&container_meta_key, "1");
    }
    if existing_mutated {
        existing = state
            .user_store
            .list_user_agents(&user.user_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    let mut preset_agents_restored = false;
    for preset in &preset_agents {
        if crate::services::user_agent_presets::find_preset_agent(&existing, preset).is_some() {
            continue;
        }
        // Always restore missing preset agents when the preset list is available.
        // This recovers users that were bootstrapped while the preset config was empty.
        let record = crate::services::user_agent_presets::create_preset_agent_record(
            state, user, preset, now,
        )
        .await;
        let _ = state.user_store.upsert_user_agent(&record);
        preset_agents_restored = true;
    }
    let mut bootstrap_meta_written = false;
    if !bootstrap_completed && !preset_agents.is_empty() {
        let _ = state.user_store.set_meta(&meta_key, "1");
        bootstrap_meta_written = true;
    }
    if existing_mutated
        || preset_agents_restored
        || bootstrap_meta_written
        || !container_layout_seeded
    {
        sync_inner_visible_after_user_change(state, &user.user_id).await;
    }
    Ok(())
}

async fn configured_preset_agents(state: &AppState) -> Vec<PresetAgent> {
    crate::services::user_agent_presets::configured_preset_agents(state).await
}

fn preset_by_name(preset_agents: &[PresetAgent], name: &str) -> Option<PresetAgent> {
    let cleaned = name.trim();
    if cleaned.is_empty() {
        return None;
    }
    preset_agents
        .iter()
        .find(|preset| preset.name == cleaned)
        .cloned()
}

fn duplicate_preset_bound_agent_ids(
    records: &[crate::storage::UserAgentRecord],
    configured_preset_ids: &HashSet<String>,
) -> HashSet<String> {
    let mut duplicates_by_preset_id: HashMap<String, Vec<&crate::storage::UserAgentRecord>> =
        HashMap::new();
    for record in records {
        if normalize_hive_id(&record.hive_id) != DEFAULT_HIVE_ID {
            continue;
        }
        let Some(binding) = record.preset_binding.as_ref() else {
            continue;
        };
        if !configured_preset_ids.contains(&binding.preset_id) {
            continue;
        }
        duplicates_by_preset_id
            .entry(binding.preset_id.clone())
            .or_default()
            .push(record);
    }

    let mut duplicate_ids = HashSet::new();
    for records in duplicates_by_preset_id.values_mut() {
        if records.len() <= 1 {
            continue;
        }
        records.sort_by(|left, right| right.updated_at.total_cmp(&left.updated_at));
        for duplicate in records.iter().skip(1) {
            duplicate_ids.insert(duplicate.agent_id.clone());
        }
    }
    duplicate_ids
}

fn normalize_agent_id(raw: &str) -> String {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return "".to_string();
    }
    let lowered = cleaned.to_ascii_lowercase();
    if lowered == "__default__" || lowered == "default" {
        return "".to_string();
    }
    cleaned.to_string()
}

fn is_default_agent_alias_value(raw: &str) -> bool {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return false;
    }
    cleaned.eq_ignore_ascii_case(DEFAULT_AGENT_ID_ALIAS) || cleaned.eq_ignore_ascii_case("default")
}

fn normalize_default_agent_config(config: &mut DefaultAgentConfig) {
    if config.name.trim().is_empty() {
        config.name = DEFAULT_AGENT_NAME.to_string();
    }
    if config.status.trim().is_empty() {
        config.status = DEFAULT_AGENT_STATUS.to_string();
    } else {
        config.status = normalize_agent_status(Some(&config.status));
    }
    if config.approval_mode.trim().is_empty() {
        config.approval_mode = DEFAULT_AGENT_APPROVAL_MODE.to_string();
    } else {
        config.approval_mode = normalize_agent_approval_mode(Some(&config.approval_mode));
    }
    if config.sandbox_container_id <= 0 {
        config.sandbox_container_id = DEFAULT_SANDBOX_CONTAINER_ID;
    }
    config.tool_names = normalize_tool_list(std::mem::take(&mut config.tool_names));
    config.ability_items = normalize_ability_items(std::mem::take(&mut config.ability_items));
    config.declared_tool_names =
        normalize_tool_list(std::mem::take(&mut config.declared_tool_names));
    config.declared_skill_names =
        normalize_tool_list(std::mem::take(&mut config.declared_skill_names));
    config.preset_questions =
        normalize_preset_questions(std::mem::take(&mut config.preset_questions));
}

async fn load_default_agent_config(
    state: &AppState,
    user_id: &str,
) -> Result<Option<DefaultAgentConfig>, Response> {
    let key = default_agent_meta_key(user_id);
    let raw = state
        .user_store
        .get_meta(&key)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return Ok(None);
    }
    let mut parsed: DefaultAgentConfig = match serde_json::from_str(cleaned) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    normalize_default_agent_config(&mut parsed);
    Ok(Some(parsed))
}

async fn build_default_agent_config(
    state: &AppState,
    user: &crate::storage::UserAccountRecord,
) -> DefaultAgentConfig {
    let context = build_user_tool_context(state, &user.user_id).await;
    let allowed = compute_allowed_tool_names(user, &context);
    let skill_name_keys = collect_context_skill_names(&context);
    let tool_names = curated_default_tool_names(&allowed);
    let now = now_ts();
    let mut config = DefaultAgentConfig {
        name: DEFAULT_AGENT_NAME.to_string(),
        description: String::new(),
        system_prompt: String::new(),
        ability_items: resolve_record_ability_items(&[], &tool_names, &[], &[], &skill_name_keys),
        tool_names,
        declared_tool_names: Vec::new(),
        declared_skill_names: Vec::new(),
        preset_questions: Vec::new(),
        approval_mode: DEFAULT_AGENT_APPROVAL_MODE.to_string(),
        status: DEFAULT_AGENT_STATUS.to_string(),
        icon: Some("avatar-046".to_string()),
        sandbox_container_id: DEFAULT_SANDBOX_CONTAINER_ID,
        created_at: now,
        updated_at: now,
    };
    let (declared_tool_names, declared_skill_names) = resolve_record_declared_names(
        &config.ability_items,
        &config.tool_names,
        &config.declared_tool_names,
        &config.declared_skill_names,
        &skill_name_keys,
    );
    config.declared_tool_names = declared_tool_names;
    config.declared_skill_names = declared_skill_names;
    normalize_default_agent_config(&mut config);
    config
}

async fn resolve_default_agent_config(
    state: &AppState,
    user: &crate::storage::UserAccountRecord,
) -> Result<DefaultAgentConfig, Response> {
    if let Some(mut config) = load_default_agent_config(state, &user.user_id).await? {
        let now = now_ts();
        if config.created_at <= 0.0 {
            config.created_at = now;
        }
        if config.updated_at <= 0.0 {
            config.updated_at = config.created_at;
        }
        return Ok(config);
    }
    let record = state
        .user_store
        .get_user_agent(&user.user_id, DEFAULT_AGENT_ID_ALIAS)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if let Some(record) = record {
        let mut config = default_agent_config_from_record(&record);
        normalize_default_agent_config(&mut config);
        return Ok(config);
    }
    Ok(build_default_agent_config(state, user).await)
}

fn default_agent_payload(
    config: &DefaultAgentConfig,
    model_name: Option<&str>,
    skill_name_keys: &HashSet<String>,
) -> Value {
    let effective_model_name = normalize_request_model_name(model_name);
    let (declared_tool_names, declared_skill_names) = resolve_record_declared_names(
        &config.ability_items,
        &config.tool_names,
        &config.declared_tool_names,
        &config.declared_skill_names,
        skill_name_keys,
    );
    let ability_items = resolve_record_ability_items(
        &config.ability_items,
        &config.tool_names,
        &declared_tool_names,
        &declared_skill_names,
        skill_name_keys,
    );
    json!({
        "id": DEFAULT_AGENT_ID_ALIAS,
        "name": config.name,
        "description": config.description,
        "system_prompt": config.system_prompt,
        "configured_model_name": Value::Null,
        "model_name": effective_model_name,
        "ability_items": ability_items.clone(),
        "abilities": { "items": ability_items },
        "tool_names": config.tool_names,
        "declared_tool_names": declared_tool_names,
        "declared_skill_names": declared_skill_names,
        "preset_questions": config.preset_questions,
        "access_level": DEFAULT_AGENT_ACCESS_LEVEL,
        "approval_mode": normalize_agent_approval_mode(Some(&config.approval_mode)),
        "is_shared": false,
        "status": normalize_agent_status(Some(&config.status)),
        "icon": config.icon,
        "hive_id": DEFAULT_HIVE_ID,
        "sandbox_container_id": normalize_sandbox_container_id(config.sandbox_container_id),
        "created_at": format_ts(config.created_at),
        "updated_at": format_ts(config.updated_at),
        "preset_binding": Value::Null,
    })
}

fn resolve_default_model_name(config: &crate::config::Config) -> Option<String> {
    let default_key = config.llm.default.trim();
    if !default_key.is_empty() {
        return Some(default_key.to_string());
    }
    resolve_available_model_names(config).into_iter().next()
}

fn resolve_available_model_names(config: &crate::config::Config) -> Vec<String> {
    let mut names = Vec::new();
    for (key, cfg) in config.llm.models.iter() {
        if !is_llm_model(cfg) {
            continue;
        }
        let trimmed = key.trim();
        if !trimmed.is_empty() {
            names.push(trimmed.to_string());
        }
    }
    names.sort();
    names.dedup();
    names
}

fn normalize_request_model_name(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn save_default_agent_config(
    state: &AppState,
    user_id: &str,
    config: &DefaultAgentConfig,
) -> Result<(), Response> {
    let key = default_agent_meta_key(user_id);
    let payload = serde_json::to_string(config)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    state
        .user_store
        .set_meta(&key, &payload)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(())
}

async fn sync_inner_visible_after_user_change(state: &AppState, user_id: &str) {
    if let Err(err) = state.inner_visible.sync_user_state(user_id).await {
        tracing::warn!("failed to sync inner-visible state for {user_id}: {err}");
    }
}

async fn sync_inner_visible_before_user_read(
    state: &AppState,
    user_id: &str,
) -> Result<(), Response> {
    state
        .inner_visible
        .sync_user_state(user_id)
        .await
        .map_err(|err| {
            error_response(
                StatusCode::BAD_REQUEST,
                format!("failed to sync inner-visible state for user read: {err}"),
            )
        })?;
    Ok(())
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}

#[derive(Debug, Deserialize)]
struct AgentAbilitiesRequest {
    #[serde(default)]
    items: Vec<AbilityDescriptor>,
}

#[derive(Debug, Deserialize)]
struct AgentCreateRequest {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default, alias = "modelName", alias = "model_name")]
    model_name: Option<String>,
    #[serde(default)]
    tool_names: Vec<String>,
    #[serde(default, alias = "abilityItems", alias = "ability_items")]
    ability_items: Option<Vec<AbilityDescriptor>>,
    #[serde(default)]
    abilities: Option<AgentAbilitiesRequest>,
    #[serde(default, alias = "declaredToolNames", alias = "declared_tool_names")]
    declared_tool_names: Option<Vec<String>>,
    #[serde(default, alias = "declaredSkillNames", alias = "declared_skill_names")]
    declared_skill_names: Option<Vec<String>>,
    #[serde(default, alias = "presetQuestions", alias = "preset_questions")]
    preset_questions: Vec<String>,
    #[serde(default)]
    is_shared: Option<bool>,
    #[serde(default)]
    status: Option<String>,
    #[serde(
        default,
        alias = "approvalMode",
        alias = "approval_mode",
        alias = "permissionLevel",
        alias = "permission_level"
    )]
    approval_mode: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    sandbox_container_id: Option<i32>,
    #[serde(
        default,
        alias = "hiveId",
        alias = "beeroomGroupId",
        alias = "beeroom_group_id"
    )]
    hive_id: Option<String>,
    #[serde(
        default,
        alias = "hiveName",
        alias = "beeroomGroupName",
        alias = "beeroom_group_name"
    )]
    hive_name: Option<String>,
    #[serde(
        default,
        alias = "hiveDescription",
        alias = "beeroomGroupDescription",
        alias = "beeroom_group_description"
    )]
    hive_description: Option<String>,
    #[serde(default, alias = "copyFromAgentId", alias = "copy_from_agent_id")]
    copy_from_agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DefaultSessionRequest {
    session_id: String,
}

#[derive(Debug, Deserialize)]
struct ThreadRuntimeRecordsQuery {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    days: Option<i64>,
    #[serde(default)]
    date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AgentUpdateRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default, alias = "modelName", alias = "model_name")]
    model_name: Option<String>,
    #[serde(default)]
    tool_names: Option<Vec<String>>,
    #[serde(default, alias = "abilityItems", alias = "ability_items")]
    ability_items: Option<Vec<AbilityDescriptor>>,
    #[serde(default)]
    abilities: Option<AgentAbilitiesRequest>,
    #[serde(default, alias = "declaredToolNames", alias = "declared_tool_names")]
    declared_tool_names: Option<Vec<String>>,
    #[serde(default, alias = "declaredSkillNames", alias = "declared_skill_names")]
    declared_skill_names: Option<Vec<String>>,
    #[serde(default, alias = "presetQuestions", alias = "preset_questions")]
    preset_questions: Option<Vec<String>>,
    #[serde(default)]
    is_shared: Option<bool>,
    #[serde(default)]
    status: Option<String>,
    #[serde(
        default,
        alias = "approvalMode",
        alias = "approval_mode",
        alias = "permissionLevel",
        alias = "permission_level"
    )]
    approval_mode: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    sandbox_container_id: Option<i32>,
    #[serde(
        default,
        alias = "hiveId",
        alias = "beeroomGroupId",
        alias = "beeroom_group_id"
    )]
    hive_id: Option<String>,
    #[serde(
        default,
        alias = "hiveName",
        alias = "beeroomGroupName",
        alias = "beeroom_group_name"
    )]
    hive_name: Option<String>,
    #[serde(
        default,
        alias = "hiveDescription",
        alias = "beeroomGroupDescription",
        alias = "beeroom_group_description"
    )]
    hive_description: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{
        default_agent_payload, requested_create_ability_items, AgentCreateRequest,
        DefaultAgentConfig,
    };
    use serde_json::json;
    use std::collections::HashSet;

    #[test]
    fn create_request_reads_top_level_ability_items() {
        let payload: AgentCreateRequest = serde_json::from_value(json!({
            "name": "demo",
            "ability_items": [{
                "id": "builtin:read_file",
                "name": "read_file",
                "runtime_name": "read_file",
                "display_name": "read_file",
                "description": "",
                "input_schema": {},
                "group": "builtin",
                "source": "builtin",
                "kind": "tool"
            }]
        }))
        .expect("parse payload");
        let items = requested_create_ability_items(&payload).expect("ability items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].runtime_name, "read_file");
    }

    #[test]
    fn create_request_reads_nested_ability_items() {
        let payload: AgentCreateRequest = serde_json::from_value(json!({
            "name": "demo",
            "abilities": {
                "items": [{
                    "id": "skill:planner",
                    "name": "planner",
                    "runtime_name": "planner",
                    "display_name": "planner",
                    "description": "",
                    "input_schema": {},
                    "group": "skills",
                    "source": "skill",
                    "kind": "skill"
                }]
            }
        }))
        .expect("parse payload");
        let items = requested_create_ability_items(&payload).expect("ability items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].runtime_name, "planner");
    }

    #[test]
    fn default_agent_payload_keeps_declared_dependencies() {
        let payload = default_agent_payload(
            &DefaultAgentConfig {
                name: "Default Agent".to_string(),
                description: String::new(),
                system_prompt: String::new(),
                ability_items: Vec::new(),
                tool_names: vec!["read_file".to_string(), "planner".to_string()],
                declared_tool_names: vec!["read_file".to_string()],
                declared_skill_names: vec!["planner".to_string()],
                preset_questions: Vec::new(),
                approval_mode: "full_auto".to_string(),
                status: "active".to_string(),
                icon: None,
                sandbox_container_id: 1,
                created_at: 1.0,
                updated_at: 1.0,
            },
            None,
            &HashSet::from(["planner".to_string()]),
        );

        assert_eq!(payload["tool_names"], json!(["read_file", "planner"]));
        assert_eq!(payload["declared_tool_names"], json!(["read_file"]));
        assert_eq!(payload["declared_skill_names"], json!(["planner"]));
    }
}
