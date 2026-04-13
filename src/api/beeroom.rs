use crate::api::user_context::resolve_user;
use crate::services::swarm::beeroom::{
    claim_mother_agent, collect_agent_activity, get_mother_agent_id, mother_meta_key,
    resolve_preferred_mother_agent_id, set_mother_agent, snapshot_team_run,
};
use crate::state::AppState;
use crate::storage::{normalize_hive_id, HiveRecord, UserAgentRecord, DEFAULT_HIVE_ID};
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::{routing::get, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/beeroom/groups",
            get(list_beeroom_groups).post(create_beeroom_group),
        )
        .route(
            "/wunder/beeroom/groups/{group_id}",
            get(get_beeroom_group)
                .put(update_beeroom_group)
                .delete(delete_beeroom_group),
        )
        .route(
            "/wunder/beeroom/groups/{group_id}/move_agents",
            axum::routing::post(move_agents_to_group),
        )
        .route(
            "/wunder/beeroom/groups/{group_id}/missions",
            get(list_beeroom_missions),
        )
        .route(
            "/wunder/beeroom/groups/{group_id}/missions/{mission_id}",
            get(get_beeroom_mission),
        )
}

async fn list_beeroom_groups(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(query): Query<ListBeeroomGroupsQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    state
        .user_store
        .ensure_default_hive(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let include_archived = query.include_archived.unwrap_or(false);
    let groups = state
        .user_store
        .list_hives(&user_id, include_archived)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let limit = query.mission_limit.unwrap_or(10).clamp(1, 50);
    let mut items = Vec::with_capacity(groups.len());
    for group in groups {
        items.push(group_payload(state.as_ref(), &group, limit)?);
    }
    Ok(Json(
        json!({ "data": { "items": items, "total": items.len() } }),
    ))
}

async fn create_beeroom_group(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<CreateBeeroomGroupRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    state
        .user_store
        .ensure_default_hive(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let name = payload.name.trim();
    if name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "beeroom name is required".to_string(),
        ));
    }
    let mut hive_id = payload
        .group_id
        .as_deref()
        .map(normalize_hive_id)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| normalize_hive_id(name));
    if hive_id == DEFAULT_HIVE_ID {
        hive_id = format!("beeroom-{}", Uuid::new_v4().simple());
    }
    if state
        .user_store
        .get_hive(&user_id, &hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .is_some()
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            format!("beeroom group {hive_id} already exists"),
        ));
    }

    let now = now_ts();
    let record = HiveRecord {
        hive_id,
        user_id: user_id.clone(),
        name: name.to_string(),
        description: payload.description.unwrap_or_default(),
        is_default: false,
        status: "active".to_string(),
        created_time: now,
        updated_time: now,
    };
    state
        .user_store
        .upsert_hive(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    if let Some(mother_agent_id) = payload
        .mother_agent_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let agent = state
            .user_store
            .get_user_agent(&user_id, mother_agent_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        if agent.is_some() {
            state
                .user_store
                .move_agents_to_hive(&user_id, &record.hive_id, &[mother_agent_id.to_string()])
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            claim_mother_agent(
                state.storage.as_ref(),
                &user_id,
                &record.hive_id,
                mother_agent_id,
            )
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
    }

    Ok(Json(
        json!({ "data": group_payload(state.as_ref(), &record, 10)? }),
    ))
}

async fn get_beeroom_group(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(group_id): AxumPath<String>,
    Query(query): Query<ListBeeroomGroupsQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let group = load_group(state.as_ref(), &user_id, &group_id)?;
    let mission_limit = query.mission_limit.unwrap_or(20).clamp(1, 100);
    let missions = load_group_missions(state.as_ref(), &user_id, &group.hive_id, mission_limit)?;
    let agents = state
        .user_store
        .list_user_agents_by_hive_with_default(&user_id, &group.hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let activity = collect_agent_activity(
        state.storage.as_ref(),
        Some(state.monitor.as_ref()),
        &user_id,
        &group.hive_id,
        &agents,
    )
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "group": group_payload(state.as_ref(), &group, mission_limit)?,
            "agents": agents
                .iter()
                .map(|agent| agent_payload(agent, activity.get(&agent.agent_id)))
                .collect::<Vec<_>>(),
            "missions": missions,
        }
    })))
}

