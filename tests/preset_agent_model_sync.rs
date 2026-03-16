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
    config::{Config, LlmModelConfig, UserAgentPresetConfig},
    config_store::ConfigStore,
    state::{AppState, AppStateInitOptions},
};

struct TestContext {
    state: Arc<AppState>,
    app: Router,
    token: String,
    _temp_dir: TempDir,
}

const PRESET_NAME: &str = "Preset Auto Model";

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
        retry: Some(0),
        max_rounds: Some(4),
        max_context: Some(16_384),
        max_output: Some(256),
        support_vision: Some(false),
        support_hearing: Some(false),
        stream: Some(false),
        stream_include_usage: Some(false),
        history_compaction_ratio: None,
        history_compaction_reset: None,
        tool_call_mode: Some("tool_call".to_string()),
        model_type: Some(model_type.to_string()),
        stop: None,
        mock_if_unconfigured: None,
    }
}

fn build_preset_config(preset_id: &str, model_name: Option<&str>) -> UserAgentPresetConfig {
    UserAgentPresetConfig {
        preset_id: preset_id.to_string(),
        revision: 1,
        name: PRESET_NAME.to_string(),
        description: "preset model test".to_string(),
        system_prompt: "You are a preset agent.".to_string(),
        model_name: model_name.map(str::to_string),
        icon_name: "spark".to_string(),
        icon_color: "#94a3b8".to_string(),
        sandbox_container_id: 2,
        tool_names: Vec::new(),
        declared_tool_names: Vec::new(),
        declared_skill_names: Vec::new(),
        preset_questions: Vec::new(),
        approval_mode: "full_auto".to_string(),
        status: "active".to_string(),
    }
}

async fn build_test_context_with_config<F>(username: &str, configure: F) -> TestContext
where
    F: FnOnce(&mut Config),
{
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = Config::default();
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = temp_dir
        .path()
        .join("preset-agent-model-sync.db")
        .to_string_lossy()
        .to_string();
    config.workspace.root = temp_dir
        .path()
        .join("workspaces")
        .to_string_lossy()
        .to_string();
    config.skills.enabled.clear();
    configure(&mut config);

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
        let bearer = format!("Bearer {token}");
        builder = builder.header(AUTHORIZATION, bearer);
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

fn find_agent_id_by_name(items: &[Value], name: &str) -> String {
    items
        .iter()
        .find(|item| item["name"] == json!(name))
        .and_then(|item| item["id"].as_str())
        .expect("agent id by name")
        .to_string()
}

fn find_agent_by_name<'a>(items: &'a [Value], name: &str) -> &'a Value {
    items
        .iter()
        .find(|item| item["name"] == json!(name))
        .expect("agent not found by name")
}

fn find_preset_item<'a>(items: &'a [Value], preset_id: &str) -> &'a Value {
    items
        .iter()
        .find(|item| item["preset_id"] == json!(preset_id))
        .expect("preset not found by id")
}

async fn list_user_agents(app: &Router, token: &str) -> Vec<Value> {
    let (status, payload) = send_json(app, Some(token), Method::GET, "/wunder/agents", None).await;
    assert_eq!(status, StatusCode::OK);
    payload["data"]["items"]
        .as_array()
        .expect("agents list should be array")
        .clone()
}

async fn list_admin_presets(app: &Router) -> Vec<Value> {
    let (status, payload) =
        send_json(app, None, Method::GET, "/wunder/admin/preset_agents", None).await;
    assert_eq!(status, StatusCode::OK);
    payload["data"]["items"]
        .as_array()
        .expect("preset list should be array")
        .clone()
}

