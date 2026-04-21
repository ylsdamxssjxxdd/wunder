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
    storage::{HiveRecord, UserAgentRecord, DEFAULT_SANDBOX_CONTAINER_ID},
};

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
        .join("orchestration-state-routes.db")
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
    app.state
        .user_store
        .upsert_hive(&record)
        .expect("upsert hive");
    record
}

fn create_agent(
    app: &TestApp,
    user: &TestUser,
    hive_id: &str,
    agent_id: &str,
    name: &str,
    prefer_mother: bool,
) -> UserAgentRecord {
    let now = now_ts();
    let record = UserAgentRecord {
        agent_id: agent_id.to_string(),
        user_id: user.user_id.clone(),
        hive_id: hive_id.to_string(),
        name: name.to_string(),
        description: format!("description for {name}"),
        system_prompt: format!("system prompt for {name}"),
        model_name: Some("gpt-5.4".to_string()),
        ability_items: Vec::new(),
        tool_names: Vec::new(),
        declared_tool_names: Vec::new(),
        declared_skill_names: Vec::new(),
        preset_questions: Vec::new(),
        access_level: "A".to_string(),
        approval_mode: "full_auto".to_string(),
        is_shared: false,
        status: "active".to_string(),
        icon: None,
        sandbox_container_id: DEFAULT_SANDBOX_CONTAINER_ID,
        created_at: now,
        updated_at: now,
        preset_binding: None,
        silent: false,
        prefer_mother,
    };
    app.state
        .user_store
        .upsert_user_agent(&record)
        .expect("upsert user agent");
    record
}

fn member_session_id(payload: &Value, pointer: &str, agent_id: &str) -> String {
    payload
        .pointer(pointer)
        .and_then(Value::as_array)
        .and_then(|items| {
            items.iter().find_map(|item| {
                let candidate = item.get("agent_id").and_then(Value::as_str)?.trim();
                if candidate == agent_id.trim() {
                    item.get("session_id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                } else {
                    None
                }
            })
        })
        .unwrap_or_default()
}

fn history_item<'a>(payload: &'a Value, orchestration_id: &str) -> &'a Value {
    payload
        .pointer("/data/items")
        .and_then(Value::as_array)
        .and_then(|items| {
            items.iter().find(|item| {
                item.get("orchestration_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    == Some(orchestration_id.trim())
            })
        })
        .expect("history item")
}

