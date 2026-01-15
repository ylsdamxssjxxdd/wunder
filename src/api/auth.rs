use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::state::AppState;
use crate::user_store::UserStore;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing::get, routing::post, Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/auth/register", post(register))
        .route("/wunder/auth/login", post(login))
        .route("/wunder/auth/demo", post(login_demo))
        .route("/wunder/auth/me", get(me))
}

#[derive(Debug, Deserialize)]
struct RegisterRequest {
    username: String,
    #[serde(default)]
    email: Option<String>,
    password: String,
    #[serde(default)]
    access_level: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct DemoLoginRequest {
    #[serde(default)]
    demo_id: Option<String>,
}

async fn register(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RegisterRequest>,
) -> Result<Json<serde_json::Value>, Response> {
    let username = payload.username.trim();
    let password = payload.password.trim();
    if username.is_empty() || password.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let access_level = payload.access_level.as_deref();
    state
        .user_store
        .create_user(
            username,
            payload.email,
            password,
            access_level,
            vec!["user".to_string()],
            "active",
            false,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let session = state
        .user_store
        .login(username, password)
        .map_err(|err| error_response(StatusCode::UNAUTHORIZED, err.to_string()))?;
    Ok(Json(auth_response(session.user, session.token.token)))
}

async fn login(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<serde_json::Value>, Response> {
    let username = payload.username.trim();
    let password = payload.password.trim();
    if username.is_empty() || password.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let session = state
        .user_store
        .login(username, password)
        .map_err(|err| error_response(StatusCode::UNAUTHORIZED, err.to_string()))?;
    Ok(Json(auth_response(session.user, session.token.token)))
}

async fn login_demo(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<DemoLoginRequest>,
) -> Result<Json<serde_json::Value>, Response> {
    let session = state
        .user_store
        .demo_login(payload.demo_id.as_deref())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(auth_response(session.user, session.token.token)))
}

async fn me(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    Ok(Json(
        json!({ "data": UserStore::to_profile(&resolved.user) }),
    ))
}

fn auth_response(user: crate::storage::UserAccountRecord, token: String) -> serde_json::Value {
    json!({
        "data": {
            "access_token": token,
            "user": UserStore::to_profile(&user)
        }
    })
}

fn error_response(status: StatusCode, message: String) -> Response {
    (status, Json(json!({ "detail": { "message": message } }))).into_response()
}
