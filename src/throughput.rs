use crate::monitor::MonitorState;
use crate::orchestrator::Orchestrator;
use crate::schemas::{TokenUsage, WunderRequest};
use chrono::{DateTime, Utc};
use futures::future::join_all;
use parking_lot::Mutex as ParkingMutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs as tokio_fs;
use tokio::sync::Mutex;
use uuid::Uuid;

const DEFAULT_USER_PREFIX: &str = "throughput_user";
const MAX_CONCURRENCY: usize = 500;
const MAX_ERROR_SAMPLES: usize = 20;
const MAX_REPORT_HISTORY: usize = 50;
const REPORT_DIR: &str = "data/throughput";
const REPORT_INDEX_FILE: &str = "index.json";
const MIN_PREFILL_DURATION_S: f64 = 0.05;
const LATENCY_BUCKETS_MS: [u64; 12] = [
    50, 100, 200, 300, 500, 800, 1000, 1500, 2000, 3000, 5000, 10000,
];

#[derive(Clone)]
pub struct ThroughputManager {
    inner: Arc<ThroughputManagerInner>,
}

struct ThroughputManagerInner {
    state: Mutex<ThroughputState>,
}

struct ThroughputState {
    active: Option<ActiveRun>,
    history: Vec<ThroughputSnapshot>,
}

struct ActiveRun {
    id: String,
    config: ThroughputConfig,
    started_at: DateTime<Utc>,
    started_instant: Instant,
    finished_at: Option<DateTime<Utc>>,
    finished_instant: Option<Instant>,
    status: RunStatus,
    metrics: Arc<ThroughputMetrics>,
    stop_flag: Arc<AtomicBool>,
}

#[derive(Clone, Copy)]
enum RunStatus {
    Running,
    Stopping,
    Finished,
    Stopped,
}

impl RunStatus {
    fn as_str(&self) -> &'static str {
        match self {
            RunStatus::Running => "running",
            RunStatus::Stopping => "stopping",
            RunStatus::Finished => "finished",
            RunStatus::Stopped => "stopped",
        }
    }
}

#[derive(Clone)]
pub struct ThroughputConfig {
    pub max_concurrency: usize,
    pub step: usize,
    pub questions: Vec<String>,
    pub user_id_prefix: String,
    pub model_name: Option<String>,
    pub request_timeout_s: f64,
}

impl ThroughputConfig {
    pub fn new(
        max_concurrency: usize,
        step: usize,
        question: Option<String>,
        questions: Option<Vec<String>>,
        user_id_prefix: Option<String>,
        model_name: Option<String>,
        request_timeout_s: Option<f64>,
    ) -> Result<Self, String> {
        let mut questions = questions.unwrap_or_default();
        if questions.is_empty() {
            if let Some(question) = question {
                questions.push(question);
            }
        }
        let questions = questions
            .into_iter()
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>();
        if questions.is_empty() {
            return Err("问题不能为空".to_string());
        }
        if max_concurrency == 0 {
            return Err("最大并发必须大于 0".to_string());
        }
        if max_concurrency > MAX_CONCURRENCY {
            return Err(format!("最大并发不能超过 {MAX_CONCURRENCY}"));
        }
        let prefix = user_id_prefix
            .unwrap_or_else(|| DEFAULT_USER_PREFIX.to_string())
            .trim()
            .to_string();
        let prefix = if prefix.is_empty() {
            DEFAULT_USER_PREFIX.to_string()
        } else {
            prefix
        };
        let timeout = request_timeout_s.unwrap_or(0.0);
        let timeout = if timeout.is_finite() && timeout > 0.0 {
            timeout
        } else {
            0.0
        };
        let model_name = model_name
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty());
        Ok(Self {
            max_concurrency,
            step,
            questions,
            user_id_prefix: prefix,
            model_name,
            request_timeout_s: timeout,
        })
    }
}

struct ThroughputMetrics {
    total: AtomicU64,
    success: AtomicU64,
    error: AtomicU64,
    total_latency_ms: AtomicU64,
    first_token_latency_total_ms: AtomicU64,
    first_token_latency_count: AtomicU64,
    min_latency_ms: AtomicU64,
    max_latency_ms: AtomicU64,
    input_tokens: AtomicU64,
    output_tokens: AtomicU64,
    total_tokens: AtomicU64,
    buckets: Vec<AtomicU64>,
    errors: ParkingMutex<Vec<ThroughputErrorSnapshot>>,
    samples: ParkingMutex<Vec<ThroughputSample>>,
}

