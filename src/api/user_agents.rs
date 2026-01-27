// 用户智能体 API：创建/管理用户自定义智能体。
use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::state::AppState;
use crate::user_access::{
    build_user_tool_context, compute_allowed_tool_names, filter_user_agents_by_access,
    is_agent_allowed,
};
use anyhow::Result;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{routing::get, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;
use uuid::Uuid;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/agents", get(list_agents).post(create_agent))
        .route(
            "/wunder/agents/{agent_id}",
            get(get_agent).put(update_agent).delete(delete_agent),
        )
}

async fn list_agents(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let agents = state
        .user_store
        .list_user_agents(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let access = state
        .user_store
        .get_user_agent_access(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let filtered = filter_user_agents_by_access(&resolved.user, access.as_ref(), agents);
    let items = filtered
        .iter()
        .map(|record| agent_payload(record))
        .collect::<Vec<_>>();
    Ok(Json(
        json!({ "data": { "total": items.len(), "items": items } }),
    ))
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
        .get_user_agent(&user_id, cleaned)
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
    let mut tool_names = normalize_tool_list(payload.tool_names);
    if !tool_names.is_empty() {
        let context = build_user_tool_context(&state, &user_id).await;
        let allowed = compute_allowed_tool_names(&resolved.user, &context);
        tool_names = filter_allowed_tools(&tool_names, &allowed);
    }
    let access_level =
        normalize_agent_level(payload.access_level.as_deref(), &resolved.user.access_level);
    let status = normalize_agent_status(payload.status.as_deref());
    let now = now_ts();
    let record = crate::storage::UserAgentRecord {
        agent_id: format!("agent_{}", Uuid::new_v4().simple()),
        user_id: user_id.clone(),
        name,
        description: payload.description.unwrap_or_default(),
        system_prompt: payload.system_prompt.unwrap_or_default(),
        tool_names,
        access_level,
        status,
        icon: payload.icon,
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
    if let Some(tool_names) = payload.tool_names {
        let mut normalized = normalize_tool_list(tool_names);
        if !normalized.is_empty() {
            let context = build_user_tool_context(&state, &user_id).await;
            let allowed = compute_allowed_tool_names(&resolved.user, &context);
            normalized = filter_allowed_tools(&normalized, &allowed);
        }
        record.tool_names = normalized;
    }
    if let Some(access_level) = payload.access_level {
        record.access_level =
            normalize_agent_level(Some(&access_level), &resolved.user.access_level);
    }
    if let Some(status) = payload.status {
        record.status = normalize_agent_status(Some(&status));
    }
    if payload.icon.is_some() {
        record.icon = payload.icon;
    }
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
    Ok(Json(json!({ "data": { "id": cleaned } })))
}

fn agent_payload(record: &crate::storage::UserAgentRecord) -> Value {
    json!({
        "id": record.agent_id,
        "name": record.name,
        "description": record.description,
        "system_prompt": record.system_prompt,
        "tool_names": record.tool_names,
        "access_level": record.access_level,
        "status": record.status,
        "icon": record.icon,
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

fn normalize_agent_level(requested: Option<&str>, user_level: &str) -> String {
    let user_level = normalize_access_level(user_level);
    let desired = normalize_access_level(requested.unwrap_or(&user_level));
    if access_level_rank(&desired) > access_level_rank(&user_level) {
        user_level
    } else {
        desired
    }
}

fn normalize_agent_status(raw: Option<&str>) -> String {
    let status = raw.unwrap_or("active").trim();
    if status.is_empty() {
        "active".to_string()
    } else {
        status.to_string()
    }
}

fn normalize_access_level(raw: &str) -> String {
    let level = raw.trim().to_uppercase();
    if level == "B" || level == "C" {
        level
    } else {
        "A".to_string()
    }
}

fn access_level_rank(level: &str) -> i32 {
    match level.trim().to_uppercase().as_str() {
        "C" => 1,
        "B" => 2,
        _ => 3,
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

fn error_response(status: StatusCode, message: String) -> Response {
    (status, Json(json!({ "detail": { "message": message } }))).into_response()
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
    access_level: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    icon: Option<String>,
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
    access_level: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    icon: Option<String>,
}
