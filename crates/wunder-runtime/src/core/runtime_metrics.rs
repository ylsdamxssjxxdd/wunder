use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const ALERT_BLOCKING_MAX_QUEUE_MS: u64 = 1_000;
const ALERT_BLOCKING_MAX_EXEC_MS: u64 = 10_000;
const ALERT_QUEUE_BUSY_COUNT: u64 = 1;
const ALERT_LONG_TASK_WARN_COUNT: u64 = 1;

#[derive(Debug, Serialize)]
pub struct RuntimeMetricsSnapshot {
    pub generated_at_s: f64,
    pub blocking: Vec<BlockingMetricSnapshot>,
    pub queues: Vec<QueueMetricSnapshot>,
    pub long_tasks: Vec<LongTaskMetricSnapshot>,
    pub alerts: Vec<RuntimeMetricAlert>,
    pub thresholds: RuntimeMetricThresholds,
}

#[derive(Debug, Serialize)]
pub struct RuntimeMetricThresholds {
    pub blocking_max_queue_ms: u64,
    pub blocking_max_exec_ms: u64,
    pub queue_busy_count: u64,
    pub long_task_warn_count: u64,
}

#[derive(Debug, Serialize)]
pub struct RuntimeMetricAlert {
    pub severity: &'static str,
    pub source: &'static str,
    pub label: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct BlockingMetricSnapshot {
    pub kind: String,
    pub label: String,
    pub calls: u64,
    pub ok: u64,
    pub errors: u64,
    pub queue_timeouts: u64,
    pub exec_timeouts: u64,
    pub join_errors: u64,
    pub in_flight: u64,
    pub avg_queue_ms: f64,
    pub max_queue_ms: u64,
    pub avg_exec_ms: f64,
    pub max_exec_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct QueueMetricSnapshot {
    pub name: String,
    pub capacity: u64,
    pub max_in_flight: u64,
    pub enqueued: u64,
    pub waited_enqueues: u64,
    pub busy: u64,
    pub closed: u64,
    pub processed: u64,
    pub failed: u64,
    pub in_flight: u64,
    pub avg_wait_ms: f64,
    pub max_wait_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct LongTaskMetricSnapshot {
    pub label: String,
    pub started: u64,
    pub finished: u64,
    pub warnings: u64,
    pub panics: u64,
    pub in_flight: u64,
    pub avg_elapsed_ms: f64,
    pub max_elapsed_ms: u64,
}

#[derive(Default)]
struct RuntimeMetrics {
    blocking: Mutex<BTreeMap<String, Arc<BlockingMetric>>>,
    queues: Mutex<BTreeMap<String, Arc<QueueMetric>>>,
    long_tasks: Mutex<BTreeMap<String, Arc<LongTaskMetric>>>,
}

#[derive(Default)]
struct BlockingMetric {
    kind: String,
    label: String,
    calls: AtomicU64,
    ok: AtomicU64,
    errors: AtomicU64,
    queue_timeouts: AtomicU64,
    exec_timeouts: AtomicU64,
    join_errors: AtomicU64,
    in_flight: AtomicU64,
    total_queue_ms: AtomicU64,
    max_queue_ms: AtomicU64,
    total_exec_ms: AtomicU64,
    max_exec_ms: AtomicU64,
}

#[derive(Default)]
struct QueueMetric {
    name: String,
    capacity: AtomicU64,
    max_in_flight: AtomicU64,
    enqueued: AtomicU64,
    waited_enqueues: AtomicU64,
    busy: AtomicU64,
    closed: AtomicU64,
    processed: AtomicU64,
    failed: AtomicU64,
    in_flight: AtomicU64,
    total_wait_ms: AtomicU64,
    max_wait_ms: AtomicU64,
}

#[derive(Default)]
struct LongTaskMetric {
    label: String,
    started: AtomicU64,
    finished: AtomicU64,
    warnings: AtomicU64,
    panics: AtomicU64,
    in_flight: AtomicU64,
    total_elapsed_ms: AtomicU64,
    max_elapsed_ms: AtomicU64,
}

static RUNTIME_METRICS: OnceLock<RuntimeMetrics> = OnceLock::new();

fn metrics() -> &'static RuntimeMetrics {
    RUNTIME_METRICS.get_or_init(RuntimeMetrics::default)
}

pub fn record_blocking_started(kind: &str, label: &str) {
    let metric = blocking_metric(kind, label);
    metric.calls.fetch_add(1, Ordering::Relaxed);
    metric.in_flight.fetch_add(1, Ordering::Relaxed);
}

pub fn record_blocking_finished(kind: &str, label: &str, queued_ms: u64, exec_ms: u64, ok: bool) {
    let metric = blocking_metric(kind, label);
    metric.in_flight.fetch_sub(1, Ordering::Relaxed);
    metric
        .total_queue_ms
        .fetch_add(queued_ms, Ordering::Relaxed);
    metric.total_exec_ms.fetch_add(exec_ms, Ordering::Relaxed);
    update_max(&metric.max_queue_ms, queued_ms);
    update_max(&metric.max_exec_ms, exec_ms);
    if ok {
        metric.ok.fetch_add(1, Ordering::Relaxed);
    } else {
        metric.errors.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn record_blocking_queue_timeout(kind: &str, label: &str, queued_ms: u64) {
    let metric = blocking_metric(kind, label);
    metric.calls.fetch_add(1, Ordering::Relaxed);
    metric.queue_timeouts.fetch_add(1, Ordering::Relaxed);
    metric
        .total_queue_ms
        .fetch_add(queued_ms, Ordering::Relaxed);
    update_max(&metric.max_queue_ms, queued_ms);
}

pub fn record_blocking_exec_timeout(kind: &str, label: &str, queued_ms: u64, exec_ms: u64) {
    let metric = blocking_metric(kind, label);
    metric.exec_timeouts.fetch_add(1, Ordering::Relaxed);
    metric
        .total_queue_ms
        .fetch_add(queued_ms, Ordering::Relaxed);
    metric.total_exec_ms.fetch_add(exec_ms, Ordering::Relaxed);
    update_max(&metric.max_queue_ms, queued_ms);
    update_max(&metric.max_exec_ms, exec_ms);
}

pub fn record_blocking_join_error(kind: &str, label: &str, queued_ms: u64) {
    let metric = blocking_metric(kind, label);
    metric.in_flight.fetch_sub(1, Ordering::Relaxed);
    metric.join_errors.fetch_add(1, Ordering::Relaxed);
    metric
        .total_queue_ms
        .fetch_add(queued_ms, Ordering::Relaxed);
    update_max(&metric.max_queue_ms, queued_ms);
}

pub fn record_queue_config(name: &str, capacity: usize, max_in_flight: usize) {
    let metric = queue_metric(name);
    update_max(&metric.capacity, capacity.max(1) as u64);
    update_max(&metric.max_in_flight, max_in_flight.max(1) as u64);
}

pub fn record_queue_enqueued(name: &str, waited: bool, wait_ms: u64) {
    let metric = queue_metric(name);
    metric.enqueued.fetch_add(1, Ordering::Relaxed);
    if waited {
        metric.waited_enqueues.fetch_add(1, Ordering::Relaxed);
        metric.total_wait_ms.fetch_add(wait_ms, Ordering::Relaxed);
        update_max(&metric.max_wait_ms, wait_ms);
    }
}

pub fn record_queue_busy(name: &str) {
    queue_metric(name).busy.fetch_add(1, Ordering::Relaxed);
}

pub fn record_queue_closed(name: &str) {
    queue_metric(name).closed.fetch_add(1, Ordering::Relaxed);
}

pub fn record_queue_item_started(name: &str) {
    queue_metric(name).in_flight.fetch_add(1, Ordering::Relaxed);
}

pub fn record_queue_item_finished(name: &str, ok: bool) {
    let metric = queue_metric(name);
    metric.in_flight.fetch_sub(1, Ordering::Relaxed);
    metric.processed.fetch_add(1, Ordering::Relaxed);
    if !ok {
        metric.failed.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn record_long_task_started(label: &str) {
    let metric = long_task_metric(label);
    metric.started.fetch_add(1, Ordering::Relaxed);
    metric.in_flight.fetch_add(1, Ordering::Relaxed);
}

pub fn record_long_task_finished(label: &str, elapsed_ms: u64, warned: bool) {
    let metric = long_task_metric(label);
    metric.in_flight.fetch_sub(1, Ordering::Relaxed);
    metric.finished.fetch_add(1, Ordering::Relaxed);
    metric
        .total_elapsed_ms
        .fetch_add(elapsed_ms, Ordering::Relaxed);
    update_max(&metric.max_elapsed_ms, elapsed_ms);
    if warned {
        metric.warnings.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn record_long_task_panic(label: &str, elapsed_ms: u64) {
    let metric = long_task_metric(label);
    metric.in_flight.fetch_sub(1, Ordering::Relaxed);
    metric.panics.fetch_add(1, Ordering::Relaxed);
    metric
        .total_elapsed_ms
        .fetch_add(elapsed_ms, Ordering::Relaxed);
    update_max(&metric.max_elapsed_ms, elapsed_ms);
}

pub fn snapshot() -> RuntimeMetricsSnapshot {
    let blocking = snapshot_blocking();
    let queues = snapshot_queues();
    let long_tasks = snapshot_long_tasks();
    let alerts = build_alerts(&blocking, &queues, &long_tasks);
    RuntimeMetricsSnapshot {
        generated_at_s: now_ts(),
        blocking,
        queues,
        long_tasks,
        alerts,
        thresholds: RuntimeMetricThresholds {
            blocking_max_queue_ms: ALERT_BLOCKING_MAX_QUEUE_MS,
            blocking_max_exec_ms: ALERT_BLOCKING_MAX_EXEC_MS,
            queue_busy_count: ALERT_QUEUE_BUSY_COUNT,
            long_task_warn_count: ALERT_LONG_TASK_WARN_COUNT,
        },
    }
}

fn blocking_metric(kind: &str, label: &str) -> Arc<BlockingMetric> {
    let key = format!("{kind}:{label}");
    let mut guard = metrics()
        .blocking
        .lock()
        .expect("runtime blocking metrics lock poisoned");
    guard
        .entry(key)
        .or_insert_with(|| {
            Arc::new(BlockingMetric {
                kind: kind.to_string(),
                label: label.to_string(),
                ..Default::default()
            })
        })
        .clone()
}

fn queue_metric(name: &str) -> Arc<QueueMetric> {
    let mut guard = metrics()
        .queues
        .lock()
        .expect("runtime queue metrics lock poisoned");
    guard
        .entry(name.to_string())
        .or_insert_with(|| {
            Arc::new(QueueMetric {
                name: name.to_string(),
                ..Default::default()
            })
        })
        .clone()
}

fn long_task_metric(label: &str) -> Arc<LongTaskMetric> {
    let mut guard = metrics()
        .long_tasks
        .lock()
        .expect("runtime long task metrics lock poisoned");
    guard
        .entry(label.to_string())
        .or_insert_with(|| {
            Arc::new(LongTaskMetric {
                label: label.to_string(),
                ..Default::default()
            })
        })
        .clone()
}

fn snapshot_blocking() -> Vec<BlockingMetricSnapshot> {
    let guard = metrics()
        .blocking
        .lock()
        .expect("runtime blocking metrics lock poisoned");
    let mut items = guard
        .values()
        .map(|metric| {
            let calls = metric.calls.load(Ordering::Relaxed);
            let total_queue_ms = metric.total_queue_ms.load(Ordering::Relaxed);
            let total_exec_ms = metric.total_exec_ms.load(Ordering::Relaxed);
            BlockingMetricSnapshot {
                kind: metric.kind.clone(),
                label: metric.label.clone(),
                calls,
                ok: metric.ok.load(Ordering::Relaxed),
                errors: metric.errors.load(Ordering::Relaxed),
                queue_timeouts: metric.queue_timeouts.load(Ordering::Relaxed),
                exec_timeouts: metric.exec_timeouts.load(Ordering::Relaxed),
                join_errors: metric.join_errors.load(Ordering::Relaxed),
                in_flight: metric.in_flight.load(Ordering::Relaxed),
                avg_queue_ms: avg(total_queue_ms, calls),
                max_queue_ms: metric.max_queue_ms.load(Ordering::Relaxed),
                avg_exec_ms: avg(total_exec_ms, calls),
                max_exec_ms: metric.max_exec_ms.load(Ordering::Relaxed),
            }
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .calls
            .cmp(&left.calls)
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.label.cmp(&right.label))
    });
    items
}

fn snapshot_queues() -> Vec<QueueMetricSnapshot> {
    let guard = metrics()
        .queues
        .lock()
        .expect("runtime queue metrics lock poisoned");
    let mut items = guard
        .values()
        .map(|metric| {
            let waited_enqueues = metric.waited_enqueues.load(Ordering::Relaxed);
            QueueMetricSnapshot {
                name: metric.name.clone(),
                capacity: metric.capacity.load(Ordering::Relaxed),
                max_in_flight: metric.max_in_flight.load(Ordering::Relaxed),
                enqueued: metric.enqueued.load(Ordering::Relaxed),
                waited_enqueues,
                busy: metric.busy.load(Ordering::Relaxed),
                closed: metric.closed.load(Ordering::Relaxed),
                processed: metric.processed.load(Ordering::Relaxed),
                failed: metric.failed.load(Ordering::Relaxed),
                in_flight: metric.in_flight.load(Ordering::Relaxed),
                avg_wait_ms: avg(
                    metric.total_wait_ms.load(Ordering::Relaxed),
                    waited_enqueues,
                ),
                max_wait_ms: metric.max_wait_ms.load(Ordering::Relaxed),
            }
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .enqueued
            .cmp(&left.enqueued)
            .then_with(|| left.name.cmp(&right.name))
    });
    items
}

fn snapshot_long_tasks() -> Vec<LongTaskMetricSnapshot> {
    let guard = metrics()
        .long_tasks
        .lock()
        .expect("runtime long task metrics lock poisoned");
    let mut items = guard
        .values()
        .map(|metric| {
            let finished = metric.finished.load(Ordering::Relaxed);
            LongTaskMetricSnapshot {
                label: metric.label.clone(),
                started: metric.started.load(Ordering::Relaxed),
                finished,
                warnings: metric.warnings.load(Ordering::Relaxed),
                panics: metric.panics.load(Ordering::Relaxed),
                in_flight: metric.in_flight.load(Ordering::Relaxed),
                avg_elapsed_ms: avg(metric.total_elapsed_ms.load(Ordering::Relaxed), finished),
                max_elapsed_ms: metric.max_elapsed_ms.load(Ordering::Relaxed),
            }
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .started
            .cmp(&left.started)
            .then_with(|| left.label.cmp(&right.label))
    });
    items
}

fn build_alerts(
    blocking: &[BlockingMetricSnapshot],
    queues: &[QueueMetricSnapshot],
    long_tasks: &[LongTaskMetricSnapshot],
) -> Vec<RuntimeMetricAlert> {
    let mut alerts = Vec::new();
    for item in blocking {
        if item.queue_timeouts > 0 || item.exec_timeouts > 0 || item.join_errors > 0 {
            alerts.push(RuntimeMetricAlert {
                severity: "critical",
                source: "blocking",
                label: item.label.clone(),
                message: format!(
                    "{} queue_timeouts={} exec_timeouts={} join_errors={}",
                    item.kind, item.queue_timeouts, item.exec_timeouts, item.join_errors
                ),
            });
        } else if item.max_queue_ms >= ALERT_BLOCKING_MAX_QUEUE_MS
            || item.max_exec_ms >= ALERT_BLOCKING_MAX_EXEC_MS
        {
            alerts.push(RuntimeMetricAlert {
                severity: "warning",
                source: "blocking",
                label: item.label.clone(),
                message: format!(
                    "{} max_queue_ms={} max_exec_ms={}",
                    item.kind, item.max_queue_ms, item.max_exec_ms
                ),
            });
        }
    }
    for item in queues {
        if item.busy >= ALERT_QUEUE_BUSY_COUNT || item.failed > 0 || item.closed > 0 {
            alerts.push(RuntimeMetricAlert {
                severity: if item.busy > 0 { "warning" } else { "critical" },
                source: "queue",
                label: item.name.clone(),
                message: format!(
                    "busy={} failed={} closed={}",
                    item.busy, item.failed, item.closed
                ),
            });
        }
    }
    for item in long_tasks {
        if item.panics > 0 || item.warnings >= ALERT_LONG_TASK_WARN_COUNT {
            alerts.push(RuntimeMetricAlert {
                severity: if item.panics > 0 {
                    "critical"
                } else {
                    "warning"
                },
                source: "long_task",
                label: item.label.clone(),
                message: format!(
                    "warnings={} panics={} max_elapsed_ms={}",
                    item.warnings, item.panics, item.max_elapsed_ms
                ),
            });
        }
    }
    alerts
}

fn update_max(target: &AtomicU64, value: u64) {
    let mut current = target.load(Ordering::Relaxed);
    while value > current {
        match target.compare_exchange(current, value, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(next) => current = next,
        }
    }
}

fn avg(total: u64, count: u64) -> f64 {
    if count == 0 {
        0.0
    } else {
        ((total as f64 / count as f64) * 100.0).round() / 100.0
    }
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn duration_ms_u64(elapsed_ms: f64) -> u64 {
    if !elapsed_ms.is_finite() || elapsed_ms <= 0.0 {
        0
    } else {
        elapsed_ms.round().min(u64::MAX as f64) as u64
    }
}

pub(crate) fn elapsed_ms_u64(started: std::time::Instant) -> u64 {
    duration_ms_u64(started.elapsed().as_secs_f64() * 1000.0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn snapshot_includes_recorded_runtime_metrics() {
        super::record_blocking_started("db", "runtime_metrics.test.blocking");
        super::record_blocking_finished("db", "runtime_metrics.test.blocking", 2, 3, true);
        super::record_queue_enqueued("runtime_metrics.test.queue", false, 0);
        super::record_queue_busy("runtime_metrics.test.queue");
        super::record_long_task_started("runtime_metrics.test.long");
        super::record_long_task_finished("runtime_metrics.test.long", 4, false);

        let snapshot = super::snapshot();
        assert!(snapshot
            .blocking
            .iter()
            .any(|item| item.label == "runtime_metrics.test.blocking" && item.calls > 0));
        assert!(snapshot
            .queues
            .iter()
            .any(|item| item.name == "runtime_metrics.test.queue" && item.busy > 0));
        assert!(snapshot
            .long_tasks
            .iter()
            .any(|item| item.label == "runtime_metrics.test.long" && item.finished > 0));
    }
}
