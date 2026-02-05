use crate::api::user_context::resolve_user;
use crate::cron::{handle_cron_action, list_cron_runs, CronActionRequest};
use crate::i18n;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct CronActionQuery {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CronRunsQuery {
    #[serde(default)]
    user_id: Option<String>,
    job_id: String,
    #[serde(default)]
    limit: Option<i64>,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/cron/list", get(cron_list))
        .route("/wunder/cron/runs", get(cron_runs))
        .route("/wunder/cron/add", post(cron_add))
        .route("/wunder/cron/update", post(cron_update))
        .route("/wunder/cron/remove", post(cron_remove))
        .route("/wunder/cron/enable", post(cron_enable))
        .route("/wunder/cron/disable", post(cron_disable))
        .route("/wunder/cron/get", post(cron_get))
        .route("/wunder/cron/run", post(cron_run))
        .route("/wunder/cron/action", post(cron_action))
}

async fn cron_list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<CronActionQuery>,
) -> Result<Json<Value>, Response> {
    let payload = CronActionRequest {
        action: "list".to_string(),
        job: None,
    };
    handle_action(state, headers, query, payload, None).await
}

async fn cron_runs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<CronRunsQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let cleaned = query.job_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.task_id_required"),
        ));
    }
    let limit = query.limit.unwrap_or(20);
    let payload = list_cron_runs(
        state.storage.clone(),
        &resolved.user.user_id,
        cleaned,
        limit,
    )
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": payload })))
}

async fn cron_add(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<CronActionQuery>,
    Json(payload): Json<CronActionRequest>,
) -> Result<Json<Value>, Response> {
    handle_action(state, headers, query, payload, Some("add")).await
}

async fn cron_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<CronActionQuery>,
    Json(payload): Json<CronActionRequest>,
) -> Result<Json<Value>, Response> {
    handle_action(state, headers, query, payload, Some("update")).await
}

async fn cron_remove(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<CronActionQuery>,
    Json(payload): Json<CronActionRequest>,
) -> Result<Json<Value>, Response> {
    handle_action(state, headers, query, payload, Some("remove")).await
}

async fn cron_enable(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<CronActionQuery>,
    Json(payload): Json<CronActionRequest>,
) -> Result<Json<Value>, Response> {
    handle_action(state, headers, query, payload, Some("enable")).await
}

async fn cron_disable(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<CronActionQuery>,
    Json(payload): Json<CronActionRequest>,
) -> Result<Json<Value>, Response> {
    handle_action(state, headers, query, payload, Some("disable")).await
}

async fn cron_get(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<CronActionQuery>,
    Json(payload): Json<CronActionRequest>,
) -> Result<Json<Value>, Response> {
    handle_action(state, headers, query, payload, Some("get")).await
}

async fn cron_run(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<CronActionQuery>,
    Json(payload): Json<CronActionRequest>,
) -> Result<Json<Value>, Response> {
    handle_action(state, headers, query, payload, Some("run")).await
}

async fn cron_action(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<CronActionQuery>,
    Json(payload): Json<CronActionRequest>,
) -> Result<Json<Value>, Response> {
    handle_action(state, headers, query, payload, None).await
}

async fn handle_action(
    state: Arc<AppState>,
    headers: HeaderMap,
    query: CronActionQuery,
    mut payload: CronActionRequest,
    forced_action: Option<&str>,
) -> Result<Json<Value>, Response> {
    if let Some(action) = forced_action {
        payload.action = action.to_string();
    }
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let config = state.config_store.get().await;
    let result = handle_cron_action(
        config,
        state.storage.clone(),
        Some(state.orchestrator.clone()),
        state.user_store.clone(),
        state.user_tool_manager.clone(),
        state.skills.clone(),
        &resolved.user.user_id,
        query.session_id.as_deref(),
        query.agent_id.as_deref(),
        payload,
    )
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": result })))
}

fn error_response(status: StatusCode, message: String) -> Response {
    (status, Json(json!({ "detail": { "message": message } }))).into_response()
}
