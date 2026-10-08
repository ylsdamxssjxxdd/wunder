//! 单智能体（A2）与契约端点（A3）的端到端回归。
//!
//! 覆盖：单实例收敛与归档、绑定唯一性、bind/unbind/rebind 计数、sync 的
//! safe/force/dry_run 语义、`/wunder/user/agent` 形状与 customizable 强制、
//! 管理侧列表的绑定字段。
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
    config::{Config, LlmModelConfig, PresetCustomizable, UserAgentPresetConfig},
    config_store::ConfigStore,
    state::{AppState, AppStateInitOptions},
    storage::UserAgentRecord,
};

const PRESET_A: &str = "preset_contract_a";
const PRESET_B: &str = "preset_contract_b";
const PRESET_A_NAME: &str = "Contract Preset A";
const PRESET_B_NAME: &str = "Contract Preset B";

struct TestContext {
    state: Arc<AppState>,
    app: Router,
    admin_token: String,
    _temp_dir: TempDir,
}

fn build_preset(
    preset_id: &str,
    name: &str,
    model_name: Option<&str>,
    customizable: PresetCustomizable,
) -> UserAgentPresetConfig {
    UserAgentPresetConfig {
        preset_id: preset_id.to_string(),
        revision: 1,
        name: name.to_string(),
        description: format!("{name} description"),
        system_prompt: format!("{name} system prompt"),
        preview_skill: false,
        model_name: model_name.map(str::to_string),
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
        customizable,
    }
}

fn build_llm_model() -> LlmModelConfig {
    LlmModelConfig {
        enable: Some(true),
        provider: Some("openai".to_string()),
        api_mode: None,
        base_url: Some("http://127.0.0.1:18099/v1".to_string()),
        api_key: Some("test-key".to_string()),
        model: Some("provider-model".to_string()),
        temperature: Some(0.0),
        timeout_s: Some(15),
        max_rounds: Some(4),
        max_context: Some(16_384),
        max_output: Some(256),
        stream: Some(false),
        tool_call_mode: Some("tool_call".to_string()),
        model_type: Some("llm".to_string()),
        ..Default::default()
    }
}

async fn build_context<F>(configure: F) -> TestContext
where
    F: FnOnce(&mut Config, &Path),
{
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = Config::default();
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = temp_dir
        .path()
        .join("cloud-contract.db")
        .to_string_lossy()
        .to_string();
    config.workspace.root = temp_dir
        .path()
        .join("workspaces")
        .to_string_lossy()
        .to_string();
    config.skills.enabled.clear();
    config.llm.default = "model-default".to_string();
    config.llm.models.clear();
    config
        .llm
        .models
        .insert("model-default".to_string(), build_llm_model());
    for model in ["model-a", "model-b", "model-c"] {
        config
            .llm
            .models
            .insert(model.to_string(), build_llm_model());
    }
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
    state
        .user_store
        .ensure_default_admin()
        .expect("ensure default admin");
    let admin_token = state
        .user_store
        .create_session_token("admin")
        .expect("create admin token")
        .token;
    let app = build_router(state.clone());
    TestContext {
        state,
        app,
        admin_token,
        _temp_dir: temp_dir,
    }
}

