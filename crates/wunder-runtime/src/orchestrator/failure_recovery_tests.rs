use super::*;
use crate::state::{AppState, AppStateInitOptions};
use axum::{http::StatusCode, routing::post, Json, Router};
use std::sync::atomic::Ordering;

fn run_test(
    make_future: impl FnOnce() -> std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>
        + Send
        + 'static,
) {
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

struct Fixture {
    state: Arc<AppState>,
    requests: Arc<ParkingMutex<Vec<Value>>>,
    healthy: Arc<AtomicBool>,
    server: JoinHandle<()>,
    _root: tempfile::TempDir,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl Fixture {
    async fn new(scenario: &'static str, stream: bool) -> Self {
        let requests = Arc::new(ParkingMutex::new(Vec::<Value>::new()));
        let healthy = Arc::new(AtomicBool::new(false));
        let captured = requests.clone();
        let recovered = healthy.clone();
        let app = Router::new().route("/v1/chat/completions", post(move |Json(payload): Json<Value>| {
            let index = {
                let mut requests = captured.lock();
                let index = requests.len();
                requests.push(payload);
                index
            };
            let healthy = recovered.load(Ordering::SeqCst);
            async move {
                let status = if healthy { 200 } else { match scenario {
                    "auth" => 401,
                    "unavailable" => 503,
                    "transient" if index == 0 => 503,
                    "tool" if index == 1 => 503,
                    _ => 200,
                }};
                let body = if status != 200 {
                    json!({"error":{"message": if scenario == "auth" { "authentication rejected: upstream timeout 429 context length exceeded" } else { "temporary failure" }}}).to_string()
                } else if scenario.starts_with("envelope_") && !healthy && (matches!(scenario, "envelope_auth" | "envelope_unavailable") || index == 0) {
                    let payload = if scenario == "envelope_auth" {
                        json!({"type":"error","error":{"type":"authentication_error","message":"fixture timeout 429 context length exceeded"}})
                    } else if scenario == "envelope_unavailable" {
                        json!({"type":"error","code":"server_error","message":"fixture context length exceeded"})
                    } else {
                        json!({"type":"error","code":"rate_limit_exceeded","message":"fixture throttled"})
                    };
                    if stream { format!("data: {payload}\n\ndata: [DONE]\n\n") } else { payload.to_string() }
                } else if scenario == "malformed" && index == 0 {
                    "data: {invalid-json}\n\ndata: [DONE]\n\n".to_string()
                } else if scenario == "truncated" && index == 0 {
                    format!("data: {}\n\n", json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_incomplete","type":"function","function":{"name":"write_file","arguments":"{\"path\":\"probe.txt\",\"content\":\"partial\"}"}}]}}]}))
                } else if scenario == "tool" && index == 0 {
                    let message = json!({"role":"assistant","tool_calls":[{"id":"call_fixture","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"missing-fixture.txt\"}"}}]});
                    if stream {
                        format!("data: {}\n\ndata: [DONE]\n\n", json!({"choices":[{"delta":message,"finish_reason":"tool_calls"}]}))
                    } else {
                        json!({"choices":[{"message":message,"finish_reason":"tool_calls"}]}).to_string()
                    }
                } else if stream {
                    format!("data: {}\n\ndata: [DONE]\n\n", json!({"choices":[{"delta":{"content":"completed"},"finish_reason":"stop"}]}))
                } else {
                    json!({"choices":[{"message":{"content":"completed"},"finish_reason":"stop"}]}).to_string()
                };
                (StatusCode::from_u16(status).unwrap(), body)
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let root = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.tools.builtin.enabled = vec![
            resolve_tool_name("read_file"),
            resolve_tool_name("write_file"),
        ];
        config.storage.backend = "sqlite".into();
        config.storage.data_root = root.path().join("data").to_string_lossy().into_owned();
        config.workspace.root = root.path().join("workspace").to_string_lossy().into_owned();
        config.llm.default = "fixture-model".into();
        config.llm.models.insert(
            "fixture-model".into(),
            LlmModelConfig {
                provider: Some("openai_compatible".into()),
                model: Some("fixture-model".into()),
                base_url: Some(format!("http://{address}/v1")),
                max_context: Some(131072),
                max_rounds: Some(8),
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
        let state = Arc::new(
            AppState::new_with_options(
                store,
                config,
                AppStateInitOptions::cli_default().with_start_thread_runtime(false),
            )
            .unwrap(),
        );
        Self {
            state,
            requests,
            healthy,
            server,
            _root: root,
        }
    }

    fn request(&self, tools: bool) -> WunderRequest {
        let mut request: WunderRequest = serde_json::from_value(json!({
            "user_id":"fixture-user", "session_id":"fixture-thread", "question":"input",
            "skip_tool_calls":!tools, "tool_names":["read_file", "write_file"],
        }))
        .unwrap();
        request.is_admin = true;
        request
    }

    fn assert_settled(&self, expected: &str) {
        let page = self
            .state
            .storage
            .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
            .unwrap();
        assert_eq!(page.runtime_status, expected);
    }
}

