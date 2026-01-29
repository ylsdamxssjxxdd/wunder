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
        .route("/wunder/auth/me", get(me).patch(update_me))
}

#[derive(Debug, Deserialize)]
struct RegisterRequest {
    username: String,
    #[serde(default)]
    email: Option<String>,
    password: String,
    #[serde(default)]
    access_level: Option<String>,
    #[serde(default)]
    unit_id: Option<String>,
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

#[derive(Debug, Deserialize)]
struct UpdateProfileRequest {
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    email: Option<String>,
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
            payload.unit_id,
            vec!["user".to_string()],
            "active",
            false,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let session = state
        .user_store
        .login(username, password)
        .map_err(|err| error_response(StatusCode::UNAUTHORIZED, err.to_string()))?;
    let profile = build_user_profile(&state, &session.user)?;
    Ok(Json(auth_response(profile, session.token.token)))
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
    let profile = build_user_profile(&state, &session.user)?;
    Ok(Json(auth_response(profile, session.token.token)))
}

async fn login_demo(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<DemoLoginRequest>,
) -> Result<Json<serde_json::Value>, Response> {
    let session = state
        .user_store
        .demo_login(payload.demo_id.as_deref())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let profile = build_user_profile(&state, &session.user)?;
    Ok(Json(auth_response(profile, session.token.token)))
}

async fn me(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let profile = build_user_profile(&state, &resolved.user)?;
    Ok(Json(json!({ "data": profile })))
}

async fn update_me(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UpdateProfileRequest>,
) -> Result<Json<serde_json::Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let mut record = resolved.user.clone();
    let mut changed = false;
    if let Some(username) = payload.username {
        let trimmed = username.trim();
        if trimmed.is_empty() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.content_required"),
            ));
        }
        if UserStore::is_default_admin(&record.user_id) && trimmed != record.username {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "默认管理员账号不可修改".to_string(),
            ));
        }
        if let Some(normalized) = UserStore::normalize_user_id(trimmed) {
            if normalized != record.username {
                let existing = state
                    .user_store
                    .get_user_by_username(&normalized)
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
                if let Some(existing) = existing {
                    if existing.user_id != record.user_id {
                        return Err(error_response(
                            StatusCode::BAD_REQUEST,
                            "用户名已被占用".to_string(),
                        ));
                    }
                }
                record.username = normalized;
                changed = true;
            }
        } else {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "用户名格式不正确".to_string(),
            ));
        }
    }
    if let Some(email) = payload.email {
        let trimmed = email.trim();
        if trimmed.is_empty() {
            if record.email.is_some() {
                record.email = None;
                changed = true;
            }
        } else if record.email.as_deref() != Some(trimmed) {
            let existing = state
                .user_store
                .get_user_by_email(trimmed)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            if let Some(existing) = existing {
                if existing.user_id != record.user_id {
                    return Err(error_response(
                        StatusCode::BAD_REQUEST,
                        "邮箱已被占用".to_string(),
                    ));
                }
            }
            record.email = Some(trimmed.to_string());
            changed = true;
        }
    }
    if changed {
        record.updated_at = now_ts();
        state
            .user_store
            .update_user(&record)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    let profile = build_user_profile(&state, &record)?;
    Ok(Json(json!({ "data": profile })))
}

fn auth_response(profile: crate::user_store::UserProfile, token: String) -> serde_json::Value {
    json!({
        "data": {
            "access_token": token,
            "user": profile
        }
    })
}

fn build_user_profile(
    state: &AppState,
    user: &crate::storage::UserAccountRecord,
) -> Result<crate::user_store::UserProfile, Response> {
    let unit = user
        .unit_id
        .as_deref()
        .map(|unit_id| {
            state
                .user_store
                .get_org_unit(unit_id)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))
        })
        .transpose()?
        .flatten();
    Ok(UserStore::to_profile_with_unit(user, unit.as_ref()))
}

fn error_response(status: StatusCode, message: String) -> Response {
    (status, Json(json!({ "detail": { "message": message } }))).into_response()
}

fn now_ts() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}
