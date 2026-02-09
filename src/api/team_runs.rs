use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::services::swarm::events::{
    TEAM_FINISH, TEAM_START, TEAM_TASK_DISPATCH, TEAM_TASK_UPDATE,
};
use crate::state::AppState;
use crate::storage::{TeamRunRecord, TeamTaskRecord, DEFAULT_HIVE_ID};
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
            "/wunder/chat/team_runs",
            get(list_team_runs).post(create_team_run),
        )
        .route("/wunder/chat/team_runs/{team_run_id}", get(get_team_run))
        .route(
            "/wunder/chat/team_runs/{team_run_id}/cancel",
            axum::routing::post(cancel_team_run),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/team_runs",
            get(list_team_runs_by_session),
        )
}

async fn create_team_run(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<CreateTeamRunRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    state
        .user_store
        .ensure_default_hive(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let parent_session_id = payload.parent_session_id.trim().to_string();
    if parent_session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.session_required"),
        ));
    }

    let resolved_hive_id = DEFAULT_HIVE_ID.to_string();

    let swarm_config = state.config_store.get().await.tools.swarm.clone();
    let max_parallel_tasks = swarm_config.max_parallel_tasks_per_team.max(1) as i64;
    if payload.tasks.len() as i64 > max_parallel_tasks {
        return Err(error_with_code(
            StatusCode::BAD_REQUEST,
            "SWARM_POLICY_BLOCKED",
            format!(
                "task count {} exceeds max_parallel_tasks_per_team {}",
                payload.tasks.len(),
                max_parallel_tasks
            ),
        ));
    }
    let max_active_runs = swarm_config.max_active_team_runs.max(1) as i64;
    let (recent_runs, _) = state
        .user_store
        .list_team_runs(
            &user_id,
            Some(&resolved_hive_id),
            None,
            0,
            max_active_runs * 4,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let active_runs = recent_runs
        .iter()
        .filter(|run| is_active_team_status(&run.status))
        .count() as i64;
    if active_runs >= max_active_runs {
        return Err(error_with_code(
            StatusCode::TOO_MANY_REQUESTS,
            "SWARM_POLICY_BLOCKED",
            format!(
                "active team runs {} reached max_active_team_runs {}",
                active_runs, max_active_runs
            ),
        ));
    }

    let mut task_total = 0i64;
    let mut tasks = Vec::new();
    for task in payload.tasks {
        let agent_id = task.agent_id.trim().to_string();
        if agent_id.is_empty() {
            continue;
        }
        let _agent = state
            .user_store
            .get_user_agent(&user_id, &agent_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
            .ok_or_else(|| {
                error_with_code(
                    StatusCode::BAD_REQUEST,
                    "SWARM_HIVE_DENIED",
                    format!("agent {agent_id} not found"),
                )
            })?;
        task_total += 1;
        tasks.push((
            agent_id,
            task.target_session_id,
            task.priority.unwrap_or(0).clamp(-100, 100),
        ));
    }

    if task_total <= 0 {
        return Err(error_with_code(
            StatusCode::BAD_REQUEST,
            "SWARM_POLICY_BLOCKED",
            "team run requires at least one valid task".to_string(),
        ));
    }

    let now = now_ts();
    let team_run_id = format!("team_{}", Uuid::new_v4().simple());
    let strategy = payload
        .strategy
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("parallel_all")
        .to_string();
    let timeout_s = payload
        .timeout_s
        .map(|value| value.max(1.0))
        .unwrap_or(swarm_config.default_timeout_s as f64);
    let merge_policy = payload
        .merge_policy
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("collect")
        .to_string();

    let record = TeamRunRecord {
        team_run_id: team_run_id.clone(),
        user_id: user_id.clone(),
        hive_id: resolved_hive_id.clone(),
        parent_session_id: parent_session_id.clone(),
        parent_agent_id: resolve_parent_agent_id(&state, &user_id, &parent_session_id),
        strategy,
        status: "queued".to_string(),
        task_total,
        task_success: 0,
        task_failed: 0,
        context_tokens_total: 0,
        context_tokens_peak: 0,
        model_round_total: 0,
        started_time: Some(now),
        finished_time: None,
        elapsed_s: None,
        summary: Some(format!(
            "merge_policy={merge_policy}; timeout_s={timeout_s:.0}"
        )),
        error: None,
        updated_time: now,
    };
    state
        .user_store
        .upsert_team_run(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    emit_team_event(
        state.as_ref(),
        &parent_session_id,
        TEAM_START,
        json!({
            "team_run_id": team_run_id,
            "hive_id": resolved_hive_id,
            "task_total": task_total,
            "strategy": record.strategy,
            "merge_policy": merge_policy,
            "timeout_s": timeout_s,
        }),
    );

    for (agent_id, target_session_id, priority) in tasks {
        let task_record = TeamTaskRecord {
            task_id: format!("task_{}", Uuid::new_v4().simple()),
            team_run_id: record.team_run_id.clone(),
            user_id: user_id.clone(),
            hive_id: record.hive_id.clone(),
            agent_id,
            target_session_id,
            spawned_session_id: None,
            status: "queued".to_string(),
            retry_count: 0,
            priority,
            started_time: None,
            finished_time: None,
            elapsed_s: None,
            result_summary: None,
            error: None,
            updated_time: now,
        };
        state
            .user_store
            .upsert_team_task(&task_record)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        emit_team_event(
            state.as_ref(),
            &parent_session_id,
            TEAM_TASK_DISPATCH,
            json!({
                "team_run_id": task_record.team_run_id,
                "task_id": task_record.task_id,
                "hive_id": task_record.hive_id,
                "agent_id": task_record.agent_id,
                "priority": task_record.priority,
                "status": task_record.status,
            }),
        );
    }

    Ok(Json(json!({
        "data": {
            "team_run_id": record.team_run_id,
            "hive_id": DEFAULT_HIVE_ID,
            "task_total": record.task_total,
            "status": record.status,
        }
    })))
}

async fn get_team_run(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(team_run_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let run = state
        .user_store
        .get_team_run(team_run_id.trim())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "team run not found".to_string()))?;
    if run.user_id != user_id {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "team run not found".to_string(),
        ));
    }
    let tasks = state
        .user_store
        .list_team_tasks(&run.team_run_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "run": team_run_payload(&run),
            "tasks": tasks
                .into_iter()
                .map(|item| team_task_payload(&item))
                .collect::<Vec<_>>(),
        }
    })))
}

