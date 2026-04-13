use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::org_units;
use crate::services::external as external_service;
use crate::services::user_access::filter_user_agents_by_access;
use crate::services::work_state_reset::reset_user_work_state;
use crate::state::AppState;
use crate::storage::{ChatSessionRecord, OrgUnitRecord, UserAccountRecord, UserAgentRecord};
use crate::user_store::{build_default_agent_record_from_storage, UserStore};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::{routing::get, routing::post, Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{Duration, Local, TimeZone};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

#[cfg(test)]
const DEFAULT_EXTERNAL_LAUNCH_PASSWORD: &str = external_service::DEFAULT_EXTERNAL_LAUNCH_PASSWORD;
const USER_PROFILE_RUNTIME_RECORD_LIMIT: i64 = 5000;
const USER_PROFILE_SESSION_TREND_DAYS: i64 = 7;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/auth/register", post(register))
        .route("/wunder/auth/login", post(login))
        .route("/wunder/auth/reset_password", post(reset_password))
        .route("/wunder/auth/demo", post(login_demo))
        .route("/wunder/auth/external/login", post(external_login))
        .route("/wunder/auth/external/code", post(external_issue_code))
        .route("/wunder/auth/external/launch", post(external_launch))
        .route(
            "/wunder/auth/external/token_launch",
            post(external_token_launch),
        )
        .route(
            "/wunder/auth/external/token_login",
            post(external_token_launch),
        )
        .route("/wunder/auth/external/exchange", post(external_exchange))
        .route("/wunder/auth/org_units", get(list_org_units))
        .route(
            "/wunder/auth/me/preferences",
            get(me_preferences).patch(update_me_preferences),
        )
        .route(
            "/wunder/auth/me/reset_work_state",
            post(reset_my_work_state),
        )
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
struct ResetPasswordRequest {
    username: String,
    email: String,
    new_password: String,
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
struct ExternalLaunchRequest {
    /// Shared secret, configured via `security.external_auth_key` (or env `WUNDER_EXTERNAL_AUTH_KEY`).
    key: String,
    /// External system account identifier (will be normalized to wunder user_id).
    username: String,
    /// Optional external password for backward compatibility.
    /// If omitted, launch flow issues session token without password sync.
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    unit_id: Option<String>,
    #[serde(default)]
    agent_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExternalTokenLaunchRequest {
    token: String,
    user_id: String,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    unit_id: Option<String>,
    #[serde(default)]
    agent_name: Option<String>,
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
    #[serde(default)]
    current_password: Option<String>,
    #[serde(default)]
    new_password: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateMyPreferencesRequest {
    #[serde(default)]
    theme_mode: Option<String>,
    #[serde(default)]
    theme_palette: Option<String>,
    #[serde(default)]
    avatar_icon: Option<String>,
    #[serde(default)]
    avatar_color: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct UserPreferenceRecord {
    #[serde(default = "default_theme_mode")]
    theme_mode: String,
    #[serde(default = "default_theme_palette")]
    theme_palette: String,
    #[serde(default = "default_avatar_icon")]
    avatar_icon: String,
    #[serde(default = "default_avatar_color")]
    avatar_color: String,
    #[serde(default)]
    updated_at: f64,
}

#[derive(Debug, Serialize)]
struct ExternalLaunchResult {
    code: String,
    expires_at: f64,
    entry_path: String,
    agent_id: String,
    agent_name: String,
    created: bool,
    updated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExternalTokenLoginTarget {
    agent_id: String,
    agent_name: String,
    focus_mode: bool,
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
    let desktop_mode = is_desktop_mode(&state).await;
    let requested_unit_id = normalize_optional_id(payload.unit_id.as_deref());
    let create_unit_id = if desktop_mode {
        None
    } else {
        requested_unit_id.clone()
    };
    let mut created_user = state
        .user_store
        .create_user(
            username,
            payload.email,
            password,
            access_level,
            create_unit_id,
            vec!["user".to_string()],
            "active",
            false,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, localize_register_error(&err)))?;
    if desktop_mode && requested_unit_id != created_user.unit_id {
        created_user.unit_id = requested_unit_id;
        created_user.updated_at = now_ts();
        state.user_store.update_user(&created_user).map_err(|err| {
            error_response(StatusCode::BAD_REQUEST, localize_register_error(&err))
        })?;
    }
    let session = state
        .user_store
        .login(username, password)
        .map_err(|err| error_response(StatusCode::UNAUTHORIZED, localize_register_error(&err)))?;
    let profile = build_user_profile_value(&state, &session.user)?;
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
    let profile = build_user_profile_value(&state, &session.user)?;
    Ok(Json(auth_response(profile, session.token.token)))
}

async fn reset_password(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ResetPasswordRequest>,
) -> Result<Json<Value>, Response> {
    let normalized_username = UserStore::normalize_user_id(&payload.username).ok_or_else(|| {
        error_response(
            StatusCode::BAD_REQUEST,
            localize_reset_password_error_message("invalid username"),
        )
    })?;
    let email = payload.email.trim();
    let new_password = payload.new_password.trim();
    if email.is_empty() || new_password.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }

    let mut record = state
        .user_store
        .get_user_by_username(&normalized_username)
        .map_err(|err| {
            error_response(
                StatusCode::BAD_REQUEST,
                localize_reset_password_error_message(&err.to_string()),
            )
        })?
        .ok_or_else(|| {
            error_response(
                StatusCode::UNAUTHORIZED,
                localize_reset_password_error_message("account or email mismatch"),
            )
        })?;

    if record.is_demo {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "演示账号不支持重置密码".to_string(),
        ));
    }
    if record.status.trim().to_ascii_lowercase() != "active" {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            localize_reset_password_error_message("user disabled"),
        ));
    }
    let stored_email = record.email.as_deref().map(str::trim).unwrap_or_default();
    if stored_email.is_empty() || !stored_email.eq_ignore_ascii_case(email) {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            localize_reset_password_error_message("account or email mismatch"),
        ));
    }
    if UserStore::verify_password(&record.password_hash, new_password) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            localize_reset_password_error_message("password same as current"),
        ));
    }

    record.password_hash = UserStore::hash_password(new_password).map_err(|err| {
        error_response(
            StatusCode::BAD_REQUEST,
            localize_reset_password_error_message(&err.to_string()),
        )
    })?;
    record.updated_at = now_ts();
    state.user_store.update_user(&record).map_err(|err| {
        error_response(
            StatusCode::BAD_REQUEST,
            localize_reset_password_error_message(&err.to_string()),
        )
    })?;

    Ok(Json(json!({
        "data": {
            "ok": true
        }
    })))
}