fn create_user_token(context: &TestContext, username: &str) -> String {
    let user = context
        .state
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
    context
        .state
        .user_store
        .create_session_token(&user.user_id)
        .expect("create token")
        .token
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

fn store_agent(
    context: &TestContext,
    user_id: &str,
    agent_id: &str,
    name: &str,
    updated_at: f64,
    preset_id: Option<&str>,
) {
    let binding = preset_id.map(|preset_id| wunder_server::storage::UserAgentPresetBinding {
        preset_id: preset_id.to_string(),
        preset_revision: 1,
        last_applied: wunder_server::storage::UserAgentPresetSnapshot {
            name: name.to_string(),
            description: String::new(),
            system_prompt: String::new(),
            preview_skill: false,
            model_name: None,
            ability_items: Vec::new(),
            tool_names: Vec::new(),
            declared_tool_names: Vec::new(),
            declared_skill_names: Vec::new(),
            visible_unit_ids: Vec::new(),
            preset_questions: Vec::new(),
            approval_mode: "full_auto".to_string(),
            status: "active".to_string(),
            icon: None,
            sandbox_container_id: 1,
        },
    });
    context
        .state
        .user_store
        .upsert_user_agent(&UserAgentRecord {
            agent_id: agent_id.to_string(),
            user_id: user_id.to_string(),
            name: name.to_string(),
            description: String::new(),
            system_prompt: String::new(),
            preview_skill: false,
            model_name: None,
            ability_items: Vec::new(),
            tool_names: Vec::new(),
            declared_tool_names: Vec::new(),
            declared_skill_names: Vec::new(),
            visible_unit_ids: Vec::new(),
            preset_questions: Vec::new(),
            access_level: "A".to_string(),
            approval_mode: "full_auto".to_string(),
            is_shared: false,
            status: "active".to_string(),
            icon: None,
            sandbox_container_id: 1,
            created_at: updated_at,
            updated_at,
            preset_binding: binding,
            silent: false,
            prefer_mother: false,
        })
        .expect("seed agent record");
}

fn user_id_of(context: &TestContext, username: &str) -> String {
    context
        .state
        .user_store
        .get_user_by_username(username)
        .expect("load user")
        .expect("user exists")
        .user_id
}

async fn list_admin_presets(context: &TestContext) -> Vec<Value> {
    let (status, payload) = send_json(
        &context.app,
        Some(&context.admin_token),
        Method::GET,
        "/wunder/admin/preset_agents",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{payload}");
    payload["data"]["items"]
        .as_array()
        .expect("preset items")
        .clone()
}

async fn update_admin_presets(context: &TestContext, items: Vec<Value>) -> Vec<Value> {
    let (status, payload) = send_json(
        &context.app,
        Some(&context.admin_token),
        Method::POST,
        "/wunder/admin/preset_agents",
        Some(json!({ "items": items })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{payload}");
    payload["data"]["items"]
        .as_array()
        .expect("preset items")
        .clone()
}

fn preset_item<'a>(items: &'a [Value], preset_id: &str) -> &'a Value {
    items
        .iter()
        .find(|item| item["preset_id"] == json!(preset_id))
        .expect("preset item")
}

async fn bind_users(
    context: &TestContext,
    preset_id: &str,
    user_ids: &[String],
    action: &str,
    new_preset_id: Option<&str>,
) -> (StatusCode, Value) {
    let mut body = json!({
        "preset_id": preset_id,
        "user_ids": user_ids,
        "action": action,
    });
    if let Some(new_preset_id) = new_preset_id {
        body["new_preset_id"] = json!(new_preset_id);
    }
    send_json(
        &context.app,
        Some(&context.admin_token),
        Method::POST,
        "/wunder/admin/preset_agents/bindings",
        Some(body),
    )
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn single_instance_convergence_archives_legacy_instances() {
    let context = build_context(|config, _| {
        config.user_agents.presets = vec![
            build_preset(PRESET_A, PRESET_A_NAME, None, PresetCustomizable::default()),
            build_preset(PRESET_B, PRESET_B_NAME, None, PresetCustomizable::default()),
        ];
    })
    .await;
    let token = create_user_token(&context, "converge_user");
    let user_id = user_id_of(&context, "converge_user");

    // Legacy state: two instances bound to A (the newer one wins), one bound to
    // B, and one unbound custom agent.
    store_agent(
        &context,
        &user_id,
        "agent_legacy_a_old",
        "Legacy A old",
        100.0,
        Some(PRESET_A),
    );
    store_agent(
        &context,
        &user_id,
        "agent_legacy_a_new",
        "Legacy A new",
        200.0,
        Some(PRESET_A),
    );
    store_agent(
        &context,
        &user_id,
        "agent_legacy_b",
        "Legacy B",
        150.0,
        Some(PRESET_B),
    );
    store_agent(
        &context,
        &user_id,
        "agent_custom",
        "Custom agent",
        120.0,
        None,
    );
    // A thread of an archived instance must survive convergence.
    context
        .state
        .user_store
        .upsert_chat_session(&wunder_server::storage::ChatSessionRecord {
            session_id: "session_of_archived_agent".to_string(),
            user_id: user_id.clone(),
            title: "Legacy thread".to_string(),
            status: "active".to_string(),
            created_at: 100.0,
            updated_at: 100.0,
            last_message_at: 100.0,
            agent_id: Some("agent_legacy_a_old".to_string()),
            workspace_id: None,
            tool_overrides: Vec::new(),
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        })
        .expect("seed legacy thread");

    let (status, payload) = send_json(
        &context.app,
        Some(&token),
        Method::GET,
        "/wunder/agents",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{payload}");
    assert_eq!(payload["data"]["total"], json!(1));
    let items = payload["data"]["items"].as_array().expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0]["id"],
        json!("agent_legacy_a_new"),
        "the newest instance bound to the surviving preset stays"
    );

    let stored = context
        .state
        .user_store
        .list_user_agents(&user_id)
        .expect("list agents");
    assert_eq!(stored.len(), 4, "archiving never deletes legacy instances");
    let bound = stored
        .iter()
        .filter(|record| record.preset_binding.is_some())
        .collect::<Vec<_>>();
    assert_eq!(bound.len(), 1, "exactly one instance owns the binding");
    assert_eq!(bound[0].agent_id, "agent_legacy_a_new");
    assert_eq!(
        bound[0]
            .preset_binding
            .as_ref()
            .map(|binding| binding.preset_id.as_str()),
        Some(PRESET_A)
    );
    for archived_id in ["agent_legacy_a_old", "agent_legacy_b", "agent_custom"] {
        let record = stored
            .iter()
            .find(|record| record.agent_id == archived_id)
            .expect("archived record exists");
        assert_eq!(
            record.status, "archived",
            "{archived_id} should be archived"
        );
        assert!(record.preset_binding.is_none());
    }
    let legacy_thread = context
        .state
        .user_store
        .get_chat_session(&user_id, "session_of_archived_agent")
        .expect("load legacy thread")
        .expect("legacy thread survives convergence");
    assert_eq!(legacy_thread.status, "active");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bindings_endpoint_counts_create_rebind_and_unbind() {
    let context = build_context(|config, _| {
        config.user_agents.presets = vec![
            build_preset(PRESET_A, PRESET_A_NAME, None, PresetCustomizable::default()),
            build_preset(PRESET_B, PRESET_B_NAME, None, PresetCustomizable::default()),
        ];
    })
    .await;
    let _token_a = create_user_token(&context, "bind_user_a");
    let _token_b = create_user_token(&context, "bind_user_b");
    let user_a = user_id_of(&context, "bind_user_a");
    let user_b = user_id_of(&context, "bind_user_b");
    let users = vec![user_a.clone(), user_b.clone()];

    let (status, created) = bind_users(&context, PRESET_A, &users, "bind", None).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    assert_eq!(created["data"]["preset_id"], json!(PRESET_A));
    assert_eq!(created["data"]["affected_users"], json!(2));
    assert_eq!(created["data"]["created_agents"], json!(2));
    assert_eq!(created["data"]["rebound_agents"], json!(0));

    let (status, again) = bind_users(&context, PRESET_A, &users, "bind", None).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["data"]["affected_users"], json!(2));
    assert_eq!(again["data"]["created_agents"], json!(0));
    assert_eq!(
        again["data"]["rebound_agents"],
        json!(0),
        "same preset is idempotent"
    );

    let (status, rebound) = bind_users(
        &context,
        PRESET_A,
        &[user_a.clone()],
        "unbind",
        Some(PRESET_B),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rebound}");
    assert_eq!(rebound["data"]["preset_id"], json!(PRESET_B));
    assert_eq!(rebound["data"]["affected_users"], json!(1));
    assert_eq!(rebound["data"]["rebound_agents"], json!(1));
    assert_eq!(rebound["data"]["created_agents"], json!(0));

    let user_a_agents = context
        .state
        .user_store
        .list_user_agents(&user_a)
        .expect("list user a agents");
    let instance = user_a_agents
        .iter()
        .filter(|record| record.preset_binding.is_some())
        .collect::<Vec<_>>();
    assert_eq!(instance.len(), 1, "binding stays unique per user");
    assert_eq!(
        instance[0]
            .preset_binding
            .as_ref()
            .map(|binding| binding.preset_id.as_str()),
        Some(PRESET_B)
    );
    assert_eq!(
        instance[0].name, PRESET_B_NAME,
        "rebinding applies the target preset content"
    );

    let (status, rejected) =
        bind_users(&context, PRESET_A, &[user_a.clone()], "unbind", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        rejected["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("new_preset_id"),
        "{rejected}"
    );

    let (status, listed) = send_json(
        &context.app,
        Some(&context.admin_token),
        Method::GET,
        &format!("/wunder/admin/preset_agents/{PRESET_B}/bindings?page=1&page_size=20"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed["data"]["total"], json!(1));
    let items = listed["data"]["items"].as_array().expect("binding items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["user_id"], json!(user_a));
    assert_eq!(items[0]["username"], json!("bind_user_a"));
    assert_eq!(items[0]["agent_id"], json!(instance[0].agent_id));
    assert_eq!(items[0]["customized"], json!([]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preset_bindings_list_pages_filters_and_reports_customized_fields() {
    let context = build_context(|config, _| {
        config.user_agents.presets = vec![build_preset(
            PRESET_A,
            PRESET_A_NAME,
            Some("model-a"),
            PresetCustomizable {
                model_name: true,
                ..PresetCustomizable::default()
            },
        )];
    })
    .await;
    let token_a = create_user_token(&context, "page_user_a");
    let _token_b = create_user_token(&context, "page_user_b");
    let _token_c = create_user_token(&context, "page_user_c");
    let users = vec![
        user_id_of(&context, "page_user_a"),
        user_id_of(&context, "page_user_b"),
        user_id_of(&context, "page_user_c"),
    ];
    let (status, _) = bind_users(&context, PRESET_A, &users, "bind", None).await;
    assert_eq!(status, StatusCode::OK);

    // One user customizes the model so the binding list reports the field.
    let (status, instance) = send_json(
        &context.app,
        Some(&token_a),
        Method::GET,
        "/wunder/user/agent",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{instance}");
    let agent_id = instance["data"]["agent"]["id"]
        .as_str()
        .expect("agent id")
        .to_string();
    let (status, updated) = send_json(
        &context.app,
        Some(&token_a),
        Method::PUT,
        "/wunder/user/agent",
        Some(json!({ "model_name": "model-b" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");

    let (status, page_two) = send_json(
        &context.app,
        Some(&context.admin_token),
        Method::GET,
        &format!("/wunder/admin/preset_agents/{PRESET_A}/bindings?page=2&page_size=2"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page_two}");
    assert_eq!(page_two["data"]["total"], json!(3));
    assert_eq!(page_two["data"]["items"].as_array().map(Vec::len), Some(1));

    let (status, filtered) = send_json(
        &context.app,
        Some(&context.admin_token),
        Method::GET,
        &format!("/wunder/admin/preset_agents/{PRESET_A}/bindings?keyword=page_user_a"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{filtered}");
    assert_eq!(filtered["data"]["total"], json!(1));
    let item = &filtered["data"]["items"][0];
    assert_eq!(item["user_id"], json!(users[0]));
    assert_eq!(item["agent_id"], json!(agent_id));
    assert_eq!(item["customized"], json!(["model_name"]));

    // page_size above the contract cap is clamped, never unbounded.
    let (status, clamped) = send_json(
        &context.app,
        Some(&context.admin_token),
        Method::GET,
        &format!("/wunder/admin/preset_agents/{PRESET_A}/bindings?page_size=5000"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{clamped}");
    assert!(
        clamped["data"]["items"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0)
            <= 100,
        "page_size must be capped at 100"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preset_sync_safe_skips_user_edits_and_force_overrides_them() {
    let context = build_context(|config, _| {
        config.user_agents.presets = vec![build_preset(
            PRESET_A,
            PRESET_A_NAME,
            Some("model-a"),
            PresetCustomizable {
                model_name: true,
                ..PresetCustomizable::default()
            },
        )];
    })
    .await;
    let token = create_user_token(&context, "sync_user");
    let user_id = user_id_of(&context, "sync_user");
    let users = vec![user_id.clone()];
    let (status, _) = bind_users(&context, PRESET_A, &users, "bind", None).await;
    assert_eq!(status, StatusCode::OK);

    let (status, instance) = send_json(
        &context.app,
        Some(&token),
        Method::GET,
        "/wunder/user/agent",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{instance}");
    assert_eq!(
        instance["data"]["agent"]["configured_model_name"],
        json!("model-a")
    );

    // The user picks their own model (a customizable field).
    let (status, updated) = send_json(
        &context.app,
        Some(&token),
        Method::PUT,
        "/wunder/user/agent",
        Some(json!({ "model_name": "model-b" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(
        updated["data"]["agent"]["configured_model_name"],
        json!("model-b")
    );

    // The admin changes the preset model.
    let items = list_admin_presets(&context).await;
    let mut next_items = items.clone();
    for item in &mut next_items {
        if item["preset_id"] == json!(PRESET_A) {
            item["model_name"] = json!("model-c");
        }
    }
    let saved = update_admin_presets(&context, next_items).await;
    assert_eq!(
        preset_item(&saved, PRESET_A)["model_name"],
        json!("model-c")
    );

    let (status, safe) = send_json(
        &context.app,
        Some(&context.admin_token),
        Method::POST,
        "/wunder/admin/preset_agents/sync",
        Some(json!({ "preset_id": PRESET_A, "mode": "safe", "dry_run": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{safe}");
    assert_eq!(safe["data"]["preset_id"], json!(PRESET_A));
    assert_eq!(safe["data"]["mode"], json!("safe"));
    assert_eq!(safe["data"]["dry_run"], json!(false));
    assert_eq!(
        safe["data"]["skipped_customized"],
        json!(1),
        "the customized model must be preserved in safe mode: {safe}"
    );
    assert_eq!(safe["data"]["updated_agents"], json!(0));

    let (_, after_safe) = send_json(
        &context.app,
        Some(&token),
        Method::GET,
        "/wunder/user/agent",
        None,
    )
    .await;
    assert_eq!(
        after_safe["data"]["agent"]["configured_model_name"],
        json!("model-b"),
        "safe sync keeps the user edit"
    );

    let (status, force) = send_json(
        &context.app,
        Some(&context.admin_token),
        Method::POST,
        "/wunder/admin/preset_agents/sync",
        Some(json!({ "preset_id": PRESET_A, "mode": "force", "dry_run": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{force}");
    assert_eq!(force["data"]["mode"], json!("force"));
    assert_eq!(force["data"]["updated_agents"], json!(1));
    assert_eq!(
        force["data"]["skipped_customized"],
        json!(0),
        "force overwrites every preset-owned field: {force}"
    );

    let (_, after_force) = send_json(
        &context.app,
        Some(&token),
        Method::GET,
        "/wunder/user/agent",
        None,
    )
    .await;
    assert_eq!(
        after_force["data"]["agent"]["configured_model_name"],
        json!("model-c"),
        "force sync overwrites the user edit"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preset_sync_dry_run_previews_without_writing() {
    let context = build_context(|config, _| {
        config.user_agents.presets = vec![build_preset(
            PRESET_A,
            PRESET_A_NAME,
            Some("model-a"),
            PresetCustomizable::default(),
        )];
    })
    .await;
    let _token = create_user_token(&context, "dry_run_user");
    let user_id = user_id_of(&context, "dry_run_user");
    let (status, _) = bind_users(&context, PRESET_A, &[user_id.clone()], "bind", None).await;
    assert_eq!(status, StatusCode::OK);

    let items = list_admin_presets(&context).await;
    let mut next_items = items.clone();
    for item in &mut next_items {
        if item["preset_id"] == json!(PRESET_A) {
            item["model_name"] = json!("model-c");
        }
    }
    update_admin_presets(&context, next_items).await;

    let (status, preview) = send_json(
        &context.app,
        Some(&context.admin_token),
        Method::POST,
        "/wunder/admin/preset_agents/sync",
        Some(json!({ "preset_id": PRESET_A, "mode": "safe", "dry_run": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["data"]["dry_run"], json!(true));
    assert!(
        preview["data"]["affected_users"].as_u64().unwrap_or(0) >= 1,
        "{preview}"
    );
    assert_eq!(preview["data"]["updated_agents"], json!(1));

    let stored = context
        .state
        .user_store
        .list_user_agents(&user_id)
        .expect("list agents");
    let bound = stored
        .iter()
        .find(|record| record.preset_binding.is_some())
        .expect("bound instance");
    assert_eq!(
        bound.model_name.as_deref(),
        Some("model-a"),
        "dry_run must not touch storage"
    );

    let (status, applied) = send_json(
        &context.app,
        Some(&context.admin_token),
        Method::POST,
        "/wunder/admin/preset_agents/sync",
        Some(json!({ "preset_id": PRESET_A, "mode": "safe", "dry_run": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    let stored = context
        .state
        .user_store
        .list_user_agents(&user_id)
        .expect("list agents");
    let bound = stored
        .iter()
        .find(|record| record.preset_binding.is_some())
        .expect("bound instance");
    assert_eq!(bound.model_name.as_deref(), Some("model-c"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_agent_route_shape_and_customizable_enforcement() {
    let context = build_context(|config, _| {
        config.user_agents.presets = vec![build_preset(
            PRESET_A,
            PRESET_A_NAME,
            Some("model-a"),
            PresetCustomizable {
                system_prompt: true,
                ..PresetCustomizable::default()
            },
        )];
    })
    .await;
    let token = create_user_token(&context, "shape_user");

    let (status, payload) = send_json(
        &context.app,
        Some(&token),
        Method::GET,
        "/wunder/user/agent",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{payload}");
    let data = &payload["data"];
    assert!(
        data["agent"].is_object(),
        "agent payload required: {payload}"
    );
    assert_eq!(data["preset_binding"]["preset_id"], json!(PRESET_A));
    assert_eq!(data["preset_binding"]["name"], json!(PRESET_A_NAME));
    for key in [
        "system_prompt",
        "welcome",
        "model_name",
        "reasoning_effort",
        "tool_names",
        "approval_mode",
    ] {
        assert!(
            data["customizable"][key].is_boolean(),
            "customizable.{key} must be a boolean: {payload}"
        );
    }
    assert_eq!(data["customizable"]["system_prompt"], json!(true));
    assert_eq!(data["customizable"]["model_name"], json!(false));

    // Declared but not open -> rejected with an explicit code.
    let (status, denied) = send_json(
        &context.app,
        Some(&token),
        Method::PUT,
        "/wunder/user/agent",
        Some(json!({ "model_name": "model-b" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{denied}");
    assert_eq!(denied["error"]["code"], json!("FIELD_NOT_CUSTOMIZABLE"));
    assert_eq!(denied["detail"]["fields"][0]["field"], json!("model_name"));

    // Preset-owned fields are rejected too (never silently written).
    let (status, denied_name) = send_json(
        &context.app,
        Some(&token),
        Method::PUT,
        "/wunder/user/agent",
        Some(json!({ "name": "Renamed by user" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{denied_name}");
    assert_eq!(
        denied_name["detail"]["fields"][0]["reason"],
        json!("preset_owned")
    );

    let (status, accepted) = send_json(
        &context.app,
        Some(&token),
        Method::PUT,
        "/wunder/user/agent",
        Some(json!({ "system_prompt": "user prompt" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    assert_eq!(
        accepted["data"]["agent"]["system_prompt"],
        json!("user prompt"),
        "the response echoes the written values"
    );
    assert_eq!(
        accepted["data"]["preset_binding"]["preset_id"],
        json!(PRESET_A)
    );

    let (status, listed) = send_json(
        &context.app,
        Some(&token),
        Method::GET,
        "/wunder/agents",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["data"]["total"], json!(1));
    assert_eq!(
        listed["data"]["items"][0]["system_prompt"],
        json!("user prompt")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_user_lists_expose_binding_columns() {
    let context = build_context(|config, _| {
        config.user_agents.presets = vec![build_preset(
            PRESET_A,
            PRESET_A_NAME,
            Some("model-a"),
            PresetCustomizable {
                approval_mode: true,
                ..PresetCustomizable::default()
            },
        )];
    })
    .await;
    let token = create_user_token(&context, "list_user");
    let user_id = user_id_of(&context, "list_user");
    let (status, _) = bind_users(&context, PRESET_A, &[user_id.clone()], "bind", None).await;
    assert_eq!(status, StatusCode::OK);

    let (status, account_payload) = send_json(
        &context.app,
        Some(&context.admin_token),
        Method::GET,
        "/wunder/admin/user_accounts?keyword=list_user",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{account_payload}");
    let account = account_payload["data"]["items"]
        .as_array()
        .expect("account items")
        .iter()
        .find(|item| item["user_id"] == json!(user_id))
        .expect("listed account");
    assert_eq!(account["preset_id"], json!(PRESET_A));
    assert!(account["agent_id"].is_string(), "{account}");
    assert_eq!(account["customized_fields"], json!([]));

    // The monitor/thread panel (`{users: [...]}`) is a session-derived summary:
    // it lists users with recorded activity and carries the same binding columns
    // whenever a row is present.
    let (status, session_payload) = send_json(
        &context.app,
        Some(&token),
        Method::POST,
        "/wunder/chat/sessions",
        Some(json!({ "title": "list user session" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session_payload}");

    let (status, users_payload) = send_json(
        &context.app,
        Some(&context.admin_token),
        Method::GET,
        "/wunder/admin/users",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{users_payload}");
    let users = users_payload["users"]
        .as_array()
        .expect("users array")
        .clone();
    if let Some(entry) = users.iter().find(|item| item["user_id"] == json!(user_id)) {
        assert_eq!(entry["preset_id"], json!(PRESET_A));
        assert!(entry["agent_id"].is_string(), "{entry}");
        assert!(entry["customized_fields"].is_array(), "{entry}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_preset_list_reports_bound_users_and_customizable_surface() {
    let context = build_context(|config, temp_root| {
        config.user_agents.worker_cards_root = temp_root
            .join("preset_worker_cards")
            .to_string_lossy()
            .to_string();
        config.user_agents.presets = vec![build_preset(
            PRESET_A,
            PRESET_A_NAME,
            Some("model-a"),
            PresetCustomizable {
                model_name: true,
                ..PresetCustomizable::default()
            },
        )];
    })
    .await;

    // Persist through the admin API so the preset becomes asset-backed and
    // carries a real `updated_at`.
    let items = list_admin_presets(&context).await;
    update_admin_presets(&context, items).await;
    let _token = create_user_token(&context, "bound_user");
    let user_id = user_id_of(&context, "bound_user");
    let (status, _) = bind_users(&context, PRESET_A, &[user_id], "bind", None).await;
    assert_eq!(status, StatusCode::OK);

    let items = list_admin_presets(&context).await;
    let preset = preset_item(&items, PRESET_A);
    assert!(
        preset.get("sandbox_container_id").is_none(),
        "container ids are gone from the preset payload: {preset}"
    );
    assert_eq!(preset["bound_users"], json!(1));
    assert_eq!(preset["customizable"]["model_name"], json!(true));
    assert_eq!(preset["customizable"]["approval_mode"], json!(false));
    assert!(
        preset["updated_at"].is_number(),
        "asset-backed presets report a real updated_at: {preset}"
    );
}