async fn cancel_team_run(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(team_run_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let mut run = state
        .user_store
        .get_team_run(team_run_id.trim())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "team run not found".to_string()))?;
    if run.user_id != user_id {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "team run not found".to_string(),
        ));
    }

    let now = now_ts();
    run.status = "cancelled".to_string();
    run.finished_time = Some(now);
    run.elapsed_s = run.started_time.map(|start| (now - start).max(0.0));
    run.updated_time = now;
    state
        .user_store
        .upsert_team_run(&run)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let tasks = state
        .user_store
        .list_team_tasks(&run.team_run_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    for mut task in tasks {
        if matches!(task.status.as_str(), "success" | "failed" | "cancelled") {
            continue;
        }
        task.status = "cancelled".to_string();
        task.updated_time = now;
        task.finished_time = Some(now);
        task.elapsed_s = task.started_time.map(|start| (now - start).max(0.0));
        state
            .user_store
            .upsert_team_task(&task)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        emit_team_event(
            state.as_ref(),
            &run.parent_session_id,
            TEAM_TASK_UPDATE,
            json!({
                "team_run_id": task.team_run_id,
                "task_id": task.task_id,
                "hive_id": task.hive_id,
                "agent_id": task.agent_id,
                "status": task.status,
            }),
        );
    }

    emit_team_event(
        state.as_ref(),
        &run.parent_session_id,
        TEAM_FINISH,
        json!({
            "team_run_id": run.team_run_id,
            "hive_id": run.hive_id,
            "status": run.status,
            "updated_time": run.updated_time,
        }),
    );

    Ok(Json(json!({ "data": team_run_payload(&run) })))
}

