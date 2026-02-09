use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::state::AppState;
use crate::storage::{normalize_hive_id, HiveRecord, UserAgentRecord};
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::{routing::get, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;
use uuid::Uuid;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/hives", get(list_hives).post(create_hive))
        .route("/wunder/hives/{hive_id}", axum::routing::patch(update_hive))
        .route("/wunder/hives/{hive_id}/summary", get(get_hive_summary))
        .route(
            "/wunder/hives/{hive_id}/agents",
            axum::routing::post(move_hive_agents),
        )
}

async fn list_hives(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(query): Query<ListHivesQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    state
        .user_store
        .ensure_default_hive(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let items = state
        .user_store
        .list_hives(&user_id, query.include_archived.unwrap_or(false))
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .into_iter()
        .map(hive_payload)
        .collect::<Vec<_>>();
    Ok(Json(
        json!({ "data": { "total": items.len(), "items": items } }),
    ))
}

async fn create_hive(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<CreateHiveRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    state
        .user_store
        .ensure_default_hive(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let name = payload.name.trim();
    if name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }

    let source_hive_id = payload
        .copy_from_hive_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(normalize_hive_id);
    let source_agents = if let Some(source_hive_id) = source_hive_id.as_deref() {
        let source_hive = state
            .user_store
            .get_hive(&user_id, source_hive_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        if source_hive.is_none() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                format!("source hive {source_hive_id} not found"),
            ));
        }
        state
            .user_store
            .list_user_agents_by_hive(&user_id, source_hive_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    } else {
        Vec::new()
    };

    let now = now_ts();
    let record = HiveRecord {
        hive_id: normalize_hive_id(&format!("hive_{}", Uuid::new_v4().simple())),
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

    let mut copied_agent_total = 0usize;
    for source in source_agents {
        let cloned = clone_agent_for_hive(&source, &record.hive_id, now_ts());
        state
            .user_store
            .upsert_user_agent(&cloned)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        copied_agent_total += 1;
    }

    let mut data = hive_payload(record);
    if let Some(object) = data.as_object_mut() {
        object.insert("copied_agent_total".to_string(), json!(copied_agent_total));
    }
    Ok(Json(json!({ "data": data })))
}

async fn update_hive(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(hive_id): AxumPath<String>,
    Json(payload): Json<UpdateHiveRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let normalized_hive_id = normalize_hive_id(&hive_id);
    let mut record = state
        .user_store
        .get_hive(&user_id, &normalized_hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "hive not found".to_string()))?;
    if let Some(name) = payload.name {
        let cleaned = name.trim();
        if !cleaned.is_empty() {
            record.name = cleaned.to_string();
        }
    }
    if let Some(description) = payload.description {
        record.description = description;
    }
    if let Some(status) = payload.status {
        let cleaned = status.trim().to_ascii_lowercase();
        if cleaned == "active" || cleaned == "archived" {
            record.status = cleaned;
        }
    }
    if let Some(is_default) = payload.is_default {
        record.is_default = is_default;
    }
    record.updated_time = now_ts();
    state
        .user_store
        .upsert_hive(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": hive_payload(record) })))
}

async fn get_hive_summary(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(hive_id): AxumPath<String>,
    Query(query): Query<HiveSummaryQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let normalized_hive_id = normalize_hive_id(&hive_id);
    let hive = state
        .user_store
        .get_hive(&user_id, &normalized_hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if hive.is_none() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "hive not found".to_string(),
        ));
    }

    let agents = state
        .user_store
        .list_user_agents_by_hive(&user_id, &normalized_hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let agent_ids = agents
        .iter()
        .map(|item| item.agent_id.as_str())
        .collect::<HashSet<_>>();

    let lookback_minutes = query.lookback_minutes.unwrap_or(60).clamp(5, 1440);
    let lookback_seconds = lookback_minutes as f64 * 60.0;
    let now = now_ts();
    let (runs, _) = state
        .user_store
        .list_team_runs(&user_id, Some(&normalized_hive_id), None, 0, 300)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let mut running_team_runs = 0i64;
    let mut timeout_total = 0i64;
    let mut recent_finished_total = 0i64;
    let mut recent_success_total = 0i64;
    let mut recent_elapsed_sum = 0.0;
    let mut recent_elapsed_count = 0i64;
    for run in &runs {
        let status = run.status.trim().to_ascii_lowercase();
        if matches!(status.as_str(), "queued" | "running" | "merging") {
            running_team_runs += 1;
        }
        if status == "timeout" {
            timeout_total += 1;
        }
        if let Some(finished_time) = run.finished_time {
            if now - finished_time <= lookback_seconds {
                recent_finished_total += 1;
                if status == "success" {
                    recent_success_total += 1;
                }
                if let Some(elapsed_s) = run
                    .elapsed_s
                    .filter(|value| value.is_finite() && *value >= 0.0)
                {
                    recent_elapsed_sum += elapsed_s;
                    recent_elapsed_count += 1;
                }
            }
        }
    }

    let locks = state
        .user_store
        .list_session_locks_by_user(&user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let running_agent_total = locks
        .into_iter()
        .filter(|lock| agent_ids.contains(lock.agent_id.as_str()))
        .count() as i64;

    let success_rate = if recent_finished_total <= 0 {
        0.0
    } else {
        recent_success_total as f64 / recent_finished_total as f64
    };
    let average_elapsed_s = if recent_elapsed_count <= 0 {
        0.0
    } else {
        recent_elapsed_sum / recent_elapsed_count as f64
    };

    Ok(Json(json!({
        "data": {
            "hive_id": normalized_hive_id,
            "lookback_minutes": lookback_minutes,
            "agent_total": agents.len(),
            "running_agent_total": running_agent_total,
            "running_team_runs": running_team_runs,
            "recent_success_rate": success_rate,
            "recent_finished_total": recent_finished_total,
            "average_elapsed_s": average_elapsed_s,
            "timeout_total": timeout_total,
        }
    })))
}

async fn move_hive_agents(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(hive_id): AxumPath<String>,
    Json(payload): Json<MoveHiveAgentsRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let normalized_hive_id = normalize_hive_id(&hive_id);
    let hive = state
        .user_store
        .get_hive(&user_id, &normalized_hive_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if hive.is_none() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "hive not found".to_string(),
        ));
    }
    let affected = state
        .user_store
        .move_agents_to_hive(&user_id, &normalized_hive_id, &payload.agent_ids)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "hive_id": normalized_hive_id,
            "moved": affected,
        }
    })))
}