#[test]
fn permanent_api_failure_settles_and_next_turn_reuses_the_thread() {
    run_test(|| {
        Box::pin(async {
            for (scenario, stream) in [
                ("auth", false),
                ("auth", true),
                ("envelope_auth", false),
                ("envelope_auth", true),
            ] {
                let fixture = Fixture::new(scenario, stream).await;
                let error = fixture
                    .state
                    .kernel
                    .orchestrator
                    .run(fixture.request(false))
                    .await
                    .unwrap_err();
                let error = error.downcast_ref::<OrchestratorError>().unwrap();
                assert_eq!(error.code(), "LLM_REQUEST_REJECTED");
                assert!(!error.retryable());
                assert_eq!(fixture.requests.lock().len(), 1);
                fixture.assert_settled("failed");
                fixture.healthy.store(true, Ordering::SeqCst);
                let result = fixture
                    .state
                    .kernel
                    .orchestrator
                    .run(fixture.request(false))
                    .await
                    .unwrap();
                assert_eq!(result.answer, "completed");
                fixture.assert_settled("completed");
            }
        })
    });
}

#[test]
fn transient_api_failure_recovers_with_unchanged_system_prompt() {
    run_test(|| {
        Box::pin(async {
            for (scenario, stream) in [
                ("transient", false),
                ("transient", true),
                ("envelope_transient", false),
                ("envelope_transient", true),
                ("malformed", true),
            ] {
                let fixture = Fixture::new(scenario, stream).await;
                let result = fixture
                    .state
                    .kernel
                    .orchestrator
                    .run(fixture.request(false))
                    .await
                    .unwrap();
                assert_eq!(result.answer, "completed");
                fixture.assert_settled("completed");
                let requests = fixture.requests.lock();
                assert_eq!(requests.len(), 2);
                assert_eq!(requests[0]["messages"], requests[1]["messages"]);
                assert_eq!(requests[0]["stream"], requests[1]["stream"]);
            }
        })
    });
}

#[test]
fn truncated_tool_stream_is_retried_without_publishing_a_tool_result() {
    run_test(|| {
        Box::pin(async {
            let fixture = Fixture::new("truncated", true).await;
            let result = fixture
                .state
                .kernel
                .orchestrator
                .run(fixture.request(true))
                .await
                .unwrap();
            assert_eq!(result.answer, "completed");
            fixture.assert_settled("completed");
            let requests = fixture.requests.lock();
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0]["messages"], requests[1]["messages"]);
            assert!(!requests[1]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["role"] == "tool"));
            assert!(!fixture
                .state
                .workspace
                .workspace_root("fixture-user")
                .join("probe.txt")
                .exists());
        })
    });
}