async fn login_demo(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<DemoLoginRequest>,
) -> Result<Json<serde_json::Value>, Response> {
    let session = state
        .user_store
        .demo_login(payload.demo_id.as_deref())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let profile = build_user_profile_value(&state, &session.user)?;
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
    let desktop_mode = is_desktop_mode(&state).await;

    let user_store = state.user_store.clone();
    let username_snapshot = username.to_string();
    let password_snapshot = password.to_string();
    let unit_snapshot = unit_id.clone();
    let desktop_mode_snapshot = desktop_mode;
    let (session, created, updated) = tokio::task::spawn_blocking(move || {
        provision_external_user(
            &user_store,
            &username_snapshot,
            &password_snapshot,
            unit_snapshot,
            desktop_mode_snapshot,
        )
    })
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let profile = build_user_profile_value(&state, &session.user)?;
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
    let desktop_mode = is_desktop_mode(&state).await;

    let user_store = state.user_store.clone();
    let username_snapshot = username.to_string();
    let password_snapshot = password.to_string();
    let unit_snapshot = unit_id.clone();
    let desktop_mode_snapshot = desktop_mode;
    let (session, created, updated) = tokio::task::spawn_blocking(move || {
        provision_external_user(
            &user_store,
            &username_snapshot,
            &password_snapshot,
            unit_snapshot,
            desktop_mode_snapshot,
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
    let profile = build_user_profile_value(&state, &user)?;
    Ok(Json(json!({
        "data": {
            "access_token": record.token,
            "user": profile,
        }
    })))
}

async fn external_launch(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ExternalLaunchRequest>,
) -> Result<Json<Value>, Response> {
    validate_external_key(&state, &payload.key).await?;

    let username = payload.username.trim();
    let password = payload
        .password
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if username.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }

    let unit_id = normalize_optional_id(payload.unit_id.as_deref());
    let desktop_mode = is_desktop_mode(&state).await;

    let user_store = state.user_store.clone();
    let username_snapshot = username.to_string();
    let password_snapshot = password;
    let unit_snapshot = unit_id.clone();
    let desktop_mode_snapshot = desktop_mode;
    let (session, created, updated) = tokio::task::spawn_blocking(move || {
        provision_external_launch_session(
            &user_store,
            &username_snapshot,
            password_snapshot.as_deref(),
            unit_snapshot,
            desktop_mode_snapshot,
        )
    })
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let launch = build_external_launch_result(
        &state,
        session,
        created,
        updated,
        payload.agent_name.as_deref(),
    )
    .await?;

    Ok(Json(json!({ "data": launch })))
}

async fn external_token_launch(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ExternalTokenLaunchRequest>,
) -> Result<Json<Value>, Response> {
    let raw_token = payload.token.trim();
    let requested_user_id = payload.user_id.trim();
    if requested_user_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }

    let config = state.config_store.get().await;
    let user_id_claim = config.external_embed_jwt_user_id_claim();
    let validated_user_id = config
        .external_embed_jwt_secret()
        .and_then(|jwt_secret| {
            validate_external_embed_jwt(raw_token, &jwt_secret, &user_id_claim, requested_user_id)
                .ok()
        })
        .unwrap_or_else(|| requested_user_id.to_string());

    let launch_username = payload
        .username
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(validated_user_id.as_str());
    let unit_id = normalize_optional_id(payload.unit_id.as_deref());
    let desktop_mode = is_desktop_mode(&state).await;

    let user_store = state.user_store.clone();
    let username_snapshot = launch_username.to_string();
    let unit_snapshot = unit_id.clone();
    let desktop_mode_snapshot = desktop_mode;
    let (session, created, updated) = tokio::task::spawn_blocking(move || {
        provision_external_launch_session(
            &user_store,
            &username_snapshot,
            None,
            unit_snapshot,
            desktop_mode_snapshot,
        )
    })
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let target_agent =
        resolve_external_token_login_target(&state, &session.user, payload.agent_name.as_deref())
            .await?;
    let profile = build_user_profile_value(&state, &session.user)?;
    Ok(Json(json!({
        "data": {
            "access_token": session.token.token,
            "user": profile,
            "agent_id": target_agent.agent_id,
            "agent_name": target_agent.agent_name,
            "focus_mode": target_agent.focus_mode,
            "created": created,
            "updated": updated,
        }
    })))
}

fn normalize_agent_name_lookup_key(value: &str) -> String {
    value.trim().to_lowercase()
}

fn build_external_token_login_target(
    agent: &UserAgentRecord,
    focus_mode: bool,
) -> ExternalTokenLoginTarget {
    ExternalTokenLoginTarget {
        agent_id: agent.agent_id.clone(),
        agent_name: agent.name.clone(),
        focus_mode,
    }
}

fn resolve_external_token_login_target_from_candidates(
    requested_agent_name: Option<&str>,
    default_agent: &UserAgentRecord,
    owned_agents: &[UserAgentRecord],
    shared_agents: &[UserAgentRecord],
) -> ExternalTokenLoginTarget {
    let fallback = || build_external_token_login_target(default_agent, false);
    let requested_key = requested_agent_name
        .map(normalize_agent_name_lookup_key)
        .filter(|value| !value.is_empty());
    let Some(requested_key) = requested_key else {
        return fallback();
    };

    if let Some(agent) = owned_agents
        .iter()
        .find(|item| normalize_agent_name_lookup_key(&item.name) == requested_key)
    {
        return build_external_token_login_target(agent, true);
    }
    if let Some(agent) = shared_agents
        .iter()
        .find(|item| normalize_agent_name_lookup_key(&item.name) == requested_key)
    {
        return build_external_token_login_target(agent, true);
    }
    if normalize_agent_name_lookup_key(&default_agent.name) == requested_key {
        return build_external_token_login_target(default_agent, true);
    }

    fallback()
}

async fn resolve_external_token_login_target(
    state: &Arc<AppState>,
    user: &UserAccountRecord,
    requested_agent_name: Option<&str>,
) -> Result<ExternalTokenLoginTarget, Response> {
    let access = state
        .user_store
        .get_user_agent_access(&user.user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let owned_agents = state
        .user_store
        .list_user_agents(&user.user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let shared_agents = state
        .user_store
        .list_shared_user_agents(&user.user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let storage = state.user_store.storage_backend();
    let default_agent = build_default_agent_record_from_storage(storage.as_ref(), &user.user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let owned_agents = filter_user_agents_by_access(user, access.as_ref(), owned_agents);
    let shared_agents = filter_user_agents_by_access(user, access.as_ref(), shared_agents);

    Ok(resolve_external_token_login_target_from_candidates(
        requested_agent_name,
        &default_agent,
        &owned_agents,
        &shared_agents,
    ))
}

async fn me(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let profile = build_user_profile_value(&state, &resolved.user)?;
    Ok(Json(json!({ "data": profile })))
}

async fn update_me(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UpdateProfileRequest>,
) -> Result<Json<serde_json::Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let desktop_mode = is_desktop_mode(&state).await;
    let mut record = resolved.user.clone();
    let UpdateProfileRequest {
        username,
        email,
        unit_id,
        current_password,
        new_password,
    } = payload;
    let mut changed = false;
    if let Some(username) = username {
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
                "鐢ㄦ埛鍚嶆牸寮忎笉姝ｇ‘".to_string(),
            ));
        }
    }
    if let Some(email) = email {
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
                        "閭宸茶鍗犵敤".to_string(),
                    ));
                }
            }
            record.email = Some(trimmed.to_string());
            changed = true;
        }
    }
    if let Some(unit_id) = unit_id {
        let next_unit_id = normalize_optional_id(Some(&unit_id));
        if desktop_mode {
            if next_unit_id != record.unit_id {
                record.unit_id = next_unit_id;
                changed = true;
            }
        } else {
            let units = state
                .user_store
                .list_org_units()
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            let unit_map = build_unit_map(&units);
            if let Some(next_unit_id) = next_unit_id.as_deref() {
                if !unit_map.contains_key(next_unit_id) {
                    return Err(error_response(
                        StatusCode::NOT_FOUND,
                        i18n::t("error.org_unit_not_found"),
                    ));
                }
            }
            if next_unit_id != record.unit_id {
                record.unit_id = next_unit_id;
                changed = true;
            }
        }
    }
    let current_password = current_password.unwrap_or_default();
    let new_password = new_password.unwrap_or_default();
    let current_password = current_password.trim();
    let new_password = new_password.trim();
    if !current_password.is_empty() || !new_password.is_empty() {
        if record.is_demo {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "婕旂ず璐﹀彿鏆備笉鏀寔淇敼瀵嗙爜".to_string(),
            ));
        }
        if current_password.is_empty() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "请输入当前密码".to_string(),
            ));
        }
        if new_password.is_empty() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "璇疯緭鍏ユ柊瀵嗙爜".to_string(),
            ));
        }
        if !UserStore::verify_password(&record.password_hash, current_password) {
            return Err(error_response(
                StatusCode::UNAUTHORIZED,
                "当前密码不正确".to_string(),
            ));
        }
        if current_password == new_password {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "鏂板瘑鐮佷笉鑳戒笌褰撳墠瀵嗙爜鐩稿悓".to_string(),
            ));
        }
        record.password_hash = UserStore::hash_password(new_password).map_err(|err| {
            error_response(
                StatusCode::BAD_REQUEST,
                localize_update_profile_error_message(&err.to_string()),
            )
        })?;
        changed = true;
    }
    if changed {
        record.updated_at = now_ts();
        state.user_store.update_user(&record).map_err(|err| {
            error_response(
                StatusCode::BAD_REQUEST,
                localize_update_profile_error_message(&err.to_string()),
            )
        })?;
    }
    let profile = build_user_profile_value(&state, &record)?;
    Ok(Json(json!({ "data": profile })))
}

