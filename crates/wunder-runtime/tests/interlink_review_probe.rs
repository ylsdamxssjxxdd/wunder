//! Cloud access surface (T2+T3) route regression: scope guard, device
//! registration idempotency, exposed model list, quota/concurrency admission
//! shapes, log upload idempotency and device revocation.
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
    config::{Config, LlmModelConfig},
    config_store::ConfigStore,
    state::{AppState, AppStateInitOptions},
};

const DEVICE_HEADER: &str = "x-wunder-device-id";

#[tokio::test]
async fn concurrent_registration_binds_one_device_without_orphan_records() {
    let context = build_context(|_| {}).await;
    let user = create_user_id(&context, "fixture_registration");
    let token = create_local_token(&context, &user);
    let first = send_request(
        &context.app,
        Method::POST,
        "/wunder/cloud/devices",
        Some(&token),
        &[],
        Some(register_device_body("desktop", "fixture_a")),
    );
    let second = send_request(
        &context.app,
        Method::POST,
        "/wunder/cloud/devices",
        Some(&token),
        &[],
        Some(register_device_body("desktop", "fixture_b")),
    );
    let (first, second) = tokio::join!(first, second);
    assert!(first.0 == StatusCode::OK || second.0 == StatusCode::OK);
    let (devices, total) = context
        .state
        .storage
        .list_cloud_devices(Some(&user), 0, 50)
        .unwrap();
    assert_eq!(total, 1);
    let auth = context
        .state
        .user_store
        .authenticate_token_details(&token)
        .unwrap()
        .unwrap();
    let binding = context
        .state
        .storage
        .get_user_session_device(auth.family_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(binding.device_id, devices[0].device_id);
}

struct TestContext {
    state: Arc<AppState>,
    app: Router,
    admin_token: String,
    _temp_dir: TempDir,
}

#[tokio::test]
async fn immutable_device_identity_blocks_header_body_and_scope_spoofing() {
    let context = build_context(|_| {}).await;
    let user = create_user_id(&context, "fixture_identity");
    let first = create_local_token(&context, &user);
    let a = register_device(&context, &first, "desktop", "fixture_a").await;
    let second = create_local_token(&context, &user);
    let b = register_device(&context, &second, "desktop", "fixture_b").await;
    assert_ne!(a, b);
    let (status, _, _) = send_request(
        &context.app,
        Method::GET,
        "/wunder/cloud/account",
        Some(&first),
        &[(DEVICE_HEADER, &b)],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, body, _) = send_request(
        &context.app,
        Method::POST,
        "/wunder/interlink/commands",
        Some(&first),
        &[],
        Some(json!({"to":"cloud","kind":"node.summary","from":"device:spoofed","args":{}})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["from"], format!("device:{a}"));
    let (status, body, _) = send_request(
        &context.app, Method::POST, "/wunder/interlink/commands", Some(&first), &[],
        Some(json!({"to":"cloud","kind":"thread.cancel","args":{"local_thread_id":"fixture_missing"}})),
    ).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["data"]["approval_state"], "none",
        "own cloud thread control must not park behind an invisible approval"
    );
    for endpoint in [
        "/wunder/interlink/node_secret",
        "/wunder/interlink/channel_ticket",
    ] {
        let (status, _, _) = send_request(
            &context.app,
            Method::POST,
            endpoint,
            Some(&first),
            &[],
            Some(json!({"device_id":b})),
        )
        .await;
        assert!(
            matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN),
            "{endpoint}: {status}"
        );
    }
    let (status, _, _) = send_request(
        &context.app,
        Method::POST,
        "/wunder/cloud/devices",
        Some(&first),
        &[],
        Some(register_device_body("cli", "fixture_c")),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let unbound = create_local_token(&context, &user);
    let (status, _, _) = send_request(
        &context.app,
        Method::POST,
        "/wunder/interlink/commands",
        Some(&unbound),
        &[],
        Some(json!({"to":"cloud","kind":"node.summary","args":{}})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

fn build_llm_model(base_url: Option<String>, expose: Option<bool>) -> LlmModelConfig {
    LlmModelConfig {
        enable: Some(true),
        provider: Some("openai_compatible".to_string()),
        api_mode: None,
        base_url: base_url.or_else(|| Some("http://127.0.0.1:18099/v1".to_string())),
        api_key: Some("test-key".to_string()),
        model: Some("provider-model".to_string()),
        temperature: Some(0.0),
        timeout_s: Some(5),
        max_rounds: Some(4),
        max_context: Some(16_384),
        max_output: Some(256),
        stream: Some(false),
        tool_call_mode: Some("tool_call".to_string()),
        model_type: Some("llm".to_string()),
        expose,
        ..Default::default()
    }
}

async fn build_context<F>(configure: F) -> TestContext
where
    F: FnOnce(&mut Config),
{
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = Config::default();
    config.storage.backend = "sqlite".to_string();
    config.storage.data_root = temp_dir.path().join("data").to_string_lossy().to_string();
    config.workspace.root = temp_dir
        .path()
        .join("workspaces")
        .to_string_lossy()
        .to_string();
    config.skills.enabled.clear();
    config.llm.default = "cloud-model".to_string();
    config.llm.models.clear();
    config
        .llm
        .models
        .insert("cloud-model".to_string(), build_llm_model(None, Some(true)));
    configure(&mut config);

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
    state
        .user_store
        .ensure_default_admin()
        .expect("ensure default admin");
    let admin_token = state
        .user_store
        .create_session_token("admin")
        .expect("create admin token")
        .token;
    let app = build_router(state.clone());
    TestContext {
        state,
        app,
        admin_token,
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

fn create_local_token(context: &TestContext, user_id: &str) -> String {
    context
        .state
        .user_store
        .create_session_token_with_scope(user_id, "local_desktop")
        .expect("create local token")
        .token
}

async fn send_request(
    app: &Router,
    method: Method,
    path: &str,
    token: Option<&str>,
    headers: &[(&str, &str)],
    payload: Option<Value>,
) -> (StatusCode, Value, Vec<(String, String)>) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header(AUTHORIZATION, format!("Bearer {token}"));
    }
    for (name, value) in headers {
        builder = builder.header(*name, *value);
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
    let response_headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_ascii_lowercase(),
                value.to_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body, response_headers)
}

fn register_device_body(client: &str, name: &str) -> Value {
    json!({
        "client": client,
        "name": name,
        "os": "test-os",
        "arch": "test-arch",
        "app_version": "0.0.0",
    })
}

async fn register_device(context: &TestContext, token: &str, client: &str, name: &str) -> String {
    let (status, body, _) = send_request(
        &context.app,
        Method::POST,
        "/wunder/cloud/devices",
        Some(token),
        &[],
        Some(register_device_body(client, name)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "register device failed: {body}");
    body["data"]["device_id"]
        .as_str()
        .expect("device_id in response")
        .to_string()
}

#[tokio::test]
async fn admin_logout_invalidates_every_user_surface() {
    let context = build_context(|_| {}).await;
    let user_id = create_user_id(&context, "fixture_account");
    let mut sessions = Vec::new();
    for scope in ["user_web", "local_desktop", "local_cli"] {
        let token = context
            .state
            .user_store
            .create_session_token_with_scope(&user_id, scope)
            .unwrap()
            .token;
        sessions.push((scope, token));
    }
    let (status, body, _) = send_request(
        &context.app,
        Method::POST,
        &format!("/wunder/admin/user_accounts/{user_id}/logout"),
        Some(&context.admin_token),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["data"]["session_scopes"],
        json!(["user_web", "default", "local_desktop", "local_cli"])
    );
    for (scope, token) in sessions {
        let (status, _, _) = send_request(
            &context.app,
            Method::GET,
            "/wunder/auth/me",
            Some(&token),
            &[],
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        println!("review logout scope={scope} status={status}");
    }
}

#[tokio::test]
async fn device_revocation_invalidates_cloud_and_interlink() {
    let context = build_context(|_| {}).await;
    let user_id = create_user_id(&context, "fixture_account");
    let token = create_local_token(&context, &user_id);
    let device_id = register_device(&context, &token, "desktop", "fixture_device").await;
    let (status, body, _) = send_request(
        &context.app,
        Method::DELETE,
        &format!("/wunder/admin/cloud/devices/{device_id}"),
        Some(&context.admin_token),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, _, _) = send_request(
        &context.app,
        Method::GET,
        "/wunder/cloud/account",
        Some(&token),
        &[(DEVICE_HEADER, &device_id)],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, body, _) = send_request(
        &context.app,
        Method::POST,
        "/wunder/interlink/commands",
        Some(&token),
        &[],
        Some(json!({"to": "cloud", "kind": "node.summary", "args": {}})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
}

#[tokio::test]
async fn independent_local_logins_coexist() {
    let context = build_context(|_| {}).await;
    create_user_id(&context, "fixture_account");
    let store = &context.state.user_store;
    let (first, refresh) = store
        .login_local_with_refresh_token("fixture_account", "password-123", "local_desktop")
        .unwrap();
    let (second, _) = store
        .login_local_with_refresh_token("fixture_account", "password-123", "local_desktop")
        .unwrap();
    assert!(store
        .authenticate_token_details(&first.token.token)
        .unwrap()
        .is_some());
    assert!(store
        .authenticate_token_details(&second.token.token)
        .unwrap()
        .is_some());
    assert!(
        !context
            .state
            .storage
            .get_user_refresh_token(&refresh.refresh_token)
            .unwrap()
            .unwrap()
            .revoked
    );
}