async fn update_beeroom_group(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(group_id): AxumPath<String>,
    Json(payload): Json<UpdateBeeroomGroupRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    state
        .user_store
        .ensure_default_hive(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let mut group = load_group(state.as_ref(), &user_id, &group_id)?;
    let name = payload.name.trim();
    if name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "beeroom name is required".to_string(),
        ));
    }

    group.name = name.to_string();
    group.description = payload.description.unwrap_or_default();
    group.updated_time = now_ts();
    state
        .user_store
        .upsert_hive(&group)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    match payload.mother_agent_id.as_deref().map(str::trim) {
        Some("") => {
            state
                .storage
                .set_meta(&mother_meta_key(&user_id, &group.hive_id), "")
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
        Some(mother_agent_id) => {
            state
                .user_store
                .get_user_agent(&user_id, mother_agent_id)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
                .ok_or_else(|| {
                    error_response(StatusCode::BAD_REQUEST, "mother agent not found".to_string())
                })?;
            state
                .user_store
                .move_agents_to_hive(&user_id, &group.hive_id, &[mother_agent_id.to_string()])
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            set_mother_agent(state.storage.as_ref(), &user_id, &group.hive_id, mother_agent_id)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
        None => {}
    }

    Ok(Json(json!({ "data": group_payload(state.as_ref(), &group, 10)? })))
}

async fn delete_beeroom_group(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(group_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    state
        .user_store
        .ensure_default_hive(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let group = load_group(state.as_ref(), &user_id, &group_id)?;
    if group.is_default || normalize_hive_id(&group.hive_id) == DEFAULT_HIVE_ID {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "default beeroom group cannot be deleted".to_string(),
        ));
    }

    let member_ids = state
        .user_store
        .list_user_agents_by_hive(&user_id, &group.hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .into_iter()
        .map(|agent| agent.agent_id)
        .collect::<Vec<_>>();
    let reset_agent_total = if member_ids.is_empty() {
        0
    } else {
        state
            .user_store
            .move_agents_to_hive(&user_id, DEFAULT_HIVE_ID, &member_ids)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    };
    let deleted_mission_total = state
        .user_store
        .delete_team_runs_by_hive(&user_id, &group.hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let deleted_chat_message_total = state
        .user_store
        .delete_beeroom_chat_messages(&user_id, &group.hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let deleted = state
        .user_store
        .delete_hive(&user_id, &group.hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if deleted <= 0 {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "beeroom group not found".to_string(),
        ));
    }
    if deleted_chat_message_total > 0 {
        state
            .projection
            .beeroom
            .publish_chat_cleared(
                &user_id,
                &group.hive_id,
                deleted_chat_message_total,
                now_ts(),
            )
            .await;
    }
    Ok(Json(json!({
        "data": {
            "deleted": deleted,
            "group_id": group.hive_id,
            "reset_agent_total": reset_agent_total,
            "deleted_mission_total": deleted_mission_total,
            "deleted_chat_message_total": deleted_chat_message_total,
            "fallback_group_id": DEFAULT_HIVE_ID,
        }
    })))
}

async fn move_agents_to_group(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(group_id): AxumPath<String>,
    Json(payload): Json<MoveAgentsRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let group = load_group(state.as_ref(), &user_id, &group_id)?;
    let agent_ids = payload
        .agent_ids
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if agent_ids.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "agent_ids is empty".to_string(),
        ));
    }
    let moved = state
        .user_store
        .move_agents_to_hive(&user_id, &group.hive_id, &agent_ids)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(
        json!({ "data": { "moved": moved, "group_id": group.hive_id } }),
    ))
}

