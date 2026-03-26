use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{header::AUTHORIZATION, Method, Request, StatusCode},
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tower::ServiceExt;
use wunder_server::{
    build_desktop_router,
    config::{Config, LlmModelConfig},
    config_store::ConfigStore,
    history::HistoryManager,
    state::{AppState, AppStateInitOptions},
};

const MOCK_CONTEXT_LIMIT: i64 = 4500;

#[derive(Default)]
struct MindieOverflowMockState {
    total_calls: AtomicUsize,
    overflow_calls: AtomicUsize,
    success_calls: AtomicUsize,
}

struct TestContext {
    app: Router,
    state: Arc<AppState>,
    token: String,
    user_id: String,
    mock_state: Arc<MindieOverflowMockState>,
    _temp_dir: TempDir,
}

async fn build_test_context(username: &str) -> TestContext {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let (base_url, mock_state) = spawn_mindie_overflow_mock_server().await;

    let mut config = Config::default();
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = temp_dir
        .path()
        .join("context-overflow-recovery.db")
        .to_string_lossy()
        .to_string();
    config.workspace.root = temp_dir
        .path()
        .join("workspaces")
        .to_string_lossy()
        .to_string();
    config.llm.default = "mindie-overflow-mock".to_string();
    config.llm.models.insert(
        "mindie-overflow-mock".to_string(),
        LlmModelConfig {
            enable: Some(true),
            provider: Some("openai".to_string()),
            api_mode: None,
            base_url: Some(base_url),
            api_key: Some("test-key".to_string()),
            model: Some("mindie-overflow-mock".to_string()),
            temperature: Some(0.0),
            timeout_s: Some(20),
            retry: Some(0),
            max_rounds: Some(8),
            max_context: Some(32_768),
            max_output: Some(512),
            support_vision: Some(false),
            support_hearing: Some(false),
            stream: Some(false),
            stream_include_usage: Some(false),
            history_compaction_ratio: Some(0.9),
            history_compaction_reset: None,
            tool_call_mode: Some("tool_call".to_string()),
            reasoning_effort: None,
            model_type: Some("llm".to_string()),
            stop: None,
            mock_if_unconfigured: None,
        },
    );

    let config_store = ConfigStore::new(temp_dir.path().join("wunder.override.yaml"));
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
        state,
        token,
        user_id: user.user_id,
        mock_state,
        _temp_dir: temp_dir,
    }
}

async fn spawn_mindie_overflow_mock_server() -> (String, Arc<MindieOverflowMockState>) {
    let state = Arc::new(MindieOverflowMockState::default());
    let app = Router::new()
        .route("/v1/chat/completions", post(mock_chat_completions))
        .with_state(state.clone());

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock llm listener");
    let addr = listener.local_addr().expect("mock llm addr");
    tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, app).await {
            eprintln!("[context_overflow_recovery_regression] mock llm server failed: {err}");
        }
    });
    (format!("http://{addr}"), state)
}

async fn mock_chat_completions(
    State(state): State<Arc<MindieOverflowMockState>>,
    Json(payload): Json<Value>,
) -> (StatusCode, Json<Value>) {
    state.total_calls.fetch_add(1, Ordering::Relaxed);
    let estimated_tokens = estimate_request_tokens(&payload);
    if estimated_tokens > MOCK_CONTEXT_LIMIT {
        let overflow_index = state.overflow_calls.fetch_add(1, Ordering::Relaxed) + 1;
        let message = if overflow_index % 2 == 0 {
            format!(
                "InternalError.Algo.InvalidParameter: Range of prompt length should be [1, {MOCK_CONTEXT_LIMIT}]"
            )
        } else {
            format!(
                "模型调用失败：提示词过长，最大上下文长度为 {MOCK_CONTEXT_LIMIT}，请缩短输入后重试。"
            )
        };
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "type": "invalid_request_error",
                    "code": "InvalidParameter",
                    "message": message,
                }
            })),
        );
    }

    state.success_calls.fetch_add(1, Ordering::Relaxed);
    (
        StatusCode::OK,
        Json(json!({
            "id": "chatcmpl_context_recovery_test",
            "object": "chat.completion",
            "created": 1_773_620_812,
            "model": "mindie-overflow-mock",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "ok"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": estimated_tokens.max(1),
                "completion_tokens": 8,
                "total_tokens": estimated_tokens.max(1) + 8
            }
        })),
    )
}

fn estimate_request_tokens(payload: &Value) -> i64 {
    let message_tokens = payload
        .get("messages")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(estimate_message_tokens).sum::<i64>())
        .unwrap_or(0);
    let tool_tokens = payload
        .get("tools")
        .map(|value| approx_token_count(&value.to_string()))
        .unwrap_or(0);
    message_tokens
        .saturating_add(tool_tokens)
        .saturating_add(64)
}

fn estimate_message_tokens(message: &Value) -> i64 {
    let content_tokens =
        estimate_content_tokens(message.get("content").unwrap_or(&Value::Null)).max(0);
    let reasoning_tokens = message
        .get("reasoning_content")
        .or_else(|| message.get("reasoning"))
        .map(|value| approx_token_count(&value.to_string()))
        .unwrap_or(0);
    content_tokens
        .saturating_add(reasoning_tokens)
        .saturating_add(4)
}

