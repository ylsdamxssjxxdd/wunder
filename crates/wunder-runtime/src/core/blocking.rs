use anyhow::{anyhow, Result};
use std::fmt;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tokio::time::timeout;
use tracing::{debug, warn};

use super::runtime_metrics;

const DB_CONCURRENCY: usize = 24;
const FS_CONCURRENCY: usize = 12;
const CPU_CONCURRENCY: usize = 4;
const EXTERNAL_CONCURRENCY: usize = 8;

const DB_QUEUE_TIMEOUT_MS: u64 = 5_000;
const FS_QUEUE_TIMEOUT_MS: u64 = 5_000;
const CPU_QUEUE_TIMEOUT_MS: u64 = 2_000;
const EXTERNAL_QUEUE_TIMEOUT_MS: u64 = 5_000;

const DB_EXEC_TIMEOUT_MS: u64 = 30_000;
const FS_EXEC_TIMEOUT_MS: u64 = 60_000;
const CPU_EXEC_TIMEOUT_MS: u64 = 60_000;
const EXTERNAL_EXEC_TIMEOUT_MS: u64 = 30_000;

const BLOCKING_KIND_COUNT: usize = 5;

// A blocking thread cannot be cancelled, so its permit stays claimed until the
// operation returns even after the caller gave up. These states mark exactly the
// permits held by a task that already exceeded its execution timeout.
const TASK_RUNNING: u8 = 0;
const TASK_ABANDONED: u8 = 1;
const TASK_RETURNED: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockingKind {
    Db,
    View,
    Fs,
    Cpu,
    External,
}

impl BlockingKind {
    const fn queue_timeout(self) -> Duration {
        Duration::from_millis(match self {
            Self::Db => DB_QUEUE_TIMEOUT_MS,
            Self::View => 150,
            Self::Fs => FS_QUEUE_TIMEOUT_MS,
            Self::Cpu => CPU_QUEUE_TIMEOUT_MS,
            Self::External => EXTERNAL_QUEUE_TIMEOUT_MS,
        })
    }

    const fn exec_timeout(self) -> Duration {
        Duration::from_millis(match self {
            Self::Db => DB_EXEC_TIMEOUT_MS,
            Self::View => 2000,
            Self::Fs => FS_EXEC_TIMEOUT_MS,
            Self::Cpu => CPU_EXEC_TIMEOUT_MS,
            Self::External => EXTERNAL_EXEC_TIMEOUT_MS,
        })
    }

    const fn concurrency(self) -> usize {
        match self {
            Self::Db => DB_CONCURRENCY,
            Self::View => 8,
            Self::Fs => FS_CONCURRENCY,
            Self::Cpu => CPU_CONCURRENCY,
            Self::External => EXTERNAL_CONCURRENCY,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Db => "db",
            Self::View => "view",
            Self::Fs => "fs",
            Self::Cpu => "cpu",
            Self::External => "external",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Db => 0,
            Self::View => 4,
            Self::Fs => 1,
            Self::Cpu => 2,
            Self::External => 3,
        }
    }

    fn semaphore(self) -> Arc<Semaphore> {
        Arc::clone(&class_semaphores()[self.index()])
    }
}

impl fmt::Display for BlockingKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

static CLASS_SEMAPHORES: OnceLock<[Arc<Semaphore>; BLOCKING_KIND_COUNT]> = OnceLock::new();
static CLASS_STALLED_PERMITS: [AtomicU64; BLOCKING_KIND_COUNT] =
    [const { AtomicU64::new(0) }; BLOCKING_KIND_COUNT];

fn class_semaphores() -> &'static [Arc<Semaphore>; BLOCKING_KIND_COUNT] {
    CLASS_SEMAPHORES.get_or_init(|| {
        [
            Arc::new(Semaphore::new(BlockingKind::Db.concurrency().max(1))),
            Arc::new(Semaphore::new(BlockingKind::Fs.concurrency().max(1))),
            Arc::new(Semaphore::new(BlockingKind::Cpu.concurrency().max(1))),
            Arc::new(Semaphore::new(BlockingKind::External.concurrency().max(1))),
            Arc::new(Semaphore::new(BlockingKind::View.concurrency().max(1))),
        ]
    })
}

fn class_kinds() -> [BlockingKind; BLOCKING_KIND_COUNT] {
    [
        BlockingKind::View,
        BlockingKind::Db,
        BlockingKind::Fs,
        BlockingKind::Cpu,
        BlockingKind::External,
    ]
}

