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
    server.abort();
}
