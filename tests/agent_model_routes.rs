use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{header::AUTHORIZATION, Method, Request, StatusCode},
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
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

struct TestContext {
    app: Router,
    token: String,
    mock_state: Option<Arc<MockLlmState>>,
    _temp_dir: TempDir,
}

#[derive(Default)]
struct MockLlmState {
    models: Mutex<Vec<String>>,
}

impl MockLlmState {
    fn push_model(&self, model: String) {
        if let Ok(mut guard) = self.models.lock() {
            guard.push(model);
        }
    }

    fn all_models(&self) -> Vec<String> {
        self.models
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
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
        .join("agent-model-routes.db")
        .to_string_lossy()
        .to_string();
    config.workspace.root = temp_dir
        .path()
        .join("workspaces")
        .to_string_lossy()
        .to_string();
    configure(&mut config);

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
        app: build_desktop_router(state),
        token,
        mock_state: None,
        _temp_dir: temp_dir,
    }
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
            eprintln!("[agent_model_routes] mock llm server failed: {err}");
        }
    });
    (format!("http://{addr}"), state)
}

async fn build_test_context_with_mock_llm(username: &str) -> TestContext {
    let (base_url, mock_state) = spawn_mock_llm_server().await;
    let mut context = build_test_context_with_config(username, |config| {
        config.llm.default = "model-default".to_string();
        config.llm.models.clear();
        config.llm.models.insert(
            "model-default".to_string(),
            LlmModelConfig {
                enable: Some(true),
                provider: Some("openai".to_string()),
                api_mode: None,
                base_url: Some(base_url.clone()),
                api_key: Some("test-key".to_string()),
                model: Some("provider-default".to_string()),
                temperature: Some(0.0),
                timeout_s: Some(15),
                retry: Some(0),
                max_rounds: Some(4),
                max_context: Some(16_384),
                max_output: Some(256),
                support_vision: Some(false),
                support_hearing: Some(false),
                stream: Some(false),
                stream_include_usage: Some(false),
                history_compaction_ratio: None,
                history_compaction_reset: None,
                tool_call_mode: Some("tool_call".to_string()),
                reasoning_effort: None,
                model_type: Some("llm".to_string()),
                stop: None,
                mock_if_unconfigured: None,
            },
        );
        config.llm.models.insert(
            "model-agent".to_string(),
            LlmModelConfig {
                enable: Some(true),
                provider: Some("openai".to_string()),
                api_mode: None,
                base_url: Some(base_url.clone()),
                api_key: Some("test-key".to_string()),
                model: Some("provider-agent".to_string()),
                temperature: Some(0.0),
                timeout_s: Some(15),
                retry: Some(0),
                max_rounds: Some(4),
                max_context: Some(16_384),
                max_output: Some(256),
                support_vision: Some(false),
                support_hearing: Some(false),
                stream: Some(false),
                stream_include_usage: Some(false),
                history_compaction_ratio: None,
                history_compaction_reset: None,
                tool_call_mode: Some("tool_call".to_string()),
                reasoning_effort: None,
                model_type: Some("llm".to_string()),
                stop: None,
                mock_if_unconfigured: None,
            },
        );
        config.llm.models.insert(
            "model-embedding".to_string(),
            LlmModelConfig {
                enable: Some(true),
                provider: Some("openai".to_string()),
                api_mode: None,
                base_url: Some(base_url),
                api_key: Some("test-key".to_string()),
                model: Some("provider-embedding".to_string()),
                temperature: Some(0.0),
                timeout_s: Some(15),
                retry: Some(0),
                max_rounds: Some(4),
                max_context: Some(16_384),
                max_output: Some(256),
                support_vision: Some(false),
                support_hearing: Some(false),
                stream: Some(false),
                stream_include_usage: Some(false),
                history_compaction_ratio: None,
                history_compaction_reset: None,
                tool_call_mode: Some("tool_call".to_string()),
                reasoning_effort: None,
                model_type: Some("embedding".to_string()),
                stop: None,
                mock_if_unconfigured: None,
            },
        );
    })
    .await;
    context.mock_state = Some(mock_state);
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

async fn mock_chat_completions(
    State(state): State<Arc<MockLlmState>>,
    Json(payload): Json<Value>,
) -> Json<Value> {
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    state.push_model(model);
    Json(json!({
        "id": "chatcmpl_agent_model_test",
        "object": "chat.completion",
        "created": 1_773_620_812,
        "model": "mock",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "ok",
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 32,
            "completion_tokens": 8,
            "total_tokens": 40
        }
    }))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agents_models_and_effective_model_fallback_work_end_to_end() {
    let context = build_test_context_with_mock_llm("agent_model_user").await;

    let (status, models_payload) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        "/wunder/agents/models",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        models_payload["data"]["items"],
        json!(["model-agent", "model-default"])
    );
    assert_eq!(
        models_payload["data"]["default_model_name"],
        json!("model-default")
    );

    let (status, created_agent) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/agents",
        Some(json!({
            "name": "Model Override Agent",
            "model_name": "model-agent"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let agent_id = created_agent["data"]["id"]
        .as_str()
        .expect("agent id")
        .to_string();
    assert_eq!(
        created_agent["data"]["configured_model_name"],
        json!("model-agent")
    );
    assert_eq!(created_agent["data"]["model_name"], json!("model-agent"));

    let (status, updated_agent) = send_json(
        &context.app,
        &context.token,
        Method::PUT,
        &format!("/wunder/agents/{agent_id}"),
        Some(json!({
            "model_name": ""
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated_agent["data"]["configured_model_name"], Value::Null);
    assert_eq!(updated_agent["data"]["model_name"], json!("model-default"));

    let (status, fetched_agent) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        &format!("/wunder/agents/{agent_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched_agent["data"]["configured_model_name"], Value::Null);
    assert_eq!(fetched_agent["data"]["model_name"], json!("model-default"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chat_uses_agent_model_override_instead_of_default_model() {
    let context = build_test_context_with_mock_llm("agent_model_chat_user").await;

    let (status, default_session) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/chat/sessions",
        Some(json!({
            "title": "Default model session"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let default_session_id = default_session["data"]["id"]
        .as_str()
        .expect("default session id");

    let (status, default_reply) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/chat/sessions/{default_session_id}/messages"),
        Some(json!({
            "content": "hello",
            "stream": false
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(default_reply["data"]["answer"], json!("ok"));

    let (status, created_agent) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/agents",
        Some(json!({
            "name": "Custom model agent",
            "model_name": "model-agent"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let agent_id = created_agent["data"]["id"].as_str().expect("agent id");

    let (status, agent_session) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/chat/sessions",
        Some(json!({
            "title": "Agent model session",
            "agent_id": agent_id
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let agent_session_id = agent_session["data"]["id"]
        .as_str()
        .expect("agent session id");

    let (status, agent_reply) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/chat/sessions/{agent_session_id}/messages"),
        Some(json!({
            "content": "hello again",
            "stream": false
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(agent_reply["data"]["answer"], json!("ok"));

    let mock_state = context.mock_state.expect("mock llm state");
    let models = mock_state.all_models();
    assert!(
        models.len() >= 2,
        "expected at least 2 model invocations, got {:?}",
        models
    );
    assert_eq!(models[0], "provider-default");
    assert_eq!(models[1], "provider-agent");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_detail_returns_persisted_context_tokens() {
    let context = build_test_context_with_mock_llm("session_context_tokens_user").await;

    let (status, created_session) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        "/wunder/chat/sessions",
        Some(json!({
            "title": "Context token session"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let session_id = created_session["data"]["id"].as_str().expect("session id");

    let (status, reply) = send_json(
        &context.app,
        &context.token,
        Method::POST,
        &format!("/wunder/chat/sessions/{session_id}/messages"),
        Some(json!({
            "content": "Please keep enough context for this thread.",
            "stream": false
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["data"]["answer"], json!("ok"));

    let (status, session_detail) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        &format!("/wunder/chat/sessions/{session_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        session_detail["data"]["context_tokens"]
            .as_i64()
            .unwrap_or_default()
            > 0,
        "expected persisted context tokens in session detail, got {session_detail:?}"
    );

    let (status, session_list) = send_json(
        &context.app,
        &context.token,
        Method::GET,
        "/wunder/chat/sessions",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let listed_context_tokens = session_list["data"]["items"]
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|item| item["id"] == json!(session_id))
                .and_then(|item| item.get("context_tokens").and_then(Value::as_i64))
        })
        .unwrap_or_default();
    assert!(
        listed_context_tokens > 0,
        "expected persisted context tokens in session list, got {session_list:?}"
    );
}
