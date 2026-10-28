use super::*;
use crate::state::{AppState, AppStateInitOptions};
use axum::{routing::post, Json, Router};
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn request_execution_future_sizes_stay_within_stack_budget() {
    let root = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.server.mode = "cli".into();
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
    let orchestrator = &state.kernel.orchestrator;
    let request: WunderRequest = serde_json::from_value(json!({
        "user_id":"fixture-user","session_id":"fixture-thread","question":"fixture input"
    }))
    .unwrap();
    let prepared = PreparedRequest {
        user_id: request.user_id.clone(),
        workspace_id: "fixture-workspace".into(),
        question: request.question.clone(),
        client_message_id: None,
        session_id: "fixture-thread".into(),
        tool_names: None,
        skip_tool_calls: false,
        model_name: None,
        config_overrides: None,
        agent_prompt: None,
        preview_skill: false,
        agent_id: None,
        stream: false,
        attachments: None,
        language: "en".into(),
        allow_queue: false,
        is_admin: true,
        enforce_runtime_queue: false,
        approval_tx: None,
        thread_turn_id: Some(Uuid::new_v4()),
        thread_user_round: Some(1),
        thread_resume_from_seq: 0,
    };
    let emitter = EventEmitter::new(
        prepared.session_id.clone(),
        prepared.user_id.clone(),
        None,
        Some(state.storage.clone()),
        state.monitor.clone(),
        true,
        None,
    );
    // Measure unpolled concrete futures, before any caller can hide their size
    // behind a Box. A small task wrapper alone cannot protect a giant action's
    // construction/poll frames on the Windows default stack.
    let execution = orchestrator.execute_request(prepared, emitter);
    let run = orchestrator.run(request.clone());
    let stream = orchestrator.stream(request);
    for (name, bytes, budget) in [
        (
            "execute_request",
            std::mem::size_of_val(&execution),
            128 * 1024,
        ),
        ("run", std::mem::size_of_val(&run), 64 * 1024),
        ("stream", std::mem::size_of_val(&stream), 64 * 1024),
    ] {
        assert!(
            bytes <= budget,
            "{name} future uses {bytes} bytes; budget is {budget}"
        );
    }
}

