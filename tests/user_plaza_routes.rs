use axum::{
    body::{to_bytes, Body},
    http::{header::AUTHORIZATION, Method, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;
use wunder_server::{
    build_router,
    config::Config,
    config_store::ConfigStore,
    state::{AppState, AppStateInitOptions},
    storage::{HiveRecord, UserAgentRecord, DEFAULT_SANDBOX_CONTAINER_ID},
    user_plaza::UserPlazaItemRecord,
};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipWriter};

struct TestApp {
    state: Arc<AppState>,
    app: Router,
    _temp_dir: TempDir,
}

struct TestUser {
    token: String,
    user_id: String,
}

async fn build_test_app() -> TestApp {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = Config::default();
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = temp_dir
        .path()
        .join("user-plaza-routes.db")
        .to_string_lossy()
        .to_string();
    config.workspace.root = temp_dir
        .path()
        .join("workspaces")
        .to_string_lossy()
        .to_string();
    config.skills.enabled.clear();

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

    TestApp {
        app: build_router(state.clone()),
        state,
        _temp_dir: temp_dir,
    }
}

fn create_user_session(app: &TestApp, username: &str) -> TestUser {
    let user = app
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
    let token = app
        .state
        .user_store
        .create_session_token(&user.user_id)
        .expect("create token")
        .token;
    TestUser {
        token,
        user_id: user.user_id,
    }
}

async fn send_json(
    app: &Router,
    token: &str,
    method: Method,
    path: &str,
    payload: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(AUTHORIZATION, format!("Bearer {token}"));
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

async fn publish_plaza_item(
    app: &TestApp,
    user: &TestUser,
    kind: &str,
    source_key: &str,
    title: &str,
) -> Value {
    let (status, payload) = send_json(
        &app.app,
        &user.token,
        Method::POST,
        "/wunder/plaza/items",
        Some(json!({
            "kind": kind,
            "source_key": source_key,
            "title": title,
            "summary": format!("summary for {title}"),
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    payload
}

async fn import_plaza_item(app: &TestApp, user: &TestUser, item_id: &str) -> (StatusCode, Value) {
    send_json(
        &app.app,
        &user.token,
        Method::POST,
        &format!("/wunder/plaza/items/{item_id}/import"),
        Some(json!({})),
    )
    .await
}

fn item_id_from_payload(payload: &Value) -> String {
    payload
        .pointer("/data/item_id")
        .and_then(Value::as_str)
        .expect("plaza item id")
        .to_string()
}

fn load_plaza_record(app: &TestApp, item_id: &str) -> UserPlazaItemRecord {
    let raw = app
        .state
        .user_store
        .get_meta(&format!("user_plaza:item:{item_id}"))
        .expect("query plaza meta")
        .expect("plaza meta should exist");
    serde_json::from_str(&raw).expect("parse plaza record")
}

fn create_custom_skill(app: &TestApp, user: &TestUser, skill_name: &str) -> PathBuf {
    let skill_dir = app.state.user_tool_store.get_skill_root(&user.user_id).join(skill_name);
    fs::create_dir_all(skill_dir.join("notes")).expect("create skill notes dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: {skill_name}\ndescription: plaza skill\n---\n# {skill_name}\n"),
    )
    .expect("write SKILL.md");
    fs::write(skill_dir.join("notes").join("guide.txt"), "skill guide")
        .expect("write skill guide");
    skill_dir
}

fn create_hive(
    app: &TestApp,
    user: &TestUser,
    hive_id: &str,
    name: &str,
    description: &str,
) -> HiveRecord {
    let record = HiveRecord {
        hive_id: hive_id.to_string(),
        user_id: user.user_id.clone(),
        name: name.to_string(),
        description: description.to_string(),
        is_default: false,
        status: "active".to_string(),
        created_time: now_ts(),
        updated_time: now_ts(),
    };
    app.state.user_store.upsert_hive(&record).expect("upsert hive");
    record
}

fn create_agent(
    app: &TestApp,
    user: &TestUser,
    hive_id: &str,
    agent_id: &str,
    name: &str,
    description: &str,
    icon: Option<&str>,
    silent: bool,
    prefer_mother: bool,
) -> UserAgentRecord {
    let now = now_ts();
    let record = UserAgentRecord {
        agent_id: agent_id.to_string(),
        user_id: user.user_id.clone(),
        hive_id: hive_id.to_string(),
        name: name.to_string(),
        description: description.to_string(),
        system_prompt: format!("system prompt for {name}"),
        model_name: Some("gpt-5.4".to_string()),
        ability_items: Vec::new(),
        tool_names: Vec::new(),
        declared_tool_names: Vec::new(),
        declared_skill_names: Vec::new(),
        preset_questions: vec!["What should I do next?".to_string()],
        access_level: "A".to_string(),
        approval_mode: "full_auto".to_string(),
        is_shared: false,
        status: "active".to_string(),
        icon: icon.map(str::to_string),
        sandbox_container_id: DEFAULT_SANDBOX_CONTAINER_ID,
        created_at: now,
        updated_at: now,
        preset_binding: None,
        silent,
        prefer_mother,
    };
    app.state
        .user_store
        .upsert_user_agent(&record)
        .expect("upsert agent");
    record
}

fn overwrite_with_illegal_skill_archive(path: &Path) {
    let file = fs::File::create(path).expect("create archive file");
    let mut writer = ZipWriter::new(file);
    let options = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    writer
        .start_file("shared/../evil.txt", options)
        .expect("start illegal zip entry");
    writer
        .write_all(b"malicious payload")
        .expect("write illegal payload");
    writer.finish().expect("finish illegal archive");
}

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_items_filters_by_owner_and_kind_and_skips_missing_artifacts() {
    let app = build_test_app().await;
    let owner = create_user_session(&app, "plaza_owner");
    let viewer = create_user_session(&app, "plaza_viewer");

    create_custom_skill(&app, &owner, "plaza_owner_skill");
    let owner_skill = publish_plaza_item(
        &app,
        &owner,
        "skill_pack",
        "plaza_owner_skill",
        "Owner Visible Skill",
    )
    .await;
    let owner_skill_id = item_id_from_payload(&owner_skill);

    let hive = create_hive(
        &app,
        &owner,
        "owner_ops_hive",
        "Owner Ops",
        "owner worker hive",
    );
    let agent = create_agent(
        &app,
        &owner,
        &hive.hive_id,
        "agent_owner_worker_card",
        "Owner Worker Card",
        "owner worker description",
        Some("palette:#d6a84a"),
        false,
        false,
    );
    let owner_worker = publish_plaza_item(
        &app,
        &owner,
        "worker_card",
        &agent.agent_id,
        "Owner Worker Card Share",
    )
    .await;
    let owner_worker_id = item_id_from_payload(&owner_worker);

    create_custom_skill(&app, &viewer, "plaza_viewer_visible_skill");
    let viewer_visible = publish_plaza_item(
        &app,
        &viewer,
        "skill_pack",
        "plaza_viewer_visible_skill",
        "Viewer Visible Skill",
    )
    .await;
    let viewer_visible_id = item_id_from_payload(&viewer_visible);

    create_custom_skill(&app, &viewer, "plaza_viewer_hidden_skill");
    let viewer_hidden = publish_plaza_item(
        &app,
        &viewer,
        "skill_pack",
        "plaza_viewer_hidden_skill",
        "Viewer Hidden Skill",
    )
    .await;
    let viewer_hidden_id = item_id_from_payload(&viewer_hidden);
    let hidden_record = load_plaza_record(&app, &viewer_hidden_id);
    fs::remove_file(&hidden_record.artifact_path).expect("delete hidden plaza artifact");

    let (status, skill_list) = send_json(
        &app.app,
        &owner.token,
        Method::GET,
        "/wunder/plaza/items?kind=skill",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let skill_items = skill_list["data"]["items"]
        .as_array()
        .expect("skill items array");
    assert_eq!(skill_items.len(), 2);
    assert!(skill_items
        .iter()
        .all(|item| item.get("kind").and_then(Value::as_str) == Some("skill_pack")));
    let visible_ids = skill_items
        .iter()
        .filter_map(|item| item.get("item_id").and_then(Value::as_str))
        .map(str::to_string)
        .collect::<HashSet<_>>();
    assert!(visible_ids.contains(&owner_skill_id));
    assert!(visible_ids.contains(&viewer_visible_id));
    assert!(!visible_ids.contains(&viewer_hidden_id));
    let owner_skill_mine = skill_items
        .iter()
        .find(|item| item.get("item_id").and_then(Value::as_str) == Some(owner_skill_id.as_str()))
        .and_then(|item| item.get("mine"))
        .and_then(Value::as_bool);
    let viewer_visible_mine = skill_items
        .iter()
        .find(|item| item.get("item_id").and_then(Value::as_str) == Some(viewer_visible_id.as_str()))
        .and_then(|item| item.get("mine"))
        .and_then(Value::as_bool);
    assert_eq!(owner_skill_mine, Some(true));
    assert_eq!(viewer_visible_mine, Some(false));

    let (status, mine_only) = send_json(
        &app.app,
        &owner.token,
        Method::GET,
        "/wunder/plaza/items?mine_only=true",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let mine_only_items = mine_only["data"]["items"]
        .as_array()
        .expect("mine only items");
    let mine_only_ids = mine_only_items
        .iter()
        .filter_map(|item| item.get("item_id").and_then(Value::as_str))
        .map(str::to_string)
        .collect::<HashSet<_>>();
    assert_eq!(mine_only_ids.len(), 2);
    assert!(mine_only_ids.contains(&owner_skill_id));
    assert!(mine_only_ids.contains(&owner_worker_id));
    assert!(mine_only_items
        .iter()
        .all(|item| item.get("mine").and_then(Value::as_bool) == Some(true)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_pack_import_extracts_skill_for_other_user() {
    let app = build_test_app().await;
    let owner = create_user_session(&app, "plaza_skill_owner");
    let importer = create_user_session(&app, "plaza_skill_importer");

    create_custom_skill(&app, &owner, "plaza_skill_shared");
    let published = publish_plaza_item(
        &app,
        &owner,
        "skill_pack",
        "plaza_skill_shared",
        "Shared Skill Pack",
    )
    .await;
    let item_id = item_id_from_payload(&published);

    let (status, payload) = import_plaza_item(&app, &importer, &item_id).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        payload.pointer("/data/kind").and_then(Value::as_str),
        Some("skill_pack")
    );
    assert!(
        payload
            .pointer("/data/skill_import/extracted")
            .and_then(Value::as_u64)
            .expect("extracted file total")
            >= 2
    );
    let top_dirs = payload
        .pointer("/data/skill_import/top_level_dirs")
        .and_then(Value::as_array)
        .expect("top level dirs");
    assert!(top_dirs
        .iter()
        .any(|value| value.as_str() == Some("plaza_skill_shared")));

    let imported_root = app.state.user_tool_store.get_skill_root(&importer.user_id);
    assert!(imported_root.join("plaza_skill_shared").join("SKILL.md").is_file());
    assert!(imported_root
        .join("plaza_skill_shared")
        .join("notes")
        .join("guide.txt")
        .is_file());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn worker_card_import_preserves_silent_and_prefer_mother() {
    let app = build_test_app().await;
    let owner = create_user_session(&app, "plaza_worker_owner");
    let importer = create_user_session(&app, "plaza_worker_importer");

    let hive = create_hive(
        &app,
        &owner,
        "worker_card_hive",
        "Worker Card Hive",
        "worker card export hive",
    );
    let source_agent = create_agent(
        &app,
        &owner,
        &hive.hive_id,
        "agent_worker_card_source",
        "Quiet Mother",
        "preserve silent and prefer_mother",
        Some("palette:#8c5e3c"),
        true,
        true,
    );
    let published = publish_plaza_item(
        &app,
        &owner,
        "worker_card",
        &source_agent.agent_id,
        "Quiet Mother Card",
    )
    .await;
    let item_id = item_id_from_payload(&published);

    let (status, payload) = import_plaza_item(&app, &importer, &item_id).await;
    assert_eq!(status, StatusCode::OK);
    let imported_agent_id = payload
        .pointer("/data/imported_agent_id")
        .and_then(Value::as_str)
        .expect("imported agent id");
    let imported_hive_id = payload
        .pointer("/data/imported_hive_id")
        .and_then(Value::as_str)
        .expect("imported hive id");

    let imported_agent = app
        .state
        .user_store
        .get_user_agent(&importer.user_id, imported_agent_id)
        .expect("query imported agent")
        .expect("imported agent should exist");
    assert_eq!(imported_agent.name, source_agent.name);
    assert_eq!(imported_agent.description, source_agent.description);
    assert_eq!(imported_agent.icon, source_agent.icon);
    assert_eq!(imported_agent.model_name, source_agent.model_name);
    assert!(imported_agent.silent);
    assert!(imported_agent.prefer_mother);

    let imported_hive = app
        .state
        .user_store
        .get_hive(&importer.user_id, imported_hive_id)
        .expect("query imported hive")
        .expect("imported hive should exist");
    assert_eq!(imported_hive.name, hive.name);
    assert_eq!(imported_hive.description, hive.description);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hive_pack_import_reports_hive_id_and_recreates_agents() {
    let app = build_test_app().await;
    let owner = create_user_session(&app, "plaza_hive_owner");
    let importer = create_user_session(&app, "plaza_hive_importer");

    let hive = create_hive(
        &app,
        &owner,
        "shared_ops_hive",
        "Shared Ops Hive",
        "hive pack export source",
    );
    create_agent(
        &app,
        &owner,
        &hive.hive_id,
        "agent_shared_ops_mother",
        "Mother Operator",
        "mother worker",
        Some("palette:#aa7744"),
        false,
        true,
    );
    create_agent(
        &app,
        &owner,
        &hive.hive_id,
        "agent_shared_ops_worker",
        "Silent Operator",
        "silent worker",
        Some("palette:#557799"),
        true,
        false,
    );
    let published = publish_plaza_item(
        &app,
        &owner,
        "hive_pack",
        &hive.hive_id,
        "Shared Ops Hive Pack",
    )
    .await;
    let item_id = item_id_from_payload(&published);

    let (status, payload) = import_plaza_item(&app, &importer, &item_id).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        payload.pointer("/data/imported_job/status").and_then(Value::as_str),
        Some("completed")
    );
    let imported_hive_id = payload
        .pointer("/data/imported_hive_id")
        .and_then(Value::as_str)
        .expect("imported hive id")
        .to_string();
    assert_eq!(
        payload
            .pointer("/data/imported_job/report/hive_id")
            .and_then(Value::as_str),
        Some(imported_hive_id.as_str())
    );

    let imported_hive = app
        .state
        .user_store
        .get_hive(&importer.user_id, &imported_hive_id)
        .expect("query imported hive")
        .expect("imported hive should exist");
    assert_eq!(imported_hive.name, hive.name);

    let imported_agents = app
        .state
        .user_store
        .list_user_agents_by_hive(&importer.user_id, &imported_hive_id)
        .expect("list imported agents");
    assert_eq!(imported_agents.len(), 2);
    assert!(imported_agents
        .iter()
        .any(|agent| agent.name == "Mother Operator" && agent.prefer_mother));
    assert!(imported_agents
        .iter()
        .any(|agent| agent.name == "Silent Operator" && agent.silent));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn delete_item_requires_owner_and_removes_artifact_and_meta() {
    let app = build_test_app().await;
    let owner = create_user_session(&app, "plaza_delete_owner");
    let other = create_user_session(&app, "plaza_delete_other");

    create_custom_skill(&app, &owner, "plaza_delete_skill");
    let published = publish_plaza_item(
        &app,
        &owner,
        "skill_pack",
        "plaza_delete_skill",
        "Delete Me",
    )
    .await;
    let item_id = item_id_from_payload(&published);
    let record = load_plaza_record(&app, &item_id);
    assert!(Path::new(&record.artifact_path).is_file());

    let (status, payload) = send_json(
        &app.app,
        &other.token,
        Method::DELETE,
        &format!("/wunder/plaza/items/{item_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        payload.pointer("/error/message").and_then(Value::as_str),
        Some("plaza item not found")
    );
    assert!(Path::new(&record.artifact_path).is_file());
    assert!(app
        .state
        .user_store
        .get_meta(&format!("user_plaza:item:{item_id}"))
        .expect("query plaza meta")
        .is_some());

    let (status, payload) = send_json(
        &app.app,
        &owner.token,
        Method::DELETE,
        &format!("/wunder/plaza/items/{item_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        payload.pointer("/data/ok").and_then(Value::as_bool),
        Some(true)
    );
    assert!(!Path::new(&record.artifact_path).exists());
    assert!(app
        .state
        .user_store
        .get_meta(&format!("user_plaza:item:{item_id}"))
        .expect("query plaza meta")
        .is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_pack_import_rejects_illegal_archive_paths() {
    let app = build_test_app().await;
    let owner = create_user_session(&app, "plaza_illegal_owner");
    let importer = create_user_session(&app, "plaza_illegal_importer");

    create_custom_skill(&app, &owner, "plaza_illegal_skill");
    let published = publish_plaza_item(
        &app,
        &owner,
        "skill_pack",
        "plaza_illegal_skill",
        "Illegal Skill Pack",
    )
    .await;
    let item_id = item_id_from_payload(&published);
    let record = load_plaza_record(&app, &item_id);
    overwrite_with_illegal_skill_archive(Path::new(&record.artifact_path));

    let (status, payload) = import_plaza_item(&app, &importer, &item_id).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        payload.pointer("/error/message").and_then(Value::as_str),
        Some("skill archive contains illegal paths")
    );

    let imported_root = app.state.user_tool_store.get_skill_root(&importer.user_id);
    assert!(!imported_root.join("evil.txt").exists());
    assert!(!imported_root.join("shared").join("evil.txt").exists());
}
