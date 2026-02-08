use crate::auth as guard_auth;
use crate::i18n;
use crate::state::AppState;
use crate::storage::UserAccountRecord;
use crate::user_store::UserStore;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;

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
    let token_user = if let Some(token) = guard_auth::extract_bearer_token(headers) {
        let user_store = state.user_store.clone();
        match tokio::task::spawn_blocking(move || user_store.authenticate_token(&token)).await {
            Ok(Ok(user)) => user,
            Ok(Err(err)) => {
                warn!("resolve_user token auth failed: {err}");
                None
            }
            Err(err) => {
                warn!("resolve_user token auth join failed: {err}");
                None
            }
        }
    } else {
        None
    };
    let token_is_admin = token_user
        .as_ref()
        .map(UserStore::is_admin)
        .unwrap_or(false);

    let config = state.config_store.get().await;
    let api_key_valid = config.api_key().as_ref().is_some_and(|expected| {
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
            let user_store = state.user_store.clone();
            let requested_user_id = requested.to_string();
            let user =
                tokio::task::spawn_blocking(move || user_store.get_user_by_id(&requested_user_id))
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
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
    crate::api::errors::error_response(status, message)
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
        unit_id: None,
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
