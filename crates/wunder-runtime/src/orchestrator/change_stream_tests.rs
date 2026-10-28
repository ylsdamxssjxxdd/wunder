use super::*;
use crate::state::{AppState, AppStateInitOptions};
use event_stream::StreamSignal;

#[tokio::test(flavor = "current_thread")]
async fn cancellation_wakes_waiter_without_polling_and_keeps_source() {
    let (state, _dir) = build_test_state("cancel_signal").await;
    let session = "fixture-cancel-thread";
    state
        .monitor
        .register(session, "fixture-owner", "", "fixture", false);
    let waiter = state.kernel.orchestrator.wait_for_cancelled(session);
    tokio::pin!(waiter);
    // Poll explicitly: no wall-clock deadline and no timer advance can make
    // a periodic-only implementation pass this check.
    assert!(futures::poll!(waiter.as_mut()).is_pending());
    state.monitor.cancel_with_source(session, "fixture-stop");
    let error = match futures::poll!(waiter.as_mut()) {
        std::task::Poll::Ready(error) => error,
        std::task::Poll::Pending => panic!("cancellation did not wake immediately"),
    };
    assert_eq!(
        error.to_payload()["detail"]["cancel_source"],
        "fixture-stop"
    );
    let already_cancelled = state.kernel.orchestrator.wait_for_cancelled(session);
    tokio::pin!(already_cancelled);
    assert!(futures::poll!(already_cancelled.as_mut()).is_ready());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stream_completion_waits_for_capacity_and_executor_cleanup() {
    let (state, _dir) = build_test_state("stream_finish").await;
    let (queue_tx, mut queue_rx) = mpsc::channel(1);
    let emitter = EventEmitter::new(
        "fixture-thread".into(),
        "fixture-owner".into(),
        Some(queue_tx.clone()),
        None,
        state.monitor.clone(),
        false,
        None,
    );
    queue_tx
        .send(StreamSignal::Event(StreamEvent {
            event: "fixture".into(),
            data: json!({}),
            id: None,
            timestamp: None,
        }))
        .await
        .unwrap();
    let finishing = {
        let emitter = emitter.clone();
        tokio::spawn(async move { emitter.finish().await })
    };
    tokio::task::yield_now().await;
    assert!(!finishing.is_finished());
    assert!(matches!(
        queue_rx.recv().await,
        Some(StreamSignal::Event(_))
    ));
    finishing.await.unwrap();
    assert!(matches!(queue_rx.recv().await, Some(StreamSignal::Done)));

    let (event_tx, mut event_rx) = mpsc::channel(1);
    let (cleanup_tx, cleanup_rx) = tokio::sync::oneshot::channel();
    let runner = tokio::spawn(async move {
        let _ = cleanup_rx.await;
        // A manual operation can emit completion and then let its outer
        // request finish. The pump must drain both bounded signals.
        queue_tx.send(StreamSignal::Done).await.unwrap();
        queue_tx.send(StreamSignal::Done).await.unwrap();
    });
    state.kernel.orchestrator.spawn_stream_pump(
        "fixture-thread".into(),
        queue_rx,
        event_tx,
        emitter,
        runner,
    );
    // The pump must yield, and must keep the receiver open until cleanup ends.
    tokio::time::timeout(Duration::from_secs(1), tokio::task::yield_now())
        .await
        .unwrap();
    assert!(matches!(
        event_rx.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    cleanup_tx.send(()).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn child_tree_cancellation_wakes_nested_waiter_without_polling() {
    let (state, _dir) = build_test_state("child_cancel_signal").await;
    let child = state
        .monitor
        .register_child_run("fixture-child", "fixture-parent")
        .unwrap();
    let waiter = state
        .kernel
        .orchestrator
        .wait_for_cancelled("fixture-child");
    tokio::pin!(waiter);
    assert!(futures::poll!(waiter.as_mut()).is_pending());
    state
        .monitor
        .cancel_with_source("fixture-parent", "fixture-stop");
    assert!(futures::poll!(waiter.as_mut()).is_ready());
    drop(child);
}

async fn build_test_state(name: &str) -> (Arc<AppState>, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.storage.backend = "sqlite".into();
    config.storage.data_root = root
        .path()
        .join(format!("{name}"))
        .to_string_lossy()
        .into_owned();
    config.workspace.root = root.path().join("workspace").to_string_lossy().into_owned();
    let store = ConfigStore::new(root.path().join("config.yaml"));
    let config_for_store = config.clone();
    store
        .update(|current| *current = config_for_store)
        .await
        .unwrap();
    let state = Arc::new(
        AppState::new_with_options(
            store,
            config,
            AppStateInitOptions::cli_default().with_start_thread_runtime(false),
        )
        .unwrap(),
    );
    (state, root)
}

fn accept_turn(state: &AppState, session: &str, user: &str) -> String {
    let input = json!({
        "role": "user",
        "content": "你好",
        "client_message_id": Uuid::new_v4().to_string(),
    });
    let accepted = state
        .storage
        .accept_thread_turn(user, session, &input)
        .unwrap();
    accepted["turn_id"].as_str().unwrap().to_string()
}

async fn start_turn(state: &AppState, session: &str, user: &str) -> String {
    let turn = accept_turn(state, session, user);
    let committer = &state.kernel.orchestrator.committer;
    committer
        .freeze_session(
            user,
            session,
            "fixture frozen instructions",
            "fixture-workspace",
        )
        .await
        .unwrap();
    committer
        .begin_turn(user, session, &turn, "fixture-model")
        .await
        .unwrap();
    turn
}

async fn drain_changes(queue_rx: &mut mpsc::Receiver<StreamSignal>) -> (Vec<Value>, Vec<Value>) {
    let mut changes = Vec::new();
    let mut tails = Vec::new();
    while let Ok(signal) = queue_rx.try_recv() {
        if let StreamSignal::Event(event) = signal {
            match event.event.as_str() {
                "thread_change" => changes.push(event.data),
                "thread_item_tail" => tails.push(event.data),
                _ => {}
            }
        }
    }
    (changes, tails)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn completed_response_and_failure_detail_remain_separate_durable_items() {
    let (state, _dir) = build_test_state("trajectory_completion").await;
    let (queue_tx, mut queue_rx) = mpsc::channel(64);
    let emitter = EventEmitter::new(
        "fixture-thread".into(),
        "fixture-owner".into(),
        Some(queue_tx),
        Some(state.storage.clone()),
        state.monitor.clone(),
        false,
        None,
    )
    .with_committer(state.kernel.orchestrator.committer.clone());
    let turn = start_turn(&state, "fixture-thread", "fixture-owner").await;
    emitter.bind_turn(&turn, 1);
    let before = state
        .storage
        .latest_thread_change_seq_by_session("fixture-thread")
        .unwrap();
    emitter.emit("llm_request", json!({"model_round":1})).await;
    emitter
        .emit(
            "llm_stream_retry",
            json!({"model_round":1,"attempt":1,"max_attempts":3,
        "error":"fixture transient error","will_retry":true}),
        )
        .await;
    assert_eq!(
        state
            .storage
            .latest_thread_change_seq_by_session("fixture-thread")
            .unwrap(),
        before,
        "in-flight request and retry are live only"
    );
    emitter
        .emit(
            "llm_output",
            json!({"model_round":1,"content":"response","reasoning":"thought"}),
        )
        .await;
    let item_id = format!("{turn}:text-1");
    let completed = state
        .storage
        .get_thread_item("fixture-owner", "fixture-thread", &item_id, true)
        .unwrap()
        .unwrap();
    emitter
        .emit(
            "turn_terminal",
            json!({"model_round":1,"status":"failed","error":{
        "code":"UPSTREAM_TIMEOUT","message":"fixture provider timeout",
        "error_meta":{"source_stage":"llm","retryable":true,"recovery_action":"retry_later"}},
        "retry_after_ms":500}),
        )
        .await;
    state
        .storage
        .commit_thread_item(
            "fixture-owner",
            &json!({"session_id":"fixture-thread",
        "turn_id":turn,"item_id":item_id,"kind":"assistant_message","status":"completed",
        "content":"late replacement"}),
        )
        .unwrap();
    assert_eq!(
        state
            .storage
            .get_thread_item("fixture-owner", "fixture-thread", &item_id, true)
            .unwrap()
            .unwrap(),
        completed,
        "completed bodies are immutable"
    );
    let page = state
        .storage
        .thread_view_page("fixture-owner", "fixture-thread", "trajectory", None, 20)
        .unwrap();
    let assistant = page
        .trajectory
        .iter()
        .find(|row| row.kind == "assistant_message")
        .unwrap();
    assert_eq!(assistant.status, "completed");
    let failure = page
        .trajectory
        .iter()
        .find(|row| row.kind == "terminal")
        .unwrap();
    assert_eq!(failure.status, "failed");
    let diagnostic = &failure.diagnostics.as_ref().unwrap()["failure"];
    assert_eq!(diagnostic["code"], "UPSTREAM_TIMEOUT");
    assert_eq!(diagnostic["source_stage"], "llm");
    assert_eq!(diagnostic["retryable"], true);
    let mut retry_seen = false;
    while let Ok(StreamSignal::Event(event)) = queue_rx.try_recv() {
        retry_seen |= event.event == "llm_stream_retry";
    }
    assert!(
        retry_seen,
        "retry diagnostics remain visible on the live stream"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_emits_keep_change_cursor_order_in_queue() {
    let (state, _dir) = build_test_state("change_stream_order").await;
    let (queue_tx, mut queue_rx) = mpsc::channel::<StreamSignal>(512);
    let emitter = EventEmitter::new(
        "session-a".into(),
        "user-a".into(),
        Some(queue_tx),
        Some(state.storage.clone()),
        state.monitor.clone(),
        false,
        None,
    )
    .with_committer(state.kernel.orchestrator.committer.clone());
    let turn = start_turn(&state, "session-a", "user-a").await;
    emitter.bind_turn(&turn, 1);

    let mut tasks = Vec::new();
    for i in 0..6 {
        let worker = emitter.clone();
        let turn = turn.clone();
        tasks.push(tokio::spawn(async move {
            for j in 0..16 {
                worker
                    .emit(
                        "tool_result",
                        json!({
                            "turn_id": turn,
                            "model_round": 1,
                            "tool_call_id": format!("call_{i}_{j}"),
                            "name": "tool", "ok":true,
                        }),
                    )
                    .await;
            }
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    emitter.finish().await;

    let (changes, tails) = drain_changes(&mut queue_rx).await;
    assert!(tails.is_empty(), "v1 emitter must not receive tail frames");
    // Emit-path frames nest the payload under `data` (enrich_event_payload);
    // feeder frames carry it flat. The client adapter accepts both.
    let cursor_of = |data: &Value| -> Option<i64> {
        data["cursor"]
            .as_i64()
            .or_else(|| data["data"]["cursor"].as_i64())
    };
    let mut cursors: Vec<i64> = changes.iter().filter_map(cursor_of).collect();
    let total = cursors.len();
    assert!(
        total >= 96,
        "every committed item must surface a change frame (changes={} cursors={total})",
        changes.len()
    );
    assert!(
        cursors.windows(2).all(|pair| pair[0] < pair[1]),
        "wire order must match durable commit order"
    );
    cursors.sort();
    cursors.dedup();
    assert_eq!(
        cursors.len(),
        changes.iter().filter(|d| cursor_of(d).is_some()).count(),
        "change cursors must be unique across concurrent emitters"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_turn_commit_publishes_latest_durable_cursor() {
    let (state, _dir) = build_test_state("change_stream_turn_publish").await;
    let turn = accept_turn(&state, "session-a", "user-a");
    let mut wake = state.kernel.orchestrator.change_hub.subscribe("session-a");

    let changed = state
        .kernel
        .orchestrator
        .committer
        .update_turn(
            "user-a",
            "session-a",
            &turn,
            "running",
            "",
            &json!({"status":"running"}),
        )
        .await
        .expect("commit turn status");
    assert!(changed);

    tokio::time::timeout(std::time::Duration::from_secs(1), wake.changed())
        .await
        .expect("hub wake")
        .expect("hub open");
    assert_eq!(
        *wake.borrow(),
        state
            .storage
            .latest_thread_change_seq_by_session("session-a")
            .expect("latest durable cursor")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subagent_message_replay_uses_one_stable_durable_item() {
    let (state, _dir) = build_test_state("change_stream_subagent_message").await;
    let (queue_tx, mut queue_rx) = mpsc::channel::<StreamSignal>(64);
    let emitter = EventEmitter::new(
        "session-a".into(),
        "user-a".into(),
        Some(queue_tx),
        Some(state.storage.clone()),
        state.monitor.clone(),
        false,
        None,
    )
    .with_committer(state.kernel.orchestrator.committer.clone());
    let turn = start_turn(&state, "session-a", "user-a").await;
    emitter.bind_turn(&turn, 1);
    let data = json!({
        "message_id":"mail-1", "source_session_id":"child-a",
        "session_id":"session-a", "kind":"completion", "message":"result",
        "delivery":"applied", "model_round":1
    });
    emitter.emit("subagent_message", data.clone()).await;
    let after_first = state
        .storage
        .latest_thread_change_seq_by_session("session-a")
        .expect("first durable cursor");
    emitter.emit("subagent_message", data).await;
    assert_eq!(
        state
            .storage
            .latest_thread_change_seq_by_session("session-a")
            .expect("replayed durable cursor"),
        after_first,
        "identical mailbox delivery must be a durable no-op"
    );

    let (queued, _tails) = drain_changes(&mut queue_rx).await;
    assert_eq!(
        queued
            .iter()
            .filter(|frame| frame["data"]["item_id"] == json!(format!("{turn}:subagent-mail-1")))
            .count(),
        1,
        "only the first commit may produce a live durable receipt"
    );
    let changes = state
        .storage
        .list_thread_changes_by_session("session-a", 0, 100)
        .expect("load immutable change payloads");
    let durable_messages: Vec<_> = changes
        .iter()
        .filter(|change| {
            change["change_type"] == "item_upsert"
                && change["payload"]["kind"] == "subagent_message"
        })
        .collect();
    assert_eq!(durable_messages.len(), 1);
    let payload = &durable_messages[0]["payload"];
    assert_eq!(payload["item_id"], json!(format!("{turn}:subagent-mail-1")));
    assert_eq!(payload["payload"]["message"], "result");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_text_deltas_never_append_completed_history() {
    let (state, _dir) = build_test_state("change_stream_tail").await;
    let (queue_tx, mut queue_rx) = mpsc::channel::<StreamSignal>(256);
    let emitter = EventEmitter::new(
        "session-a".into(),
        "user-a".into(),
        Some(queue_tx),
        Some(state.storage.clone()),
        state.monitor.clone(),
        false,
        None,
    )
    .with_committer(state.kernel.orchestrator.committer.clone());
    let turn = start_turn(&state, "session-a", "user-a").await;
    emitter.bind_turn(&turn, 1);
    emitter.record_model_request(1);
    emitter.record_account_credit_consumption(1);

    emitter
        .emit("llm_request", json!({"turn_id": turn, "model_round": 1}))
        .await;
    emitter
        .emit(
            "llm_output_delta",
            json!({"turn_id": turn, "model_round": 1, "delta": "hé😀"}),
        )
        .await;
    emitter
        .emit(
            "llm_output_delta",
            json!({"turn_id": turn, "model_round": 1, "delta": "世界"}),
        )
        .await;
    emitter
        .emit(
            "llm_output_delta",
            json!({"turn_id": turn, "model_round": 1, "reasoning_delta": "思考"}),
        )
        .await;
    emitter.finish().await;
    let mut deltas = Vec::new();
    let mut tails = Vec::new();
    let mut durable = 0;
    while let Ok(StreamSignal::Event(event)) = queue_rx.try_recv() {
        if event.event == "thread_item_delta" {
            deltas.push(event.data["data"].clone());
        }
        if event.event == "thread_item_tail" {
            tails.push(event.data["data"].clone());
        }
        if event.event == "thread_change" {
            durable += 1;
        }
    }
    assert_eq!(durable, 0);
    assert_eq!(deltas.len(), 3);
    assert_eq!(deltas[0]["source_event"], "llm_output_delta");
    assert_eq!(deltas[0]["delta"], "hé😀");
    assert_eq!(deltas[1]["delta"], "世界");
    assert_eq!(deltas[2]["reasoning_delta"], "思考");
    assert_eq!(tails.len(), 3);
    assert_eq!(tails[0]["item_id"], json!(format!("{turn}:text-1")));
    assert_eq!(tails[0]["field"], "content");
    assert_eq!(tails[0]["offset"], 0);
    // The astral character makes this four UTF-16 code units rather than the
    // three Rust Unicode scalar values; the browser uses this cursor.
    assert_eq!(tails[1]["field"], "content");
    assert_eq!(tails[1]["offset"], 4);
    assert_eq!(tails[1]["text"], "世界");
    assert_eq!(tails[2]["field"], "reasoning");
    assert_eq!(tails[2]["offset"], 0);
    assert_eq!(tails[2]["text"], "思考");
    assert!(state
        .storage
        .get_thread_item("user-a", "session-a", &format!("{turn}:text-1"), true)
        .unwrap()
        .is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn work_process_stream_admits_tools_before_results_with_stable_order() {
    let (state, _dir) = build_test_state("process_order").await;
    let (queue_tx, mut queue_rx) = mpsc::channel::<StreamSignal>(32);
    let emitter = EventEmitter::new(
        "fixture-thread".into(),
        "fixture-owner".into(),
        Some(queue_tx),
        None,
        state.monitor.clone(),
        false,
        None,
    );
    emitter.bind_turn("fixture-turn", 1);
    for (event, data) in [
        ("llm_request", json!({"model_round":1})),
        (
            "llm_output",
            json!({"model_round":1,"reasoning":"Fixture thought"}),
        ),
        (
            "tool_call",
            json!({"model_round":1,"tool_call_id":"fixture-first","tool":"read_file","args":{"path":"fixture.txt"}}),
        ),
        (
            "tool_call",
            json!({"model_round":1,"tool_call_id":"fixture-second","tool":"read_file"}),
        ),
        (
            "tool_result",
            json!({"model_round":1,"tool_call_id":"fixture-second","tool":"read_file","ok":true}),
        ),
        (
            "tool_result",
            json!({"model_round":1,"tool_call_id":"fixture-first","tool":"read_file","ok":true}),
        ),
    ] {
        emitter.emit(event, data).await;
    }
    let mut items = Vec::new();
    while let Ok(StreamSignal::Event(event)) = queue_rx.try_recv() {
        if event.event == "thread_process_item" {
            items.push(event.data["data"].clone());
        }
    }
    assert_eq!(items.len(), 5);
    assert_eq!(items[1]["status"], "running");
    assert_eq!(items[1]["item_id"], items[4]["item_id"]);
    assert_eq!(items[2]["item_id"], items[3]["item_id"]);
    assert_eq!(
        items[1]["payload"]["process_seq"],
        items[4]["payload"]["process_seq"]
    );
    assert!(
        items[1]["payload"]["process_seq"].as_i64().unwrap()
            < items[2]["payload"]["process_seq"].as_i64().unwrap()
    );
    assert!(items
        .iter()
        .all(|item| serde_json::to_vec(item).unwrap().len() < 8192));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn work_process_observers_receive_running_tools_and_thought_tails_without_durable_completion() {
    let (state, _dir) = build_test_state("process_observer").await;
    let mut observer = state.kernel.orchestrator.change_hub.subscribe_live("fixture-thread");
    let emitter = EventEmitter::new("fixture-thread".into(), "fixture-owner".into(), None, None,
        state.monitor.clone(), false, None).with_committer(state.kernel.orchestrator.committer.clone());
    emitter.bind_turn("fixture-turn", 1);
    emitter.emit("llm_output_delta", json!({"model_round":1,"reasoning_delta":"Fixture thought"})).await;
    emitter.emit("tool_call", json!({"model_round":1,"tool_call_id":"fixture-call","tool":"read_file"})).await;
    let tail = observer.try_recv().unwrap();
    let tool = observer.try_recv().unwrap();
    assert_eq!(tail.event, "thread_item_tail");
    assert_eq!(tail.data["data"]["text"], "Fixture thought");
    assert_eq!(tool.event, "thread_process_item");
    assert_eq!(tool.data["data"]["status"], "running");
    assert_eq!(tool.data["data"]["item_id"], "fixture-turn:tool-fixture-call");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn model_context_append_does_not_replace_completed_stream_item() {
    let (state, _dir) = build_test_state("change_stream_terminal_item").await;
    let (queue_tx, mut queue_rx) = mpsc::channel::<StreamSignal>(256);
    let emitter = EventEmitter::new(
        "session-a".into(),
        "user-a".into(),
        Some(queue_tx),
        Some(state.storage.clone()),
        state.monitor.clone(),
        false,
        None,
    )
    .with_committer(state.kernel.orchestrator.committer.clone());
    let turn = start_turn(&state, "session-a", "user-a").await;
    emitter.bind_turn(&turn, 1);
    emitter
        .emit("llm_request", json!({"turn_id": turn, "model_round": 1}))
        .await;

    emitter
        .emit(
            "llm_output",
            json!({"model_round":1,"content":"done","reasoning":"thinking"}),
        )
        .await;
    let round = RoundInfo::new(1, 1);
    let stats = json!({"message_stats": {
        "interaction_duration_s": 1.25,
        "visible_decode_speed_tps": 42.0
    }});
    let orchestrator = &state.kernel.orchestrator;
    orchestrator
        .append_chat(
            "user-a",
            "session-a",
            "assistant",
            Some(&json!("done")),
            None,
            Some(&stats),
            Some("thinking"),
            None,
            None,
            None,
            RoundInfo {
                thread_turn_id: Some(Uuid::parse_str(&turn).unwrap()),
                ..round
            },
        )
        .await;

    let item = state
        .storage
        .get_thread_turn("user-a", "session-a", &turn, -1, 100, true)
        .unwrap()
        .unwrap()["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["item_id"] == format!("{turn}:text-1"))
        .cloned()
        .expect("stable assistant item");
    assert_eq!(item["payload"]["content"], "done");
    assert_eq!(item["payload"]["reasoning"], "thinking");
    assert!(
        item["payload"]["meta"].is_null(),
        "model-context accounting cannot rewrite completed output"
    );
    assert_eq!(
        state
            .storage
            .get_thread_turn("user-a", "session-a", &turn, -1, 100, true)
            .unwrap()
            .unwrap()["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["kind"] == "assistant_message")
            .count(),
        1
    );
    while queue_rx.try_recv().is_ok() {}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn completed_output_projects_context_and_quota_stats_without_history_rewrite() {
    let (state, _dir) = build_test_state("change_stream_output_stats").await;
    let (queue_tx, mut queue_rx) = mpsc::channel::<StreamSignal>(64);
    let emitter = EventEmitter::new(
        "session-a".into(),
        "user-a".into(),
        Some(queue_tx),
        Some(state.storage.clone()),
        state.monitor.clone(),
        false,
        None,
    )
    .with_committer(state.kernel.orchestrator.committer.clone());
    let turn = start_turn(&state, "session-a", "user-a").await;
    emitter.bind_turn(&turn, 1);
    // Production accounts for dispatch before llm_output. Emitting a completed
    // body alone must not invent a provider request or its billable credit.
    emitter.record_model_request(1);

    emitter
        .emit(
            "llm_output",
            json!({
                "model_round": 1,
                "content": "done",
                "usage": {
                    "input_tokens": 321,
                    "output_tokens": 34,
                    "total_tokens": 355,
                    "estimated": false
                },
                "prefill_duration_s": 0.2,
                "decode_duration_s": 0.8,
                "visible_decode_speed_tps": 42.5,
            }),
        )
        .await;

    let page = state
        .storage
        .thread_view_page("user-a", "session-a", "chat", None, 12)
        .expect("chat view");
    let stats = page.chat[0]
        .assistant
        .stats
        .as_ref()
        .expect("assistant bubble stats");
    assert_eq!(stats["context_occupancy_tokens"], json!(321));
    assert_eq!(stats["model_request_count"], json!(1));
    assert_eq!(stats["account_credits_consumed"], json!(1));
    assert_eq!(stats["toolCalls"], json!(0));
    assert_eq!(stats["tool_calls"], json!(0));
    assert_eq!(stats["usage"]["total_tokens"], json!(355));
    assert_eq!(stats["interaction_duration_s"], json!(1.0));
    while queue_rx.try_recv().is_ok() {}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn completed_output_preserves_actual_tool_starts_across_parallel_results_and_replay() {
    let (state, _dir) = build_test_state("change_stream_tool_stats").await;
    let emitter = EventEmitter::new(
        "session-a".into(),
        "user-a".into(),
        None,
        Some(state.storage.clone()),
        state.monitor.clone(),
        false,
        None,
    )
    .with_committer(state.kernel.orchestrator.committer.clone());
    let turn = start_turn(&state, "session-a", "user-a").await;
    emitter.bind_turn(&turn, 1);
    let first = emitter.clone();
    let second = emitter.clone();
    tokio::join!(
        async move {
            first.record_tool_call_started();
            first
                .emit(
                    "tool_call",
                    json!({"model_round":1,"tool_call_id":"sample-call-a","tool":"read_file"}),
                )
                .await;
        },
        async move {
            second.record_tool_call_started();
            second
                .emit(
                    "tool_call",
                    json!({"model_round":1,"tool_call_id":"sample-call-b","tool":"read_file"}),
                )
                .await;
        },
    );
    for _ in 0..2 {
        emitter.emit("tool_result", json!({"model_round":1,"tool_call_id":"sample-call-b","tool":"read_file","ok":false})).await;
        emitter.emit("tool_result", json!({"model_round":1,"tool_call_id":"sample-call-a","tool":"read_file","ok":true})).await;
    }
    for _ in 0..2 {
        let output = emitter.emit("llm_output", json!({"model_round":2,"content":"sample answer","usage":{"input_tokens":10,"output_tokens":2,"total_tokens":12}})).await;
        assert_eq!(
            output.data.pointer("/data/message_stats/toolCalls"),
            Some(&json!(2))
        );
        let final_event = emitter
            .emit("final", json!({"answer":"sample answer"}))
            .await;
        assert_eq!(
            final_event.data.pointer("/data/tool_calls"),
            Some(&json!(2))
        );
        let page = state
            .storage
            .thread_view_page("user-a", "session-a", "chat", None, 12)
            .unwrap();
        let stats = page.chat[0].assistant.stats.as_ref().unwrap();
        assert_eq!(stats["toolCalls"], json!(2));
        assert_eq!(stats["tool_calls"], json!(2));
    }
    assert_eq!(emitter.accumulated_tool_calls(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interleaved_model_deltas_keep_round_identity_without_durable_registration() {
    let (state, _dir) = build_test_state("change_stream_interleaved_tails").await;
    let (queue_tx, mut queue_rx) = mpsc::channel::<StreamSignal>(256);
    let emitter = EventEmitter::new(
        "session-a".into(),
        "user-a".into(),
        Some(queue_tx),
        Some(state.storage.clone()),
        state.monitor.clone(),
        false,
        None,
    )
    .with_committer(state.kernel.orchestrator.committer.clone());
    let turn = start_turn(&state, "session-a", "user-a").await;
    emitter.bind_turn(&turn, 1);

    for model_round in [1, 2] {
        emitter
            .emit(
                "llm_request",
                json!({"turn_id": turn, "model_round": model_round}),
            )
            .await;
    }
    for (model_round, delta) in [(1, "甲"), (2, "乙"), (1, "丙"), (2, "丁")] {
        emitter
            .emit(
                "llm_output_delta",
                json!({"turn_id": turn, "model_round": model_round, "delta": delta}),
            )
            .await;
    }
    emitter.finish().await;

    let mut deltas = Vec::new();
    while let Ok(StreamSignal::Event(event)) = queue_rx.try_recv() {
        assert_ne!(event.event, "thread_change");
        if event.event == "thread_item_delta" {
            deltas.push(event.data["data"].clone());
        }
    }
    assert_eq!(deltas.len(), 4);
    for (data, (model_round, text)) in
        deltas
            .iter()
            .zip([(1, "甲"), (2, "乙"), (1, "丙"), (2, "丁")])
    {
        assert_eq!(data["model_round"], model_round);
        assert_eq!(data["delta"], text);
        assert_eq!(data["turn_id"], turn);
    }
    for model_round in [1, 2] {
        assert!(state
            .storage
            .get_thread_item(
                "user-a",
                "session-a",
                &format!("{turn}:text-{model_round}"),
                true
            )
            .unwrap()
            .is_none());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn thread_change_frames_embed_item_payloads() {
    let (state, _dir) = build_test_state("change_stream_embed").await;
    state
        .user_store
        .upsert_chat_session(&crate::storage::ChatSessionRecord {
            session_id: "session-a".into(),
            user_id: "user-a".into(),
            title: "测试会话".into(),
            status: "active".into(),
            created_at: 0.0,
            updated_at: 0.0,
            last_message_at: 0.0,
            agent_id: None,
            workspace_id: None,
            tool_overrides: Vec::new(),
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        })
        .unwrap();
    let turn = start_turn(&state, "session-a", "user-a").await;

    let frames = state
        .workspace
        .try_load_thread_changes("session-a", 0, 100)
        .unwrap();
    let user_frame = frames
        .iter()
        .find(|frame| {
            frame["event"] == "thread_change"
                && frame["data"]["change_type"] == "item_upsert"
                && frame["data"]["item_id"] == json!(format!("{turn}:user"))
        })
        .expect("user item change must be replayed");
    let item = &user_frame["data"]["payload"];
    assert_eq!(item["kind"], "user_message");
    assert_eq!(item["payload"]["role"], "user");
    assert_eq!(item["payload"]["content"], "你好");
    assert!(user_frame["data"]["cursor"].as_i64().unwrap() > 0);

    // A cursor beyond the session's latest change seq must demand a snapshot.
    let latest = state
        .storage
        .latest_thread_change_seq_by_session("session-a")
        .unwrap();
    let ahead = state
        .workspace
        .try_load_thread_changes("session-a", latest + 5, 100)
        .unwrap();
    assert_eq!(ahead.len(), 1);
    assert_eq!(ahead[0]["event"], "thread_snapshot_required");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn feedback_commit_wakes_display_without_rewriting_canonical_history() {
    let (state, _dir) = build_test_state("change_stream_feedback").await;
    let turn = start_turn(&state, "session-a", "user-a").await;
    let item_id = format!("{turn}:answer");
    state
        .kernel
        .orchestrator
        .committer
        .commit_item(
            "user-a",
            &json!({
                "session_id":"session-a", "turn_id":turn, "item_id":item_id,
                "kind":"assistant_message", "status":"completed", "visibility":"user",
                "role":"assistant", "content":"ok", "user_round":1
            }),
        )
        .await
        .expect("commit assistant item")
        .expect("item receipt");
    let mut wake = state.kernel.orchestrator.change_hub.subscribe("session-a");

    let feedback = state
        .kernel
        .orchestrator
        .committer
        .set_feedback("user-a", "session-a", &item_id, "up")
        .await
        .expect("feedback commit")
        .expect("first feedback accepted");
    assert_eq!(feedback["vote"], json!("up"));
    // The unified commit exit publishes the durable cursor after success.

    tokio::time::timeout(std::time::Duration::from_secs(1), wake.changed())
        .await
        .expect("hub wake")
        .expect("hub open");
    assert_eq!(
        *wake.borrow(),
        state
            .storage
            .latest_thread_change_seq_by_session("session-a")
            .expect("latest durable cursor")
    );

    // Feedback is mutable display metadata; the canonical completed item and
    // its cursor/revision remain exactly as originally committed.
    let changes = state
        .storage
        .list_thread_changes_by_session("session-a", 0, 100)
        .unwrap();
    let feedback_change = changes
        .iter()
        .rev()
        .find(|change| {
            change["change_type"] == "item_upsert" && change["item_id"] == json!(item_id)
        })
        .expect("feedback item change");
    assert_eq!(feedback_change["revision"], json!(1));
    let payload = &feedback_change["payload"];
    assert_eq!(payload["kind"], json!("assistant_message"));
    assert!(payload["payload"]["feedback"].is_null());
    assert_eq!(
        state
            .storage
            .get_thread_item("user-a", "session-a", &item_id, false)
            .unwrap()
            .unwrap()["payload"]["feedback"]["vote"],
        "up"
    );

    // A repeated vote is a no-op: no new change and no wake.
    let after = state
        .storage
        .latest_thread_change_seq_by_session("session-a")
        .unwrap();
    let noop = state
        .kernel
        .orchestrator
        .committer
        .set_feedback("user-a", "session-a", &item_id, "down")
        .await
        .expect("feedback commit");
    assert!(noop.is_none());
    assert_eq!(
        state
            .storage
            .latest_thread_change_seq_by_session("session-a")
            .unwrap(),
        after
    );
    assert!(
        !wake.has_changed().expect("wake watch open"),
        "no-op feedback must not wake feeders"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn completed_llm_output_waits_for_runner_to_close_turn_and_user_bubble() {
    let (state, _dir) = build_test_state("change_stream_terminal_reconcile").await;
    let (queue_tx, _queue_rx) = mpsc::channel::<StreamSignal>(128);
    let emitter = EventEmitter::new(
        "session-a".into(),
        "user-a".into(),
        Some(queue_tx),
        Some(state.storage.clone()),
        state.monitor.clone(),
        false,
        None,
    )
    .with_committer(state.kernel.orchestrator.committer.clone());
    let turn = start_turn(&state, "session-a", "user-a").await;
    emitter.bind_turn(&turn, 1);

    emitter
        .emit(
            "llm_output",
            json!({
                "turn_id": turn,
                "model_round": 1,
                "content": "已完成",
                "reasoning": "",
                "tool_calls": [],
            }),
        )
        .await;

    let pending = state
        .storage
        .thread_view_changes("user-a", "session-a", 0)
        .unwrap();
    assert_eq!(
        pending.runtime_status, "running",
        "model action completion is not a user turn boundary"
    );
    state
        .kernel
        .orchestrator
        .committer
        .update_turn(
            "user-a",
            "session-a",
            &turn,
            "completed",
            "",
            &json!({"status":"completed"}),
        )
        .await
        .unwrap();

    let turns = state
        .storage
        .list_thread_turns("user-a", "session-a", None, 10)
        .expect("refresh turns");
    assert_eq!(turns[0]["status"], json!("completed"));
    let detail = state
        .storage
        .get_thread_turn_page("user-a", "session-a", &turn, None, None, 100, true)
        .expect("load turn")
        .expect("turn exists");
    let user_item = detail["items"]
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|item| item["item_id"] == json!(format!("{turn}:user")))
        })
        .expect("user bubble");
    assert_eq!(user_item["status"], json!("completed"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn refresh_keeps_running_until_canonical_turn_terminal() {
    let (state, _dir) = build_test_state("change_stream_terminal_recovery").await;
    let turn = start_turn(&state, "session-a", "user-a").await;
    // Simulate the historical failure window: the final assistant item was
    // durable, but the follow-up turn update never ran.
    state
        .storage
        .append_thread_item(
            "user-a",
            &json!({
                "session_id":"session-a", "turn_id":turn,
                "item_id":format!("{turn}:text-1"),
                "kind":"assistant_message", "event_type":"llm_output",
                "status":"completed", "visibility":"user", "content":"已完成",
                "tool_calls":[]
            }),
        )
        .expect("append historical final item");

    let turns = state
        .storage
        .list_thread_turns("user-a", "session-a", None, 10)
        .expect("refresh turns");
    assert_eq!(turns[0]["status"], json!("running"));
    assert_eq!(
        state
            .storage
            .thread_view_page("user-a", "session-a", "chat", None, 12)
            .unwrap()
            .runtime_status,
        "running"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admission_does_not_freeze_prompt_and_session_meta_keeps_initial_instructions() {
    use wunder_core::thread_store::ThreadId;
    let (state, _dir) = build_test_state("change_stream_system_prompt").await;
    let turn = accept_turn(&state, "session-a", "user-a");
    let thread = ThreadId::new("session-a").unwrap();
    assert!(state.thread_store.session_meta(&thread).await.is_err());
    assert_eq!(
        state
            .storage
            .latest_thread_change_seq_by_session("session-a")
            .unwrap(),
        0
    );
    let committer = &state.kernel.orchestrator.committer;
    assert_eq!(
        committer
            .freeze_session(
                "user-a",
                "session-a",
                "fixture first prompt",
                "fixture-workspace"
            )
            .await
            .unwrap(),
        "fixture first prompt"
    );
    let after_freeze = state
        .storage
        .latest_thread_change_seq_by_session("session-a")
        .unwrap();
    assert_eq!(
        committer
            .freeze_session(
                "user-a",
                "session-a",
                "different later prompt",
                "fixture-workspace"
            )
            .await
            .unwrap(),
        "fixture first prompt"
    );
    assert_eq!(
        state
            .storage
            .latest_thread_change_seq_by_session("session-a")
            .unwrap(),
        after_freeze
    );
    let meta = state.thread_store.session_meta(&thread).await.unwrap();
    assert_eq!(meta.initial_system_prompt, "fixture first prompt");
    committer
        .begin_turn("user-a", "session-a", &turn, "fixture-model")
        .await
        .unwrap();
    // Completed display history never contains a second system prompt item.
    let changes = state
        .storage
        .list_thread_changes_by_session("session-a", 0, 100)
        .unwrap();
    assert!(!changes
        .iter()
        .any(|change| change["payload"]["kind"] == "system_message"));
}