async fn update_admin_presets(app: &Router, items: Vec<Value>) -> Vec<Value> {
    let (status, payload) = send_json(
        app,
        None,
        Method::POST,
        "/wunder/admin/preset_agents",
        Some(json!({ "items": items })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    payload["data"]["items"]
        .as_array()
        .expect("updated preset list should be array")
        .clone()
}

async fn sync_preset(
    app: &Router,
    preset_id: &str,
    mode: &str,
    scope_unit_id: Option<&str>,
) -> Value {
    let mut payload = json!({
        "preset_id": preset_id,
        "mode": mode,
    });
    if let Some(unit_id) = scope_unit_id {
        payload["scope_unit_id"] = json!(unit_id);
    }
    let (status, body) = send_json(
        app,
        None,
        Method::POST,
        "/wunder/admin/preset_agents/sync",
        Some(payload),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    body
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preset_agent_bootstrap_uses_preset_model_name() {
    let context = build_test_context_with_config("preset_model_bootstrap_user", |config| {
        config.llm.default = "model-default".to_string();
        config.llm.models.clear();
        config.llm.models.insert(
            "model-default".to_string(),
            build_llm_model("http://127.0.0.1:18080/v1", "provider-default", "llm"),
        );
        config.llm.models.insert(
            "model-preset".to_string(),
            build_llm_model("http://127.0.0.1:18080/v1", "provider-preset", "llm"),
        );
        config.llm.models.insert(
            "model-embedding".to_string(),
            build_llm_model(
                "http://127.0.0.1:18080/v1",
                "provider-embedding",
                "embedding",
            ),
        );
        config.user_agents.presets = vec![build_preset_config(
            "preset_auto_model_bootstrap",
            Some("model-preset"),
        )];
    })
    .await;

    let admin_items = list_admin_presets(&context.app).await;
    let admin_preset = find_preset_item(&admin_items, "preset_auto_model_bootstrap");
    assert_eq!(admin_preset["model_name"], json!("model-preset"));

    let user_items = list_user_agents(&context.app, &context.token).await;
    let preset_agent = find_agent_by_name(&user_items, PRESET_NAME);
    assert_eq!(preset_agent["configured_model_name"], json!("model-preset"));
    assert_eq!(preset_agent["model_name"], json!("model-preset"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preset_model_sync_safe_and_force_respects_user_override() {
    let context = build_test_context_with_config("preset_sync_user_a", |config| {
        config.llm.default = "model-default".to_string();
        config.llm.models.clear();
        config.llm.models.insert(
            "model-default".to_string(),
            build_llm_model("http://127.0.0.1:18080/v1", "provider-default", "llm"),
        );
        config.llm.models.insert(
            "model-a".to_string(),
            build_llm_model("http://127.0.0.1:18080/v1", "provider-a", "llm"),
        );
        config.llm.models.insert(
            "model-b".to_string(),
            build_llm_model("http://127.0.0.1:18080/v1", "provider-b", "llm"),
        );
        config.llm.models.insert(
            "model-c".to_string(),
            build_llm_model("http://127.0.0.1:18080/v1", "provider-c", "llm"),
        );
        config.user_agents.presets = vec![build_preset_config(
            "preset_sync_model_name",
            Some("model-a"),
        )];
    })
    .await;

    let user_b = context
        .state
        .user_store
        .create_user(
            "preset_sync_user_b",
            Some("preset_sync_user_b@example.test".to_string()),
            "password-123",
            Some("A"),
            None,
            vec!["user".to_string()],
            "active",
            false,
        )
        .expect("create second user");
    let token_b = context
        .state
        .user_store
        .create_session_token(&user_b.user_id)
        .expect("create second user token")
        .token;

    let admin_items = list_admin_presets(&context.app).await;
    let preset_id = find_preset_item(&admin_items, "preset_sync_model_name")["preset_id"]
        .as_str()
        .expect("preset id")
        .to_string();

    let sync_v1 = sync_preset(&context.app, &preset_id, "safe", None).await;
    let created_v1 = sync_v1["data"]["summary"]["created_agents"]
        .as_u64()
        .expect("created_agents should be number");
    assert!(
        created_v1 >= 2,
        "expected at least 2 created agents, got {created_v1}"
    );

    let user_a_v1 = list_user_agents(&context.app, &context.token).await;
    let user_b_v1 = list_user_agents(&context.app, &token_b).await;
    let user_b_agent_id = find_agent_id_by_name(&user_b_v1, PRESET_NAME);
    assert_eq!(
        find_agent_by_name(&user_a_v1, PRESET_NAME)["configured_model_name"],
        json!("model-a")
    );
    assert_eq!(
        find_agent_by_name(&user_b_v1, PRESET_NAME)["configured_model_name"],
        json!("model-a")
    );

    let (status, updated_user_b) = send_json(
        &context.app,
        Some(&token_b),
        Method::PUT,
        &format!("/wunder/agents/{user_b_agent_id}"),
        Some(json!({ "model_name": "user-custom-model" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        updated_user_b["data"]["configured_model_name"],
        json!("user-custom-model")
    );

    let mut admin_items_v2 = list_admin_presets(&context.app).await;
    for item in &mut admin_items_v2 {
        if item["preset_id"] == json!(preset_id.as_str()) {
            item["model_name"] = json!("model-b");
        }
    }
    let updated_items_v2 = update_admin_presets(&context.app, admin_items_v2).await;
    let preset_v2 = find_preset_item(&updated_items_v2, &preset_id);
    assert_eq!(preset_v2["revision"], json!(2));
    assert_eq!(preset_v2["model_name"], json!("model-b"));

    let sync_safe = sync_preset(&context.app, &preset_id, "safe", None).await;
    let safe_updated = sync_safe["data"]["summary"]["updated_agents"]
        .as_u64()
        .expect("updated_agents should be number");
    let safe_rebound = sync_safe["data"]["summary"]["rebound_agents"]
        .as_u64()
        .expect("rebound_agents should be number");
    let safe_overridden = sync_safe["data"]["summary"]["overridden_agents"]
        .as_u64()
        .expect("overridden_agents should be number");
    assert!(
        safe_updated >= 1,
        "expected at least 1 updated agent in safe sync, got {safe_updated}"
    );
    assert!(
        safe_rebound >= 1,
        "expected at least 1 rebound agent in safe sync, got {safe_rebound}"
    );
    assert!(
        safe_overridden >= 1,
        "expected at least 1 overridden agent in safe sync, got {safe_overridden}"
    );

    let user_a_v2 = list_user_agents(&context.app, &context.token).await;
    let user_b_v2 = list_user_agents(&context.app, &token_b).await;
    assert_eq!(
        find_agent_by_name(&user_a_v2, PRESET_NAME)["configured_model_name"],
        json!("model-b")
    );
    assert_eq!(
        find_agent_by_name(&user_b_v2, PRESET_NAME)["configured_model_name"],
        json!("user-custom-model")
    );

    let mut admin_items_v3 = list_admin_presets(&context.app).await;
    for item in &mut admin_items_v3 {
        if item["preset_id"] == json!(preset_id.as_str()) {
            item["model_name"] = json!("model-c");
        }
    }
    let updated_items_v3 = update_admin_presets(&context.app, admin_items_v3).await;
    let preset_v3 = find_preset_item(&updated_items_v3, &preset_id);
    assert_eq!(preset_v3["revision"], json!(3));
    assert_eq!(preset_v3["model_name"], json!("model-c"));

    let sync_force = sync_preset(&context.app, &preset_id, "force", None).await;
    let force_updated = sync_force["data"]["summary"]["updated_agents"]
        .as_u64()
        .expect("updated_agents should be number");
    assert!(
        force_updated >= 2,
        "expected at least 2 updated agents in force sync, got {force_updated}"
    );

    let user_a_v3 = list_user_agents(&context.app, &context.token).await;
    let user_b_v3 = list_user_agents(&context.app, &token_b).await;
    assert_eq!(
        find_agent_by_name(&user_a_v3, PRESET_NAME)["configured_model_name"],
        json!("model-c")
    );
    assert_eq!(
        find_agent_by_name(&user_b_v3, PRESET_NAME)["configured_model_name"],
        json!("model-c")
    );
}
