use super::*;

fn config(provider: &str) -> LlmModelConfig {
    LlmModelConfig {
        enable: Some(true),
        provider: Some(provider.to_string()),
        model: Some("test-model".to_string()),
        temperature: Some(0.7),
        stream: Some(false),
        stream_include_usage: Some(false),
        model_type: Some("llm".to_string()),
        ..Default::default()
    }
}

fn messages() -> Vec<ChatMessage> {
    vec![
        ChatMessage {
            role: "system".to_string(),
            content: Value::String("You are a test assistant.".to_string()),
            reasoning_content: None,
            anthropic_thinking_blocks: None,
            responses_reasoning: None,
            tool_calls: None,
            tool_call_id: None,
        },
        ChatMessage {
            role: "user".to_string(),
            content: Value::String("list files".to_string()),
            reasoning_content: None,
            anthropic_thinking_blocks: None,
            responses_reasoning: None,
            tool_calls: None,
            tool_call_id: None,
        },
        ChatMessage {
            role: "assistant".to_string(),
            content: Value::String(String::new()),
            reasoning_content: None,
            anthropic_thinking_blocks: None,
            responses_reasoning: None,
            tool_calls: Some(json!([{
                "id": "call_1",
                "type": "function",
                "function": { "name": "list_files", "arguments": "{\"path\":\".\"}" }
            }])),
            tool_call_id: None,
        },
        ChatMessage {
            role: "tool".to_string(),
            content: Value::String("[\"src\",\"Cargo.toml\"]".to_string()),
            reasoning_content: None,
            anthropic_thinking_blocks: None,
            responses_reasoning: None,
            tool_calls: None,
            tool_call_id: Some("call_1".to_string()),
        },
    ]
}

fn tools() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "list_files",
                "description": "List files under a path",
                "parameters": { "type": "object", "properties": {} }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read a file",
                "parameters": { "type": "object", "properties": {} }
            }
        }),
    ]
}

fn cache_control_count(value: &Value) -> usize {
    match value {
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| usize::from(key == "cache_control") + cache_control_count(value))
            .sum(),
        Value::Array(items) => items.iter().map(cache_control_count).sum(),
        _ => 0,
    }
}

#[test]
fn anthropic_cache_marks_previous_tail_and_uses_extended_static_ttl() {
    let previous_tail = json!([{"type": "text", "text": "list files"}]);
    let sink = Arc::new(parking_lot::Mutex::new(None));
    let client = LlmClient::new(Client::new(), config("anthropic")).with_anthropic_cache_state(
        Some(previous_tail),
        Arc::clone(&sink),
        true,
    );
    let payload = client.build_request_payload_with_tools(&messages(), false, Some(&tools()));

    assert_eq!(cache_control_count(&payload), 4);
    assert_eq!(
        payload["tools"][1]["cache_control"],
        json!({"type": "ephemeral", "ttl": "1h"})
    );
    assert_eq!(
        payload["system"][0]["cache_control"],
        json!({"type": "ephemeral", "ttl": "1h"})
    );
    assert_eq!(
        payload["messages"][0]["content"][0]["cache_control"],
        json!({"type": "ephemeral"})
    );
    assert_eq!(
        payload["messages"][2]["content"][0]["cache_control"],
        json!({"type": "ephemeral"})
    );
    assert_eq!(
        sink.lock().clone(),
        Some(json!([{
            "type": "tool_result",
            "tool_use_id": "call_1",
            "content": "[\"src\",\"Cargo.toml\"]"
        }]))
    );
}

#[test]
fn anthropic_cache_tail_skips_thinking_only_assistant_output() {
    let mut replay = messages();
    replay.truncate(3);
    replay[2].anthropic_thinking_blocks = Some(json!([
        {"type": "thinking", "thinking": "step one", "signature": "signed-abc"}
    ]));
    replay[2].tool_calls = None;
    let sink = Arc::new(parking_lot::Mutex::new(None));
    let client = LlmClient::new(Client::new(), config("anthropic")).with_anthropic_cache_state(
        None,
        Arc::clone(&sink),
        false,
    );
    let payload = client.build_request_payload(&replay, false);

    assert!(
        payload["messages"][1]["content"][0]
            .get("cache_control")
            .is_none()
    );
    assert_eq!(
        payload["messages"][0]["content"][0]["cache_control"],
        json!({"type": "ephemeral"})
    );
    assert_eq!(
        sink.lock().clone(),
        Some(json!([{"type": "text", "text": "list files"}]))
    );
}

#[test]
fn openai_prompt_cache_key_is_scoped_to_official_openai_payloads() {
    let mut replay = messages();
    replay[2].anthropic_thinking_blocks = Some(json!([
        {"type": "thinking", "thinking": "step one", "signature": "signed-abc"}
    ]));
    for api_mode in ["chat_completions", "responses"] {
        let mut model = config("openai");
        model.api_mode = Some(api_mode.into());
        let client =
            LlmClient::new(Client::new(), model).with_prompt_cache_key("thread-cache-key".into());
        let payload = client.build_request_payload(&replay, false);
        assert_eq!(payload["prompt_cache_key"], json!("thread-cache-key"));
        assert!(!payload.to_string().contains("anthropic_thinking_blocks"));
    }
    let client = LlmClient::new(Client::new(), config("openai_compatible"))
        .with_prompt_cache_key("thread-cache-key".into());
    let payload = client.build_request_payload(&replay, false);
    assert!(payload.get("prompt_cache_key").is_none());
    assert!(!payload.to_string().contains("anthropic_thinking_blocks"));
}