/// Reserved capacity across every blocking class. The blocking pool has to be at
/// least this large or tokio's shared pool becomes the unaccounted bottleneck.
pub fn total_concurrency() -> usize {
    class_kinds()
        .into_iter()
        .map(|kind| kind.concurrency())
        .sum()
}

/// Permits still held by a task whose caller already gave up on it.
pub fn stalled_permits(kind: BlockingKind) -> usize {
    CLASS_STALLED_PERMITS[kind.index()].load(Ordering::Relaxed) as usize
}

#[derive(Debug, Clone, Copy)]
pub struct ClassStall {
    pub kind: &'static str,
    pub stalled_permits: usize,
    pub concurrency: usize,
}

/// Classes holding stuck permits, surfaced through runtime metric alerts.
pub fn class_stalls() -> Vec<ClassStall> {
    class_kinds()
        .into_iter()
        .filter_map(|kind| {
            let stalled = stalled_permits(kind);
            (stalled > 0).then_some(ClassStall {
                kind: kind.name(),
                stalled_permits: stalled,
                concurrency: kind.concurrency(),
            })
        })
        .collect()
}

pub async fn run_db<T, F>(label: &'static str, task: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    run(BlockingKind::Db, label, task).await
}

/// Presentation reads have reserved capacity and a short queue budget.
pub async fn run_view<T, F>(label: &'static str, task: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    run(BlockingKind::View, label, task).await
}

pub async fn run_fs<T, F>(label: &'static str, task: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    run(BlockingKind::Fs, label, task).await
}

pub async fn run_cpu<T, F>(label: &'static str, task: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    run(BlockingKind::Cpu, label, task).await
}

pub async fn run_external<T, F>(label: &'static str, task: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    run(BlockingKind::External, label, task).await
}

pub async fn run_external_with_timeout<T, F>(
    label: &'static str,
    exec_timeout: Duration,
    task: F,
) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    run_with_exec_timeout(BlockingKind::External, label, exec_timeout, task).await
}

pub async fn run<T, F>(kind: BlockingKind, label: &'static str, task: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    run_with_exec_timeout(kind, label, kind.exec_timeout(), task).await
}

