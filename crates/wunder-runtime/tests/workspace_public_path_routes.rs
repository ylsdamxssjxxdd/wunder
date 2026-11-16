//! Chat document previews request `/wunder/workspace/content` with the full
//! public path the agent emitted, e.g.
//! `/workspaces/<user>/workspace/temp/report.md`. The cloud layout keeps the
//! agent workspace one `workspace` level below the public physical root, so
//! the content endpoint must map the public path through the public root
//! instead of re-joining the relative part onto the agent workspace (which
//! produced `workspace/workspace/...` and a 404 that rendered as
//! "该文件已被移除"). The download endpoint already mapped correctly; these
//! routes pin both.
#![cfg(feature = "sqlite-storage")]
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
    state: Arc<AppState>,
    app: Router,
    username: String,
    _temp_dir: TempDir,
}

async fn build_context(username: &str) -> TestContext {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = Config::default();
    config.storage.backend = "sqlite".to_string();
    config.storage.data_root = temp_dir.path().join("data").to_string_lossy().to_string();
    config.workspace.root = temp_dir
        .path()
        .join("workspaces")
        .to_string_lossy()
        .to_string();

    let state = Arc::new(
        AppState::new_with_options(
            ConfigStore::new(temp_dir.path().join("wunder.yaml")),
            config,
            AppStateInitOptions::cli_default(),
        )
        .expect("create app state"),
    );
    state.workspace.set_flatten_agent_scope(true);
    state
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

    let app = build_router(state.clone());
    TestContext {
        state,
        app,
        username: username.to_string(),
        _temp_dir: temp_dir,
    }
}

async fn send_request(
    app: &Router,
    method: Method,
    path: &str,
    token: Option<&str>,
    payload: Option<Value>,
) -> (StatusCode, Vec<u8>) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header(AUTHORIZATION, format!("Bearer {token}"));
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
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    (status, bytes.to_vec())
}

async fn login(context: &TestContext) -> String {
    let (status, bytes) = send_request(
        &context.app,
        Method::POST,
        "/wunder/auth/login",
        None,
        Some(json!({
            "username": context.username,
            "password": "password-123",
        })),
    )
    .await;
    let body: Value = serde_json::from_slice(&bytes).expect("login body");
    assert_eq!(status, StatusCode::OK, "login failed: {body}");
    body["data"]["access_token"]
        .as_str()
        .expect("access token")
        .to_string()
}

/// Seed the file the agent would have written inside its cloud workspace
/// (`<root>/<user>/workspace/temp/...`), which the sandbox sees as
/// `/workspaces/<user>/workspace/temp/...`.
fn seed_agent_workspace_file(context: &TestContext, relative: &str, content: &str) {
    // Run the one-off cloud flatten migration before seeding: it merges files
    // from the user root into `<user>/workspace`, which would otherwise relocate
    // the fixture on the first request and make every case look like a 404.
    let _ = context.state.workspace.ensure_user_root(&context.username);
    let scoped = context
        .state
        .workspace
        .scoped_user_id(&context.username, Some("any-agent"));
    let root = context.state.workspace.workspace_root(&scoped);
    let target = root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
    std::fs::create_dir_all(target.parent().expect("parent")).expect("mkdir");
    std::fs::write(&target, content).expect("write fixture");
}

#[tokio::test]
async fn content_endpoint_resolves_public_workspace_path_with_workspace_segment() {
    let context = build_context("doc_preview_user").await;
    let token = login(&context).await;
    seed_agent_workspace_file(
        &context,
        "temp/edit_tool_test_2026.md",
        "# 测试文件\n正文内容\n",
    );

    let path = format!(
        "/wunder/workspace/content?path=/workspaces/{}/workspace/temp/edit_tool_test_2026.md&include_content=true",
        context.username
    );
    let (status, bytes) = send_request(&context.app, Method::GET, &path, Some(&token), None).await;
    let body: Value = serde_json::from_slice(&bytes).expect("content body");
    assert_eq!(
        status,
        StatusCode::OK,
        "public workspace path failed: {body}"
    );
    assert_eq!(body["content"], "# 测试文件\n正文内容\n");
    assert_eq!(body["path"], "temp/edit_tool_test_2026.md");
}

