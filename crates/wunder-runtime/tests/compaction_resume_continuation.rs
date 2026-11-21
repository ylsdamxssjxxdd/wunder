//! End-to-end regression for the compaction resume path.
//!
//! Reproduces the failure shape that made a long turn stop after compaction:
//! the summarizer omits the mandatory `resume_action:` line and the summary's
//! remaining-work section is not empty. The runtime must infer `continue`, inject
//! a continuation note that still shows the pending work, and keep tools
//! available so the turn finishes the job instead of reporting it as delivered.
//!
//! Flow: one user turn -> model asks for a tool -> tool succeeds -> the provider
//! reports a context overflow -> overflow recovery compacts -> the summary
//! request is answered without `resume_action` -> the rebuilt continuation
//! request is inspected -> the mock answers it with another tool call, which the
//! runtime has to execute (a turn that treated the compaction note as "done"
//! would stop before this point).

use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{header::AUTHORIZATION, Method, Request, StatusCode},
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tower::ServiceExt;
use wunder_server::{
    build_desktop_router,
    config::{Config, LlmModelConfig},
    config_store::ConfigStore,
    state::{AppState, AppStateInitOptions},
};

const PROVIDER_LIMIT_HINT: i64 = 16_384;
const TOOL_OUTPUT_MARKER: &str = "compaction-resume-tool-ok";
const TOOL_OUTPUT_PATH: &str = "resume-check.txt";
const RESUMED_TOOL_OUTPUT_PATH: &str = "resume-check-after-compaction.txt";
const RESUMED_TOOL_OUTPUT_MARKER: &str = "written-after-compaction";

/// The summary the model would have written: plausible, detailed, and missing
/// both the `resume_action:` line and any "no remaining work" statement.
const COMPACTION_SUMMARY: &str = "## Current progress\n\nBuilt the demo scene module and rendered the first stills.\n\n## Key findings / decisions\n\n- Kept the night look for the whole film.\n\n## Important context / constraints\n\n- Subtitles stay below y=840.\n\n## What remains to be done\n\n1. Render `final.mp4`.\n2. Verify the frame sheet.\n\n## Critical references\n\n- `references/engine.md`";

struct ResumeMockState {
    context_limit: i64,
    summary_calls: AtomicUsize,
    overflow_calls: AtomicUsize,
    normal_calls: AtomicUsize,
    tool_calls_emitted: AtomicUsize,
    overflow_after_tool_pending: AtomicBool,
    requests: Mutex<Vec<Value>>,
}

impl ResumeMockState {
    fn new() -> Self {
        Self {
            context_limit: PROVIDER_LIMIT_HINT,
            summary_calls: AtomicUsize::new(0),
            overflow_calls: AtomicUsize::new(0),
            normal_calls: AtomicUsize::new(0),
            tool_calls_emitted: AtomicUsize::new(0),
            overflow_after_tool_pending: AtomicBool::new(true),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn record(&self, payload: &Value) {
        self.requests
            .lock()
            .expect("mock request lock")
            .push(payload.clone());
    }

    fn requests(&self) -> Vec<Value> {
        self.requests.lock().expect("mock request lock").clone()
    }
}

struct TestContext {
    app: Router,
    token: String,
    mock_state: Arc<ResumeMockState>,
    _temp_dir: TempDir,
}

async fn build_test_context(username: &str) -> TestContext {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let (base_url, mock_state) = spawn_mock_llm_server().await;

    let mut config = Config::default();
    config.server.mode = "cli".to_string();
    config.storage.backend = "sqlite".to_string();
    config.storage.data_root = temp_dir.path().join("data").to_string_lossy().to_string();
    config.workspace.root = temp_dir
        .path()
        .join("workspaces")
        .to_string_lossy()
        .to_string();
    config.tools.builtin.enabled = vec!["写入文件".to_string()];
    config.llm.default = "compaction-resume-mock".to_string();
    config.llm.models.insert(
        "compaction-resume-mock".to_string(),
        LlmModelConfig {
            enable: Some(true),
            provider: Some("openai".to_string()),
            api_mode: None,
            base_url: Some(base_url),
            api_key: Some("test-key".to_string()),
            model: Some("compaction-resume-mock".to_string()),
            temperature: Some(0.0),
            timeout_s: Some(20),
            max_rounds: Some(8),
            max_context: Some(32_768),
            max_output: Some(512),
            thinking_token_budget: None,
            support_vision: Some(false),
            support_hearing: Some(false),
            stream: Some(false),
            stream_include_usage: Some(false),
            history_compaction_ratio: Some(0.9),
            tool_call_mode: Some("tool_call".to_string()),
            reasoning_effort: None,
            model_type: Some("llm".to_string()),
            stop: None,
            mock_if_unconfigured: None,
            ..Default::default()
        },
    );

    let config_store = ConfigStore::new(temp_dir.path().join("wunder.yaml"));
    let config_for_store = config.clone();
    config_store
        .update(|current| *current = config_for_store.clone())
        .await
        .expect("update config store");

    let state = Arc::new(
        AppState::new_with_options(config_store, config, AppStateInitOptions::cli_default())
            .expect("create app state"),
    );
    let user = state
        .user_store
        .create_user(
            username,
            Some(format!("{username}@example.test")),
            "password-123",
            Some("A"),
            None,
            vec!["user".to_string()],
            "active",
            false,
        )
        .expect("create user");
    let token = state
        .user_store
        .create_session_token(&user.user_id)
        .expect("create token")
        .token;

    TestContext {
        app: build_desktop_router(state.clone()),
        token,
        mock_state,
        _temp_dir: temp_dir,
    }
}

async fn spawn_mock_llm_server() -> (String, Arc<ResumeMockState>) {
    let state = Arc::new(ResumeMockState::new());
    let app = Router::new()
        .route("/v1/chat/completions", post(mock_chat_completions))
        .with_state(state.clone());

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock llm listener");
    let addr = listener.local_addr().expect("mock llm addr");
    tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, app).await {
            eprintln!("[compaction_resume_continuation] mock llm server failed: {err}");
        }
    });
    (format!("http://{addr}"), state)
}