async fn reset_my_work_state(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let summary = reset_user_work_state(&state, &resolved.user.user_id, "auth_reset_work_state")
        .await
        .map_err(|err| error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json(json!({ "data": summary })))
}

async fn me_preferences(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let preferences = load_user_preferences(&state, &resolved.user.user_id)?;
    Ok(Json(json!({ "data": preferences })))
}

async fn update_me_preferences(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UpdateMyPreferencesRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let mut record = load_user_preferences(&state, &resolved.user.user_id)?;
    let before = record.clone();

    if let Some(theme_mode) = payload.theme_mode {
        record.theme_mode = normalize_theme_mode(&theme_mode);
    }
    if let Some(theme_palette) = payload.theme_palette {
        record.theme_palette = normalize_theme_palette(&theme_palette);
    }
    if let Some(avatar_icon) = payload.avatar_icon {
        record.avatar_icon = normalize_avatar_icon(&avatar_icon);
    }
    if let Some(avatar_color) = payload.avatar_color {
        record.avatar_color = normalize_avatar_color(&avatar_color);
    }
    normalize_user_preferences_in_place(&mut record);

    if record != before {
        record.updated_at = now_ts();
        save_user_preferences(&state, &resolved.user.user_id, &record)?;
    }

    Ok(Json(json!({ "data": record })))
}

async fn list_org_units(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, Response> {
    let mut units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if is_desktop_mode(&state).await {
        units = build_desktop_flat_org_units(&state, units)?;
    }
    let tree = org_units::build_unit_tree(&units);
    let items = units.iter().map(org_unit_payload).collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "items": items, "tree": tree } })))
}

fn auth_response(profile: Value, token: String) -> serde_json::Value {
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
    let fallback_unit = normalize_optional_id(user.unit_id.as_deref())
        .filter(|_| unit.is_none())
        .map(|unit_id| lightweight_unit_record(&unit_id));
    Ok(UserStore::to_profile_with_unit(
        user,
        unit.as_ref().or(fallback_unit.as_ref()),
    ))
}

