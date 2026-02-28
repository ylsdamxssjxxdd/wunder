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
                state.user_presence.touch(&user.user_id, now_ts());
                return Ok(ResolvedUser { user: user.clone() });
            }
            if requested_user_matches_token_scope(requested, &user.user_id) {
                let mut scoped = user.clone();
                scoped.user_id = requested.to_string();
                state.user_presence.touch(&user.user_id, now_ts());
                return Ok(ResolvedUser { user: scoped });
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
            if let Some(user) = user {
                state.user_presence.touch(&user.user_id, now_ts());
                return Ok(ResolvedUser { user });
            }
            let user = build_virtual_user(requested);
            return Ok(ResolvedUser { user });
        }
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            i18n::t("error.auth_required"),
        ));
    }

    if let Some(user) = token_user {
        state.user_presence.touch(&user.user_id, now_ts());
        return Ok(ResolvedUser { user });
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
