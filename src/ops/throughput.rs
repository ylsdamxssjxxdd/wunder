use crate::monitor::MonitorState;
use crate::orchestrator::Orchestrator;
use crate::schemas::{TokenUsage, WunderRequest};
use chrono::{DateTime, Local, Utc};
use futures::future::join_all;
use parking_lot::Mutex as ParkingMutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{hash_map::DefaultHasher, HashSet};
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
const BUILTIN_QUESTION_SET_NAME: &str = "builtin";
const BUILTIN_QUESTIONS: [&str; 50] = [
    "用一句话解释什么是大型语言模型。",
    "列出三种常见的数据库索引类型及用途。",
    "用 100 字以内说明 HTTP 与 HTTPS 的区别。",
    "将下面中文翻译成英文：人工智能正在改变世界。",
    "写出 Rust 中所有权的核心规则。",
    "给出一个用于计算阶乘的伪代码。",
    "解释什么是幂等接口，并举一个例子。",
    "比较 TCP 与 UDP 的主要差异。",
    "简要说明缓存击穿、穿透、雪崩的区别。",
    "给出 5 个提升 API 吞吐量的优化点。",
    "简述 LRU 缓存的工作原理。",
    "给出一个判断质数的算法思路。",
    "用要点说明日志采样的优缺点。",
    "写一个 SQL 查询示例：按城市统计用户数。",
    "解释什么是向量数据库及常见场景。",
    "举例说明并发与并行的区别。",
    "将英文翻译成中文：Latency is more important than throughput in interactive systems.",
    "给出一次 HTTP 请求的关键阶段。",
    "写出 JSON 与 YAML 的主要差别。",
    "简要描述 OAuth2 的授权码流程。",
    "用 3 点说明怎样设计可观测性指标。",
    "解释什么是 RPS 与 TPS。",
    "给出一次压测需要收集的核心指标列表。",
    "写出一个二分查找的步骤。",
    "简要说明 CAP 定理的含义。",
    "解释什么是消息队列以及常见用途。",
    "用 50 字以内说明什么是背压。",
    "给出一个字符串反转的示例代码（任意语言）。",
    "解释什么是服务降级，并给出一个场景。",
    "列出 4 种常见的监控告警策略。",
    "简要说明什么是微服务架构。",
    "写出 Kubernetes 的两项核心能力。",
    "给出一次接口超时排查的思路。",
    "解释什么是冷启动以及可能影响。",
    "列出 3 种常见的负载均衡算法。",
    "用一句话描述 B 树与 B+ 树的区别。",
    "解释什么是上下文窗口（Context Window）。",
    "给出一个文本摘要任务的评估指标。",
    "说明为什么需要限流，并举例。",
    "用 3 点描述 API 版本管理的建议。",
    "写出一次日志追踪链路的关键字段。",
    "解释幂等重试可能带来的问题。",
    "简要描述分布式锁的实现方式。",
    "列出 3 种常见的序列化格式。",
    "解释什么是热分区以及影响。",
    "给出一个简单的正则表达式，用于匹配邮箱。",
    "用两句话描述什么是向量相似度检索。",
    "说明如何估算文本的 token 数量。",
    "给出一个性能测试的基线建立方法。",
    "简要说明如何设置最大输出 token 的意义。",
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
    pub concurrency_list: Vec<usize>,
    pub max_concurrency: usize,
    pub questions: Vec<String>,
    pub question_set: String,
    pub user_id_prefix: String,
    pub model_name: Option<String>,
    pub request_timeout_s: f64,
    pub max_tokens: Option<u32>,
}

impl ThroughputConfig {
    pub fn new(
        concurrency_list: Vec<usize>,
        user_id_prefix: Option<String>,
        model_name: Option<String>,
        request_timeout_s: Option<f64>,
        max_tokens: Option<u32>,
    ) -> Result<Self, String> {
        let concurrency_list = normalize_concurrency_list(concurrency_list)?;
        let max_concurrency = concurrency_list.iter().copied().max().unwrap_or_default();
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
        let max_tokens = max_tokens.filter(|value| *value > 0);
        let questions = builtin_questions();
        let question_set = BUILTIN_QUESTION_SET_NAME.to_string();
        Ok(Self {
            concurrency_list,
            max_concurrency,
            questions,
            question_set,
            user_id_prefix: prefix,
            model_name,
            request_timeout_s: timeout,
            max_tokens,
        })
    }
}