async fn list_beeroom_missions(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(group_id): AxumPath<String>,
    Query(query): Query<ListMissionsQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let group = load_group(state.as_ref(), &user_id, &group_id)?;
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let offset = query.offset.unwrap_or(0).max(0);
    let (runs, total) = state
        .user_store
        .list_team_runs(&user_id, Some(&group.hive_id), None, offset, limit)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut items = Vec::with_capacity(runs.len());
    for run in runs {
        let snapshot =
            snapshot_team_run(state.storage.as_ref(), Some(state.monitor.as_ref()), &run)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        items.push(mission_payload(&snapshot));
    }
    Ok(Json(json!({ "data": { "items": items, "total": total } })))
}

async fn get_beeroom_mission(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath((group_id, mission_id)): AxumPath<(String, String)>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let group = load_group(state.as_ref(), &user_id, &group_id)?;
    let run = state
        .user_store
        .get_team_run(mission_id.trim())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "mission not found".to_string()))?;
    if run.user_id != user_id
        || normalize_hive_id(&run.hive_id) != normalize_hive_id(&group.hive_id)
    {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "mission not found".to_string(),
        ));
    }
    let snapshot = snapshot_team_run(state.storage.as_ref(), Some(state.monitor.as_ref()), &run)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": mission_payload(&snapshot) })))
}

pub(crate) fn load_group(
    state: &AppState,
    user_id: &str,
    group_id: &str,
) -> Result<HiveRecord, Response> {
    let normalized = normalize_hive_id(group_id);
    if normalized == DEFAULT_HIVE_ID {
        return state
            .user_store
            .ensure_default_hive(user_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()));
    }
    state
        .user_store
        .get_hive(user_id, &normalized)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "beeroom group not found".to_string()))
}

fn load_group_missions(
    state: &AppState,
    user_id: &str,
    hive_id: &str,
    limit: i64,
) -> Result<Vec<Value>, Response> {
    let (runs, _) = state
        .user_store
        .list_team_runs(user_id, Some(hive_id), None, 0, limit)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut items = Vec::with_capacity(runs.len());
    for run in runs {
        let snapshot =
            snapshot_team_run(state.storage.as_ref(), Some(state.monitor.as_ref()), &run)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        items.push(mission_payload(&snapshot));
    }
    Ok(items)
}

