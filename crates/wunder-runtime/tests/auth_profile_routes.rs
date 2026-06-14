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
    build_desktop_router,
    config::Config,
    config_store::ConfigStore,
    state::{AppState, AppStateInitOptions},
};

struct TestContext {
    app: Router,
    state: Arc<AppState>,
    token: String,
    user_id: String,
    _temp_dir: TempDir,
}

async fn build_test_context(username: &str) -> TestContext {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = Config::default();
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = temp_dir
        .path()
        .join("auth-profile-routes.db")
        .to_string_lossy()
        .to_string();
    config.workspace.root = temp_dir
        .path()
        .join("workspaces")
        .to_string_lossy()
        .to_string();

    let config_store = ConfigStore::new(temp_dir.path().join("wunder.yaml"));
    let config_for_store = config.clone();
    config_store
        .update(|current| *current = config_for_store.clone())
        .await
        .expect("update config store");

    let state = Arc::new(
        AppState::new_with_options(config_store, config, AppStateInitOptions::cli_default())
            .expect("create app state"),
    );
    let user = state
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
        .expect("create user");
    let token = state
        .user_store
        .create_session_token(&user.user_id)
        .expect("create token")
        .token;

    TestContext {
        app: build_desktop_router(state.clone()),
        state,
        token,
        user_id: user.user_id,
        _temp_dir: temp_dir,
    }
}

async fn send_json(
    app: &Router,
    token: &str,
    method: Method,
    path: &str,
    payload: Value,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .expect("build request"),
        )
        .await
        .expect("send request");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    let payload = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("parse response json")
    };
    (status, payload)
}

#[tokio::test]
async fn auth_me_patch_updates_user_password() {
    let context = build_test_context("profile_route_user").await;

    let (status, payload) = send_json(
        &context.app,
        &context.token,
        Method::PATCH,
        "/wunder/auth/me",
        json!({
            "current_password": "password-123",
            "new_password": "password-456"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        payload.pointer("/data/id").and_then(Value::as_str),
        Some(context.user_id.as_str())
    );

    let updated = context
        .state
        .user_store
        .login("profile_route_user", "password-456")
        .expect("login with updated password");
    assert_eq!(updated.user.user_id, context.user_id);
    let error = context
        .state
        .user_store
        .login("profile_route_user", "password-123")
        .expect_err("old password should be rejected");
    assert_eq!(error.to_string(), "invalid password");
}

#[tokio::test]
async fn auth_me_patch_rejects_wrong_current_password() {
    let context = build_test_context("profile_route_user_denied").await;

    let (status, payload) = send_json(
        &context.app,
        &context.token,
        Method::PATCH,
        "/wunder/auth/me",
        json!({
            "current_password": "wrong-password",
            "new_password": "password-456"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        payload.pointer("/error/message").and_then(Value::as_str),
        Some("当前密码不正确")
    );

    let original = context
        .state
        .user_store
        .login("profile_route_user_denied", "password-123")
        .expect("original password should still work");
    assert_eq!(original.user.user_id, context.user_id);
}