#[test]
fn unavailable_api_exhausts_a_finite_budget_and_releases_the_thread() {
    run_test(|| {
        Box::pin(async {
            let fixture = Fixture::new("unavailable", false).await;
            let error = tokio::time::timeout(
                Duration::from_secs(70),
                fixture
                    .state
                    .kernel
                    .orchestrator
                    .run(fixture.request(false)),
            )
            .await
            .unwrap()
            .unwrap_err();
            assert_eq!(
                error.downcast_ref::<OrchestratorError>().unwrap().code(),
                "LLM_UNAVAILABLE"
            );
            assert_eq!(fixture.requests.lock().len(), 6);
            fixture.assert_settled("failed");
            fixture.healthy.store(true, Ordering::SeqCst);
            assert_eq!(
                fixture
                    .state
                    .kernel
                    .orchestrator
                    .run(fixture.request(false))
                    .await
                    .unwrap()
                    .answer,
                "completed"
            );
        })
    });
}

#[test]
fn unavailable_payload_exhaustion_never_reenters_context_compaction() {
    run_test(|| {
        Box::pin(async {
            let fixture = Fixture::new("envelope_unavailable", true).await;
            let error = tokio::time::timeout(
                Duration::from_secs(70),
                fixture
                    .state
                    .kernel
                    .orchestrator
                    .run(fixture.request(false)),
            )
            .await
            .unwrap()
            .unwrap_err();
            assert_eq!(
                error.downcast_ref::<OrchestratorError>().unwrap().code(),
                "LLM_UNAVAILABLE"
            );
            {
                let requests = fixture.requests.lock();
                assert_eq!(requests.len(), 6);
                assert!(requests
                    .iter()
                    .all(|request| request["messages"] == requests[0]["messages"]));
            }
            fixture.assert_settled("failed");
            fixture.healthy.store(true, Ordering::SeqCst);
            assert_eq!(
                fixture
                    .state
                    .kernel
                    .orchestrator
                    .run(fixture.request(false))
                    .await
                    .unwrap()
                    .answer,
                "completed"
            );
            fixture.assert_settled("completed");
        })
    });
}

#[test]
fn tool_failure_is_paired_once_and_survives_followup_api_retry() {
    run_test(|| {
        Box::pin(async {
            for stream in [false, true] {
                let fixture = Fixture::new("tool", stream).await;
                let result = fixture
                    .state
                    .kernel
                    .orchestrator
                    .run(fixture.request(true))
                    .await
                    .unwrap();
                assert_eq!(result.answer, "completed");
                fixture.assert_settled("completed");
                let requests = fixture.requests.lock();
                assert_eq!(requests.len(), 3);
                assert_eq!(requests[1]["messages"], requests[2]["messages"]);
                let results: Vec<_> = requests[2]["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|message| message["role"] == "tool")
                    .collect();
                assert_eq!(results.len(), 1);
                assert_eq!(results[0]["tool_call_id"], "call_fixture");
                let observation: Value =
                    serde_json::from_str(results[0]["content"].as_str().unwrap()).unwrap();
                assert_eq!(observation["ok"], false);
                assert_eq!(requests[0]["messages"][0], requests[2]["messages"][0]);
                assert_eq!(requests[1]["stream"], requests[2]["stream"]);
            }
        })
    });
}

#[test]
fn cancellation_during_backoff_prevents_another_provider_request() {
    run_test(|| {
        Box::pin(async {
            let fixture = Fixture::new("unavailable", false).await;
            let orchestrator = fixture.state.kernel.orchestrator.clone();
            let request = fixture.request(false);
            let runner = tokio::spawn(async move { orchestrator.run(request).await });
            tokio::time::timeout(Duration::from_secs(10), async {
                while fixture.requests.lock().is_empty() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            tokio::time::sleep(Duration::from_millis(150)).await;
            fixture
                .state
                .kernel
                .thread_runtime
                .cancel_session_activity("fixture-user", "fixture-thread", "fixture-stop")
                .await
                .unwrap();
            let error = tokio::time::timeout(Duration::from_secs(5), runner)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err();
            assert_eq!(
                error.downcast_ref::<OrchestratorError>().unwrap().code(),
                "CANCELLED"
            );
            assert_eq!(fixture.requests.lock().len(), 1);
            fixture.assert_settled("cancelled");
            fixture.healthy.store(true, Ordering::SeqCst);
            assert_eq!(
                fixture
                    .state
                    .kernel
                    .orchestrator
                    .run(fixture.request(false))
                    .await
                    .unwrap()
                    .answer,
                "completed"
            );
        })
    });
}
