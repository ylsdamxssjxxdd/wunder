use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::org_units;
use crate::state::AppState;
use crate::storage::{
    normalize_sandbox_container_id, OrgUnitRecord, UserAccountRecord, UserAgentRecord,
    DEFAULT_HIVE_ID,
};
use crate::user_access::{build_user_tool_context, compute_allowed_tool_names};
use crate::user_store::UserStore;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::{routing::get, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use uuid::Uuid;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/auth/register", post(register))
        .route("/wunder/auth/login", post(login))
        .route("/wunder/auth/demo", post(login_demo))
        .route("/wunder/auth/external/login", post(external_login))
        .route("/wunder/auth/external/code", post(external_issue_code))
        .route("/wunder/auth/external/launch", post(external_launch))
        .route("/wunder/auth/external/exchange", post(external_exchange))
        .route("/wunder/auth/org_units", get(list_org_units))
        .route(
            "/wunder/auth/me/preferences",
            get(me_preferences).patch(update_me_preferences),
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
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if desktop_mode && requested_unit_id != created_user.unit_id {
        created_user.unit_id = requested_unit_id;
        created_user.updated_at = now_ts();
        state
            .user_store
            .update_user(&created_user)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
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
    let profile = build_user_profile(&state, &user)?;
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

    let config = state.config_store.get().await;
    let preset_agent_name = config.external_embed_preset_agent_name().ok_or_else(|| {
        error_response(
            StatusCode::FORBIDDEN,
            "external embed preset agent is not configured".to_string(),
        )
    })?;

    let target_agent =
        resolve_or_create_external_embed_agent(&state, &session.user, &preset_agent_name).await?;
    let record = state
        .external_auth_codes
        .issue(
            session.user.user_id.clone(),
            session.token.token.clone(),
            60.0,
        )
        .await;
    let entry_path = format!(
        "/app/embed/chat?wunder_code={}&agent_id={}&embed=1",
        record.code, target_agent.agent_id
    );

    Ok(Json(json!({
        "data": {
            "code": record.code,
            "expires_at": record.expires_at,
            "entry_path": entry_path,
            "agent_id": target_agent.agent_id,
            "agent_name": target_agent.name,
            "created": created,
            "updated": updated,
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
    let desktop_mode = is_desktop_mode(&state).await;
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
            let previous_level = record
                .unit_id
                .as_ref()
                .and_then(|value| unit_map.get(value))
                .map(|unit| unit.level);
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
    let fallback_unit = normalize_optional_id(user.unit_id.as_deref())
        .filter(|_| unit.is_none())
        .map(|unit_id| lightweight_unit_record(&unit_id));
    Ok(UserStore::to_profile_with_unit(
        user,
        unit.as_ref().or(fallback_unit.as_ref()),
    ))
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
    preset_agent_name: &str,
) -> Result<UserAgentRecord, Response> {
    let cleaned_name = preset_agent_name.trim();
    if cleaned_name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "external embed preset agent is empty".to_string(),
        ));
    }
    let config = state.config_store.get().await;
    let preset = config
        .user_agents
        .presets
        .into_iter()
        .find(|item| item.name.trim() == cleaned_name)
        .ok_or_else(|| {
            error_response(
                StatusCode::NOT_FOUND,
                format!("preset agent '{cleaned_name}' not found"),
            )
        })?;

    let all_agents = state
        .user_store
        .list_user_agents(&user.user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut candidates = all_agents
        .into_iter()
        .filter(|item| item.name.trim() == cleaned_name)
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.updated_at.total_cmp(&left.updated_at));

    // Prefer user-customized same-name agent over preset-template records.
    if let Some(custom) = candidates
        .iter()
        .find(|item| !is_external_embed_preset_template(item, &preset))
        .cloned()
    {
        return Ok(custom);
    }
    if let Some(existing) = candidates.first().cloned() {
        return Ok(existing);
    }

    state
        .user_store
        .ensure_default_hive(&user.user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let context = build_user_tool_context(state, &user.user_id).await;
    let mut tool_names = compute_allowed_tool_names(user, &context)
        .into_iter()
        .collect::<Vec<_>>();
    tool_names.sort();

    let icon_name = if preset.icon_name.trim().is_empty() {
        "spark".to_string()
    } else {
        preset.icon_name.trim().to_string()
    };
    let icon_color = if preset.icon_color.trim().is_empty() {
        "#94a3b8".to_string()
    } else {
        preset.icon_color.trim().to_string()
    };
    let icon = json!({
        "name": icon_name,
        "color": icon_color
    })
    .to_string();
    let now = now_ts();
    let created = UserAgentRecord {
        agent_id: format!("agent_{}", Uuid::new_v4().simple()),
        user_id: user.user_id.clone(),
        hive_id: DEFAULT_HIVE_ID.to_string(),
        name: cleaned_name.to_string(),
        description: preset.description.trim().to_string(),
        system_prompt: preset.system_prompt.trim().to_string(),
        declared_tool_names: Vec::new(),
        declared_skill_names: Vec::new(),
        tool_names,
        preset_questions: Vec::new(),
        access_level: "A".to_string(),
        approval_mode: "auto_edit".to_string(),
        is_shared: false,
        status: "active".to_string(),
        icon: Some(icon),
        sandbox_container_id: normalize_sandbox_container_id(preset.sandbox_container_id),
        created_at: now,
        updated_at: now,
    };
    state
        .user_store
        .upsert_user_agent(&created)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(created)
}

fn is_external_embed_preset_template(
    candidate: &UserAgentRecord,
    preset: &crate::config::UserAgentPresetConfig,
) -> bool {
    let preset_description = preset.description.trim();
    let preset_prompt = preset.system_prompt.trim();
    candidate.description.trim() == preset_description
        && candidate.system_prompt.trim() == preset_prompt
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

        if sync_external_unit_binding(user_store, &mut user, unit_id.as_deref(), desktop_mode)? {
            user.updated_at = now_ts();
            user_store.update_user(&user)?;
            updated = true;
        }
    } else {
        let create_unit_id = if desktop_mode { None } else { unit_id.clone() };
        let mut created_user = user_store.create_user(
            &normalized,
            None,
            password,
            Some("A"),
            create_unit_id,
            vec!["user".to_string()],
            "active",
            false,
        )?;
        if sync_external_unit_binding(
            user_store,
            &mut created_user,
            unit_id.as_deref(),
            desktop_mode,
        )? {
            created_user.updated_at = now_ts();
            user_store.update_user(&created_user)?;
            updated = true;
        }
        created = true;
    }

    let session = user_store.login(&normalized, password)?;
    Ok((session, created, updated))
}

fn provision_external_launch_session(
    user_store: &UserStore,
    username: &str,
    password: Option<&str>,
    unit_id: Option<String>,
    desktop_mode: bool,
) -> anyhow::Result<(crate::user_store::UserSession, bool, bool)> {
    if let Some(password) = password {
        let cleaned = password.trim();
        if !cleaned.is_empty() {
            return provision_external_user(user_store, username, cleaned, unit_id, desktop_mode);
        }
    }

    let normalized = UserStore::normalize_user_id(username)
        .ok_or_else(|| anyhow::anyhow!("invalid username"))?;
    if UserStore::is_default_admin(&normalized) {
        return Err(anyhow::anyhow!("admin account is protected"));
    }

    let mut created = false;
    let mut updated = false;
    let existing = user_store.get_user_by_username(&normalized)?;
    let user = if let Some(mut user) = existing {
        if UserStore::is_admin(&user) {
            return Err(anyhow::anyhow!("admin account is protected"));
        }
        if user.status.trim().to_lowercase() != "active" {
            return Err(anyhow::anyhow!("user disabled"));
        }
        if sync_external_unit_binding(user_store, &mut user, unit_id.as_deref(), desktop_mode)? {
            user.updated_at = now_ts();
            user_store.update_user(&user)?;
            updated = true;
        }
        user
    } else {
        let create_unit_id = if desktop_mode { None } else { unit_id.clone() };
        let mut created_user = user_store.create_user(
            &normalized,
            None,
            &format!("ext_launch_{}", Uuid::new_v4().simple()),
            Some("A"),
            create_unit_id,
            vec!["user".to_string()],
            "active",
            false,
        )?;
        if sync_external_unit_binding(
            user_store,
            &mut created_user,
            unit_id.as_deref(),
            desktop_mode,
        )? {
            created_user.updated_at = now_ts();
            user_store.update_user(&created_user)?;
            updated = true;
        }
        created = true;
        created_user
    };
    let token = user_store.create_session_token(&user.user_id)?;
    Ok((
        crate::user_store::UserSession { user, token },
        created,
        updated,
    ))
}

fn sync_external_unit_binding(
    user_store: &UserStore,
    user: &mut UserAccountRecord,
    unit_id: Option<&str>,
    desktop_mode: bool,
) -> anyhow::Result<bool> {
    let Some(next_unit_id) = unit_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(false);
    };
    if user.unit_id.as_deref() == Some(next_unit_id) {
        return Ok(false);
    }
    if desktop_mode {
        user.unit_id = Some(next_unit_id.to_string());
        return Ok(true);
    }

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
        user.daily_quota = UserStore::default_daily_quota_by_level(Some(next_unit.level));
    }
    Ok(true)
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
        normalize_avatar_color, normalize_avatar_icon, normalize_theme_mode,
        normalize_theme_palette,
    };

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
}
