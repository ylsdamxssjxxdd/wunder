use crate::auth as guard_auth;
use crate::services::sim_lab::{self, SimLabRunRequest};
use crate::state::AppState;
use crate::user_store::UserStore;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::{routing::get, routing::post, Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/admin/sim_lab/projects", get(list_projects))
        .route("/wunder/admin/sim_lab/runs", post(run_simulations))
}

async fn list_projects(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, Response> {
    ensure_admin(&state, &headers)?;
    Ok(Json(json!({
        "data": {
            "items": sim_lab::list_projects(),
        }
    })))
}

async fn run_simulations(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<SimLabRunRequest>,
) -> Result<Json<Value>, Response> {
    ensure_admin(&state, &headers)?;
    let report = sim_lab::run_sim_lab(state.clone(), payload)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": report })))
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
