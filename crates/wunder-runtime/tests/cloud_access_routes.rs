//! Cloud access surface (T2+T3) route regression: scope guard, device
//! registration idempotency, exposed model list, quota/concurrency admission
//! shapes, log upload idempotency and device revocation.
use axum::{
    body::{to_bytes, Body},
    http::{header::AUTHORIZATION, Method, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use std::{path::Path, sync::Arc, time::Duration};
use tempfile::TempDir;
use tower::ServiceExt;
use wunder_server::{
    build_router,
    config::{Config, LlmModelConfig},
    config_store::ConfigStore,
    state::{AppState, AppStateInitOptions},
};

const DEVICE_HEADER: &str = "x-wunder-device-id";
const QUEUE_HEADER: &str = "x-wunder-queue-id";
const REQUEST_ID_HEADER: &str = "x-wunder-request-id";

struct TestContext {
    state: Arc<AppState>,
    app: Router,
    admin_token: String,
    _temp_dir: TempDir,
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
    config.storage.db_path = temp_dir
        .path()
        .join("cloud-access.db")
        .to_string_lossy()
        .to_string();
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

// ---------------------------------------------------------------------------
// Auth and scope
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cloud_requires_bearer_token() {
    let context = build_context(|_| {}).await;
    for (method, path) in [
        (Method::POST, "/wunder/cloud/devices"),
        (Method::GET, "/wunder/cloud/account"),
        (Method::GET, "/wunder/cloud/v1/models"),
    ] {
        let payload = if method == Method::POST {
            Some(register_device_body("desktop", "anon-host"))
        } else {
            None
        };
        let (status, body, _) = send_request(&context.app, method, path, None, &[], payload).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "path {path}: {body}");
    }
}

#[tokio::test]
async fn cloud_rejects_non_local_scope() {
    let context = build_context(|_| {}).await;
    let user_id = create_user_id(&context, "scope_user");
    let web_token = context
        .state
        .user_store
        .create_session_token(&user_id)
        .expect("token")
        .token;
    let (status, body, _) = send_request(
        &context.app,
        Method::GET,
        "/wunder/cloud/account",
        Some(&web_token),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], json!("CLOUD_SCOPE_FORBIDDEN"));
}

// ---------------------------------------------------------------------------
// Device registration
// ---------------------------------------------------------------------------

#[tokio::test]
async fn device_registration_is_idempotent() {
    let context = build_context(|_| {}).await;
    let user_id = create_user_id(&context, "device_user");
    let token = create_local_token(&context, &user_id);

    let device_a = register_device(&context, &token, "desktop", "workstation").await;
    let device_a_again = register_device(&context, &token, "desktop", "workstation").await;
    assert_eq!(device_a, device_a_again, "same identity reuses the device");

    let device_b = register_device(&context, &token, "cli", "workstation").await;
    assert_ne!(device_a, device_b, "different client means new device");

    let (_, body, _) = send_request(
        &context.app,
        Method::GET,
        "/wunder/cloud/account",
        Some(&token),
        &[(DEVICE_HEADER, device_a.as_str())],
        None,
    )
    .await;
    assert_eq!(body["data"]["device"]["device_id"], json!(device_a));
    assert_eq!(body["data"]["concurrency"]["max_per_user"], json!(2));
    assert!(body["data"]["quota"]["daily_grant"].as_i64().unwrap_or(0) > 0);
}

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

#[tokio::test]
async fn models_expose_only_allowed_models() {
    let context = build_context(|config| {
        config.llm.models.clear();
        config.llm.models.insert(
            "model-exposed".to_string(),
            build_llm_model(None, Some(true)),
        );
        config
            .llm
            .models
            .insert("model-allow".to_string(), build_llm_model(None, None));
        config
            .llm
            .models
            .insert("model-hidden".to_string(), build_llm_model(None, None));
        config.llm.models.insert(
            "model-off".to_string(),
            LlmModelConfig {
                enable: Some(false),
                ..build_llm_model(None, Some(true))
            },
        );
        config.cloud.expose_models = vec!["model-allow".to_string()];
    })
    .await;
    let user_id = create_user_id(&context, "models_user");
    let token = create_local_token(&context, &user_id);
    let device_id = register_device(&context, &token, "desktop", "models-host").await;

    let (status, body, _) = send_request(
        &context.app,
        Method::GET,
        "/wunder/cloud/v1/models",
        Some(&token),
        &[(DEVICE_HEADER, device_id.as_str())],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let entries = body["data"].as_array().expect("data array");
    let ids = entries
        .iter()
        .map(|entry| entry["id"].as_str().expect("id").to_string())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["model-allow", "model-exposed"]);
    for entry in entries {
        assert_eq!(entry["object"], json!("model"));
        assert_eq!(entry["owned_by"], json!("wunder"));
        assert!(entry.get("api_key").is_none());
        assert!(entry.get("base_url").is_none());
        assert!(entry.get("provider").is_none());
        assert_eq!(entry["context"], json!(16_384));
    }
    assert_eq!(body["data"][0]["is_default"], json!(false));
}

// ---------------------------------------------------------------------------
// Quota admission
// ---------------------------------------------------------------------------

#[tokio::test]
async fn chat_quota_insufficient_shape() {
    let context = build_context(|_| {}).await;
    let user_id = create_user_id(&context, "poor_user");
    let token = create_local_token(&context, &user_id);
    let device_id = register_device(&context, &token, "desktop", "poor-host").await;

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let daily_grant = 1000;
    context
        .state
        .storage
        .set_user_quota_balance(&user_id, &today, daily_grant, 0)
        .expect("set quota balance");

    let payload = json!({
        "model": "cloud-model",
        "messages": [{"role": "user", "content": "hello"}],
        "stream": false,
    });
    let (status, body, headers) = send_request(
        &context.app,
        Method::POST,
        "/wunder/cloud/v1/chat/completions",
        Some(&token),
        &[(DEVICE_HEADER, device_id.as_str())],
        Some(payload),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["error"]["code"], json!("USER_QUOTA_INSUFFICIENT"));
    let quota_account = &body["error"]["quota_account"];
    assert_eq!(quota_account["quota_balance"], json!(0));
    assert_eq!(quota_account["daily_quota_grant"], json!(daily_grant));
    assert!(
        headers.iter().any(|(name, _)| name == REQUEST_ID_HEADER),
        "request id header must be present"
    );

    // quota_blocked call record visible on the admin surface.
    let (status, body, _) = send_request(
        &context.app,
        Method::GET,
        &format!("/wunder/admin/cloud/calls?user_id={user_id}&status=quota_blocked"),
        Some(&context.admin_token),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["total"], json!(1));
    assert_eq!(body["data"]["items"][0]["quota_consumed"], json!(false));
}

// ---------------------------------------------------------------------------
// Concurrency admission (queue semantics, no real upstream)
// ---------------------------------------------------------------------------

async fn spawn_hanging_upstream() -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind hanging upstream");
    let addr = listener.local_addr().expect("local addr");
    let handle = tokio::spawn(async move {
        let mut held = Vec::new();
        loop {
            match listener.accept().await {
                Ok((socket, _)) => held.push(socket),
                Err(_) => break,
            }
        }
    });
    (addr, handle)
}

