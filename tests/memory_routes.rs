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
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::time::{sleep, Duration};
use tower::ServiceExt;
use wunder_server::{
    build_desktop_router,
    config::{Config, LlmModelConfig},
    config_store::ConfigStore,
    state::{AppState, AppStateInitOptions},
};

struct TestContext {
    app: Router,
    token: String,
    mock_llm_state: Option<Arc<MockLlmState>>,
    _temp_dir: TempDir,
}

#[derive(Default)]
struct MockLlmState {
    total_calls: AtomicUsize,
    chat_calls: AtomicUsize,
    extraction_calls: AtomicUsize,
    chat_tool_names: Mutex<Vec<Vec<String>>>,
}

impl MockLlmState {
    fn push_chat_tool_names(&self, tool_names: Vec<String>) {
        if let Ok(mut guard) = self.chat_tool_names.lock() {
            guard.push(tool_names);
        }
    }

    fn last_chat_tool_names(&self) -> Vec<String> {
        self.chat_tool_names
            .lock()
            .ok()
            .and_then(|guard| guard.last().cloned())
            .unwrap_or_default()
    }
}

async fn build_test_context(username: &str) -> TestContext {
    build_test_context_with_config(username, |_| {}).await
}

async fn build_test_context_with_config<F>(username: &str, configure: F) -> TestContext
where
    F: FnOnce(&mut Config),
{
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = Config::default();
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = temp_dir
        .path()
        .join("memory-routes.db")
        .to_string_lossy()
        .to_string();
    config.workspace.root = temp_dir
        .path()
        .join("workspaces")
        .to_string_lossy()
        .to_string();
    configure(&mut config);

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
        mock_llm_state: None,
        _temp_dir: temp_dir,
    }
}

async fn build_test_context_with_mock_llm(username: &str) -> TestContext {
    build_test_context_with_mock_llm_and_tool_mode(username, "tool_call").await
}

async fn build_test_context_with_mock_llm_and_tool_mode(
    username: &str,
    tool_call_mode: &str,
) -> TestContext {
    let (base_url, mock_llm_state) = spawn_mock_llm_server().await;
    let tool_call_mode = tool_call_mode.to_string();
    let mut context = build_test_context_with_config(username, |config| {
        config.llm.default = "mock-auto-memory".to_string();
        config.llm.models.insert(
            "mock-auto-memory".to_string(),
            LlmModelConfig {
                enable: Some(true),
                provider: Some("openai".to_string()),
                api_mode: None,
                base_url: Some(base_url.clone()),
                api_key: Some("memory-test-key".to_string()),
                model: Some("mock-auto-memory".to_string()),
                temperature: Some(0.0),
                timeout_s: Some(15),
                retry: Some(0),
                max_rounds: Some(4),
                max_context: Some(16_384),
                max_output: Some(512),
                support_vision: Some(false),
                support_hearing: Some(false),
                stream: Some(false),
                stream_include_usage: Some(false),
                history_compaction_ratio: None,
                history_compaction_reset: None,
                tool_call_mode: Some(tool_call_mode.clone()),
                reasoning_effort: None,
                model_type: None,
                stop: None,
                mock_if_unconfigured: None,
            },
        );
    })
    .await;
    context.mock_llm_state = Some(mock_llm_state);
    context
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

async fn wait_for_memory_count(
    app: &Router,
    token: &str,
    min_count: usize,
    attempts: usize,
) -> Value {
    for _ in 0..attempts {
        let (status, payload) = send_json(
            app,
            token,
            Method::GET,
            "/wunder/agents/__default__/memories?limit=200&include_invalidated=true",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let count = payload["data"]["total"].as_u64().unwrap_or(0) as usize;
        if count >= min_count {
            return payload;
        }
        sleep(Duration::from_millis(150)).await;
    }
    panic!("memory fragments did not reach expected count");
}

async fn spawn_mock_llm_server() -> (String, Arc<MockLlmState>) {
    let state = Arc::new(MockLlmState::default());
    let app = Router::new()
        .route("/v1/chat/completions", post(mock_chat_completions))
        .with_state(state.clone());

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock llm listener");
    let addr = listener.local_addr().expect("mock llm addr");
    tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, app).await {
            eprintln!("[memory_routes] mock llm server failed: {err}");
        }
    });
    (format!("http://{addr}"), state)
}

