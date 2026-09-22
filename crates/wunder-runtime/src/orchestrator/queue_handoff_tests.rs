use super::*;
use crate::state::{AppState, AppStateInitOptions};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queue_handoff_parks_original_turn_and_keeps_cancel_available() {
    let root = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.storage.backend = "sqlite".into();
    config.storage.db_path = root.path().join("state.db").to_string_lossy().into_owned();
    config.workspace.root = root.path().join("workspace").to_string_lossy().into_owned();
    config.server.max_active_sessions = 1;
    let store = ConfigStore::new(root.path().join("config.yaml"));
    store
        .update(|current| *current = config.clone())
        .await
        .unwrap();
    let state = AppState::new_with_options(
        store,
        config,
        AppStateInitOptions::cli_default().with_start_thread_runtime(false),
    )
    .unwrap();
    let orchestrator = state.kernel.orchestrator.clone();
    state
        .monitor
        .register("session-a", "user-a", "agent-a", "input", false, false);
    state
        .storage
        .try_acquire_session_lock("session-a", "user-a", "agent-a", 60.0, 1)
        .unwrap();
    orchestrator
        .thread_runtime
        .begin_turn("session-a", "turn-a");
    let _owner = orchestrator.scheduling.register("session-a", false);
    let emitter = EventEmitter::new(
        "session-a".into(),
        "user-a".into(),
        None,
        Some(state.storage.clone()),
        state.monitor.clone(),
        false,
        0,
        None,
    );
    orchestrator.scheduling.request_slot("session-b").unwrap();
    let worker = orchestrator.clone();
    let output = emitter.clone();
    let paused = tokio::spawn(async move {
        worker
            .yield_queue_slot("session-a", &output, RoundInfo::new(1, 2))
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while orchestrator.scheduling.state("session-a") != Some("suspended") {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(!paused.is_finished());
    assert_eq!(
        state
            .storage
            .try_acquire_session_lock("session-b", "user-b", "agent-b", 60.0, 1)
            .unwrap(),
        SessionLockStatus::Acquired
    );
    assert_eq!(
        state
            .storage
            .try_acquire_session_lock("session-a", "user-a", "agent-a", 60.0, 1)
            .unwrap(),
        SessionLockStatus::UserBusy
    );
    state.storage.release_session_lock("session-b").unwrap();
    orchestrator.scheduling.grant_resume().unwrap();
    tokio::time::timeout(Duration::from_secs(5), paused)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let snapshot = orchestrator
        .get_tool_session_runtime_snapshot("session-a")
        .unwrap();
    assert_eq!(
        (
            snapshot["thread_status"].clone(),
            snapshot["active_turn_id"].clone()
        ),
        (json!("running"), json!("turn-a"))
    );
    flush_stream_event_persist_queue().await;
    let events = state
        .storage
        .load_stream_events("session-a", 0, 100)
        .unwrap();
    assert!(events.iter().any(|event| event["event"] == "queue_enter"));
    assert!(events.iter().any(|event| event["event"] == "queue_start"));

    orchestrator.scheduling.request_slot("session-b").unwrap();
    let worker = orchestrator.clone();
    let cancelled = tokio::spawn(async move {
        worker
            .yield_queue_slot("session-a", &emitter, RoundInfo::new(1, 2))
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while orchestrator.scheduling.state("session-a") != Some("suspended") {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(state.monitor.cancel("session-a"));
    let error = tokio::time::timeout(Duration::from_secs(5), cancelled)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code(), "CANCELLED");
}
