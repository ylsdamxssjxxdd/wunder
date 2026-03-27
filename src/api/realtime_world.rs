use crate::api::errors::error_response;
use crate::api::user_context::resolve_user;
use crate::services::directory::RouteTargetKind;
use crate::services::presence::ProjectionTargetKind;
use crate::state::AppState;
use crate::user_store::UserStore;
use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::{routing::get, Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/realtime/metrics", get(realtime_metrics))
        .route(
            "/wunder/realtime/missions/{team_run_id}",
            get(mission_realtime_snapshot),
        )
        .route(
            "/wunder/realtime/sessions/{session_id}",
            get(session_realtime_snapshot),
        )
}

async fn realtime_metrics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    if !UserStore::is_admin(&resolved.user) {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            "realtime metrics require admin access",
        ));
    }
    let now = now_ts();
    Ok(Json(json!({
        "data": {
            "presence": {
                "projection_watch_metrics": state.control.presence.projection_watch_metrics(now),
            },
            "route_leases": state.control.route_leases.metrics_snapshot(),
            "timestamp": now,
        }
    })))
}

async fn session_realtime_snapshot(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let cleaned_session_id = session_id.trim();
    ensure_session_realtime_access(&state, &resolved.user, cleaned_session_id)?;
    let now = now_ts();
    let thread_id = format!("thread_{cleaned_session_id}");
    let submit_lease = state
        .control
        .route_leases
        .submit_snapshot(cleaned_session_id);
    let thread_route = state
        .control
        .route_leases
        .route_snapshot(RouteTargetKind::Thread, &thread_id);
    let session_watch = state.control.presence.projection_watch_snapshot(
        ProjectionTargetKind::Session,
        cleaned_session_id,
        now,
    );
    let monitor = state.monitor.get_record(cleaned_session_id).map(|record| {
        json!({
            "status": record.get("status").cloned().unwrap_or(Value::Null),
            "busy": record.get("busy").cloned().unwrap_or(Value::Null),
            "updated_time": record.get("updated_time").cloned().unwrap_or(Value::Null),
            "thread_status": record.get("thread_status").cloned().unwrap_or(Value::Null),
            "active_turn_id": record.get("active_turn_id").cloned().unwrap_or(Value::Null),
            "subscriber_count": record.get("subscriber_count").cloned().unwrap_or(Value::Null),
        })
    });

    Ok(Json(json!({
        "data": {
            "session_id": cleaned_session_id,
            "thread_id": thread_id,
            "submit_lease": submit_lease,
            "thread_route": thread_route,
            "session_watch": session_watch,
            "monitor": monitor,
            "timestamp": now,
        }
    })))
}

async fn mission_realtime_snapshot(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(team_run_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let run = ensure_mission_realtime_access(&state, &resolved.user, team_run_id.trim())?;
    let now = now_ts();
    let mission_route = state
        .control
        .route_leases
        .route_snapshot(RouteTargetKind::Mission, &run.team_run_id);
    let beeroom_group_watch = state.control.presence.projection_watch_snapshot(
        ProjectionTargetKind::BeeroomGroup,
        &run.hive_id,
        now,
    );
    let parent_session_watch = state.control.presence.projection_watch_snapshot(
        ProjectionTargetKind::Session,
        &run.parent_session_id,
        now,
    );
    let tasks = state
        .user_store
        .list_team_tasks(&run.team_run_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    Ok(Json(json!({
        "data": {
            "mission": mission_payload(&run),
            "mission_route": mission_route,
            "beeroom_group_watch": beeroom_group_watch,
            "parent_session_watch": parent_session_watch,
            "task_summary": build_mission_task_summary(&tasks),
            "latest_task": build_latest_task_payload(&tasks),
            "timestamp": now,
        }
    })))
}

fn ensure_session_realtime_access(
    state: &AppState,
    user: &crate::storage::UserAccountRecord,
    session_id: &str,
) -> Result<(), Response> {
    let cleaned_session_id = session_id.trim();
    if cleaned_session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "session_id is required",
        ));
    }
    if UserStore::is_admin(user) {
        let thread_id = format!("thread_{cleaned_session_id}");
        let known = state.monitor.get_record(cleaned_session_id).is_some()
            || state
                .control
                .route_leases
                .submit_snapshot(cleaned_session_id)
                .is_some()
            || state
                .control
                .route_leases
                .route_snapshot(RouteTargetKind::Thread, &thread_id)
                .is_some();
        if known {
            return Ok(());
        }
        return Err(error_response(StatusCode::NOT_FOUND, "session not found"));
    }
    match state
        .user_store
        .get_chat_session(&user.user_id, cleaned_session_id)
    {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(error_response(StatusCode::NOT_FOUND, "session not found")),
        Err(err) => Err(error_response(StatusCode::BAD_REQUEST, err.to_string())),
    }
}