fn estimate_content_tokens(content: &Value) -> i64 {
    match content {
        Value::Null => 0,
        Value::String(text) => approx_token_count(text),
        Value::Array(parts) => parts
            .iter()
            .map(|part| {
                if let Some(obj) = part.as_object() {
                    if obj.get("type").and_then(Value::as_str) == Some("text") {
                        return obj
                            .get("text")
                            .and_then(Value::as_str)
                            .map(approx_token_count)
                            .unwrap_or(0);
                    }
                    if obj.get("type").and_then(Value::as_str) == Some("image_url")
                        || obj.contains_key("image_url")
                    {
                        return 256;
                    }
                }
                approx_token_count(&part.to_string())
            })
            .sum(),
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("text") {
                return map
                    .get("text")
                    .and_then(Value::as_str)
                    .map(approx_token_count)
                    .unwrap_or(0);
            }
            if map.get("type").and_then(Value::as_str) == Some("image_url")
                || map.contains_key("image_url")
            {
                return 256;
            }
            approx_token_count(&content.to_string())
        }
        other => approx_token_count(&other.to_string()),
    }
}

fn approx_token_count(text: &str) -> i64 {
    if text.is_empty() {
        return 0;
    }
    ((text.len() as f64) / 4.0).ceil() as i64
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

fn build_pressure_question(round: usize, repeat: usize) -> String {
    let payload = "context-pressure-payload ".repeat(repeat);
    format!(
        "[mindie-overflow-regression] round={round}\nPlease keep conversation alive and continue after compaction.\n{payload}"
    )
}

async fn run_pressure_rounds(
    context: &TestContext,
    session_id: &str,
    rounds: usize,
    repeat: usize,
) {
    for round in 1..=rounds {
        let question = build_pressure_question(round, repeat);
        let (status, payload) = send_json(
            &context.app,
            &context.token,
            Method::POST,
            &format!("/wunder/chat/sessions/{session_id}/messages"),
            Some(json!({
                "content": question,
                "stream": false
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "round {round} failed: {payload}");
        let answer = payload["data"]["answer"].as_str().unwrap_or("");
        assert!(
            !answer.trim().is_empty(),
            "round {round} empty answer payload: {payload}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mindie_context_overflow_recovers_and_session_keeps_running() {
    let context = build_test_context("mindie_context_recovery_user").await;
    let (status, created_session) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/chat/sessions",
        Some(json!({ "title": "MindIE context overflow regression" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let session_id = created_session["data"]["id"].as_str().expect("session id");

    run_pressure_rounds(&context, session_id, 12, 600).await;

    let overflow_calls = context.mock_state.overflow_calls.load(Ordering::Relaxed);
    let success_calls = context.mock_state.success_calls.load(Ordering::Relaxed);
    assert!(overflow_calls > 0, "expected overflow calls, got 0");
    assert!(
        success_calls >= 12,
        "expected at least 12 successful calls, got {success_calls}"
    );

    let persisted_limit_hint = context
        .state
        .workspace
        .load_session_context_limit_hint(&context.user_id, session_id);
    assert_eq!(persisted_limit_hint, Some(MOCK_CONTEXT_LIMIT));
    assert!(
        !context
            .state
            .workspace
            .load_session_context_overflow(&context.user_id, session_id),
        "context overflow marker should be cleared after successful recovery"
    );

    let raw_history = context
        .state
        .workspace
        .load_history(&context.user_id, session_id, 0)
        .expect("load raw history");
    assert!(
        raw_history
            .iter()
            .any(HistoryManager::is_compaction_summary_item),
        "expected compaction summary item in raw history"
    );

    let replay_messages = HistoryManager.load_history_messages(
        context.state.workspace.as_ref(),
        &context.user_id,
        session_id,
        0,
    );
    assert!(
        !replay_messages.is_empty(),
        "expected replay history to remain available after recovery"
    );
    assert!(
        replay_messages.len() < raw_history.len(),
        "expected replay history to be compacted, raw={}, replay={}",
        raw_history.len(),
        replay_messages.len()
    );
    assert!(
        replay_messages
            .first()
            .and_then(|item| item.get("role"))
            .and_then(Value::as_str)
            == Some("user"),
        "expected replay history to start with a user summary/tail message"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "long-running stress regression; run manually when validating deployment"]
async fn mindie_context_overflow_recovery_stays_alive_for_50_rounds() {
    let context = build_test_context("mindie_context_recovery_50_rounds").await;
    let (status, created_session) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/chat/sessions",
        Some(json!({ "title": "MindIE 50 rounds context stress" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let session_id = created_session["data"]["id"].as_str().expect("session id");

    run_pressure_rounds(&context, session_id, 50, 600).await;

    let overflow_calls = context.mock_state.overflow_calls.load(Ordering::Relaxed);
    let success_calls = context.mock_state.success_calls.load(Ordering::Relaxed);
    assert!(overflow_calls > 0, "expected overflow calls, got 0");
    assert!(
        success_calls >= 50,
        "expected at least 50 successful calls, got {success_calls}"
    );
}