#[tokio::test]
async fn responses_round_trip_sends_cache_key_and_normalizes_nested_usage() {
    use axum::Router;
    use axum::extract::Json;
    use axum::routing::post;
    use std::sync::Mutex as StdMutex;
    use tokio::net::TcpListener;

    let captured = Arc::new(StdMutex::new(Vec::new()));
    let captured_by_route = Arc::clone(&captured);
    let app = Router::new().route(
        "/v1/responses",
        post(move |Json(payload): Json<Value>| {
            let captured = Arc::clone(&captured_by_route);
            async move {
                captured.lock().expect("capture request").push(payload);
                Json(json!({
                    "object": "response",
                    "status": "completed",
                    "output": [{
                        "type": "message",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": "done"}]
                    }],
                    "usage": {
                        "input_tokens": 100,
                        "input_tokens_details": {"cached_tokens": 80, "cache_write_tokens": 5},
                        "output_tokens": 20,
                        "total_tokens": 120
                    }
                }))
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve test app");
    });

    let mut model = config("openai");
    model.api_mode = Some("responses".into());
    model.base_url = Some(format!("http://{addr}/v1"));
    let client =
        LlmClient::new(Client::new(), model).with_prompt_cache_key("thread-cache-key".into());
    let response = client
        .complete(&messages())
        .await
        .expect("responses round trip");
    assert_eq!(response.content, "done");
    let usage = response.usage.expect("provider usage");
    assert_eq!(usage.input, 15);
    assert_eq!(usage.cache_read, 80);
    assert_eq!(usage.cache_write, 5);
    assert_eq!(usage.prompt_tokens(), 100);
    assert_eq!(usage.total, 120);
    let requests = captured.lock().expect("read captured request");
    assert_eq!(requests[0]["prompt_cache_key"], json!("thread-cache-key"));
}

#[tokio::test]
async fn anthropic_stream_round_trip_returns_cache_usage_and_signed_thinking() {
    use axum::Router;
    use axum::body::Body;
    use axum::extract::Json;
    use axum::http::StatusCode;
    use axum::response::Response;
    use axum::routing::post;
    use std::sync::Mutex as StdMutex;
    use tokio::net::TcpListener;

    let captured = Arc::new(StdMutex::new(Vec::new()));
    let captured_by_route = Arc::clone(&captured);
    let app = Router::new().route(
        "/v1/messages",
        post(move |Json(payload): Json<Value>| {
            let captured = Arc::clone(&captured_by_route);
            async move {
                captured.lock().expect("capture request").push(payload);
                let sse = concat!(
                    "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":12,\"cache_read_input_tokens\":80,\"cache_creation_input_tokens\":5,\"output_tokens\":0}}}\n\n",
                    "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"step \"}}\n\n",
                    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"one\"}}\n\n",
                    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"signed-abc\"}}\n\n",
                    "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                    "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"done\"}}\n\n",
                    "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
                    "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":3}}\n\n",
                    "data: {\"type\":\"message_stop\"}\n\n"
                );
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .body(Body::from(sse))
                    .expect("build anthropic sse response")
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve test app");
    });

    let mut model = config("anthropic");
    model.base_url = Some(format!("http://{addr}/v1"));
    model.stream = Some(true);
    let previous_tail = json!([{"type": "text", "text": "list files"}]);
    let sink = Arc::new(parking_lot::Mutex::new(None));
    let client = LlmClient::new(Client::new(), model).with_anthropic_cache_state(
        Some(previous_tail),
        Arc::clone(&sink),
        true,
    );
    let response = client
        .stream_complete_with_callback_with_tools(
            &messages(),
            Some(&tools()),
            |_delta, _reasoning| async { Ok(()) },
        )
        .await
        .expect("anthropic stream round trip");

    assert_eq!(response.content, "done");
    assert_eq!(response.reasoning, "step one");
    assert_eq!(
        response.anthropic_thinking_blocks,
        Some(json!([
            {"type": "thinking", "thinking": "step one", "signature": "signed-abc"}
        ]))
    );
    let usage = response.usage.expect("provider usage");
    assert_eq!(usage.input, 12);
    assert_eq!(usage.cache_read, 80);
    assert_eq!(usage.cache_write, 5);
    assert_eq!(usage.prompt_tokens(), 97);
    assert_eq!(usage.total, 100);

    let requests = captured.lock().expect("read captured request");
    assert_eq!(requests.len(), 1);
    assert_eq!(cache_control_count(&requests[0]), 4);
    assert_eq!(
        requests[0]["system"][0]["cache_control"],
        json!({"type": "ephemeral", "ttl": "1h"})
    );
}
