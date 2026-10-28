use crate::api::errors::error_response_with_detail;
use crate::auth as guard_auth;
use crate::core::blocking;
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
    pub session_scope: Option<String>,
    pub device_id: Option<String>,
}

/// Long-lived transports periodically revalidate their original credential.
/// Rotation retires that credential, causing a clean authenticated reconnect.
pub async fn credential_active(state: &AppState, token: &str, user_id: &str) -> bool {
    let store = state.user_store.clone();
    let token = token.to_string();
    let expected = user_id.to_string();
    let result = blocking::run_auth("api.transport.revalidate", move || {
        store
            .authenticate_token_details(&token)
            .map(|auth| auth.is_some_and(|auth| auth.user.user_id == expected))
    })
    .await;
    match result {
        Ok(active) => active,
        Err(error) => {
            warn!(%error, "transport credential validation failed");
            false
        }
    }
}

pub async fn resolve_user(
    state: &AppState,
    headers: &HeaderMap,
    user_id: Option<&str>,
) -> Result<ResolvedUser, Response> {
    resolve_user_internal(state, headers, user_id, false).await
}

pub async fn resolve_user_for_device_registration(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<ResolvedUser, Response> {
    resolve_user_internal(state, headers, None, true).await
}

async fn resolve_user_internal(
    state: &AppState,
    headers: &HeaderMap,
    user_id: Option<&str>,
    allow_unbound: bool,
) -> Result<ResolvedUser, Response> {
    let requested = user_id
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    let token_auth = if let Some(token) = guard_auth::extract_bearer_token(headers) {
        let user_store = state.user_store.clone();
        match blocking::run_auth("api.user_context.authenticate_token", move || {
            user_store.authenticate_token_details(&token)
        })
        .await
        {
            Ok(user) => user,
            Err(err) => {
                warn!("resolve_user token auth failed: {err}");
                None
            }
        }
    } else {
        None
    };
    let mut bound_device_id = None;
    if let Some(auth) = token_auth.as_ref() {
        let local = crate::user_store::UserStore::is_local_refresh_scope(&auth.session_scope);
        if local && auth.family_id.is_none() && !allow_unbound {
            return Err(error_response(
                StatusCode::UNAUTHORIZED,
                "device registration required".to_string(),
            ));
        }
        if let Some(family) = auth.family_id.as_deref() {
            let storage = state.storage.clone();
            let family = family.to_string();
            let binding = blocking::run_db("api.user_context.device", move || {
                storage.get_user_session_device(&family)
            })
            .await
            .map_err(|_| {
                error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "session validation unavailable".to_string(),
                )
            })?;
            if local && binding.is_none() && !allow_unbound {
                return Err(error_response(
                    StatusCode::UNAUTHORIZED,
                    "device registration required".to_string(),
                ));
            }
            bound_device_id = binding.as_ref().map(|bound| bound.device_id.clone());
            if let (Some(bound), Some(header)) = (binding, headers.get("x-wunder-device-id")) {
                if header.to_str().ok() != Some(bound.device_id.as_str()) {
                    return Err(error_response(
                        StatusCode::UNAUTHORIZED,
                        "device identity mismatch".to_string(),
                    ));
                }
            }
        }
    }
    let token_user = token_auth.as_ref().map(|auth| auth.user.clone());
    let token_session_scope = token_auth.as_ref().map(|auth| auth.session_scope.clone());
    let token_presence_kind =
        crate::services::presence::client_kind_from_scope(token_session_scope.as_deref());
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
                state
                    .control
                    .presence
                    .touch_user(&user.user_id, token_presence_kind, now_ts());
                return Ok(ResolvedUser {
                    user: user.clone(),
                    session_scope: token_session_scope.clone(),
                    device_id: bound_device_id.clone(),
                });
            }
            if requested_user_matches_token_scope(requested, &user.user_id) {
                let mut scoped = user.clone();
                scoped.user_id = requested.to_string();
                state
                    .control
                    .presence
                    .touch_user(&user.user_id, token_presence_kind, now_ts());
                return Ok(ResolvedUser {
                    user: scoped,
                    session_scope: token_session_scope.clone(),
                    device_id: bound_device_id.clone(),
                });
            }
        }
        if api_key_valid || token_is_admin {
            let user_store = state.user_store.clone();
            let requested_user_id = requested.to_string();
            let user = blocking::run_db("api.user_context.get_user", move || {
                user_store.get_user_by_id(&requested_user_id)
            })
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            if let Some(user) = user {
                state
                    .control
                    .presence
                    .touch_user(&user.user_id, token_presence_kind, now_ts());
                return Ok(ResolvedUser {
                    user,
                    session_scope: None,
                    device_id: None,
                });
            }
            let user = build_virtual_user(requested);
            return Ok(ResolvedUser {
                user,
                session_scope: None,
                device_id: None,
            });
        }
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            i18n::t("error.auth_required"),
        ));
    }

    if let Some(user) = token_user {
        state
            .control
            .presence
            .touch_user(&user.user_id, token_presence_kind, now_ts());
        return Ok(ResolvedUser {
            user,
            session_scope: token_session_scope,
            device_id: bound_device_id,
        });
    }

    Err(error_response(
        StatusCode::UNAUTHORIZED,
        i18n::t("error.auth_required"),
    ))
}

