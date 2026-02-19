use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::org_units;
use crate::state::AppState;
use crate::storage::OrgUnitRecord;
use crate::user_store::UserStore;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::{routing::get, routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/auth/register", post(register))
        .route("/wunder/auth/login", post(login))
        .route("/wunder/auth/demo", post(login_demo))
        .route("/wunder/auth/external/login", post(external_login))
        .route("/wunder/auth/external/code", post(external_issue_code))
        .route("/wunder/auth/external/exchange", post(external_exchange))
        .route("/wunder/auth/org_units", get(list_org_units))
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
struct ExternalLoginRequest {
    /// Shared secret, configured via `security.external_auth_key` (or env `WUNDER_EXTERNAL_AUTH_KEY`).
    key: String,
    /// External system account identifier (will be normalized to wunder user_id).
    username: String,
    /// External system password (used to set/verify wunder password).
    password: String,
    #[serde(default)]
    unit_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExternalExchangeRequest {
    code: String,
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
    #[serde(default)]
    unit_id: Option<String>,
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

async fn external_login(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ExternalLoginRequest>,
) -> Result<Json<Value>, Response> {
    validate_external_key(&state, &payload.key).await?;

    let username = payload.username.trim();
    let password = payload.password.trim();
    if username.is_empty() || password.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }

    let unit_id = normalize_optional_id(payload.unit_id.as_deref());

    let user_store = state.user_store.clone();
    let username_snapshot = username.to_string();
    let password_snapshot = password.to_string();
    let unit_snapshot = unit_id.clone();
    let (session, created, updated) = tokio::task::spawn_blocking(move || {
        provision_external_user(
            &user_store,
            &username_snapshot,
            &password_snapshot,
            unit_snapshot,
        )
    })
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let profile = build_user_profile(&state, &session.user)?;
    Ok(Json(json!({
        "data": {
            "access_token": session.token.token,
            "user": profile,
            "created": created,
            "updated": updated,
        }
    })))
}

async fn external_issue_code(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ExternalLoginRequest>,
) -> Result<Json<Value>, Response> {
    validate_external_key(&state, &payload.key).await?;

    let username = payload.username.trim();
    let password = payload.password.trim();
    if username.is_empty() || password.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }

    let unit_id = normalize_optional_id(payload.unit_id.as_deref());

    let user_store = state.user_store.clone();
    let username_snapshot = username.to_string();
    let password_snapshot = password.to_string();
    let unit_snapshot = unit_id.clone();
    let (session, created, updated) = tokio::task::spawn_blocking(move || {
        provision_external_user(
            &user_store,
            &username_snapshot,
            &password_snapshot,
            unit_snapshot,
        )
    })
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let record = state
        .external_auth_codes
        .issue(
            session.user.user_id.clone(),
            session.token.token.clone(),
            60.0,
        )
        .await;

    Ok(Json(json!({
        "data": {
            "code": record.code,
            "expires_at": record.expires_at,
            "created": created,
            "updated": updated,
        }
    })))
}

async fn external_exchange(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ExternalExchangeRequest>,
) -> Result<Json<Value>, Response> {
    let record = state
        .external_auth_codes
        .take(&payload.code)
        .await
        .ok_or_else(|| error_response(StatusCode::UNAUTHORIZED, "code expired".to_string()))?;

    let user = state
        .user_store
        .get_user_by_id(&record.user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "user not found".to_string()))?;
    let profile = build_user_profile(&state, &user)?;
    Ok(Json(json!({
        "data": {
            "access_token": record.token,
            "user": profile,
        }
    })))
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
    if let Some(unit_id) = payload.unit_id {
        let units = state
            .user_store
            .list_org_units()
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let unit_map = build_unit_map(&units);
        let previous_level = record
            .unit_id
            .as_ref()
            .and_then(|value| unit_map.get(value))
            .map(|unit| unit.level);
        let next_unit_id = normalize_optional_id(Some(&unit_id));
        if let Some(next_unit_id) = next_unit_id.as_deref() {
            if !unit_map.contains_key(next_unit_id) {
                return Err(error_response(
                    StatusCode::NOT_FOUND,
                    i18n::t("error.org_unit_not_found"),
                ));
            }
        }
        if next_unit_id != record.unit_id {
            let previous_default = UserStore::default_daily_quota_by_level(previous_level);
            record.unit_id = next_unit_id;
            let next_level = record
                .unit_id
                .as_ref()
                .and_then(|value| unit_map.get(value))
                .map(|unit| unit.level);
            if record.daily_quota == previous_default {
                record.daily_quota = UserStore::default_daily_quota_by_level(next_level);
            }
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

async fn list_org_units(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, Response> {
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let tree = org_units::build_unit_tree(&units);
    let items = units.iter().map(org_unit_payload).collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "items": items, "tree": tree } })))
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

