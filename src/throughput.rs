use crate::orchestrator::Orchestrator;
use crate::schemas::WunderRequest;
use chrono::{DateTime, Utc};
use parking_lot::Mutex as ParkingMutex;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs as tokio_fs;
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

const DEFAULT_USER_PREFIX: &str = "throughput_user";
const MAX_USERS: usize = 500;
const MAX_DURATION_S: f64 = 24.0 * 60.0 * 60.0;
const MAX_ERROR_SAMPLES: usize = 20;
const MAX_REPORT_HISTORY: usize = 50;
const REPORT_DIR: &str = "data/throughput";
const REPORT_INDEX_FILE: &str = "index.json";
const SAMPLE_INTERVAL_MS: u64 = 1000;
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
    pub users: usize,
    pub duration_s: f64,
    pub questions: Vec<String>,
    pub user_id_prefix: String,
    pub request_timeout_s: f64,
}

impl ThroughputConfig {
    pub fn new(
        users: usize,
        duration_s: f64,
        question: Option<String>,
        questions: Option<Vec<String>>,
        user_id_prefix: Option<String>,
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
        if users == 0 {
            return Err("模拟用户数必须大于 0".to_string());
        }
        if users > MAX_USERS {
            return Err(format!("模拟用户数不能超过 {MAX_USERS}"));
        }
        if !duration_s.is_finite() || duration_s < 0.0 {
            return Err("模拟时间必须为非负数".to_string());
        }
        if duration_s > MAX_DURATION_S {
            let limit = MAX_DURATION_S as u64;
            return Err(format!("模拟时间不能超过 {limit} 秒"));
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
        Ok(Self {
            users,
            duration_s,
            questions,
            user_id_prefix: prefix,
            request_timeout_s: timeout,
        })
    }
}

struct ThroughputMetrics {
    total: AtomicU64,
    success: AtomicU64,
    error: AtomicU64,
    total_latency_ms: AtomicU64,
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
    ) {
        self.total.fetch_add(1, Ordering::Relaxed);
        self.total_latency_ms
            .fetch_add(latency_ms, Ordering::Relaxed);
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

    fn push_sample(&self, elapsed_s: f64) {
        let snapshot = self.snapshot(elapsed_s);
        let sample = ThroughputSample::from_snapshot(snapshot, elapsed_s);
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
    pub users: usize,
    pub duration_s: f64,
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

#[derive(Clone, Serialize, Deserialize)]
pub struct ThroughputSample {
    pub timestamp: String,
    pub elapsed_s: f64,
    pub total_requests: u64,
    pub success_requests: u64,
    pub error_requests: u64,
    pub rps: f64,
    pub avg_latency_ms: Option<u64>,
    pub p50_latency_ms: Option<u64>,
    pub p90_latency_ms: Option<u64>,
    pub p99_latency_ms: Option<u64>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub avg_total_tokens: Option<u64>,
}

impl ThroughputSample {
    fn from_snapshot(metrics: ThroughputMetricsSnapshot, elapsed_s: f64) -> Self {
        Self {
            timestamp: Utc::now().to_rfc3339(),
            elapsed_s,
            total_requests: metrics.total_requests,
            success_requests: metrics.success_requests,
            error_requests: metrics.error_requests,
            rps: metrics.rps,
            avg_latency_ms: metrics.avg_latency_ms,
            p50_latency_ms: metrics.p50_latency_ms,
            p90_latency_ms: metrics.p90_latency_ms,
            p99_latency_ms: metrics.p99_latency_ms,
            input_tokens: metrics.input_tokens,
            output_tokens: metrics.output_tokens,
            total_tokens: metrics.total_tokens,
            avg_total_tokens: metrics.avg_total_tokens,
        }
    }
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
                run_id,
                config,
                metrics,
                stop_flag,
                started_instant,
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
                users: self.config.users,
                duration_s: self.config.duration_s,
                question: self.config.questions.first().cloned(),
                questions: self.config.questions.clone(),
                user_id_prefix: self.config.user_id_prefix.clone(),
                stream: true,
                model_name: None,
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

async fn run_supervisor(
    inner: Arc<ThroughputManagerInner>,
    orchestrator: Arc<Orchestrator>,
    run_id: String,
    config: ThroughputConfig,
    metrics: Arc<ThroughputMetrics>,
    stop_flag: Arc<AtomicBool>,
    started_instant: Instant,
) {
    let end_at = if config.duration_s > 0.0 {
        Some(started_instant + Duration::from_secs_f64(config.duration_s))
    } else {
        None
    };
    let done_notify = Arc::new(Notify::new());
    let sampler_handle = {
        let metrics = Arc::clone(&metrics);
        let stop_flag = Arc::clone(&stop_flag);
        let done_notify = Arc::clone(&done_notify);
        let duration_s = config.duration_s;
        tokio::spawn(async move {
            run_sampler(metrics, started_instant, duration_s, stop_flag, done_notify).await;
        })
    };
    let mut handles = Vec::with_capacity(config.users);
    let questions = Arc::new(config.questions.clone());
    for index in 0..config.users {
        let orchestrator = Arc::clone(&orchestrator);
        let metrics = Arc::clone(&metrics);
        let stop_flag = Arc::clone(&stop_flag);
        let questions = Arc::clone(&questions);
        let user_index = index + 1;
        let user_id = format!("{prefix}-{user_index}", prefix = config.user_id_prefix);
        let request_timeout_s = config.request_timeout_s;
        let handle = tokio::spawn(async move {
            run_worker(
                orchestrator,
                user_id,
                questions,
                end_at,
                stop_flag,
                metrics,
                request_timeout_s,
            )
            .await;
        });
        handles.push(handle);
    }
    for handle in handles {
        let _ = handle.await;
    }
    done_notify.notify_waiters();
    let _ = sampler_handle.await;

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

async fn run_worker(
    orchestrator: Arc<Orchestrator>,
    user_id: String,
    questions: Arc<Vec<String>>,
    end_at: Option<Instant>,
    stop_flag: Arc<AtomicBool>,
    metrics: Arc<ThroughputMetrics>,
    request_timeout_s: f64,
) {
    let single_shot = end_at.is_none();
    let mut seed = seed_for_user(&user_id);
    loop {
        if stop_flag.load(Ordering::Relaxed) {
            break;
        }
        if let Some(end_at) = end_at {
            if Instant::now() >= end_at {
                break;
            }
        }
        let started = Instant::now();
        let question = select_question(&questions, &mut seed).to_string();
        let request = WunderRequest {
            user_id: user_id.clone(),
            question,
            tool_names: Vec::new(),
            stream: true,
            session_id: None,
            model_name: None,
            language: None,
            config_overrides: None,
            attachments: None,
        };
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
        match result {
            Ok(response) => {
                metrics.record(latency_ms, response.usage, None);
            }
            Err(err) => {
                metrics.record(latency_ms, None, Some(err));
            }
        }
        if single_shot {
            break;
        }
    }
}

async fn run_sampler(
    metrics: Arc<ThroughputMetrics>,
    started_instant: Instant,
    duration_s: f64,
    stop_flag: Arc<AtomicBool>,
    done_notify: Arc<Notify>,
) {
    let interval = Duration::from_millis(SAMPLE_INTERVAL_MS);
    loop {
        if stop_flag.load(Ordering::Relaxed) {
            break;
        }
        if duration_s > 0.0 && started_instant.elapsed().as_secs_f64() >= duration_s {
            break;
        }
        let elapsed_s = started_instant.elapsed().as_secs_f64();
        metrics.push_sample(elapsed_s);
        tokio::select! {
            _ = done_notify.notified() => break,
            _ = tokio::time::sleep(interval) => {}
        }
    }
    let elapsed_s = started_instant.elapsed().as_secs_f64();
    metrics.push_sample(elapsed_s);
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
