use super::error_response;
use crate::{auth, core::blocking, i18n, state::AppState, user_store::UserStore};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::post,
    Json, Router,
};
use serde_json::Value;
use std::sync::Arc;

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new().route(
        "/wunder/admin/monitor/{session_id}/priority",
        post(prioritize),
    )
}

async fn prioritize(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, Response> {
    // Keep authorization at this boundary as well as the server middleware (shared desktop router).
    let config = state.config_store.get().await;
    let mut authorized = config
        .api_key()
        .is_some_and(|expected| auth::extract_api_key(&headers).is_some_and(|key| key == expected));
    if !authorized {
        if let Some(token) = auth::extract_bearer_token(&headers) {
            let store = state.user_store.clone();
            authorized =
                blocking::run_db("queue.admin_auth", move || store.authenticate_token(&token))
                    .await
                    .ok()
                    .flatten()
                    .as_ref()
                    .is_some_and(UserStore::is_admin);
        }
    }
    if !authorized {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            i18n::t("error.permission_denied"),
        ));
    }
    state
        .kernel
        .thread_runtime
        .prioritize_session(&session_id, authorized)
        .await
        .map(Json)
        .map_err(|_| {
            error_response(
                StatusCode::CONFLICT,
                "Queue task is no longer available".into(),
            )
        })
}