async fn mock_chat_completions(
    State(state): State<Arc<ResumeMockState>>,
    Json(payload): Json<Value>,
) -> (StatusCode, Json<Value>) {
    state.record(&payload);
    let last_content = last_message_content(&payload);
    // The compaction request carries the compaction prompt plus the current-user
    // block and its constraints.
    if last_content.contains("CONTEXT CHECKPOINT COMPACTION")
        || last_content.contains("[Compaction constraints /")
    {
        state.summary_calls.fetch_add(1, Ordering::Relaxed);
        return (
            StatusCode::OK,
            Json(completion_body(&payload, json!(COMPACTION_SUMMARY), "stop")),
        );
    }

    // The tool result is the second model call of the turn; that is the point
    // where a real provider reports a context overflow mid-turn.
    let normal_index = state.normal_calls.fetch_add(1, Ordering::Relaxed);
    if normal_index == 1
        && state
            .overflow_after_tool_pending
            .swap(false, Ordering::SeqCst)
    {
        state.overflow_calls.fetch_add(1, Ordering::Relaxed);
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "type": "invalid_request_error",
                    "code": "InvalidParameter",
                    "message": format!(
                        "Range of prompt length should be [1, {}]",
                        state.context_limit
                    ),
                }
            })),
        );
    }

    let emitted = state.tool_calls_emitted.fetch_add(1, Ordering::SeqCst);
    if emitted < 2 {
        // The first call proves the turn reached a successful tool step before
        // compaction; the second one is issued *after* the resumed request, so a
        // turn that stopped at the compaction note would never reach it.
        let (path, marker) = if emitted == 0 {
            (TOOL_OUTPUT_PATH, TOOL_OUTPUT_MARKER)
        } else {
            (RESUMED_TOOL_OUTPUT_PATH, RESUMED_TOOL_OUTPUT_MARKER)
        };
        let tool_name = pick_write_tool_name(&payload);
        let mut body = completion_body(&payload, Value::Null, "tool_calls");
        body["choices"][0]["message"]["tool_calls"] = json!([{
            "id": format!("call_compaction_resume_{}", emitted + 1),
            "type": "function",
            "function": {
                "name": tool_name,
                "arguments": json!({ "path": path, "content": marker }).to_string(),
            }
        }]);
        return (StatusCode::OK, Json(body));
    }

    (
        StatusCode::OK,
        Json(completion_body(&payload, json!("ok"), "stop")),
    )
}

fn completion_body(payload: &Value, content: Value, finish_reason: &str) -> Value {
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("compaction-resume-mock");
    json!({
        "id": "chatcmpl_compaction_resume",
        "object": "chat.completion",
        "created": 1_773_620_812,
        "model": model,
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": content },
            "finish_reason": finish_reason
        }],
        "usage": { "prompt_tokens": 16, "completion_tokens": 8, "total_tokens": 24 }
    })
}

fn last_message_content(payload: &Value) -> String {
    payload
        .get("messages")
        .and_then(Value::as_array)
        .and_then(|messages| messages.last())
        .and_then(|message| message.get("content"))
        .map(|content| match content {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        })
        .unwrap_or_default()
}

