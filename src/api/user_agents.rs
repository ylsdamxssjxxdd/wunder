// 用户智能体 API：创建/管理用户自定义智能体。
use crate::api::user_context::resolve_user;
use crate::config::UserAgentPresetConfig;
use crate::i18n;
use crate::monitor::MonitorState;
use crate::state::AppState;
use crate::storage::{
    normalize_hive_id, normalize_sandbox_container_id, DEFAULT_HIVE_ID,
    DEFAULT_SANDBOX_CONTAINER_ID,
};
use crate::user_access::{
    build_user_tool_context, compute_allowed_tool_names, filter_user_agents_by_access,
    is_agent_allowed,
};
use anyhow::Result;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::{routing::get, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;

const DEFAULT_AGENT_ACCESS_LEVEL: &str = "A";

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/agents", get(list_agents).post(create_agent))
        .route("/wunder/agents/shared", get(list_shared_agents))
        .route("/wunder/agents/running", get(list_running_agents))
        .route(
            "/wunder/agents/{agent_id}",
            get(get_agent).put(update_agent).delete(delete_agent),
        )
        .route(
            "/wunder/agents/{agent_id}/default-session",
            get(get_default_session).post(set_default_session),
        )
}

async fn list_agents(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
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
    let items = filtered.iter().map(agent_payload).collect::<Vec<_>>();
    Ok(Json(
        json!({ "data": { "total": items.len(), "items": items } }),
    ))
}

async fn list_shared_agents(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let agents = state
        .user_store
        .list_shared_user_agents(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let access = state
        .user_store
        .get_user_agent_access(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let filtered = filter_user_agents_by_access(&resolved.user, access.as_ref(), agents);
    let items = filtered.iter().map(agent_payload).collect::<Vec<_>>();
    Ok(Json(
        json!({ "data": { "total": items.len(), "items": items } }),
    ))
}

async fn list_running_agents(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
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

    fn should_replace(current: &AgentStatusCandidate, next: &AgentStatusCandidate) -> bool {
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
    let owned_agents = filter_user_agents_by_access(&resolved.user, access.as_ref(), owned_agents);

    let shared_agents = state
        .user_store
        .list_shared_user_agents(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let shared_agents = filter_user_agents_by_access(&resolved.user, access.as_ref(), shared_agents);

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
            if should_replace(current, &next) {
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
        let session_user_id = record.get("user_id").and_then(Value::as_str).unwrap_or("").trim();
        if session_user_id != user_id {
            continue;
        }
        let agent_id = record.get("agent_id").and_then(Value::as_str).unwrap_or("").trim();
        if !allowed_set.contains(agent_id) {
            continue;
        }
        let status = record.get("status").and_then(Value::as_str).unwrap_or("").trim();
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
            if should_replace(current, &next) {
                status_by_agent.insert(agent_id.to_string(), next);
            }
        }
    }

    // 3) Recently completed/error sessions, used to display a transient state without frontend inference.
    let now = now_ts();
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
        let session_user_id = record.get("user_id").and_then(Value::as_str).unwrap_or("").trim();
        if session_user_id != user_id {
            continue;
        }
        let agent_id = record.get("agent_id").and_then(Value::as_str).unwrap_or("").trim();
        if !allowed_set.contains(agent_id) {
            continue;
        }
        let Some(current) = status_by_agent.get(agent_id) else {
            continue;
        };
        if state_rank(current.state) > state_rank(STATE_IDLE) {
            continue;
        }
        let status = record.get("status").and_then(Value::as_str).unwrap_or("").trim();
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
        status_by_agent.insert(
            agent_id.to_string(),
            AgentStatusCandidate {
                state,
                updated_time,
                session_id,
                expires_at: None,
                pending_question: false,
                last_error,
            },
        );
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
                    map.insert("last_error".to_string(), Value::String(last_error.to_string()));
                }
            }
            payload
        })
        .collect::<Vec<_>>();

    Ok(Json(json!({ "data": { "total": items.len(), "items": items } })))
}