#[tokio::test]
async fn chat_concurrency_queue_semantics() {
    let (addr, upstream_handle) = spawn_hanging_upstream().await;
    let context = build_context(|config| {
        config.cloud.max_concurrent_calls_per_user = 1;
        config.llm.models.clear();
        config.llm.models.insert(
            "cloud-model".to_string(),
            build_llm_model(Some(format!("http://{addr}/v1")), Some(true)),
        );
    })
    .await;
    let user_id = create_user_id(&context, "queue_user");
    let token = create_local_token(&context, &user_id);
    let device_id = register_device(&context, &token, "desktop", "queue-host").await;
    let payload = json!({
        "model": "cloud-model",
        "messages": [{"role": "user", "content": "hello"}],
        "stream": false,
    });

    // Occupies the only slot and hangs on the upstream.
    let first_app = context.app.clone();
    let first_token = token.clone();
    let first_device = device_id.clone();
    let first_payload = json!({
        "model": "cloud-model",
        "messages": [{"role": "user", "content": "hello"}],
        "stream": false,
    });
    let first_task = tokio::spawn(async move {
        send_request(
            &first_app,
            Method::POST,
            "/wunder/cloud/v1/chat/completions",
            Some(&first_token),
            &[(DEVICE_HEADER, first_device.as_str())],
            Some(first_payload),
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(400)).await;

    // Full: enqueue with a fresh queue id.
    let payload = json!({
        "model": "cloud-model",
        "messages": [{"role": "user", "content": "queued"}],
        "stream": false,
    });
    let (status, body, headers) = send_request(
        &context.app,
        Method::POST,
        "/wunder/cloud/v1/chat/completions",
        Some(&token),
        &[(DEVICE_HEADER, device_id.as_str())],
        Some(payload.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["error"]["code"], json!("CLOUD_BUSY"));
    let queue_id = body["error"]["queue_id"]
        .as_str()
        .expect("queue id")
        .to_string();
    assert_eq!(body["error"]["queue_ahead"], json!(0));
    assert_eq!(body["error"]["retry_after_ms"], json!(3000));
    assert!(
        headers
            .iter()
            .any(|(name, value)| name == QUEUE_HEADER && *value == queue_id),
        "queue id header must mirror the body"
    );

    // Queue-front retry while the slot is still busy: still queued at 0.
    let (status, body, _) = send_request(
        &context.app,
        Method::POST,
        "/wunder/cloud/v1/chat/completions",
        Some(&token),
        &[
            (DEVICE_HEADER, device_id.as_str()),
            (QUEUE_HEADER, queue_id.as_str()),
        ],
        Some(payload.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"]["queue_id"], json!(queue_id));
    assert_eq!(body["error"]["queue_ahead"], json!(0));

    // A second waiter lands behind the first.
    let (status, body, _) = send_request(
        &context.app,
        Method::POST,
        "/wunder/cloud/v1/chat/completions",
        Some(&token),
        &[(DEVICE_HEADER, device_id.as_str())],
        Some(payload.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"]["queue_ahead"], json!(1));

    // Free the slot by failing the hung call.
    upstream_handle.abort();
    let (first_status, first_body, _) = tokio::time::timeout(Duration::from_secs(10), first_task)
        .await
        .expect("first call settles")
        .expect("join first task");
    assert_eq!(first_status, StatusCode::BAD_GATEWAY, "{first_body}");

    // Queue-front retry with the same id is now admitted (it reaches the dead
    // upstream instead of returning CLOUD_BUSY).
    let (status, body, headers) = send_request(
        &context.app,
        Method::POST,
        "/wunder/cloud/v1/chat/completions",
        Some(&token),
        &[
            (DEVICE_HEADER, device_id.as_str()),
            (QUEUE_HEADER, queue_id.as_str()),
        ],
        Some(payload),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(body["error"]["code"], json!("UPSTREAM_ERROR"));
    assert!(
        headers.iter().any(|(name, _)| name == REQUEST_ID_HEADER),
        "admitted calls carry the request id"
    );
}

// ---------------------------------------------------------------------------
// Device logs
// ---------------------------------------------------------------------------

fn log_entry(seq: i64, message: &str) -> Value {
    json!({
        "seq": seq,
        "level": "info",
        "category": "runtime",
        "event": "heartbeat",
        "message": message,
        "local_session_id": null,
        "created_at": 1_700_000_000.0,
    })
}

#[tokio::test]
async fn logs_upload_is_idempotent_and_bounded() {
    let context = build_context(|_| {}).await;
    let user_id = create_user_id(&context, "log_user");
    let token = create_local_token(&context, &user_id);
    let device_id = register_device(&context, &token, "cli", "log-host").await;

    let payload = json!({
        "device_id": device_id,
        "client": "cli",
        "logs": [log_entry(1, "first"), log_entry(2, "second")],
    });
    let (status, body, _) = send_request(
        &context.app,
        Method::POST,
        "/wunder/cloud/logs",
        Some(&token),
        &[],
        Some(payload.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["accepted"], json!(2));
    assert_eq!(body["data"]["last_seq"], json!(2));

    // Replay: duplicates are skipped, cursor stays at the reported max.
    let (status, body, _) = send_request(
        &context.app,
        Method::POST,
        "/wunder/cloud/logs",
        Some(&token),
        &[],
        Some(payload),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["accepted"], json!(0));
    assert_eq!(body["data"]["last_seq"], json!(2));

    // More than 100 entries is rejected.
    let oversized = json!({
        "device_id": device_id,
        "client": "cli",
        "logs": (0..101).map(|seq| log_entry(seq, "x")).collect::<Vec<_>>(),
    });
    let (status, body, _) = send_request(
        &context.app,
        Method::POST,
        "/wunder/cloud/logs",
        Some(&token),
        &[],
        Some(oversized),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], json!("PAYLOAD_TOO_LARGE"));

    // Body beyond 256KB is rejected.
    let fat = "m".repeat(3 * 1024);
    let fat_payload = json!({
        "device_id": device_id,
        "client": "cli",
        "logs": (0..100).map(|seq| log_entry(seq, &fat)).collect::<Vec<_>>(),
    });
    assert!(fat_payload.to_string().len() > 256 * 1024);
    let (status, body, _) = send_request(
        &context.app,
        Method::POST,
        "/wunder/cloud/logs",
        Some(&token),
        &[],
        Some(fat_payload),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], json!("PAYLOAD_TOO_LARGE"));

    // Device of another user is rejected.
    let other_id = create_user_id(&context, "log_other");
    let other_token = create_local_token(&context, &other_id);
    let (status, body, _) = send_request(
        &context.app,
        Method::POST,
        "/wunder/cloud/logs",
        Some(&other_token),
        &[],
        Some(json!({
            "device_id": device_id,
            "client": "cli",
            "logs": [log_entry(3, "intrusion")],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body["error"]["code"], json!("DEVICE_REVOKED"));
}

// ---------------------------------------------------------------------------
// Revocation and admin surface
// ---------------------------------------------------------------------------

#[tokio::test]
async fn device_revocation_blocks_access() {
    let context = build_context(|_| {}).await;
    let user_id = create_user_id(&context, "revoke_user");
    let token = create_local_token(&context, &user_id);
    let device_id = register_device(&context, &token, "desktop", "revoke-host").await;

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
    assert_eq!(body["data"]["revoked"], json!(true));

    let (status, body, _) = send_request(
        &context.app,
        Method::GET,
        "/wunder/cloud/account",
        Some(&token),
        &[(DEVICE_HEADER, device_id.as_str())],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body["error"]["code"], json!("DEVICE_REVOKED"));

    // Re-login on the same identity keeps the revoked flag.
    let same_device = register_device(&context, &token, "desktop", "revoke-host").await;
    assert_eq!(same_device, device_id);
    let (status, body, _) = send_request(
        &context.app,
        Method::GET,
        "/wunder/cloud/account",
        Some(&token),
        &[(DEVICE_HEADER, device_id.as_str())],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");

    let (status, body, _) = send_request(
        &context.app,
        Method::GET,
        &format!("/wunder/admin/cloud/devices?user_id={user_id}"),
        Some(&context.admin_token),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["items"][0]["revoked"], json!(true));
    assert_eq!(body["data"]["items"][0]["device_id"], json!(device_id));

    let (status, body, _) = send_request(
        &context.app,
        Method::GET,
        "/wunder/admin/cloud/device_logs",
        Some(&context.admin_token),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["data"]["items"].is_array());
}

// ---------------------------------------------------------------------------
// Disabled switch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cloud_disabled_returns_404_for_user_surface() {
    let context = build_context(|config| {
        config.cloud.enabled = false;
    })
    .await;
    let user_id = create_user_id(&context, "disabled_user");
    let token = create_local_token(&context, &user_id);
    for (method, path) in [
        (Method::POST, "/wunder/cloud/devices"),
        (Method::GET, "/wunder/cloud/account"),
        (Method::GET, "/wunder/cloud/v1/models"),
    ] {
        let payload = if method == Method::POST {
            Some(register_device_body("desktop", "disabled-host"))
        } else {
            None
        };
        let (status, body, _) =
            send_request(&context.app, method, path, Some(&token), &[], payload).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "path {path}: {body}");
        assert_eq!(body["error"]["code"], json!("CLOUD_DISABLED"));
    }

    // Admin surface stays available while the user surface is closed.
    let (status, _, _) = send_request(
        &context.app,
        Method::GET,
        "/wunder/admin/cloud/devices",
        Some(&context.admin_token),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn admin_lists_support_pagination_filters() {
    let context = build_context(|_| {}).await;
    let user_id = create_user_id(&context, "page_user");
    let token = create_local_token(&context, &user_id);
    let device_id = register_device(&context, &token, "desktop", "page-host").await;

    let payload = json!({
        "device_id": device_id,
        "client": "desktop",
        "logs": [
            {"seq": 1, "level": "info", "category": "runtime", "event": "a", "created_at": 1.0},
            {"seq": 2, "level": "error", "category": "runtime", "event": "b", "created_at": 2.0},
        ],
    });
    let (status, body, _) = send_request(
        &context.app,
        Method::POST,
        "/wunder/cloud/logs",
        Some(&token),
        &[],
        Some(payload),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body, _) = send_request(
        &context.app,
        Method::GET,
        &format!("/wunder/admin/cloud/device_logs?device_id={device_id}&level=error"),
        Some(&context.admin_token),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["total"], json!(1));
    assert_eq!(body["data"]["items"][0]["event"], json!("b"));

    let (status, body, _) = send_request(
        &context.app,
        Method::GET,
        "/wunder/admin/cloud/device_logs?offset=0&limit=1",
        Some(&context.admin_token),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["items"].as_array().expect("items").len(), 1);

    let (status, body, _) = send_request(
        &context.app,
        Method::GET,
        &format!("/wunder/admin/cloud/calls?device_id={device_id}"),
        Some(&context.admin_token),
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["total"], json!(0));
}
