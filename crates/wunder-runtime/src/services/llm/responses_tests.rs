use super::*;
use axum::body::Body;
use axum::extract::{Json, State};
use axum::http::header;
use axum::response::Response;
use axum::routing::post;
use axum::Router;
use std::sync::Mutex;

fn config() -> LlmModelConfig {
    LlmModelConfig {
        provider: Some("openai".into()),
        api_mode: Some("responses".into()),
        model: Some("gpt-5.2".into()),
        temperature: Some(0.4),
        reasoning_effort: Some("high".into()),
        stream_include_usage: Some(true),
        ..Default::default()
    }
}

#[test]
fn responses_official_request_uses_stateless_reasoning_and_compatible_fields() {
    let mut model = config();
    model.stop = Some(vec!["stop-marker".into()]);
    let tools = [json!({"type":"function","name":"lookup","parameters":{
        "type":"object","properties":{"label":{"type":"string"}}
    }})];
    let payload = LlmClient::new(Client::new(), model.clone()).build_request_payload_with_tools(
        &[],
        true,
        Some(&tools),
    );
    assert_eq!(payload["reasoning"]["effort"], "high");
    assert_eq!(payload["store"], false);
    assert_eq!(payload["include"], json!(["reasoning.encrypted_content"]));
    assert_eq!(payload["tools"][0]["strict"], false);
    for key in ["temperature", "stream_options", "stop"] {
        assert!(payload.get(key).is_none(), "unexpected {key}");
    }
    model.reasoning_effort = Some("none".into());
    let payload = LlmClient::new(Client::new(), model).build_request_payload(&[], false);
    assert_eq!(payload["temperature"], 0.4);
}

#[test]
fn responses_gpt_gateway_keeps_optional_arguments_and_cache_options() {
    let tools = [
        json!({"type":"function","function":{"name":"lookup","parameters":{
            "oneOf":[
                {"type":"object","properties":{"action":{"const":"list"}},"required":["action"]},
                {"type":"object","properties":{"action":{"const":"get"},"label":{"type":"string"}},"required":["action","label"]}
            ]
        }}}),
        json!({"type":"function","name":"explicit","strict":true}),
        json!({"type":"custom","name":"raw_tool","format":{"type":"text"}}),
    ];
    for provider in ["openai", "openai_compatible"] {
        let mut model = config();
        model.provider = Some(provider.into());
        model.base_url = Some("https://gateway.example.invalid/v1".into());
        let client =
            LlmClient::new(Client::new(), model).with_prompt_cache_key("thread-cache-key".into());
        let payload = client.build_request_payload_with_tools(&[], true, Some(&tools));
        assert_eq!(payload["tools"][0]["strict"], false);
        assert_eq!(
            payload["tools"][0]["parameters"]["required"],
            json!(["action"])
        );
        assert_eq!(
            payload["tools"][0]["parameters"]["properties"]["label"]["type"],
            "string"
        );
        assert!(payload["tools"][0]["parameters"].get("oneOf").is_none());
        assert_eq!(payload["tools"][1]["strict"], true);
        assert!(payload["tools"][2].get("strict").is_none());
        assert_eq!(payload["prompt_cache_key"], "thread-cache-key");
        assert_eq!(payload["include"], json!(["reasoning.encrypted_content"]));
        assert_eq!(payload["store"], false);
        assert!(payload.get("temperature").is_none());
    }
    let mut model = config();
    model.provider = Some("openai_compatible".into());
    model.model = Some("test-model".into());
    model.base_url = Some("https://gateway.example.invalid/v1".into());
    let client =
        LlmClient::new(Client::new(), model).with_prompt_cache_key("thread-cache-key".into());
    let payload = client.build_request_payload_with_tools(&[], false, Some(&tools));
    assert_eq!(payload["tools"][0]["strict"], false);
    assert!(payload["tools"][0]["parameters"].get("oneOf").is_some());
    for field in ["prompt_cache_key", "include", "store"] {
        assert!(payload.get(field).is_none(), "unexpected {field}");
    }
}

