use super::*;

fn scenario() -> ThroughputConfig {
    ThroughputConfig {
        model_name: "model".into(),
        concurrency: 1,
        input_tokens: 1024,
        output_tokens: 1024,
    }
}

#[test]
fn rejects_invalid_presets_disabled_models_and_context_overflow() {
    let mut config = Config::default();
    let model = LlmModelConfig {
        provider: Some("openai".into()),
        model: Some("model".into()),
        max_context: Some(2048),
        ..Default::default()
    };
    config.llm.models.insert("model".into(), model);
    assert!(scenario().resolve(&config).is_ok());
    let mut invalid = scenario();
    invalid.input_tokens = 2048;
    assert!(invalid.resolve(&config).is_err());
    invalid = scenario();
    invalid.concurrency = 0;
    assert!(invalid.resolve(&config).is_err());
    invalid.concurrency = 2;
    invalid.input_tokens = 1234;
    invalid.output_tokens = 345;
    assert!(invalid.resolve(&config).is_ok());
    invalid.input_tokens = 0;
    assert!(invalid.resolve(&config).is_err());
    config.llm.models.get_mut("model").unwrap().enable = Some(false);
    assert!(scenario().resolve(&config).is_err());
    assert!(serde_json::from_value::<ThroughputConfig>(serde_json::json!({"concurrency_list":[1],"model_name":"model","input_tokens":1024,"output_tokens":1024})).is_err());
}

#[test]
fn aggregates_concurrent_requests_against_batch_target() {
    let one = ThroughputMetrics {
        input_tokens: Some(100),
        output_tokens: Some(20),
        reasoning_tokens: Some(5),
        estimated_output_tokens: 20,
        ttft_ms: Some(100.0),
        max_ttft_ms: Some(100.0),
        decode_tps: Some(100.0),
        avg_decode_tps: Some(100.0),
        prefill_tps: Some(1000.0),
        avg_prefill_tps: Some(1000.0),
        finish_reason: Some("stop".into()),
        target_reached: Some(true),
        ..Default::default()
    };
    let mut two = one.clone();
    two.ttft_ms = Some(300.0);
    two.max_ttft_ms = Some(300.0);
    two.decode_tps = Some(200.0);
    two.avg_decode_tps = Some(200.0);
    two.prefill_tps = Some(2000.0);
    two.avg_prefill_tps = Some(2000.0);
    let batch = super::aggregate_metrics(&[Some(one), Some(two)]);
    assert_eq!(batch.input_tokens, Some(200));
    assert_eq!(batch.output_tokens, Some(40));
    assert_eq!(batch.reasoning_tokens, Some(10));
    assert_eq!(batch.target_reached, Some(true));
    assert_eq!(batch.ttft_ms, Some(200.0));
    assert_eq!(batch.max_ttft_ms, Some(300.0));
    assert_eq!(batch.avg_decode_tps, Some(150.0));
    assert_eq!(batch.avg_prefill_tps, Some(1500.0));
    assert!(batch
        .decode_tps
        .is_some_and(|value| value > 128.0 && value < 130.0));
    assert!(batch
        .prefill_tps
        .is_some_and(|value| value > 666.0 && value < 667.0));
}

#[tokio::test]
async fn cancellation_conflict_tickets_and_summary_persistence() {
    use axum::{routing::post, Router};
    let app = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            std::future::pending::<()>().await;
            ""
        }),
    );
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("summary.json");
    let manager = ThroughputManager::with_history_path(path.clone());
    let ticket = manager.issue_ticket();
    assert!(manager.consume_ticket(&ticket));
    assert!(!manager.consume_ticket(&ticket));
    let model = LlmModelConfig {
        provider: Some("openai_compatible".into()),
        model: Some("model".into()),
        base_url: Some(format!("http://{address}/v1")),
        ..Default::default()
    };
    let run = manager.start(scenario(), model.clone()).await.unwrap();
    assert!(manager.start(scenario(), model).await.is_err());
    manager.stop().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if manager.inner.lock().cancel.is_none() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let saved = history::load(&path);
    assert_eq!(
        (saved.len(), saved[0].id.as_str(), saved[0].status.as_str()),
        (1, run.id.as_str(), "stopped")
    );
    assert!(manager.report(Some("../invalid")).await.is_err());
    assert!(saved[0].error.is_none());
    assert!(!std::fs::read_to_string(path).unwrap().contains("messages"));
    server.abort();
}

#[tokio::test]
async fn history_is_bounded_and_replaces_previous_summary() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("summary.json");
    let record = ThroughputSnapshot {
        id: "record".into(),
        status: "finished".into(),
        config: scenario(),
        started_at: String::new(),
        finished_at: None,
        elapsed_s: 1.0,
        length_control: "fixed".into(),
        simulated: false,
        simulation_speed: None,
        metrics: ThroughputMetrics::default(),
        error: None,
        persistence_error: false,
    };
    history::save(&path, &vec![record.clone(); 60])
        .await
        .unwrap();
    assert_eq!(history::load(&path).len(), HISTORY_LIMIT);
    history::save(&path, &[record]).await.unwrap();
    assert_eq!(history::load(&path).len(), 1);
}