fn round_ids(payload: &Value, pointer: &str) -> Vec<String> {
    payload
        .pointer(pointer)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("id").and_then(Value::as_str).map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn round_user_message(payload: &Value, pointer: &str, round_id: &str) -> String {
    payload
        .pointer(pointer)
        .and_then(Value::as_array)
        .and_then(|items| {
            items.iter().find_map(|item| {
                let candidate = item.get("id").and_then(Value::as_str)?.trim();
                if candidate == round_id.trim() {
                    item.get("user_message")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                } else {
                    None
                }
            })
        })
        .unwrap_or_default()
}

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exit_orchestration_clears_active_state_and_rebinds_fresh_main_threads() {
    let app = build_test_app().await;
    let user = create_user_session(&app, "orch_exit_user");
    let hive = create_hive(
        &app,
        &user,
        "orch_exit_hive",
        "Orch Exit Hive",
        "orchestration exit",
    );
    let mother = create_agent(
        &app,
        &user,
        &hive.hive_id,
        "agent_mother_exit",
        "Mother Exit",
        true,
    );
    let worker = create_agent(
        &app,
        &user,
        &hive.hive_id,
        "agent_worker_exit",
        "Worker Exit",
        false,
    );

    let (status, create_payload) = send_json(
        &app.app,
        &user.token,
        Method::POST,
        "/wunder/beeroom/orchestration/state/create",
        Some(json!({
            "group_id": hive.hive_id,
            "mother_agent_id": mother.agent_id,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let orchestration_id = create_payload
        .pointer("/data/state/orchestration_id")
        .and_then(Value::as_str)
        .expect("orchestration id")
        .to_string();
    let mother_session_before =
        member_session_id(&create_payload, "/data/member_threads", &mother.agent_id);
    let worker_session_before =
        member_session_id(&create_payload, "/data/member_threads", &worker.agent_id);
    assert!(!mother_session_before.is_empty());
    assert!(!worker_session_before.is_empty());

    let (status, exit_payload) = send_json(
        &app.app,
        &user.token,
        Method::POST,
        "/wunder/beeroom/orchestration/state/exit",
        Some(json!({
            "group_id": hive.hive_id,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        exit_payload
            .pointer("/data/active")
            .and_then(Value::as_bool),
        Some(false)
    );

    let mother_session_after =
        member_session_id(&exit_payload, "/data/member_threads", &mother.agent_id);
    let worker_session_after =
        member_session_id(&exit_payload, "/data/member_threads", &worker.agent_id);
    assert!(!mother_session_after.is_empty());
    assert!(!worker_session_after.is_empty());
    assert_ne!(mother_session_after, mother_session_before);
    assert_ne!(worker_session_after, worker_session_before);

    let (status, state_payload) = send_json(
        &app.app,
        &user.token,
        Method::GET,
        &format!(
            "/wunder/beeroom/orchestration/state?group_id={}",
            hive.hive_id
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        state_payload
            .pointer("/data/active")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(state_payload.pointer("/data/state"), Some(&Value::Null));

    let (status, history_payload) = send_json(
        &app.app,
        &user.token,
        Method::GET,
        &format!(
            "/wunder/beeroom/orchestration/history?group_id={}",
            hive.hive_id
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let item = history_item(&history_payload, &orchestration_id);
    assert_eq!(item.get("status").and_then(Value::as_str), Some("closed"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_pending_round_does_not_survive_history_restore() {
    let app = build_test_app().await;
    let user = create_user_session(&app, "orch_restore_user");
    let hive = create_hive(
        &app,
        &user,
        "orch_restore_hive",
        "Orch Restore Hive",
        "orchestration restore",
    );
    let mother = create_agent(
        &app,
        &user,
        &hive.hive_id,
        "agent_mother_restore",
        "Mother Restore",
        true,
    );
    let worker = create_agent(
        &app,
        &user,
        &hive.hive_id,
        "agent_worker_restore",
        "Worker Restore",
        false,
    );

    let (status, create_payload) = send_json(
        &app.app,
        &user.token,
        Method::POST,
        "/wunder/beeroom/orchestration/state/create",
        Some(json!({
            "group_id": hive.hive_id,
            "mother_agent_id": mother.agent_id,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let orchestration_id = create_payload
        .pointer("/data/state/orchestration_id")
        .and_then(Value::as_str)
        .expect("orchestration id")
        .to_string();
    let mother_session_before =
        member_session_id(&create_payload, "/data/member_threads", &mother.agent_id);
    let worker_session_before =
        member_session_id(&create_payload, "/data/member_threads", &worker.agent_id);
    assert!(!mother_session_before.is_empty());
    assert!(!worker_session_before.is_empty());

    let (status, reserve_round_one_payload) = send_json(
        &app.app,
        &user.token,
        Method::POST,
        "/wunder/beeroom/orchestration/rounds/reserve",
        Some(json!({
            "group_id": hive.hive_id,
            "round_id": "round_01",
            "round_index": 1,
            "user_message": "第一轮正式消息",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        reserve_round_one_payload
            .pointer("/data/round/id")
            .and_then(Value::as_str),
        Some("round_01")
    );

    let (status, finalize_round_one_payload) = send_json(
        &app.app,
        &user.token,
        Method::POST,
        "/wunder/beeroom/orchestration/rounds/finalize",
        Some(json!({
            "group_id": hive.hive_id,
            "round_id": "round_01",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        finalize_round_one_payload
            .pointer("/data/round/finalized_at")
            .and_then(Value::as_f64)
            .unwrap_or_default()
            > 0.0
    );

    let (status, reserve_round_two_payload) = send_json(
        &app.app,
        &user.token,
        Method::POST,
        "/wunder/beeroom/orchestration/rounds/reserve",
        Some(json!({
            "group_id": hive.hive_id,
            "round_index": 2,
            "user_message": "这一轮会被停止",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        reserve_round_two_payload
            .pointer("/data/round/id")
            .and_then(Value::as_str),
        Some("round_02")
    );

    let (status, cancel_round_two_payload) = send_json(
        &app.app,
        &user.token,
        Method::POST,
        "/wunder/beeroom/orchestration/rounds/cancel",
        Some(json!({
            "group_id": hive.hive_id,
            "round_id": "round_02",
            "message_started_at": 1000.0,
            "message_ended_at": 1001.0,
            "remove_round": true,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        round_ids(&cancel_round_two_payload, "/data/round_state/rounds"),
        vec!["round_01".to_string()]
    );
    assert_eq!(
        round_user_message(
            &cancel_round_two_payload,
            "/data/round_state/rounds",
            "round_01"
        ),
        "第一轮正式消息".to_string()
    );
    assert_eq!(
        cancel_round_two_payload
            .pointer("/data/round_state/suppressed_message_ranges")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );

    let (status, active_history_payload) = send_json(
        &app.app,
        &user.token,
        Method::GET,
        &format!(
            "/wunder/beeroom/orchestration/history?group_id={}",
            hive.hive_id
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let active_item = history_item(&active_history_payload, &orchestration_id);
    assert_eq!(
        active_item
            .get("latest_round_index")
            .and_then(Value::as_i64),
        Some(1)
    );
    assert_eq!(
        active_item.get("status").and_then(Value::as_str),
        Some("active")
    );

    let (status, exit_payload) = send_json(
        &app.app,
        &user.token,
        Method::POST,
        "/wunder/beeroom/orchestration/state/exit",
        Some(json!({
            "group_id": hive.hive_id,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        exit_payload
            .pointer("/data/active")
            .and_then(Value::as_bool),
        Some(false)
    );

    let (status, restore_payload) = send_json(
        &app.app,
        &user.token,
        Method::POST,
        "/wunder/beeroom/orchestration/history/restore",
        Some(json!({
            "group_id": hive.hive_id,
            "orchestration_id": orchestration_id,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        round_ids(&restore_payload, "/data/state/round_state/rounds"),
        vec!["round_01".to_string()]
    );
    assert_eq!(
        round_user_message(
            &restore_payload,
            "/data/state/round_state/rounds",
            "round_01"
        ),
        "第一轮正式消息".to_string()
    );
    assert_eq!(
        restore_payload
            .pointer("/data/history/latest_round_index")
            .and_then(Value::as_i64),
        Some(1)
    );
    assert_eq!(
        restore_payload
            .pointer("/data/state/mother_session_id")
            .and_then(Value::as_str),
        Some(mother_session_before.as_str())
    );
    assert_eq!(
        member_session_id(&restore_payload, "/data/member_threads", &mother.agent_id),
        mother_session_before
    );
    assert_eq!(
        member_session_id(&restore_payload, "/data/member_threads", &worker.agent_id),
        worker_session_before
    );

    let (status, state_payload) = send_json(
        &app.app,
        &user.token,
        Method::GET,
        &format!(
            "/wunder/beeroom/orchestration/state?group_id={}",
            hive.hive_id
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        state_payload
            .pointer("/data/active")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        round_ids(&state_payload, "/data/state/round_state/rounds"),
        vec!["round_01".to_string()]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disconnected_history_restore_keeps_orchestration_inactive() {
    let app = build_test_app().await;
    let user = create_user_session(&app, "orch_inactive_restore_user");
    let hive = create_hive(
        &app,
        &user,
        "orch_inactive_restore_hive",
        "Orch Inactive Restore Hive",
        "inactive restore",
    );
    let mother = create_agent(
        &app,
        &user,
        &hive.hive_id,
        "agent_mother_inactive_restore",
        "Mother Inactive Restore",
        true,
    );
    let worker = create_agent(
        &app,
        &user,
        &hive.hive_id,
        "agent_worker_inactive_restore",
        "Worker Inactive Restore",
        false,
    );

    let (status, create_payload) = send_json(
        &app.app,
        &user.token,
        Method::POST,
        "/wunder/beeroom/orchestration/state/create",
        Some(json!({
            "group_id": hive.hive_id,
            "mother_agent_id": mother.agent_id,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let orchestration_id = create_payload
        .pointer("/data/state/orchestration_id")
        .and_then(Value::as_str)
        .expect("orchestration id")
        .to_string();
    let mother_session_before =
        member_session_id(&create_payload, "/data/member_threads", &mother.agent_id);
    let worker_session_before =
        member_session_id(&create_payload, "/data/member_threads", &worker.agent_id);
    assert!(!mother_session_before.is_empty());
    assert!(!worker_session_before.is_empty());

    let mother_thread_before = app
        .state
        .user_store
        .get_agent_thread(&user.user_id, &mother.agent_id)
        .expect("mother thread before")
        .expect("mother thread record before");
    let worker_thread_before = app
        .state
        .user_store
        .get_agent_thread(&user.user_id, &worker.agent_id)
        .expect("worker thread before")
        .expect("worker thread record before");

    let (status, exit_payload) = send_json(
        &app.app,
        &user.token,
        Method::POST,
        "/wunder/beeroom/orchestration/state/exit",
        Some(json!({
            "group_id": hive.hive_id,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let mother_fresh_session =
        member_session_id(&exit_payload, "/data/member_threads", &mother.agent_id);
    let worker_fresh_session =
        member_session_id(&exit_payload, "/data/member_threads", &worker.agent_id);
    assert_ne!(mother_fresh_session, mother_session_before);
    assert_ne!(worker_fresh_session, worker_session_before);

    let (status, restore_payload) = send_json(
        &app.app,
        &user.token,
        Method::POST,
        "/wunder/beeroom/orchestration/history/restore",
        Some(json!({
            "group_id": hive.hive_id,
            "orchestration_id": orchestration_id,
            "activate": false,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        restore_payload
            .pointer("/data/state/active")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        restore_payload
            .pointer("/data/history/status")
            .and_then(Value::as_str),
        Some("closed")
    );
    assert_eq!(
        member_session_id(&restore_payload, "/data/member_threads", &mother.agent_id),
        mother_session_before
    );
    assert_eq!(
        member_session_id(&restore_payload, "/data/member_threads", &worker.agent_id),
        worker_session_before
    );

    let (status, state_payload) = send_json(
        &app.app,
        &user.token,
        Method::GET,
        &format!(
            "/wunder/beeroom/orchestration/state?group_id={}",
            hive.hive_id
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        state_payload
            .pointer("/data/active")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(state_payload.pointer("/data/state"), Some(&Value::Null));

    let mother_thread_after = app
        .state
        .user_store
        .get_agent_thread(&user.user_id, &mother.agent_id)
        .expect("mother thread after")
        .expect("mother thread record after");
    let worker_thread_after = app
        .state
        .user_store
        .get_agent_thread(&user.user_id, &worker.agent_id)
        .expect("worker thread after")
        .expect("worker thread record after");
    assert_eq!(mother_thread_after.session_id, mother_fresh_session);
    assert_eq!(worker_thread_after.session_id, worker_fresh_session);
    assert_eq!(mother_thread_before.session_id, mother_session_before);
    assert_eq!(worker_thread_before.session_id, worker_session_before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn get_state_repairs_mother_main_thread_back_to_orchestration_session() {
    let app = build_test_app().await;
    let user = create_user_session(&app, "orch_repair_user");
    let hive = create_hive(
        &app,
        &user,
        "orch_repair_hive",
        "Orch Repair Hive",
        "repair mother main thread",
    );
    let mother = create_agent(
        &app,
        &user,
        &hive.hive_id,
        "agent_mother_repair",
        "Mother Repair",
        true,
    );
    let _worker = create_agent(
        &app,
        &user,
        &hive.hive_id,
        "agent_worker_repair",
        "Worker Repair",
        false,
    );

    let (status, create_payload) = send_json(
        &app.app,
        &user.token,
        Method::POST,
        "/wunder/beeroom/orchestration/state/create",
        Some(json!({
            "group_id": hive.hive_id,
            "mother_agent_id": mother.agent_id,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let mother_orchestration_session =
        member_session_id(&create_payload, "/data/member_threads", &mother.agent_id);
    assert!(!mother_orchestration_session.is_empty());

    let detached_session_id = "sess_detached_mother_repair";
    let now = now_ts();
    app.state
        .user_store
        .upsert_chat_session(&wunder_server::storage::ChatSessionRecord {
            session_id: detached_session_id.to_string(),
            user_id: user.user_id.clone(),
            title: "Detached Mother Session".to_string(),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
            last_message_at: now,
            agent_id: Some(mother.agent_id.clone()),
            tool_overrides: Vec::new(),
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        })
        .expect("create detached chat session");
    app.state
        .user_store
        .upsert_agent_thread(&wunder_server::storage::AgentThreadRecord {
            thread_id: format!("thread_{detached_session_id}"),
            user_id: user.user_id.clone(),
            agent_id: mother.agent_id.clone(),
            session_id: detached_session_id.to_string(),
            status: "idle".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("rebind detached mother thread");

    let before = app
        .state
        .user_store
        .get_agent_thread(&user.user_id, &mother.agent_id)
        .expect("load mother thread before")
        .expect("mother thread before exists");
    assert_eq!(before.session_id, detached_session_id);

    let (status, state_payload) = send_json(
        &app.app,
        &user.token,
        Method::GET,
        &format!(
            "/wunder/beeroom/orchestration/state?group_id={}",
            hive.hive_id
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        state_payload
            .pointer("/data/active")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        state_payload
            .pointer("/data/state/mother_session_id")
            .and_then(Value::as_str),
        Some(mother_orchestration_session.as_str())
    );

    let repaired = app
        .state
        .user_store
        .get_agent_thread(&user.user_id, &mother.agent_id)
        .expect("load mother thread after")
        .expect("mother thread after exists");
    assert_eq!(repaired.session_id, mother_orchestration_session);
    assert_eq!(
        repaired.thread_id,
        format!("thread_{mother_orchestration_session}")
    );
}