// Exercise the real runner on ordinary test/Tokio stacks. Increasing the stack
// here would hide future-size regressions that crash the Windows service.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn thread_lifecycle_stream_commits_file_and_terminal_before_final() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let app = Router::new().route("/v1/chat/completions", post(move |Json(request): Json<Value>| {
        let index = counter.fetch_add(1, Ordering::SeqCst);
        async move {
            let (message, reason) = if index == 0 {
                let name = request["tools"].as_array().unwrap().iter()
                    .find_map(|tool| tool.pointer("/function/name").and_then(Value::as_str)
                        .filter(|name| *name == "write_file")).expect("write_file exposed");
                (json!({"role":"assistant","tool_calls":[{"id":"fixture-call","type":"function",
                    "function":{"name":name,"arguments":json!({"path":"fixture-output.txt","content":"fixture output"}).to_string()}}]}), "tool_calls")
            } else {
                (json!({"role":"assistant","content":"fixture answer"}), "stop")
            };
            format!("data: {}\n\ndata: [DONE]\n\n", json!({"choices":[{"delta":message,"finish_reason":reason}],
                "usage":{"prompt_tokens":100,"completion_tokens":20,"total_tokens":120}}))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let root = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    // Exercise the real filesystem tool without depending on a remote sandbox.
    config.server.mode = "cli".into();
    config.storage.backend = "sqlite".into();
    config.storage.data_root = root.path().join("data").to_string_lossy().into_owned();
    config.workspace.root = root.path().join("workspace").to_string_lossy().into_owned();
    config.tools.builtin.enabled = vec![resolve_tool_name("write_file")];
    config.llm.default = "fixture-model".into();
    config.llm.models.insert(
        "fixture-model".into(),
        LlmModelConfig {
            provider: Some("openai_compatible".into()),
            model: Some("fixture-model".into()),
            base_url: Some(format!("http://{address}/v1")),
            max_rounds: Some(4),
            max_context: Some(131072),
            stream: Some(true),
            tool_call_mode: Some("function_call".into()),
            ..Default::default()
        },
    );
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
    let mut request: WunderRequest = serde_json::from_value(json!({
        "user_id":"fixture-user","session_id":"fixture-thread","question":"fixture input",
        "stream":true,"tool_names":["write_file"]
    }))
    .unwrap();
    request.is_admin = true;
    let stream = state.kernel.orchestrator.stream(request).await.unwrap();
    tokio::pin!(stream);
    let mut wrote = false;
    let mut final_seen = false;
    while let Some(event) = tokio::time::timeout(Duration::from_secs(30), stream.next())
        .await
        .unwrap()
    {
        let event = event.unwrap();
        if event.event == "workspace_update" {
            wrote = true;
            let workspace_id = event
                .data
                .pointer("/data/workspace_id")
                .and_then(Value::as_str)
                .unwrap();
            let path = state
                .workspace
                .resolve_path(workspace_id, "fixture-output.txt")
                .unwrap();
            assert_eq!(std::fs::read_to_string(path).unwrap(), "fixture output");
            let page = state
                .storage
                .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
                .unwrap();
            assert!(
                page.chat[0].workspace_change_seq > 0,
                "file invalidation survives queue/replay transport"
            );
        }
        if event.event == "final" {
            final_seen = true;
            // Changes reads do no repair. This asserts the actual runner commit,
            // rather than allowing a recovery read to make the test pass.
            let changes = state
                .storage
                .thread_view_changes("fixture-user", "fixture-thread", 0)
                .unwrap();
            assert_eq!(changes.runtime_status, "completed");
            assert!(changes.active_turn_id.is_empty());
            assert_eq!(changes.rows[0].assistant.content, "fixture answer");
            assert_eq!(
                event.data.pointer("/data/message_stats/toolCalls"),
                Some(&json!(1))
            );
            assert_eq!(
                changes.rows[0].assistant.stats.as_ref().unwrap()["toolCalls"],
                json!(1)
            );
        }
    }
    assert!(wrote && final_seen);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let turn = state
        .storage
        .list_thread_turns("fixture-user", "fixture-thread", None, 12)
        .unwrap();
    assert_eq!(turn[0]["status"], "completed");
    let page = state
        .storage
        .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
        .unwrap();
    assert_eq!(page.runtime_status, "completed");
    assert_eq!(page.chat[0].assistant.content, "fixture answer");
    assert_eq!(
        page.chat[0].assistant.stats.as_ref().unwrap()["toolCalls"],
        json!(1)
    );
    // Reopen through a fresh storage handle so the assertion cannot depend on
    // the execution's in-memory messages or live event snapshot.
    let reopened = crate::storage::thread_store::LocalThreadStore::open(
        state.kernel.orchestrator.thread_store.root(),
    )
    .unwrap();
    let context = reopened
        .model_messages(
            &wunder_core::thread_store::ThreadId::new("fixture-thread").unwrap(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(context.first().unwrap()["content"], "fixture input");
    assert!(context
        .iter()
        .any(|message| message["role"] == "assistant" && message.get("tool_calls").is_some()));
    assert!(context
        .iter()
        .any(|message| message["role"] == "tool" && message["tool_call_id"] == "fixture-call"));
    assert_eq!(context.last().unwrap()["content"], "fixture answer");
    let reopened_items = wunder_core::thread_store::ThreadHistoryReader::list_items(
        &reopened,
        wunder_core::thread_store::ItemListQuery {
            thread_id: wunder_core::thread_store::ThreadId::new("fixture-thread").unwrap(),
            turn_id: None,
            cursor: None,
            limit: 32,
        },
    )
    .await
    .unwrap();
    let reopened_answer = reopened_items
        .entries
        .iter()
        .find(|entry| {
            entry.item.kind == wunder_core::thread_store::CanonicalItemKind::AssistantMessage
                && entry.item.payload["content"] == "fixture answer"
        })
        .expect("completed answer survives reopening");
    let reopened_stats = reopened_answer
        .item
        .payload
        .get("message_stats")
        .or_else(|| reopened_answer.item.payload.pointer("/meta/message_stats"))
        .expect("completed answer retains invocation stats");
    assert_eq!(reopened_stats["toolCalls"], json!(1));
    assert_eq!(reopened_stats["tool_calls"], json!(1));
    server.abort();
}

async fn fixture_state(root: &tempfile::TempDir, mut config: Config) -> AppState {
    config.server.mode = "cli".into();
    config.storage.backend = "sqlite".into();
    config.storage.data_root = root.path().join("data").to_string_lossy().into_owned();
    config.workspace.root = root.path().join("workspace").to_string_lossy().into_owned();
    let store = ConfigStore::new(root.path().join("config.yaml"));
    store
        .update(|current| *current = config.clone())
        .await
        .unwrap();
    AppState::new_with_options(
        store,
        config,
        AppStateInitOptions::cli_default().with_start_thread_runtime(false),
    )
    .unwrap()
}

fn fixture_llm_config(config: &mut Config, address: &std::net::SocketAddr) {
    config.llm.default = "fixture-model".into();
    config.llm.models.insert(
        "fixture-model".into(),
        LlmModelConfig {
            provider: Some("openai_compatible".into()),
            model: Some("fixture-model".into()),
            base_url: Some(format!("http://{address}/v1")),
            max_rounds: Some(4),
            max_context: Some(131072),
            stream: Some(true),
            tool_call_mode: Some("function_call".into()),
            ..Default::default()
        },
    );
}

fn collect_stream(
    stream: impl Stream<Item = Result<StreamEvent, std::convert::Infallible>> + Send + 'static,
) -> (
    tokio::sync::mpsc::UnboundedReceiver<(String, Value)>,
    tokio::task::JoinHandle<()>,
) {
    let (events_tx, events_rx) = tokio::sync::mpsc::unbounded_channel();
    let collector = tokio::spawn(async move {
        tokio::pin!(stream);
        while let Some(event) = stream.next().await {
            let event = event.unwrap();
            let _ = events_tx.send((event.event, event.data));
        }
    });
    (events_rx, collector)
}

fn drain_events(
    events_rx: &mut tokio::sync::mpsc::UnboundedReceiver<(String, Value)>,
) -> Vec<(String, Value)> {
    let mut events = Vec::new();
    while let Ok(event) = events_rx.try_recv() {
        events.push(event);
    }
    events
}

fn assert_cancelled_settlement(state: &AppState, events: &[(String, Value)]) {
    assert!(
        events.iter().any(|(name, data)| name == "error"
            && data.pointer("/data/code") == Some(&json!("CANCELLED"))),
        "cancel must surface a CANCELLED error frame: {events:?}"
    );
    assert!(
        events.iter().any(|(name, data)| name == "turn_terminal"
            && data.pointer("/data/status") == Some(&json!("cancelled"))),
        "cancel must emit turn_terminal{{cancelled}}: {events:?}"
    );
    assert!(
        !events.iter().any(|(name, _)| name == "final"),
        "cancelled turn must not emit a final frame: {events:?}"
    );
    let turns = state
        .storage
        .list_thread_turns("fixture-user", "fixture-thread", None, 12)
        .unwrap();
    assert_eq!(turns[0]["status"], "cancelled");
    let page = state
        .storage
        .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
        .unwrap();
    assert_eq!(page.runtime_status, "cancelled");
    let record = state.monitor.get_record("fixture-thread").unwrap();
    assert_eq!(record["status"], json!("cancelled"));
}

// 停止受理落在模型调用挂起期间：执行体必须在取消检查点短路，按 cancelled 结清，
// 持久轮次、呈现面与监视器同语义，且不发出 final 帧。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_while_model_call_hangs_settles_cancelled() {
    let app = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            std::future::pending::<()>().await;
            #[allow(unreachable_code)]
            String::new()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let root = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    fixture_llm_config(&mut config, &address);
    let state = fixture_state(&root, config).await;
    let mut request: WunderRequest = serde_json::from_value(json!({
        "user_id":"fixture-user","session_id":"fixture-thread","question":"fixture input","stream":true
    }))
    .unwrap();
    request.is_admin = true;
    let stream = state.kernel.orchestrator.stream(request).await.unwrap();
    let (mut events_rx, collector) = collect_stream(stream);
    // 等执行体注册为 running（模型请求在途、挂起）。
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let running = state
            .monitor
            .get_record("fixture-thread")
            .and_then(|record| record["status"].as_str().map(str::to_owned))
            .is_some_and(|status| status == "running");
        if running {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "executor never registered as running"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    state
        .kernel
        .thread_runtime
        .cancel_session_activity("fixture-user", "fixture-thread", "test_stop")
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(30), collector)
        .await
        .unwrap()
        .unwrap();
    server.abort();
    let events = drain_events(&mut events_rx);
    assert_cancelled_settlement(&state, &events);
}

// 停止受理落在工具审批等待期间（工具阶段挂起）：审批按取消收口，工具不得执行，
// 不得再发起下一轮模型调用，全链路按 cancelled 结清。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_while_tool_waits_for_approval_settles_cancelled() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let app = Router::new().route("/v1/chat/completions", post(move |Json(request): Json<Value>| {
        let index = counter.fetch_add(1, Ordering::SeqCst);
        async move {
            let (message, reason) = if index == 0 {
                let name = request["tools"].as_array().unwrap().iter()
                    .find_map(|tool| tool.pointer("/function/name").and_then(Value::as_str)
                        .filter(|name| *name == "write_file")).expect("write_file exposed");
                (json!({"role":"assistant","tool_calls":[{"id":"fixture-call","type":"function",
                    "function":{"name":name,"arguments":json!({"path":"fixture-hang.txt","content":"must never land"}).to_string()}}]}), "tool_calls")
            } else {
                (json!({"role":"assistant","content":"fixture after approval"}), "stop")
            };
            format!("data: {}\n\ndata: [DONE]\n\n", json!({"choices":[{"delta":message,"finish_reason":reason}],
                "usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12}}))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let root = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.tools.builtin.enabled = vec![resolve_tool_name("write_file")];
    config.security.approval_mode = Some("suggest".into());
    fixture_llm_config(&mut config, &address);
    let state = fixture_state(&root, config).await;
    let (approval_tx, mut approval_rx) = crate::core::approval::new_channel();
    let mut request: WunderRequest = serde_json::from_value(json!({
        "user_id":"fixture-user","session_id":"fixture-thread","question":"fixture input",
        "stream":true,"tool_names":["write_file"]
    }))
    .unwrap();
    request.is_admin = true;
    request.approval_tx = Some(approval_tx);
    let stream = state.kernel.orchestrator.stream(request).await.unwrap();
    let (mut events_rx, collector) = collect_stream(stream);
    // 等工具进入审批等待：执行体挂起在工具阶段。
    let approval = tokio::time::timeout(Duration::from_secs(30), approval_rx.recv())
        .await
        .unwrap()
        .expect("approval request arrives");
    assert_eq!(approval.tool, resolve_tool_name("write_file"));
    state
        .kernel
        .thread_runtime
        .cancel_session_activity("fixture-user", "fixture-thread", "test_stop")
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(30), collector)
        .await
        .unwrap()
        .unwrap();
    server.abort();
    let events = drain_events(&mut events_rx);
    assert_cancelled_settlement(&state, &events);
    assert!(
        !events.iter().any(|(name, _)| name == "workspace_update"),
        "cancelled tool must not produce a workspace change: {events:?}"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "cancelled tool wait must not be followed by another model round"
    );
}

// 停止晚于完成到达：结清结果优先，轮次保持 completed、监视器保持 finished、
// 回复气泡完整，停止操作幂等（不再取消任何轮次）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_after_completion_keeps_completed_settlement() {
    let app = Router::new().route("/v1/chat/completions", post(|| async {
        format!("data: {}\n\ndata: [DONE]\n\n", json!({"choices":[{"delta":{"role":"assistant","content":"fixture answer"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12}}))
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let root = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    fixture_llm_config(&mut config, &address);
    let state = fixture_state(&root, config).await;
    let mut request: WunderRequest = serde_json::from_value(json!({
        "user_id":"fixture-user","session_id":"fixture-thread","question":"fixture input","stream":true
    }))
    .unwrap();
    request.is_admin = true;
    let stream = state.kernel.orchestrator.stream(request).await.unwrap();
    let (mut events_rx, collector) = collect_stream(stream);
    tokio::time::timeout(Duration::from_secs(30), collector)
        .await
        .unwrap()
        .unwrap();
    server.abort();
    let events = drain_events(&mut events_rx);
    assert!(
        events.iter().any(|(name, _)| name == "final"),
        "completed turn must emit a final frame: {events:?}"
    );
    let settled = state
        .kernel
        .thread_runtime
        .cancel_session_activity("fixture-user", "fixture-thread", "test_stop")
        .await
        .unwrap();
    assert_eq!(
        settled.turns_cancelled, 0,
        "stop after completion is a no-op"
    );
    let turns = state
        .storage
        .list_thread_turns("fixture-user", "fixture-thread", None, 12)
        .unwrap();
    assert_eq!(turns[0]["status"], "completed");
    let page = state
        .storage
        .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
        .unwrap();
    assert_eq!(page.runtime_status, "completed");
    assert_eq!(page.chat[0].assistant.content, "fixture answer");
    let record = state.monitor.get_record("fixture-thread").unwrap();
    assert_eq!(
        record["status"],
        json!("finished"),
        "late stop must not rewrite the finished monitor record"
    );
}

async fn fixture_cancelled_turn(state: &AppState, cancel_turn_only: bool) -> (String, RoundInfo) {
    let accepted = state
        .storage
        .accept_thread_turn(
            "fixture-user",
            "fixture-thread",
            &json!({"content":"Fixture input"}),
        )
        .unwrap();
    state
        .kernel
        .orchestrator
        .committer
        .freeze_session(
            "fixture-user",
            "fixture-thread",
            "frozen fixture instructions",
            "fixture-workspace",
        )
        .await
        .unwrap();
    let turn = accepted["turn_id"].as_str().unwrap().to_string();
    state
        .monitor
        .register("fixture-thread", "fixture-user", "", "fixture input", true);
    if cancel_turn_only {
        // 停止受理落在最后检查点之后：持久轮次已 cancelled，监视器未置取消标志。
        let settled = state
            .kernel
            .orchestrator
            .committer
            .update_turn(
                "fixture-user",
                "fixture-thread",
                &turn,
                "cancelled",
                "",
                &json!({"stop_reason":"test_stop"}),
            )
            .await
            .unwrap();
        assert!(settled);
    } else {
        // 停止已受理且取消对执行体可见。
        state
            .monitor
            .cancel_with_source("fixture-thread", "test_stop");
        state
            .kernel
            .orchestrator
            .committer
            .cancel_active_turns("fixture-user", "fixture-thread", "test_stop")
            .await
            .unwrap();
    }
    let round_info = RoundInfo::user_only_thread(1, Uuid::parse_str(&turn).unwrap());
    (turn, round_info)
}

fn fixture_emitter(state: &AppState) -> EventEmitter {
    EventEmitter::new(
        "fixture-thread".into(),
        "fixture-user".into(),
        None,
        Some(state.storage.clone()),
        state.monitor.clone(),
        true,
        None,
    )
}

// D1 修复点：持久面拒绝完成写入（轮次已 cancelled）时，finish_request_success
// 必须按取消语义失败，而不是照发完成语义。旧实现返回 Ok 并把监视器改写为 finished。
#[tokio::test]
async fn finish_request_success_honours_turn_cancelled_before_commit() {
    let root = tempfile::tempdir().unwrap();
    let state = fixture_state(&root, Config::default()).await;
    let (turn, round_info) = fixture_cancelled_turn(&state, true).await;
    let emitter = fixture_emitter(&state);
    let usage = TokenUsage::default();
    let result = state
        .kernel
        .orchestrator
        .finish_request_success(
            "fixture-user",
            "fixture-thread",
            None,
            "fixture-round",
            "fixture input",
            "fixture answer",
            &emitter,
            round_info,
            None,
            None,
            false,
            false,
            &usage,
            "stop",
            None,
            true,
            LlmModelConfig::default(),
        )
        .await;
    let err = result.expect_err("cancelled turn must not settle as completed");
    assert_eq!(err.code(), "CANCELLED");
    let turns = state
        .storage
        .list_thread_turns("fixture-user", "fixture-thread", None, 12)
        .unwrap();
    assert_eq!(turns[0]["status"], "cancelled");
    assert_eq!(turns[0]["turn_id"], json!(turn));
    let record = state.monitor.get_record("fixture-thread").unwrap();
    assert_ne!(
        record["status"],
        json!("finished"),
        "rejected completion must not flap the monitor to finished"
    );
}

// D1 修复点（前置闸形态）：停止已受理、取消可见时，finish_request_success
// 在进入完成提交前直接返回 CANCELLED。
#[tokio::test]
async fn finish_request_success_refuses_after_cancel_accepted() {
    let root = tempfile::tempdir().unwrap();
    let state = fixture_state(&root, Config::default()).await;
    let (_turn, round_info) = fixture_cancelled_turn(&state, false).await;
    let emitter = fixture_emitter(&state);
    let usage = TokenUsage::default();
    let result = state
        .kernel
        .orchestrator
        .finish_request_success(
            "fixture-user",
            "fixture-thread",
            None,
            "fixture-round",
            "fixture input",
            "fixture answer",
            &emitter,
            round_info,
            None,
            None,
            false,
            false,
            &usage,
            "stop",
            None,
            true,
            LlmModelConfig::default(),
        )
        .await;
    let err = result.expect_err("accepted cancel must refuse completion settlement");
    assert_eq!(err.code(), "CANCELLED");
    let turns = state
        .storage
        .list_thread_turns("fixture-user", "fixture-thread", None, 12)
        .unwrap();
    assert_eq!(turns[0]["status"], "cancelled");
    let record = state.monitor.get_record("fixture-thread").unwrap();
    assert_ne!(record["status"], json!("finished"));
}
