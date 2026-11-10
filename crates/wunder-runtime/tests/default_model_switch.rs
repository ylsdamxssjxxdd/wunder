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
    config::{Config, LlmModelConfig, PresetCustomizable, UserAgentPresetConfig},
    config_store::ConfigStore,
    state::{AppState, AppStateInitOptions},
};

struct TestContext {
    state: Arc<AppState>,
    app: Router,
    token: String,
    _temp_dir: TempDir,
}

fn build_llm_model(base_url: &str, model: &str, model_type: &str) -> LlmModelConfig {
    LlmModelConfig {
        enable: Some(true),
        provider: Some("openai".to_string()),
        api_mode: None,
        base_url: Some(base_url.to_string()),
        api_key: Some("test-key".to_string()),
        model: Some(model.to_string()),
        temperature: Some(0.0),
        timeout_s: Some(15),
        max_rounds: Some(4),
        max_context: Some(16_384),
        max_output: Some(256),
        thinking_token_budget: None,
        support_vision: Some(false),
        support_hearing: Some(false),
        stream: Some(false),
        stream_include_usage: Some(false),
        history_compaction_ratio: None,
        tool_call_mode: Some("tool_call".to_string()),
        reasoning_effort: None,
        model_type: Some(model_type.to_string()),
        stop: None,
        mock_if_unconfigured: None,
        ..Default::default()
    }
}

fn build_preset_config(preset_id: &str) -> UserAgentPresetConfig {
    UserAgentPresetConfig {
        preset_id: preset_id.to_string(),
        revision: 1,
        name: "Switch Default Preset".to_string(),
        description: "preset".to_string(),
        system_prompt: "You are a preset agent.".to_string(),
        preview_skill: false,
        model_name: None,
        icon: None,
        icon_name: "spark".to_string(),
        icon_color: "#94a3b8".to_string(),
        sandbox_container_id: 1,
        tool_names: Vec::new(),
        declared_tool_names: Vec::new(),
        declared_skill_names: Vec::new(),
        visible_unit_ids: Vec::new(),
        preset_questions: Vec::new(),
        approval_mode: "full_auto".to_string(),
        status: "active".to_string(),
        customizable: PresetCustomizable::ALL,
    }
}

async fn build_context<F>(username: &str, configure: F) -> TestContext
where
    F: FnOnce(&mut Config),
{
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = Config::default();
    config.storage.backend = "sqlite".to_string();
    config.storage.data_root = temp_dir.path().join("data").to_string_lossy().to_string();
    config.workspace.root = temp_dir
        .path()
        .join("workspaces")
        .to_string_lossy()
        .to_string();
    config.skills.enabled.clear();
    configure(&mut config);

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

    let app = build_router(state.clone());
    TestContext {
        state,
        app,
        token,
        _temp_dir: temp_dir,
    }
}

async fn send_json(
    app: &Router,
    token: Option<&str>,
    method: Method,
    path: &str,
    payload: Option<Value>,
) -> (StatusCode, Value) {
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
    let payload = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("parse response json")
    };
    (status, payload)
}

async fn switch_admin_default(context: &TestContext, new_default: &str) {
    context
        .state
        .user_store
        .ensure_default_admin()
        .expect("ensure default admin");
    let admin_token = context
        .state
        .user_store
        .create_session_token("admin")
        .expect("admin token")
        .token;
    let config = context.state.config_store.get().await;
    let mut llm = config.llm.clone();
    llm.default = new_default.to_string();
    let (status, payload) = send_json(
        &context.app,
        Some(&admin_token),
        Method::POST,
        "/wunder/admin/llm",
        Some(json!({ "llm": llm })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{payload}");
}

async fn agent_payload(context: &TestContext) -> Value {
    let (status, payload) = send_json(
        &context.app,
        Some(&context.token),
        Method::GET,
        "/wunder/agents",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{payload}");
    payload["data"]["items"][0].clone()
}

async fn create_session(context: &TestContext, agent_id: &str) -> Value {
    let (status, payload) = send_json(
        &context.app,
        Some(&context.token),
        Method::POST,
        "/wunder/chat/sessions",
        Some(json!({ "title": "new thread", "agent_id": agent_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{payload}");
    payload["data"].clone()
}

fn base_models(config: &mut Config) {
    config.llm.default = "model-a".to_string();
    config.llm.models.clear();
    config.llm.models.insert(
        "model-a".to_string(),
        build_llm_model("http://127.0.0.1:18080/v1", "provider-a", "llm"),
    );
    config.llm.models.insert(
        "model-b".to_string(),
        build_llm_model("http://127.0.0.1:18080/v1", "provider-b", "llm"),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn new_thread_follows_admin_default_switch_for_preset_agent() {
    let context = build_context("switch_user_preset", |config| {
        base_models(config);
        config.user_agents.presets = vec![build_preset_config("preset_switch")];
    })
    .await;

    let before = agent_payload(&context).await;
    let agent_id = before["id"].as_str().expect("agent id").to_string();
    assert_eq!(before["configured_model_name"], Value::Null);
    assert_eq!(before["model_name"], json!("model-a"));
    assert_eq!(
        create_session(&context, &agent_id).await["model_key"],
        json!("model-a")
    );

    switch_admin_default(&context, "model-b").await;

    let after = agent_payload(&context).await;
    assert_eq!(after["configured_model_name"], Value::Null);
    assert_eq!(after["model_name"], json!("model-b"));
    assert_eq!(
        create_session(&context, &agent_id).await["model_key"],
        json!("model-b")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn new_thread_follows_admin_default_switch_for_default_agent() {
    let context = build_context("switch_user_default", base_models).await;

    let before = agent_payload(&context).await;
    assert_eq!(before["id"], json!("__default__"));
    assert_eq!(before["configured_model_name"], Value::Null);
    assert_eq!(before["model_name"], json!("model-a"));
    assert_eq!(
        create_session(&context, "").await["model_key"],
        json!("model-a")
    );

    switch_admin_default(&context, "model-b").await;

    let after = agent_payload(&context).await;
    assert_eq!(after["configured_model_name"], Value::Null);
    assert_eq!(after["model_name"], json!("model-b"));
    assert_eq!(
        create_session(&context, "").await["model_key"],
        json!("model-b")
    );
}