fn build_unit_map(units: &[OrgUnitRecord]) -> HashMap<String, OrgUnitRecord> {
    units
        .iter()
        .map(|unit| (unit.unit_id.clone(), unit.clone()))
        .collect()
}

fn org_unit_payload(record: &OrgUnitRecord) -> Value {
    json!({
        "unit_id": record.unit_id,
        "parent_id": record.parent_id,
        "name": record.name,
        "level": record.level,
        "path": record.path,
        "path_name": record.path_name,
        "sort_order": record.sort_order,
        "leader_ids": record.leader_ids,
        "created_at": record.created_at,
        "updated_at": record.updated_at,
    })
}

fn normalize_optional_id(raw: Option<&str>) -> Option<String> {
    raw.map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
}

async fn validate_external_key(state: &Arc<AppState>, key: &str) -> Result<(), Response> {
    let provided = key.trim();
    if provided.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let config = state.config_store.get().await;
    let expected = config.external_auth_key();
    let Some(expected) = expected else {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            "external auth disabled".to_string(),
        ));
    };
    if provided != expected {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            i18n::t("error.api_key_invalid"),
        ));
    }
    Ok(())
}

fn provision_external_user(
    user_store: &UserStore,
    username: &str,
    password: &str,
    unit_id: Option<String>,
) -> anyhow::Result<(crate::user_store::UserSession, bool, bool)> {
    let normalized = UserStore::normalize_user_id(username)
        .ok_or_else(|| anyhow::anyhow!("invalid username"))?;
    if UserStore::is_default_admin(&normalized) {
        return Err(anyhow::anyhow!("admin account is protected"));
    }
    let password = password.trim();
    if password.is_empty() {
        return Err(anyhow::anyhow!("password is empty"));
    }

    let mut created = false;
    let mut updated = false;

    let existing = user_store.get_user_by_username(&normalized)?;
    if let Some(mut user) = existing {
        if UserStore::is_admin(&user) {
            return Err(anyhow::anyhow!("admin account is protected"));
        }
        if user.status.trim().to_lowercase() != "active" {
            return Err(anyhow::anyhow!("user disabled"));
        }

        // Keep wunder password in sync with external system password.
        if !UserStore::verify_password(&user.password_hash, password) {
            user.password_hash = UserStore::hash_password(password)?;
            user.updated_at = now_ts();
            user_store.update_user(&user)?;
            updated = true;
        }

        // Update unit binding when provided.
        if let Some(next_unit_id) = unit_id.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            if user.unit_id.as_deref() != Some(next_unit_id) {
                let previous_level = user
                    .unit_id
                    .as_deref()
                    .and_then(|id| user_store.get_org_unit(id).ok().flatten())
                    .map(|unit| unit.level);
                let next_unit = user_store
                    .get_org_unit(next_unit_id)?
                    .ok_or_else(|| anyhow::anyhow!("unit not found"))?;
                let previous_default = UserStore::default_daily_quota_by_level(previous_level);
                user.unit_id = Some(next_unit.unit_id.clone());
                if user.daily_quota == previous_default {
                    user.daily_quota =
                        UserStore::default_daily_quota_by_level(Some(next_unit.level));
                }
                user.updated_at = now_ts();
                user_store.update_user(&user)?;
                updated = true;
            }
        }
    } else {
        user_store.create_user(
            &normalized,
            None,
            password,
            Some("A"),
            unit_id,
            vec!["user".to_string()],
            "active",
            false,
        )?;
        created = true;
    }

    let session = user_store.login(&normalized, password)?;
    Ok((session, created, updated))
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}

fn now_ts() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}
