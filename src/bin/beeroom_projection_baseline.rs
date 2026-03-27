use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast::error::RecvError;
use wunder_server::projection::beeroom::{
    BeeroomProjectionMetricsSnapshot, BeeroomProjectionService,
};
use wunder_server::storage::{SqliteStorage, StorageBackend};

#[derive(Debug, Clone, Serialize)]
struct Args {
    events: usize,
    publishers: usize,
    warmup: usize,
    user_id: String,
    group_id: String,
}

#[derive(Debug, Serialize)]
struct LatencySummary {
    samples: usize,
    p50_ms: u64,
    p95_ms: u64,
    p99_ms: u64,
    max_ms: u64,
}

#[derive(Debug, Serialize)]
struct BaselineSummary {
    events: usize,
    publishers: usize,
    warmup: usize,
    publish_duration_ms: u128,
    publish_rate_eps: f64,
    expected_watch_events: usize,
    observed_watch_events: usize,
    lagged_skipped_events: u64,
    watch_latency: LatencySummary,
    service_metrics: BeeroomProjectionMetricsSnapshot,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_args(std::env::args().collect())?;
    let summary = run_baseline(args.clone()).await?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({ "args": args, "summary": summary }))?
    );
    Ok(())
}

async fn run_baseline(args: Args) -> Result<BaselineSummary> {
    let db_path = std::env::temp_dir().join(format!(
        "wunder_beeroom_projection_baseline_{}.db",
        uuid::Uuid::new_v4().simple()
    ));
    let storage: Arc<dyn StorageBackend> =
        Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    let service = Arc::new(BeeroomProjectionService::new(storage));

    for seq in 0..args.warmup {
        service
            .publish_group_event(
                &args.user_id,
                &args.group_id,
                "benchmark_warmup",
                json!({ "seq": seq }),
            )
            .await;
    }

    let mut receiver = service
        .subscribe_group(&args.user_id, &args.group_id)
        .await
        .map_err(|err| anyhow!("subscribe failed: {err}"))?;
    let receive_done = Arc::new(AtomicBool::new(false));
    let receive_done_snapshot = receive_done.clone();
    let expected_events = args.events;

    let receiver_task = tokio::spawn(async move {
        let mut observed = 0usize;
        let mut lagged_skipped = 0u64;
        let mut latencies_ms = Vec::with_capacity(expected_events);
        loop {
            if observed >= expected_events {
                break;
            }
            match tokio::time::timeout(Duration::from_millis(500), receiver.recv()).await {
                Ok(Ok(event)) => {
                    observed = observed.saturating_add(1);
                    let now = now_ts();
                    let latency_ms = ((now - event.created_at).max(0.0) * 1000.0) as u64;
                    latencies_ms.push(latency_ms);
                }
                Ok(Err(RecvError::Lagged(skipped))) => {
                    lagged_skipped = lagged_skipped.saturating_add(skipped as u64);
                }
                Ok(Err(RecvError::Closed)) => break,
                Err(_) => {
                    if receive_done_snapshot.load(Ordering::Relaxed) {
                        break;
                    }
                }
            }
        }
        (observed, lagged_skipped, latencies_ms)
    });

    let publisher_total = args.publishers.max(1);
    let barrier = Arc::new(tokio::sync::Barrier::new(publisher_total.saturating_add(1)));
    let mut handles = Vec::new();
    let events_per_publisher = args.events / publisher_total;
    let events_remainder = args.events % publisher_total;
    for publisher_index in 0..publisher_total {
        let service_snapshot = service.clone();
        let barrier_snapshot = barrier.clone();
        let user_id = args.user_id.clone();
        let group_id = args.group_id.clone();
        let assign_count = events_per_publisher + usize::from(publisher_index < events_remainder);
        handles.push(tokio::spawn(async move {
            barrier_snapshot.wait().await;
            for seq in 0..assign_count {
                service_snapshot
                    .publish_group_event(
                        &user_id,
                        &group_id,
                        "benchmark_event",
                        json!({
                            "publisher": publisher_index,
                            "seq": seq,
                        }),
                    )
                    .await;
            }
        }));
    }

    let publish_start = Instant::now();
    barrier.wait().await;
    for handle in handles {
        handle
            .await
            .map_err(|err| anyhow!("publisher task failed: {err}"))?;
    }
    let publish_elapsed = publish_start.elapsed();
    receive_done.store(true, Ordering::Relaxed);
    let (observed_watch_events, lagged_skipped_events, mut latencies_ms) = receiver_task
        .await
        .map_err(|err| anyhow!("receiver task failed: {err}"))?;
    latencies_ms.sort_unstable();
    let watch_latency = LatencySummary {
        samples: latencies_ms.len(),
        p50_ms: percentile(&latencies_ms, 50),
        p95_ms: percentile(&latencies_ms, 95),
        p99_ms: percentile(&latencies_ms, 99),
        max_ms: latencies_ms.last().copied().unwrap_or(0),
    };
    let publish_elapsed_secs = publish_elapsed.as_secs_f64().max(0.001);
    let publish_rate_eps = args.events as f64 / publish_elapsed_secs;

    Ok(BaselineSummary {
        events: args.events,
        publishers: publisher_total,
        warmup: args.warmup,
        publish_duration_ms: publish_elapsed.as_millis(),
        publish_rate_eps,
        expected_watch_events: args.events,
        observed_watch_events,
        lagged_skipped_events,
        watch_latency,
        service_metrics: service.metrics_snapshot(),
    })
}

