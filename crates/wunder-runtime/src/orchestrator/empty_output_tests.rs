use super::*;
use crate::state::{AppState, AppStateInitOptions};
use axum::{routing::post, Json, Router};

fn run_on_large_stack<F, Fut>(make_future: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()>,
{
    // The debug runner's future includes the full orchestration and SQLite
    // initialization chain. Windows' default test-worker stack is too small.
    // Construct the future on this thread as well, so moving a large future
    // does not first overflow the harness stack.
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_stack_size(16 * 1024 * 1024)
                .enable_all()
                .build()
                .unwrap()
                .block_on(make_future());
        })
        .unwrap()
        .join()
        .unwrap();
}

async fn exercise_output_recovery(kind: &str, stream: bool, recover: bool) {
    let requests = Arc::new(ParkingMutex::new(Vec::<Value>::new()));
    let captured = requests.clone();
    let kind = kind.to_string();
    let tool_case = matches!(kind.as_str(), "arguments" | "final" | "valid_tool");
    let valid_first = matches!(kind.as_str(), "valid_tool" | "valid_text");
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move |Json(payload): Json<Value>| {
            let index = {
                let mut requests = captured.lock();
                let index = requests.len();
                requests.push(payload);
                index
            };
            let kind = kind.clone();
            async move {
                let message = if recover && index > 0 {
                    json!({"content":"completed"})
                } else {
                    match kind.as_str() {
                    "empty" => json!({"content":""}),
                    "valid_text" => json!({"content":"completed"}),
                    "valid_tool" => json!({"tool_calls":[{"id":"call_1","type":"function",
                        "function":{"name":"final_response","arguments":"{\"content\":\"completed\"}"}}]}),
                        "arguments" => json!({"tool_calls":[{"id":"call_1","type":"function",
                        "function":{"name":"final_response","arguments":"{\"content\":\""}}]}),
                        "final" => json!({"tool_calls":[{"id":"call_1","type":"function",
                        "function":{"name":"final_response","arguments":"{}"}}]}),
                        _ => json!({"content":"","reasoning_content":"analysis"}),
                    }
                };
                let reason = if recover && index > 0 {
                    "stop"
                } else {
                    "length"
                };
                let usage = json!({"prompt_tokens":100,"completion_tokens":8192,"total_tokens":8292,
                "completion_tokens_details":{"reasoning_tokens":8192}});
                if stream {
                    format!(
                        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                        json!({"choices":[{"delta":message,"finish_reason":reason}]}),
                        json!({"choices":[],"usage":usage})
                    )
                } else {
                    json!({"choices":[{"message":message,"finish_reason":reason}],"usage":usage})
                        .to_string()
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let root = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.tools.builtin.enabled = vec![resolve_tool_name("final_response")];
    config.storage.backend = "sqlite".into();
    config.storage.data_root = root.path().join("data").to_string_lossy().into_owned();
    config.workspace.root = root.path().join("workspace").to_string_lossy().into_owned();
    config.llm.default = "model_1".into();
    config.llm.models.insert(
        "model_1".into(),
        LlmModelConfig {
            provider: Some("openai_compatible".into()),
            model: Some("model_1".into()),
            base_url: Some(format!("http://{address}/v1")),
            max_rounds: Some(10),
            max_context: Some(131072),
            stream: Some(stream),
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
        "user_id":"user_1", "question":"input", "session_id":"session_1",
        "stream":stream, "skip_tool_calls":!tool_case, "tool_names":["final_response"],
    }))
    .unwrap();
    request.is_admin = true;
    let result = state.kernel.orchestrator.run(request).await;
    if recover {
        assert_eq!(result.unwrap().answer, "completed");
    } else {
        let error = result.unwrap_err();
        let error = error.downcast_ref::<OrchestratorError>().unwrap();
        assert_eq!(
            (error.code(), error.recovery_action()),
            ("LLM_OUTPUT_LOOP", "retry_next_turn")
        );
        assert_eq!(error.detail().unwrap()["finish_reason"], "length");
        assert_eq!(error.detail().unwrap()["output_limit_reached"], true);
    }
    if valid_first {
        let page = state
            .storage
            .thread_view_page("user_1", "session_1", "chat", None, 12)
            .unwrap();
        assert_eq!(page.runtime_status, "completed");
        assert_eq!(page.chat[0].assistant.content, "completed");
        let reopened = crate::storage::thread_store::LocalThreadStore::open(
            state.kernel.orchestrator.thread_store.root(),
        )
        .unwrap();
        let context = reopened
            .model_messages(
                &wunder_core::thread_store::ThreadId::new("session_1").unwrap(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(context.last().unwrap()["content"], "completed");
        assert!(context
            .iter()
            .all(|message| message.get("tool_calls").is_none()));
    }
    let requests = requests.lock();
    assert_eq!(
        requests.len(),
        if valid_first { 1 } else { 2 },
        "no hidden empty-response retries"
    );
    assert_eq!(requests[0]["thinking_token_budget"], 2048);
    if valid_first {
        server.abort();
        return;
    }
    assert_eq!(requests[1]["enable_thinking"], false);
    assert_eq!(
        requests[1]["chat_template_kwargs"]["enable_thinking"],
        false
    );
    assert!(requests[1].get("thinking_token_budget").is_none());
    assert_eq!(
        requests[0]["messages"][0], requests[1]["messages"][0],
        "system prompt stays frozen"
    );
    assert_eq!(requests[0]["max_tokens"], requests[1]["max_tokens"]);
    server.abort();
}

#[test]
fn empty_output_stops_after_one_recovery_across_stream_and_nonstream() {
    run_on_large_stack(|| async {
        for stream in [false, true] {
            for kind in ["reasoning", "empty", "arguments", "final"] {
                exercise_output_recovery(kind, stream, false).await;
            }
        }
    });
}

#[test]
fn empty_output_recovers_without_changing_system_prompt() {
    run_on_large_stack(|| async {
        for stream in [false, true] {
            exercise_output_recovery("reasoning", stream, true).await;
            exercise_output_recovery("arguments", stream, true).await;
        }
    });
}

#[test]
fn empty_output_guard_keeps_usable_answers_and_complete_calls_at_limit() {
    run_on_large_stack(|| async {
        for stream in [false, true] {
            exercise_output_recovery("valid_text", stream, true).await;
            exercise_output_recovery("valid_tool", stream, true).await;
        }
    });
}
