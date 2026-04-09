use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{header::AUTHORIZATION, Method, Request, StatusCode},
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::time::sleep;
use tower::ServiceExt;
use wunder_server::{
    build_desktop_router,
    config::{Config, LlmModelConfig},
    config_store::ConfigStore,
    history::HistoryManager,
    state::{AppState, AppStateInitOptions},
};

const DEFAULT_MOCK_CONTEXT_LIMIT: i64 = 4500;

struct MindieOverflowMockState {
    context_limit: i64,
    total_calls: AtomicUsize,
    overflow_calls: AtomicUsize,
    success_calls: AtomicUsize,
    last_success_payload: Mutex<Option<Value>>,
}

impl MindieOverflowMockState {
    fn new(context_limit: i64) -> Self {
        Self {
            context_limit,
            total_calls: AtomicUsize::new(0),
            overflow_calls: AtomicUsize::new(0),
            success_calls: AtomicUsize::new(0),
            last_success_payload: Mutex::new(None),
        }
    }
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
    build_test_context_with_compaction(username, 32_768, None).await
}

async fn build_test_context_with_compaction(
    username: &str,
    max_context: u32,
    reset_mode: Option<&str>,
) -> TestContext {
    build_test_context_with_compaction_and_mock_limit(
        username,
        max_context,
        reset_mode,
        DEFAULT_MOCK_CONTEXT_LIMIT,
    )
    .await
}

async fn build_test_context_with_compaction_and_mock_limit(
    username: &str,
    max_context: u32,
    reset_mode: Option<&str>,
    mock_context_limit: i64,
) -> TestContext {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let (base_url, mock_state) = spawn_mindie_overflow_mock_server(mock_context_limit).await;

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
            max_context: Some(max_context),
            max_output: Some(512),
            support_vision: Some(false),
            support_hearing: Some(false),
            stream: Some(false),
            stream_include_usage: Some(false),
            history_compaction_ratio: Some(0.9),
            history_compaction_reset: reset_mode.map(str::to_string),
            tool_call_mode: Some("tool_call".to_string()),
            reasoning_effort: None,
            model_type: Some("llm".to_string()),
            stop: None,
            mock_if_unconfigured: None,
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
        state,
        token,
        user_id: user.user_id,
        mock_state,
        _temp_dir: temp_dir,
    }
}

