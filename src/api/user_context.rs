use crate::auth as guard_auth;
use crate::i18n;
use crate::state::AppState;
use crate::storage::UserAccountRecord;
use crate::user_store::UserStore;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct ResolvedUser {
    pub user: UserAccountRecord,
}

pub async fn resolve_user(
    state: &AppState,
    headers: &HeaderMap,
    user_id: Option<&str>,
) -> Result<ResolvedUser, Response> {
    let requested = user_id
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    let token_user = guard_auth::extract_bearer_token(headers)
        .and_then(|token| state.user_store.authenticate_token(&token).ok().flatten());
    let token_is_admin = token_user
        .as_ref()
        .map(UserStore::is_admin)
        .unwrap_or(false);

    let config = state.config_store.get().await;
    let api_key_valid = config.api_key().as_ref().map_or(false, |expected| {
        guard_auth::extract_api_key(headers)
            .map(|value| value == *expected)
            .unwrap_or(false)
    });

    if let Some(requested) = requested {
        if let Some(user) = token_user.as_ref() {
            if user.user_id == requested {
                return Ok(ResolvedUser { user: user.clone() });
            }
        }
        if api_key_valid || token_is_admin {
            let user = state
                .user_store
                .get_user_by_id(requested)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            let user = user.unwrap_or_else(|| build_virtual_user(requested));
            return Ok(ResolvedUser { user });
        }
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            i18n::t("error.auth_required"),
        ));
    }

    if let Some(user) = token_user {
        return Ok(ResolvedUser { user });
    }

    Err(error_response(
        StatusCode::UNAUTHORIZED,
        i18n::t("error.auth_required"),
    ))
}

fn error_response(status: StatusCode, message: String) -> Response {
    (
        status,
        axum::Json(json!({ "detail": { "message": message } })),
    )
        .into_response()
}

fn build_virtual_user(user_id: &str) -> UserAccountRecord {
    let now = now_ts();
    UserAccountRecord {
        user_id: user_id.to_string(),
        username: user_id.to_string(),
        email: None,
        password_hash: String::new(),
        roles: vec!["user".to_string()],
        status: "active".to_string(),
        access_level: "A".to_string(),
        daily_quota: 0,
        daily_quota_used: 0,
        daily_quota_date: None,
        is_demo: false,
        created_at: now,
        updated_at: now,
        last_login_at: None,
    }
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}
