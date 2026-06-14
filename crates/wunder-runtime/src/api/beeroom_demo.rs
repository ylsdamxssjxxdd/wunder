use crate::api::beeroom::load_group;
use crate::api::user_context::resolve_user;
use crate::services::beeroom_demo::{self, StartBeeroomDemoRequest};
use crate::state::AppState;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::{routing::get, Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/beeroom/groups/{group_id}/demo_runs",
            axum::routing::post(start_beeroom_demo_run),
        )
        .route(
            "/wunder/beeroom/groups/{group_id}/demo_runs/{run_id}",
            get(get_beeroom_demo_run),
        )
        .route(
            "/wunder/beeroom/groups/{group_id}/demo_runs/{run_id}/cancel",
            axum::routing::post(cancel_beeroom_demo_run),
        )
}

async fn start_beeroom_demo_run(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(group_id): AxumPath<String>,
    Json(payload): Json<StartBeeroomDemoRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let group = load_group(state.as_ref(), &user_id, &group_id)?;
    let result = beeroom_demo::start_demo_run(state.clone(), &user_id, &group.hive_id, payload)
        .await
        .map_err(map_start_error)?;
    Ok(Json(json!({ "data": result })))
}

async fn get_beeroom_demo_run(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath((group_id, run_id)): AxumPath<(String, String)>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let group = load_group(state.as_ref(), &user_id, &group_id)?;
    let snapshot = beeroom_demo::get_demo_run_snapshot(&user_id, &group.hive_id, &run_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "demo run not found".to_string()))?;
    Ok(Json(json!({ "data": snapshot })))
}

async fn cancel_beeroom_demo_run(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath((group_id, run_id)): AxumPath<(String, String)>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id;
    let group = load_group(state.as_ref(), &user_id, &group_id)?;
    let snapshot = beeroom_demo::cancel_demo_run(state.clone(), &user_id, &group.hive_id, &run_id)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "demo run not found".to_string()))?;
    Ok(Json(json!({ "data": snapshot })))
}

fn map_start_error(err: anyhow::Error) -> Response {
    let message = err.to_string();
    if message.to_ascii_lowercase().contains("already active") {
        return error_response(StatusCode::CONFLICT, message);
    }
    error_response(StatusCode::BAD_REQUEST, message)
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}
