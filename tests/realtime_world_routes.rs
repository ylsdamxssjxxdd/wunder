use axum::{
    body::{to_bytes, Body},
    http::{header::AUTHORIZATION, Method, Request, StatusCode},
    Router,
};
use serde_json::Value;
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;
use wunder_server::{
    build_desktop_router,
    config::Config,
    config_store::ConfigStore,
    presence::ProjectionTargetKind,
    state::{AppState, AppStateInitOptions},
    storage::{ChatSessionRecord, TeamRunRecord, TeamTaskRecord},
};

struct TestContext {
    app: Router,
    state: Arc<AppState>,
    token: String,
    admin_token: String,
    user_id: String,
    _temp_dir: TempDir,
}

async fn build_test_context(username: &str) -> TestContext {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = Config::default();
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = temp_dir
        .path()
        .join("realtime-world-routes.db")
        .to_string_lossy()
        .to_string();
    config.workspace.root = temp_dir
        .path()
        .join("workspaces")
        .to_string_lossy()
        .to_string();

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

    let admin = state
        .user_store
        .create_user(
            &format!("{username}_admin"),
            Some(format!("{username}_admin@example.test")),
            "password-123",
            Some("A"),
            None,
            vec!["admin".to_string()],
            "active",
            false,
        )
        .expect("create admin user");
    let admin_token = state
        .user_store
        .create_session_token(&admin.user_id)
        .expect("create admin token")
        .token;

    TestContext {
        app: build_desktop_router(state.clone()),
        state,
        token,
        admin_token,
        user_id: user.user_id,
        _temp_dir: temp_dir,
    }
}

