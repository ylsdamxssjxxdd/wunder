//! Integration checks for the shared simulated prefill timing.
use std::time::{Duration, Instant};
use wunder_server::virtual_llm::timing;

#[test]
fn simulated_prefill_budget_uses_two_thousand_tokens_per_second() {
    assert_eq!(
        [0, 1024, 8192, 1048576]
            .map(|tokens| timing::VirtualModelSpeed::Fast.prefill_duration(tokens)),
        [
            Duration::ZERO,
            Duration::from_millis(512),
            Duration::from_millis(4096),
            Duration::from_millis(524288)
        ]
    );
}

#[tokio::test]
async fn simulated_prefill_waits_for_its_budget_and_is_cancellable() {
    let started = Instant::now();
    timing::wait_for_prefill(200, timing::VirtualModelSpeed::Fast).await;
    assert!(started.elapsed() >= Duration::from_millis(100));
    assert!(tokio::time::timeout(
        Duration::from_millis(20),
        timing::wait_for_prefill(1_048_576, timing::VirtualModelSpeed::Slow)
    )
    .await
    .is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn fast_profile_concurrency_preserves_per_request_rates_and_reasoning() {
    use std::sync::Arc;
    use tokio::{sync::Barrier, task::JoinSet};
    use wunder_server::virtual_llm::{
        emit_virtual_deltas, estimate_virtual_usage, VirtualReplayTurn,
    };
    for concurrency in [1, 8, 32] {
        let barrier = Arc::new(Barrier::new(concurrency));
        let mut tasks = JoinSet::new();
        let batch_started = Instant::now();
        for _ in 0..concurrency {
            let barrier = Arc::clone(&barrier);
            tasks.spawn(async move {
                let turn = VirtualReplayTurn {
                    finish_reason: None,
                    content: " one".repeat(768),
                    reasoning: " one".repeat(256),
                    usage: None,
                    tool_calls: None,
                    source_log_id: String::new(),
                    source_log_name: String::new(),
                    source_round: 1,
                    source_model_round: Some(1),
                    format: "synthetic".into(),
                };
                barrier.wait().await;
                let started = Instant::now();
                timing::wait_for_prefill(1024, timing::VirtualModelSpeed::Fast).await;
                let mut first = None;
                let mut last = None;
                let mut answer_bytes = 0;
                let mut reasoning_bytes = 0;
                emit_virtual_deltas(
                    &turn,
                    true,
                    timing::VirtualModelSpeed::Fast,
                    |answer, reasoning| {
                        assert!(
                            answer_bytes == 0 || reasoning.is_empty(),
                            "reasoning must precede answer"
                        );
                        first.get_or_insert_with(|| started.elapsed().as_secs_f64());
                        last = Some(started.elapsed().as_secs_f64());
                        answer_bytes += answer.len();
                        reasoning_bytes += reasoning.len();
                        std::future::ready(Ok(()))
                    },
                )
                .await
                .unwrap();
                assert_eq!((answer_bytes, reasoning_bytes), (768 * 4, 256 * 4));
                let usage = estimate_virtual_usage(&[], &turn);
                assert_eq!(
                    (usage.output, usage.reasoning, usage.total),
                    (768, Some(256), 1024)
                );
                let first = first.unwrap();
                let decode = 1023.0 / (last.unwrap() - first);
                assert!((0.512..0.85).contains(&first));
                assert!((190.0..210.0).contains(&decode));
                (first * 1000.0, decode)
            });
        }
        let mut samples = Vec::new();
        while let Some(result) = tasks.join_next().await {
            samples.push(result.unwrap());
        }
        assert_eq!(samples.len(), concurrency);
        let mean_ttft = samples.iter().map(|sample| sample.0).sum::<f64>() / concurrency as f64;
        let mean_decode = samples.iter().map(|sample| sample.1).sum::<f64>() / concurrency as f64;
        let aggregate = concurrency as f64 * 1024.0 / batch_started.elapsed().as_secs_f64();
        println!("fast concurrency={concurrency}, mean_ttft_ms={mean_ttft:.2}, mean_decode_tps={mean_decode:.2}, aggregate_e2e_tps={aggregate:.2}");
    }
}
