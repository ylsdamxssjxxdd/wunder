use axum::{
    body::{to_bytes, Body},
    http::{Method, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::time::{sleep, Duration};
use tower::ServiceExt;
use wunder_server::{
    build_router,
    config::Config,
    config_store::ConfigStore,
    state::{AppState, AppStateInitOptions},
    storage::ChannelAccountRecord,
};

struct TestContext {
    app: Router,
    state: Arc<AppState>,
    _temp_dir: TempDir,
}

async fn build_test_context() -> TestContext {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = Config::default();
    config.channels.enabled = true;
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = temp_dir
        .path()
        .join("qqbot-webhook-runtime-logs.db")
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

    TestContext {
        app: build_router(state.clone()),
        state,
        _temp_dir: temp_dir,
    }
}

async fn send_json(
    app: &Router,
    method: Method,
    path: &str,
    payload: Value,
) -> (StatusCode, Value) {
    send_json_with_headers(app, method, path, payload, &[]).await
}

async fn send_json_with_headers(
    app: &Router,
    method: Method,
    path: &str,
    payload: Value,
    extra_headers: &[(&str, &str)],
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    for (key, value) in extra_headers {
        builder = builder.header(*key, *value);
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(Body::from(payload.to_string()))
                .expect("build request"),
        )
        .await
        .expect("send request");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    let payload = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("parse response json")
    };
    (status, payload)
}

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn qqbot_webhook_resolves_account_by_app_id_and_records_validation_log() {
    let context = build_test_context().await;
    let account_id = "uacc_qq_runtime_1";
    context
        .state
        .storage
        .upsert_channel_account(&ChannelAccountRecord {
            channel: "qqbot".to_string(),
            account_id: account_id.to_string(),
            config: json!({
                "qqbot": {
                    "app_id": "1234567890",
                    "client_secret": "test-secret"
                }
            }),
            status: "active".to_string(),
            created_at: now_ts(),
            updated_at: now_ts(),
        })
        .expect("upsert qqbot account");

    let (status, payload) = send_json(
        &context.app,
        Method::POST,
        "/wunder/channel/qqbot/webhook",
        json!({
            "op": 13,
            "app_id": 1234567890,
            "d": {
                "plain_token": "plain-token",
                "event_ts": 1700000000
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["plain_token"], json!("plain-token"));
    assert!(payload["signature"]
        .as_str()
        .map(|value| value.len() == 128)
        .unwrap_or(false));

    let logs =
        context
            .state
            .control
            .channels
            .list_runtime_logs(Some("qqbot"), Some(account_id), 20);
    assert!(logs.iter().any(|item| item.event == "validation_succeeded"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn qqbot_webhook_records_runtime_log_when_account_resolution_fails() {
    let context = build_test_context().await;
    let (status, _payload) = send_json(
        &context.app,
        Method::POST,
        "/wunder/channel/qqbot/webhook",
        json!({
            "op": 13,
            "app_id": "1234567890",
            "d": {
                "plain_token": "plain-token",
                "event_ts": "1700000000"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let logs = context
        .state
        .control
        .channels
        .list_runtime_logs(Some("qqbot"), None, 20);
    let log = logs
        .iter()
        .find(|item| item.event == "account_resolve_failed")
        .expect("account resolve failed log should exist");
    assert!(log.message.contains("app_id_hint=1234567890"));
    let callback_log = logs
        .iter()
        .find(|item| item.event == "callback_received")
        .expect("callback received log should exist");
    assert!(callback_log.message.contains("app_id_hint=1234567890"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn qqbot_webhook_resolves_account_by_header_app_id() {
    let context = build_test_context().await;
    let account_id = "uacc_qq_runtime_header";
    context
        .state
        .storage
        .upsert_channel_account(&ChannelAccountRecord {
            channel: "qqbot".to_string(),
            account_id: account_id.to_string(),
            config: json!({
                "qqbot": {
                    "app_id": "1234567890",
                    "client_secret": "test-secret"
                }
            }),
            status: "active".to_string(),
            created_at: now_ts(),
            updated_at: now_ts(),
        })
        .expect("upsert qqbot account");

    let (status, payload) = send_json_with_headers(
        &context.app,
        Method::POST,
        "/wunder/channel/qqbot/webhook",
        json!({
            "op": 13,
            "d": {
                "plain_token": "plain-token",
                "event_ts": 1700000001
            }
        }),
        &[("x-bot-appid", "1234567890")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["plain_token"], json!("plain-token"));
    assert!(payload["signature"]
        .as_str()
        .map(|value| value.len() == 128)
        .unwrap_or(false));

    let logs = context
        .state
        .control
        .channels
        .list_runtime_logs(Some("qqbot"), None, 30);
    assert!(logs.iter().any(|item| {
        item.event == "callback_received" && item.message.contains("app_id_hint=1234567890")
    }));
    assert!(logs.iter().any(|item| item.event == "validation_succeeded"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn qqbot_dispatch_event_is_not_rejected_by_missing_channel_token() {
    let context = build_test_context().await;
    let account_id = "uacc_qq_runtime_dispatch";
    context
        .state
        .storage
        .upsert_channel_account(&ChannelAccountRecord {
            channel: "qqbot".to_string(),
            account_id: account_id.to_string(),
            config: json!({
                "owner_user_id": "test-owner",
                "inbound_token": "test-inbound-token",
                "qqbot": {
                    "app_id": "1234567890",
                    "client_secret": "test-secret"
                }
            }),
            status: "active".to_string(),
            created_at: now_ts(),
            updated_at: now_ts(),
        })
        .expect("upsert qqbot account");

    let (status, payload) = send_json(
        &context.app,
        Method::POST,
        "/wunder/channel/qqbot/webhook",
        json!({
            "op": 0,
            "t": "C2C_MESSAGE_CREATE",
            "app_id": "1234567890",
            "d": {
                "id": "msg_dispatch_1",
                "content": "hello",
                "timestamp": "2026-03-17T10:00:00Z",
                "author": {
                    "user_openid": "openid_1"
                }
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["op"], json!(12));

    // inbound worker runs asynchronously; wait a short time for runtime logs.
    sleep(Duration::from_millis(250)).await;

    let logs = context
        .state
        .control
        .channels
        .list_runtime_logs(Some("qqbot"), None, 50);
    assert!(logs.iter().any(|item| item.event == "callback_received"));
    assert!(!logs.iter().any(|item| {
        item.event == "inbound_worker_rejected" && item.message.contains("invalid channel token")
    }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn qqbot_webhook_token_mode_resolves_account_and_validates_signature() {
    let context = build_test_context().await;
    let account_id = "uacc_qq_runtime_token_mode";
    context
        .state
        .storage
        .upsert_channel_account(&ChannelAccountRecord {
            channel: "qqbot".to_string(),
            account_id: account_id.to_string(),
            config: json!({
                "qqbot": {
                    "token": "1234567890:test-secret"
                }
            }),
            status: "active".to_string(),
            created_at: now_ts(),
            updated_at: now_ts(),
        })
        .expect("upsert qqbot account");

    let (status, payload) = send_json(
        &context.app,
        Method::POST,
        "/wunder/channel/qqbot/webhook",
        json!({
            "op": 13,
            "app_id": "1234567890",
            "d": {
                "plain_token": "plain-token",
                "event_ts": "1700000000"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["plain_token"], json!("plain-token"));
    assert!(payload["signature"]
        .as_str()
        .map(|value| value.len() == 128)
        .unwrap_or(false));

    let logs =
        context
            .state
            .control
            .channels
            .list_runtime_logs(Some("qqbot"), Some(account_id), 30);
    assert!(logs.iter().any(|item| item.event == "validation_succeeded"));
}
