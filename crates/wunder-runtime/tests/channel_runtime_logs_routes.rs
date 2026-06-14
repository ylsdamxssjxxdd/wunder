use axum::{
    body::{to_bytes, Body},
    http::{header::AUTHORIZATION, Method, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;
use uuid::Uuid;
use wunder_server::{
    build_desktop_router,
    config::Config,
    config_store::ConfigStore,
    state::{AppState, AppStateInitOptions},
    storage::{ChannelAccountRecord, ChannelBindingRecord, ChannelUserBindingRecord},
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
    config.channels.enabled = true;
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = temp_dir
        .path()
        .join("channel-runtime-logs-routes.db")
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
    payload: Option<Value>,
) -> (StatusCode, Value) {
    let bearer = format!("Bearer {token}");
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(AUTHORIZATION, bearer);
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

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

fn make_user_binding_id(
    user_id: &str,
    channel: &str,
    account_id: &str,
    peer_kind: &str,
    peer_id: &str,
) -> String {
    let key = format!(
        "ubind:{user_id}|{channel}|{account_id}|{peer_kind}|{peer_id}",
        user_id = user_id.trim().to_ascii_lowercase(),
        channel = channel.trim().to_ascii_lowercase(),
        account_id = account_id.trim().to_ascii_lowercase(),
        peer_kind = peer_kind.trim().to_ascii_lowercase(),
        peer_id = peer_id.trim().to_ascii_lowercase(),
    );
    format!(
        "ubind_{}",
        Uuid::new_v5(&Uuid::NAMESPACE_OID, key.as_bytes()).simple()
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_channel_runtime_logs_filters_by_agent_and_collapses_repeats() {
    let context = build_test_context("runtime_log_user").await;
    let now = now_ts();

    let account_a = "uacc_agent_a";
    let account_b = "uacc_agent_b";
    context
        .state
        .storage
        .upsert_channel_account(&ChannelAccountRecord {
            channel: "feishu".to_string(),
            account_id: account_a.to_string(),
            config: json!({
                "display_name": "Feishu A",
            }),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("upsert account a");
    context
        .state
        .storage
        .upsert_channel_account(&ChannelAccountRecord {
            channel: "xmpp".to_string(),
            account_id: account_b.to_string(),
            config: json!({
                "display_name": "XMPP B",
            }),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("upsert account b");

    let user_binding_a = ChannelUserBindingRecord {
        channel: "feishu".to_string(),
        account_id: account_a.to_string(),
        peer_kind: "group".to_string(),
        peer_id: "*".to_string(),
        user_id: context.user_id.clone(),
        created_at: now,
        updated_at: now,
    };
    let user_binding_b = ChannelUserBindingRecord {
        channel: "xmpp".to_string(),
        account_id: account_b.to_string(),
        peer_kind: "user".to_string(),
        peer_id: "*".to_string(),
        user_id: context.user_id.clone(),
        created_at: now,
        updated_at: now,
    };
    context
        .state
        .storage
        .upsert_channel_user_binding(&user_binding_a)
        .expect("upsert user binding a");
    context
        .state
        .storage
        .upsert_channel_user_binding(&user_binding_b)
        .expect("upsert user binding b");

    context
        .state
        .storage
        .upsert_channel_binding(&ChannelBindingRecord {
            binding_id: make_user_binding_id(&context.user_id, "feishu", account_a, "group", "*"),
            channel: "feishu".to_string(),
            account_id: account_a.to_string(),
            peer_kind: Some("group".to_string()),
            peer_id: Some("*".to_string()),
            agent_id: Some("agent_a".to_string()),
            tool_overrides: Vec::new(),
            priority: 100,
            enabled: true,
            created_at: now,
            updated_at: now,
        })
        .expect("upsert binding a");
    context
        .state
        .storage
        .upsert_channel_binding(&ChannelBindingRecord {
            binding_id: make_user_binding_id(&context.user_id, "xmpp", account_b, "user", "*"),
            channel: "xmpp".to_string(),
            account_id: account_b.to_string(),
            peer_kind: Some("user".to_string()),
            peer_id: Some("*".to_string()),
            agent_id: Some("agent_b".to_string()),
            tool_overrides: Vec::new(),
            priority: 100,
            enabled: true,
            created_at: now,
            updated_at: now,
        })
        .expect("upsert binding b");

    context.state.control.channels.record_runtime_warn(
        "feishu",
        Some(account_a),
        "long_connection_failed",
        "connection refused, retry_in=3s",
    );
    context.state.control.channels.record_runtime_warn(
        "feishu",
        Some(account_a),
        "long_connection_failed",
        "connection refused, retry_in=30s",
    );
    context.state.control.channels.record_runtime_warn(
        "xmpp",
        Some(account_b),
        "long_connection_failed",
        "xmpp disconnected: timeout",
    );

    let (status_agent, payload_agent) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        "/wunder/channels/runtime_logs?agent_id=agent_a&limit=20",
        None,
    )
    .await;
    assert_eq!(status_agent, StatusCode::OK);
    let items_agent = payload_agent["data"]["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        payload_agent["data"]["status"]["collector_alive"],
        json!(true)
    );
    assert_eq!(items_agent.len(), 1);
    assert_eq!(items_agent[0]["channel"], json!("feishu"));
    assert_eq!(items_agent[0]["account_id"], json!(account_a));
    assert_eq!(items_agent[0]["event"], json!("long_connection_failed"));
    assert_eq!(items_agent[0]["repeat_count"], json!(2));

    let (status_all, payload_all) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        "/wunder/channels/runtime_logs?limit=20",
        None,
    )
    .await;
    assert_eq!(status_all, StatusCode::OK);
    let items_all = payload_all["data"]["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        payload_all["data"]["status"]["collector_alive"],
        json!(true)
    );
    assert_eq!(items_all.len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_channel_runtime_logs_falls_back_to_account_agent_id_when_binding_missing() {
    let context = build_test_context("runtime_log_agent_fallback").await;
    let now = now_ts();
    let account_id = "uacc_agent_cfg";
    context
        .state
        .storage
        .upsert_channel_account(&ChannelAccountRecord {
            channel: "qqbot".to_string(),
            account_id: account_id.to_string(),
            config: json!({
                "agent_id": "agent_cfg",
                "qqbot": {
                    "app_id": "1234567890"
                }
            }),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("upsert account");
    context
        .state
        .storage
        .upsert_channel_user_binding(&ChannelUserBindingRecord {
            channel: "qqbot".to_string(),
            account_id: account_id.to_string(),
            peer_kind: "group".to_string(),
            peer_id: "*".to_string(),
            user_id: context.user_id.clone(),
            created_at: now,
            updated_at: now,
        })
        .expect("upsert user binding");

    context.state.control.channels.record_runtime_warn(
        "qqbot",
        Some(account_id),
        "inbound_enqueue_failed",
        "qqbot inbound enqueue failed: timeout",
    );

    let (status, payload) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        "/wunder/channels/runtime_logs?agent_id=agent_cfg&limit=20",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = payload["data"]["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(payload["data"]["status"]["collector_alive"], json!(true));
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["channel"], json!("qqbot"));
    assert_eq!(items[0]["account_id"], json!(account_id));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_logs_probe_endpoint_writes_runtime_probe_log() {
    let context = build_test_context("runtime_probe_user").await;
    let now = now_ts();
    let account_id = "uacc_probe_1";
    context
        .state
        .storage
        .upsert_channel_account(&ChannelAccountRecord {
            channel: "qqbot".to_string(),
            account_id: account_id.to_string(),
            config: json!({
                "owner_user_id": context.user_id.clone(),
                "qqbot": {
                    "app_id": "1234567890"
                }
            }),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("upsert probe account");

    let (status_probe, payload_probe) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/channels/runtime_logs/probe",
        Some(json!({
            "channel": "qqbot",
            "account_id": account_id,
            "message": "probe-from-test"
        })),
    )
    .await;
    assert_eq!(status_probe, StatusCode::OK);
    assert_eq!(payload_probe["data"]["event"], json!("runtime_probe"));
    assert_eq!(payload_probe["data"]["account_id"], json!(account_id));

    let (status_logs, payload_logs) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        "/wunder/channels/runtime_logs?channel=qqbot&account_id=uacc_probe_1&limit=20",
        None,
    )
    .await;
    assert_eq!(status_logs, StatusCode::OK);
    let items = payload_logs["data"]["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(items
        .iter()
        .any(|item| item["event"] == json!("runtime_probe")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_logs_owner_fallback_works_without_user_binding() {
    let context = build_test_context("runtime_owner_fallback").await;
    let now = now_ts();
    let account_id = "uacc_owner_only";
    context
        .state
        .storage
        .upsert_channel_account(&ChannelAccountRecord {
            channel: "qqbot".to_string(),
            account_id: account_id.to_string(),
            config: json!({
                "owner_user_id": context.user_id.clone(),
                "qqbot": {
                    "app_id": "1234567890"
                }
            }),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("upsert owner fallback account");
    context.state.control.channels.record_runtime_warn(
        "qqbot",
        Some(account_id),
        "inbound_enqueue_failed",
        "owner-only account log",
    );

    let (status, payload) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        "/wunder/channels/runtime_logs?channel=qqbot&limit=20",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = payload["data"]["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["account_id"], json!(account_id));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upsert_qqbot_account_writes_runtime_init_logs() {
    let context = build_test_context("runtime_upsert_log_user").await;
    let (status_upsert, payload_upsert) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/channels/accounts",
        Some(json!({
            "channel": "qqbot",
            "create_new": true,
            "enabled": true,
            "config": {
                "qqbot": {
                    "app_id": "1234567890",
                    "client_secret": "test-secret",
                    "markdown_support": true
                }
            }
        })),
    )
    .await;
    assert_eq!(status_upsert, StatusCode::OK);
    let account_id = payload_upsert["data"]["account_id"]
        .as_str()
        .expect("account_id should exist")
        .to_string();

    let (status_logs, payload_logs) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        &format!("/wunder/channels/runtime_logs?channel=qqbot&account_id={account_id}&limit=20"),
        None,
    )
    .await;
    assert_eq!(status_logs, StatusCode::OK);
    let items = payload_logs["data"]["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(items
        .iter()
        .any(|item| item["event"] == json!("account_upserted")));
    assert!(items
        .iter()
        .any(|item| item["event"] == json!("qqbot_config_ready")));
}