async fn list_team_runs(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(query): Query<ListTeamRunsQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let parent_session_id = query
        .parent_session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let offset = query.offset.unwrap_or(0).max(0);
    let (runs, total) = state
        .user_store
        .list_team_runs(
            &user_id,
            None,
            parent_session_id,
            offset,
            limit,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "total": total,
            "items": runs.into_iter().map(|run| team_run_payload(&run)).collect::<Vec<_>>(),
        }
    })))
}

async fn list_team_runs_by_session(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<ListTeamRunsQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0).max(0);
    let (runs, total) = state
        .user_store
        .list_team_runs(
            &user_id,
            None,
            Some(session_id.trim()),
            offset,
            limit,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "total": total,
            "items": runs.into_iter().map(|run| team_run_payload(&run)).collect::<Vec<_>>(),
        }
    })))
}

fn resolve_parent_agent_id(
    state: &AppState,
    user_id: &str,
    parent_session_id: &str,
) -> Option<String> {
    state
        .user_store
        .get_chat_session(user_id, parent_session_id)
        .ok()
        .flatten()
        .and_then(|session| session.agent_id)
}

fn is_active_team_status(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "queued" | "running" | "merging"
    )
}

fn emit_team_event(state: &AppState, session_id: &str, event_type: &str, payload: Value) {
    let cleaned_session_id = session_id.trim();
    if cleaned_session_id.is_empty() {
        return;
    }
    state
        .monitor
        .record_event(cleaned_session_id, event_type, &payload);
}

fn team_run_payload(record: &TeamRunRecord) -> Value {
    json!({
        "team_run_id": record.team_run_id,
        "user_id": record.user_id,
        "hive_id": DEFAULT_HIVE_ID,
        "parent_session_id": record.parent_session_id,
        "parent_agent_id": record.parent_agent_id,
        "strategy": record.strategy,
        "status": record.status,
        "task_total": record.task_total,
        "task_success": record.task_success,
        "task_failed": record.task_failed,
        "context_tokens_total": record.context_tokens_total,
        "context_tokens_peak": record.context_tokens_peak,
        "model_round_total": record.model_round_total,
        "started_time": record.started_time,
        "finished_time": record.finished_time,
        "elapsed_s": record.elapsed_s,
        "summary": record.summary,
        "error": record.error,
        "updated_time": record.updated_time,
    })
}

fn team_task_payload(record: &TeamTaskRecord) -> Value {
    json!({
        "task_id": record.task_id,
        "team_run_id": record.team_run_id,
        "user_id": record.user_id,
        "hive_id": DEFAULT_HIVE_ID,
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

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}

fn error_with_code(status: StatusCode, code: &str, message: String) -> Response {
    crate::api::errors::error_response_with_detail(status, Some(code), message, None, None)
}

#[derive(Debug, Deserialize)]
struct CreateTeamRunRequest {
    parent_session_id: String,
    #[serde(default)]
    strategy: Option<String>,
    #[serde(default)]
    merge_policy: Option<String>,
    #[serde(default)]
    timeout_s: Option<f64>,
    #[serde(default)]
    tasks: Vec<CreateTeamTaskRequest>,
}

#[derive(Debug, Deserialize)]
struct CreateTeamTaskRequest {
    agent_id: String,
    #[serde(default)]
    target_session_id: Option<String>,
    #[serde(default)]
    priority: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ListTeamRunsQuery {
    #[serde(default)]
    parent_session_id: Option<String>,
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}