fn percentile(values: &[u64], p: usize) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let bounded = p.min(100);
    let index = ((values.len() - 1) as f64 * (bounded as f64 / 100.0)).round() as usize;
    values
        .get(index)
        .copied()
        .unwrap_or_else(|| values[values.len() - 1])
}

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

fn parse_args(raw_args: Vec<String>) -> Result<Args> {
    let mut args = Args {
        events: 5000,
        publishers: 8,
        warmup: 300,
        user_id: "baseline_user".to_string(),
        group_id: "baseline_group".to_string(),
    };
    let mut index = 1usize;
    while index < raw_args.len() {
        let key = raw_args[index].as_str();
        match key {
            "--events" => {
                index = index.saturating_add(1);
                args.events = parse_usize(raw_args.get(index), "--events")?;
            }
            "--publishers" => {
                index = index.saturating_add(1);
                args.publishers = parse_usize(raw_args.get(index), "--publishers")?;
            }
            "--warmup" => {
                index = index.saturating_add(1);
                args.warmup = parse_usize(raw_args.get(index), "--warmup")?;
            }
            "--user-id" => {
                index = index.saturating_add(1);
                args.user_id = parse_string(raw_args.get(index), "--user-id")?;
            }
            "--group-id" => {
                index = index.saturating_add(1);
                args.group_id = parse_string(raw_args.get(index), "--group-id")?;
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => {
                return Err(anyhow!(
                    "unknown flag: {other}. use --help to see supported options"
                ));
            }
        }
        index = index.saturating_add(1);
    }
    if args.events == 0 {
        return Err(anyhow!("--events must be greater than 0"));
    }
    if args.publishers == 0 {
        return Err(anyhow!("--publishers must be greater than 0"));
    }
    if args.user_id.trim().is_empty() || args.group_id.trim().is_empty() {
        return Err(anyhow!("--user-id and --group-id must not be empty"));
    }
    Ok(args)
}

fn parse_usize(raw: Option<&String>, flag: &str) -> Result<usize> {
    let value = raw.ok_or_else(|| anyhow!("{flag} requires a value"))?;
    value
        .parse::<usize>()
        .map_err(|err| anyhow!("invalid value for {flag}: {err}"))
}

fn parse_string(raw: Option<&String>, flag: &str) -> Result<String> {
    let value = raw.ok_or_else(|| anyhow!("{flag} requires a value"))?;
    let cleaned = value.trim();
    if cleaned.is_empty() {
        return Err(anyhow!("{flag} must not be empty"));
    }
    Ok(cleaned.to_string())
}

fn print_help() {
    println!(
        "beeroom_projection_baseline\n\
         --events <N>       total publish events (default: 5000)\n\
         --publishers <N>   concurrent publishers (default: 8)\n\
         --warmup <N>       warmup events before measure (default: 300)\n\
         --user-id <text>   benchmark user id (default: baseline_user)\n\
         --group-id <text>  benchmark group id (default: baseline_group)\n\
         --help             print help"
    );
}
