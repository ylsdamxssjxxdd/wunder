use axum::{
    body::{to_bytes, Body},
    http::{Method, Request, StatusCode},
    Router,
};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;
use wunder_server::{
    auth::{is_admin_path, is_leader_path},
    build_desktop_router, build_router,
    config::Config,
    config_store::ConfigStore,
    state::{AppState, AppStateInitOptions},
    tools::{builtin_tool_specs, resolve_tool_name},
};

struct ContractContext {
    server_app: Router,
    desktop_app: Router,
    _state: Arc<AppState>,
    _temp_dir: TempDir,
}

async fn build_contract_context() -> ContractContext {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let mut config = Config::default();
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = temp_dir
        .path()
        .join("backend-contracts.db")
        .to_string_lossy()
        .to_string();
    config.workspace.root = temp_dir
        .path()
        .join("workspaces")
        .to_string_lossy()
        .to_string();
    config.server.max_active_sessions = 3;

    let config_store = ConfigStore::new(temp_dir.path().join("wunder.yaml"));
    let config_for_store = config.clone();
    config_store
        .update(|current| *current = config_for_store.clone())
        .await
        .expect("write test config");

    let state = Arc::new(
        AppState::new_with_options(config_store, config, AppStateInitOptions::cli_default())
            .expect("create app state"),
    );

    ContractContext {
        server_app: build_router(state.clone()),
        desktop_app: build_desktop_router(state.clone()),
        _state: state,
        _temp_dir: temp_dir,
    }
}

async fn request_json(app: &Router, method: Method, path: &str, body: Option<Value>) -> StatusCode {
    let body = body
        .map(|value| Body::from(value.to_string()))
        .unwrap_or_else(Body::empty);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .body(body)
                .expect("build request"),
        )
        .await
        .expect("send request");
    let status = response.status();
    let _ = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("drain response");
    status
}

#[test]
fn admin_path_classifier_keeps_public_and_admin_surfaces_separate() {
    let public_paths = [
        "/",
        "/wunder/auth/login",
        "/wunder/chat/sessions",
        "/wunder/workspace/content",
        "/wunder/user_tools/catalog",
        "/wunder/plaza/items",
        "/wunder/external/workflows",
    ];
    for path in public_paths {
        assert!(
            !is_admin_path(path),
            "{path} should remain public/user-scoped"
        );
    }

    let admin_paths = [
        "/wunder",
        "/wunder/admin/server",
        "/wunder/admin/performance/sample",
        "/wunder/admin/user_accounts",
        "/wunder/admin/gateway/status",
        "/a2a",
    ];
    for path in admin_paths {
        assert!(is_admin_path(path), "{path} should require admin guard");
    }
}

#[test]
fn leader_path_classifier_is_limited_to_org_and_user_account_admin_routes() {
    assert!(is_leader_path("/wunder/admin/org_units"));
    assert!(is_leader_path("/wunder/admin/org_units/root"));
    assert!(is_leader_path("/wunder/admin/user_accounts"));
    assert!(is_leader_path("/wunder/admin/user_accounts/user-id"));
    assert!(!is_leader_path("/wunder/admin/performance/sample"));
    assert!(!is_leader_path("/wunder/admin/gateway/status"));
    assert!(!is_leader_path("/wunder/admin/users"));
}

#[tokio::test]
async fn server_router_exposes_admin_surface_but_desktop_router_does_not() {
    let context = build_contract_context().await;

    let server_status = request_json(
        &context.server_app,
        Method::GET,
        "/wunder/admin/server",
        None,
    )
    .await;
    let desktop_status = request_json(
        &context.desktop_app,
        Method::GET,
        "/wunder/admin/server",
        None,
    )
    .await;
    let desktop_user_status =
        request_json(&context.desktop_app, Method::GET, "/wunder/auth/me", None).await;

    assert_eq!(server_status, StatusCode::OK);
    assert_eq!(desktop_status, StatusCode::NOT_FOUND);
    assert_eq!(desktop_user_status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn performance_sample_rejects_invalid_concurrency_before_running_work() {
    let context = build_contract_context().await;

    let zero_status = request_json(
        &context.server_app,
        Method::POST,
        "/wunder/admin/performance/sample",
        Some(serde_json::json!({ "concurrency": 0 })),
    )
    .await;
    let over_limit_status = request_json(
        &context.server_app,
        Method::POST,
        "/wunder/admin/performance/sample",
        Some(serde_json::json!({ "concurrency": 4 })),
    )
    .await;

    assert_eq!(zero_status, StatusCode::BAD_REQUEST);
    assert_eq!(over_limit_status, StatusCode::BAD_REQUEST);
}

#[test]
fn builtin_tool_catalog_keeps_core_file_tools_available_and_unique() {
    let specs = builtin_tool_specs();
    let mut names = HashSet::new();
    for spec in &specs {
        let name = spec.name.trim();
        assert!(!name.is_empty(), "builtin tool name must not be empty");
        assert!(
            names.insert(name.to_string()),
            "builtin tool name must be unique: {name}"
        );
        assert!(
            !spec.description.trim().is_empty(),
            "builtin tool description must not be empty: {name}"
        );
        assert!(
            spec.input_schema.is_object(),
            "builtin tool input schema must be an object: {name}"
        );
    }

    for alias in [
        "list_files",
        "write_file",
        "read_file",
        "search_content",
        "apply_patch",
        "execute_command",
    ] {
        let canonical = resolve_tool_name(alias);
        assert!(
            names.contains(&canonical),
            "{alias} should resolve to builtin tool {canonical}"
        );
    }
}
