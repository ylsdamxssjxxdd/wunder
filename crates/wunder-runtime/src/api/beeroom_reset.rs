use crate::api::errors::error_response;
use crate::api::user_context::resolve_user;
use crate::services::beeroom_reset::reset_beeroom_group;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::{Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route(
        "/wunder/beeroom/groups/{group_id}/reset",
        axum::routing::post(reset_group),
    )
}

async fn reset_group(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let group = super::beeroom::load_group(state.as_ref(), &resolved.user.user_id, &group_id)?;
    let summary = reset_beeroom_group(&state, &resolved.user.user_id, &group.hive_id)
        .await
        .map_err(|error| error_response(StatusCode::BAD_REQUEST, error.to_string()))?;
    state
        .projection
        .beeroom
        .publish_chat_cleared(
            &resolved.user.user_id,
            &group.hive_id,
            summary.removed_chat_messages,
            now_ts(),
        )
        .await;
    Ok(Json(json!({ "data": summary })))
}

fn now_ts() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}