async fn spawn_mindie_overflow_mock_server(
    context_limit: i64,
) -> (String, Arc<MindieOverflowMockState>) {
    let state = Arc::new(MindieOverflowMockState::new(context_limit));
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
    let context_limit = state.context_limit;
    let estimated_tokens = estimate_request_tokens(&payload);
    if estimated_tokens > context_limit {
        let overflow_index = state.overflow_calls.fetch_add(1, Ordering::Relaxed) + 1;
        let message = if overflow_index % 2 == 0 {
            format!(
                "InternalError.Algo.InvalidParameter: Range of prompt length should be [1, {context_limit}]"
            )
        } else {
            format!(
                "模型调用失败：提示词过长，最大上下文长度为 {context_limit}，请缩短输入后重试。"
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
    *state
        .last_success_payload
        .lock()
        .expect("last success payload lock") = Some(payload.clone());
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

async fn load_session_events(context: &TestContext, session_id: &str) -> Value {
    let (status, payload) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        &format!("/wunder/chat/sessions/{session_id}/events"),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "load session events failed: {payload}"
    );
    payload.get("data").cloned().unwrap_or(Value::Null)
}

fn find_session_round<'a>(events_data: &'a Value, user_round: i64) -> Option<&'a Value> {
    events_data
        .get("rounds")
        .and_then(Value::as_array)
        .and_then(|rounds| {
            rounds
                .iter()
                .find(|round| round.get("user_round").and_then(Value::as_i64) == Some(user_round))
        })
}

fn build_pressure_question(round: usize, repeat: usize) -> String {
    let payload = "context-pressure-payload ".repeat(repeat);
    format!(
        "[mindie-overflow-regression] round={round}\nPlease keep conversation alive and continue after compaction.\n{payload}"
    )
}

async fn create_test_session(context: &TestContext, title: &str) -> String {
    let (status, created_session) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/chat/sessions",
        Some(json!({ "title": title })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    created_session["data"]["id"]
        .as_str()
        .expect("session id")
        .to_string()
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

fn latest_compaction_summary_item(history: &[Value]) -> Option<&Value> {
    history
        .iter()
        .rev()
        .find(|item| HistoryManager::is_compaction_summary_item(item))
}

fn latest_compaction_summary_item_with_trigger<'a>(
    history: &'a [Value],
    trigger_mode: &str,
) -> Option<&'a Value> {
    history.iter().rev().find(|item| {
        HistoryManager::is_compaction_summary_item(item)
            && item
                .get("meta")
                .and_then(Value::as_object)
                .and_then(|meta| meta.get("trigger_mode"))
                .and_then(Value::as_str)
                == Some(trigger_mode)
    })
}

fn latest_replacement_history_snapshot(history: &[Value]) -> Vec<Value> {
    latest_compaction_summary_item(history)
        .and_then(|item| item.get("meta"))
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("replacement_history"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn summary_body(content: &str) -> &str {
    content
        .split_once('\n')
        .map(|(_, body)| body.trim())
        .unwrap_or_else(|| content.trim())
}

fn latest_success_request_messages(context: &TestContext) -> Vec<Value> {
    context
        .mock_state
        .last_success_payload
        .lock()
        .expect("last success payload lock")
        .as_ref()
        .and_then(|payload| payload.get("messages"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn canonicalize_message_for_comparison(message: &Value) -> Value {
    let mut normalized = serde_json::Map::new();
    if let Some(role) = message.get("role").and_then(Value::as_str) {
        normalized.insert("role".to_string(), Value::String(role.to_string()));
    }
    normalized.insert(
        "content".to_string(),
        message
            .get("content")
            .cloned()
            .unwrap_or(Value::String(String::new())),
    );
    if let Some(reasoning) = message
        .get("reasoning_content")
        .or_else(|| message.get("reasoning"))
        .cloned()
    {
        normalized.insert("reasoning_content".to_string(), reasoning);
    }
    if let Some(tool_calls) = message
        .get("tool_calls")
        .or_else(|| message.get("tool_call"))
        .or_else(|| message.get("function_call"))
        .cloned()
    {
        normalized.insert("tool_calls".to_string(), tool_calls);
    }
    if let Some(tool_call_id) = message
        .get("tool_call_id")
        .or_else(|| message.get("toolCallId"))
        .or_else(|| message.get("call_id"))
        .or_else(|| message.get("callId"))
        .cloned()
    {
        normalized.insert("tool_call_id".to_string(), tool_call_id);
    }
    Value::Object(normalized)
}

fn assert_request_uses_replacement_history(
    request_messages: &[Value],
    replacement_history: &[Value],
    current_user_marker: &str,
) {
    assert!(
        request_messages.len() >= 2,
        "expected request to include at least system and current user"
    );
    assert_eq!(
        request_messages
            .first()
            .and_then(|item| item.get("role"))
            .and_then(Value::as_str),
        Some("system"),
        "expected request to keep system message before replacement_history"
    );
    let current_user = request_messages.last().expect("request current user");
    assert!(
        current_user
            .get("content")
            .and_then(Value::as_str)
            .is_some_and(|content| content.contains(current_user_marker)),
        "expected request to keep current user after replacement_history"
    );
    let base = &request_messages[1..request_messages.len().saturating_sub(1)];
    let expected = replacement_history
        .iter()
        .map(canonicalize_message_for_comparison)
        .collect::<Vec<_>>();
    let actual = base
        .iter()
        .map(canonicalize_message_for_comparison)
        .collect::<Vec<_>>();
    assert_eq!(
        actual, expected,
        "next request should be rebuilt directly from committed replacement_history"
    );
}

async fn trigger_manual_compaction_and_wait(
    context: &TestContext,
    session_id: &str,
) -> (i64, Value) {
    let (status, accepted) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/chat/sessions/{session_id}/compaction"),
        Some(json!({})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "manual compaction request failed: {accepted}"
    );
    let accepted_round = accepted["data"]["user_round"]
        .as_i64()
        .expect("manual compaction user round");

    for _ in 0..120 {
        let data = load_session_events(context, session_id).await;
        let running = data
            .get("running")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let compaction_event = data
            .get("rounds")
            .and_then(Value::as_array)
            .and_then(|rounds| {
                rounds.iter().find_map(|round| {
                    if round.get("user_round").and_then(Value::as_i64) != Some(accepted_round) {
                        return None;
                    }
                    round
                        .get("events")
                        .and_then(Value::as_array)
                        .and_then(|events| {
                            events.iter().find_map(|event| {
                                if event.get("event").and_then(Value::as_str) == Some("compaction")
                                {
                                    event.get("data").cloned()
                                } else {
                                    None
                                }
                            })
                        })
                })
            });
        if let Some(compaction_event) = compaction_event.filter(|_| !running) {
            return (accepted_round, compaction_event);
        }
        sleep(Duration::from_millis(100)).await;
    }

    panic!("manual compaction did not finish in time for session {session_id}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mindie_context_overflow_recovers_and_session_keeps_running() {
    let context = build_test_context("mindie_context_recovery_user").await;
    let session_id = create_test_session(&context, "MindIE context overflow regression").await;

    run_pressure_rounds(&context, &session_id, 12, 600).await;

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
        .load_session_context_limit_hint(&context.user_id, &session_id);
    assert_eq!(persisted_limit_hint, Some(DEFAULT_MOCK_CONTEXT_LIMIT));
    assert!(
        !context
            .state
            .workspace
            .load_session_context_overflow(&context.user_id, &session_id),
        "context overflow marker should be cleared after successful recovery"
    );

    let raw_history = context
        .state
        .workspace
        .load_history(&context.user_id, &session_id, 0)
        .expect("load raw history");
    assert!(
        raw_history
            .iter()
            .any(HistoryManager::is_compaction_summary_item),
        "expected compaction summary item in raw history"
    );
    let summary_item = latest_compaction_summary_item(&raw_history)
        .expect("expected latest compaction summary item");
    let summary_text = summary_item
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("");
    let summary_body = summary_body(summary_text);
    assert!(
        !summary_body.starts_with("...("),
        "persisted summary should not collapse into placeholder fragment: {summary_text}"
    );
    assert!(
        summary_body.chars().count() > 32,
        "persisted summary should remain readable after compaction: {summary_text}"
    );

    let replay_messages = HistoryManager.load_history_messages(
        context.state.workspace.as_ref(),
        &context.user_id,
        &session_id,
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
async fn compaction_replay_uses_committed_summary_in_next_request() {
    let context = build_test_context_with_compaction_and_mock_limit(
        "mindie_compaction_request_shape",
        32_768,
        Some("keep"),
        12_000,
    )
    .await;
    let session_id = create_test_session(&context, "Compaction request shape").await;

    run_pressure_rounds(&context, &session_id, 6, 420).await;
    trigger_manual_compaction_and_wait(&context, &session_id).await;

    let raw_history = context
        .state
        .workspace
        .load_history(&context.user_id, &session_id, 0)
        .expect("load raw history after compaction");
    let replacement_history = latest_replacement_history_snapshot(&raw_history);
    assert!(
        !replacement_history.is_empty(),
        "manual compaction should commit replacement_history snapshot"
    );

    let replay_messages = HistoryManager.load_history_messages(
        context.state.workspace.as_ref(),
        &context.user_id,
        &session_id,
        0,
    );
    assert_eq!(
        replay_messages
            .iter()
            .map(canonicalize_message_for_comparison)
            .collect::<Vec<_>>(),
        replacement_history
            .iter()
            .map(canonicalize_message_for_comparison)
            .collect::<Vec<_>>(),
        "persisted replay should be rebuilt from committed replacement_history"
    );
    let replay_summary = replay_messages
        .iter()
        .find(|item| HistoryManager::is_compaction_summary_item(item))
        .and_then(|item| item.get("content"))
        .and_then(Value::as_str)
        .expect("expected replay summary after compaction")
        .to_string();

    let (status, payload) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/chat/sessions/{session_id}/messages"),
        Some(json!({
            "content": build_pressure_question(7, 420),
            "stream": false
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "follow-up round failed: {payload}");

    let request_messages = latest_success_request_messages(&context);
    assert_request_uses_replacement_history(
        &request_messages,
        &replacement_history,
        "[mindie-overflow-regression] round=7",
    );
    let request_summary = request_messages
        .iter()
        .find(|item| HistoryManager::is_compaction_summary_item(item))
        .and_then(|item| item.get("content"))
        .and_then(Value::as_str)
        .expect("summary-first auto compaction should keep the committed summary in next request");

    assert!(
        !summary_body(request_summary).starts_with("...("),
        "next request summary should not degrade into placeholder fragment: {request_summary}"
    );
    assert!(
        summary_body(request_summary).chars().count() > 32,
        "next request summary should remain readable after auto compaction: {request_summary}"
    );
    assert!(
        !summary_body(&replay_summary).starts_with("...("),
        "replay summary should not be a broken placeholder: {replay_summary}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compaction_reset_modes_align_history_replay_shapes() {
    const ROUNDS: usize = 6;
    const REPEAT: usize = 24;

    for mode in ["zero", "current", "keep"] {
        let context = build_test_context_with_compaction(
            &format!("mindie_compaction_reset_{mode}"),
            32_768,
            Some(mode),
        )
        .await;
        let session_id = create_test_session(&context, &format!("Compaction reset {mode}")).await;

        run_pressure_rounds(&context, &session_id, ROUNDS, REPEAT).await;
        let (_, compaction_event) = trigger_manual_compaction_and_wait(&context, &session_id).await;

        let raw_history = context
            .state
            .workspace
            .load_history(&context.user_id, &session_id, 0)
            .expect("load raw history");
        let summary_item = latest_compaction_summary_item_with_trigger(&raw_history, "manual")
            .or_else(|| latest_compaction_summary_item(&raw_history))
            .expect("expected latest compaction summary item");
        assert_eq!(
            summary_item
                .get("meta")
                .and_then(Value::as_object)
                .and_then(|meta| meta.get("reset_mode"))
                .and_then(Value::as_str),
            Some(mode),
            "mode={mode} latest summary should record effective reset mode"
        );
        let replacement_history = latest_replacement_history_snapshot(&raw_history);
        assert!(
            !replacement_history.is_empty(),
            "mode={mode} manual compaction should commit replacement_history snapshot"
        );
        let replay_messages = HistoryManager.load_history_messages(
            context.state.workspace.as_ref(),
            &context.user_id,
            &session_id,
            0,
        );
        assert_eq!(
            replay_messages
                .iter()
                .map(canonicalize_message_for_comparison)
                .collect::<Vec<_>>(),
            replacement_history
                .iter()
                .map(canonicalize_message_for_comparison)
                .collect::<Vec<_>>(),
            "mode={mode} persisted replay should be rebuilt directly from committed replacement_history"
        );

        match mode {
            "zero" => {
                assert_eq!(
                    compaction_event
                        .get("recent_user_messages_retained")
                        .and_then(Value::as_u64),
                    Some(0),
                    "zero mode should not keep a middle recent-user window in the rebuilt compaction context"
                );
            }
            "current" => {
                assert_eq!(
                    compaction_event
                        .get("recent_user_messages_retained")
                        .and_then(Value::as_u64),
                    Some(0),
                    "current mode should not keep a middle recent-user window in the rebuilt compaction context"
                );
            }
            "keep" => {
                assert!(
                    compaction_event
                        .get("recent_user_messages_retained")
                        .and_then(Value::as_u64)
                        .unwrap_or(0)
                        >= 1,
                    "keep mode should keep at least one middle recent-user message in the rebuilt compaction context"
                );
            }
            _ => unreachable!("unexpected reset mode"),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn manual_compaction_keeps_single_compaction_id_across_events_and_summary_meta() {
    let context = build_test_context_with_compaction_and_mock_limit(
        "mindie_compaction_id_alignment",
        32_768,
        Some("keep"),
        12_000,
    )
    .await;
    let session_id = create_test_session(&context, "Compaction id alignment").await;

    run_pressure_rounds(&context, &session_id, 6, 420).await;
    let (accepted_round, compaction_event) =
        trigger_manual_compaction_and_wait(&context, &session_id).await;
    let compaction_id = compaction_event
        .get("compaction_id")
        .and_then(Value::as_str)
        .expect("manual compaction should emit compaction_id")
        .to_string();
    assert!(
        compaction_id.starts_with("cmp_manual_"),
        "unexpected compaction_id format: {compaction_id}"
    );

    let raw_history = context
        .state
        .workspace
        .load_history(&context.user_id, &session_id, 0)
        .expect("load raw history");
    let summary_item = latest_compaction_summary_item_with_trigger(&raw_history, "manual")
        .or_else(|| latest_compaction_summary_item(&raw_history))
        .expect("expected persisted manual compaction summary");
    assert_eq!(
        summary_item
            .get("meta")
            .and_then(Value::as_object)
            .and_then(|meta| meta.get("compaction_id"))
            .and_then(Value::as_str),
        Some(compaction_id.as_str()),
        "persisted summary meta should keep the committed compaction_id"
    );

    let events_data = load_session_events(&context, &session_id).await;
    let round = find_session_round(&events_data, accepted_round).expect("manual compaction round");
    let events = round
        .get("events")
        .and_then(Value::as_array)
        .expect("manual compaction round events");

    let mut saw_compacting_progress = false;
    let mut saw_summary_request = false;
    let mut saw_summary_response = false;
    let mut saw_final_compaction = false;

    for event in events {
        let event_type = event.get("event").and_then(Value::as_str).unwrap_or("");
        let data = event.get("data").unwrap_or(event);
        let event_compaction_id = data.get("compaction_id").and_then(Value::as_str);

        match event_type {
            "progress" => {
                let stage = data.get("stage").and_then(Value::as_str).unwrap_or("");
                if stage == "compacting" || stage == "context_guard" {
                    assert_eq!(
                        event_compaction_id,
                        Some(compaction_id.as_str()),
                        "manual compaction progress should keep one compaction_id"
                    );
                    if stage == "compacting" {
                        saw_compacting_progress = true;
                    }
                }
            }
            "llm_request" => {
                if data.get("purpose").and_then(Value::as_str) == Some("compaction_summary") {
                    assert_eq!(
                        event_compaction_id,
                        Some(compaction_id.as_str()),
                        "compaction summary request should keep one compaction_id"
                    );
                    saw_summary_request = true;
                }
            }
            "llm_response" => {
                if data.get("purpose").and_then(Value::as_str) == Some("compaction_summary") {
                    assert_eq!(
                        event_compaction_id,
                        Some(compaction_id.as_str()),
                        "compaction summary response should keep one compaction_id"
                    );
                    saw_summary_response = true;
                }
            }
            "compaction" => {
                assert_eq!(
                    event_compaction_id,
                    Some(compaction_id.as_str()),
                    "manual compaction final event should keep one compaction_id"
                );
                saw_final_compaction = true;
            }
            _ => {}
        }
    }

    assert!(
        saw_compacting_progress,
        "manual compaction round should include compacting progress event"
    );
    assert!(
        saw_summary_request,
        "manual compaction round should include compaction summary request event"
    );
    assert!(
        saw_summary_response,
        "manual compaction round should include compaction summary response event"
    );
    assert!(
        saw_final_compaction,
        "manual compaction round should include final compaction event"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "long-running stress regression; run manually when validating deployment"]
async fn mindie_context_overflow_recovery_stays_alive_for_50_rounds() {
    let context = build_test_context("mindie_context_recovery_50_rounds").await;
    let session_id = create_test_session(&context, "MindIE 50 rounds context stress").await;

    run_pressure_rounds(&context, &session_id, 50, 600).await;

    let overflow_calls = context.mock_state.overflow_calls.load(Ordering::Relaxed);
    let success_calls = context.mock_state.success_calls.load(Ordering::Relaxed);
    assert!(overflow_calls > 0, "expected overflow calls, got 0");
    assert!(
        success_calls >= 50,
        "expected at least 50 successful calls, got {success_calls}"
    );
}