#[test]
fn responses_replay_does_not_duplicate_large_tool_arguments_in_model_history() {
    let body = json!({"output":[
        {"type":"reasoning","id":"rs_test","summary":[],"encrypted_content":"opaque-test-data"},
        {"type":"custom_tool_call","call_id":"call_test","name":"apply_patch","input":"x".repeat(32 * 1024)}
    ]});
    let replay = responses::OutputItems::from_body(&body, "test-scope".into()).unwrap();
    assert_eq!(replay["items"], json!([body["output"][0]]));
}

#[tokio::test]
async fn responses_round_trip_replays_native_items_in_order_after_history_reload() {
    type Requests = Arc<Mutex<Vec<Value>>>;
    let reasoning_item = json!({"type":"reasoning","id":"rs_test","summary":[],
        "encrypted_content":"opaque-test-data"});
    let function_item = json!({"type":"function_call","id":"fc_test","call_id":"call_test",
        "name":"lookup","arguments":" {\"label\":\"test\"} ","status":"completed"});
    let commentary_item = json!({"type":"message","id":"msg_commentary","role":"assistant",
        "phase":"commentary","status":"completed","content":[{"type":"output_text","text":"Checking.","annotations":[]}]});
    let custom_item = json!({"type":"custom_tool_call","id":"ctc_test","call_id":"call_custom",
        "name":"raw_tool","input":" {\"input\":42} \n"});
    let final_item = json!({"type":"message","id":"msg_final","role":"assistant",
        "phase":"final_answer","status":"completed","content":[{"type":"output_text","text":"done","annotations":[]}]});
    let events = [
        json!({"type":"response.output_item.done","output_index":0,"item":reasoning_item}),
        json!({"type":"response.output_item.added","output_index":1,"item":{
            "type":"function_call","id":"fc_test","call_id":"call_test","name":"lookup","arguments":""
        }}),
        json!({"type":"response.function_call_arguments.delta","item_id":"fc_test","delta":"{\"stale\":1}"}),
        json!({"type":"response.function_call_arguments.done","item_id":"fc_test","name":"lookup","arguments":"{\"label\":\"test\"}"}),
        json!({"type":"response.output_item.done","output_index":1,"item":function_item}),
        json!({"type":"response.output_item.done","output_index":2,"item":commentary_item}),
        json!({"type":"response.output_item.added","output_index":3,"item":{
            "type":"custom_tool_call","id":"ctc_test","call_id":"call_custom","name":"raw_tool","input":""
        }}),
        json!({"type":"response.custom_tool_call_input.delta","item_id":"ctc_test","delta":"old"}),
        json!({"type":"response.custom_tool_call_input.done","item_id":"ctc_test","input":" {\"input\":42} \n"}),
        json!({"type":"response.output_item.done","output_index":3,"item":custom_item}),
        json!({"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":20,"output_tokens":5},
            "output":[{"type":"reasoning","id":"rs_test","summary":[]},function_item,commentary_item,custom_item]}}),
    ].iter().map(|event| format!("data: {event}\n\n")).collect::<String>();
    let captured = Requests::default();
    let server_final_item = final_item.clone();
    let app = Router::new()
        .route(
            "/v1/responses",
            post(
                move |State(captured): State<Requests>, Json(payload): Json<Value>| {
                    let events = events.clone();
                    let final_item = server_final_item.clone();
                    async move {
                        let mut requests = captured.lock().unwrap();
                        requests.push(payload);
                        if requests.len() == 1 {
                            Response::builder()
                                .header(header::CONTENT_TYPE, "text/event-stream")
                                .body(Body::from(events))
                                .unwrap()
                        } else {
                            Response::builder()
                                .header(header::CONTENT_TYPE, "application/json")
                                .body(Body::from(
                                    json!({"status":"completed","output":[final_item]}).to_string(),
                                ))
                                .unwrap()
                        }
                    }
                },
            ),
        )
        .with_state(captured.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut model = config();
    model.base_url = Some(format!("http://{addr}/v1"));
    let client = LlmClient::new(Client::new(), model.clone())
        .with_prompt_cache_key("thread-cache-key".into());
    let tools = [
        json!({"type":"function","name":"lookup","parameters":{"type":"object"}}),
        json!({"type":"custom","name":"raw_tool","format":{"type":"text"}}),
    ];
    let mut messages = vec![ChatMessage {
        role: "user".into(),
        content: json!("test"),
        reasoning_content: None,
        anthropic_thinking_blocks: None,
        responses_reasoning: None,
        tool_calls: None,
        tool_call_id: None,
    }];
    let response = client
        .stream_complete_with_callback_with_tools(&messages, Some(&tools), |_, _| async { Ok(()) })
        .await
        .unwrap();
    assert_eq!(response.usage.unwrap().input, 20);
    let replay = response.responses_reasoning.unwrap();
    let calls = response.tool_calls.unwrap();
    assert_eq!(
        calls[0]["function"]["arguments"],
        function_item["arguments"]
    );
    assert_eq!(calls[1]["input_kind"], "text");
    assert_eq!(
        serde_json::from_str::<Value>(calls[1]["function"]["arguments"].as_str().unwrap()).unwrap(),
        json!({"input":" {\"input\":42} \n"})
    );
    assert_eq!(response.content, "Checking.");
    let mut persisted = json!({"role":"assistant","content":response.content,"tool_calls":calls,
        "meta":{"responses_reasoning":replay}});
    let reloaded =
        crate::storage::thread_store::compact_replayed_model_message(persisted.clone()).unwrap();
    crate::storage::thread_store::strip_responses_reasoning_from_chat_event(&mut persisted);
    assert!(persisted["meta"].get("responses_reasoning").is_none());
    messages.push(ChatMessage {
        role: "assistant".into(),
        content: reloaded["content"].clone(),
        reasoning_content: None,
        anthropic_thinking_blocks: None,
        responses_reasoning: Some(reloaded["responses_reasoning"].clone()),
        tool_calls: Some(reloaded["tool_calls"].clone()),
        tool_call_id: None,
    });
    for id in ["call_test", "call_custom"] {
        messages.push(ChatMessage {
            role: "tool".into(),
            content: json!("ok"),
            reasoning_content: None,
            anthropic_thinking_blocks: None,
            responses_reasoning: None,
            tool_calls: None,
            tool_call_id: Some(id.into()),
        });
    }
    let answer = client
        .complete_with_tools(&messages, Some(&tools))
        .await
        .unwrap();
    assert_eq!(answer.content, "done");
    let requests = captured.lock().unwrap();
    assert!(requests[0].get("stream_options").is_none());
    assert_eq!(
        requests[0]["prompt_cache_key"],
        requests[1]["prompt_cache_key"]
    );
    assert_eq!(requests[0]["tools"], requests[1]["tools"]);
    let input = requests[1]["input"].as_array().unwrap();
    assert!(input.starts_with(requests[0]["input"].as_array().unwrap()));
    assert_eq!(
        input[1..5],
        [reasoning_item, function_item, commentary_item, custom_item]
    );
    assert_eq!(
        input[5],
        json!({"type":"function_call_output","call_id":"call_test","output":"ok"})
    );
    assert_eq!(
        input[6],
        json!({"type":"custom_tool_call_output","call_id":"call_custom","output":"ok"})
    );
    messages.push(ChatMessage {
        role: "assistant".into(),
        content: json!(answer.content),
        reasoning_content: None,
        anthropic_thinking_blocks: None,
        responses_reasoning: answer.responses_reasoning,
        tool_calls: None,
        tool_call_id: None,
    });
    let next_request = client.build_request_payload_with_tools(&messages, true, Some(&tools));
    let next_input = next_request["input"].as_array().unwrap();
    assert!(next_input.starts_with(input));
    assert_eq!(next_input.last(), Some(&final_item));

    let mut filtered = messages.clone();
    let assistant = &mut filtered[1];
    assistant
        .tool_calls
        .as_mut()
        .unwrap()
        .as_array_mut()
        .unwrap()
        .pop();
    filtered.remove(3);
    let filtered_request = client.build_request_payload_with_tools(&filtered, false, Some(&tools));
    let filtered_input = filtered_request["input"].as_array().unwrap();
    assert!(!filtered_input
        .iter()
        .any(|item| item["call_id"] == "call_custom"));
    assert_eq!(filtered_input[2]["content"], "Checking.");
    assert_eq!(filtered_input[3]["call_id"], "call_test");

    model.model = Some("other-model".into());
    let payload = LlmClient::new(Client::new(), model).build_request_payload_with_tools(
        &messages,
        false,
        Some(&tools),
    );
    assert!(!payload["input"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["type"] == "reasoning"));
    server.abort();
}
