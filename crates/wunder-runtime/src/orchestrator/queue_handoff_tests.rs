use super::*;
use crate::state::{AppState, AppStateInitOptions};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queue_handoff_parks_original_turn_and_keeps_cancel_available() {
    let root = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.storage.backend = "sqlite".into();
    config.storage.data_root = root.path().join("data").to_string_lossy().into_owned();
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
    let accepted = state
        .storage
        .accept_thread_turn("user-a", "session-a", &json!({"content":"input"}))
        .unwrap();
    let durable_turn = accepted["turn_id"].as_str().unwrap();
    state
        .monitor
        .register("session-a", "user-a", "agent-a", "input", false);
    state
        .storage
        .try_acquire_session_lock("session-a", "user-a", "agent-a", 60.0, 1)
        .unwrap();
    orchestrator
        .thread_runtime
        .begin_turn("session-a", "turn-a");
    let _owner = orchestrator.scheduling.register("session-a", false);
    let (live_tx, mut live_rx) = tokio::sync::mpsc::channel(16);
    let emitter = EventEmitter::new(
        "session-a".into(),
        "user-a".into(),
        Some(live_tx),
        Some(state.storage.clone()),
        state.monitor.clone(),
        false,
        None,
    )
    .with_committer(orchestrator.committer.clone());
    emitter.bind_turn(durable_turn, 1);
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
    let events = state
        .storage
        .list_thread_changes_by_session("session-a", 0, 100)
        .unwrap();
    assert!(
        events.is_empty(),
        "queue state is live control, not history"
    );
    let mut queue_events = Vec::new();
    while queue_events.len() < 2 {
        let event = tokio::time::timeout(Duration::from_secs(1), live_rx.recv())
            .await
            .unwrap()
            .expect("live queue control event");
        if let StreamSignal::Event(event) = event {
            if matches!(event.event.as_str(), "queue_enter" | "queue_start") {
                queue_events.push(event);
            }
        }
    }
    assert_eq!(queue_events[0].event, "queue_enter");
    assert_eq!(queue_events[0].data["data"]["queue_state"], "suspended");
    assert_eq!(queue_events[1].event, "queue_start");
    assert_eq!(queue_events[1].data["data"]["queue_state"], "resumed");
    assert!(state
        .storage
        .load_stream_events("session-a", 0, 100)
        .unwrap()
        .is_empty());

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