async fn get_agent(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(agent_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let cleaned = agent_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let record = state
        .user_store
        .get_user_agent_by_id(cleaned)
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
    Ok(Json(json!({ "data": agent_payload(&record) })))
}

async fn create_agent(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<AgentCreateRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
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
    let target_hive_id = DEFAULT_HIVE_ID.to_string();

    let copy_from_agent_id = payload
        .copy_from_agent_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let copy_source = if let Some(copy_id) = copy_from_agent_id {
        let source = state
            .user_store
            .get_user_agent(&user_id, copy_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
            .ok_or_else(|| {
                error_response(StatusCode::NOT_FOUND, i18n::t("error.agent_not_found"))
            })?;
        Some(source)
    } else {
        None
    };

    let mut tool_names = if let Some(source) = copy_source.as_ref() {
        source.tool_names.clone()
    } else {
        normalize_tool_list(payload.tool_names.clone())
    };
    if !tool_names.is_empty() {
        let context = build_user_tool_context(&state, &user_id).await;
        let allowed = compute_allowed_tool_names(&resolved.user, &context);
        tool_names = filter_allowed_tools(&tool_names, &allowed);
    }

    let access_level = DEFAULT_AGENT_ACCESS_LEVEL.to_string();
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

    let record = crate::storage::UserAgentRecord {
        agent_id: format!("agent_{}", Uuid::new_v4().simple()),
        user_id: user_id.clone(),
        hive_id: target_hive_id,
        name,
        description,
        system_prompt,
        tool_names,
        access_level,
        is_shared,
        status,
        icon,
        sandbox_container_id,
        created_at: now,
        updated_at: now,
    };
    state
        .user_store
        .upsert_user_agent(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": agent_payload(&record) })))
}

async fn update_agent(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(agent_id): AxumPath<String>,
    Json(payload): Json<AgentUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let cleaned = agent_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let mut record = state
        .user_store
        .get_user_agent(&user_id, cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.agent_not_found")))?;
    if let Some(name) = payload.name {
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
    if let Some(is_shared) = payload.is_shared {
        record.is_shared = is_shared;
    }
    if let Some(tool_names) = payload.tool_names {
        let mut normalized = normalize_tool_list(tool_names);
        if !normalized.is_empty() {
            let context = build_user_tool_context(&state, &user_id).await;
            let allowed = compute_allowed_tool_names(&resolved.user, &context);
            normalized = filter_allowed_tools(&normalized, &allowed);
        }
        record.tool_names = normalized;
    }
    if let Some(status) = payload.status {
        record.status = normalize_agent_status(Some(&status));
    }
    if payload.icon.is_some() {
        record.icon = payload.icon;
    }
    if let Some(sandbox_container_id) = payload.sandbox_container_id {
        record.sandbox_container_id = normalize_sandbox_container_id(sandbox_container_id);
    }
    record.hive_id = DEFAULT_HIVE_ID.to_string();
    record.updated_at = now_ts();
    state
        .user_store
        .upsert_user_agent(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": agent_payload(&record) })))
}

async fn delete_agent(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(agent_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let cleaned = agent_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    state
        .user_store
        .delete_user_agent(&resolved.user.user_id, cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
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
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
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
    Json(payload): Json<DefaultSessionRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
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
        .agent_runtime
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

fn agent_payload(record: &crate::storage::UserAgentRecord) -> Value {
    json!({
        "id": record.agent_id,
        "name": record.name,
        "description": record.description,
        "system_prompt": record.system_prompt,
        "tool_names": record.tool_names,
        "access_level": record.access_level,
        "is_shared": record.is_shared,
        "status": record.status,
        "icon": record.icon,
        "hive_id": DEFAULT_HIVE_ID,
        "sandbox_container_id": normalize_sandbox_container_id(record.sandbox_container_id),
        "created_at": format_ts(record.created_at),
        "updated_at": format_ts(record.updated_at),
    })
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

const PRESET_META_PREFIX: &str = "user_agent_presets_v1:";
const LEGACY_EMAIL_PRESET_NAME: &str = "邮件写作";
const LEGACY_MEETING_NAME: &str = "会议纪要";
const LEGACY_PLAN_NAME: &str = "方案策划";
const PRESET_CONTAINER_ID_SCI_DRAW: i32 = 4;
const PRESET_CONTAINER_ID_POLICY_ANALYSIS: i32 = 5;
const PRESET_CONTAINER_ID_OFFICIAL_WRITING: i32 = 6;
const PRESET_CONTAINER_META_PREFIX: &str = "user_agent_presets_container_v1:";

#[derive(Clone)]
struct PresetAgent {
    name: String,
    description: String,
    system_prompt: String,
    icon_name: String,
    icon_color: String,
    sandbox_container_id: i32,
}

impl PresetAgent {
    fn from_config(config: UserAgentPresetConfig) -> Option<Self> {
        let name = config.name.trim();
        if name.is_empty() {
            return None;
        }
        let icon_name = if config.icon_name.trim().is_empty() {
            "spark".to_string()
        } else {
            config.icon_name.trim().to_string()
        };
        let icon_color = if config.icon_color.trim().is_empty() {
            "#94a3b8".to_string()
        } else {
            config.icon_color.trim().to_string()
        };
        Some(Self {
            name: name.to_string(),
            description: config.description.trim().to_string(),
            system_prompt: config.system_prompt.trim().to_string(),
            icon_name,
            icon_color,
            sandbox_container_id: normalize_sandbox_container_id(config.sandbox_container_id),
        })
    }
}

async fn ensure_preset_agents(
    state: &AppState,
    user: &crate::storage::UserAccountRecord,
) -> Result<(), Response> {
    let meta_key = format!("{PRESET_META_PREFIX}{}", user.user_id);
    let container_meta_key = format!("{PRESET_CONTAINER_META_PREFIX}{}", user.user_id);
    let preset_agents = configured_preset_agents(state).await;
    let preset_name_set = preset_agents
        .iter()
        .map(|preset| preset.name.trim().to_string())
        .collect::<HashSet<_>>();
    let mut existing = state
        .user_store
        .list_user_agents(&user.user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    if !preset_name_set.is_empty() {
        let mut duplicates_by_name: HashMap<String, Vec<crate::storage::UserAgentRecord>> =
            HashMap::new();
        for record in &existing {
            if normalize_hive_id(&record.hive_id) != DEFAULT_HIVE_ID {
                continue;
            }
            let trimmed_name = record.name.trim();
            if trimmed_name.is_empty() || !preset_name_set.contains(trimmed_name) {
                continue;
            }
            duplicates_by_name
                .entry(trimmed_name.to_string())
                .or_default()
                .push(record.clone());
        }

        let mut duplicate_ids = HashSet::new();
        for records in duplicates_by_name.values_mut() {
            if records.len() <= 1 {
                continue;
            }
            records.sort_by(|left, right| right.updated_at.total_cmp(&left.updated_at));
            for duplicate in records.iter().skip(1) {
                duplicate_ids.insert(duplicate.agent_id.clone());
                let _ = state
                    .user_store
                    .delete_user_agent(&user.user_id, &duplicate.agent_id);
            }
        }
        if !duplicate_ids.is_empty() {
            existing.retain(|record| !duplicate_ids.contains(&record.agent_id));
        }
    }

    let mut existing_names: HashSet<String> = existing
        .iter()
        .map(|record| record.name.trim().to_string())
        .collect();
    let now = now_ts();
    let container_layout_seeded = state
        .user_store
        .get_meta(&container_meta_key)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .is_some();
    for record in &existing {
        let trimmed_name = record.name.trim();
        let mut updated = record.clone();
        let mut changed = false;
        if trimmed_name == LEGACY_EMAIL_PRESET_NAME {
            changed = apply_legacy_preset_upgrade(
                &mut updated,
                &preset_agents,
                PRESET_CONTAINER_ID_OFFICIAL_WRITING,
            );
            if changed {
                existing_names.remove(LEGACY_EMAIL_PRESET_NAME);
                existing_names.insert(updated.name.trim().to_string());
            }
        } else if trimmed_name == LEGACY_MEETING_NAME {
            changed = apply_legacy_preset_upgrade(
                &mut updated,
                &preset_agents,
                PRESET_CONTAINER_ID_SCI_DRAW,
            );
            if changed {
                existing_names.remove(LEGACY_MEETING_NAME);
                existing_names.insert(updated.name.trim().to_string());
            }
        } else if trimmed_name == LEGACY_PLAN_NAME {
            changed = apply_legacy_preset_upgrade(
                &mut updated,
                &preset_agents,
                PRESET_CONTAINER_ID_POLICY_ANALYSIS,
            );
            if changed {
                existing_names.remove(LEGACY_PLAN_NAME);
                existing_names.insert(updated.name.trim().to_string());
            }
        }

        if !container_layout_seeded {
            if let Some(container_id) =
                preset_sandbox_container_id(updated.name.trim(), &preset_agents)
            {
                if updated.sandbox_container_id == DEFAULT_SANDBOX_CONTAINER_ID
                    && updated.sandbox_container_id != container_id
                {
                    updated.sandbox_container_id = container_id;
                    changed = true;
                }
            }
        }

        if changed {
            updated.updated_at = now;
            let _ = state.user_store.upsert_user_agent(&updated);
        }
    }
    if !container_layout_seeded {
        let _ = state.user_store.set_meta(&container_meta_key, "1");
    }
    let seeded = state
        .user_store
        .get_meta(&meta_key)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if seeded.is_some() {
        return Ok(());
    }
    let context = build_user_tool_context(state, &user.user_id).await;
    let mut tool_names = compute_allowed_tool_names(user, &context)
        .into_iter()
        .collect::<Vec<_>>();
    tool_names.sort();
    let access_level = DEFAULT_AGENT_ACCESS_LEVEL.to_string();
    for preset in &preset_agents {
        let preset_name = preset.name.trim();
        if existing_names.contains(preset_name) {
            continue;
        }
        let record = crate::storage::UserAgentRecord {
            agent_id: format!("agent_{}", Uuid::new_v4().simple()),
            user_id: user.user_id.clone(),
            hive_id: DEFAULT_HIVE_ID.to_string(),
            name: preset.name.clone(),
            description: preset.description.clone(),
            system_prompt: preset.system_prompt.clone(),
            tool_names: tool_names.clone(),
            access_level: access_level.clone(),
            is_shared: false,
            status: "active".to_string(),
            icon: Some(build_icon_payload(&preset.icon_name, &preset.icon_color)),
            sandbox_container_id: preset.sandbox_container_id,
            created_at: now,
            updated_at: now,
        };
        let _ = state.user_store.upsert_user_agent(&record);
        existing_names.insert(preset_name.to_string());
    }
    let _ = state.user_store.set_meta(&meta_key, "1");
    Ok(())
}

async fn configured_preset_agents(state: &AppState) -> Vec<PresetAgent> {
    let config = state.config_store.get().await;
    let mut seen_names = HashSet::new();
    let mut presets = Vec::new();
    for preset in config.user_agents.presets {
        let Some(preset) = PresetAgent::from_config(preset) else {
            continue;
        };
        if seen_names.insert(preset.name.clone()) {
            presets.push(preset);
        }
    }
    presets
}

fn apply_legacy_preset_upgrade(
    record: &mut crate::storage::UserAgentRecord,
    preset_agents: &[PresetAgent],
    sandbox_container_id: i32,
) -> bool {
    let Some(preset) = preset_agents
        .iter()
        .find(|item| item.sandbox_container_id == sandbox_container_id)
    else {
        return false;
    };
    let mut changed = false;
    if record.name != preset.name {
        record.name = preset.name.clone();
        changed = true;
    }
    if record.description != preset.description {
        record.description = preset.description.clone();
        changed = true;
    }
    if record.system_prompt != preset.system_prompt {
        record.system_prompt = preset.system_prompt.clone();
        changed = true;
    }
    let icon_payload = build_icon_payload(&preset.icon_name, &preset.icon_color);
    if record.icon.as_deref() != Some(icon_payload.as_str()) {
        record.icon = Some(icon_payload);
        changed = true;
    }
    changed
}

fn preset_sandbox_container_id(name: &str, preset_agents: &[PresetAgent]) -> Option<i32> {
    let cleaned = name.trim();
    if cleaned.is_empty() {
        return None;
    }
    preset_agents
        .iter()
        .find(|preset| preset.name == cleaned)
        .map(|preset| preset.sandbox_container_id)
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

fn build_icon_payload(name: &str, color: &str) -> String {
    serde_json::json!({ "name": name, "color": color }).to_string()
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}

#[derive(Debug, Deserialize)]
struct AgentCreateRequest {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default)]
    tool_names: Vec<String>,
    #[serde(default)]
    is_shared: Option<bool>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    sandbox_container_id: Option<i32>,
    #[serde(default, alias = "copyFromAgentId", alias = "copy_from_agent_id")]
    copy_from_agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DefaultSessionRequest {
    session_id: String,
}

#[derive(Debug, Deserialize)]
struct AgentUpdateRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default)]
    tool_names: Option<Vec<String>>,
    #[serde(default)]
    is_shared: Option<bool>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    sandbox_container_id: Option<i32>,
}