async fn send_request(
    app: &Router,
    token: &str,
    method: Method,
    path: &str,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    let payload = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("parse response json")
    };
    (status, payload)
}

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_realtime_snapshot_reports_route_and_watch_state() {
    let context = build_test_context("realtime_user").await;
    let session_id = "sess_realtime_snapshot";
    context
        .state
        .user_store
        .upsert_chat_session(&ChatSessionRecord {
            session_id: session_id.to_string(),
            user_id: context.user_id.clone(),
            title: "Realtime".to_string(),
            status: "active".to_string(),
            created_at: now_ts(),
            updated_at: now_ts(),
            last_message_at: now_ts(),
            agent_id: None,
            tool_overrides: Vec::new(),
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        })
        .expect("upsert chat session");

    let _submit_lease = context
        .state
        .control
        .route_leases
        .try_acquire_submit_lease(session_id, "thread_runtime_test")
        .expect("submit lease");
    let _thread_lease = context
        .state
        .control
        .route_leases
        .try_acquire_route_lease(
            wunder_server::directory::RouteTargetKind::Thread,
            &format!("thread_{session_id}"),
            "thread_runtime_test",
            Some(session_id),
            Some(&context.user_id),
        )
        .expect("thread route lease");
    context.state.control.presence.watch_projection(
        "conn-1",
        "req-1",
        &context.user_id,
        ProjectionTargetKind::Session,
        session_id,
        now_ts(),
    );

    let (status, payload) = send_request(
        &context.app,
        &context.token,
        Method::GET,
        &format!("/wunder/realtime/sessions/{session_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["data"]["session_id"], session_id);
    assert_eq!(
        payload["data"]["submit_lease"]["owner_id"],
        "thread_runtime_test"
    );
    assert_eq!(
        payload["data"]["thread_route"]["owner_id"],
        "thread_runtime_test"
    );
    assert_eq!(payload["data"]["session_watch"]["watch_count"], 1);
    assert_eq!(payload["data"]["session_watch"]["user_count"], 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn realtime_metrics_requires_admin_and_reports_control_plane_counts() {
    let context = build_test_context("realtime_metrics_user").await;
    context.state.control.presence.watch_projection(
        "conn-admin",
        "req-admin",
        &context.user_id,
        ProjectionTargetKind::Session,
        "sess_metrics",
        now_ts(),
    );
    let _thread_lease = context
        .state
        .control
        .route_leases
        .try_acquire_route_lease(
            wunder_server::directory::RouteTargetKind::Thread,
            "thread_sess_metrics",
            "thread_runtime_test",
            Some("sess_metrics"),
            Some(&context.user_id),
        )
        .expect("thread route lease");

    let (forbidden_status, _) = send_request(
        &context.app,
        &context.token,
        Method::GET,
        "/wunder/realtime/metrics",
    )
    .await;
    assert_eq!(forbidden_status, StatusCode::FORBIDDEN);

    let (status, payload) = send_request(
        &context.app,
        &context.admin_token,
        Method::GET,
        "/wunder/realtime/metrics",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        payload["data"]["presence"]["projection_watch_metrics"]["total_watch_count"],
        1
    );
    assert_eq!(payload["data"]["route_leases"]["active_route_count"], 1);
    assert_eq!(payload["data"]["route_leases"]["thread_route_count"], 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mission_realtime_snapshot_reports_route_watch_and_task_summary() {
    let context = build_test_context("realtime_mission_user").await;
    let team_run_id = "team_run_realtime_snapshot";
    let parent_session_id = "sess_mission_parent";
    let hive_id = "hive-realtime";
    context
        .state
        .user_store
        .upsert_chat_session(&ChatSessionRecord {
            session_id: parent_session_id.to_string(),
            user_id: context.user_id.clone(),
            title: "Mission Parent".to_string(),
            status: "active".to_string(),
            created_at: now_ts(),
            updated_at: now_ts(),
            last_message_at: now_ts(),
            agent_id: None,
            tool_overrides: Vec::new(),
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        })
        .expect("upsert parent session");
    context
        .state
        .user_store
        .upsert_team_run(&TeamRunRecord {
            team_run_id: team_run_id.to_string(),
            user_id: context.user_id.clone(),
            hive_id: hive_id.to_string(),
            parent_session_id: parent_session_id.to_string(),
            parent_agent_id: Some("agent-parent".to_string()),
            mother_agent_id: Some("agent-mother".to_string()),
            strategy: "parallel".to_string(),
            status: "running".to_string(),
            task_total: 2,
            task_success: 1,
            task_failed: 0,
            context_tokens_total: 120,
            context_tokens_peak: 64,
            model_round_total: 5,
            started_time: Some(now_ts() - 5.0),
            finished_time: None,
            elapsed_s: Some(5.0),
            summary: Some("running summary".to_string()),
            error: None,
            updated_time: now_ts(),
        })
        .expect("upsert team run");
    context
        .state
        .user_store
        .upsert_team_task(&TeamTaskRecord {
            task_id: "task-success".to_string(),
            team_run_id: team_run_id.to_string(),
            user_id: context.user_id.clone(),
            hive_id: hive_id.to_string(),
            agent_id: "agent-a".to_string(),
            target_session_id: Some(parent_session_id.to_string()),
            spawned_session_id: Some("spawned-a".to_string()),
            session_run_id: Some("run-a".to_string()),
            status: "success".to_string(),
            retry_count: 0,
            priority: 4,
            started_time: Some(now_ts() - 4.0),
            finished_time: Some(now_ts() - 2.0),
            elapsed_s: Some(2.0),
            result_summary: Some("done".to_string()),
            error: None,
            updated_time: now_ts() - 2.0,
        })
        .expect("upsert success task");
    context
        .state
        .user_store
        .upsert_team_task(&TeamTaskRecord {
            task_id: "task-running".to_string(),
            team_run_id: team_run_id.to_string(),
            user_id: context.user_id.clone(),
            hive_id: hive_id.to_string(),
            agent_id: "agent-b".to_string(),
            target_session_id: Some(parent_session_id.to_string()),
            spawned_session_id: Some("spawned-b".to_string()),
            session_run_id: Some("run-b".to_string()),
            status: "running".to_string(),
            retry_count: 1,
            priority: 9,
            started_time: Some(now_ts() - 1.0),
            finished_time: None,
            elapsed_s: Some(1.0),
            result_summary: None,
            error: None,
            updated_time: now_ts() - 1.0,
        })
        .expect("upsert running task");

    let _mission_lease = context
        .state
        .control
        .route_leases
        .try_acquire_route_lease(
            wunder_server::directory::RouteTargetKind::Mission,
            team_run_id,
            "mission_runtime_test",
            Some(parent_session_id),
            Some(&context.user_id),
        )
        .expect("mission route lease");
    context.state.control.presence.watch_projection(
        "conn-mission",
        "req-beeroom",
        &context.user_id,
        ProjectionTargetKind::BeeroomGroup,
        hive_id,
        now_ts(),
    );
    context.state.control.presence.watch_projection(
        "conn-mission",
        "req-parent",
        &context.user_id,
        ProjectionTargetKind::Session,
        parent_session_id,
        now_ts(),
    );

    let (status, payload) = send_request(
        &context.app,
        &context.token,
        Method::GET,
        &format!("/wunder/realtime/missions/{team_run_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["data"]["mission"]["mission_id"], team_run_id);
    assert_eq!(
        payload["data"]["mission_route"]["owner_id"],
        "mission_runtime_test"
    );
    assert_eq!(payload["data"]["beeroom_group_watch"]["watch_count"], 1);
    assert_eq!(payload["data"]["parent_session_watch"]["watch_count"], 1);
    assert_eq!(payload["data"]["task_summary"]["total"], 2);
    assert_eq!(payload["data"]["task_summary"]["running"], 1);
    assert_eq!(payload["data"]["task_summary"]["success"], 1);
    assert_eq!(payload["data"]["task_summary"]["highest_priority"], 9);
    assert_eq!(payload["data"]["latest_task"]["task_id"], "task-running");
}