fn pick_write_tool_name(payload: &Value) -> String {
    payload
        .get("tools")
        .and_then(Value::as_array)
        .and_then(|tools| {
            tools.iter().find_map(|tool| {
                let name = tool.pointer("/function/name").and_then(Value::as_str)?;
                if name == "write_file" || name.contains("写入文件") {
                    Some(name.to_string())
                } else {
                    None
                }
            })
        })
        .unwrap_or_else(|| "写入文件".to_string())
}

async fn send_json(
    app: &Router,
    token: &str,
    method: Method,
    path: &str,
    payload: Option<Value>,
) -> (StatusCode, Value) {
    let bearer = format!("Bearer {token}");
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(AUTHORIZATION, bearer);
    let body = if let Some(json_body) = payload {
        builder = builder.header("content-type", "application/json");
        Body::from(json_body.to_string())
    } else {
        Body::empty()
    };
    let response = app
        .clone()
        .oneshot(builder.body(body).expect("build request"))
        .await
        .expect("send request");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    let payload = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("parse response json")
    };
    (status, payload)
}

async fn create_test_session(context: &TestContext, title: &str) -> String {
    let (status, created) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/chat/sessions",
        Some(json!({ "title": title })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create session failed: {created}");
    created["data"]["id"]
        .as_str()
        .expect("session id")
        .to_string()
}

fn message_text(message: &Value) -> String {
    message
        .get("content")
        .map(|content| match content {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        })
        .unwrap_or_default()
}

fn find_continuation_request(requests: &[Value]) -> Option<&Value> {
    requests.iter().find(|request| {
        request
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| messages.last())
            .map(|message| message_text(message).contains("[Compaction continuation]"))
            .unwrap_or(false)
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compaction_after_tool_success_keeps_tools_and_pending_work() {
    let context = build_test_context("compaction_resume_continuation_user").await;
    let session_id = create_test_session(&context, "Compaction resume continuation").await;

    let (status, payload) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/chat/sessions/{session_id}/messages"),
        Some(json!({
            "content": "Continue the demo build: run the render step and report progress.",
            "stream": false
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "turn failed: {payload}");
    assert_eq!(payload["data"]["answer"].as_str(), Some("ok"));

    let summary_calls = context.mock_state.summary_calls.load(Ordering::Relaxed);
    let overflow_calls = context.mock_state.overflow_calls.load(Ordering::Relaxed);
    assert_eq!(
        summary_calls, 1,
        "expected exactly one compaction summary call"
    );
    assert_eq!(
        overflow_calls, 1,
        "expected the provider overflow to happen once"
    );

    let requests = context.mock_state.requests();
    let continuation_index = requests
        .iter()
        .position(|request| find_continuation_request(std::slice::from_ref(request)).is_some())
        .expect("expected a continuation request after compaction");
    let continuation = find_continuation_request(&requests).expect("continuation request");

    // The resumed turn must be told to keep working and must still see the TODO.
    let note = continuation
        .get("messages")
        .and_then(Value::as_array)
        .and_then(|messages| messages.last())
        .map(message_text)
        .unwrap_or_default();
    assert!(
        note.contains("- resume_action: continue"),
        "continuation note must infer continue, got: {note}"
    );
    assert!(
        note.contains("## What remains to be done"),
        "continuation note must carry the pending-work section, got: {note}"
    );
    assert!(
        note.contains("1. Render `final.mp4`."),
        "continuation note must keep the concrete pending step, got: {note}"
    );
    assert!(
        !note.contains("Do not call tools"),
        "continuation note must not forbid tools after a compaction, got: {note}"
    );
    assert!(
        !note.contains("...(truncated)"),
        "continuation note must not truncate the summary, got: {note}"
    );

    // The tool result produced before compaction must survive into the resumed request.
    let all_text = |request: &Value| {
        request
            .get("messages")
            .and_then(Value::as_array)
            .map(|messages| {
                messages
                    .iter()
                    .map(message_text)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    };
    let resumed_text = all_text(continuation);
    assert!(
        resumed_text.contains(TOOL_OUTPUT_PATH),
        "tool output produced before compaction must survive into the resumed request: {resumed_text}"
    );

    // The resumed turn has to keep working: the mock answers the continuation
    // with another write_file call, so a later request must carry its receipt.
    // A turn that treated the compaction note as "done" would stop right here.
    let after_continuation = requests[continuation_index + 1..]
        .iter()
        .map(all_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !after_continuation.is_empty(),
        "resumed turn stopped at the continuation note without running a tool"
    );
    assert!(
        after_continuation.contains(RESUMED_TOOL_OUTPUT_PATH),
        "resumed turn must execute the tool requested after compaction: {after_continuation}"
    );
}