impl ThroughputMetrics {
    fn new() -> Self {
        let mut buckets = Vec::with_capacity(LATENCY_BUCKETS_MS.len() + 1);
        for _ in 0..=LATENCY_BUCKETS_MS.len() {
            buckets.push(AtomicU64::new(0));
        }
        Self {
            total: AtomicU64::new(0),
            success: AtomicU64::new(0),
            error: AtomicU64::new(0),
            total_latency_ms: AtomicU64::new(0),
            first_token_latency_total_ms: AtomicU64::new(0),
            first_token_latency_count: AtomicU64::new(0),
            min_latency_ms: AtomicU64::new(u64::MAX),
            max_latency_ms: AtomicU64::new(0),
            input_tokens: AtomicU64::new(0),
            output_tokens: AtomicU64::new(0),
            total_tokens: AtomicU64::new(0),
            buckets,
            errors: ParkingMutex::new(Vec::new()),
            samples: ParkingMutex::new(Vec::new()),
        }
    }

    fn record(
        &self,
        latency_ms: u64,
        usage: Option<crate::schemas::TokenUsage>,
        error_message: Option<String>,
        first_token_latency_ms: Option<u64>,
    ) {
        self.total.fetch_add(1, Ordering::Relaxed);
        self.total_latency_ms
            .fetch_add(latency_ms, Ordering::Relaxed);
        if let Some(first_token_latency_ms) = first_token_latency_ms {
            if first_token_latency_ms > 0 {
                self.first_token_latency_total_ms
                    .fetch_add(first_token_latency_ms, Ordering::Relaxed);
                self.first_token_latency_count
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        if error_message.is_some() {
            self.error.fetch_add(1, Ordering::Relaxed);
        } else {
            self.success.fetch_add(1, Ordering::Relaxed);
        }
        self.update_min_latency(latency_ms);
        self.update_max_latency(latency_ms);
        self.update_latency_bucket(latency_ms);
        if let Some(usage) = usage {
            self.input_tokens.fetch_add(usage.input, Ordering::Relaxed);
            self.output_tokens
                .fetch_add(usage.output, Ordering::Relaxed);
            self.total_tokens.fetch_add(usage.total, Ordering::Relaxed);
        }
        if let Some(message) = error_message {
            self.push_error(message);
        }
    }

    fn update_min_latency(&self, latency_ms: u64) {
        let mut current = self.min_latency_ms.load(Ordering::Relaxed);
        while latency_ms < current {
            match self.min_latency_ms.compare_exchange(
                current,
                latency_ms,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(updated) => current = updated,
            }
        }
    }

    fn update_max_latency(&self, latency_ms: u64) {
        let mut current = self.max_latency_ms.load(Ordering::Relaxed);
        while latency_ms > current {
            match self.max_latency_ms.compare_exchange(
                current,
                latency_ms,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(updated) => current = updated,
            }
        }
    }

    fn update_latency_bucket(&self, latency_ms: u64) {
        let mut index = LATENCY_BUCKETS_MS.len();
        for (idx, bound) in LATENCY_BUCKETS_MS.iter().enumerate() {
            if latency_ms <= *bound {
                index = idx;
                break;
            }
        }
        if let Some(bucket) = self.buckets.get(index) {
            bucket.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn push_error(&self, message: String) {
        let mut guard = self.errors.lock();
        guard.push(ThroughputErrorSnapshot {
            timestamp: Utc::now().to_rfc3339(),
            message,
        });
        if guard.len() > MAX_ERROR_SAMPLES {
            let overflow = guard.len() - MAX_ERROR_SAMPLES;
            guard.drain(0..overflow);
        }
    }

    fn push_sample(&self, sample: ThroughputSample) {
        self.samples.lock().push(sample);
    }

    fn samples(&self) -> Vec<ThroughputSample> {
        self.samples.lock().clone()
    }

    fn snapshot(&self, elapsed_s: f64) -> ThroughputMetricsSnapshot {
        let total = self.total.load(Ordering::Relaxed);
        let success = self.success.load(Ordering::Relaxed);
        let error = self.error.load(Ordering::Relaxed);
        let total_latency_ms = self.total_latency_ms.load(Ordering::Relaxed);
        let first_token_latency_total_ms =
            self.first_token_latency_total_ms.load(Ordering::Relaxed);
        let first_token_latency_count = self.first_token_latency_count.load(Ordering::Relaxed);
        let min_latency_raw = self.min_latency_ms.load(Ordering::Relaxed);
        let max_latency_ms = self.max_latency_ms.load(Ordering::Relaxed);
        let min_latency_ms = if min_latency_raw == u64::MAX {
            None
        } else {
            Some(min_latency_raw)
        };
        let avg_latency_ms = if total > 0 {
            Some((total_latency_ms as f64 / total as f64).round() as u64)
        } else {
            None
        };
        let first_token_latency_ms = if first_token_latency_count > 0 {
            Some(
                (first_token_latency_total_ms as f64 / first_token_latency_count as f64).round()
                    as u64,
            )
        } else {
            None
        };
        let rps = if elapsed_s > 0.0 {
            ((total as f64 / elapsed_s) * 100.0).round() / 100.0
        } else {
            0.0
        };
        let bucket_counts = self
            .buckets
            .iter()
            .map(|bucket| bucket.load(Ordering::Relaxed))
            .collect::<Vec<_>>();
        let p50 = estimate_percentile(&bucket_counts, 0.5);
        let p90 = estimate_percentile(&bucket_counts, 0.9);
        let p99 = estimate_percentile(&bucket_counts, 0.99);
        let input_tokens = self.input_tokens.load(Ordering::Relaxed);
        let output_tokens = self.output_tokens.load(Ordering::Relaxed);
        let total_tokens = self.total_tokens.load(Ordering::Relaxed);
        let avg_total_tokens = if success > 0 {
            Some((total_tokens as f64 / success as f64).round() as u64)
        } else {
            None
        };
        let latency_buckets = build_bucket_snapshots(&bucket_counts);
        ThroughputMetricsSnapshot {
            total_requests: total,
            success_requests: success,
            error_requests: error,
            rps,
            avg_latency_ms,
            first_token_latency_ms,
            min_latency_ms,
            max_latency_ms: if total > 0 {
                Some(max_latency_ms)
            } else {
                None
            },
            p50_latency_ms: p50,
            p90_latency_ms: p90,
            p99_latency_ms: p99,
            input_tokens,
            output_tokens,
            total_tokens,
            avg_total_tokens,
            latency_buckets,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ThroughputSnapshot {
    pub run: ThroughputRunSnapshot,
    pub metrics: ThroughputMetricsSnapshot,
    pub errors: Vec<ThroughputErrorSnapshot>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ThroughputRunSnapshot {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub max_concurrency: usize,
    #[serde(default)]
    pub step: usize,
    pub question: Option<String>,
    #[serde(default)]
    pub questions: Vec<String>,
    pub user_id_prefix: String,
    pub stream: bool,
    pub model_name: Option<String>,
    pub request_timeout_s: f64,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub elapsed_s: f64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ThroughputMetricsSnapshot {
    pub total_requests: u64,
    pub success_requests: u64,
    pub error_requests: u64,
    pub rps: f64,
    pub avg_latency_ms: Option<u64>,
    #[serde(default)]
    pub first_token_latency_ms: Option<u64>,
    pub min_latency_ms: Option<u64>,
    pub max_latency_ms: Option<u64>,
    pub p50_latency_ms: Option<u64>,
    pub p90_latency_ms: Option<u64>,
    pub p99_latency_ms: Option<u64>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub avg_total_tokens: Option<u64>,
    pub latency_buckets: Vec<LatencyBucketSnapshot>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct LatencyBucketSnapshot {
    pub le_ms: Option<u64>,
    pub count: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ThroughputErrorSnapshot {
    pub timestamp: String,
    pub message: String,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ThroughputSample {
    pub timestamp: String,
    pub concurrency: usize,
    pub elapsed_s: f64,
    pub total_requests: u64,
    pub success_requests: u64,
    pub error_requests: u64,
    pub rps: f64,
    pub avg_latency_ms: Option<u64>,
    pub p50_latency_ms: Option<u64>,
    pub p90_latency_ms: Option<u64>,
    pub p99_latency_ms: Option<u64>,
    pub total_prefill_speed_tps: Option<f64>,
    pub single_prefill_speed_tps: Option<f64>,
    pub total_decode_speed_tps: Option<f64>,
    pub single_decode_speed_tps: Option<f64>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub avg_total_tokens: Option<u64>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ThroughputReport {
    pub summary: ThroughputSnapshot,
    #[serde(default)]
    pub samples: Vec<ThroughputSample>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ThroughputStatusResponse {
    pub active: Option<ThroughputSnapshot>,
    pub history: Vec<ThroughputSnapshot>,
}

impl ThroughputManager {
    pub fn new() -> Self {
        let history = load_report_index();
        Self {
            inner: Arc::new(ThroughputManagerInner {
                state: Mutex::new(ThroughputState {
                    active: None,
                    history,
                }),
            }),
        }
    }

    pub async fn start(
        &self,
        orchestrator: Arc<Orchestrator>,
        monitor: Arc<MonitorState>,
        config: ThroughputConfig,
    ) -> Result<ThroughputSnapshot, String> {
        let mut state = self.inner.state.lock().await;
        if let Some(active) = state.active.as_ref() {
            if matches!(active.status, RunStatus::Running | RunStatus::Stopping) {
                return Err("已有运行中的压测任务，请先停止或等待完成".to_string());
            }
        }
        if let Some(active) = state.active.take() {
            state.history.push(active.snapshot());
            if state.history.len() > MAX_REPORT_HISTORY {
                let overflow = state.history.len() - MAX_REPORT_HISTORY;
                state.history.drain(0..overflow);
            }
        }

        let run_id = Uuid::new_v4().simple().to_string();
        let metrics = Arc::new(ThroughputMetrics::new());
        let stop_flag = Arc::new(AtomicBool::new(false));
        let started_at = Utc::now();
        let started_instant = Instant::now();
        let active = ActiveRun {
            id: run_id.clone(),
            config: config.clone(),
            started_at,
            started_instant,
            finished_at: None,
            finished_instant: None,
            status: RunStatus::Running,
            metrics: Arc::clone(&metrics),
            stop_flag: Arc::clone(&stop_flag),
        };
        state.active = Some(active);
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            run_supervisor(
                inner,
                orchestrator,
                monitor,
                run_id,
                config,
                metrics,
                stop_flag,
            )
            .await;
        });
        Ok(state.active.as_ref().map(ActiveRun::snapshot).unwrap())
    }

    pub async fn stop(&self) -> Result<ThroughputSnapshot, String> {
        let mut state = self.inner.state.lock().await;
        let Some(active) = state.active.as_mut() else {
            return Err("当前没有运行中的压测任务".to_string());
        };
        if matches!(active.status, RunStatus::Finished | RunStatus::Stopped) {
            return Err("当前压测任务已结束".to_string());
        }
        active.status = RunStatus::Stopping;
        active.stop_flag.store(true, Ordering::Relaxed);
        Ok(active.snapshot())
    }

    pub async fn status(&self) -> ThroughputStatusResponse {
        let state = self.inner.state.lock().await;
        ThroughputStatusResponse {
            active: state.active.as_ref().map(ActiveRun::snapshot),
            history: state.history.clone(),
        }
    }

    pub async fn report(&self, run_id: Option<&str>) -> Result<ThroughputReport, String> {
        let target_id = {
            let state = self.inner.state.lock().await;
            if let Some(run_id) = run_id {
                if let Some(active) = state.active.as_ref() {
                    if active.id == run_id {
                        return Ok(active.report());
                    }
                }
                Some(run_id.to_string())
            } else if let Some(active) = state.active.as_ref() {
                return Ok(active.report());
            } else if let Some(last) = state.history.last() {
                Some(last.run.id.clone())
            } else {
                None
            }
        };
        let Some(target_id) = target_id else {
            return Err("暂无可导出的压测结果".to_string());
        };
        load_report(&target_id).await
    }
}

impl ActiveRun {
    fn snapshot(&self) -> ThroughputSnapshot {
        let elapsed_s = if let Some(finished_instant) = self.finished_instant {
            finished_instant
                .duration_since(self.started_instant)
                .as_secs_f64()
        } else {
            self.started_instant.elapsed().as_secs_f64()
        };
        let metrics = self.metrics.snapshot(elapsed_s);
        let errors = self.metrics.errors.lock().clone();
        ThroughputSnapshot {
            run: ThroughputRunSnapshot {
                id: self.id.clone(),
                status: self.status.as_str().to_string(),
                max_concurrency: self.config.max_concurrency,
                step: self.config.step,
                question: self.config.questions.first().cloned(),
                questions: self.config.questions.clone(),
                user_id_prefix: self.config.user_id_prefix.clone(),
                stream: true,
                model_name: self.config.model_name.clone(),
                request_timeout_s: self.config.request_timeout_s,
                started_at: self.started_at.to_rfc3339(),
                finished_at: self.finished_at.map(|value| value.to_rfc3339()),
                elapsed_s,
            },
            metrics,
            errors,
        }
    }

    fn report(&self) -> ThroughputReport {
        let summary = self.snapshot();
        let samples = self.metrics.samples();
        ThroughputReport { summary, samples }
    }
}

fn build_sequence(max_concurrency: usize, step: usize) -> Vec<usize> {
    if max_concurrency == 0 {
        return Vec::new();
    }
    if step == 0 {
        return vec![max_concurrency];
    }
    let mut sequence = Vec::new();
    let mut current = 1usize;
    while current < max_concurrency {
        sequence.push(current);
        current = current.saturating_add(step);
    }
    if sequence.last().copied() != Some(max_concurrency) {
        sequence.push(max_concurrency);
    }
    sequence
}

#[derive(Default)]
struct SessionSpeed {
    prefill_tokens: Option<i64>,
    prefill_duration_s: Option<f64>,
    prefill_speed_tps: Option<f64>,
    decode_tokens: Option<i64>,
    decode_duration_s: Option<f64>,
    decode_speed_tps: Option<f64>,
}

#[derive(Default)]
struct SpeedAccumulator {
    prefill_speed_sum: f64,
    prefill_speed_count: u64,
    decode_speed_sum: f64,
    decode_speed_count: u64,
    prefill_tokens_total: u64,
    decode_tokens_total: u64,
}

fn resolve_speed(tokens: Option<i64>, duration: Option<f64>, speed: Option<f64>) -> Option<f64> {
    if let (Some(tokens), Some(duration)) = (tokens, duration) {
        if tokens > 0 && duration > 0.0 {
            return Some(tokens as f64 / duration);
        }
    }
    if let Some(value) = speed {
        if value > 0.0 {
            return Some(value);
        }
    }
    None
}

impl SpeedAccumulator {
    fn record(&mut self, speed: &SessionSpeed) {
        if let Some(tokens) = speed.prefill_tokens {
            if tokens > 0 {
                self.prefill_tokens_total = self.prefill_tokens_total.saturating_add(tokens as u64);
            }
        }
        if let Some(value) = resolve_speed(
            speed.prefill_tokens,
            speed.prefill_duration_s,
            speed.prefill_speed_tps,
        ) {
            self.prefill_speed_sum += value;
            self.prefill_speed_count += 1;
        }
        if let Some(tokens) = speed.decode_tokens {
            if tokens > 0 {
                self.decode_tokens_total = self.decode_tokens_total.saturating_add(tokens as u64);
            }
        }
        if let Some(value) = resolve_speed(
            speed.decode_tokens,
            speed.decode_duration_s,
            speed.decode_speed_tps,
        ) {
            self.decode_speed_sum += value;
            self.decode_speed_count += 1;
        }
    }

    fn single_prefill_speed(&self) -> Option<f64> {
        if self.prefill_speed_count > 0 {
            return Some(self.prefill_speed_sum / self.prefill_speed_count as f64);
        }
        None
    }

    fn single_decode_speed(&self) -> Option<f64> {
        if self.decode_speed_count > 0 {
            return Some(self.decode_speed_sum / self.decode_speed_count as f64);
        }
        None
    }

    fn total_prefill_speed(&self, elapsed_s: f64, fallback_tokens: u64) -> Option<f64> {
        if self.prefill_speed_count > 0 {
            return Some(self.prefill_speed_sum);
        }
        if elapsed_s <= 0.0 {
            return None;
        }
        let tokens = if self.prefill_tokens_total > 0 {
            self.prefill_tokens_total
        } else {
            fallback_tokens
        };
        if tokens > 0 {
            Some(tokens as f64 / elapsed_s)
        } else {
            None
        }
    }

    fn total_decode_speed(&self, elapsed_s: f64, fallback_tokens: u64) -> Option<f64> {
        if self.decode_speed_count > 0 {
            return Some(self.decode_speed_sum);
        }
        if elapsed_s <= 0.0 {
            return None;
        }
        let tokens = if self.decode_tokens_total > 0 {
            self.decode_tokens_total
        } else {
            fallback_tokens
        };
        if tokens > 0 {
            Some(tokens as f64 / elapsed_s)
        } else {
            None
        }
    }
}

struct RequestOutcome {
    latency_ms: u64,
    usage: Option<TokenUsage>,
    error_message: Option<String>,
    speed: SessionSpeed,
}

async fn run_supervisor(
    inner: Arc<ThroughputManagerInner>,
    orchestrator: Arc<Orchestrator>,
    monitor: Arc<MonitorState>,
    run_id: String,
    config: ThroughputConfig,
    metrics: Arc<ThroughputMetrics>,
    stop_flag: Arc<AtomicBool>,
) {
    let sequence = build_sequence(config.max_concurrency, config.step);
    let questions = Arc::new(config.questions.clone());
    let user_prefix = config.user_id_prefix.clone();
    for concurrency in sequence {
        if stop_flag.load(Ordering::Relaxed) {
            break;
        }
        let step_started = Instant::now();
        let step_metrics = Arc::new(ThroughputMetrics::new());
        let request_timeout_s = config.request_timeout_s;
        let model_name = config.model_name.clone();
        let run_id_ref = run_id.as_str();
        let user_prefix_ref = user_prefix.as_str();
        let tasks = (0..concurrency)
            .map(|index| {
                run_request(
                    Arc::clone(&orchestrator),
                    Arc::clone(&monitor),
                    run_id_ref,
                    user_prefix_ref,
                    concurrency,
                    index,
                    Arc::clone(&questions),
                    model_name.clone(),
                    request_timeout_s,
                )
            })
            .collect::<Vec<_>>();
        let results = join_all(tasks).await;
        let mut speed_acc = SpeedAccumulator::default();
        for outcome in results {
            let usage = outcome.usage.clone();
            let first_token_latency_ms = outcome.speed.prefill_duration_s.and_then(|duration| {
                if duration > 0.0 {
                    Some((duration * 1000.0).round() as u64)
                } else {
                    None
                }
            });
            metrics.record(
                outcome.latency_ms,
                usage.clone(),
                outcome.error_message.clone(),
                first_token_latency_ms,
            );
            step_metrics.record(
                outcome.latency_ms,
                usage,
                outcome.error_message,
                first_token_latency_ms,
            );
            speed_acc.record(&outcome.speed);
        }
        let elapsed_s = step_started.elapsed().as_secs_f64();
        let snapshot = step_metrics.snapshot(elapsed_s);
        let concurrency_f = concurrency as f64;
        let mut total_prefill_speed =
            speed_acc.total_prefill_speed(elapsed_s, snapshot.input_tokens);
        let mut total_decode_speed =
            speed_acc.total_decode_speed(elapsed_s, snapshot.output_tokens);
        let mut single_prefill_speed = speed_acc.single_prefill_speed();
        let mut single_decode_speed = speed_acc.single_decode_speed();
        if single_prefill_speed.is_none() {
            if let Some(total) = total_prefill_speed {
                if concurrency_f > 0.0 {
                    single_prefill_speed = Some(total / concurrency_f);
                }
            }
        }
        if single_decode_speed.is_none() {
            if let Some(total) = total_decode_speed {
                if concurrency_f > 0.0 {
                    single_decode_speed = Some(total / concurrency_f);
                }
            }
        }
        if total_prefill_speed.is_none() {
            if let Some(single) = single_prefill_speed {
                if concurrency_f > 0.0 {
                    total_prefill_speed = Some(single * concurrency_f);
                }
            }
        }
        if total_decode_speed.is_none() {
            if let Some(single) = single_decode_speed {
                if concurrency_f > 0.0 {
                    total_decode_speed = Some(single * concurrency_f);
                }
            }
        }
        let sample = ThroughputSample {
            timestamp: Utc::now().to_rfc3339(),
            concurrency,
            elapsed_s,
            total_requests: snapshot.total_requests,
            success_requests: snapshot.success_requests,
            error_requests: snapshot.error_requests,
            rps: snapshot.rps,
            avg_latency_ms: snapshot.avg_latency_ms,
            p50_latency_ms: snapshot.p50_latency_ms,
            p90_latency_ms: snapshot.p90_latency_ms,
            p99_latency_ms: snapshot.p99_latency_ms,
            total_prefill_speed_tps: total_prefill_speed,
            single_prefill_speed_tps: single_prefill_speed,
            total_decode_speed_tps: total_decode_speed,
            single_decode_speed_tps: single_decode_speed,
            input_tokens: snapshot.input_tokens,
            output_tokens: snapshot.output_tokens,
            total_tokens: snapshot.total_tokens,
            avg_total_tokens: snapshot.avg_total_tokens,
        };
        metrics.push_sample(sample);
    }

    let mut report_to_persist = None;
    let mut history_to_persist = None;
    {
        let mut state = inner.state.lock().await;
        if let Some(active) = state.active.as_mut() {
            if active.id == run_id {
                active.finished_at = Some(Utc::now());
                active.finished_instant = Some(Instant::now());
                active.status = if active.stop_flag.load(Ordering::Relaxed) {
                    RunStatus::Stopped
                } else {
                    RunStatus::Finished
                };
                let report = active.report();
                state.history.push(report.summary.clone());
                if state.history.len() > MAX_REPORT_HISTORY {
                    let overflow = state.history.len() - MAX_REPORT_HISTORY;
                    state.history.drain(0..overflow);
                }
                report_to_persist = Some(report);
                history_to_persist = Some(state.history.clone());
                state.active = None;
            }
        }
    }
    if let Some(report) = report_to_persist {
        let _ = persist_report(&report).await;
    }
    if let Some(history) = history_to_persist {
        let _ = persist_report_index(&history).await;
    }
}

async fn run_request(
    orchestrator: Arc<Orchestrator>,
    monitor: Arc<MonitorState>,
    run_id: &str,
    user_prefix: &str,
    concurrency: usize,
    index: usize,
    questions: Arc<Vec<String>>,
    model_name: Option<String>,
    request_timeout_s: f64,
) -> RequestOutcome {
    let user_index = index + 1;
    let user_id = format!("{user_prefix}-{concurrency}-{user_index}");
    let session_id = format!("throughput_{run_id}_{concurrency}_{user_index}");
    let mut seed = seed_for_user(&user_id);
    let question = select_question(&questions, &mut seed).to_string();
    let request = WunderRequest {
        user_id: user_id.clone(),
        question,
        tool_names: Vec::new(),
        stream: true,
        session_id: Some(session_id.clone()),
        model_name,
        language: None,
        config_overrides: None,
        attachments: None,
    };
    let started = Instant::now();
    let result = if request_timeout_s > 0.0 {
        tokio::time::timeout(
            Duration::from_secs_f64(request_timeout_s),
            orchestrator.run(request),
        )
        .await
        .map_err(|_| "请求超时".to_string())
        .and_then(|value| value.map_err(|err| err.to_string()))
    } else {
        orchestrator
            .run(request)
            .await
            .map_err(|err| err.to_string())
    };
    let latency_ms = started.elapsed().as_millis() as u64;
    let (usage, error_message) = match result {
        Ok(response) => (response.usage, None),
        Err(err) => (None, Some(err)),
    };
    let detail = monitor.get_detail(&session_id);
    let mut speed = extract_session_speed(detail);
    if speed.prefill_duration_s.is_none() {
        if let Some(record) = monitor.get_record(&session_id) {
            let fallback = extract_session_speed_from_record(&record);
            merge_session_speed(&mut speed, &fallback);
        }
    }
    RequestOutcome {
        latency_ms,
        usage,
        error_message,
        speed,
    }
}

fn extract_session_speed(detail: Option<Value>) -> SessionSpeed {
    let Some(detail) = detail else {
        return SessionSpeed::default();
    };
    let session = detail.get("session").unwrap_or(&detail);
    SessionSpeed {
        prefill_tokens: parse_i64_value(session.get("prefill_tokens")),
        prefill_duration_s: normalize_prefill_duration(parse_f64_value(
            session.get("prefill_duration_s"),
        )),
        prefill_speed_tps: parse_f64_value(session.get("prefill_speed_tps")),
        decode_tokens: parse_i64_value(session.get("decode_tokens")),
        decode_duration_s: normalize_duration(parse_f64_value(session.get("decode_duration_s"))),
        decode_speed_tps: parse_f64_value(session.get("decode_speed_tps")),
    }
}

fn parse_i64_value(value: Option<&Value>) -> Option<i64> {
    value
        .and_then(Value::as_i64)
        .or_else(|| value.and_then(Value::as_u64).map(|value| value as i64))
}

fn parse_f64_value(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .or_else(|| value.and_then(Value::as_i64).map(|value| value as f64))
        .or_else(|| value.and_then(Value::as_u64).map(|value| value as f64))
}

fn parse_event_timestamp(value: Option<&Value>) -> Option<f64> {
    if let Some(timestamp) = parse_f64_value(value) {
        return Some(timestamp);
    }
    let text = value.and_then(Value::as_str)?;
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|dt| dt.timestamp_millis() as f64 / 1000.0)
}

fn normalize_duration(value: Option<f64>) -> Option<f64> {
    let value = value?;
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    Some(value)
}

fn normalize_prefill_duration(value: Option<f64>) -> Option<f64> {
    let value = normalize_duration(value)?;
    if value < MIN_PREFILL_DURATION_S {
        return Some(MIN_PREFILL_DURATION_S);
    }
    Some(value)
}

fn parse_usage_tokens_from_event(data: &Value) -> (Option<i64>, Option<i64>) {
    if let Some(usage) = data.get("usage").and_then(Value::as_object) {
        let input = parse_i64_value(usage.get("input_tokens"));
        let output = parse_i64_value(usage.get("output_tokens"));
        if input.is_some() || output.is_some() {
            return (input, output);
        }
    }
    (
        parse_i64_value(data.get("input_tokens")),
        parse_i64_value(data.get("output_tokens")),
    )
}

fn extract_session_speed_from_record(record: &Value) -> SessionSpeed {
    let mut start_ts: Option<f64> = None;
    let mut first_output_ts: Option<f64> = None;
    let mut last_output_ts: Option<f64> = None;
    let mut input_tokens: Option<i64> = None;
    let mut output_tokens: Option<i64> = None;
    let mut prefill_duration_s: Option<f64> = None;
    let mut decode_duration_s: Option<f64> = None;
    let Some(events) = record.get("events").and_then(Value::as_array) else {
        return SessionSpeed::default();
    };
    for event in events {
        let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");
        let timestamp = parse_event_timestamp(event.get("timestamp"));
        let data = event.get("data").unwrap_or(&Value::Null);
        match event_type {
            "llm_request" => {
                if start_ts.is_none() {
                    start_ts = timestamp;
                }
            }
            "llm_output_delta" | "llm_output" => {
                if first_output_ts.is_none() {
                    first_output_ts = timestamp;
                }
                if let Some(ts) = timestamp {
                    last_output_ts = Some(ts);
                }
            }
            _ => {}
        }
        if matches!(event_type, "llm_output" | "token_usage") {
            let (input, output) = parse_usage_tokens_from_event(data);
            if input_tokens.is_none() {
                input_tokens = input;
            }
            if output.is_some() {
                output_tokens = output;
            }
            if prefill_duration_s.is_none() {
                prefill_duration_s =
                    normalize_prefill_duration(parse_f64_value(data.get("prefill_duration_s")));
            }
            if decode_duration_s.is_none() {
                decode_duration_s =
                    normalize_duration(parse_f64_value(data.get("decode_duration_s")));
            }
        }
    }
    if prefill_duration_s.is_none() {
        if let (Some(start), Some(first_output)) = (start_ts, first_output_ts) {
            prefill_duration_s = normalize_prefill_duration(Some((first_output - start).max(0.0)));
        }
    }
    if decode_duration_s.is_none() {
        if let (Some(first_output), Some(last_output)) = (first_output_ts, last_output_ts) {
            decode_duration_s = normalize_duration(Some((last_output - first_output).max(0.0)));
        }
    }
    let prefill_speed_tps = match (input_tokens, prefill_duration_s) {
        (Some(tokens), Some(duration)) if tokens > 0 && duration > 0.0 => {
            Some(tokens as f64 / duration)
        }
        _ => None,
    };
    let decode_speed_tps = match (output_tokens, decode_duration_s) {
        (Some(tokens), Some(duration)) if tokens > 0 && duration > 0.0 => {
            Some(tokens as f64 / duration)
        }
        _ => None,
    };
    SessionSpeed {
        prefill_tokens: input_tokens,
        prefill_duration_s,
        prefill_speed_tps,
        decode_tokens: output_tokens,
        decode_duration_s,
        decode_speed_tps,
    }
}

fn merge_session_speed(target: &mut SessionSpeed, fallback: &SessionSpeed) {
    if target.prefill_tokens.is_none() {
        target.prefill_tokens = fallback.prefill_tokens;
    }
    if target.prefill_duration_s.is_none() {
        target.prefill_duration_s = fallback.prefill_duration_s;
    }
    if target.prefill_speed_tps.is_none() {
        target.prefill_speed_tps = fallback.prefill_speed_tps;
    }
    if target.decode_tokens.is_none() {
        target.decode_tokens = fallback.decode_tokens;
    }
    if target.decode_duration_s.is_none() {
        target.decode_duration_s = fallback.decode_duration_s;
    }
    if target.decode_speed_tps.is_none() {
        target.decode_speed_tps = fallback.decode_speed_tps;
    }
}

fn seed_for_user(user_id: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    user_id.hash(&mut hasher);
    let time_seed = Utc::now().timestamp_millis() as u64;
    hasher.finish() ^ time_seed
}

fn select_question<'a>(questions: &'a [String], seed: &mut u64) -> &'a str {
    if questions.len() <= 1 {
        return questions.first().map(String::as_str).unwrap_or("");
    }
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let index = (*seed as usize) % questions.len();
    questions[index].as_str()
}

fn estimate_percentile(counts: &[u64], percentile: f64) -> Option<u64> {
    let total: u64 = counts.iter().sum();
    if total == 0 {
        return None;
    }
    let target = (total as f64 * percentile).ceil().max(1.0) as u64;
    let mut cumulative = 0u64;
    for (index, count) in counts.iter().enumerate() {
        cumulative += count;
        if cumulative >= target {
            if index < LATENCY_BUCKETS_MS.len() {
                return Some(LATENCY_BUCKETS_MS[index]);
            }
            return LATENCY_BUCKETS_MS.last().copied();
        }
    }
    LATENCY_BUCKETS_MS.last().copied()
}

fn build_bucket_snapshots(counts: &[u64]) -> Vec<LatencyBucketSnapshot> {
    let mut snapshots = Vec::with_capacity(counts.len());
    for (index, count) in counts.iter().enumerate() {
        let le_ms = if index < LATENCY_BUCKETS_MS.len() {
            Some(LATENCY_BUCKETS_MS[index])
        } else {
            None
        };
        snapshots.push(LatencyBucketSnapshot {
            le_ms,
            count: *count,
        });
    }
    snapshots
}

fn report_dir() -> PathBuf {
    PathBuf::from(REPORT_DIR)
}

fn report_index_path() -> PathBuf {
    report_dir().join(REPORT_INDEX_FILE)
}

fn report_file_path(run_id: &str) -> PathBuf {
    report_dir().join(format!("{run_id}.json"))
}

fn load_report_index() -> Vec<ThroughputSnapshot> {
    let path = report_index_path();
    let data = match fs::read_to_string(&path) {
        Ok(data) => data,
        Err(_) => return Vec::new(),
    };
    match serde_json::from_str::<Vec<ThroughputSnapshot>>(&data) {
        Ok(mut history) => {
            if history.len() > MAX_REPORT_HISTORY {
                let overflow = history.len() - MAX_REPORT_HISTORY;
                history.drain(0..overflow);
            }
            history
        }
        Err(_) => Vec::new(),
    }
}

async fn persist_report(report: &ThroughputReport) -> Result<(), String> {
    let dir = report_dir();
    tokio_fs::create_dir_all(&dir)
        .await
        .map_err(|err| err.to_string())?;
    let payload = serde_json::to_vec_pretty(report).map_err(|err| err.to_string())?;
    let path = report_file_path(&report.summary.run.id);
    tokio_fs::write(path, payload)
        .await
        .map_err(|err| err.to_string())?;
    Ok(())
}

async fn persist_report_index(history: &[ThroughputSnapshot]) -> Result<(), String> {
    let dir = report_dir();
    tokio_fs::create_dir_all(&dir)
        .await
        .map_err(|err| err.to_string())?;
    let payload = serde_json::to_vec_pretty(history).map_err(|err| err.to_string())?;
    let path = report_index_path();
    tokio_fs::write(path, payload)
        .await
        .map_err(|err| err.to_string())?;
    Ok(())
}

async fn load_report(run_id: &str) -> Result<ThroughputReport, String> {
    let path = report_file_path(run_id);
    let payload = tokio_fs::read(&path)
        .await
        .map_err(|_| "未找到对应压测报告".to_string())?;
    serde_json::from_slice::<ThroughputReport>(&payload).map_err(|err| err.to_string())
}
