//! Isolated real HTTP/WS peer for release-browser display regression.
//! All state lives in a fresh temporary directory; no repository config is read.
use anyhow::Result;
use axum::{extract::State, routing::post, Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;
use tower_http::services::{ServeDir, ServeFile};
use wunder_server::{
    api,
    config::Config,
    config_store::ConfigStore,
    state::{AppState, AppStateInitOptions},
    storage::{ChatSessionRecord, StressThreadScenario, StressThreadSpec},
};

#[tokio::main]
async fn main() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut config = Config::default();
    config.storage.backend = "sqlite".into();
    config.storage.data_root = dir.path().join("data").to_string_lossy().into();
    config.workspace.root = dir
        .path()
        .join("fixture-workspace")
        .to_string_lossy()
        .into();
    config.skills.enabled.clear();
    let store = ConfigStore::new(dir.path().join("fixture.yaml"));
    let saved = config.clone();
    store.update(|c| *c = saved.clone()).await?;
    let state = Arc::new(AppState::new_with_options(
        store,
        config,
        AppStateInitOptions::cli_default().with_start_thread_runtime(false),
    )?);
    let user = state.user_store.create_user(
        "fixture-user",
        None,
        "fixture-password",
        None,
        None,
        vec!["user".into()],
        "active",
        false,
    )?;
    state.storage.upsert_chat_session(&ChatSessionRecord {
        session_id: "fixture-thread".into(),
        user_id: user.user_id.clone(),
        title: "Fixture conversation".into(),
        status: "active".into(),
        created_at: 1.,
        updated_at: 1.,
        last_message_at: 1.,
        agent_id: None,
        workspace_id: None,
        tool_overrides: vec![],
        parent_session_id: None,
        parent_message_id: None,
        spawn_label: None,
        spawned_by: None,
    })?;
    // Keep startup bounded while exercising the same rollout writer and
    // completed-item projector as real threads and the stress generator.
    let spec = StressThreadSpec {
        session_id: "fixture-thread".into(),
        title: "Fixture conversation".into(),
        workspace_id: None,
        user_rounds: 20,
        model_rounds_per_turn: 30,
        seed: Some(7),
        scenario: StressThreadScenario::Complex,
    };
    let stats = state
        .thread_store
        .generate_stress_thread(&user.user_id, &spec, |_, _| {})
        .await?;
    let cursor = state
        .storage
        .latest_thread_change_seq_by_session("fixture-thread")?;
    state
        .kernel
        .orchestrator
        .notify_thread_committed("fixture-thread", cursor);
    let dist = std::path::PathBuf::from("frontend/dist");
    let app = api::build_router(state.clone())
        .merge(
            Router::new()
                .route("/fixture/append", post(append))
                .with_state(state),
        )
        .fallback_service(
            ServeDir::new(&dist).not_found_service(ServeFile::new(dist.join("index.html"))),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:4176").await?;
    println!("fixture ready: {} persisted items", stats.items);
    axum::serve(listener, app).await?;
    drop(dir);
    Ok(())
}
async fn append(State(state): State<Arc<AppState>>) -> Json<Value> {
    let user = state
        .user_store
        .get_user_by_username("fixture-user")
        .unwrap()
        .unwrap();
    let turn = state
        .storage
        .accept_thread_turn(
            &user.user_id,
            "fixture-thread",
            &json!({
                "content":"Fixture live input",
                "client_message_id":format!("fixture-live-{}", uuid::Uuid::new_v4())
            }),
        )
        .unwrap();
    let id = turn["turn_id"].as_str().unwrap();
    state
        .storage
        .append_thread_item(
            &user.user_id,
            &json!({
                "session_id":"fixture-thread",
                "turn_id":id,
                "item_id":format!("{id}:text-1"),
                "kind":"assistant_message",
                "role":"assistant",
                "status":"completed",
                "content":"Fixture live answer"
            }),
        )
        .unwrap();
    state
        .storage
        .update_thread_turn(
            &user.user_id,
            "fixture-thread",
            id,
            "completed",
            "",
            &json!({}),
        )
        .unwrap();
    let cursor = state
        .storage
        .latest_thread_change_seq_by_session("fixture-thread")
        .unwrap();
    state
        .kernel
        .orchestrator
        .notify_thread_committed("fixture-thread", cursor);
    Json(json!({"ok":true}))
}
