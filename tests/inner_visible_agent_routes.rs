use axum::{
    body::{to_bytes, Body},
    http::{header::AUTHORIZATION, Method, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tower::ServiceExt;
use wunder_server::{
    build_router,
    config::Config,
    config_store::ConfigStore,
    state::{AppState, AppStateInitOptions},
    user_access::{build_user_tool_context, compute_allowed_tool_names},
};

struct TestContext {
    state: Arc<AppState>,
    app: Router,
    token: String,
    user_id: String,
    _temp_dir: TempDir,
}

async fn build_test_context(username: &str) -> TestContext {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = Config::default();
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = temp_dir
        .path()
        .join("inner-visible-agent-routes.db")
        .to_string_lossy()
        .to_string();
    config.workspace.root = temp_dir
        .path()
        .join("workspaces")
        .to_string_lossy()
        .to_string();
    config.skills.enabled.clear();

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
        app: build_router(state.clone()),
        state,
        token,
        user_id: user.user_id,
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

fn resolve_private_root(state: &AppState, user_id: &str) -> PathBuf {
    let scoped = state.workspace.scoped_user_id_by_container(user_id, 0);
    state.workspace.workspace_root(&scoped)
}

fn pick_stable_allowed_tool(allowed: &HashSet<String>, exclude: Option<&str>) -> Option<String> {
    let exclude = exclude.unwrap_or_default().trim().to_string();
    let mut candidates = allowed
        .iter()
        .filter(|name| {
            let trimmed = name.trim();
            !trimmed.is_empty()
                && trimmed != exclude
                && !trimmed.contains('@')
                && !trimmed.contains("://")
        })
        .cloned()
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        candidates = allowed
            .iter()
            .filter(|name| {
                let trimmed = name.trim();
                !trimmed.is_empty() && trimmed != exclude
            })
            .cloned()
            .collect::<Vec<_>>();
    }
    candidates.sort();
    candidates.into_iter().next()
}

fn to_string_list(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn get_agent_reads_worker_card_updates_after_pre_read_sync() {
    let context = build_test_context("inner_visible_sync_user").await;
    let (status, created) = send_json(
        &context.app,
        Some(&context.token),
        Method::POST,
        "/wunder/agents",
        Some(json!({ "name": "Sync Source Agent", "system_prompt": "old prompt" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let agent_id = created["data"]["id"]
        .as_str()
        .expect("agent id")
        .to_string();

    let private_root = resolve_private_root(context.state.as_ref(), &context.user_id);
    let skill_name = "sync_self_skill";
    let skill_alias = format!("{}@{}", context.user_id, skill_name);
    let skill_dir = private_root.join("skills").join(skill_name);
    fs::create_dir_all(&skill_dir).expect("create skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: {skill_name}\ndescription: test\n---\n# {skill_name}\n"),
    )
    .expect("write SKILL.md");
    context
        .state
        .user_tool_store
        .update_skills(&context.user_id, vec![skill_name.to_string()], Vec::new())
        .expect("enable custom skill");
    context
        .state
        .user_tool_manager
        .clear_skill_cache(Some(&context.user_id));

    let user = context
        .state
        .user_store
        .get_user_by_id(&context.user_id)
        .expect("query user")
        .expect("user exists");
    let tool_context = build_user_tool_context(context.state.as_ref(), &context.user_id).await;
    let allowed = compute_allowed_tool_names(&user, &tool_context);
    let selected_tool = pick_stable_allowed_tool(&allowed, Some(&skill_alias));

    let worker_card_path = private_root
        .join("agents")
        .join(format!("{agent_id}.worker-card.json"));
    let mut worker_card: Value =
        serde_json::from_str(&fs::read_to_string(&worker_card_path).expect("read worker card"))
            .expect("parse worker card");
    std::thread::sleep(Duration::from_millis(30));
    worker_card["metadata"]["name"] = json!("Synced Agent Name");
    worker_card["prompt"]["system_prompt"] = Value::Null;
    worker_card["prompt"]["extra_prompt"] = json!("new prompt from worker-card");
    if let Some(tool_name) = selected_tool.as_ref() {
        worker_card["abilities"]["tool_names"] = json!([tool_name.clone()]);
    } else {
        worker_card["abilities"]["tool_names"] = json!([]);
    }
    worker_card["abilities"]["skills"] = json!([skill_alias.clone()]);
    worker_card["interaction"]["preset_questions"] = json!(["Q1", "Q2"]);
    worker_card["runtime"]["approval_mode"] = json!("suggest");
    worker_card["runtime"]["sandbox_container_id"] = json!(3);
    fs::write(
        &worker_card_path,
        serde_json::to_vec_pretty(&worker_card).expect("serialize worker card"),
    )
    .expect("write worker card");

    let (status, fetched) = send_json(
        &context.app,
        Some(&context.token),
        Method::GET,
        &format!("/wunder/agents/{agent_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let data = &fetched["data"];
    assert_eq!(data["name"], json!("Synced Agent Name"));
    assert_eq!(data["system_prompt"], json!("new prompt from worker-card"));
    if let Some(tool_name) = selected_tool.as_ref() {
        assert_eq!(data["declared_tool_names"], json!([tool_name.clone()]));
    } else {
        assert_eq!(data["declared_tool_names"], json!([]));
    }
    assert_eq!(data["declared_skill_names"], json!([skill_alias.clone()]));
    assert_eq!(data["approval_mode"], json!("suggest"));
    assert_eq!(data["sandbox_container_id"], json!(3));
    let tool_names = to_string_list(&data["tool_names"]);
    if let Some(tool_name) = selected_tool.as_ref() {
        assert!(
            tool_names.contains(tool_name),
            "tool_names should include selected tool"
        );
    }
    if allowed.contains(&skill_alias) {
        assert!(
            tool_names.contains(&skill_alias),
            "tool_names should include allowed user skill alias"
        );
    }

    let (status, listed) = send_json(
        &context.app,
        Some(&context.token),
        Method::GET,
        "/wunder/agents",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = listed["data"]["items"]
        .as_array()
        .expect("agent list array");
    let listed_agent = items
        .iter()
        .find(|item| item["id"] == json!(agent_id))
        .expect("target agent in list");
    assert_eq!(listed_agent["name"], json!("Synced Agent Name"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn get_agent_survives_invalid_worker_card_and_keeps_last_good_state() {
    let context = build_test_context("inner_visible_invalid_user").await;
    let (status, created) = send_json(
        &context.app,
        Some(&context.token),
        Method::POST,
        "/wunder/agents",
        Some(json!({ "name": "Stable Agent", "system_prompt": "stable prompt" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let agent_id = created["data"]["id"]
        .as_str()
        .expect("agent id")
        .to_string();

    let private_root = resolve_private_root(context.state.as_ref(), &context.user_id);
    let worker_card_path = private_root
        .join("agents")
        .join(format!("{agent_id}.worker-card.json"));
    std::thread::sleep(Duration::from_millis(30));
    fs::write(
        &worker_card_path,
        "{ \"schema_version\": \"wunder/worker-card@1\", \"prompt\": ",
    )
    .expect("write broken worker card");

    let (status, fetched) = send_json(
        &context.app,
        Some(&context.token),
        Method::GET,
        &format!("/wunder/agents/{agent_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["data"]["name"], json!("Stable Agent"));
    assert_eq!(fetched["data"]["system_prompt"], json!("stable prompt"));

    let repaired: Value = serde_json::from_str(
        &fs::read_to_string(&worker_card_path).expect("read repaired worker card"),
    )
    .expect("worker card should be repaired to valid json");
    assert_eq!(
        repaired
            .pointer("/prompt/extra_prompt")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "stable prompt"
    );
}
