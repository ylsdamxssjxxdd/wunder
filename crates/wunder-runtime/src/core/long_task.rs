use futures::FutureExt;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;
use tracing::{debug, warn};

use super::runtime_metrics;

pub(crate) struct TaskScope {
    label: &'static str,
    started: Instant,
}

pub(crate) fn scope(label: &'static str) -> TaskScope {
    runtime_metrics::record_long_task_started(label);
    TaskScope {
        label,
        started: Instant::now(),
    }
}

impl Drop for TaskScope {
    fn drop(&mut self) {
        runtime_metrics::record_long_task_finished(
            self.label,
            runtime_metrics::elapsed_ms_u64(self.started),
            false,
        );
    }
}

pub fn spawn<F>(label: &'static str, future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    spawn_inner(label, None, future)
}

pub fn spawn_with_warn_after<F>(
    label: &'static str,
    warn_after: Duration,
    future: F,
) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    spawn_inner(label, Some(warn_after), future)
}

fn spawn_inner<F>(
    label: &'static str,
    warn_after: Option<Duration>,
    future: F,
) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    tokio::spawn(async move {
        let started = Instant::now();
        runtime_metrics::record_long_task_started(label);
        debug!(long_task.label = label, "long task started");
        let output = match AssertUnwindSafe(future).catch_unwind().await {
            Ok(output) => output,
            Err(payload) => {
                runtime_metrics::record_long_task_panic(
                    label,
                    runtime_metrics::elapsed_ms_u64(started),
                );
                std::panic::resume_unwind(payload);
            }
        };
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        let elapsed_ms_u64 = runtime_metrics::elapsed_ms_u64(started);
        let warned = warn_after.is_some_and(|threshold| started.elapsed() >= threshold);
        runtime_metrics::record_long_task_finished(label, elapsed_ms_u64, warned);
        if warned {
            warn!(
                long_task.label = label,
                long_task.elapsed_ms = elapsed_ms,
                "long task finished after warning threshold"
            );
        } else {
            debug!(
                long_task.label = label,
                long_task.elapsed_ms = elapsed_ms,
                "long task finished"
            );
        }
        output
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    #[test]
    fn scoped_task_records_completion_on_early_exit() {
        let label = "long_task.test.scope.early_exit";
        let before = super::runtime_metrics::snapshot()
            .long_tasks
            .into_iter()
            .find(|metric| metric.label == label);
        let started_before = before.as_ref().map_or(0, |metric| metric.started);
        let finished_before = before.as_ref().map_or(0, |metric| metric.finished);
        let operation = || -> Result<(), ()> {
            let _scope = super::scope(label);
            Err(())
        };
        assert!(operation().is_err());
        let after = super::runtime_metrics::snapshot()
            .long_tasks
            .into_iter()
            .find(|metric| metric.label == label)
            .unwrap();
        assert_eq!(after.started, started_before + 1);
        assert_eq!(after.finished, finished_before + 1);
        assert_eq!(after.in_flight, 0);
    }

    #[tokio::test]
    async fn long_task_spawn_completes() {
        let handle = super::spawn("long_task.test", async {});
        handle.await.expect("long task should join");
    }

    #[tokio::test]
    async fn long_task_spawn_with_warn_after_completes() {
        let handle = super::spawn_with_warn_after(
            "long_task.test.warn_after",
            Duration::from_secs(60),
            async {},
        );
        handle.await.expect("long task should join");
    }
}
