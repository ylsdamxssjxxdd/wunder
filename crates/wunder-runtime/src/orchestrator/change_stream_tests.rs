use super::*;
use crate::state::{AppState, AppStateInitOptions};
use event_stream::StreamSignal;

async fn build_test_state(name: &str) -> (Arc<AppState>, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.storage.backend = "sqlite".into();
    config.storage.db_path = root
        .path()
        .join(format!("{name}.db"))
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_emits_keep_change_cursor_order_in_queue() {
    let (state, _dir) = build_test_state("change_stream_order").await;
    let (queue_tx, mut queue_rx) = mpsc::channel::<StreamSignal>(STREAM_EVENT_QUEUE_SIZE);
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
    let turn = accept_turn(&state, "session-a", "user-a");
    emitter.bind_turn(&turn, 1);

    let mut tasks = Vec::new();
    for i in 0..6 {
        let worker = emitter.clone();
        let turn = turn.clone();
        tasks.push(tokio::spawn(async move {
            for j in 0..16 {
                worker
                    .emit(
                        "tool_call",
                        json!({
                            "turn_id": turn,
                            "model_round": 1,
                            "tool_call_id": format!("call_{i}_{j}"),
                            "name": "tool",
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
    eprintln!("debug: changes={} cursors={total}", changes.len());
    assert!(
        total >= 96,
        "every committed item must surface a change frame (changes={} cursors={total})",
        changes.len()
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
    let turn = accept_turn(&state, "session-a", "user-a");
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
async fn change_stream_emits_tail_frames_with_utf16_offsets() {
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
    let turn = accept_turn(&state, "session-a", "user-a");
    emitter.bind_turn(&turn, 1);

    emitter
        .emit("llm_request", json!({"turn_id": turn, "model_round": 1}))
        .await;
    emitter
        .emit(
            "llm_output_delta",
            json!({"turn_id": turn, "model_round": 1, "delta": "héllo"}),
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

    let (_changes, tails) = drain_changes(&mut queue_rx).await;
    assert_eq!(tails.len(), 3, "one tail frame per emitted field delta");
    assert_eq!(tails[0]["item_id"], json!(format!("{turn}:text-1")));
    assert_eq!(tails[0]["field"], "content");
    assert_eq!(tails[0]["offset"], 0);
    assert_eq!(tails[0]["text"], "héllo");
    // "héllo" is 5 UTF-16 code units; the next content delta appends there.
    assert_eq!(tails[1]["field"], "content");
    assert_eq!(tails[1]["offset"], 5);
    assert_eq!(tails[1]["text"], "世界");
    assert_eq!(tails[2]["field"], "reasoning");
    assert_eq!(tails[2]["offset"], 0);
    assert_eq!(tails[2]["text"], "思考");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn assistant_terminal_history_upserts_the_registered_stream_item() {
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
    let turn = accept_turn(&state, "session-a", "user-a");
    emitter.bind_turn(&turn, 1);
    emitter
        .emit("llm_request", json!({"turn_id": turn, "model_round": 1}))
        .await;

    let round = RoundInfo::new(1, 1);
    let stats = json!({"message_stats": {
        "interaction_duration_s": 1.25,
        "visible_decode_speed_tps": 42.0
    }});
    let orchestrator = &state.kernel.orchestrator;
    orchestrator.append_chat(
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
    );
    state.workspace.flush_writes();

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
    assert_eq!(
        item["payload"]["meta"]["message_stats"]["visible_decode_speed_tps"],
        42.0
    );
    assert_eq!(item["payload"]["reasoning_content"], "thinking");
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
async fn interleaved_model_tails_keep_item_offsets_and_durable_bases() {
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
    let turn = accept_turn(&state, "session-a", "user-a");
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

    let (changes, tails) = drain_changes(&mut queue_rx).await;
    assert_eq!(tails.len(), 4);
    assert_eq!(tails[0]["item_id"], json!(format!("{turn}:text-1")));
    assert_eq!(tails[0]["offset"], 0);
    assert_eq!(tails[1]["item_id"], json!(format!("{turn}:text-2")));
    assert_eq!(tails[1]["offset"], 0);
    assert_eq!(tails[2]["item_id"], json!(format!("{turn}:text-1")));
    assert_eq!(tails[2]["offset"], 1);
    assert_eq!(tails[3]["item_id"], json!(format!("{turn}:text-2")));
    assert_eq!(tails[3]["offset"], 1);

    let cursor_of = |data: &Value| -> Option<i64> {
        data["cursor"]
            .as_i64()
            .or_else(|| data["data"]["cursor"].as_i64())
    };
    for model_round in [1, 2] {
        let item_id = format!("{turn}:text-{model_round}");
        let item_cursor = changes
            .iter()
            .find(|change| {
                let data = change.get("data").unwrap_or(change);
                data["change_type"] == "item_upsert" && data["item_id"] == item_id
            })
            .and_then(cursor_of)
            .expect("registered item durable change");
        // The first flush can advance the dependency to its text_block cursor.
        // In every case the tail must wait for a durable frame of the same
        // item and never inherit the other model round's cursor.
        assert!(tails
            .iter()
            .filter(|tail| tail["item_id"] == item_id)
            .all(|tail| tail["base_seq"]
                .as_i64()
                .is_some_and(|seq| seq >= item_cursor)));
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
    let turn = accept_turn(&state, "session-a", "user-a");

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
async fn feedback_commit_wakes_feeder_with_snapshot_payload() {
    let (state, _dir) = build_test_state("change_stream_feedback").await;
    let turn = accept_turn(&state, "session-a", "user-a");
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

    // The change payload must be the committed snapshot: it carries the locked
    // feedback and never reads the mutable current item on replay.
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
    assert_eq!(feedback_change["revision"], json!(2));
    let payload = &feedback_change["payload"];
    assert_eq!(payload["kind"], json!("assistant_message"));
    assert_eq!(payload["payload"]["feedback"]["vote"], json!("up"));

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
async fn freeze_system_prompt_append_wakes_feeder_with_snapshot() {
    let (state, _dir) = build_test_state("change_stream_system_prompt").await;
    let turn = accept_turn(&state, "session-a", "user-a");
    let mut wake = state.kernel.orchestrator.change_hub.subscribe("session-a");

    let item = state
        .workspace
        .build_session_system_prompt_item("user-a", "session-a", "你是测试助手。", Some("zh"))
        .expect("build system prompt item")
        .expect("system prompt item");
    // The unified commit exit persists the append and publishes the receipt
    // cursor only on the real commit.
    state
        .kernel
        .orchestrator
        .committer
        .append_item("user-a", &item)
        .await
        .expect("freeze system prompt");

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

    let changes = state
        .storage
        .list_thread_changes_by_session("session-a", 0, 100)
        .unwrap();
    let prompt_change = changes
        .iter()
        .find(|change| {
            change["change_type"] == "item_upsert"
                && change["item_id"] == json!(format!("session-a:system-prompt"))
        })
        .expect("system prompt change");
    let payload = &prompt_change["payload"];
    assert_eq!(payload["kind"], json!("system_message"));
    assert_eq!(payload["turn_id"], json!(turn));
    assert_eq!(payload["payload"]["content"], json!("你是测试助手。"));
}
