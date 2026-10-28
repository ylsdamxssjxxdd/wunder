use super::*;
use axum::{body::Body, http::StatusCode, response::Response, routing::post, Router};
use std::sync::atomic::{AtomicUsize, Ordering};

async fn fixture(
    status: StatusCode,
    body: String,
) -> (LlmClient, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let captured = calls.clone();
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            captured.fetch_add(1, Ordering::SeqCst);
            let body = body.clone();
            async move {
                Response::builder()
                    .status(status)
                    .body(Body::from(body))
                    .unwrap()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = LlmClient::new(
        Client::new(),
        LlmModelConfig {
            provider: Some("openai_compatible".into()),
            model: Some("fixture-model".into()),
            base_url: Some(format!("http://{address}/v1")),
            ..Default::default()
        },
    );
    (client, calls, server)
}

fn messages() -> Vec<ChatMessage> {
    vec![ChatMessage {
        role: "user".into(),
        content: json!("input"),
        reasoning_content: None,
        tool_calls: None,
        tool_call_id: None,
    }]
}

#[tokio::test]
async fn transient_http_errors_never_bypass_the_engine_backoff() {
    for status in [408, 409, 425, 429, 500, 502, 503, 504, 529] {
        let (client, calls, server) = fixture(
            StatusCode::from_u16(status).unwrap(),
            json!({"error":{"message":"temporary failure"}}).to_string(),
        )
        .await;
        let error = client
            .stream_complete_with_callback(&messages(), |_, _| async { Ok(()) })
            .await
            .unwrap_err();
        assert_eq!(error.downcast_ref::<LlmHttpError>().unwrap().status, status);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "no hidden fallback for {status}"
        );
        server.abort();
    }
}

#[tokio::test]
async fn permanent_http_errors_preserve_status_and_do_not_fallback() {
    for status in [400, 401, 403, 404, 422] {
        let (client, calls, server) = fixture(
            StatusCode::from_u16(status).unwrap(),
            json!({"error":{"message":"upstream timeout 429"}}).to_string(),
        )
        .await;
        for stream in [false, true] {
            let error = if stream {
                client
                    .stream_complete_with_callback(&messages(), |_, _| async { Ok(()) })
                    .await
                    .unwrap_err()
            } else {
                client.complete(&messages()).await.unwrap_err()
            };
            let failure = error.downcast_ref::<LlmHttpError>().unwrap();
            assert_eq!(failure.status, status);
            assert!(!failure.retryable());
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        server.abort();
    }
    let quota = LlmHttpError::new(429, r#"{"error":{"code":"insufficient_quota"}}"#);
    assert!(!quota.retryable());
}

#[tokio::test]
async fn unfinished_streams_never_become_successful_answers_or_tool_calls() {
    for payload in [
        json!({"choices":[{"delta":{"content":"partial"}}]}),
        json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_fixture","type":"function","function":{"name":"write_file","arguments":"{\"path\":\"probe.txt\",\"content\":\"partial\"}"}}]}}]}),
        json!({"choices":[],"usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}}),
    ] {
        let (client, _, server) = fixture(StatusCode::OK, format!("data: {payload}\n\n")).await;
        let error = client
            .stream_complete_with_callback(&messages(), |_, _| async { Ok(()) })
            .await
            .unwrap_err();
        assert!(error.to_string().contains("without a completed response"));
        server.abort();
    }
}

#[tokio::test]
async fn terminal_markers_and_finish_reasons_remain_compatible() {
    for body in [
        "data: {\"choices\":[{\"delta\":{\"content\":\"complete\"},\"finish_reason\":\"stop\"}]}".to_string(),
        "data: {\"choices\":[{\"delta\":{\"content\":\"complete\"}}]}\n\ndata: [DONE]\n\n".to_string(),
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"complete\"}]}]}}\n\n".to_string(),
        "data: {\"type\":\"message_stop\"}\n\n".to_string(),
    ] {
        let (client, _, server) = fixture(StatusCode::OK, body).await;
        client.stream_complete_with_callback(&messages(), |_, _| async { Ok(()) }).await.unwrap();
        server.abort();
    }
}

