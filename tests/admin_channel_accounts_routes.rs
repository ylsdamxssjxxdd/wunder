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
    storage::{
        ChannelAccountRecord, ChannelBindingRecord, ChannelMessageRecord, ChannelSessionRecord,
        ChannelUserBindingRecord, ListChannelUserBindingsQuery,
    },
};

struct TestContext {
    app: Router,
    state: Arc<AppState>,
    _temp_dir: TempDir,
}

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

async fn build_test_context(db_name: &str) -> TestContext {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = Config::default();
    config.channels.enabled = true;
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = temp_dir
        .path()
        .join(format!("{db_name}.db"))
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
    let app = build_router(state.clone());
    TestContext {
        app,
        state,
        _temp_dir: temp_dir,
    }
}

async fn send_json(app: &Router, method: Method, path: &str, payload: Option<Value>) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(path);
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_channel_accounts_exposes_owner_and_communication_stats() {
    let context = build_test_context("admin-channel-accounts-stats").await;
    let now = now_ts();
    let owner = context
        .state
        .user_store
        .create_user(
            "channel_owner",
            Some("channel_owner@example.test".to_string()),
            "password-123",
            Some("A"),
            None,
            vec!["user".to_string()],
            "active",
            false,
        )
        .expect("create owner");

    context
        .state
        .storage
        .upsert_channel_account(&ChannelAccountRecord {
            channel: "feishu".to_string(),
            account_id: "acc_stats".to_string(),
            config: json!({}),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("upsert account");
    context
        .state
        .storage
        .upsert_channel_binding(&ChannelBindingRecord {
            binding_id: "bind_stats".to_string(),
            channel: "feishu".to_string(),
            account_id: "acc_stats".to_string(),
            peer_kind: Some("group".to_string()),
            peer_id: Some("*".to_string()),
            agent_id: None,
            tool_overrides: Vec::new(),
            priority: 100,
            enabled: true,
            created_at: now,
            updated_at: now,
        })
        .expect("upsert channel binding");
    context
        .state
        .storage
        .upsert_channel_user_binding(&ChannelUserBindingRecord {
            channel: "feishu".to_string(),
            account_id: "acc_stats".to_string(),
            peer_kind: "group".to_string(),
            peer_id: "room_a".to_string(),
            user_id: owner.user_id.clone(),
            created_at: now,
            updated_at: now,
        })
        .expect("upsert user binding a");
    context
        .state
        .storage
        .upsert_channel_user_binding(&ChannelUserBindingRecord {
            channel: "feishu".to_string(),
            account_id: "acc_stats".to_string(),
            peer_kind: "group".to_string(),
            peer_id: "room_b".to_string(),
            user_id: owner.user_id.clone(),
            created_at: now,
            updated_at: now + 1.0,
        })
        .expect("upsert user binding b");
    context
        .state
        .storage
        .upsert_channel_session(&ChannelSessionRecord {
            channel: "feishu".to_string(),
            account_id: "acc_stats".to_string(),
            peer_kind: "group".to_string(),
            peer_id: "room_a".to_string(),
            thread_id: Some("thread-1".to_string()),
            session_id: "session-1".to_string(),
            agent_id: None,
            user_id: owner.user_id.clone(),
            tts_enabled: None,
            tts_voice: None,
            metadata: None,
            last_message_at: now + 2.0,
            created_at: now,
            updated_at: now + 2.0,
        })
        .expect("upsert session");
    context
        .state
        .storage
        .insert_channel_message(&ChannelMessageRecord {
            channel: "feishu".to_string(),
            account_id: "acc_stats".to_string(),
            peer_kind: "group".to_string(),
            peer_id: "room_a".to_string(),
            thread_id: Some("thread-1".to_string()),
            session_id: "session-1".to_string(),
            message_id: Some("msg-1".to_string()),
            sender_id: Some("sender-a".to_string()),
            message_type: "text".to_string(),
            payload: json!({ "text": "hello" }),
            raw_payload: None,
            created_at: now + 2.0,
        })
        .expect("insert message 1");
    context
        .state
        .storage
        .insert_channel_message(&ChannelMessageRecord {
            channel: "feishu".to_string(),
            account_id: "acc_stats".to_string(),
            peer_kind: "group".to_string(),
            peer_id: "room_a".to_string(),
            thread_id: Some("thread-1".to_string()),
            session_id: "session-1".to_string(),
            message_id: Some("msg-2".to_string()),
            sender_id: Some("sender-b".to_string()),
            message_type: "text".to_string(),
            payload: json!({ "text": "world" }),
            raw_payload: None,
            created_at: now + 3.0,
        })
        .expect("insert message 2");

    let (status, payload) =
        send_json(&context.app, Method::GET, "/wunder/admin/channels/accounts", None).await;
    assert_eq!(status, StatusCode::OK);

    let items = payload["data"]["items"].as_array().expect("items array");
    let item = items
        .iter()
        .find(|record| record["channel"] == json!("feishu") && record["account_id"] == json!("acc_stats"))
        .expect("target account exists");
    assert_eq!(item["owner_user_id"], json!(owner.user_id));
    assert_eq!(item["owner_username"], json!(owner.username));
    assert_eq!(item["owner_count"], json!(1));
    assert_eq!(item["binding_count"], json!(2));
    assert_eq!(item["session_count"], json!(1));
    assert_eq!(item["message_count"], json!(2));
    assert_eq!(item["communication_count"], json!(2));
    assert!(item["last_communication_at"].as_f64().unwrap_or(0.0) > 0.0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_channel_account_delete_removes_account_and_bindings() {
    let context = build_test_context("admin-channel-account-delete").await;
    let now = now_ts();
    let owner = context
        .state
        .user_store
        .create_user(
            "channel_owner_delete",
            Some("channel_owner_delete@example.test".to_string()),
            "password-123",
            Some("A"),
            None,
            vec!["user".to_string()],
            "active",
            false,
        )
        .expect("create owner");

    context
        .state
        .storage
        .upsert_channel_account(&ChannelAccountRecord {
            channel: "feishu".to_string(),
            account_id: "acc_delete".to_string(),
            config: json!({}),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("upsert account");
    context
        .state
        .storage
        .upsert_channel_binding(&ChannelBindingRecord {
            binding_id: "bind_delete".to_string(),
            channel: "feishu".to_string(),
            account_id: "acc_delete".to_string(),
            peer_kind: Some("user".to_string()),
            peer_id: Some("*".to_string()),
            agent_id: None,
            tool_overrides: Vec::new(),
            priority: 100,
            enabled: true,
            created_at: now,
            updated_at: now,
        })
        .expect("upsert binding");
    context
        .state
        .storage
        .upsert_channel_user_binding(&ChannelUserBindingRecord {
            channel: "feishu".to_string(),
            account_id: "acc_delete".to_string(),
            peer_kind: "user".to_string(),
            peer_id: "peer-1".to_string(),
            user_id: owner.user_id.clone(),
            created_at: now,
            updated_at: now,
        })
        .expect("upsert user binding");

    let (status, payload) = send_json(
        &context.app,
        Method::DELETE,
        "/wunder/admin/channels/accounts/feishu/acc_delete",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["data"]["deleted_accounts"], json!(1));
    assert_eq!(payload["data"]["deleted_bindings"], json!(1));
    assert_eq!(payload["data"]["deleted_user_bindings"], json!(1));

    let account = context
        .state
        .storage
        .get_channel_account("feishu", "acc_delete")
        .expect("query account after delete");
    assert!(account.is_none());

    let bindings = context
        .state
        .storage
        .list_channel_bindings(Some("feishu"))
        .expect("list bindings");
    assert!(!bindings
        .iter()
        .any(|record| record.account_id == "acc_delete"));

    let (_, total_user_bindings) = context
        .state
        .storage
        .list_channel_user_bindings(ListChannelUserBindingsQuery {
            channel: Some("feishu"),
            account_id: Some("acc_delete"),
            peer_kind: None,
            peer_id: None,
            user_id: None,
            offset: 0,
            limit: 50,
        })
        .expect("list user bindings");
    assert_eq!(total_user_bindings, 0);
}