fn requested_user_matches_token_scope(requested: &str, token_user_id: &str) -> bool {
    if requested == token_user_id {
        return true;
    }
    let Some(mut suffix) = requested.strip_prefix(token_user_id) else {
        return false;
    };
    if suffix.is_empty() {
        return true;
    }

    loop {
        if suffix.is_empty() {
            return true;
        }
        if let Some(rest) = suffix.strip_prefix("__c__") {
            let digits_len = rest.chars().take_while(|ch| ch.is_ascii_digit()).count();
            if digits_len == 0 {
                return false;
            }
            suffix = &rest[digits_len..];
            continue;
        }
        if let Some(rest) = suffix.strip_prefix("__a__") {
            let short_scope_len = rest.chars().take_while(|ch| ch.is_ascii_hexdigit()).count();
            if short_scope_len == 0 {
                return false;
            }
            suffix = &rest[short_scope_len..];
            continue;
        }
        if let Some(rest) = suffix.strip_prefix("__agent__") {
            let legacy_scope_len = rest
                .chars()
                .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
                .count();
            if legacy_scope_len == 0 {
                return false;
            }
            suffix = &rest[legacy_scope_len..];
            continue;
        }
        return false;
    }
}

fn error_response(status: StatusCode, message: String) -> Response {
    if status == StatusCode::UNAUTHORIZED {
        return error_response_with_detail(status, Some("AUTH_REQUIRED"), message, None, None);
    }
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
        quota_balance: 0,
        quota_granted_total: 0,
        quota_used_total: 0,
        last_quota_grant_date: None,
        experience_total: 0,
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

#[cfg(test)]
mod tests {
    use super::requested_user_matches_token_scope;

    #[test]
    fn requested_user_matches_known_scope_suffixes() {
        assert!(requested_user_matches_token_scope("alice", "alice"));
        assert!(requested_user_matches_token_scope("alice__c__2", "alice"));
        assert!(requested_user_matches_token_scope(
            "alice__a__1a2b3c4d",
            "alice"
        ));
        assert!(requested_user_matches_token_scope(
            "alice__agent__writer_dev",
            "alice"
        ));
        assert!(requested_user_matches_token_scope(
            "alice__c__2__a__1a2b3c4d",
            "alice"
        ));
        assert!(requested_user_matches_token_scope(
            "alice__c__2__agent__legacy_id",
            "alice"
        ));
    }

    #[test]
    fn requested_user_rejects_other_or_invalid_scopes() {
        assert!(!requested_user_matches_token_scope("bob", "alice"));
        assert!(!requested_user_matches_token_scope("alice__c__", "alice"));
        assert!(!requested_user_matches_token_scope("alice__a__", "alice"));
        assert!(!requested_user_matches_token_scope(
            "alice__agent__",
            "alice"
        ));
        assert!(!requested_user_matches_token_scope(
            "alice__unknown__1",
            "alice"
        ));
    }
}