fn builtin_questions() -> Vec<String> {
    BUILTIN_QUESTIONS
        .iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn normalize_concurrency_list(list: Vec<usize>) -> Result<Vec<usize>, String> {
    let mut normalized = Vec::new();
    let mut seen = HashSet::new();
    for value in list {
        if value == 0 {
            return Err("并发数必须大于 0".to_string());
        }
        if value > MAX_CONCURRENCY {
            return Err(format!("并发数不能超过 {MAX_CONCURRENCY}"));
        }
        if seen.insert(value) {
            normalized.push(value);
        }
    }
    if normalized.is_empty() {
        return Err("并发列表不能为空".to_string());
    }
    Ok(normalized)
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
            timestamp: Local::now().to_rfc3339(),
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
    pub concurrency_list: Vec<usize>,
    #[serde(default)]
    pub question_set: Option<String>,
    #[serde(default)]
    pub question_count: usize,
    pub user_id_prefix: String,
    pub stream: bool,
    pub model_name: Option<String>,
    pub request_timeout_s: f64,
    #[serde(default)]
    pub max_tokens: Option<u32>,
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

impl Default for ThroughputManager {
    fn default() -> Self {
        Self::new()
    }
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
        match state.active.as_ref().map(ActiveRun::snapshot) {
            Some(snapshot) => Ok(snapshot),
            None => Err("throughput run start failed: active snapshot missing".to_string()),
        }
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
            } else {
                state.history.last().map(|last| last.run.id.clone())
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
                concurrency_list: self.config.concurrency_list.clone(),
                question_set: Some(self.config.question_set.clone()),
                question_count: self.config.questions.len(),
                user_id_prefix: self.config.user_id_prefix.clone(),
                stream: true,
                model_name: self.config.model_name.clone(),
                request_timeout_s: self.config.request_timeout_s,
                max_tokens: self.config.max_tokens,
                started_at: self.started_at.with_timezone(&Local).to_rfc3339(),
                finished_at: self
                    .finished_at
                    .map(|value| value.with_timezone(&Local).to_rfc3339()),
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
    prefill_duration_total_s: f64,
    decode_tokens_total: u64,
    decode_duration_total_s: f64,
}

fn record_speed_sample(
    tokens: Option<i64>,
    duration: Option<f64>,
    speed: Option<f64>,
    tokens_total: &mut u64,
    duration_total_s: &mut f64,
    speed_sum: &mut f64,
    speed_count: &mut u64,
) {
    let tokens = tokens.filter(|value| *value > 0).map(|value| value as u64);
    let duration = duration.filter(|value| *value > 0.0);
    let speed = speed.filter(|value| *value > 0.0);
    let mut resolved_speed = None;
    if let (Some(tokens), Some(duration)) = (tokens, duration) {
        *tokens_total = tokens_total.saturating_add(tokens);
        *duration_total_s += duration;
        resolved_speed = Some(tokens as f64 / duration);
    } else if let (Some(tokens), Some(speed)) = (tokens, speed) {
        let derived_duration = tokens as f64 / speed;
        if derived_duration.is_finite() && derived_duration > 0.0 {
            *tokens_total = tokens_total.saturating_add(tokens);
            *duration_total_s += derived_duration;
            resolved_speed = Some(speed);
        }
    } else if let Some(speed) = speed {
        resolved_speed = Some(speed);
    }
    if let Some(value) = resolved_speed {
        *speed_sum += value;
        *speed_count += 1;
    }
}

impl SpeedAccumulator {
    fn record(&mut self, speed: &SessionSpeed) {
        record_speed_sample(
            speed.prefill_tokens,
            speed.prefill_duration_s,
            speed.prefill_speed_tps,
            &mut self.prefill_tokens_total,
            &mut self.prefill_duration_total_s,
            &mut self.prefill_speed_sum,
            &mut self.prefill_speed_count,
        );
        record_speed_sample(
            speed.decode_tokens,
            speed.decode_duration_s,
            speed.decode_speed_tps,
            &mut self.decode_tokens_total,
            &mut self.decode_duration_total_s,
            &mut self.decode_speed_sum,
            &mut self.decode_speed_count,
        );
    }

    fn single_prefill_speed(&self) -> Option<f64> {
        if self.prefill_tokens_total > 0 && self.prefill_duration_total_s > 0.0 {
            return Some(self.prefill_tokens_total as f64 / self.prefill_duration_total_s);
        }
        if self.prefill_speed_count > 0 {
            return Some(self.prefill_speed_sum / self.prefill_speed_count as f64);
        }
        None
    }

    fn single_decode_speed(&self) -> Option<f64> {
        if self.decode_tokens_total > 0 && self.decode_duration_total_s > 0.0 {
            return Some(self.decode_tokens_total as f64 / self.decode_duration_total_s);
        }
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
        let tokens = if fallback_tokens > 0 {
            fallback_tokens
        } else {
            self.prefill_tokens_total
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
        let tokens = if fallback_tokens > 0 {
            fallback_tokens
        } else {
            self.decode_tokens_total
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
    let sequence = config.concurrency_list.clone();
    let questions = Arc::new(config.questions.clone());
    let user_prefix = config.user_id_prefix.clone();
    let max_tokens = config.max_tokens;
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
                    max_tokens,
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
            timestamp: Local::now().to_rfc3339(),
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

#[allow(clippy::too_many_arguments)]
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
    max_tokens: Option<u32>,
) -> RequestOutcome {
    let user_index = index + 1;
    let user_id = format!("{user_prefix}-{concurrency}-{user_index}");
    let session_id = format!("throughput_{run_id}_{concurrency}_{user_index}");
    let mut seed = seed_for_user(&user_id);
    let question = select_question(&questions, &mut seed).to_string();
    let config_overrides = build_max_tokens_override(model_name.as_deref(), max_tokens);
    let request = WunderRequest {
        user_id: user_id.clone(),
        question,
        tool_names: Vec::new(),
        skip_tool_calls: true,
        stream: true,
        debug_payload: false,
        session_id: Some(session_id.clone()),
        agent_id: None,
        model_name,
        language: None,
        config_overrides,
        agent_prompt: None,
        attachments: None,
        allow_queue: true,
        is_admin: false,
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

fn build_max_tokens_override(model_name: Option<&str>, max_tokens: Option<u32>) -> Option<Value> {
    let max_tokens = max_tokens.filter(|value| *value > 0)?;
    let model_name = model_name?.trim();
    if model_name.is_empty() {
        return None;
    }
    Some(json!({
        "llm": {
            "models": {
                model_name: {
                    "max_output": max_tokens
                }
            }
        }
    }))
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