#[tokio::test]
async fn successful_http_status_never_hides_provider_failures() {
    for payload in [
        json!({"error":{"code":"invalid_api_key","message":"fixture rejection"}}),
        json!({"type":"error","code":"rate_limit_exceeded","message":"fixture throttled"}),
        json!({"status":"failed","error":null}),
        json!({"type":"response.failed","response":{"status":"failed","error":{"code":"server_error","message":"fixture unavailable"}}}),
        json!({"status":"completed","error":{"code":"server_error","message":"fixture unavailable"},"output":[{"type":"function_call","call_id":"call_fixture","name":"write_file","arguments":"{}"}]}),
    ] {
        for stream in [false, true] {
            let body = if stream {
                format!("data: {payload}\n\ndata: [DONE]\n\n")
            } else {
                payload.to_string()
            };
            let (client, calls, server) = fixture(StatusCode::OK, body).await;
            let result = if stream {
                client
                    .stream_complete_with_callback(&messages(), |_, _| async { Ok(()) })
                    .await
            } else {
                client.complete(&messages()).await
            };
            assert!(
                result.is_err(),
                "failure accepted: stream={stream}, payload={payload}"
            );
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            server.abort();
        }
    }
}

#[tokio::test]
async fn null_error_fields_do_not_reject_completed_responses() {
    let payload = json!({"status":"completed","error":null,"output":[{"type":"message","content":[{"type":"output_text","text":"completed"}]}]});
    for stream in [false, true] {
        let body = if stream {
            format!(
                "data: {}\n\n",
                json!({"type":"response.completed","response":payload})
            )
        } else {
            payload.to_string()
        };
        let (client, _, server) = fixture(StatusCode::OK, body).await;
        let response = if stream {
            client
                .stream_complete_with_callback(&messages(), |_, _| async { Ok(()) })
                .await
        } else {
            client.complete(&messages()).await
        }
        .unwrap();
        assert_eq!(response.content, "completed");
        server.abort();
    }
}

async fn fragmented_fixture(body: Vec<u8>) -> (LlmClient, tokio::task::JoinHandle<()>) {
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            let body = body.clone();
            async move {
                let chunks =
                    futures::stream::unfold(body.into_iter(), |mut remaining| async move {
                        let byte = remaining.next()?;
                        tokio::time::sleep(Duration::from_millis(2)).await;
                        Some((Ok::<_, std::io::Error>(vec![byte]), remaining))
                    });
                Response::builder().body(Body::from_stream(chunks)).unwrap()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = LlmClient::new(
        Client::new(),
        LlmModelConfig {
            provider: Some("openai_compatible".into()),
            model: Some("fixture-model".into()),
            base_url: Some(format!("http://{address}/v1")),
            ..Default::default()
        },
    );
    (client, server)
}

#[tokio::test]
async fn fragmented_utf8_preserves_content_reasoning_and_tool_arguments() {
    let content = "示例🙂";
    let reasoning = "检查✓";
    let arguments = json!({"path":"示例.txt","content":"内容🙂"}).to_string();
    let payload = json!({"choices":[{"delta":{"content":content,"reasoning_content":reasoning,"tool_calls":[{"index":0,"id":"call_fixture","type":"function","function":{"name":"write_file","arguments":arguments}}]},"finish_reason":"tool_calls"}]});
    let (client, server) =
        fragmented_fixture(format!("data: {payload}\n\ndata: [DONE]\n\n").into_bytes()).await;
    let mut visible = String::new();
    let mut thought = String::new();
    let response = client
        .stream_complete_with_callback(&messages(), |text, analysis| {
            visible.push_str(&text);
            thought.push_str(&analysis);
            std::future::ready(Ok(()))
        })
        .await
        .unwrap();
    assert_eq!(response.content, content);
    assert_eq!(response.reasoning, reasoning);
    assert_eq!(visible, content);
    assert_eq!(thought, reasoning);
    assert_eq!(
        response.tool_calls.unwrap()[0]["function"]["arguments"],
        arguments
    );
    server.abort();
}

#[tokio::test]
async fn malformed_stream_events_cannot_be_hidden_by_a_done_marker() {
    let mut invalid_utf8 = b"data: {\"choices\":[{\"delta\":{\"content\":\"".to_vec();
    invalid_utf8.push(0xff);
    invalid_utf8.extend_from_slice(b"\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n");
    for body in [
        invalid_utf8,
        b"data: {invalid-json}\n\ndata: [DONE]\n\n".to_vec(),
    ] {
        let (client, server) = fragmented_fixture(body).await;
        assert!(client
            .stream_complete_with_callback(&messages(), |_, _| async { Ok(()) })
            .await
            .is_err());
        server.abort();
    }
}
