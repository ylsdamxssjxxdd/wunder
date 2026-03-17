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
        ChannelAccountRecord, ChannelBindingRecord, ChannelMessageRecord, ChannelOutboxRecord,
        ChannelSessionRecord, ChannelUserBindingRecord, ListChannelUserBindingsQuery,
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

async fn send_json(
    app: &Router,
    method: Method,
    path: &str,
    payload: Option<Value>,
) -> (StatusCode, Value) {
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
    context
        .state
        .storage
        .enqueue_channel_outbox(&ChannelOutboxRecord {
            outbox_id: "outbox_sent_stats".to_string(),
            channel: "feishu".to_string(),
            account_id: "acc_stats".to_string(),
            peer_kind: "group".to_string(),
            peer_id: "room_a".to_string(),
            thread_id: Some("thread-1".to_string()),
            payload: json!({"text":"sent"}),
            status: "sent".to_string(),
            retry_count: 0,
            retry_at: now,
            last_error: None,
            created_at: now,
            updated_at: now,
            delivered_at: Some(now + 3.0),
        })
        .expect("insert outbox sent");
    context
        .state
        .storage
        .enqueue_channel_outbox(&ChannelOutboxRecord {
            outbox_id: "outbox_failed_stats".to_string(),
            channel: "feishu".to_string(),
            account_id: "acc_stats".to_string(),
            peer_kind: "group".to_string(),
            peer_id: "room_a".to_string(),
            thread_id: Some("thread-1".to_string()),
            payload: json!({"text":"failed"}),
            status: "failed".to_string(),
            retry_count: 2,
            retry_at: now,
            last_error: Some("network timeout".to_string()),
            created_at: now,
            updated_at: now + 4.0,
            delivered_at: None,
        })
        .expect("insert outbox failed");

    let (status, payload) = send_json(
        &context.app,
        Method::GET,
        "/wunder/admin/channels/accounts",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let items = payload["data"]["items"].as_array().expect("items array");
    let item = items
        .iter()
        .find(|record| {
            record["channel"] == json!("feishu") && record["account_id"] == json!("acc_stats")
        })
        .expect("target account exists");
    assert_eq!(item["owner_user_id"], json!(owner.user_id));
    assert_eq!(item["owner_username"], json!(owner.username));
    assert_eq!(item["owner_count"], json!(1));
    assert_eq!(item["binding_count"], json!(2));
    assert_eq!(item["session_count"], json!(1));
    assert_eq!(item["message_count"], json!(2));
    assert_eq!(item["inbound_message_count"], json!(2));
    assert_eq!(item["outbound_total_count"], json!(2));
    assert_eq!(item["outbound_sent_count"], json!(1));
    assert_eq!(item["outbound_failed_count"], json!(1));
    assert_eq!(item["outbound_retry_count"], json!(0));
    assert_eq!(item["communication_count"], json!(4));
    assert_eq!(item["has_issue"], json!(true));
    assert!(item["last_communication_at"].as_f64().unwrap_or(0.0) > 0.0);

    let (status, payload) = send_json(
        &context.app,
        Method::GET,
        "/wunder/admin/channels/accounts?issue_only=true",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let filtered_items = payload["data"]["items"].as_array().expect("filtered items");
    assert_eq!(filtered_items.len(), 1);
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
    context
        .state
        .storage
        .upsert_channel_session(&ChannelSessionRecord {
            channel: "feishu".to_string(),
            account_id: "acc_delete".to_string(),
            peer_kind: "user".to_string(),
            peer_id: "peer-1".to_string(),
            thread_id: Some("thread-delete".to_string()),
            session_id: "session-delete".to_string(),
            agent_id: None,
            user_id: owner.user_id.clone(),
            tts_enabled: None,
            tts_voice: None,
            metadata: None,
            last_message_at: now,
            created_at: now,
            updated_at: now,
        })
        .expect("upsert session");
    context
        .state
        .storage
        .insert_channel_message(&ChannelMessageRecord {
            channel: "feishu".to_string(),
            account_id: "acc_delete".to_string(),
            peer_kind: "user".to_string(),
            peer_id: "peer-1".to_string(),
            thread_id: Some("thread-delete".to_string()),
            session_id: "session-delete".to_string(),
            message_id: Some("msg-delete".to_string()),
            sender_id: Some("sender-delete".to_string()),
            message_type: "text".to_string(),
            payload: json!({"text":"delete me"}),
            raw_payload: None,
            created_at: now,
        })
        .expect("insert delete message");
    context
        .state
        .storage
        .enqueue_channel_outbox(&ChannelOutboxRecord {
            outbox_id: "outbox_delete".to_string(),
            channel: "feishu".to_string(),
            account_id: "acc_delete".to_string(),
            peer_kind: "user".to_string(),
            peer_id: "peer-1".to_string(),
            thread_id: Some("thread-delete".to_string()),
            payload: json!({"text":"outbound delete"}),
            status: "pending".to_string(),
            retry_count: 0,
            retry_at: now,
            last_error: None,
            created_at: now,
            updated_at: now,
            delivered_at: None,
        })
        .expect("insert delete outbox");

    let (impact_status, impact_payload) = send_json(
        &context.app,
        Method::GET,
        "/wunder/admin/channels/accounts/feishu/acc_delete/impact",
        None,
    )
    .await;
    assert_eq!(impact_status, StatusCode::OK);
    assert_eq!(impact_payload["data"]["bindings"], json!(1));
    assert_eq!(impact_payload["data"]["user_bindings"], json!(1));
    assert_eq!(impact_payload["data"]["sessions"], json!(1));
    assert_eq!(impact_payload["data"]["messages"], json!(1));
    assert_eq!(impact_payload["data"]["outbox_total"], json!(1));

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
    assert_eq!(payload["data"]["deleted_sessions"], json!(1));
    assert_eq!(payload["data"]["deleted_messages"], json!(1));
    assert_eq!(payload["data"]["deleted_outbox"], json!(1));

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

    let (_, total_sessions) = context
        .state
        .storage
        .list_channel_sessions(Some("feishu"), Some("acc_delete"), None, None, 0, 10)
        .expect("list sessions");
    assert_eq!(total_sessions, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_channel_accounts_batch_disable_enable_updates_status() {
    let context = build_test_context("admin-channel-account-batch-disable-enable").await;
    let now = now_ts();
    context
        .state
        .storage
        .upsert_channel_account(&ChannelAccountRecord {
            channel: "feishu".to_string(),
            account_id: "acc_batch_enable_a".to_string(),
            config: json!({}),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("upsert account a");
    context
        .state
        .storage
        .upsert_channel_account(&ChannelAccountRecord {
            channel: "feishu".to_string(),
            account_id: "acc_batch_enable_b".to_string(),
            config: json!({}),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("upsert account b");

    let (disable_status, disable_payload) = send_json(
        &context.app,
        Method::POST,
        "/wunder/admin/channels/accounts/batch",
        Some(json!({
            "action": "disable",
            "items": [
                { "channel": "feishu", "account_id": "acc_batch_enable_a" },
                { "channel": "feishu", "account_id": "acc_batch_enable_b" }
            ]
        })),
    )
    .await;
    assert_eq!(disable_status, StatusCode::OK);
    assert_eq!(disable_payload["data"]["action"], json!("disable"));
    assert_eq!(disable_payload["data"]["total"], json!(2));
    assert_eq!(disable_payload["data"]["success"], json!(2));
    assert_eq!(disable_payload["data"]["failed"], json!(0));
    assert_eq!(disable_payload["data"]["skipped"], json!(0));

    let account_a = context
        .state
        .storage
        .get_channel_account("feishu", "acc_batch_enable_a")
        .expect("load account a")
        .expect("account a exists");
    let account_b = context
        .state
        .storage
        .get_channel_account("feishu", "acc_batch_enable_b")
        .expect("load account b")
        .expect("account b exists");
    assert_eq!(account_a.status, "disabled");
    assert_eq!(account_b.status, "disabled");

    let (enable_status, enable_payload) = send_json(
        &context.app,
        Method::POST,
        "/wunder/admin/channels/accounts/batch",
        Some(json!({
            "action": "enable",
            "items": [
                { "channel": "feishu", "account_id": "acc_batch_enable_a" },
                { "channel": "feishu", "account_id": "missing_account" }
            ]
        })),
    )
    .await;
    assert_eq!(enable_status, StatusCode::OK);
    assert_eq!(enable_payload["data"]["action"], json!("enable"));
    assert_eq!(enable_payload["data"]["total"], json!(2));
    assert_eq!(enable_payload["data"]["success"], json!(1));
    assert_eq!(enable_payload["data"]["failed"], json!(0));
    assert_eq!(enable_payload["data"]["skipped"], json!(1));

    let account_a_after_enable = context
        .state
        .storage
        .get_channel_account("feishu", "acc_batch_enable_a")
        .expect("load account a after enable")
        .expect("account a exists after enable");
    assert_eq!(account_a_after_enable.status, "active");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_channel_accounts_batch_delete_removes_records() {
    let context = build_test_context("admin-channel-account-batch-delete").await;
    let now = now_ts();
    let owner = context
        .state
        .user_store
        .create_user(
            "channel_owner_batch_delete",
            Some("channel_owner_batch_delete@example.test".to_string()),
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
            account_id: "acc_batch_delete_a".to_string(),
            config: json!({}),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("upsert account a");
    context
        .state
        .storage
        .upsert_channel_account(&ChannelAccountRecord {
            channel: "feishu".to_string(),
            account_id: "acc_batch_delete_b".to_string(),
            config: json!({}),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("upsert account b");
    context
        .state
        .storage
        .upsert_channel_binding(&ChannelBindingRecord {
            binding_id: "bind_batch_delete".to_string(),
            channel: "feishu".to_string(),
            account_id: "acc_batch_delete_a".to_string(),
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
            account_id: "acc_batch_delete_a".to_string(),
            peer_kind: "user".to_string(),
            peer_id: "peer-batch-delete".to_string(),
            user_id: owner.user_id.clone(),
            created_at: now,
            updated_at: now,
        })
        .expect("upsert user binding");
    context
        .state
        .storage
        .upsert_channel_session(&ChannelSessionRecord {
            channel: "feishu".to_string(),
            account_id: "acc_batch_delete_a".to_string(),
            peer_kind: "user".to_string(),
            peer_id: "peer-batch-delete".to_string(),
            thread_id: Some("thread-batch-delete".to_string()),
            session_id: "session-batch-delete".to_string(),
            agent_id: None,
            user_id: owner.user_id.clone(),
            tts_enabled: None,
            tts_voice: None,
            metadata: None,
            last_message_at: now,
            created_at: now,
            updated_at: now,
        })
        .expect("upsert session");
    context
        .state
        .storage
        .insert_channel_message(&ChannelMessageRecord {
            channel: "feishu".to_string(),
            account_id: "acc_batch_delete_a".to_string(),
            peer_kind: "user".to_string(),
            peer_id: "peer-batch-delete".to_string(),
            thread_id: Some("thread-batch-delete".to_string()),
            session_id: "session-batch-delete".to_string(),
            message_id: Some("msg-batch-delete".to_string()),
            sender_id: Some("sender-batch-delete".to_string()),
            message_type: "text".to_string(),
            payload: json!({"text":"batch delete me"}),
            raw_payload: None,
            created_at: now,
        })
        .expect("insert message");
    context
        .state
        .storage
        .enqueue_channel_outbox(&ChannelOutboxRecord {
            outbox_id: "outbox_batch_delete".to_string(),
            channel: "feishu".to_string(),
            account_id: "acc_batch_delete_a".to_string(),
            peer_kind: "user".to_string(),
            peer_id: "peer-batch-delete".to_string(),
            thread_id: Some("thread-batch-delete".to_string()),
            payload: json!({"text":"batch outbound"}),
            status: "pending".to_string(),
            retry_count: 0,
            retry_at: now,
            last_error: None,
            created_at: now,
            updated_at: now,
            delivered_at: None,
        })
        .expect("insert outbox");

    let (status, payload) = send_json(
        &context.app,
        Method::POST,
        "/wunder/admin/channels/accounts/batch",
        Some(json!({
            "action": "delete",
            "items": [
                { "channel": "feishu", "account_id": "acc_batch_delete_a" },
                { "channel": "feishu", "account_id": "acc_batch_delete_b" },
                { "channel": "feishu", "account_id": "acc_batch_delete_missing" }
            ]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["data"]["action"], json!("delete"));
    assert_eq!(payload["data"]["total"], json!(3));
    assert_eq!(payload["data"]["success"], json!(2));
    assert_eq!(payload["data"]["failed"], json!(0));
    assert_eq!(payload["data"]["skipped"], json!(1));
    assert_eq!(payload["data"]["deleted_accounts"], json!(2));
    assert_eq!(payload["data"]["deleted_bindings"], json!(1));
    assert_eq!(payload["data"]["deleted_user_bindings"], json!(1));
    assert_eq!(payload["data"]["deleted_sessions"], json!(1));
    assert_eq!(payload["data"]["deleted_messages"], json!(1));
    assert_eq!(payload["data"]["deleted_outbox"], json!(1));

    let account_a = context
        .state
        .storage
        .get_channel_account("feishu", "acc_batch_delete_a")
        .expect("load account a after delete");
    let account_b = context
        .state
        .storage
        .get_channel_account("feishu", "acc_batch_delete_b")
        .expect("load account b after delete");
    assert!(account_a.is_none());
    assert!(account_b.is_none());

    let (_, total_user_bindings) = context
        .state
        .storage
        .list_channel_user_bindings(ListChannelUserBindingsQuery {
            channel: Some("feishu"),
            account_id: Some("acc_batch_delete_a"),
            peer_kind: None,
            peer_id: None,
            user_id: None,
            offset: 0,
            limit: 50,
        })
        .expect("list user bindings");
    assert_eq!(total_user_bindings, 0);

    let (_, total_sessions) = context
        .state
        .storage
        .list_channel_sessions(
            Some("feishu"),
            Some("acc_batch_delete_a"),
            None,
            None,
            0,
            10,
        )
        .expect("list sessions");
    assert_eq!(total_sessions, 0);
}