fn ensure_mission_realtime_access(
    state: &AppState,
    user: &crate::storage::UserAccountRecord,
    team_run_id: &str,
) -> Result<crate::storage::TeamRunRecord, Response> {
    let cleaned_team_run_id = team_run_id.trim();
    if cleaned_team_run_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "team_run_id is required",
        ));
    }
    let run = state
        .user_store
        .get_team_run(cleaned_team_run_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "team run not found"))?;
    if UserStore::is_admin(user) || run.user_id == user.user_id {
        return Ok(run);
    }
    Err(error_response(StatusCode::NOT_FOUND, "team run not found"))
}

fn mission_payload(run: &crate::storage::TeamRunRecord) -> Value {
    json!({
        "team_run_id": run.team_run_id,
        "mission_id": run.team_run_id,
        "user_id": run.user_id,
        "hive_id": run.hive_id,
        "parent_session_id": run.parent_session_id,
        "parent_agent_id": run.parent_agent_id,
        "mother_agent_id": run.mother_agent_id,
        "strategy": run.strategy,
        "status": run.status,
        "task_total": run.task_total,
        "task_success": run.task_success,
        "task_failed": run.task_failed,
        "context_tokens_total": run.context_tokens_total,
        "context_tokens_peak": run.context_tokens_peak,
        "model_round_total": run.model_round_total,
        "started_time": run.started_time,
        "finished_time": run.finished_time,
        "elapsed_s": run.elapsed_s,
        "summary": run.summary,
        "error": run.error,
        "updated_time": run.updated_time,
    })
}

fn build_mission_task_summary(tasks: &[crate::storage::TeamTaskRecord]) -> Value {
    let mut queued = 0i64;
    let mut running = 0i64;
    let mut success = 0i64;
    let mut failed = 0i64;
    let mut timeout = 0i64;
    let mut cancelled = 0i64;
    let mut unknown = 0i64;
    let mut highest_priority = 0i64;
    let mut latest_updated_time = 0.0f64;

    for task in tasks {
        highest_priority = highest_priority.max(task.priority);
        latest_updated_time = latest_updated_time.max(task.updated_time);
        match normalize_status(&task.status).as_str() {
            "queued" | "pending" | "ready" => queued += 1,
            "running" | "merging" => running += 1,
            "success" => success += 1,
            "failed" => failed += 1,
            "timeout" => timeout += 1,
            "cancelled" => cancelled += 1,
            _ => unknown += 1,
        }
    }

    json!({
        "total": tasks.len(),
        "queued": queued,
        "running": running,
        "success": success,
        "failed": failed,
        "timeout": timeout,
        "cancelled": cancelled,
        "unknown": unknown,
        "terminal": success + failed + timeout + cancelled,
        "active": queued + running,
        "highest_priority": highest_priority,
        "latest_updated_time": latest_updated_time,
    })
}

fn build_latest_task_payload(tasks: &[crate::storage::TeamTaskRecord]) -> Value {
    let Some(task) = tasks
        .iter()
        .max_by(|left, right| left.updated_time.total_cmp(&right.updated_time))
    else {
        return Value::Null;
    };
    json!({
        "task_id": task.task_id,
        "agent_id": task.agent_id,
        "status": task.status,
        "priority": task.priority,
        "target_session_id": task.target_session_id,
        "spawned_session_id": task.spawned_session_id,
        "session_run_id": task.session_run_id,
        "result_summary": task.result_summary,
        "error": task.error,
        "updated_time": task.updated_time,
    })
}

fn normalize_status(status: &str) -> String {
    status.trim().to_ascii_lowercase()
}

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}
