use super::*;
use crate::config::Config;
use crate::orchestrator::ConfigStore;
use crate::state::{AppState, AppStateInitOptions};

async fn fixture() -> (tempfile::TempDir, AppState) {
    let root = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.storage.backend = "sqlite".into();
    config.storage.data_root = root.path().join("data").to_string_lossy().into_owned();
    config.workspace.root = root.path().join("workspace").to_string_lossy().into_owned();
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
    (root, state)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_commit_gates_backpressure_instead_of_rejecting_threads() {
    let (_root, state) = fixture().await;
    let committer = &state.kernel.orchestrator.committer;
    let active = std::sync::atomic::AtomicUsize::new(0);
    let peak = std::sync::atomic::AtomicUsize::new(0);
    let requests = (0..2000).map(|index| {
        let active = &active;
        let peak = &peak;
        async move {
            let session = format!("fixture-thread-{index}");
            let gate = committer.gate(&session).await.unwrap();
            let count = active.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            peak.fetch_max(count, std::sync::atomic::Ordering::SeqCst);
            tokio::task::yield_now().await;
            active.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            drop(gate);
        }
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(60),
        futures::future::join_all(requests),
    )
    .await
    .expect("bounded commit slots must drain concurrent callers");
    assert_eq!(active.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(peak.load(std::sync::atomic::Ordering::SeqCst), 256);
    assert_eq!(committer.commit_slots.available_permits(), 256);
    assert!(committer.gates.lock().len() <= 256);
}

#[tokio::test]
async fn inquiry_claim_survives_interrupted_settlement_and_rejects_forgery() {
    let (_root, state) = fixture().await;
    let committer = &state.kernel.orchestrator.committer;
    let waiting = committer
        .accept_turn(
            "fixture-user",
            "fixture-thread",
            &json!({"content":"fixture question"}),
        )
        .await
        .unwrap();
    let turn = waiting["turn_id"].as_str().unwrap();
    committer
        .freeze_session(
            "fixture-user",
            "fixture-thread",
            "fixture frozen instructions",
            "",
        )
        .await
        .unwrap();
    committer
        .begin_turn("fixture-user", "fixture-thread", turn, "fixture")
        .await
        .unwrap();
    assert!(committer
        .update_turn(
            "fixture-user",
            "fixture-thread",
            turn,
            "waiting_input",
            "",
            &json!({})
        )
        .await
        .unwrap());
    let answer = state.thread_store.admission_store().unwrap().accept("fixture-user", "fixture-thread", &json!({"content":"fixture answer", "client_message_id":"fixture-client-a", "inquiry_turn_id":turn})).unwrap();
    assert!(!committer
        .update_turn(
            "fixture-user",
            "fixture-thread",
            turn,
            "completed",
            "",
            &json!({"stop_reason":"inquiry_answered", "answered_by_turn_id":"fixture-forged"})
        )
        .await
        .unwrap());
    let replay = committer.accept_turn("fixture-user", "fixture-thread", &json!({"content":"fixture answer", "client_message_id":"fixture-client-b", "inquiry_turn_id":turn})).await.unwrap();
    assert_eq!(replay["turn_id"], answer["turn_id"]);
    assert_eq!(replay["created"], false);
    let control =
        json!({"stop_reason":"inquiry_answered", "answered_by_turn_id":replay["turn_id"]});
    assert!(!committer
        .update_turn(
            "fixture-user",
            "fixture-thread",
            turn,
            "completed",
            "",
            &control
        )
        .await
        .unwrap());
    assert!(!committer
        .update_turn(
            "fixture-user",
            "fixture-thread",
            turn,
            "completed",
            "",
            &control
        )
        .await
        .unwrap());
    let admissions = state.thread_store.admission_store().unwrap();
    assert_eq!(
        admissions.turn("fixture-thread", turn).unwrap().unwrap()["status"],
        "completed"
    );
    assert_eq!(
        state
            .storage
            .latest_thread_user_round_by_session("fixture-thread")
            .unwrap(),
        2
    );
}

#[tokio::test]
async fn checkpoint_rejects_late_summary_after_stop_without_changing_context() {
    let (_root, state) = fixture().await;
    let committer = &state.kernel.orchestrator.committer;
    let accepted = committer
        .accept_turn(
            "fixture-user",
            "fixture-thread",
            &json!({"content":"fixture input"}),
        )
        .await
        .unwrap();
    let turn = accepted["turn_id"].as_str().unwrap();
    committer
        .freeze_session(
            "fixture-user",
            "fixture-thread",
            "fixture frozen instructions",
            "",
        )
        .await
        .unwrap();
    committer
        .begin_turn("fixture-user", "fixture-thread", turn, "fixture")
        .await
        .unwrap();
    let payload = json!({"session_id":"fixture-thread", "turn_id":turn, "role":"system", "content":"fixture summary", "meta":{"replacement_history":[{"role":"assistant", "content":"fixture retained context"}]}});
    committer
        .commit_context_checkpoint("fixture-user", &payload)
        .await
        .unwrap();
    assert!(committer
        .update_turn(
            "fixture-user",
            "fixture-thread",
            turn,
            "cancelled",
            "",
            &json!({"stop_reason":"fixture stop"})
        )
        .await
        .unwrap());
    let thread = ThreadId::new("fixture-thread").unwrap();
    let before = state
        .thread_store
        .model_messages(&thread, None)
        .await
        .unwrap();
    let position = committer.latest_position("fixture-thread").await.unwrap();
    assert!(committer
        .commit_context_checkpoint("fixture-user", &payload)
        .await
        .is_err());
    assert_eq!(
        committer.latest_position("fixture-thread").await.unwrap(),
        position
    );
    assert_eq!(
        state
            .thread_store
            .model_messages(&thread, None)
            .await
            .unwrap(),
        before
    );
    assert_eq!(
        state
            .thread_store
            .session_meta(&thread)
            .await
            .unwrap()
            .initial_system_prompt,
        "fixture frozen instructions"
    );
}

#[tokio::test]
async fn inquiry_stopped_before_answer_cannot_allocate_a_new_user_round() {
    let (_root, state) = fixture().await;
    let committer = &state.kernel.orchestrator.committer;
    let accepted = committer
        .accept_turn(
            "fixture-user",
            "fixture-thread",
            &json!({"content":"fixture input"}),
        )
        .await
        .unwrap();
    let turn = accepted["turn_id"].as_str().unwrap();
    committer
        .update_turn(
            "fixture-user",
            "fixture-thread",
            turn,
            "waiting_input",
            "",
            &json!({}),
        )
        .await
        .unwrap();
    committer
        .update_turn(
            "fixture-user",
            "fixture-thread",
            turn,
            "cancelled",
            "",
            &json!({}),
        )
        .await
        .unwrap();
    assert!(committer
        .accept_turn(
            "fixture-user",
            "fixture-thread",
            &json!({"content":"fixture answer", "inquiry_turn_id":turn})
        )
        .await
        .is_err());
    assert_eq!(
        state
            .storage
            .latest_thread_user_round_by_session("fixture-thread")
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn inquiry_unmaterialized_waiting_turn_keeps_waiting_state_until_completion() {
    let (_root, state) = fixture().await;
    let committer = &state.kernel.orchestrator.committer;
    let waiting = committer
        .accept_turn(
            "fixture-user",
            "fixture-thread",
            &json!({"content":"fixture question"}),
        )
        .await
        .unwrap();
    let turn = waiting["turn_id"].as_str().unwrap();
    committer
        .freeze_session(
            "fixture-user",
            "fixture-thread",
            "fixture frozen instructions",
            "",
        )
        .await
        .unwrap();
    committer
        .update_turn(
            "fixture-user",
            "fixture-thread",
            turn,
            "waiting_input",
            "",
            &json!({}),
        )
        .await
        .unwrap();
    let answer = committer
        .accept_turn(
            "fixture-user",
            "fixture-thread",
            &json!({"content":"fixture answer", "inquiry_turn_id":turn}),
        )
        .await
        .unwrap();
    assert_eq!(answer["user_turn_index"], 2);
    assert_eq!(
        state
            .thread_store
            .admission_store()
            .unwrap()
            .turn("fixture-thread", turn)
            .unwrap()
            .unwrap()["status"],
        "completed"
    );
    let page = state
        .storage
        .thread_view_page("fixture-user", "fixture-thread", "chat", None, 10)
        .unwrap();
    assert_eq!(page.chat[0].status, "completed");
    assert_eq!(page.chat[0].user.content, "fixture question");
}