#[tokio::test]
async fn content_endpoint_keeps_bare_relative_paths_working() {
    let context = build_context("doc_preview_bare").await;
    let token = login(&context).await;
    seed_agent_workspace_file(&context, "notes/plain.md", "plain content\n");

    let (status, bytes) = send_request(
        &context.app,
        Method::GET,
        "/wunder/workspace/content?path=notes/plain.md&include_content=true",
        Some(&token),
        None,
    )
    .await;
    let body: Value = serde_json::from_slice(&bytes).expect("content body");
    assert_eq!(status, StatusCode::OK, "bare relative path failed: {body}");
    assert_eq!(body["content"], "plain content\n");
}

#[tokio::test]
async fn download_endpoint_resolves_public_workspace_path_with_workspace_segment() {
    let context = build_context("doc_download_user").await;
    let token = login(&context).await;
    seed_agent_workspace_file(&context, "temp/result.md", "download me\n");

    let path = format!(
        "/wunder/workspace/download?path=/workspaces/{}/workspace/temp/result.md",
        context.username
    );
    let (status, bytes) = send_request(&context.app, Method::GET, &path, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "download failed");
    assert_eq!(bytes, b"download me\n");
}

/// Chat markdown may cite a bare relative path such as
/// `![](temp/test_cmd/pic.png)`. The cloud frontend turns it into a public path
/// that omits the flat layout's `workspace` level, and older messages may still
/// carry a stale `__c__N` scope. Both must resolve to the agent workspace file
/// instead of a 404 that renders as "该文件已被移除".
#[tokio::test]
async fn download_endpoint_resolves_public_path_without_workspace_segment() {
    let context = build_context("doc_download_bare_public").await;
    let token = login(&context).await;
    seed_agent_workspace_file(&context, "temp/测试图.png", "png-bytes\n");

    let path = format!(
        "/wunder/workspace/download?path=/workspaces/{}/temp/%E6%B5%8B%E8%AF%95%E5%9B%BE.png",
        context.username
    );
    let (status, bytes) = send_request(&context.app, Method::GET, &path, Some(&token), None).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "missing workspace segment 404: {bytes:?}"
    );
    assert_eq!(bytes, b"png-bytes\n");
}

#[tokio::test]
async fn download_endpoint_resolves_stale_container_scoped_public_path() {
    let context = build_context("doc_download_scoped_public").await;
    let token = login(&context).await;
    seed_agent_workspace_file(&context, "temp/stale.png", "png-bytes\n");

    let path = format!(
        "/wunder/workspace/download?path=/workspaces/{}__c__1/temp/stale.png",
        context.username
    );
    let (status, bytes) = send_request(&context.app, Method::GET, &path, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "stale scoped path 404: {bytes:?}");
    assert_eq!(bytes, b"png-bytes\n");
}

#[tokio::test]
async fn content_endpoint_resolves_public_path_without_workspace_segment() {
    let context = build_context("doc_content_bare_public").await;
    let token = login(&context).await;
    seed_agent_workspace_file(&context, "temp/plain.md", "plain content\n");

    let path = format!(
        "/wunder/workspace/content?path=/workspaces/{}/temp/plain.md&include_content=true",
        context.username
    );
    let (status, bytes) = send_request(&context.app, Method::GET, &path, Some(&token), None).await;
    let body: Value = serde_json::from_slice(&bytes).expect("content body");
    assert_eq!(
        status,
        StatusCode::OK,
        "missing workspace segment 404: {body}"
    );
    assert_eq!(body["content"], "plain content\n");
}
