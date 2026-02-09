use crate::auth as guard_auth;
use crate::state::AppState;
use crate::storage::normalize_hive_id;
use crate::user_store::UserStore;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::{routing::get, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/admin/team_runs", get(list_team_runs))
        .route("/wunder/admin/team_runs/{team_run_id}", get(get_team_run))
        .route(
            "/wunder/admin/hives/{hive_id}/team_runs",
            get(list_hive_team_runs),
        )
}

async fn list_team_runs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<AdminTeamRunsQuery>,
) -> Result<Json<Value>, Response> {
    ensure_admin(&state, &headers)?;
    let user_id = query
        .user_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            error_response(
                StatusCode::BAD_REQUEST,
                "admin list team runs requires user_id".to_string(),
            )
        })?;
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let offset = query.offset.unwrap_or(0).max(0);
    let hive_id = query.hive_id.as_deref().map(normalize_hive_id);
    let (items, total) = state
        .user_store
        .list_team_runs(
            user_id,
            hive_id.as_deref(),
            query.parent_session_id.as_deref(),
            offset,
            limit,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "total": total,
            "items": items.into_iter().map(|run| admin_run_payload(&run)).collect::<Vec<_>>(),
        }
    })))
}

async fn get_team_run(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(team_run_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    ensure_admin(&state, &headers)?;
    let run = state
        .user_store
        .get_team_run(team_run_id.trim())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "team run not found".to_string()))?;
    let tasks = state
        .user_store
        .list_team_tasks(&run.team_run_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "run": admin_run_payload(&run),
            "tasks": tasks
                .into_iter()
                .map(|task| admin_task_payload(&task))
                .collect::<Vec<_>>(),
        }
    })))
}

async fn list_hive_team_runs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(hive_id): AxumPath<String>,
    Query(query): Query<AdminHiveTeamRunsQuery>,
) -> Result<Json<Value>, Response> {
    ensure_admin(&state, &headers)?;
    let user_id = query
        .user_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            error_response(
                StatusCode::BAD_REQUEST,
                "admin hive team runs requires user_id".to_string(),
            )
        })?;
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let offset = query.offset.unwrap_or(0).max(0);
    let normalized_hive_id = normalize_hive_id(&hive_id);
    let (items, total) = state
        .user_store
        .list_team_runs(
            user_id,
            Some(&normalized_hive_id),
            query.parent_session_id.as_deref(),
            offset,
            limit,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "hive_id": normalized_hive_id,
            "total": total,
            "items": items.into_iter().map(|run| admin_run_payload(&run)).collect::<Vec<_>>(),
        }
    })))
}

fn admin_run_payload(record: &crate::storage::TeamRunRecord) -> Value {
    json!({
        "team_run_id": record.team_run_id,
        "user_id": record.user_id,
        "hive_id": normalize_hive_id(&record.hive_id),
        "parent_session_id": record.parent_session_id,
        "status": record.status,
        "strategy": record.strategy,
        "task_total": record.task_total,
        "task_success": record.task_success,
        "task_failed": record.task_failed,
        "context_tokens_total": record.context_tokens_total,
        "context_tokens_peak": record.context_tokens_peak,
        "model_round_total": record.model_round_total,
        "started_time": record.started_time,
        "finished_time": record.finished_time,
        "elapsed_s": record.elapsed_s,
        "updated_time": record.updated_time,
    })
}
fn admin_task_payload(record: &crate::storage::TeamTaskRecord) -> Value {
    json!({
        "task_id": record.task_id,
        "team_run_id": record.team_run_id,
        "user_id": record.user_id,
        "hive_id": normalize_hive_id(&record.hive_id),
        "agent_id": record.agent_id,
        "target_session_id": record.target_session_id,
        "spawned_session_id": record.spawned_session_id,
        "status": record.status,
        "retry_count": record.retry_count,
        "priority": record.priority,
        "started_time": record.started_time,
        "finished_time": record.finished_time,
        "elapsed_s": record.elapsed_s,
        "result_summary": record.result_summary,
        "error": record.error,
        "updated_time": record.updated_time,
    })
}

fn ensure_admin(state: &AppState, headers: &HeaderMap) -> Result<(), Response> {
    let Some(token) = guard_auth::extract_bearer_token(headers) else {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            "auth required".to_string(),
        ));
    };
    let user = state
        .user_store
        .authenticate_token(&token)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::UNAUTHORIZED, "auth required".to_string()))?;
    if !UserStore::is_admin(&user) {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            "admin required".to_string(),
        ));
    }
    Ok(())
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}

#[derive(Debug, Deserialize)]
struct AdminTeamRunsQuery {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    hive_id: Option<String>,
    #[serde(default)]
    parent_session_id: Option<String>,
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct AdminHiveTeamRunsQuery {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    parent_session_id: Option<String>,
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}