fn build_user_profile_value(
    state: &Arc<AppState>,
    user: &crate::storage::UserAccountRecord,
) -> Result<Value, Response> {
    let profile = build_user_profile(state.as_ref(), user)?;
    let mut payload = serde_json::to_value(profile)
        .map_err(|err| error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    if let Value::Object(ref mut map) = payload {
        let token_balance = map
            .get("token_balance")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let token_granted_total = map
            .get("token_granted_total")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let token_used_total = map
            .get("token_used_total")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let last_token_grant_date = map
            .get("last_token_grant_date")
            .cloned()
            .unwrap_or(Value::Null);
        map.insert("daily_quota".to_string(), json!(token_granted_total));
        map.insert("daily_quota_used".to_string(), json!(token_used_total));
        map.insert("daily_quota_remaining".to_string(), json!(token_balance));
        map.insert("daily_quota_date".to_string(), last_token_grant_date);
        map.insert(
            "usage_summary".to_string(),
            build_user_usage_summary(state, &user.user_id),
        );
        map.insert(
            "session_summary".to_string(),
            build_user_session_summary(state, &user.user_id),
        );
    }
    Ok(payload)
}

fn build_user_usage_summary(state: &Arc<AppState>, user_id: &str) -> Value {
    let records =
        state
            .monitor
            .load_records_by_user(user_id, None, None, USER_PROFILE_RUNTIME_RECORD_LIMIT);
    let mut consumed_tokens = 0_i64;
    for record in records {
        let Some(events) = record.get("events").and_then(Value::as_array) else {
            continue;
        };
        for event in events {
            let event_type = event
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if event_type != "round_usage" {
                continue;
            }
            let total_tokens = parse_usage_total_tokens(event.get("data").unwrap_or(&Value::Null));
            consumed_tokens = consumed_tokens.saturating_add(total_tokens.max(0));
        }
    }
    let tool_calls = state
        .workspace
        .get_user_usage_stats()
        .get(user_id)
        .and_then(|stats| stats.get("tool_records"))
        .copied()
        .unwrap_or(0)
        .max(0);
    json!({
        "consumed_tokens": consumed_tokens.max(0),
        "tool_calls": tool_calls,
    })
}

fn build_user_session_summary(state: &Arc<AppState>, user_id: &str) -> Value {
    match state
        .user_store
        .list_chat_sessions_by_status(user_id, None, None, Some("active"), 0, 0)
    {
        Ok((sessions, total)) => summarize_user_session_records(&sessions, total),
        Err(_) => summarize_user_session_records(&[], 0),
    }
}

fn summarize_user_session_records(records: &[ChatSessionRecord], total: i64) -> Value {
    let today = Local::now().date_naive();
    let mut ordered_days = Vec::new();
    let mut trend_counts = BTreeMap::new();
    for days_ago in (0..USER_PROFILE_SESSION_TREND_DAYS).rev() {
        let day = today - Duration::days(days_ago);
        let key = day.format("%Y-%m-%d").to_string();
        ordered_days.push(key.clone());
        trend_counts.insert(key, 0_i64);
    }

    let mut last_active_at = 0.0_f64;
    let mut sessions_last_7d = 0_i64;
    for record in records {
        let ts = session_activity_timestamp(record);
        if ts > last_active_at {
            last_active_at = ts;
        }
        let Some(day_key) = format_session_day_key(ts) else {
            continue;
        };
        let Some(entry) = trend_counts.get_mut(&day_key) else {
            continue;
        };
        *entry = entry.saturating_add(1);
        sessions_last_7d = sessions_last_7d.saturating_add(1);
    }

    let trend_last_7d = ordered_days
        .into_iter()
        .map(|date| {
            json!({
                "date": date,
                "count": trend_counts.get(&date).copied().unwrap_or(0),
            })
        })
        .collect::<Vec<_>>();

    json!({
        "total_sessions": total.max(0),
        "sessions_last_7d": sessions_last_7d.max(0),
        "trend_last_7d": trend_last_7d,
        "last_active_at": if last_active_at > 0.0 {
            Value::String(format_profile_ts(last_active_at))
        } else {
            Value::Null
        },
    })
}

fn session_activity_timestamp(record: &ChatSessionRecord) -> f64 {
    if record.last_message_at > 0.0 {
        record.last_message_at
    } else if record.updated_at > 0.0 {
        record.updated_at
    } else {
        record.created_at.max(0.0)
    }
}

fn format_session_day_key(ts: f64) -> Option<String> {
    if ts <= 0.0 {
        return None;
    }
    local_datetime_from_timestamp(ts).map(|dt| dt.format("%Y-%m-%d").to_string())
}

fn format_profile_ts(ts: f64) -> String {
    local_datetime_from_timestamp(ts)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

fn local_datetime_from_timestamp(ts: f64) -> Option<chrono::DateTime<Local>> {
    if !ts.is_finite() || ts <= 0.0 {
        return None;
    }
    let seconds = ts.floor() as i64;
    let nanos = ((ts - seconds as f64).max(0.0) * 1_000_000_000.0).round() as u32;
    Local
        .timestamp_opt(seconds, nanos.min(999_999_999))
        .single()
}

fn parse_usage_total_tokens(data: &Value) -> i64 {
    let direct_total = data.get("total_tokens").and_then(Value::as_i64);
    let nested_total = data
        .get("usage")
        .and_then(|usage| usage.get("total_tokens"))
        .and_then(Value::as_i64);
    if let Some(total) = direct_total.or(nested_total) {
        return total.max(0);
    }
    let direct_input = data
        .get("input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let direct_output = data
        .get("output_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if direct_input > 0 || direct_output > 0 {
        return direct_input.saturating_add(direct_output).max(0);
    }
    let nested_input = data
        .get("usage")
        .and_then(|usage| usage.get("input_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let nested_output = data
        .get("usage")
        .and_then(|usage| usage.get("output_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    nested_input.saturating_add(nested_output).max(0)
}

fn build_unit_map(units: &[OrgUnitRecord]) -> HashMap<String, OrgUnitRecord> {
    units
        .iter()
        .map(|unit| (unit.unit_id.clone(), unit.clone()))
        .collect()
}

async fn resolve_or_create_external_embed_agent(
    state: &Arc<AppState>,
    user: &UserAccountRecord,
    target_agent_name: &str,
) -> Result<UserAgentRecord, Response> {
    external_service::resolve_or_create_external_embed_agent(state, user, target_agent_name)
        .await
        .map_err(|err| {
            error_response(
                if err.to_string().contains("not found") {
                    StatusCode::NOT_FOUND
                } else {
                    StatusCode::BAD_REQUEST
                },
                err.to_string(),
            )
        })
}

fn resolve_external_embed_target_agent_name(
    requested_agent_name: Option<&str>,
    default_agent_name: Option<String>,
) -> anyhow::Result<String> {
    external_service::resolve_external_embed_target_agent_name(
        requested_agent_name,
        default_agent_name,
    )
}

async fn build_external_launch_result(
    state: &Arc<AppState>,
    session: crate::user_store::UserSession,
    created: bool,
    updated: bool,
    requested_agent_name: Option<&str>,
) -> Result<ExternalLaunchResult, Response> {
    let config = state.config_store.get().await;
    let target_agent_name = resolve_external_embed_target_agent_name(
        requested_agent_name,
        config.external_embed_preset_agent_name(),
    )
    .map_err(|err| error_response(StatusCode::FORBIDDEN, err.to_string()))?;

    let target_agent =
        resolve_or_create_external_embed_agent(state, &session.user, &target_agent_name).await?;
    let record = state
        .external_auth_codes
        .issue(
            session.user.user_id.clone(),
            session.token.token.clone(),
            60.0,
        )
        .await;

    Ok(ExternalLaunchResult {
        code: record.code.clone(),
        expires_at: record.expires_at,
        entry_path: format!(
            "/app/embed/chat?wunder_code={}&agent_id={}&embed=1",
            record.code, target_agent.agent_id
        ),
        agent_id: target_agent.agent_id,
        agent_name: target_agent.name,
        created,
        updated,
    })
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

fn default_theme_mode() -> String {
    "light".to_string()
}

fn default_theme_palette() -> String {
    "eva-orange".to_string()
}

fn default_avatar_icon() -> String {
    "initial".to_string()
}

fn default_avatar_color() -> String {
    "#3b82f6".to_string()
}

impl Default for UserPreferenceRecord {
    fn default() -> Self {
        Self {
            theme_mode: default_theme_mode(),
            theme_palette: default_theme_palette(),
            avatar_icon: default_avatar_icon(),
            avatar_color: default_avatar_color(),
            updated_at: 0.0,
        }
    }
}

fn user_preferences_meta_key(user_id: &str) -> String {
    format!("user_preferences:v1:{}", user_id.trim())
}

fn load_user_preferences(
    state: &AppState,
    user_id: &str,
) -> Result<UserPreferenceRecord, Response> {
    let key = user_preferences_meta_key(user_id);
    let raw = state
        .user_store
        .get_meta(&key)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut record = raw
        .as_deref()
        .and_then(|value| serde_json::from_str::<UserPreferenceRecord>(value).ok())
        .unwrap_or_default();
    normalize_user_preferences_in_place(&mut record);
    Ok(record)
}

fn save_user_preferences(
    state: &AppState,
    user_id: &str,
    record: &UserPreferenceRecord,
) -> Result<(), Response> {
    let key = user_preferences_meta_key(user_id);
    let payload = serde_json::to_string(record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    state
        .user_store
        .set_meta(&key, &payload)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))
}

fn normalize_user_preferences_in_place(record: &mut UserPreferenceRecord) {
    record.theme_mode = normalize_theme_mode(&record.theme_mode);
    record.theme_palette = normalize_theme_palette(&record.theme_palette);
    record.avatar_icon = normalize_avatar_icon(&record.avatar_icon);
    record.avatar_color = normalize_avatar_color(&record.avatar_color);
    if !record.updated_at.is_finite() || record.updated_at < 0.0 {
        record.updated_at = 0.0;
    }
}

fn normalize_theme_mode(value: &str) -> String {
    let normalized = value.trim().to_lowercase();
    if normalized == "dark" {
        "dark".to_string()
    } else {
        default_theme_mode()
    }
}

fn normalize_theme_palette(value: &str) -> String {
    let normalized = value.trim().to_lowercase();
    if matches!(
        normalized.as_str(),
        "hula-green" | "eva-orange" | "claw-orange" | "minimal" | "tech-blue"
    ) {
        normalized
    } else {
        default_theme_palette()
    }
}

fn normalize_avatar_icon(value: &str) -> String {
    let normalized = value.trim().to_lowercase();
    if normalized == "initial" {
        return "initial".to_string();
    }
    let Some(number) = normalized.strip_prefix("qq-avatar-") else {
        return default_avatar_icon();
    };
    if number.is_empty() || number.len() > 4 || !number.chars().all(|ch| ch.is_ascii_digit()) {
        return default_avatar_icon();
    }
    let parsed = number.parse::<u16>().ok().unwrap_or(0);
    format!("qq-avatar-{parsed:04}")
}

fn normalize_avatar_color(value: &str) -> String {
    let normalized = value.trim().to_lowercase();
    if normalized.len() == 7
        && normalized.starts_with('#')
        && normalized.chars().skip(1).all(|ch| ch.is_ascii_hexdigit())
    {
        normalized
    } else {
        default_avatar_color()
    }
}

fn normalize_optional_id(raw: Option<&str>) -> Option<String> {
    raw.map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
}

fn lightweight_unit_record(unit_id: &str) -> OrgUnitRecord {
    let cleaned = unit_id.trim();
    let now = now_ts();
    OrgUnitRecord {
        unit_id: cleaned.to_string(),
        parent_id: None,
        name: cleaned.to_string(),
        level: 1,
        path: cleaned.to_string(),
        path_name: cleaned.to_string(),
        sort_order: 0,
        leader_ids: Vec::new(),
        created_at: now,
        updated_at: now,
    }
}

fn normalize_flat_unit_record(record: &OrgUnitRecord) -> Option<OrgUnitRecord> {
    let unit_id = record.unit_id.trim();
    if unit_id.is_empty() {
        return None;
    }
    let name = record.name.trim();
    let display_name = if name.is_empty() { unit_id } else { name };
    Some(OrgUnitRecord {
        unit_id: unit_id.to_string(),
        parent_id: None,
        name: display_name.to_string(),
        level: 1,
        path: unit_id.to_string(),
        path_name: display_name.to_string(),
        sort_order: record.sort_order,
        leader_ids: Vec::new(),
        created_at: record.created_at,
        updated_at: record.updated_at,
    })
}

fn build_desktop_flat_org_units(
    state: &AppState,
    units: Vec<OrgUnitRecord>,
) -> Result<Vec<OrgUnitRecord>, Response> {
    let mut flat_map: BTreeMap<String, OrgUnitRecord> = BTreeMap::new();
    for unit in units {
        if let Some(flattened) = normalize_flat_unit_record(&unit) {
            flat_map
                .entry(flattened.unit_id.clone())
                .or_insert(flattened);
        }
    }
    let (users, _) = state
        .user_store
        .list_users(None, None, 0, 0)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    for user in users {
        if let Some(unit_id) = normalize_optional_id(user.unit_id.as_deref()) {
            flat_map
                .entry(unit_id.clone())
                .or_insert_with(|| lightweight_unit_record(&unit_id));
        }
    }
    let mut output = flat_map.into_values().collect::<Vec<_>>();
    output.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.unit_id.cmp(&right.unit_id))
    });
    for (index, unit) in output.iter_mut().enumerate() {
        unit.parent_id = None;
        unit.level = 1;
        unit.path = unit.unit_id.clone();
        unit.path_name = unit.name.clone();
        unit.sort_order = i64::try_from(index).unwrap_or(i64::MAX);
        unit.leader_ids.clear();
    }
    Ok(output)
}

async fn is_desktop_mode(state: &AppState) -> bool {
    state
        .config_store
        .get()
        .await
        .server
        .mode
        .trim()
        .eq_ignore_ascii_case("desktop")
}

fn validate_external_embed_jwt(
    token: &str,
    secret: &str,
    user_id_claim: &str,
    requested_user_id: &str,
) -> anyhow::Result<String> {
    type HmacSha256 = Hmac<Sha256>;

    let cleaned_token = token.trim();
    let cleaned_user_id = requested_user_id.trim();
    if cleaned_token.is_empty() || cleaned_user_id.is_empty() {
        return Err(anyhow::anyhow!("token or user_id is empty"));
    }

    let mut segments = cleaned_token.split('.');
    let Some(header_segment) = segments.next() else {
        return Err(anyhow::anyhow!("invalid jwt format"));
    };
    let Some(payload_segment) = segments.next() else {
        return Err(anyhow::anyhow!("invalid jwt format"));
    };
    let Some(signature_segment) = segments.next() else {
        return Err(anyhow::anyhow!("invalid jwt format"));
    };
    if segments.next().is_some() {
        return Err(anyhow::anyhow!("invalid jwt format"));
    }

    let signing_input = format!("{header_segment}.{payload_segment}");
    let signature = URL_SAFE_NO_PAD
        .decode(signature_segment)
        .map_err(|_| anyhow::anyhow!("invalid jwt signature"))?;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())?;
    mac.update(signing_input.as_bytes());
    mac.verify_slice(&signature)
        .map_err(|_| anyhow::anyhow!("invalid jwt signature"))?;

    let header_bytes = URL_SAFE_NO_PAD
        .decode(header_segment)
        .map_err(|_| anyhow::anyhow!("invalid jwt header"))?;
    let header: Value =
        serde_json::from_slice(&header_bytes).map_err(|_| anyhow::anyhow!("invalid jwt header"))?;
    let alg = header
        .get("alg")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !alg.eq_ignore_ascii_case("HS256") {
        return Err(anyhow::anyhow!("unsupported jwt algorithm"));
    }

    let payload_bytes = URL_SAFE_NO_PAD
        .decode(payload_segment)
        .map_err(|_| anyhow::anyhow!("invalid jwt payload"))?;
    let payload: Value = serde_json::from_slice(&payload_bytes)
        .map_err(|_| anyhow::anyhow!("invalid jwt payload"))?;
    let token_user_id = extract_external_claim_text(&payload, user_id_claim)
        .ok_or_else(|| anyhow::anyhow!("jwt user claim missing"))?;
    if token_user_id != cleaned_user_id {
        return Err(anyhow::anyhow!("jwt user mismatch"));
    }

    let expires_at = payload
        .get("exp")
        .and_then(Value::as_f64)
        .or_else(|| {
            payload
                .get("exp")
                .and_then(Value::as_i64)
                .map(|value| value as f64)
        })
        .or_else(|| {
            payload
                .get("exp")
                .and_then(Value::as_u64)
                .map(|value| value as f64)
        })
        .ok_or_else(|| anyhow::anyhow!("jwt exp missing"))?;
    if expires_at <= now_ts() {
        return Err(anyhow::anyhow!("jwt expired"));
    }

    Ok(token_user_id)
}

fn extract_external_claim_text(payload: &Value, key: &str) -> Option<String> {
    let value = payload.get(key)?;
    if let Some(text) = value.as_str() {
        let cleaned = text.trim();
        return (!cleaned.is_empty()).then(|| cleaned.to_string());
    }
    if let Some(number) = value.as_i64() {
        return Some(number.to_string());
    }
    if let Some(number) = value.as_u64() {
        return Some(number.to_string());
    }
    value.as_f64().map(|number| {
        if number.fract() == 0.0 {
            format!("{number:.0}")
        } else {
            number.to_string()
        }
    })
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
    desktop_mode: bool,
) -> anyhow::Result<(crate::user_store::UserSession, bool, bool)> {
    external_service::provision_external_user(user_store, username, password, unit_id, desktop_mode)
}

fn provision_external_launch_session(
    user_store: &UserStore,
    username: &str,
    password: Option<&str>,
    unit_id: Option<String>,
    desktop_mode: bool,
) -> anyhow::Result<(crate::user_store::UserSession, bool, bool)> {
    external_service::provision_external_launch_session(
        user_store,
        username,
        password,
        unit_id,
        desktop_mode,
    )
}

fn localize_register_error(err: &anyhow::Error) -> String {
    localize_register_error_message(&err.to_string())
}

fn localize_update_profile_error_message(message: &str) -> String {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return "淇濆瓨璧勬枡澶辫触锛岃绋嶅悗閲嶈瘯".to_string();
    }
    if trimmed
        .chars()
        .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
    {
        return trimmed.to_string();
    }

    let normalized = trimmed.to_ascii_lowercase();
    if normalized.contains("password is empty") || normalized.contains("password hash is empty") {
        return "新密码不能为空".to_string();
    }
    if normalized.contains("invalid password") {
        return "当前密码不正确".to_string();
    }
    if normalized.contains("user disabled") {
        return "账号已被禁用，请联系管理员".to_string();
    }
    if normalized.contains("username already exists")
        || normalized.contains("idx_user_accounts_username")
        || normalized.contains("user_accounts.username")
    {
        return "用户名已被占用".to_string();
    }
    if normalized.contains("email already exists")
        || normalized.contains("idx_user_accounts_email")
        || normalized.contains("user_accounts.email")
    {
        return "閭宸茶鍗犵敤".to_string();
    }
    if normalized.contains("invalid username") {
        return "鐢ㄦ埛鍚嶆牸寮忎笉姝ｇ‘".to_string();
    }
    if normalized.contains("unit not found") {
        return i18n::t("error.org_unit_not_found");
    }
    "淇濆瓨璧勬枡澶辫触锛岃绋嶅悗閲嶈瘯".to_string()
}

fn localize_reset_password_error_message(message: &str) -> String {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return "重置密码失败，请稍后重试".to_string();
    }
    if trimmed
        .chars()
        .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
    {
        return trimmed.to_string();
    }

    let normalized = trimmed.to_ascii_lowercase();
    if normalized.contains("password is empty") || normalized.contains("password hash is empty") {
        return "新密码不能为空".to_string();
    }
    if normalized.contains("account or email mismatch")
        || normalized.contains("user not found")
        || normalized.contains("email mismatch")
    {
        return "账号与邮箱不匹配".to_string();
    }
    if normalized.contains("password same as current") {
        return "新密码不能与当前密码相同".to_string();
    }
    if normalized.contains("invalid username") {
        return "账号格式不正确".to_string();
    }
    if normalized.contains("user disabled") {
        return "账号已被禁用，请联系管理员".to_string();
    }
    "重置密码失败，请稍后重试".to_string()
}

fn localize_register_error_message(message: &str) -> String {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return "娉ㄥ唽澶辫触锛岃绋嶅悗閲嶈瘯".to_string();
    }
    if trimmed
        .chars()
        .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
    {
        return trimmed.to_string();
    }

    let normalized = trimmed.to_ascii_lowercase();
    if normalized.contains("username already exists")
        || normalized.contains("idx_user_accounts_username")
        || normalized.contains("user_accounts.username")
    {
        return "用户名已被占用".to_string();
    }
    if normalized.contains("email already exists")
        || normalized.contains("idx_user_accounts_email")
        || normalized.contains("user_accounts.email")
    {
        return "閭宸茶鍗犵敤".to_string();
    }
    if normalized.contains("invalid username") {
        return "鐢ㄦ埛鍚嶆牸寮忎笉姝ｇ‘锛岃浣跨敤3-64浣嶅瓧姣嶃€佹暟瀛椼€佷笅鍒掔嚎銆佷腑鍒掔嚎鎴栫偣".to_string();
    }
    if normalized.contains("password is empty") || normalized.contains("password hash is empty") {
        return "瀵嗙爜涓嶈兘涓虹┖".to_string();
    }
    if normalized.contains("unit not found") {
        return i18n::t("error.org_unit_not_found");
    }
    if normalized.contains("user disabled") {
        return "账号已被禁用，请联系管理员".to_string();
    }
    if normalized.contains("admin account is protected")
        || normalized.contains("default admin account is protected")
    {
        return "璇ヨ处鍙蜂笉鍏佽娉ㄥ唽".to_string();
    }
    if normalized.contains("duplicate key value violates unique constraint")
        || normalized.contains("unique constraint failed")
        || normalized.contains("already exists")
    {
        return "账号信息已存在，请更换用户名或邮箱".to_string();
    }
    "娉ㄥ唽澶辫触锛岃绋嶅悗閲嶈瘯".to_string()
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

#[cfg(test)]
mod tests {
    use super::{
        localize_register_error_message, localize_reset_password_error_message,
        localize_update_profile_error_message, normalize_avatar_color, normalize_avatar_icon,
        normalize_theme_mode, normalize_theme_palette, provision_external_launch_session,
        resolve_external_embed_target_agent_name,
        resolve_external_token_login_target_from_candidates, summarize_user_session_records,
        validate_external_embed_jwt, DEFAULT_EXTERNAL_LAUNCH_PASSWORD,
    };
    use crate::services::user_store::UserStore;
    use crate::storage::{ChatSessionRecord, SqliteStorage, StorageBackend, UserAgentRecord};
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use hmac::{Hmac, Mac};
    use serde_json::Value;
    use sha2::Sha256;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn build_hs256_token(secret: &str, payload: &str) -> String {
        type HmacSha256 = Hmac<Sha256>;

        let header = r#"{"alg":"HS256","typ":"JWT"}"#;
        let header_segment = URL_SAFE_NO_PAD.encode(header);
        let payload_segment = URL_SAFE_NO_PAD.encode(payload);
        let signing_input = format!("{header_segment}.{payload_segment}");
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac init");
        mac.update(signing_input.as_bytes());
        let signature_segment = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
        format!("{signing_input}.{signature_segment}")
    }

    fn sample_agent(
        agent_id: &str,
        user_id: &str,
        name: &str,
        is_shared: bool,
        updated_at: f64,
    ) -> UserAgentRecord {
        UserAgentRecord {
            agent_id: agent_id.to_string(),
            user_id: user_id.to_string(),
            hive_id: "default".to_string(),
            name: name.to_string(),
            description: String::new(),
            system_prompt: String::new(),
            model_name: None,
            ability_items: Vec::new(),
            tool_names: Vec::new(),
            declared_tool_names: Vec::new(),
            declared_skill_names: Vec::new(),
            preset_questions: Vec::new(),
            access_level: "A".to_string(),
            approval_mode: "full_auto".to_string(),
            is_shared,
            status: "active".to_string(),
            icon: None,
            sandbox_container_id: 1,
            created_at: updated_at,
            updated_at,
            preset_binding: None,
            silent: false,
            prefer_mother: false,
        }
    }

    fn sample_session(
        session_id: &str,
        created_at: f64,
        updated_at: f64,
        last_message_at: f64,
    ) -> ChatSessionRecord {
        ChatSessionRecord {
            session_id: session_id.to_string(),
            user_id: "u1".to_string(),
            title: session_id.to_string(),
            status: "active".to_string(),
            created_at,
            updated_at,
            last_message_at,
            agent_id: None,
            tool_overrides: Vec::new(),
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        }
    }

    #[test]
    fn normalize_theme_defaults_to_light() {
        assert_eq!(normalize_theme_mode(""), "light");
        assert_eq!(normalize_theme_mode("unknown"), "light");
    }

    #[test]
    fn normalize_theme_palette_defaults_to_eva_orange() {
        assert_eq!(normalize_theme_palette(""), "eva-orange");
        assert_eq!(normalize_theme_palette("other"), "eva-orange");
    }

    #[test]
    fn normalize_theme_palette_keeps_supported_palettes() {
        assert_eq!(normalize_theme_palette("eva-orange"), "eva-orange");
        assert_eq!(normalize_theme_palette("claw-orange"), "claw-orange");
        assert_eq!(normalize_theme_palette("hula-green"), "hula-green");
        assert_eq!(normalize_theme_palette("minimal"), "minimal");
        assert_eq!(normalize_theme_palette("tech-blue"), "tech-blue");
    }

    #[test]
    fn normalize_avatar_icon_supports_legacy_digits() {
        assert_eq!(normalize_avatar_icon("qq-avatar-7"), "qq-avatar-0007");
        assert_eq!(normalize_avatar_icon("QQ-AVATAR-0080"), "qq-avatar-0080");
        assert_eq!(normalize_avatar_icon("unknown"), "initial");
    }

    #[test]
    fn normalize_avatar_color_accepts_hex_only() {
        assert_eq!(normalize_avatar_color("#AbCdEf"), "#abcdef");
        assert_eq!(normalize_avatar_color("rgb(0,0,0)"), "#3b82f6");
    }

    #[test]
    fn validate_external_embed_jwt_accepts_matching_hs256_token() {
        let secret = "team-secret";
        let token = build_hs256_token(secret, r#"{"sub":"1","exp":4102444800}"#);

        let validated =
            validate_external_embed_jwt(&token, secret, "sub", "1").expect("jwt should pass");

        assert_eq!(validated, "1");
    }

    #[test]
    fn validate_external_embed_jwt_rejects_mismatched_user() {
        let secret = "team-secret";
        let token = build_hs256_token(secret, r#"{"sub":"2","exp":4102444800}"#);

        let error = validate_external_embed_jwt(&token, secret, "sub", "1")
            .expect_err("jwt should fail when user mismatches");

        assert_eq!(error.to_string(), "jwt user mismatch");
    }

    #[test]
    fn summarize_user_session_records_returns_full_counts_and_trend() {
        let now = chrono::Local::now();
        let today = now.date_naive();
        let current_day = today.and_hms_opt(12, 0, 0).expect("midday");
        let prior_day = (today - chrono::Duration::days(2))
            .and_hms_opt(18, 30, 0)
            .expect("prior day");
        let old_day = (today - chrono::Duration::days(12))
            .and_hms_opt(9, 0, 0)
            .expect("old day");

        let sessions = vec![
            sample_session(
                "s_today",
                current_day
                    .and_local_timezone(chrono::Local)
                    .single()
                    .expect("today ts")
                    .timestamp() as f64,
                0.0,
                0.0,
            ),
            sample_session(
                "s_prior",
                0.0,
                prior_day
                    .and_local_timezone(chrono::Local)
                    .single()
                    .expect("prior ts")
                    .timestamp() as f64,
                0.0,
            ),
            sample_session(
                "s_old",
                0.0,
                0.0,
                old_day
                    .and_local_timezone(chrono::Local)
                    .single()
                    .expect("old ts")
                    .timestamp() as f64,
            ),
        ];

        let summary = summarize_user_session_records(&sessions, 123);
        assert_eq!(
            summary.get("total_sessions").and_then(Value::as_i64),
            Some(123)
        );
        assert_eq!(
            summary.get("sessions_last_7d").and_then(Value::as_i64),
            Some(2)
        );
        assert!(summary
            .get("last_active_at")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty()));
        assert_eq!(
            summary
                .get("trend_last_7d")
                .and_then(Value::as_array)
                .map(|items| items.len()),
            Some(7)
        );
    }

    #[test]
    fn resolve_external_embed_target_agent_name_prefers_request_value() {
        let resolved = resolve_external_embed_target_agent_name(
            Some("  鏁版嵁鍒嗘瀽  "),
            Some("鏂囩鏍″".to_string()),
        )
        .expect("requested agent name should win");

        assert_eq!(resolved, "鏁版嵁鍒嗘瀽");
    }

    #[test]
    fn resolve_external_embed_target_agent_name_falls_back_to_default_value() {
        let resolved =
            resolve_external_embed_target_agent_name(None, Some("鏂囩鏍″".to_string()))
                .expect("default preset agent should be used");

        assert_eq!(resolved, "鏂囩鏍″");
    }

    #[test]
    fn resolve_external_token_login_target_prefers_owned_agent_name_match() {
        let default_agent = sample_agent("__default__", "u1", "Default Agent", false, 1.0);
        let owned_agents = vec![
            sample_agent("agent_owned_new", "u1", "Focused Agent", false, 3.0),
            sample_agent("agent_owned_old", "u1", "Focused Agent", false, 2.0),
        ];
        let shared_agents = vec![sample_agent(
            "agent_shared",
            "u2",
            "Focused Agent",
            true,
            4.0,
        )];

        let resolved = resolve_external_token_login_target_from_candidates(
            Some(" focused agent "),
            &default_agent,
            &owned_agents,
            &shared_agents,
        );

        assert_eq!(resolved.agent_id, "agent_owned_new");
        assert_eq!(resolved.agent_name, "Focused Agent");
        assert!(resolved.focus_mode);
    }

    #[test]
    fn resolve_external_token_login_target_falls_back_to_default_when_name_not_found() {
        let default_agent = sample_agent("__default__", "u1", "Default Agent", false, 1.0);
        let owned_agents = vec![sample_agent("agent_owned", "u1", "Known Agent", false, 2.0)];
        let shared_agents = vec![sample_agent(
            "agent_shared",
            "u2",
            "Shared Agent",
            true,
            3.0,
        )];

        let resolved = resolve_external_token_login_target_from_candidates(
            Some("missing agent"),
            &default_agent,
            &owned_agents,
            &shared_agents,
        );

        assert_eq!(resolved.agent_id, "__default__");
        assert_eq!(resolved.agent_name, "Default Agent");
        assert!(!resolved.focus_mode);
    }

    #[test]
    fn provision_external_launch_session_creates_user_with_default_password() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("external-launch-auth.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let store = UserStore::new(storage as Arc<dyn StorageBackend>);

        let (session, created, updated) =
            provision_external_launch_session(&store, "external_1", None, None, false)
                .expect("create external launch session");

        assert!(created);
        assert!(!updated);
        assert_eq!(session.user.user_id, "external_1");
        let login = store
            .login("external_1", DEFAULT_EXTERNAL_LAUNCH_PASSWORD)
            .expect("login with default password");
        assert_eq!(login.user.user_id, "external_1");
    }

    #[test]
    fn localize_register_error_message_maps_duplicate_username_and_email() {
        assert_eq!(
            localize_register_error_message("username already exists"),
            "用户名已被占用"
        );
        assert_eq!(
            localize_register_error_message("UNIQUE constraint failed: user_accounts.username"),
            "用户名已被占用"
        );
        assert_eq!(
            localize_register_error_message("email already exists"),
            "邮箱已被占用"
        );
        assert_eq!(
            localize_register_error_message(
                "duplicate key value violates unique constraint \"idx_user_accounts_email\""
            ),
            "邮箱已被占用"
        );
    }

    #[test]
    fn localize_register_error_message_keeps_chinese_and_falls_back_to_generic_chinese() {
        assert_eq!(
            localize_register_error_message("邮箱已被占用"),
            "邮箱已被占用"
        );
        assert_eq!(
            localize_register_error_message("some unexpected english error"),
            "注册失败，请稍后重试"
        );
    }

    #[test]
    fn localize_update_profile_error_message_maps_password_and_conflicts() {
        assert_eq!(
            localize_update_profile_error_message("password is empty"),
            "新密码不能为空"
        );
        assert_eq!(
            localize_update_profile_error_message("invalid password"),
            "当前密码不正确"
        );
        assert_eq!(
            localize_update_profile_error_message("username already exists"),
            "用户名已被占用"
        );
    }

    #[test]
    fn localize_reset_password_error_message_maps_identity_and_password_errors() {
        assert_eq!(
            localize_reset_password_error_message("account or email mismatch"),
            "账号与邮箱不匹配"
        );
        assert_eq!(
            localize_reset_password_error_message("password same as current"),
            "新密码不能与当前密码相同"
        );
    }
}
