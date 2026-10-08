//! Refresh-token rotation regression: local login issues a refresh token,
//! rotation retires the previous pair, replaying a rotated token triggers
//! reuse detection that revokes the whole family (including live access
//! tokens), and web-scope logins keep the old response shape.
#![cfg(feature = "sqlite-storage")]
use axum::{
    body::{to_bytes, Body},
    http::{header::AUTHORIZATION, Method, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;
use wunder_server::{
    build_router,
    config::Config,
    state::{AppState, AppStateInitOptions},
    storage::UserRefreshTokenRecord,
};

const SESSION_SCOPE_HEADER: &str = "x-wunder-session-scope";
const ERROR_CODE_HEADER: &str = "x-error-code";

struct TestContext {
    state: Arc<AppState>,
    app: Router,
    _temp_dir: TempDir,
}

async fn build_context() -> TestContext {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = Config::default();
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = temp_dir
        .path()
        .join("auth-refresh.db")
        .to_string_lossy()
        .to_string();
    config.workspace.root = temp_dir
        .path()
        .join("workspaces")
        .to_string_lossy()
        .to_string();

    let state = Arc::new(
        AppState::new_with_options(
            wunder_server::config_store::ConfigStore::new(temp_dir.path().join("wunder.yaml")),
            config,
            AppStateInitOptions::cli_default(),
        )
        .expect("create app state"),
    );
    let app = build_router(state.clone());
    TestContext {
        state,
        app,
        _temp_dir: temp_dir,
    }
}

fn create_user_id(context: &TestContext, username: &str) -> String {
    context
        .state
        .user_store
        .create_user(
            username,
            Some(format!("{username}@example.test")),
            "password-123",
            Some("A"),
            None,
            vec!["user".to_string()],
            "active",
            false,
        )
        .expect("create user")
        .user_id
}

async fn send_request(
    app: &Router,
    method: Method,
    path: &str,
    token: Option<&str>,
    session_scope: Option<&str>,
    payload: Option<Value>,
) -> (StatusCode, Value, Option<String>) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header(AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(scope) = session_scope {
        builder = builder.header(SESSION_SCOPE_HEADER, scope);
    }
    let body = if let Some(json_body) = payload {
        builder = builder.header("content-type", "application/json");
        Body::from(json_body.to_string())
    } else {
        Body::empty()
    };
    let response = app
        .clone()
        .oneshot(builder.body(body).expect("build request"))
        .await
        .expect("send request");
    let status = response.status();
    let error_code = response
        .headers()
        .get(ERROR_CODE_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body, error_code)
}

async fn login(context: &TestContext, username: &str, scope: Option<&str>) -> (StatusCode, Value) {
    let (status, body, _) = send_request(
        &context.app,
        Method::POST,
        "/wunder/auth/login",
        None,
        scope,
        Some(json!({
            "username": username,
            "password": "password-123",
        })),
    )
    .await;
    (status, body)
}

fn refresh_body(refresh_token: &str) -> Value {
    json!({ "refresh_token": refresh_token })
}

// ---------------------------------------------------------------------------
// Rotation and reuse detection
// ---------------------------------------------------------------------------

#[tokio::test]
async fn refresh_rotates_and_reuse_revokes_family() {
    let context = build_context().await;
    let username = "refresh_rotate_user";
    create_user_id(&context, username);

    let (status, login_body) = login(&context, username, Some("local_desktop")).await;
    assert_eq!(status, StatusCode::OK, "local login failed: {login_body}");
    let first_access = login_body["data"]["access_token"]
        .as_str()
        .expect("access token")
        .to_string();
    let first_refresh = login_body["data"]["refresh_token"]
        .as_str()
        .expect("refresh token")
        .to_string();
    assert!(
        first_refresh.starts_with("wundr_"),
        "refresh prefix: {first_refresh}"
    );
    assert!(
        login_body["data"]["expires_at"].is_number(),
        "expires_at present"
    );

    // The issued access token works.
    let (status, _, _) = send_request(
        &context.app,
        Method::GET,
        "/wunder/auth/me",
        Some(&first_access),
        None,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "fresh access token must authenticate"
    );

    // Rotate.
    let (status, rotate_body, _) = send_request(
        &context.app,
        Method::POST,
        "/wunder/auth/refresh",
        None,
        None,
        Some(refresh_body(&first_refresh)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "refresh failed: {rotate_body}");
    let second_access = rotate_body["data"]["access_token"]
        .as_str()
        .expect("new access token")
        .to_string();
    let second_refresh = rotate_body["data"]["refresh_token"]
        .as_str()
        .expect("new refresh token")
        .to_string();
    assert_ne!(second_access, first_access);
    assert_ne!(second_refresh, first_refresh);

    // New access token works, old one is retired.
    let (status, _, _) = send_request(
        &context.app,
        Method::GET,
        "/wunder/auth/me",
        Some(&second_access),
        None,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "rotated access token must authenticate"
    );
    let (status, _, _) = send_request(
        &context.app,
        Method::GET,
        "/wunder/auth/me",
        Some(&first_access),
        None,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "old access token must be retired"
    );

    // Replaying the rotated refresh token burns the whole family.
    let (status, body, error_code) = send_request(
        &context.app,
        Method::POST,
        "/wunder/auth/refresh",
        None,
        None,
        Some(refresh_body(&first_refresh)),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "replay body: {body}");
    assert_eq!(error_code.as_deref(), Some("REFRESH_TOKEN_REUSED"));

    // The live access token of the family goes down with it.
    let (status, _, _) = send_request(
        &context.app,
        Method::GET,
        "/wunder/auth/me",
        Some(&second_access),
        None,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "family access token must be revoked"
    );

    // The replacement refresh token was revoked as part of the family.
    let (status, _, error_code) = send_request(
        &context.app,
        Method::POST,
        "/wunder/auth/refresh",
        None,
        None,
        Some(refresh_body(&second_refresh)),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code.as_deref(), Some("REFRESH_TOKEN_REUSED"));
}

// ---------------------------------------------------------------------------
// Scope gating
// ---------------------------------------------------------------------------

#[tokio::test]
async fn web_scope_login_has_no_refresh_token() {
    let context = build_context().await;
    let username = "web_scope_user";
    create_user_id(&context, username);

    let (status, body) = login(&context, username, None).await;
    assert_eq!(status, StatusCode::OK, "web login failed: {body}");
    assert!(body["data"]["access_token"].as_str().is_some());
    assert!(
        body["data"].get("refresh_token").is_none(),
        "web scope must not receive a refresh token: {body}"
    );
}

// ---------------------------------------------------------------------------
// Error codes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn refresh_with_unknown_token_returns_invalid() {
    let context = build_context().await;
    let (status, body, error_code) = send_request(
        &context.app,
        Method::POST,
        "/wunder/auth/refresh",
        None,
        None,
        Some(refresh_body("wundr_does_not_exist")),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "body: {body}");
    assert_eq!(error_code.as_deref(), Some("REFRESH_TOKEN_INVALID"));
}

#[tokio::test]
async fn refresh_with_expired_token_returns_expired_and_cleans_up() {
    let context = build_context().await;
    let username = "expired_refresh_user";
    let user_id = create_user_id(&context, username);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs_f64();
    let expired_token = "wundr_expired_probe".to_string();
    context
        .state
        .storage
        .create_user_refresh_token(&UserRefreshTokenRecord {
            refresh_token: expired_token.clone(),
            user_id: user_id.clone(),
            session_scope: "local_desktop".to_string(),
            family_id: "expired-family".to_string(),
            expires_at: now - 60.0,
            created_at: now - 3600.0,
            last_used_at: now - 3600.0,
            revoked: false,
        })
        .expect("seed expired refresh token");

    let (status, body, error_code) = send_request(
        &context.app,
        Method::POST,
        "/wunder/auth/refresh",
        None,
        None,
        Some(refresh_body(&expired_token)),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "body: {body}");
    assert_eq!(error_code.as_deref(), Some("REFRESH_TOKEN_EXPIRED"));
    assert!(
        context
            .state
            .storage
            .get_user_refresh_token(&expired_token)
            .expect("lookup after expiry")
            .is_none(),
        "expired row must be cleaned up"
    );
}

// ---------------------------------------------------------------------------
// Multi-device semantics: a new local login kicks the previous session
// ---------------------------------------------------------------------------

#[tokio::test]
async fn relogin_kicks_previous_local_family() {
    let context = build_context().await;
    let username = "relogin_kick_user";
    create_user_id(&context, username);

    // First local login.
    let (status, first_login) = login(&context, username, Some("local_desktop")).await;
    assert_eq!(status, StatusCode::OK, "first login failed: {first_login}");
    let first_access = first_login["data"]["access_token"]
        .as_str()
        .expect("first access token")
        .to_string();
    let first_refresh = first_login["data"]["refresh_token"]
        .as_str()
        .expect("first refresh token")
        .to_string();

    // Second login on the same scope replaces the first session.
    let (status, second_login) = login(&context, username, Some("local_desktop")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "second login failed: {second_login}"
    );
    let second_access = second_login["data"]["access_token"]
        .as_str()
        .expect("second access token")
        .to_string();
    let second_refresh = second_login["data"]["refresh_token"]
        .as_str()
        .expect("second refresh token")
        .to_string();

    // The kicked session's access token is dead immediately.
    let (status, _, _) = send_request(
        &context.app,
        Method::GET,
        "/wunder/auth/me",
        Some(&first_access),
        None,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "kicked access token must fail"
    );

    // The kicked refresh token is revoked, so refreshing it hits the reuse
    // branch: 401 with REFRESH_TOKEN_REUSED (not resurrecting the session).
    let (status, body, error_code) = send_request(
        &context.app,
        Method::POST,
        "/wunder/auth/refresh",
        None,
        None,
        Some(refresh_body(&first_refresh)),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "body: {body}");
    assert_eq!(error_code.as_deref(), Some("REFRESH_TOKEN_REUSED"));

    // The replacement session keeps working end to end.
    let (status, _, _) = send_request(
        &context.app,
        Method::GET,
        "/wunder/auth/me",
        Some(&second_access),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "new session must authenticate");
    let (status, rotate_body, _) = send_request(
        &context.app,
        Method::POST,
        "/wunder/auth/refresh",
        None,
        None,
        Some(refresh_body(&second_refresh)),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "new family must rotate: {rotate_body}"
    );
    let third_access = rotate_body["data"]["access_token"]
        .as_str()
        .expect("third access token")
        .to_string();
    let (status, _, _) = send_request(
        &context.app,
        Method::GET,
        "/wunder/auth/me",
        Some(&third_access),
        None,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "rotated token of live family must work"
    );
}
