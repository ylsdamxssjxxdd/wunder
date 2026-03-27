use axum::{
    body::{to_bytes, Body},
    http::{header::AUTHORIZATION, Method, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use std::{path::Path, sync::Arc};
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
const DEFAULT_AGENT_ID_ALIAS: &str = "__default__";

fn normalize_test_preset_id(preset_id: &str) -> String {
    let cleaned = preset_id.trim();
    if cleaned.is_empty() || cleaned == DEFAULT_AGENT_ID_ALIAS {
        return cleaned.to_string();
    }
    if cleaned.starts_with("preset_") {
        return cleaned.to_string();
    }
    if let Some(stripped) = cleaned.strip_prefix("agent_") {
        return format!("preset_{stripped}");
    }
    format!("preset_{cleaned}")
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
        reasoning_effort: None,
        model_type: Some(model_type.to_string()),
        stop: None,
        mock_if_unconfigured: None,
    }
}

fn build_preset_config(preset_id: &str, model_name: Option<&str>) -> UserAgentPresetConfig {
    UserAgentPresetConfig {
        preset_id: normalize_test_preset_id(preset_id),
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
    build_test_context_with_temp_config(username, |config, _| configure(config)).await
}

async fn build_test_context_with_temp_config<F>(username: &str, configure: F) -> TestContext
where
    F: FnOnce(&mut Config, &std::path::Path),
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
    configure(&mut config, temp_dir.path());

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
    let expected = normalize_test_preset_id(preset_id);
    items
        .iter()
        .find(|item| item["preset_id"] == json!(expected))
        .expect("preset not found by id")
}

fn read_tool_names(agent: &Value) -> Vec<String> {
    agent["tool_names"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn read_declared_tool_names(agent: &Value) -> Vec<String> {
    agent["declared_tool_names"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn read_declared_skill_names(agent: &Value) -> Vec<String> {
    agent["declared_skill_names"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
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

async fn list_available_tool_names(app: &Router, user_id: &str) -> Vec<String> {
    let path = format!("/wunder/tools?user_id={user_id}");
    let (status, payload) = send_json(app, None, Method::GET, &path, None).await;
    assert_eq!(status, StatusCode::OK);
    let group_keys = [
        "builtin_tools",
        "mcp_tools",
        "skills",
        "knowledge_tools",
        "user_tools",
        "shared_tools",
        "admin_builtin_tools",
        "admin_mcp_tools",
    ];
    let mut names = Vec::new();
    for key in group_keys {
        if let Some(items) = payload[key].as_array() {
            for item in items {
                if let Some(name) = item["name"].as_str() {
                    let cleaned = name.trim();
                    if !cleaned.is_empty() {
                        names.push(cleaned.to_string());
                    }
                }
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

async fn get_default_agent(app: &Router, token: Option<&str>, user_id: &str) -> Value {
    let (status, payload) = send_json(
        app,
        token,
        Method::GET,
        &format!("/wunder/agents/{DEFAULT_AGENT_ID_ALIAS}?user_id={user_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    payload["data"].clone()
}

async fn update_default_agent(
    app: &Router,
    token: Option<&str>,
    user_id: &str,
    payload: Value,
) -> Value {
    let (status, body) = send_json(
        app,
        token,
        Method::PUT,
        &format!("/wunder/agents/{DEFAULT_AGENT_ID_ALIAS}?user_id={user_id}"),
        Some(payload),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    body["data"].clone()
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

async fn export_admin_preset_worker_card(app: &Router, preset_id: &str) -> Value {
    let normalized_preset_id = normalize_test_preset_id(preset_id);
    let (status, payload) = send_json(
        app,
        None,
        Method::GET,
        &format!("/wunder/admin/preset_agents/{normalized_preset_id}/worker_card"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    payload["data"].clone()
}

fn find_worker_card_document_by_agent_id(search_root: &Path, agent_id: &str) -> Option<Value> {
    fn visit(path: &Path, agent_id: &str) -> Option<Value> {
        for entry in std::fs::read_dir(path).ok()? {
            let entry = entry.ok()?;
            let child_path = entry.path();
            if entry.file_type().ok()?.is_dir() {
                if let Some(found) = visit(&child_path, agent_id) {
                    return Some(found);
                }
                continue;
            }
            let Some(file_name) = child_path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !file_name.ends_with(".worker-card.json") {
                continue;
            }
            let document: Value =
                serde_json::from_str(&std::fs::read_to_string(&child_path).ok()?).ok()?;
            if document["metadata"]["agent_id"] == json!(agent_id) {
                return Some(document);
            }
        }
        None
    }

    visit(search_root, agent_id)
}

async fn sync_preset(
    app: &Router,
    preset_id: &str,
    mode: &str,
    scope_unit_id: Option<&str>,
) -> Value {
    let normalized_preset_id = normalize_test_preset_id(preset_id);
    let mut payload = json!({
        "preset_id": normalized_preset_id,
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
async fn admin_preset_list_includes_default_agent_item() {
    let context = build_test_context_with_config("preset_admin_default_list", |_| {}).await;

    let items = list_admin_presets(&context.app).await;
    let default_item = find_preset_item(&items, DEFAULT_AGENT_ID_ALIAS);

    assert_eq!(default_item["is_default_agent"], json!(true));
    assert!(
        default_item["name"]
            .as_str()
            .is_some_and(|name| !name.trim().is_empty()),
        "default preset item should expose a visible name"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_preset_update_ignores_default_agent_item() {
    let context = build_test_context_with_config("preset_admin_default_update", |config| {
        config.user_agents.presets = vec![build_preset_config("preset_admin_default_keep", None)];
    })
    .await;

    let items = list_admin_presets(&context.app).await;
    assert_eq!(items.len(), 2);

    let updated = update_admin_presets(&context.app, items).await;
    assert_eq!(updated.len(), 2);
    assert_eq!(
        find_preset_item(&updated, DEFAULT_AGENT_ID_ALIAS)["is_default_agent"],
        json!(true)
    );
    assert_eq!(
        context
            .state
            .config_store
            .get()
            .await
            .user_agents
            .presets
            .len(),
        1,
        "default agent item should not be persisted into ordinary preset config"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_default_agent_sync_safe_and_force_respects_user_override() {
    let context = build_test_context_with_config("default_sync_user_a", |_| {}).await;
    context
        .state
        .user_store
        .ensure_default_admin()
        .expect("ensure default admin");
    let admin_token = context
        .state
        .user_store
        .create_session_token("admin")
        .expect("create admin token")
        .token;
    context
        .state
        .user_store
        .create_user(
            "default_sync_user_b",
            Some("default_sync_user_b@example.test".to_string()),
            "password-123",
            Some("A"),
            None,
            vec!["user".to_string()],
            "active",
            false,
        )
        .expect("create second user");

    update_default_agent(
        &context.app,
        Some(&admin_token),
        "preset_template",
        json!({
            "name": "Template Default Agent",
            "description": "template-description",
            "system_prompt": "template-system-prompt",
            "tool_names": [],
            "preset_questions": ["What should I do next?"],
            "approval_mode": "full_auto",
            "status": "active",
            "sandbox_container_id": 7
        }),
    )
    .await;

    update_default_agent(
        &context.app,
        Some(&admin_token),
        "default_sync_user_b",
        json!({
            "name": "User Customized Default",
            "description": "custom-description",
            "system_prompt": "custom-system-prompt",
            "tool_names": [],
            "preset_questions": ["custom-question"],
            "approval_mode": "suggest",
            "status": "active",
            "sandbox_container_id": 3
        }),
    )
    .await;

    let safe_summary = sync_preset(&context.app, DEFAULT_AGENT_ID_ALIAS, "safe", None).await;
    assert_eq!(
        safe_summary["data"]["preset"]["preset_id"],
        json!(DEFAULT_AGENT_ID_ALIAS)
    );

    let user_a_after_safe =
        get_default_agent(&context.app, Some(&admin_token), "default_sync_user_a").await;
    assert_eq!(user_a_after_safe["name"], json!("Template Default Agent"));
    assert_eq!(
        user_a_after_safe["description"],
        json!("template-description")
    );
    assert_eq!(
        user_a_after_safe["system_prompt"],
        json!("template-system-prompt")
    );

    let user_b_after_safe =
        get_default_agent(&context.app, Some(&admin_token), "default_sync_user_b").await;
    assert_eq!(
        user_b_after_safe["description"],
        json!("custom-description"),
        "safe sync should keep customized default-agent fields"
    );
    assert_eq!(user_b_after_safe["approval_mode"], json!("suggest"));

    let force_summary = sync_preset(&context.app, DEFAULT_AGENT_ID_ALIAS, "force", None).await;
    assert_eq!(
        force_summary["data"]["preset"]["preset_id"],
        json!(DEFAULT_AGENT_ID_ALIAS)
    );

    let user_b_after_force =
        get_default_agent(&context.app, Some(&admin_token), "default_sync_user_b").await;
    assert_eq!(user_b_after_force["name"], json!("Template Default Agent"));
    assert_eq!(
        user_b_after_force["description"],
        json!("template-description")
    );
    assert_eq!(
        user_b_after_force["system_prompt"],
        json!("template-system-prompt")
    );
    assert_eq!(
        user_b_after_force["preset_questions"],
        json!(["What should I do next?"])
    );
    assert_eq!(user_b_after_force["approval_mode"], json!("full_auto"));
    assert_eq!(user_b_after_force["sandbox_container_id"], json!(7));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_default_agent_sync_force_updates_tools_and_skills() {
    let context = build_test_context_with_config("default_sync_tools_user_a", |config| {
        config.tools.builtin.enabled = vec!["读取文件".to_string(), "写入文件".to_string()];
        config.skills.enabled = vec!["技能创建器".to_string()];
    })
    .await;
    context
        .state
        .user_store
        .ensure_default_admin()
        .expect("ensure default admin");
    let admin_token = context
        .state
        .user_store
        .create_session_token("admin")
        .expect("create admin token")
        .token;
    context
        .state
        .user_store
        .create_user(
            "default_sync_tools_user_b",
            Some("default_sync_tools_user_b@example.test".to_string()),
            "password-123",
            Some("A"),
            None,
            vec!["user".to_string()],
            "active",
            false,
        )
        .expect("create second user");

    let available_tools =
        list_available_tool_names(&context.app, "default_sync_tools_user_a").await;
    let tool_name = "读取文件".to_string();
    let skill_name = "技能创建器".to_string();
    assert!(
        available_tools.iter().any(|name| name == &tool_name),
        "expected default-agent catalog to contain {tool_name}"
    );
    assert!(
        available_tools.iter().any(|name| name == &skill_name),
        "expected default-agent catalog to contain {skill_name}"
    );

    let template_default = update_default_agent(
        &context.app,
        Some(&admin_token),
        "preset_template",
        json!({
            "name": "Template Default Agent",
            "description": "template-description",
            "system_prompt": "template-system-prompt",
            "tool_names": [tool_name.clone(), skill_name.clone()],
            "preset_questions": ["What should I do next?"],
            "approval_mode": "full_auto",
            "status": "active",
            "sandbox_container_id": 7
        }),
    )
    .await;
    assert!(
        read_tool_names(&template_default)
            .iter()
            .any(|name| name == &tool_name),
        "template default agent should keep tool {tool_name}"
    );
    assert!(
        read_tool_names(&template_default)
            .iter()
            .any(|name| name == &skill_name),
        "template default agent should keep skill {skill_name}"
    );
    assert_eq!(
        read_declared_skill_names(&template_default),
        vec![skill_name.clone()]
    );

    update_default_agent(
        &context.app,
        Some(&admin_token),
        "default_sync_tools_user_b",
        json!({
            "name": "User Customized Default",
            "description": "custom-description",
            "system_prompt": "custom-system-prompt",
            "tool_names": [],
            "preset_questions": ["custom-question"],
            "approval_mode": "suggest",
            "status": "active",
            "sandbox_container_id": 3
        }),
    )
    .await;

    let safe_summary = sync_preset(&context.app, DEFAULT_AGENT_ID_ALIAS, "safe", None).await;
    let safe_created = safe_summary["data"]["summary"]["created_agents"]
        .as_u64()
        .expect("created_agents should be number");
    let safe_overridden = safe_summary["data"]["summary"]["overridden_agents"]
        .as_u64()
        .expect("overridden_agents should be number");
    assert!(
        safe_created >= 1,
        "expected at least 1 created agent in safe sync, got {safe_created}"
    );
    assert!(
        safe_overridden >= 1,
        "expected at least 1 overridden default agent in safe sync, got {safe_overridden}"
    );

    let user_a_after_safe = get_default_agent(
        &context.app,
        Some(&admin_token),
        "default_sync_tools_user_a",
    )
    .await;
    assert!(
        read_tool_names(&user_a_after_safe)
            .iter()
            .any(|name| name == &tool_name),
        "safe sync should apply tool {tool_name} for user A"
    );
    assert!(
        read_tool_names(&user_a_after_safe)
            .iter()
            .any(|name| name == &skill_name),
        "safe sync should apply skill {skill_name} for user A"
    );
    assert_eq!(
        read_declared_skill_names(&user_a_after_safe),
        vec![skill_name.clone()]
    );

    let user_b_after_safe = get_default_agent(
        &context.app,
        Some(&admin_token),
        "default_sync_tools_user_b",
    )
    .await;
    assert!(
        read_tool_names(&user_b_after_safe).is_empty(),
        "safe sync should preserve customized default-agent tools for user B"
    );
    assert!(
        read_declared_skill_names(&user_b_after_safe).is_empty(),
        "safe sync should preserve customized default-agent declared skills for user B"
    );

    let force_summary = sync_preset(&context.app, DEFAULT_AGENT_ID_ALIAS, "force", None).await;
    let force_updated = force_summary["data"]["summary"]["updated_agents"]
        .as_u64()
        .expect("updated_agents should be number");
    assert!(
        force_updated >= 1,
        "expected at least 1 updated agent in force sync, got {force_updated}"
    );

    let user_b_after_force = get_default_agent(
        &context.app,
        Some(&admin_token),
        "default_sync_tools_user_b",
    )
    .await;
    assert!(
        read_tool_names(&user_b_after_force)
            .iter()
            .any(|name| name == &tool_name),
        "force sync should apply tool {tool_name} for user B"
    );
    assert!(
        read_tool_names(&user_b_after_force)
            .iter()
            .any(|name| name == &skill_name),
        "force sync should apply skill {skill_name} for user B"
    );
    assert_eq!(
        read_declared_skill_names(&user_b_after_force),
        vec![skill_name.clone()],
        "force sync should apply declared skills for user B"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_default_agent_force_sync_can_disable_skill_creator() {
    let context =
        build_test_context_with_config("default_disable_skill_creator_user_a", |config| {
            config.tools.builtin.enabled = vec!["读取文件".to_string(), "写入文件".to_string()];
            config.skills.enabled = vec!["技能创建器".to_string()];
        })
        .await;
    context
        .state
        .user_store
        .ensure_default_admin()
        .expect("ensure default admin");
    let admin_token = context
        .state
        .user_store
        .create_session_token("admin")
        .expect("create admin token")
        .token;
    context
        .state
        .user_store
        .create_user(
            "default_disable_skill_creator_user_b",
            Some("default_disable_skill_creator_user_b@example.test".to_string()),
            "password-123",
            Some("A"),
            None,
            vec!["user".to_string()],
            "active",
            false,
        )
        .expect("create second user");

    let tool_name = "读取文件".to_string();
    let skill_name = "技能创建器".to_string();

    update_default_agent(
        &context.app,
        Some(&admin_token),
        "preset_template",
        json!({
            "name": "Template Default Agent",
            "description": "template-description",
            "system_prompt": "template-system-prompt",
            "tool_names": [tool_name.clone(), skill_name.clone()],
            "declared_tool_names": [tool_name.clone()],
            "declared_skill_names": [skill_name.clone()],
            "preset_questions": [],
            "approval_mode": "full_auto",
            "status": "active",
            "sandbox_container_id": 7
        }),
    )
    .await;
    sync_preset(&context.app, DEFAULT_AGENT_ID_ALIAS, "force", None).await;

    let user_b_with_skill = get_default_agent(
        &context.app,
        Some(&admin_token),
        "default_disable_skill_creator_user_b",
    )
    .await;
    assert!(
        read_tool_names(&user_b_with_skill)
            .iter()
            .any(|name| name == &skill_name),
        "precondition: force sync should first apply skill creator to default agent"
    );

    update_default_agent(
        &context.app,
        Some(&admin_token),
        "preset_template",
        json!({
            "name": "Template Default Agent",
            "description": "template-description",
            "system_prompt": "template-system-prompt",
            "tool_names": [tool_name.clone()],
            "declared_tool_names": [tool_name.clone()],
            "declared_skill_names": [],
            "preset_questions": [],
            "approval_mode": "full_auto",
            "status": "active",
            "sandbox_container_id": 7
        }),
    )
    .await;
    let updated_template =
        get_default_agent(&context.app, Some(&admin_token), "preset_template").await;
    assert!(
        !read_tool_names(&updated_template)
            .iter()
            .any(|name| name == &skill_name),
        "template default agent should no longer include skill creator after admin disables it"
    );
    assert!(
        read_declared_skill_names(&updated_template).is_empty(),
        "template default agent should clear declared skill creator after admin disables it"
    );
    sync_preset(&context.app, DEFAULT_AGENT_ID_ALIAS, "force", None).await;

    let user_b_after_disable = get_default_agent(
        &context.app,
        Some(&admin_token),
        "default_disable_skill_creator_user_b",
    )
    .await;
    assert!(
        !read_tool_names(&user_b_after_disable)
            .iter()
            .any(|name| name == &skill_name),
        "force sync should remove skill creator from default agent when template disables it"
    );
    assert!(
        read_declared_skill_names(&user_b_after_disable).is_empty(),
        "force sync should clear declared skill creator for default agent"
    );

    let user_b_after_reload = get_default_agent(
        &context.app,
        Some(&admin_token),
        "default_disable_skill_creator_user_b",
    )
    .await;
    assert!(
        !read_tool_names(&user_b_after_reload)
            .iter()
            .any(|name| name == &skill_name),
        "reading default agent again should not silently re-add skill creator"
    );
    assert!(
        read_declared_skill_names(&user_b_after_reload).is_empty(),
        "reading default agent again should keep declared skills cleared"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preset_agent_rename_does_not_spawn_duplicate_bootstrap_copy() {
    let context = build_test_context_with_config("preset_rename_user", |config| {
        config.user_agents.presets = vec![build_preset_config("preset_bootstrap_rename", None)];
    })
    .await;

    let agents_v1 = list_user_agents(&context.app, &context.token).await;
    assert_eq!(agents_v1.len(), 1);
    let agent_id = find_agent_id_by_name(&agents_v1, PRESET_NAME);

    let renamed_name = "Renamed Preset Agent";
    let (status, updated_payload) = send_json(
        &context.app,
        Some(&context.token),
        Method::PUT,
        &format!("/wunder/agents/{agent_id}"),
        Some(json!({ "name": renamed_name })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated_payload["data"]["name"], json!(renamed_name));

    let agents_v2 = list_user_agents(&context.app, &context.token).await;
    assert_eq!(agents_v2.len(), 1);
    assert_eq!(agents_v2[0]["name"], json!(renamed_name));
    assert!(
        agents_v2
            .iter()
            .all(|item| item["name"] != json!(PRESET_NAME)),
        "renamed preset agent should not respawn the original preset name"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preset_agent_delete_does_not_respawn_on_next_list_request() {
    let context = build_test_context_with_config("preset_delete_user", |config| {
        config.user_agents.presets = vec![build_preset_config("preset_bootstrap_delete", None)];
    })
    .await;

    let agents_v1 = list_user_agents(&context.app, &context.token).await;
    assert_eq!(agents_v1.len(), 1);
    let agent_id = find_agent_id_by_name(&agents_v1, PRESET_NAME);

    let (status, delete_payload) = send_json(
        &context.app,
        Some(&context.token),
        Method::DELETE,
        &format!("/wunder/agents/{agent_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(delete_payload["data"]["id"], json!(agent_id));

    let agents_v2 = list_user_agents(&context.app, &context.token).await;
    assert!(
        agents_v2.is_empty(),
        "deleted preset agent should stay deleted until an explicit preset sync recreates it"
    );
    let agents_v3 = list_user_agents(&context.app, &context.token).await;
    assert!(agents_v3.is_empty());
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preset_tool_sync_safe_and_force_respects_user_override() {
    let user_a_id = "preset_tool_sync_user_a";
    let context = build_test_context_with_config(user_a_id, |config| {
        config.tools.builtin.enabled = vec!["读取文件".to_string(), "写入文件".to_string()];
        config.user_agents.presets = vec![build_preset_config("preset_sync_tool_names", None)];
    })
    .await;

    let user_b = context
        .state
        .user_store
        .create_user(
            "preset_tool_sync_user_b",
            Some("preset_tool_sync_user_b@example.test".to_string()),
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

    let available_tools = list_available_tool_names(&context.app, user_a_id).await;
    assert!(
        available_tools.len() >= 2,
        "expected at least 2 available tools for tool sync regression"
    );
    let tool_a = available_tools[0].clone();
    let tool_b = available_tools[1].clone();

    let mut admin_items = list_admin_presets(&context.app).await;
    let preset_id = find_preset_item(&admin_items, "preset_sync_tool_names")["preset_id"]
        .as_str()
        .expect("preset id")
        .to_string();
    for item in &mut admin_items {
        if item["preset_id"] == json!(preset_id.as_str()) {
            item["tool_names"] = json!([tool_a.clone()]);
        }
    }
    let updated_v1 = update_admin_presets(&context.app, admin_items).await;
    assert_eq!(
        find_preset_item(&updated_v1, &preset_id)["tool_names"],
        json!([tool_a.clone()])
    );

    let sync_v1 = sync_preset(&context.app, &preset_id, "safe", None).await;
    let created_v1 = sync_v1["data"]["summary"]["created_agents"]
        .as_u64()
        .expect("created_agents should be number");
    assert!(
        created_v1 >= 2,
        "expected at least 2 created agents for tool sync baseline, got {created_v1}"
    );

    let user_a_v1 = list_user_agents(&context.app, &context.token).await;
    let user_b_v1 = list_user_agents(&context.app, &token_b).await;
    let user_b_agent_id = find_agent_id_by_name(&user_b_v1, PRESET_NAME);
    let user_a_tools_v1 = read_tool_names(find_agent_by_name(&user_a_v1, PRESET_NAME));
    let user_b_tools_v1 = read_tool_names(find_agent_by_name(&user_b_v1, PRESET_NAME));
    assert!(
        user_a_tools_v1.iter().any(|name| name == &tool_a),
        "user A should contain preset tool {tool_a} after initial sync"
    );
    assert!(
        user_b_tools_v1.iter().any(|name| name == &tool_a),
        "user B should contain preset tool {tool_a} after initial sync"
    );

    let (status, updated_user_b) = send_json(
        &context.app,
        Some(&token_b),
        Method::PUT,
        &format!("/wunder/agents/{user_b_agent_id}"),
        Some(json!({ "tool_names": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        read_tool_names(&updated_user_b["data"])
            .iter()
            .all(|name| name != &tool_a && name != &tool_b),
        "user B custom tool override should remove preset tools before sync"
    );

    let mut admin_items_v2 = list_admin_presets(&context.app).await;
    for item in &mut admin_items_v2 {
        if item["preset_id"] == json!(preset_id.as_str()) {
            item["tool_names"] = json!([tool_b.clone()]);
        }
    }
    let updated_v2 = update_admin_presets(&context.app, admin_items_v2).await;
    assert_eq!(
        find_preset_item(&updated_v2, &preset_id)["tool_names"],
        json!([tool_b.clone()])
    );

    let safe_summary = sync_preset(&context.app, &preset_id, "safe", None).await;
    let safe_updated = safe_summary["data"]["summary"]["updated_agents"]
        .as_u64()
        .expect("updated_agents should be number");
    let safe_overridden = safe_summary["data"]["summary"]["overridden_agents"]
        .as_u64()
        .expect("overridden_agents should be number");
    assert!(
        safe_updated >= 1,
        "expected at least 1 updated agent in safe tool sync, got {safe_updated}"
    );
    assert!(
        safe_overridden >= 1,
        "expected at least 1 overridden agent in safe tool sync, got {safe_overridden}"
    );

    let user_a_v2 = list_user_agents(&context.app, &context.token).await;
    let user_b_v2 = list_user_agents(&context.app, &token_b).await;
    let user_a_tools_v2 = read_tool_names(find_agent_by_name(&user_a_v2, PRESET_NAME));
    let user_b_tools_v2 = read_tool_names(find_agent_by_name(&user_b_v2, PRESET_NAME));
    assert!(
        user_a_tools_v2.iter().any(|name| name == &tool_b),
        "safe sync should update non-customized user A to tool {tool_b}"
    );
    assert!(
        user_b_tools_v2.iter().all(|name| name != &tool_b),
        "safe sync should not override customized user B tools"
    );

    let force_summary = sync_preset(&context.app, &preset_id, "force", None).await;
    let force_updated = force_summary["data"]["summary"]["updated_agents"]
        .as_u64()
        .expect("updated_agents should be number");
    assert!(
        force_updated >= 1,
        "expected at least 1 updated agent in force tool sync, got {force_updated}"
    );

    let user_b_v3 = list_user_agents(&context.app, &token_b).await;
    let user_b_tools_v3 = read_tool_names(find_agent_by_name(&user_b_v3, PRESET_NAME));
    assert!(
        user_b_tools_v3.iter().any(|name| name == &tool_b),
        "force sync should apply preset tool {tool_b} for customized user B"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preset_force_sync_repairs_declared_skills_for_users() {
    let user_a_id = "preset_skill_sync_user_a";
    let context = build_test_context_with_config(user_a_id, |config| {
        config.tools.builtin.enabled = vec!["读取文件".to_string(), "写入文件".to_string()];
        config.skills.enabled = vec!["技能创建器".to_string()];
        config.user_agents.presets = vec![build_preset_config("preset_sync_skill_names", None)];
    })
    .await;

    let user_b = context
        .state
        .user_store
        .create_user(
            "preset_skill_sync_user_b",
            Some("preset_skill_sync_user_b@example.test".to_string()),
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

    let available_tools = list_available_tool_names(&context.app, user_a_id).await;
    let tool_name = "读取文件".to_string();
    let skill_name = "技能创建器".to_string();
    assert!(
        available_tools.iter().any(|name| name == &tool_name),
        "expected available tool list to contain {tool_name}"
    );
    assert!(
        available_tools.iter().any(|name| name == &skill_name),
        "expected available tool list to contain {skill_name}"
    );

    let mut admin_items = list_admin_presets(&context.app).await;
    let preset_id = find_preset_item(&admin_items, "preset_sync_skill_names")["preset_id"]
        .as_str()
        .expect("preset id")
        .to_string();
    for item in &mut admin_items {
        if item["preset_id"] == json!(preset_id.as_str()) {
            item["tool_names"] = json!([tool_name.clone()]);
            item["declared_tool_names"] = json!([tool_name.clone()]);
            item["declared_skill_names"] = json!([skill_name.clone()]);
        }
    }
    let updated_items = update_admin_presets(&context.app, admin_items).await;
    let normalized_preset = find_preset_item(&updated_items, &preset_id);
    let normalized_tool_names = read_tool_names(normalized_preset);
    assert!(
        normalized_tool_names.iter().any(|name| name == &tool_name),
        "normalized preset should keep tool {tool_name}"
    );
    assert!(
        normalized_tool_names.iter().any(|name| name == &skill_name),
        "normalized preset should merge declared skill {skill_name} into selected tools"
    );
    assert_eq!(
        read_declared_skill_names(normalized_preset),
        vec![skill_name.clone()]
    );

    let force_summary = sync_preset(&context.app, &preset_id, "force", None).await;
    let created_agents = force_summary["data"]["summary"]["created_agents"]
        .as_u64()
        .expect("created_agents should be number");
    assert!(
        created_agents >= 2,
        "expected at least 2 created agents in force skill sync, got {created_agents}"
    );

    let user_a_agents = list_user_agents(&context.app, &context.token).await;
    let user_b_agents = list_user_agents(&context.app, &token_b).await;
    for (label, agents) in [("user A", user_a_agents), ("user B", user_b_agents)] {
        let synced_agent = find_agent_by_name(&agents, PRESET_NAME);
        let synced_tool_names = read_tool_names(synced_agent);
        assert!(
            synced_tool_names.iter().any(|name| name == &tool_name),
            "{label} should contain preset tool {tool_name} after force sync"
        );
        assert!(
            synced_tool_names.iter().any(|name| name == &skill_name),
            "{label} should contain preset skill {skill_name} after force sync"
        );
        assert_eq!(
            read_declared_skill_names(synced_agent),
            vec![skill_name.clone()],
            "{label} should keep preset declared skills after force sync"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preset_force_sync_updates_inner_visible_worker_card_skills() {
    let user_id = "preset_skill_inner_visible_user";
    let context = build_test_context_with_config(user_id, |config| {
        config.tools.builtin.enabled = vec!["读取文件".to_string(), "写入文件".to_string()];
        config.skills.enabled = vec!["技能创建器".to_string()];
        config.user_agents.presets = vec![build_preset_config("preset_skill_inner_visible", None)];
    })
    .await;

    let available_tools = list_available_tool_names(&context.app, user_id).await;
    let tool_name = "读取文件".to_string();
    let skill_name = "技能创建器".to_string();
    assert!(
        available_tools.iter().any(|name| name == &tool_name),
        "expected available tool list to contain {tool_name}"
    );
    assert!(
        available_tools.iter().any(|name| name == &skill_name),
        "expected available tool list to contain {skill_name}"
    );

    let mut admin_items = list_admin_presets(&context.app).await;
    let preset_id = find_preset_item(&admin_items, "preset_skill_inner_visible")["preset_id"]
        .as_str()
        .expect("preset id")
        .to_string();
    for item in &mut admin_items {
        if item["preset_id"] == json!(preset_id.as_str()) {
            item["tool_names"] = json!([tool_name.clone(), skill_name.clone()]);
            item["declared_tool_names"] = json!([tool_name.clone()]);
            item["declared_skill_names"] = json!([skill_name.clone()]);
        }
    }
    let updated_items = update_admin_presets(&context.app, admin_items).await;
    let normalized_preset = find_preset_item(&updated_items, &preset_id);
    assert_eq!(
        read_declared_skill_names(normalized_preset),
        vec![skill_name.clone()],
        "admin preset save should preserve declared skills when tool_names includes the same skill"
    );

    let sync_result = sync_preset(&context.app, &preset_id, "force", None).await;
    let created_agents = sync_result["data"]["summary"]["created_agents"]
        .as_u64()
        .expect("created_agents should be number");
    assert!(
        created_agents >= 1,
        "expected at least 1 created preset agent after force sync, got {created_agents}"
    );

    let user_agents = list_user_agents(&context.app, &context.token).await;
    let agent_id = find_agent_id_by_name(&user_agents, PRESET_NAME);
    let synced_agent = find_agent_by_name(&user_agents, PRESET_NAME);
    assert_eq!(
        read_declared_skill_names(synced_agent),
        vec![skill_name.clone()],
        "preset force sync should persist declared skills before inner-visible mirroring"
    );
    let search_root = context._temp_dir.path().join("workspaces");
    let worker_card = find_worker_card_document_by_agent_id(&search_root, &agent_id)
        .expect("preset sync should refresh inner-visible worker card");

    assert_eq!(
        worker_card["abilities"]["tool_names"],
        json!([tool_name.clone()]),
        "worker card should keep declared tools after preset force sync"
    );
    assert_eq!(
        worker_card["abilities"]["skills"],
        json!([skill_name.clone()]),
        "worker card should keep declared skills after preset force sync"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preset_force_sync_can_disable_skill_creator_without_reinjection() {
    let user_id = "preset_disable_skill_creator_user";
    let context = build_test_context_with_config(user_id, |config| {
        config.tools.builtin.enabled = vec!["读取文件".to_string(), "写入文件".to_string()];
        config.skills.enabled = vec!["技能创建器".to_string()];
        config.user_agents.presets =
            vec![build_preset_config("preset_disable_skill_creator", None)];
    })
    .await;

    let tool_name = "读取文件".to_string();
    let skill_name = "技能创建器".to_string();

    let mut admin_items = list_admin_presets(&context.app).await;
    let preset_id = find_preset_item(&admin_items, "preset_disable_skill_creator")["preset_id"]
        .as_str()
        .expect("preset id")
        .to_string();
    for item in &mut admin_items {
        if item["preset_id"] == json!(preset_id.as_str()) {
            item["tool_names"] = json!([tool_name.clone(), skill_name.clone()]);
            item["declared_tool_names"] = json!([tool_name.clone()]);
            item["declared_skill_names"] = json!([skill_name.clone()]);
        }
    }
    update_admin_presets(&context.app, admin_items).await;
    sync_preset(&context.app, &preset_id, "force", None).await;

    let agents_with_skill = list_user_agents(&context.app, &context.token).await;
    let agent_with_skill = find_agent_by_name(&agents_with_skill, PRESET_NAME);
    assert!(
        read_tool_names(agent_with_skill)
            .iter()
            .any(|name| name == &skill_name),
        "precondition: force sync should first apply skill creator"
    );

    let mut admin_items = list_admin_presets(&context.app).await;
    for item in &mut admin_items {
        if item["preset_id"] == json!(preset_id.as_str()) {
            item["tool_names"] = json!([tool_name.clone()]);
            item["declared_tool_names"] = json!([tool_name.clone()]);
            item["declared_skill_names"] = json!([]);
        }
    }
    let updated_items = update_admin_presets(&context.app, admin_items).await;
    let normalized_preset = find_preset_item(&updated_items, &preset_id);
    assert!(
        !read_tool_names(normalized_preset)
            .iter()
            .any(|name| name == &skill_name),
        "updated preset should no longer include skill creator"
    );
    assert!(
        read_declared_skill_names(normalized_preset).is_empty(),
        "updated preset should no longer declare skill creator"
    );

    sync_preset(&context.app, &preset_id, "force", None).await;

    let agents_after_force = list_user_agents(&context.app, &context.token).await;
    let synced_agent = find_agent_by_name(&agents_after_force, PRESET_NAME);
    let agent_id = find_agent_id_by_name(&agents_after_force, PRESET_NAME);
    assert!(
        !read_tool_names(synced_agent)
            .iter()
            .any(|name| name == &skill_name),
        "force sync should remove skill creator when preset no longer enables it"
    );
    assert!(
        read_declared_skill_names(synced_agent).is_empty(),
        "force sync should clear declared skill creator when preset no longer enables it"
    );

    let agents_after_reload = list_user_agents(&context.app, &context.token).await;
    let reloaded_agent = find_agent_by_name(&agents_after_reload, PRESET_NAME);
    assert!(
        !read_tool_names(reloaded_agent)
            .iter()
            .any(|name| name == &skill_name),
        "listing agents again should not silently re-add skill creator"
    );
    assert!(
        read_declared_skill_names(reloaded_agent).is_empty(),
        "listing agents again should keep declared skills cleared"
    );

    let (detail_status, detail_payload) = send_json(
        &context.app,
        Some(&context.token),
        Method::GET,
        &format!("/wunder/agents/{agent_id}"),
        None,
    )
    .await;
    assert_eq!(detail_status, StatusCode::OK);
    let detailed_agent = detail_payload["data"].clone();
    assert!(
        !read_tool_names(&detailed_agent)
            .iter()
            .any(|name| name == &skill_name),
        "agent detail should not silently re-add skill creator"
    );
    assert!(
        read_declared_skill_names(&detailed_agent).is_empty(),
        "agent detail should keep declared skills cleared"
    );

    let search_root = context._temp_dir.path().join("workspaces");
    let worker_card = find_worker_card_document_by_agent_id(&search_root, &agent_id)
        .expect("preset sync should keep inner-visible worker card available");
    assert_eq!(
        worker_card["abilities"]["skills"],
        json!([]),
        "inner-visible worker card should keep skill creator disabled"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preset_duplicate_bound_agents_are_compacted_on_list() {
    let context = build_test_context_with_config("preset_duplicate_compact_user", |config| {
        config.user_agents.presets = vec![build_preset_config("preset_duplicate_compact", None)];
    })
    .await;

    let agents_v1 = list_user_agents(&context.app, &context.token).await;
    assert_eq!(agents_v1.len(), 1);
    let base_agent_id = agents_v1[0]["id"]
        .as_str()
        .expect("base agent id")
        .to_string();

    let mut stored_agents = context
        .state
        .user_store
        .list_user_agents("preset_duplicate_compact_user")
        .expect("list user agents from store");
    assert_eq!(stored_agents.len(), 1);
    let mut duplicate = stored_agents.pop().expect("base record");
    duplicate.agent_id = "agent_duplicate_compacted_newest".to_string();
    duplicate.description = "newer duplicate".to_string();
    duplicate.updated_at += 300.0;
    context
        .state
        .user_store
        .upsert_user_agent(&duplicate)
        .expect("insert duplicate bound agent");

    let duplicated = context
        .state
        .user_store
        .list_user_agents("preset_duplicate_compact_user")
        .expect("list duplicated agents");
    assert_eq!(
        duplicated.len(),
        2,
        "test precondition: two duplicate agents"
    );

    let agents_v2 = list_user_agents(&context.app, &context.token).await;
    assert_eq!(
        agents_v2.len(),
        1,
        "duplicate bound agents should be compacted"
    );
    assert_ne!(
        agents_v2[0]["id"],
        json!(base_agent_id),
        "newest duplicate should be retained after compaction"
    );
    assert_eq!(agents_v2[0]["description"], json!("newer duplicate"));

    let stored_after = context
        .state
        .user_store
        .list_user_agents("preset_duplicate_compact_user")
        .expect("list agents after compaction");
    assert_eq!(stored_after.len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_preset_sync_ignores_conflicting_template_agent_state() {
    let user_id = "preset_config_authority_user";
    let context = build_test_context_with_config(user_id, |config| {
        config.tools.builtin.enabled = vec!["读取文件".to_string(), "写入文件".to_string()];
        config.user_agents.presets = vec![build_preset_config("preset_config_authority", None)];
    })
    .await;

    let available_tools = list_available_tool_names(&context.app, user_id).await;
    assert!(
        !available_tools.is_empty(),
        "expected at least 1 available tool for preset config authority regression"
    );
    let tool_a = available_tools[0].clone();
    let tool_b = "template_conflict_tool".to_string();

    let mut admin_items = list_admin_presets(&context.app).await;
    let preset_id = find_preset_item(&admin_items, "preset_config_authority")["preset_id"]
        .as_str()
        .expect("preset id")
        .to_string();
    for item in &mut admin_items {
        if item["preset_id"] == json!(preset_id.as_str()) {
            item["tool_names"] = json!([tool_a.clone()]);
            item["declared_tool_names"] = json!([]);
            item["declared_skill_names"] = json!([]);
        }
    }
    let updated_items = update_admin_presets(&context.app, admin_items).await;
    assert_eq!(
        find_preset_item(&updated_items, &preset_id)["tool_names"],
        json!([tool_a.clone()])
    );

    context
        .state
        .user_store
        .upsert_user_agent(&wunder_server::storage::UserAgentRecord {
            agent_id: "agent_conflicting_template".to_string(),
            user_id: "preset_template".to_string(),
            hive_id: "default".to_string(),
            name: PRESET_NAME.to_string(),
            description: "template conflict".to_string(),
            system_prompt: "template conflict".to_string(),
            model_name: None,
            ability_items: Vec::new(),
            tool_names: vec![tool_b.clone()],
            declared_tool_names: vec![tool_b.clone()],
            declared_skill_names: Vec::new(),
            preset_questions: Vec::new(),
            access_level: "A".to_string(),
            approval_mode: "full_auto".to_string(),
            is_shared: false,
            status: "active".to_string(),
            icon: None,
            sandbox_container_id: 1,
            created_at: 1.0,
            updated_at: 2.0,
            preset_binding: None,
        })
        .expect("insert conflicting template agent");

    let admin_after_conflict = list_admin_presets(&context.app).await;
    assert_eq!(
        find_preset_item(&admin_after_conflict, &preset_id)["tool_names"],
        json!([tool_a.clone()]),
        "ordinary preset config should remain authoritative even if preset_template has a conflicting same-name agent"
    );

    let sync_result = sync_preset(&context.app, &preset_id, "force", None).await;
    let created_agents = sync_result["data"]["summary"]["created_agents"]
        .as_u64()
        .expect("created_agents should be number");
    assert!(
        created_agents >= 1,
        "expected at least 1 created agent after force sync, got {created_agents}"
    );

    let user_agents = list_user_agents(&context.app, &context.token).await;
    let synced_agent = find_agent_by_name(&user_agents, PRESET_NAME);
    let synced_tool_names = read_tool_names(synced_agent);
    assert!(
        synced_tool_names.iter().any(|name| name == &tool_a),
        "synced preset agent should use configured preset tool {tool_a}"
    );
    assert!(
        synced_tool_names.iter().all(|name| name != &tool_b),
        "conflicting preset_template tool {tool_b} must not leak into synced users"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preset_bound_agent_tool_update_without_declared_arrays_drops_stale_dependencies() {
    let user_id = "preset_update_declared_user";
    let context = build_test_context_with_config(user_id, |config| {
        config.tools.builtin.enabled = vec!["读取文件".to_string(), "写入文件".to_string()];
        config.user_agents.presets = vec![build_preset_config("preset_update_declared", None)];
    })
    .await;

    let available_tools = list_available_tool_names(&context.app, user_id).await;
    assert!(
        !available_tools.is_empty(),
        "expected at least 1 available tool for declared dependency regression"
    );
    let tool_a = available_tools[0].clone();

    let mut admin_items = list_admin_presets(&context.app).await;
    let preset_id = find_preset_item(&admin_items, "preset_update_declared")["preset_id"]
        .as_str()
        .expect("preset id")
        .to_string();
    for item in &mut admin_items {
        if item["preset_id"] == json!(preset_id.as_str()) {
            item["tool_names"] = json!([tool_a.clone()]);
        }
    }
    let updated_items = update_admin_presets(&context.app, admin_items).await;
    assert_eq!(
        find_preset_item(&updated_items, &preset_id)["tool_names"],
        json!([tool_a.clone()])
    );

    let sync_result = sync_preset(&context.app, &preset_id, "force", None).await;
    let created_agents = sync_result["data"]["summary"]["created_agents"]
        .as_u64()
        .expect("created_agents should be number");
    assert!(
        created_agents >= 1,
        "expected at least 1 created agent after force sync, got {created_agents}"
    );

    let user_agents = list_user_agents(&context.app, &context.token).await;
    let agent_id = find_agent_id_by_name(&user_agents, PRESET_NAME);
    let synced_agent = find_agent_by_name(&user_agents, PRESET_NAME);
    assert!(
        !read_declared_tool_names(synced_agent).is_empty(),
        "preset sync should seed declared tool names before the direct user override"
    );

    let (status, updated_payload) = send_json(
        &context.app,
        Some(&context.token),
        Method::PUT,
        &format!("/wunder/agents/{agent_id}"),
        Some(json!({ "tool_names": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(read_tool_names(&updated_payload["data"]).is_empty());
    assert!(
        read_declared_tool_names(&updated_payload["data"]).is_empty(),
        "tool-only updates without declared arrays should not keep stale preset dependency names"
    );
    assert_eq!(
        updated_payload["data"]["preset_binding"]["preset_id"],
        json!(preset_id),
        "preset binding should stay visible so the frontend can suppress worker-card mismatch warnings for preset-bound agents"
    );
    assert!(
        updated_payload["data"]["preset_binding"]
            .get("last_applied")
            .is_none(),
        "frontend only needs preset identity metadata, not the full sync snapshot payload"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_preset_save_roundtrip_is_worker_card_stable() {
    let user_id = "preset_admin_roundtrip_user";
    let context = build_test_context_with_config(user_id, |config| {
        config.tools.builtin.enabled = vec!["读取文件".to_string(), "写入文件".to_string()];
        config.user_agents.presets = vec![build_preset_config("preset_admin_roundtrip", None)];
    })
    .await;

    let available_tools = list_available_tool_names(&context.app, user_id).await;
    assert!(
        !available_tools.is_empty(),
        "expected at least 1 available tool for admin roundtrip regression"
    );
    let tool_a = available_tools[0].clone();

    let mut admin_items = list_admin_presets(&context.app).await;
    let preset_id = find_preset_item(&admin_items, "preset_admin_roundtrip")["preset_id"]
        .as_str()
        .expect("preset id")
        .to_string();
    for item in &mut admin_items {
        if item["preset_id"] == json!(preset_id.as_str()) {
            item["tool_names"] = json!([tool_a.clone()]);
            item["declared_tool_names"] = json!([]);
            item["declared_skill_names"] = json!([]);
        }
    }

    let saved_once = update_admin_presets(&context.app, admin_items).await;
    let preset_once = find_preset_item(&saved_once, &preset_id);
    assert_eq!(preset_once["tool_names"], json!([tool_a.clone()]));
    assert_eq!(
        read_declared_tool_names(preset_once),
        vec![tool_a.clone()],
        "admin save should canonicalize declared tool names using the same worker-card rules as sync"
    );
    assert!(
        read_declared_skill_names(preset_once).is_empty(),
        "tool-only preset should not emit synthetic declared skills"
    );
    let revision_once = preset_once["revision"]
        .as_u64()
        .expect("revision after first save");

    let saved_twice = update_admin_presets(&context.app, saved_once.clone()).await;
    let preset_twice = find_preset_item(&saved_twice, &preset_id);
    assert_eq!(
        preset_twice["revision"]
            .as_u64()
            .expect("revision after second save"),
        revision_once,
        "saving the already-normalized preset payload should be stable and not bump revision again"
    );
    assert_eq!(preset_twice["tool_names"], preset_once["tool_names"]);
    assert_eq!(
        preset_twice["declared_tool_names"],
        preset_once["declared_tool_names"]
    );
    assert_eq!(
        preset_twice["declared_skill_names"],
        preset_once["declared_skill_names"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_preset_asset_store_uses_configured_worker_card_directory() {
    let context = build_test_context_with_temp_config("preset_asset_store_user", |config, root| {
        config.user_agents.worker_cards_root = root
            .join("preset_worker_cards")
            .to_string_lossy()
            .to_string();
    })
    .await;
    let preset_root = context._temp_dir.path().join("preset_worker_cards");

    let saved_once = update_admin_presets(
        &context.app,
        vec![json!({
            "name": "Asset Preset",
            "description": "asset-backed preset",
            "system_prompt": "asset prompt",
            "model_name": Value::Null,
            "icon_name": "spark",
            "icon_color": "#94a3b8",
            "sandbox_container_id": 2,
            "tool_names": [],
            "declared_tool_names": [],
            "declared_skill_names": [],
            "preset_questions": [],
            "approval_mode": "full_auto",
            "status": "active"
        })],
    )
    .await;
    let preset_once = saved_once
        .iter()
        .find(|item| item["name"] == json!("Asset Preset"))
        .expect("asset preset should be present after save");
    let preset_id = preset_once["preset_id"]
        .as_str()
        .expect("asset preset id")
        .to_string();
    let first_path = preset_root.join("Asset Preset.worker-card.json");
    assert!(
        first_path.exists(),
        "preset worker card should be saved to configured directory"
    );

    let listed = list_admin_presets(&context.app).await;
    assert!(
        listed
            .iter()
            .any(|item| item["preset_id"] == json!(preset_id.as_str())),
        "preset list should read back from the configured worker-card directory"
    );

    let mut renamed_payload = saved_once.clone();
    for item in &mut renamed_payload {
        if item["preset_id"] == json!(preset_id.as_str()) {
            item["name"] = json!("Asset Preset Renamed");
        }
    }
    update_admin_presets(&context.app, renamed_payload).await;
    let second_path = preset_root.join("Asset Preset Renamed.worker-card.json");
    assert!(
        second_path.exists(),
        "renamed preset should use canonical worker-card file name"
    );
    assert!(
        !first_path.exists(),
        "old worker-card file should be removed after canonical rename"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_preset_worker_card_export_hides_internal_ids_in_filename() {
    let context = build_test_context_with_config("preset_export_user", |config| {
        config.user_agents.presets = vec![build_preset_config("preset_export_demo", None)];
    })
    .await;

    let preset_export = export_admin_preset_worker_card(&context.app, "preset_export_demo").await;
    assert_eq!(
        preset_export["filename"],
        json!("Preset Auto Model.worker-card.json")
    );
    assert_eq!(
        preset_export["document"]["metadata"]["agent_id"],
        json!("preset_export_demo")
    );
    assert_eq!(preset_export["document"]["preset"]["revision"], json!(1));
    assert_eq!(
        preset_export["document"]["preset"]["status"],
        json!("active")
    );
    assert_eq!(
        preset_export["document"]["metadata"]["name"],
        json!(PRESET_NAME)
    );
    assert!(
        preset_export["document"].get("extensions").is_none(),
        "preset worker card export should not emit extension noise"
    );

    let default_export =
        export_admin_preset_worker_card(&context.app, DEFAULT_AGENT_ID_ALIAS).await;
    let default_filename = default_export["filename"]
        .as_str()
        .expect("default filename");
    assert!(
        default_filename.ends_with(".worker-card.json")
            && !default_filename.contains("__default__"),
        "default preset export should hide the internal stable-id suffix"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_default_preset_payload_exposes_canonical_declared_names() {
    let user_id = "preset_admin_default_declared_user";
    let context = build_test_context_with_config(user_id, |config| {
        config.tools.builtin.enabled = vec!["读取文件".to_string(), "写入文件".to_string()];
    })
    .await;
    context
        .state
        .user_store
        .ensure_default_admin()
        .expect("ensure default admin");
    let admin_token = context
        .state
        .user_store
        .create_session_token("admin")
        .expect("create admin token")
        .token;

    let available_tools = list_available_tool_names(&context.app, user_id).await;
    assert!(
        !available_tools.is_empty(),
        "expected at least 1 available tool for default preset declared regression"
    );
    let tool_a = available_tools[0].clone();

    update_default_agent(
        &context.app,
        Some(&admin_token),
        "preset_template",
        json!({
            "name": "Template Default Agent",
            "description": "template-description",
            "system_prompt": "template-system-prompt",
            "tool_names": [tool_a.clone()],
            "preset_questions": ["What should I do next?"],
            "approval_mode": "full_auto",
            "status": "active",
            "sandbox_container_id": 7
        }),
    )
    .await;

    let items = list_admin_presets(&context.app).await;
    let default_item = find_preset_item(&items, DEFAULT_AGENT_ID_ALIAS);
    assert_eq!(default_item["tool_names"], json!([tool_a.clone()]));
    assert_eq!(
        read_declared_tool_names(default_item),
        vec![tool_a],
        "default preset card in admin UI should expose canonical declared tool names instead of an empty placeholder"
    );
}
