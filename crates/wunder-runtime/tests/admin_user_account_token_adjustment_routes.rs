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
    user_store::UserStore,
};

struct TestContext {
    app: Router,
    state: Arc<AppState>,
    _temp_dir: TempDir,
}

async fn build_test_context(db_name: &str) -> TestContext {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = Config::default();
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
        .expect("read response body");
    let payload = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("parse response json")
    };
    (status, payload)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_user_account_token_adjustment_grants_and_deducts_tokens() {
    let context = build_test_context("admin-user-account-token-adjustment").await;
    let created = context
        .state
        .user_store
        .create_user(
            "token_adjust_target",
            Some("token_adjust_target@example.test".to_string()),
            "password-123",
            Some("A"),
            None,
            vec!["user".to_string()],
            "active",
            false,
        )
        .expect("create user");
    let today = UserStore::today_string();
    let mut record = context
        .state
        .user_store
        .get_user_by_id(&created.user_id)
        .expect("load user")
        .expect("user exists");
    record.token_balance = 10;
    record.token_granted_total = 20;
    record.token_used_total = 3;
    record.last_token_grant_date = Some(today);
    context
        .state
        .user_store
        .update_user(&record)
        .expect("seed token account");

    let path = format!(
        "/wunder/admin/user_accounts/{}/token_adjustment",
        created.user_id
    );
    let (grant_status, grant_payload) = send_json(
        &context.app,
        Method::POST,
        &path,
        json!({
            "action": "grant",
            "amount": 25
        }),
    )
    .await;
    assert_eq!(grant_status, StatusCode::OK);
    assert_eq!(grant_payload["adjustment"]["action"], json!("grant"));
    assert_eq!(grant_payload["adjustment"]["amount"], json!(25));

    let after_grant = context
        .state
        .user_store
        .get_user_by_id(&created.user_id)
        .expect("reload after grant")
        .expect("user exists");
    assert_eq!(after_grant.token_balance, 35);
    assert_eq!(after_grant.token_granted_total, 45);
    assert_eq!(after_grant.token_used_total, 3);

    let (deduct_status, deduct_payload) = send_json(
        &context.app,
        Method::POST,
        &path,
        json!({
            "action": "deduct",
            "amount": 5
        }),
    )
    .await;
    assert_eq!(deduct_status, StatusCode::OK);
    assert_eq!(deduct_payload["adjustment"]["action"], json!("deduct"));
    assert_eq!(deduct_payload["adjustment"]["amount"], json!(5));

    let after_deduct = context
        .state
        .user_store
        .get_user_by_id(&created.user_id)
        .expect("reload after deduct")
        .expect("user exists");
    assert_eq!(after_deduct.token_balance, 30);
    assert_eq!(after_deduct.token_granted_total, 45);
    assert_eq!(after_deduct.token_used_total, 8);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_user_account_token_adjustment_rejects_overdraft_deduction() {
    let context = build_test_context("admin-user-account-token-adjustment-overdraft").await;
    let created = context
        .state
        .user_store
        .create_user(
            "token_adjust_overdraft",
            Some("token_adjust_overdraft@example.test".to_string()),
            "password-123",
            Some("A"),
            None,
            vec!["user".to_string()],
            "active",
            false,
        )
        .expect("create user");
    let today = UserStore::today_string();
    let mut record = context
        .state
        .user_store
        .get_user_by_id(&created.user_id)
        .expect("load user")
        .expect("user exists");
    record.token_balance = 7;
    record.token_granted_total = 9;
    record.token_used_total = 1;
    record.last_token_grant_date = Some(today);
    context
        .state
        .user_store
        .update_user(&record)
        .expect("seed token account");

    let path = format!(
        "/wunder/admin/user_accounts/{}/token_adjustment",
        created.user_id
    );
    let (status, payload) = send_json(
        &context.app,
        Method::POST,
        &path,
        json!({
            "action": "deduct",
            "amount": 10
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(payload.get("error").is_some());

    let after = context
        .state
        .user_store
        .get_user_by_id(&created.user_id)
        .expect("reload user")
        .expect("user exists");
    assert_eq!(after.token_balance, 7);
    assert_eq!(after.token_granted_total, 9);
    assert_eq!(after.token_used_total, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_user_account_token_adjustment_materializes_pending_daily_grant_before_deduction() {
    let context = build_test_context("admin-user-account-token-adjustment-pending-daily").await;
    let created = context
        .state
        .user_store
        .create_user(
            "token_adjust_pending_daily",
            Some("token_adjust_pending_daily@example.test".to_string()),
            "password-123",
            Some("A"),
            None,
            vec!["user".to_string()],
            "active",
            false,
        )
        .expect("create user");
    let units = context
        .state
        .user_store
        .list_org_units()
        .expect("list org units");
    let mut record = context
        .state
        .user_store
        .get_user_by_id(&created.user_id)
        .expect("load user")
        .expect("user exists");
    let daily_grant = UserStore::default_daily_token_grant_by_level(
        record
            .unit_id
            .as_ref()
            .and_then(|unit_id| units.iter().find(|item| item.unit_id == *unit_id))
            .map(|item| item.level),
    );
    assert!(daily_grant > 0);
    record.token_balance = 4;
    record.token_granted_total = 4;
    record.token_used_total = 0;
    record.last_token_grant_date = Some("2000-01-01".to_string());
    context
        .state
        .user_store
        .update_user(&record)
        .expect("seed token account");

    let deduct_amount = daily_grant + 2;
    let path = format!(
        "/wunder/admin/user_accounts/{}/token_adjustment",
        created.user_id
    );
    let (status, payload) = send_json(
        &context.app,
        Method::POST,
        &path,
        json!({
            "action": "deduct",
            "amount": deduct_amount
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["adjustment"]["action"], json!("deduct"));
    assert_eq!(payload["adjustment"]["amount"], json!(deduct_amount));

    let after = context
        .state
        .user_store
        .get_user_by_id(&created.user_id)
        .expect("reload after deduct")
        .expect("user exists");
    assert_eq!(after.token_balance, 2);
    assert_eq!(after.token_granted_total, 4 + daily_grant);
    assert_eq!(after.token_used_total, deduct_amount);
    assert_eq!(after.last_token_grant_date, Some(UserStore::today_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_user_account_token_adjustment_rejects_admin_accounts() {
    let context = build_test_context("admin-user-account-token-adjustment-admin").await;
    let created = context
        .state
        .user_store
        .create_user(
            "token_adjust_admin_target",
            Some("token_adjust_admin_target@example.test".to_string()),
            "password-123",
            Some("A"),
            None,
            vec!["admin".to_string()],
            "active",
            false,
        )
        .expect("create user");

    let path = format!(
        "/wunder/admin/user_accounts/{}/token_adjustment",
        created.user_id
    );
    let (status, payload) = send_json(
        &context.app,
        Method::POST,
        &path,
        json!({
            "action": "grant",
            "amount": 10
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        payload["error"],
        json!("admin users do not use token balance limits")
    );
}
