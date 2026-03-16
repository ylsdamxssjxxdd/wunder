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
    build_desktop_router,
    config::Config,
    config_store::ConfigStore,
    state::{AppState, AppStateInitOptions},
};

struct TestContext {
    app: Router,
    token: String,
    _temp_dir: TempDir,
}

async fn build_test_context(username: &str) -> TestContext {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = Config::default();
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = temp_dir
        .path()
        .join("memory-routes.db")
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
        token,
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn memory_routes_and_prompt_preview_work_end_to_end() {
    let context = build_test_context("memory_route_user").await;

    let (status, created) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/agents/__default__/memories",
        Some(json!({
            "title_l0": "Reply style",
            "summary_l1": "Prefer concise answers.",
            "content_l2": "Prefer concise answers.",
            "fact_key": "preference::reply_style"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let memory_id = created["data"]["item"]["memory_id"]
        .as_str()
        .expect("memory id")
        .to_string();

    let (status, listed) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        "/wunder/agents/__default__/memories?limit=200",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["data"]["total"], json!(1));
    assert_eq!(listed["data"]["items"][0]["memory_id"], json!(memory_id));

    let (status, pinned) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/agents/__default__/memories/{memory_id}/pin"),
        Some(json!({ "value": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pinned["data"]["item"]["pinned"], json!(true));

    let (status, preview) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/chat/system-prompt",
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let prompt = preview["data"]["prompt"]
        .as_str()
        .expect("system prompt preview");
    assert!(prompt.contains("Prefer concise answers."));
    assert_eq!(preview["data"]["memory_preview_count"], json!(1));

    let (status, updated) = send_json(
        &context.app,
        &context.token,
        Method::PATCH,
        &format!("/wunder/agents/__default__/memories/{memory_id}"),
        Some(json!({
            "summary_l1": "Prefer bullet lists in answers.",
            "content_l2": "Prefer bullet lists in answers.",
            "pinned": true
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        updated["data"]["item"]["summary_l1"],
        json!("Prefer bullet lists in answers.")
    );

    let (status, refreshed_preview) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/chat/system-prompt",
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let refreshed_prompt = refreshed_preview["data"]["prompt"]
        .as_str()
        .expect("refreshed prompt preview");
    assert!(refreshed_prompt.contains("Prefer bullet lists in answers."));

    let (status, invalidated) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/agents/__default__/memories/{memory_id}/invalidate"),
        Some(json!({ "value": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(invalidated["data"]["item"]["status"], json!("invalidated"));

    let (status, active_list) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        "/wunder/agents/__default__/memories?limit=200",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(active_list["data"]["total"], json!(0));

    let (status, full_list) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        "/wunder/agents/__default__/memories?limit=200&include_invalidated=true",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(full_list["data"]["total"], json!(1));
    assert_eq!(
        full_list["data"]["items"][0]["status"],
        json!("invalidated")
    );

    let (status, deleted) = send_json(
        &context.app,
        &context.token,
        Method::DELETE,
        &format!("/wunder/agents/__default__/memories/{memory_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(deleted["data"]["deleted"], json!(true));

    let (status, empty_list) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        "/wunder/agents/__default__/memories?limit=200&include_invalidated=true",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(empty_list["data"]["total"], json!(0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_prompt_preview_reuses_frozen_memory_snapshot() {
    let context = build_test_context("memory_prompt_user").await;

    let (status, created) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/agents/__default__/memories",
        Some(json!({
            "title_l0": "Reply language",
            "summary_l1": "Reply in English by default.",
            "content_l2": "Reply in English by default.",
            "fact_key": "preference::reply_language"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(created["data"]["item"]["memory_id"].is_string());

    let (status, session_created) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/chat/sessions",
        Some(json!({
            "title": "Frozen memory session"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let session_id = session_created["data"]["id"]
        .as_str()
        .expect("session id")
        .to_string();

    let (status, session_preview_before_change) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/chat/sessions/{session_id}/system-prompt"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        session_preview_before_change["data"]["memory_preview_mode"],
        json!("frozen")
    );
    let frozen_prompt = session_preview_before_change["data"]["prompt"]
        .as_str()
        .expect("frozen system prompt")
        .to_string();
    assert!(frozen_prompt.contains("Reply in English by default."));

    let (status, second_memory) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/agents/__default__/memories",
        Some(json!({
            "title_l0": "Reply language",
            "summary_l1": "Reply in Chinese by default.",
            "content_l2": "Reply in Chinese by default.",
            "fact_key": "preference::reply_language_new"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(second_memory["data"]["item"]["memory_id"].is_string());

    let (status, session_preview) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/chat/sessions/{session_id}/system-prompt"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        session_preview["data"]["memory_preview_mode"],
        json!("frozen")
    );
    assert_eq!(session_preview["data"]["memory_preview_count"], json!(1));
    let reused_prompt = session_preview["data"]["prompt"]
        .as_str()
        .expect("session system prompt");
    assert!(reused_prompt.contains("Reply in English by default."));
    assert!(!reused_prompt.contains("Reply in Chinese by default."));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_memory_settings_can_toggle_auto_extract() {
    let context = build_test_context("memory_settings_user").await;

    let (status, initial_settings) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        "/wunder/agents/__default__/memory-settings",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        initial_settings["data"]["settings"]["auto_extract_enabled"],
        json!(false)
    );

    let (status, updated_settings) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/agents/__default__/memory-settings",
        Some(json!({
            "auto_extract_enabled": true
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        updated_settings["data"]["settings"]["auto_extract_enabled"],
        json!(true)
    );
    assert!(
        updated_settings["data"]["settings"]["updated_at"]
            .as_f64()
            .expect("updated at")
            > 0.0
    );

    let (status, listed) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        "/wunder/agents/__default__/memories?limit=200&include_invalidated=true",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        listed["data"]["settings"]["auto_extract_enabled"],
        json!(true)
    );

    let (status, refreshed_settings) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        "/wunder/agents/__default__/memory-settings",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        refreshed_settings["data"]["settings"]["auto_extract_enabled"],
        json!(true)
    );
}
