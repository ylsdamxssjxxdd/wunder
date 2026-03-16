use axum::{
    body::{to_bytes, Body},
    http::{Method, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tempfile::TempDir;
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

    let config_store = ConfigStore::new(temp_dir.path().join("wunder.override.yaml"));
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
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
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

    let logs = context
        .state
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
        .channels
        .list_runtime_logs(Some("qqbot"), None, 20);
    let log = logs
        .iter()
        .find(|item| item.event == "account_resolve_failed")
        .expect("account resolve failed log should exist");
    assert!(log.message.contains("app_id_hint=1234567890"));
}
