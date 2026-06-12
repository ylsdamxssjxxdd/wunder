use super::*;
    use super::*;
    use crate::api::build_desktop_router;
    use crate::config::{Config, LlmModelConfig};
    use crate::config_store::ConfigStore;
    use crate::services::memory_fragments::{MemoryFragmentInput, MemoryFragmentStore};
    use crate::state::{AppState, AppStateInitOptions};
    use crate::storage::{SqliteStorage, StorageBackend};
    use axum::{
        body::{to_bytes, Body},
        extract::State,
        http::{header::AUTHORIZATION, Method, Request, StatusCode},
        routing::post,
        Json, Router,
    };
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::net::TcpListener;
    use tokio::time::{sleep, Duration};
    use tower::ServiceExt;

    fn append_chat_record(
        storage: &Arc<dyn StorageBackend>,
        user_id: &str,
        session_id: &str,
        role: &str,
        content: &str,
    ) {
        storage
            .append_chat(
                user_id,
                &json!({
                    "role": role,
                    "content": content,
                    "session_id": session_id,
                    "timestamp": "2026-03-15T00:00:00Z"
                }),
            )
            .expect("append chat record");
    }

    #[derive(Default)]
    struct MockLlmState {
        total_calls: AtomicUsize,
        chat_calls: AtomicUsize,
        extraction_calls: AtomicUsize,
    }

    struct TestContext {
        app: Router,
        token: String,
        mock_llm_state: Arc<MockLlmState>,
        _temp_dir: tempfile::TempDir,
    }

    async fn build_test_context_with_mock_llm(username: &str) -> TestContext {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let (base_url, mock_llm_state) = spawn_mock_llm_server().await;
        let mut config = Config::default();
        config.storage.backend = "sqlite".to_string();
        config.storage.db_path = temp_dir
            .path()
            .join("memory-auto-extract-e2e.db")
            .to_string_lossy()
            .to_string();
        config.workspace.root = temp_dir
            .path()
            .join("workspaces")
            .to_string_lossy()
            .to_string();
        config.llm.default = "mock-auto-memory".to_string();
        config.llm.models.insert(
            "mock-auto-memory".to_string(),
            LlmModelConfig {
                enable: Some(true),
                provider: Some("openai".to_string()),
                api_mode: None,
                base_url: Some(base_url),
                api_key: Some("memory-test-key".to_string()),
                model: Some("mock-auto-memory".to_string()),
                temperature: Some(0.0),
                timeout_s: Some(15),
                max_rounds: Some(4),
                max_context: Some(16_384),
                max_output: Some(512),
                thinking_token_budget: None,
                support_vision: Some(false),
                support_hearing: Some(false),
                stream: Some(false),
                stream_include_usage: Some(false),
                history_compaction_ratio: None,
                tool_call_mode: Some("tool_call".to_string()),
                reasoning_effort: None,
                model_type: None,
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
            app: build_desktop_router(state),
            token,
            mock_llm_state,
            _temp_dir: temp_dir,
        }
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
                let job_ready = payload["data"]["recent_jobs"]
                    .as_array()
                    .and_then(|jobs| jobs.first())
                    .and_then(|job| job["status"].as_str())
                    .map(|status| matches!(status, "completed" | "skipped"))
                    .unwrap_or(false);
                if job_ready {
                    return payload;
                }
            }
            sleep(Duration::from_millis(150)).await;
        }
        panic!("memory fragments or auto extract job did not reach expected state");
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
                eprintln!("[memory_auto_extract] mock llm server failed: {err}");
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

    #[test]
    fn parse_llm_response_supports_tagged_json_payload() {
        let parsed = MemoryAutoExtractService::parse_llm_response(
            r#"
<memory_fragments>
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
      "importance": 0.9,
      "confidence": 0.95
    }
  ]
}
</memory_fragments>
"#,
        )
        .expect("parse llm response");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].category, "profile");
        assert_eq!(parsed[0].slot, "name");
        assert_eq!(parsed[0].summary, "用户姓名是周华健");
    }

    #[test]
    fn apply_llm_candidates_keeps_manual_memory_intact() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("memory-llm-manual.db");
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let fragment_store = MemoryFragmentStore::new(storage.clone());
        let service = MemoryAutoExtractService::new(storage.clone());

        let manual = fragment_store
            .save_fragment(
                "u1",
                Some("agent-demo"),
                MemoryFragmentInput {
                    source_type: Some("manual".to_string()),
                    category: Some("response-preference".to_string()),
                    title_l0: Some("默认使用中文回复".to_string()),
                    summary_l1: Some("默认使用中文回复".to_string()),
                    content_l2: Some("默认使用中文回复".to_string()),
                    fact_key: Some("constraint::reply_language".to_string()),
                    ..Default::default()
                },
            )
            .expect("save manual fragment");

        let outcome = service
            .apply_llm_candidates(
                "u1",
                Some("agent-demo"),
                "s1",
                Some("round-1"),
                vec![LlmExtractionCandidate {
                    category: "response-preference".to_string(),
                    slot: "reply_language".to_string(),
                    title: "默认使用英文回复".to_string(),
                    summary: "默认使用英文回复".to_string(),
                    content: "用户希望以后默认英文回复".to_string(),
                    tags: vec!["language".to_string(), "reply".to_string()],
                    tier: "core".to_string(),
                    importance: 0.9,
                    confidence: 0.9,
                }],
            )
            .expect("apply llm candidates");

        assert_eq!(
            outcome,
            MemoryAutoExtractOutcome {
                created: 0,
                updated: 0,
                skipped: 1
            }
        );
        let stored = fragment_store
            .get_fragment("u1", Some("agent-demo"), &manual.memory_id)
            .expect("get manual fragment");
        assert_eq!(stored.source_type, "manual");
        assert_eq!(stored.summary_l1, "默认使用中文回复");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn auto_extract_uses_mock_llm_end_to_end_in_lib() {
        let context = build_test_context_with_mock_llm("memory_auto_extract_lib_user").await;

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

        assert!(context.mock_llm_state.chat_calls.load(Ordering::Relaxed) >= 1);
        assert!(
            context
                .mock_llm_state
                .extraction_calls
                .load(Ordering::Relaxed)
                >= 1
        );
        assert!(context.mock_llm_state.total_calls.load(Ordering::Relaxed) >= 2);
    }

    #[test]
    fn extract_candidates_detects_response_preference() {
        let items = extract_candidates("以后请用中文回复，回答尽量简洁，不要用表格。");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].fact_key, "constraint::reply_language");
        assert_eq!(items[1].fact_key, "constraint::response_style");
        assert_eq!(items[2].fact_key, "constraint::response_format");
    }

    #[test]
    fn extract_candidates_collects_multiple_preferences_from_one_segment() {
        let items = extract_candidates("以后请用中文回复并尽量简洁且不要表格");
        let fact_keys = items
            .into_iter()
            .map(|item| item.fact_key)
            .collect::<Vec<_>>();
        assert_eq!(
            fact_keys,
            vec![
                "constraint::reply_language".to_string(),
                "constraint::response_style".to_string(),
                "constraint::response_format".to_string(),
            ]
        );
    }

    #[test]
    fn extract_candidates_inherits_reply_context_across_segments() {
        let fact_keys = extract_candidates("回答尽量简洁，不要表格。")
            .into_iter()
            .map(|item| item.fact_key)
            .collect::<Vec<_>>();
        assert_eq!(
            fact_keys,
            vec![
                "constraint::response_style".to_string(),
                "constraint::response_format".to_string(),
            ]
        );
    }

    #[test]
    fn extract_candidates_skips_profile_questions() {
        let fact_keys = extract_candidates("我叫什么")
            .into_iter()
            .map(|item| item.fact_key)
            .collect::<Vec<_>>();
        assert!(!fact_keys.contains(&"profile::name".to_string()));

        let fact_keys = extract_candidates("我是谁")
            .into_iter()
            .map(|item| item.fact_key)
            .collect::<Vec<_>>();
        assert!(!fact_keys.contains(&"profile::identity".to_string()));
    }

    #[test]
    fn capture_turn_creates_and_updates_auto_memory() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("memory-auto.db");
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let service = MemoryAutoExtractService::new(storage.clone());

        let first = service
            .capture_turn(
                "u1",
                Some("agent-demo"),
                "s1",
                Some("1"),
                "以后请用中文回复，回答尽量简洁。",
                "好的。",
            )
            .expect("first capture");
        assert_eq!(
            first,
            MemoryAutoExtractOutcome {
                created: 2,
                updated: 0,
                skipped: 0,
            }
        );

        let second = service
            .capture_turn(
                "u1",
                Some("agent-demo"),
                "s1",
                Some("2"),
                "以后请用中文回复，回答尽量详细。",
                "收到。",
            )
            .expect("second capture");
        assert_eq!(
            second,
            MemoryAutoExtractOutcome {
                created: 0,
                updated: 1,
                skipped: 1,
            }
        );

        let items = storage
            .list_memory_fragments("u1", "agent-demo")
            .expect("list memory fragments");
        assert!(items
            .iter()
            .any(|item| item.fact_key == "constraint::reply_language"));
        assert!(items.iter().any(|item| {
            item.fact_key == "constraint::response_style" && item.summary_l1.contains("详细")
        }));
    }

    #[test]
    fn capture_turn_uses_recent_user_window() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("memory-window.db");
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let service = MemoryAutoExtractService::new(storage.clone());

        append_chat_record(&storage, "u1", "s-window", "user", "以后请用中文回复。");
        append_chat_record(&storage, "u1", "s-window", "assistant", "好的。");

        let outcome = service
            .capture_turn(
                "u1",
                Some("agent-demo"),
                "s-window",
                Some("9"),
                "另外回答尽量简洁，不要表格。",
                "收到。",
            )
            .expect("capture turn with recent window");

        assert_eq!(
            outcome,
            MemoryAutoExtractOutcome {
                created: 3,
                updated: 0,
                skipped: 0,
            }
        );

        let items = storage
            .list_memory_fragments("u1", "agent-demo")
            .expect("list memory fragments");
        let fact_keys = items
            .into_iter()
            .map(|item| item.fact_key)
            .collect::<Vec<_>>();
        assert!(fact_keys.contains(&"constraint::reply_language".to_string()));
        assert!(fact_keys.contains(&"constraint::response_style".to_string()));
        assert!(fact_keys.contains(&"constraint::response_format".to_string()));
    }

    #[test]
    fn capture_turn_keeps_manual_memory_intact() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("memory-manual.db");
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let service = MemoryAutoExtractService::new(storage.clone());
        let fragment_store = MemoryFragmentStore::new(storage.clone());
        let manual = fragment_store
            .save_fragment(
                "u1",
                Some("agent-demo"),
                MemoryFragmentInput {
                    source_type: Some("manual".to_string()),
                    category: Some("response-preference".to_string()),
                    title_l0: Some(TITLE_REPLY_ZH.to_string()),
                    summary_l1: Some(TITLE_REPLY_ZH.to_string()),
                    content_l2: Some(TITLE_REPLY_ZH.to_string()),
                    fact_key: Some("constraint::reply_language".to_string()),
                    ..Default::default()
                },
            )
            .expect("save manual fragment");

        let outcome = service
            .capture_turn(
                "u1",
                Some("agent-demo"),
                "s1",
                Some("3"),
                "Please reply in English from now on.",
                "Sure.",
            )
            .expect("capture turn");

        assert_eq!(
            outcome,
            MemoryAutoExtractOutcome {
                created: 0,
                updated: 0,
                skipped: 1,
            }
        );

        let stored = storage
            .get_memory_fragment("u1", "agent-demo", &manual.memory_id)
            .expect("get fragment")
            .expect("fragment exists");
        assert_eq!(stored.source_type, "manual");
        assert_eq!(stored.summary_l1, TITLE_REPLY_ZH.to_string());
    }

    #[test]
    fn capture_turn_supersedes_changed_auto_memory() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("memory-auto-supersede.db");
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let service = MemoryAutoExtractService::new(storage.clone());
        let fragment_store = MemoryFragmentStore::new(storage.clone());

        let original = fragment_store
            .save_fragment(
                "u1",
                Some("agent-demo"),
                MemoryFragmentInput {
                    source_type: Some("auto_turn".to_string()),
                    category: Some("response-preference".to_string()),
                    title_l0: Some("Response format".to_string()),
                    summary_l1: Some("Use markdown tables.".to_string()),
                    content_l2: Some(
                        "When possible, present answers as markdown tables.".to_string(),
                    ),
                    fact_key: Some("constraint::response_format".to_string()),
                    ..Default::default()
                },
            )
            .expect("save original fragment");

        let outcome = service
            .capture_turn(
                "u1",
                Some("agent-demo"),
                "s1",
                Some("3"),
                "回答尽量简洁，不要表格。",
                "好的，我会改用简洁的要点列表。",
            )
            .expect("capture turn");

        assert!(outcome.updated >= 1);

        let items = storage
            .list_memory_fragments("u1", "agent-demo")
            .expect("list memory fragments");
        let response_format_items = items
            .iter()
            .filter(|item| item.fact_key == "constraint::response_format")
            .collect::<Vec<_>>();
        assert_eq!(response_format_items.len(), 2);

        let previous = response_format_items
            .iter()
            .find(|item| item.memory_id == original.memory_id)
            .expect("previous fragment exists");
        assert_eq!(previous.status, "superseded");

        let current = response_format_items
            .iter()
            .find(|item| item.memory_id != original.memory_id)
            .expect("current fragment exists");
        assert_eq!(current.status, "active");
        assert_eq!(
            current.supersedes_memory_id.as_deref(),
            Some(original.memory_id.as_str())
        );
        assert_eq!(
            previous.superseded_by_memory_id.as_deref(),
            Some(current.memory_id.as_str())
        );
    }
