use anyhow::{anyhow, Result};
use std::fmt;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockingKind {
    Db,
    Fs,
    Cpu,
    External,
}

impl BlockingKind {
    const fn queue_timeout(self) -> Duration {
        Duration::from_millis(match self {
            Self::Db => DB_QUEUE_TIMEOUT_MS,
            Self::Fs => FS_QUEUE_TIMEOUT_MS,
            Self::Cpu => CPU_QUEUE_TIMEOUT_MS,
            Self::External => EXTERNAL_QUEUE_TIMEOUT_MS,
        })
    }

    const fn exec_timeout(self) -> Duration {
        Duration::from_millis(match self {
            Self::Db => DB_EXEC_TIMEOUT_MS,
            Self::Fs => FS_EXEC_TIMEOUT_MS,
            Self::Cpu => CPU_EXEC_TIMEOUT_MS,
            Self::External => EXTERNAL_EXEC_TIMEOUT_MS,
        })
    }

    const fn concurrency(self) -> usize {
        match self {
            Self::Db => DB_CONCURRENCY,
            Self::Fs => FS_CONCURRENCY,
            Self::Cpu => CPU_CONCURRENCY,
            Self::External => EXTERNAL_CONCURRENCY,
        }
    }

    fn semaphore(self) -> Arc<Semaphore> {
        match self {
            Self::Db => static_semaphore(&DB_SEMAPHORE, self.concurrency()),
            Self::Fs => static_semaphore(&FS_SEMAPHORE, self.concurrency()),
            Self::Cpu => static_semaphore(&CPU_SEMAPHORE, self.concurrency()),
            Self::External => static_semaphore(&EXTERNAL_SEMAPHORE, self.concurrency()),
        }
    }
}

impl fmt::Display for BlockingKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Db => "db",
            Self::Fs => "fs",
            Self::Cpu => "cpu",
            Self::External => "external",
        };
        f.write_str(value)
    }
}

static DB_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();
static FS_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();
static CPU_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();
static EXTERNAL_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();

fn static_semaphore(slot: &'static OnceLock<Arc<Semaphore>>, permits: usize) -> Arc<Semaphore> {
    Arc::clone(slot.get_or_init(|| Arc::new(Semaphore::new(permits.max(1)))))
}

pub async fn run_db<T, F>(label: &'static str, task: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    run(BlockingKind::Db, label, task).await
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
    let queue_start = Instant::now();
    let queue_timeout = kind.queue_timeout();
    let permit = match timeout(queue_timeout, kind.semaphore().acquire_owned()).await {
        Ok(Ok(permit)) => permit,
        Ok(Err(_)) => {
            runtime_metrics::record_blocking_queue_timeout(
                &kind.to_string(),
                label,
                runtime_metrics::elapsed_ms_u64(queue_start),
            );
            return Err(anyhow!("blocking task {label} ({kind}) semaphore closed"));
        }
        Err(_) => {
            runtime_metrics::record_blocking_queue_timeout(
                &kind.to_string(),
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

    runtime_metrics::record_blocking_started(&kind.to_string(), label);
    let handle = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let started = Instant::now();
        let result = task();
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        let elapsed_ms_u64 = runtime_metrics::elapsed_ms_u64(started);
        let ok = result.is_ok();
        runtime_metrics::record_blocking_finished(
            &kind.to_string(),
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
        result
    });

    match timeout(exec_timeout, handle).await {
        Ok(Ok(result)) => result,
        Ok(Err(err)) => {
            runtime_metrics::record_blocking_join_error(&kind.to_string(), label, queued_ms_u64);
            Err(anyhow!("blocking task {label} ({kind}) join error: {err}"))
        }
        Err(_) => {
            runtime_metrics::record_blocking_exec_timeout(
                &kind.to_string(),
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
    use super::{run_cpu, run_db};
    use anyhow::{anyhow, Result};

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
