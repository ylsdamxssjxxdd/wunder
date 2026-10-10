//! User-facing queue endpoints: list parked turns of a session and act on one of them.

use super::error_response;
use crate::api::user_context::resolve_user;
use crate::state::AppState;
use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::{routing::get, routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/chat/sessions/{session_id}/queue",
            get(list_session_queue),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/queue/reorder",
            post(reorder_session_queue),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/queue/{queue_id}/prioritize",
            post(prioritize_queued_turn),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/queue/{queue_id}/cancel",
            post(cancel_queued_turn),
        )
}

#[derive(Debug, Deserialize)]
struct ReorderQueueRequest {
    #[serde(default, alias = "queueIds", alias = "task_ids", alias = "taskIds")]
    queue_ids: Vec<String>,
}

async fn resolve_owned_session(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    session_id: &str,
) -> Result<(String, String), Response> {
    let resolved = resolve_user(state, headers, None).await?;
    let cleaned = session_id.trim().to_string();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "session_id is required".to_string(),
        ));
    }
    state
        .user_store
        .get_chat_session(&resolved.user.user_id, &cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| {
            error_response(StatusCode::NOT_FOUND, crate::i18n::t("error.session_not_found"))
        })?;
    Ok((resolved.user.user_id, cleaned))
}

async fn list_session_queue(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let (user_id, session_id) = resolve_owned_session(&state, &headers, &session_id).await?;
    let data = state
        .kernel
        .thread_runtime
        .list_user_queue_tasks(&user_id, &session_id)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": data })))
}

async fn prioritize_queued_turn(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath((session_id, queue_id)): AxumPath<(String, String)>,
) -> Result<Json<Value>, Response> {
    let (user_id, session_id) = resolve_owned_session(&state, &headers, &session_id).await?;
    let data = state
        .kernel
        .thread_runtime
        .prioritize_queued_task(&user_id, &session_id, &queue_id)
        .await
        .map_err(|err| error_response(StatusCode::CONFLICT, err.to_string()))?;
    Ok(Json(json!({ "data": data })))
}

async fn cancel_queued_turn(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath((session_id, queue_id)): AxumPath<(String, String)>,
) -> Result<Json<Value>, Response> {
    let (user_id, session_id) = resolve_owned_session(&state, &headers, &session_id).await?;
    let data = state
        .kernel
        .thread_runtime
        .cancel_queued_task(&user_id, &session_id, &queue_id)
        .await
        .map_err(|err| error_response(StatusCode::CONFLICT, err.to_string()))?;
    Ok(Json(json!({ "data": data })))
}

async fn reorder_session_queue(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    Json(payload): Json<ReorderQueueRequest>,
) -> Result<Json<Value>, Response> {
    let (user_id, session_id) = resolve_owned_session(&state, &headers, &session_id).await?;
    let data = state
        .kernel
        .thread_runtime
        .reorder_queued_tasks(&user_id, &session_id, &payload.queue_ids)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": data })))
}