async fn mock_chat_completions(
    State(state): State<Arc<MockLlmState>>,
    Json(payload): Json<Value>,
) -> Json<Value> {
    state.total_calls.fetch_add(1, Ordering::Relaxed);
    let messages = payload
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let joined = messages
        .iter()
        .map(flatten_message_content)
        .collect::<Vec<_>>()
        .join("\n");
    let last_user = last_user_message(&messages);
    let is_auto_extract = last_user.contains("[Current User Message]")
        || joined.contains("<memory_fragments>")
        || joined.contains("long-term memory extraction engine")
        || joined.contains("长期记忆提炼器");

    if is_auto_extract {
        state.extraction_calls.fetch_add(1, Ordering::Relaxed);
        return Json(openai_chat_response(
            r#"<memory_fragments>
{
  "items": [
    {
      "category": "profile",
      "slot": "name",
      "title": "用户姓名",
      "summary": "用户姓名是周华健",
      "content": "用户明确说自己叫周华健",
      "tags": ["identity", "name"],
      "tier": "core",
      "importance": 0.95,
      "confidence": 0.98
    },
    {
      "category": "response-preference",
      "slot": "reply_language",
      "title": "默认使用中文回复",
      "summary": "默认使用中文回复",
      "content": "用户要求后续默认使用中文回复",
      "tags": ["language", "reply", "zh"],
      "tier": "core",
      "importance": 0.92,
      "confidence": 0.97
    }
  ]
}
</memory_fragments>"#,
        ));
    }

    state.chat_calls.fetch_add(1, Ordering::Relaxed);
    state.push_chat_tool_names(extract_tool_names(&payload));
    Json(openai_chat_response("好的，我记住了。"))
}

fn last_user_message(messages: &[Value]) -> String {
    messages
        .iter()
        .rev()
        .find(|message| message.get("role").and_then(Value::as_str) == Some("user"))
        .map(flatten_message_content)
        .unwrap_or_default()
}