fn group_payload(
    state: &AppState,
    group: &HiveRecord,
    mission_limit: i64,
) -> Result<Value, Response> {
    let agents = state
        .user_store
        .list_user_agents_by_hive_with_default(&group.user_id, &group.hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let activity = collect_agent_activity(
        state.storage.as_ref(),
        Some(state.monitor.as_ref()),
        &group.user_id,
        &group.hive_id,
        &agents,
    )
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let missions = load_group_missions(state, &group.user_id, &group.hive_id, mission_limit)?;
    let mother_agent_id = get_mother_agent_id(state.storage.as_ref(), &group.user_id, &group.hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .or_else(|| {
            resolve_preferred_mother_agent_id(
                state.storage.as_ref(),
                &group.user_id,
                &group.hive_id,
                None,
            )
            .ok()
            .flatten()
        });
    let mother_agent = mother_agent_id
        .as_deref()
        .and_then(|agent_id| agents.iter().find(|agent| agent.agent_id == agent_id));
    let active_agent_total = agents
        .iter()
        .filter(|agent| {
            activity
                .get(&agent.agent_id)
                .is_some_and(|item| !item.is_idle())
        })
        .count();
    let running_mission_total = missions
        .iter()
        .filter(|item| {
            !matches!(
                item.get("completion_status")
                    .and_then(Value::as_str)
                    .unwrap_or("running"),
                "completed" | "failed" | "cancelled"
            )
        })
        .count();

    Ok(json!({
        "group_id": group.hive_id,
        "hive_id": group.hive_id,
        "name": group.name,
        "description": group.description,
        "status": group.status,
        "is_default": group.is_default,
        "created_time": group.created_time,
        "updated_time": group.updated_time,
        "agent_total": agents.len(),
        "active_agent_total": active_agent_total,
        "idle_agent_total": agents.len().saturating_sub(active_agent_total),
        "running_mission_total": running_mission_total,
        "mission_total": missions.len(),
        "mother_agent_id": mother_agent_id,
        "mother_agent_name": mother_agent.map(|agent| agent.name.clone()),
        "members": agents
            .iter()
            .take(6)
            .map(|agent| agent_payload(agent, activity.get(&agent.agent_id)))
            .collect::<Vec<_>>(),
        "latest_mission": missions.first().cloned(),
    }))
}

fn mission_payload(snapshot: &crate::services::swarm::beeroom::TeamRunSnapshot) -> Value {
    json!({
        "team_run_id": snapshot.run.team_run_id,
        "mission_id": snapshot.run.team_run_id,
        "hive_id": snapshot.run.hive_id,
        "parent_session_id": snapshot.run.parent_session_id,
        "entry_agent_id": snapshot.run.parent_agent_id,
        "mother_agent_id": snapshot.run.mother_agent_id,
        "strategy": snapshot.run.strategy,
        "status": snapshot.run.status,
        "completion_status": snapshot.completion_status,
        "task_total": snapshot.run.task_total,
        "task_success": snapshot.run.task_success,
        "task_failed": snapshot.run.task_failed,
        "context_tokens_total": snapshot.run.context_tokens_total,
        "context_tokens_peak": snapshot.run.context_tokens_peak,
        "model_round_total": snapshot.run.model_round_total,
        "started_time": snapshot.run.started_time,
        "finished_time": snapshot.run.finished_time,
        "elapsed_s": snapshot.run.elapsed_s,
        "summary": snapshot.run.summary,
        "error": snapshot.run.error,
        "updated_time": snapshot.run.updated_time,
        "all_tasks_terminal": snapshot.all_tasks_terminal,
        "all_agents_idle": snapshot.all_agents_idle,
        "active_agent_ids": snapshot.active_agent_ids,
        "idle_agent_ids": snapshot.idle_agent_ids,
        "tasks": snapshot
            .tasks
            .iter()
            .map(|task| {
                json!({
                    "task_id": task.task_id,
                    "agent_id": task.agent_id,
                    "target_session_id": task.target_session_id,
                    "spawned_session_id": task.spawned_session_id,
                    "session_run_id": task.session_run_id,
                    "status": task.status,
                    "priority": task.priority,
                    "started_time": task.started_time,
                    "finished_time": task.finished_time,
                    "elapsed_s": task.elapsed_s,
                    "result_summary": task.result_summary,
                    "error": task.error,
                    "updated_time": task.updated_time,
                })
            })
            .collect::<Vec<_>>(),
    })
}

fn agent_payload(
    agent: &UserAgentRecord,
    activity: Option<&crate::services::swarm::beeroom::AgentActivitySnapshot>,
) -> Value {
    let active_session_ids = activity
        .map(crate::services::swarm::beeroom::AgentActivitySnapshot::active_session_ids)
        .unwrap_or_default();
    json!({
        "agent_id": agent.agent_id,
        "name": agent.name,
        "description": agent.description,
        "status": agent.status,
        "hive_id": agent.hive_id,
        "icon": agent.icon,
        "is_shared": agent.is_shared,
        "approval_mode": agent.approval_mode,
        "tool_names": agent.tool_names,
        "sandbox_container_id": agent.sandbox_container_id,
        "silent": agent.silent,
        "prefer_mother": agent.prefer_mother,
        "active_session_total": active_session_ids.len(),
        "active_session_ids": active_session_ids,
        "idle": activity.is_none_or(|item| item.is_idle()),
    })
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

#[derive(Debug, Deserialize)]
struct ListBeeroomGroupsQuery {
    #[serde(default)]
    include_archived: Option<bool>,
    #[serde(default)]
    mission_limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct CreateBeeroomGroupRequest {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default, alias = "groupId", alias = "hive_id", alias = "hiveId")]
    group_id: Option<String>,
    #[serde(default, alias = "motherAgentId", alias = "mother_agent_id")]
    mother_agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateBeeroomGroupRequest {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default, alias = "motherAgentId", alias = "mother_agent_id")]
    mother_agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MoveAgentsRequest {
    #[serde(default, alias = "agentIds", alias = "agent_ids")]
    agent_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ListMissionsQuery {
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}