fn hive_payload(record: HiveRecord) -> Value {
    json!({
        "hive_id": normalize_hive_id(&record.hive_id),
        "user_id": record.user_id,
        "name": record.name,
        "description": record.description,
        "is_default": record.is_default,
        "status": record.status,
        "created_time": record.created_time,
        "updated_time": record.updated_time,
    })
}

fn clone_agent_for_hive(source: &UserAgentRecord, target_hive_id: &str, now: f64) -> UserAgentRecord {
    UserAgentRecord {
        agent_id: format!("agent_{}", Uuid::new_v4().simple()),
        user_id: source.user_id.clone(),
        hive_id: normalize_hive_id(target_hive_id),
        name: source.name.clone(),
        description: source.description.clone(),
        system_prompt: source.system_prompt.clone(),
        tool_names: source.tool_names.clone(),
        access_level: source.access_level.clone(),
        is_shared: source.is_shared,
        status: source.status.clone(),
        icon: source.icon.clone(),
        sandbox_container_id: source.sandbox_container_id,
        created_at: now,
        updated_at: now,
    }
}

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}

#[derive(Debug, Deserialize)]
struct ListHivesQuery {
    #[serde(default)]
    include_archived: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct CreateHiveRequest {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default, alias = "copyFromHiveId", alias = "copy_from_hive_id")]
    copy_from_hive_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateHiveRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    is_default: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct HiveSummaryQuery {
    #[serde(default)]
    lookback_minutes: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct MoveHiveAgentsRequest {
    #[serde(default)]
    agent_ids: Vec<String>,
}