fn flatten_message_content(message: &Value) -> String {
    match message.get("content").unwrap_or(&Value::Null) {
        Value::String(text) => text.trim().to_string(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                let obj = item.as_object()?;
                if obj.get("type").and_then(Value::as_str) == Some("text") {
                    return obj.get("text").and_then(Value::as_str).map(str::to_string);
                }
                obj.get("content")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string(),
        other => other.to_string(),
    }
}

fn extract_tool_names(payload: &Value) -> Vec<String> {
    let mut names = payload
        .get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| {
                    tool.get("function")
                        .and_then(|function| function.get("name"))
                        .and_then(Value::as_str)
                        .or_else(|| tool.get("name").and_then(Value::as_str))
                        .map(str::to_string)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    names.sort();
    names.dedup();
    names
}

fn openai_chat_response(content: &str) -> Value {
    json!({
        "id": "chatcmpl_memory_test",
        "object": "chat.completion",
        "created": 1_773_620_812,
        "model": "mock-auto-memory",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": content,
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 64,
            "completion_tokens": 32,
            "total_tokens": 96
        }
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn memory_routes_and_prompt_preview_work_end_to_end() {
    let context = build_test_context("memory_route_user").await;

    let (status, created) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/agents/__default__/memories",
        Some(json!({
            "title_l0": "Reply style",
            "summary_l1": "Prefer concise answers.",
            "content_l2": "Prefer concise answers.",
            "fact_key": "preference::reply_style"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let memory_id = created["data"]["item"]["memory_id"]
        .as_str()
        .expect("memory id")
        .to_string();

    let (status, listed) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        "/wunder/agents/__default__/memories?limit=200",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["data"]["total"], json!(1));
    assert_eq!(listed["data"]["items"][0]["memory_id"], json!(memory_id));

    let (status, pinned) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/agents/__default__/memories/{memory_id}/pin"),
        Some(json!({ "value": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pinned["data"]["item"]["pinned"], json!(true));

    let (status, preview) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/chat/system-prompt",
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let prompt = preview["data"]["prompt"]
        .as_str()
        .expect("system prompt preview");
    assert!(prompt.contains("Prefer concise answers."));
    assert_eq!(preview["data"]["memory_preview_count"], json!(1));
    assert_eq!(preview["data"]["memory_preview_total_count"], json!(1));

    let (status, updated) = send_json(
        &context.app,
        &context.token,
        Method::PATCH,
        &format!("/wunder/agents/__default__/memories/{memory_id}"),
        Some(json!({
            "summary_l1": "Prefer bullet lists in answers.",
            "content_l2": "Prefer bullet lists in answers.",
            "pinned": true
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        updated["data"]["item"]["summary_l1"],
        json!("Prefer bullet lists in answers.")
    );

    let (status, refreshed_preview) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/chat/system-prompt",
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let refreshed_prompt = refreshed_preview["data"]["prompt"]
        .as_str()
        .expect("refreshed prompt preview");
    assert!(refreshed_prompt.contains("Prefer bullet lists in answers."));

    let (status, invalidated) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/agents/__default__/memories/{memory_id}/invalidate"),
        Some(json!({ "value": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(invalidated["data"]["item"]["status"], json!("invalidated"));

    let (status, active_list) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        "/wunder/agents/__default__/memories?limit=200",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(active_list["data"]["total"], json!(0));

    let (status, full_list) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        "/wunder/agents/__default__/memories?limit=200&include_invalidated=true",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(full_list["data"]["total"], json!(1));
    assert_eq!(
        full_list["data"]["items"][0]["status"],
        json!("invalidated")
    );

    let (status, deleted) = send_json(
        &context.app,
        &context.token,
        Method::DELETE,
        &format!("/wunder/agents/__default__/memories/{memory_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(deleted["data"]["deleted"], json!(true));

    let (status, empty_list) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        "/wunder/agents/__default__/memories?limit=200&include_invalidated=true",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(empty_list["data"]["total"], json!(0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_prompt_preview_freezes_after_first_user_message() {
    let context = build_test_context_with_mock_llm("memory_prompt_user").await;

    let (status, created) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/agents/__default__/memories",
        Some(json!({
            "title_l0": "Reply language",
            "summary_l1": "Reply in English by default.",
            "content_l2": "Reply in English by default.",
            "fact_key": "preference::reply_language"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(created["data"]["item"]["memory_id"].is_string());

    let (status, session_created) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/chat/sessions",
        Some(json!({
            "title": "Frozen memory session"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let session_id = session_created["data"]["id"]
        .as_str()
        .expect("session id")
        .to_string();

    let (status, session_preview_before_first_message) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/chat/sessions/{session_id}/system-prompt"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        session_preview_before_first_message["data"]["memory_preview_mode"],
        json!("pending")
    );
    assert_eq!(
        session_preview_before_first_message["data"]["memory_preview_total_count"],
        json!(1)
    );
    let pending_prompt = session_preview_before_first_message["data"]["prompt"]
        .as_str()
        .expect("pending system prompt")
        .to_string();
    assert!(pending_prompt.contains("Reply in English by default."));

    let (status, message_result) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/chat/sessions/{session_id}/messages"),
        Some(json!({
            "content": "Use the saved preference in this thread.",
            "stream": false
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(message_result["data"]["answer"].is_string());

    let (status, session_preview_after_first_message) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/chat/sessions/{session_id}/system-prompt"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        session_preview_after_first_message["data"]["memory_preview_mode"],
        json!("frozen")
    );
    assert_eq!(
        session_preview_after_first_message["data"]["memory_preview_total_count"],
        json!(1)
    );
    let frozen_prompt = session_preview_after_first_message["data"]["prompt"]
        .as_str()
        .expect("frozen system prompt")
        .to_string();
    assert!(frozen_prompt.contains("Reply in English by default."));

    let (status, second_memory) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/agents/__default__/memories",
        Some(json!({
            "title_l0": "Reply language",
            "summary_l1": "Reply in Chinese by default.",
            "content_l2": "Reply in Chinese by default.",
            "fact_key": "preference::reply_language_new"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(second_memory["data"]["item"]["memory_id"].is_string());

    let (status, session_preview) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/chat/sessions/{session_id}/system-prompt"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        session_preview["data"]["memory_preview_mode"],
        json!("frozen")
    );
    assert_eq!(session_preview["data"]["memory_preview_count"], json!(1));
    assert_eq!(
        session_preview["data"]["memory_preview_total_count"],
        json!(1)
    );
    let reused_prompt = session_preview["data"]["prompt"]
        .as_str()
        .expect("session system prompt");
    assert_eq!(reused_prompt, frozen_prompt);
    assert!(!reused_prompt.contains("Reply in Chinese by default."));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_prompt_and_runtime_keep_frozen_agent_tool_baseline_after_agent_edit() {
    let context =
        build_test_context_with_mock_llm_and_tool_mode(
            "frozen_tool_baseline_user",
            "function_call",
        )
        .await;

    let (status, created_agent) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/agents",
        Some(json!({
            "name": "Frozen Tool Agent",
            "system_prompt": "Use only the configured tools.",
            "tool_names": ["read_file"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let agent_id = created_agent["data"]["id"]
        .as_str()
        .expect("agent id")
        .to_string();

    let (status, session_created) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/chat/sessions",
        Some(json!({
            "title": "Frozen tool baseline session",
            "agent_id": agent_id,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let session_id = session_created["data"]["id"]
        .as_str()
        .expect("session id")
        .to_string();

    let (status, first_message) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/chat/sessions/{session_id}/messages"),
        Some(json!({
            "content": "First turn",
            "stream": false
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(first_message["data"]["answer"].is_string());

    let mock_llm_state = context
        .mock_llm_state
        .as_ref()
        .expect("mock llm state");
    assert_eq!(mock_llm_state.last_chat_tool_names(), vec!["read_file"]);

    let (status, frozen_preview_before_agent_edit) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/chat/sessions/{session_id}/system-prompt"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let frozen_prompt = frozen_preview_before_agent_edit["data"]["prompt"]
        .as_str()
        .expect("frozen prompt")
        .to_string();
    assert_eq!(
        frozen_preview_before_agent_edit["data"]["tooling_preview"]["selected_tool_names"],
        json!(["read_file"])
    );

    let (status, updated_agent) = send_json(
        &context.app,
        &context.token,
        Method::PUT,
        &format!("/wunder/agents/{agent_id}"),
        Some(json!({
            "tool_names": ["write_file"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated_agent["data"]["tool_names"], json!(["write_file"]));

    let (status, frozen_preview_after_agent_edit) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/chat/sessions/{session_id}/system-prompt"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        frozen_preview_after_agent_edit["data"]["prompt"],
        json!(frozen_prompt)
    );
    assert_eq!(
        frozen_preview_after_agent_edit["data"]["tooling_preview"]["selected_tool_names"],
        json!(["read_file"])
    );

    let (status, second_message) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/chat/sessions/{session_id}/messages"),
        Some(json!({
            "content": "Second turn",
            "stream": false
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(second_message["data"]["answer"].is_string());
    assert_eq!(mock_llm_state.last_chat_tool_names(), vec!["read_file"]);

    let (status, session_tools_updated) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/chat/sessions/{session_id}/tools"),
        Some(json!({
            "tool_overrides": ["write_file"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(session_tools_updated["data"]["tool_overrides"], json!(["write_file"]));

    let (status, frozen_preview_after_session_override) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/chat/sessions/{session_id}/system-prompt"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        frozen_preview_after_session_override["data"]["prompt"],
        json!(frozen_prompt)
    );
    assert_eq!(
        frozen_preview_after_session_override["data"]["tooling_preview"]["selected_tool_names"],
        json!(["write_file"])
    );

    let (status, third_message) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/chat/sessions/{session_id}/messages"),
        Some(json!({
            "content": "Third turn",
            "stream": false
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(third_message["data"]["answer"].is_string());
    assert_eq!(mock_llm_state.last_chat_tool_names(), vec!["write_file"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_memory_settings_can_toggle_auto_extract() {
    let context = build_test_context("memory_settings_user").await;

    let (status, initial_settings) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        "/wunder/agents/__default__/memory-settings",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        initial_settings["data"]["settings"]["auto_extract_enabled"],
        json!(false)
    );

    let (status, updated_settings) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/agents/__default__/memory-settings",
        Some(json!({
            "auto_extract_enabled": true
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        updated_settings["data"]["settings"]["auto_extract_enabled"],
        json!(true)
    );
    assert!(
        updated_settings["data"]["settings"]["updated_at"]
            .as_f64()
            .expect("updated at")
            > 0.0
    );

    let (status, listed) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        "/wunder/agents/__default__/memories?limit=200&include_invalidated=true",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        listed["data"]["settings"]["auto_extract_enabled"],
        json!(true)
    );

    let (status, refreshed_settings) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        "/wunder/agents/__default__/memory-settings",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        refreshed_settings["data"]["settings"]["auto_extract_enabled"],
        json!(true)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_extract_uses_mock_llm_and_persists_fragments_end_to_end() {
    let context = build_test_context_with_mock_llm("memory_auto_extract_user").await;

    let (status, session_created) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/chat/sessions",
        Some(json!({
            "title": "Auto Extract Session"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let session_id = session_created["data"]["id"]
        .as_str()
        .expect("session id")
        .to_string();

    let (status, updated_settings) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/agents/__default__/memory-settings",
        Some(json!({
            "auto_extract_enabled": true
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        updated_settings["data"]["settings"]["auto_extract_enabled"],
        json!(true)
    );

    let (status, message_result) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/chat/sessions/{session_id}/messages"),
        Some(json!({
            "content": "我叫周华健，以后请默认使用中文回复。",
            "stream": false
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let answer = message_result["data"]["answer"]
        .as_str()
        .expect("final answer");
    assert!(answer.contains("记住"));

    let listed = wait_for_memory_count(&context.app, &context.token, 2, 30).await;
    let items = listed["data"]["items"]
        .as_array()
        .expect("memory items array");
    assert!(items
        .iter()
        .any(|item| item["summary_l1"] == json!("用户姓名是周华健")));
    assert!(items
        .iter()
        .any(|item| item["summary_l1"] == json!("默认使用中文回复")));

    let jobs = listed["data"]["recent_jobs"]
        .as_array()
        .expect("recent jobs array");
    assert!(!jobs.is_empty());
    assert_eq!(jobs[0]["job_type"], json!("auto_extract_turn"));
    assert!(jobs[0]["status"] == json!("completed") || jobs[0]["status"] == json!("skipped"));

    let mock_llm_state = context.mock_llm_state.expect("mock llm state");
    assert!(mock_llm_state.chat_calls.load(Ordering::Relaxed) >= 1);
    assert!(mock_llm_state.extraction_calls.load(Ordering::Relaxed) >= 1);
    assert!(mock_llm_state.total_calls.load(Ordering::Relaxed) >= 2);
}