pub async fn run_with_exec_timeout<T, F>(
    kind: BlockingKind,
    label: &'static str,
    exec_timeout: Duration,
    task: F,
) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    let stalled_counter = &CLASS_STALLED_PERMITS[kind.index()];
    let concurrency = kind.concurrency().max(1) as u64;
    let stalled = stalled_counter.load(Ordering::Relaxed);
    if stalled >= concurrency {
        // Every permit of this class is held by a task its caller already gave up
        // on. Queuing behind them would only add a full queue timeout to a request
        // that cannot be served.
        runtime_metrics::record_blocking_class_reject(kind.name(), label);
        return Err(anyhow!(
            "blocking class {kind} is wedged: {stalled}/{concurrency} permits are held by tasks \
             that exceeded their execution timeout; rejected {label}"
        ));
    }

    let queue_start = Instant::now();
    let queue_timeout = kind.queue_timeout();
    let permit = match timeout(queue_timeout, kind.semaphore().acquire_owned()).await {
        Ok(Ok(permit)) => permit,
        Ok(Err(_)) => {
            runtime_metrics::record_blocking_queue_timeout(
                kind.name(),
                label,
                runtime_metrics::elapsed_ms_u64(queue_start),
            );
            return Err(anyhow!("blocking task {label} ({kind}) semaphore closed"));
        }
        Err(_) => {
            runtime_metrics::record_blocking_queue_timeout(
                kind.name(),
                label,
                runtime_metrics::elapsed_ms_u64(queue_start),
            );
            return Err(anyhow!(
                "blocking task {label} ({kind}) queue timeout after {}ms",
                queue_timeout.as_millis()
            ));
        }
    };
    let queued_ms = queue_start.elapsed().as_secs_f64() * 1000.0;
    let queued_ms_u64 = runtime_metrics::elapsed_ms_u64(queue_start);
    if queued_ms >= 100.0 {
        warn!(
            blocking.kind = %kind,
            blocking.label = label,
            blocking.queued_ms = queued_ms,
            "blocking task waited before execution"
        );
    }

    runtime_metrics::record_blocking_started(kind.name(), label);
    let task_state = Arc::new(AtomicU8::new(TASK_RUNNING));
    let handle = {
        let task_state = Arc::clone(&task_state);
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let started = Instant::now();
            let result = task();
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            let elapsed_ms_u64 = runtime_metrics::elapsed_ms_u64(started);
            let ok = result.is_ok();
            runtime_metrics::record_blocking_finished(
                kind.name(),
                label,
                queued_ms_u64,
                elapsed_ms_u64,
                ok,
            );
            debug!(
                blocking.kind = %kind,
                blocking.label = label,
                blocking.elapsed_ms = elapsed_ms,
                "blocking task finished"
            );
            if task_state.swap(TASK_RETURNED, Ordering::AcqRel) == TASK_ABANDONED {
                stalled_counter.fetch_sub(1, Ordering::AcqRel);
            }
            result
        })
    };

    match timeout(exec_timeout, handle).await {
        Ok(Ok(result)) => result,
        Ok(Err(err)) => {
            // The closure died before it could release its own permit accounting.
            if task_state.swap(TASK_RETURNED, Ordering::AcqRel) == TASK_ABANDONED {
                stalled_counter.fetch_sub(1, Ordering::AcqRel);
            }
            runtime_metrics::record_blocking_join_error(kind.name(), label, queued_ms_u64);
            Err(anyhow!("blocking task {label} ({kind}) join error: {err}"))
        }
        Err(_) => {
            if task_state.swap(TASK_ABANDONED, Ordering::AcqRel) == TASK_RUNNING {
                let stalled = stalled_counter.fetch_add(1, Ordering::AcqRel) + 1;
                warn!(
                    blocking.kind = %kind,
                    blocking.label = label,
                    blocking.stalled_permits = stalled,
                    blocking.concurrency = concurrency,
                    "blocking task gave up its wait but the permit stays claimed until the operation returns"
                );
            }
            runtime_metrics::record_blocking_exec_timeout(
                kind.name(),
                label,
                queued_ms_u64,
                exec_timeout.as_millis().min(u128::from(u64::MAX)) as u64,
            );
            Err(anyhow!(
                "blocking task {label} ({kind}) timed out after {}ms",
                exec_timeout.as_millis()
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{run_cpu, run_db, run_with_exec_timeout, stalled_permits, BlockingKind};
    use anyhow::{anyhow, Result};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    #[test]
    fn every_class_reserve_fits_inside_the_reported_total() {
        use super::class_kinds;
        let total = super::total_concurrency();
        assert_eq!(total, 56, "class reserves must stay explicit");
        for kind in class_kinds() {
            assert!(kind.concurrency() > 0);
            assert!(kind.concurrency() <= total);
        }
    }

    #[test]
    fn view_reads_have_independent_capacity_and_short_deadlines() {
        assert!(!Arc::ptr_eq(&BlockingKind::View.semaphore(), &BlockingKind::Db.semaphore()));
        assert_eq!(BlockingKind::View.concurrency(), 8);
        assert_eq!(BlockingKind::View.queue_timeout(), Duration::from_millis(150));
        assert_eq!(BlockingKind::View.exec_timeout(), Duration::from_secs(2));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn abandoned_permit_is_reclaimed_once_the_task_returns() {
        let baseline = stalled_permits(BlockingKind::External);
        let release = Arc::new(AtomicBool::new(false));
        let gate = Arc::clone(&release);
        let result = run_with_exec_timeout(
            BlockingKind::External,
            "blocking.test.abandoned_permit",
            Duration::from_millis(30),
            move || {
                while !gate.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Ok(())
            },
        )
        .await;
        assert!(
            result.is_err(),
            "the caller must stop waiting once the execution timeout passes"
        );
        assert_eq!(
            stalled_permits(BlockingKind::External),
            baseline + 1,
            "a task that outlived its caller still holds its permit and must be counted"
        );

        release.store(true, Ordering::Release);
        let recovery_started = Instant::now();
        while stalled_permits(BlockingKind::External) != baseline {
            assert!(
                recovery_started.elapsed() < Duration::from_secs(5),
                "permit accounting never recovered after the task returned"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn controlled_blocking_returns_value() {
        let value = run_db("blocking.test.ok", || Ok::<_, anyhow::Error>(42))
            .await
            .expect("blocking task should complete");
        assert_eq!(value, 42);
    }

    #[tokio::test]
    async fn controlled_blocking_propagates_task_error() {
        let result: Result<()> =
            run_cpu("blocking.test.err", || Err(anyhow!("expected error"))).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("expected error"));
    }
}
