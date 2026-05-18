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
    config_store::ConfigStore,
    state::{AppState, AppStateInitOptions},
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
    bearer_token: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = bearer_token {
        builder = builder.header(AUTHORIZATION, format!("Bearer {token}"));
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
        .expect("read response body");
    let payload = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("parse response json")
    };
    (status, payload)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_org_units_import_replaces_units_and_migrates_users() {
    let context = build_test_context("admin-org-units-import").await;

    let admin = context
        .state
        .user_store
        .ensure_default_admin()
        .expect("ensure admin");
    let admin_token = context
        .state
        .user_store
        .create_session(&admin.user_id)
        .expect("create admin token");

    let legacy_root = wunder_server::storage::OrgUnitRecord {
        unit_id: "unit_legacy_root".to_string(),
        parent_id: None,
        name: "旧一级单位".to_string(),
        level: 1,
        path: "unit_legacy_root".to_string(),
        path_name: "旧一级单位".to_string(),
        sort_order: 0,
        leader_ids: Vec::new(),
        created_at: 0.0,
        updated_at: 0.0,
    };
    context
        .state
        .user_store
        .upsert_org_unit(&legacy_root)
        .expect("seed legacy org unit");

    let imported_user = context
        .state
        .user_store
        .create_user(
            "org_import_user",
            Some("org_import_user@example.test".to_string()),
            "password-123",
            Some("A"),
            Some(legacy_root.unit_id.clone()),
            vec!["user".to_string()],
            "active",
            false,
        )
        .expect("create user");

    let payload = json!({
      "units": [
        {
          "name": "一级单位1",
          "children": [
            { "name": "二级单位1" }
          ]
        },
        {
          "name": "一级单位2",
          "children": [
            { "name": "二级单位1" }
          ]
        }
      ],
      "migrate_user_root_name": "一级单位2"
    });

    let (status, body) = send_json(
        &context.app,
        Method::POST,
        "/wunder/admin/org_units/import",
        payload,
        Some(&admin_token),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["imported_count"], json!(4));
    assert_eq!(body["data"]["migrated_user_count"], json!(1));
    assert_eq!(body["data"]["migrate_user_root_name"], json!("一级单位2"));

    let units = context.state.user_store.list_org_units().expect("list org units");
    assert_eq!(units.len(), 4);
    assert!(units.iter().all(|item| item.name.starts_with("一级单位") || item.name.starts_with("二级单位")));
    let migrated_root = units
        .iter()
        .find(|item| item.parent_id.is_none() && item.name == "一级单位2")
        .expect("migrated root exists");
    let migrated_user = context
        .state
        .user_store
        .get_user_by_id(&imported_user.user_id)
        .expect("reload user")
        .expect("user exists");
    assert_eq!(migrated_user.unit_id.as_deref(), Some(migrated_root.unit_id.as_str()));
}
