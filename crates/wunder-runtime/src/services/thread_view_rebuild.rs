//! Bounded background migration. A page open never starts a rebuild.
use crate::{core::blocking, storage::StorageBackend};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

static WORK_US: AtomicU64 = AtomicU64::new(0);
static BATCHES: AtomicU64 = AtomicU64::new(0);
static FAILURES: AtomicU64 = AtomicU64::new(0);
static STARTED: OnceLock<Instant> = OnceLock::new();

// Live thread writes update their projection in the same transaction. A clean
// maintenance pass is therefore only a cross-process/legacy recovery path;
// do not wake an idle desktop disk every few seconds for it.
const IDLE_RECOVERY_INTERVAL: Duration = Duration::from_secs(60);

fn record_work(elapsed: Duration, failed: bool) {
    WORK_US.fetch_add(
        elapsed.as_micros().min(u64::MAX as u128) as u64,
        Ordering::Relaxed,
    );
    BATCHES.fetch_add(1, Ordering::Relaxed);
    if failed {
        FAILURES.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn snapshot() -> serde_json::Value {
    serde_json::json!({
        "elapsed_ms": STARTED.get().map(|t| t.elapsed().as_secs_f64()*1000.0).unwrap_or(0.0),
        "work_ms": WORK_US.load(Ordering::Relaxed) as f64 / 1000.0,
        "batches": BATCHES.load(Ordering::Relaxed),
        "failures": FAILURES.load(Ordering::Relaxed),
        "target_duty_ratio": 0.01,
    })
}

// At most ~1% of one maintenance worker's wall time is spent rebuilding. A
// fixed 20ms delay after a slow batch allowed it to run nearly continuously.
fn maintenance_delay(elapsed: Duration) -> Duration {
    elapsed.saturating_mul(99).max(Duration::from_secs(1))
}

fn retry_delay(failures: u32) -> Duration {
    Duration::from_secs(
        5u64.saturating_mul(1u64 << failures.saturating_sub(1).min(6))
            .min(300),
    )
}

pub fn start(storage: Arc<dyn StorageBackend>) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    tokio::spawn(async move {
        STARTED.get_or_init(Instant::now);
        tokio::time::sleep(Duration::from_secs(10)).await;
        let mut failures = 0u32;
        loop {
            let started = Instant::now();
            let db = storage.clone();
            let pending = match blocking::run_db("thread_view.rebuild.pending", move || {
                db.pending_thread_view_rebuilds()
            })
            .await
            {
                Ok(pending) => pending,
                Err(error) => {
                    let elapsed = started.elapsed();
                    record_work(elapsed, true);
                    failures = failures.saturating_add(1);
                    tracing::warn!(%error, "thread view discovery deferred");
                    tokio::time::sleep(retry_delay(failures).max(maintenance_delay(elapsed))).await;
                    continue;
                }
            };
            let elapsed = started.elapsed();
            record_work(elapsed, false);
            tokio::time::sleep(maintenance_delay(elapsed)).await;
            if pending.is_empty() {
                failures = 0;
                tokio::time::sleep(IDLE_RECOVERY_INTERVAL).await;
                continue;
            }
            // A small indexed pending page, processed serially.
            for (user, session, after) in pending {
                let started = Instant::now();
                let db = storage.clone();
                let result = blocking::run_db("thread_view.rebuild.page", move || {
                    db.rebuild_thread_view_page(&user, &session, after)
                })
                .await;
                // Backoff is idle time, so never include it in the work budget.
                let elapsed = started.elapsed();
                record_work(elapsed, result.is_err());
                let mut idle = maintenance_delay(elapsed);
                if let Err(error) = result {
                    failures = failures.saturating_add(1);
                    tracing::warn!(%error,"thread view rebuild deferred");
                    idle = idle.max(retry_delay(failures));
                } else {
                    failures = 0;
                }
                tokio::time::sleep(idle).await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_batches_keep_the_same_low_duty_cycle() {
        for elapsed in [Duration::from_millis(40), Duration::from_secs(2)] {
            let idle = maintenance_delay(elapsed);
            assert!(elapsed.as_secs_f64() / (elapsed + idle).as_secs_f64() <= 0.010001);
        }
        assert_eq!(maintenance_delay(Duration::ZERO), Duration::from_secs(1));
        assert_eq!(retry_delay(1), Duration::from_secs(5));
        assert_eq!(retry_delay(2), Duration::from_secs(10));
        assert_eq!(retry_delay(u32::MAX), Duration::from_secs(300));
    }
}
