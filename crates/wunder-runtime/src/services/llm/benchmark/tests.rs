use super::*;

#[tokio::test]
async fn simulated_profiles_include_reasoning_in_the_exact_output_budget() {
    use crate::services::virtual_llm::timing::VirtualModelSpeed::{Fast, Medium, Slow};
    for speed in [Fast, Medium, Slow] {
        let mut progress = Vec::new();
        let result = simulate(&messages(64), 20, speed, |metrics| progress.push(metrics))
            .await
            .unwrap();
        assert_eq!(
            (
                result.output_tokens,
                result.reasoning_tokens,
                result.target_reached
            ),
            (Some(20), Some(5), Some(true))
        );
        assert_eq!(progress.first().unwrap().reasoning_tokens, Some(1));
        assert!(
            progress
                .iter()
                .all(|metrics| metrics.reasoning_tokens
                    == Some(metrics.output_tokens.unwrap().min(5)))
        );
        let expected = speed
            .prefill_duration(result.input_tokens.unwrap())
            .as_secs_f64()
            * 1000.0;
        assert!(result.ttft_ms.unwrap() >= expected);
        assert!(
            (result.decode_tps.unwrap() / speed.generation_tokens_per_second() as f64 - 1.0).abs()
                < 0.15
        );
    }
}

fn model(provider: &str) -> LlmModelConfig {
    crate::rustls_provider::install_process_default_provider();
    LlmModelConfig {
        provider: Some(provider.into()),
        model: Some("model".into()),
        ..Default::default()
    }
}

#[test]
fn benchmark_controls_are_provider_specific_and_clear_stop_sequences() {
    for (provider, mode, field, fixed) in [
        ("vllm", "chat_completions", "max_tokens", true),
        ("openai", "responses", "max_output_tokens", false),
        ("anthropic", "chat_completions", "max_tokens", false),
    ] {
        let mut config = model(provider);
        config.api_mode = Some(mode.into());
        config.stop = Some(vec!["stop".into()]);
        let client = LlmClient::new(reqwest::Client::new(), config);
        let payload = client.benchmark_payload(&messages(1024), 4096);
        assert_eq!(
            (
                payload[field].as_u64(),
                payload.get("min_tokens").and_then(Value::as_u64),
                payload.get("ignore_eos").and_then(Value::as_bool)
            ),
            (Some(4096), fixed.then_some(4096), fixed.then_some(true))
        );
        assert!(
            payload.get("tools").is_none()
                && payload.get("stop").is_none()
                && payload.get("stop_sequences").is_none()
        );
    }
}

#[test]
fn generated_input_matches_estimate_for_every_preset() {
    for target in crate::throughput::INPUT_PRESETS {
        let messages = messages(target);
        let bytes: usize = messages
            .iter()
            .map(|message| message.content.as_str().unwrap().len())
            .sum();
        assert_eq!((bytes + 32).div_ceil(4), target as usize);
    }
}

#[test]
fn stream_metrics_use_provider_usage_and_actual_generation_interval() {
    let mut stats = StreamStats::default();
    stats
        .event(
            "data: {\"choices\":[{\"delta\":{\"content\":\"one\"}}]}",
            0.5,
        )
        .unwrap();
    stats.event("data: {\"choices\":[{\"delta\":{\"content\":\" two\"},\"finish_reason\":\"length\"}],\"usage\":{\"prompt_tokens\":2048,\"completion_tokens\":1024}}", 2.5).unwrap();
    assert!(stats.event("data: [DONE]", 3.0).unwrap());
    assert_eq!(
        stats.metrics(1024, 4.0, true),
        BenchmarkMetrics {
            input_tokens: Some(2048),
            output_tokens: Some(1024),
            reasoning_tokens: None,
            estimated_output_tokens: 2,
            ttft_ms: Some(500.0),
            decode_tps: Some(511.5),
            prefill_tps: Some(4096.0),
            end_to_end_tps: Some(256.0),
            finish_reason: Some("length".into()),
            target_reached: Some(true),
        }
    );
    assert_eq!(stats.metrics(2048, 4.0, true).target_reached, Some(false));
}

#[test]
fn missing_usage_is_unverified_and_provider_errors_are_redacted() {
    let mut stats = StreamStats::default();
    stats
        .event(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"one\"}",
            0.1,
        )
        .unwrap();
    stats
        .event("data: {\"type\":\"response.completed\"}", 0.2)
        .unwrap();
    assert_eq!(stats.metrics(1024, 0.2, true).target_reached, None);
    assert_eq!(
        stats.event("data: {\"error\":{\"message\":\"private\"}}", 0.3),
        Err("模型 API 在生成期间报告错误".into())
    );
}

#[test]
fn input_only_usage_does_not_invent_an_output_measurement() {
    let mut stats = StreamStats::default();
    stats
        .event("data: {\"usage\":{\"input_tokens\":1024}}", 0.1)
        .unwrap();
    let metrics = stats.metrics(1024, 1.0, true);
    assert_eq!(
        (
            metrics.input_tokens,
            metrics.output_tokens,
            metrics.target_reached
        ),
        (Some(1024), None, None)
    );
}

#[test]
fn anthropic_usage_deltas_preserve_input_and_count_cache() {
    let mut stats = StreamStats::default();
    stats.event("data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":100,\"cache_read_input_tokens\":900}}}", 0.1).unwrap();
    stats
        .event(
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"one\"}}",
            0.2,
        )
        .unwrap();
    stats.event("data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"},\"usage\":{\"output_tokens\":1024}}", 0.4).unwrap();
    let metrics = stats.metrics(1024, 0.5, true);
    assert_eq!(
        (
            metrics.input_tokens,
            metrics.output_tokens,
            metrics.target_reached
        ),
        (Some(1000), Some(1024), Some(true))
    );
}

#[tokio::test]
async fn benchmark_stream_handles_split_utf8_and_rejects_truncated_stream() {
    use axum::{body::Body, response::Response, routing::post, Router};
    let app = Router::new().route("/v1/chat/completions", post(|| async {
        let text = "data: {\"choices\":[{\"delta\":{\"content\":\"字\"}}]}\r\n\r\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}],\"usage\":{\"prompt_tokens\":1024,\"completion_tokens\":1024}}\n\ndata: [DONE]";
        let chunks: Vec<_> = text.as_bytes().chunks(1).map(|chunk| Ok::<_, std::convert::Infallible>(bytes::Bytes::copy_from_slice(chunk))).collect();
        Response::builder().header("content-type", "text/event-stream").body(Body::from_stream(futures::stream::iter(chunks))).unwrap()
    })).route("/broken/v1/chat/completions", post(|| async {
        Response::builder().header("content-type", "text/event-stream").body(Body::from("data: {\"choices\":[{\"delta\":{\"content\":\"one\"}}]}\n\n")).unwrap()
    }));
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let mut config = model("openai_compatible");
    config.base_url = Some(format!("http://{address}/v1"));
    let client = LlmClient::new(reqwest::Client::new(), config.clone());
    let result = client
        .benchmark(&messages(1024), 1024, |_| {})
        .await
        .unwrap();
    assert_eq!(
        (
            result.input_tokens,
            result.output_tokens,
            result.target_reached
        ),
        (Some(1024), Some(1024), Some(true))
    );
    config.base_url = Some(format!("http://{address}/broken"));
    let client = LlmClient::new(reqwest::Client::new(), config);
    assert_eq!(
        client.benchmark(&messages(1024), 1024, |_| {}).await,
        Err("模型响应流提前断开，未收到完成事件".into())
    );
    server.abort();
}
