pub(crate) const DEFAULT_CRON_ERROR_BACKOFF_SCHEDULE_MS: [u64; 5] =
    [30_000, 60_000, 5 * 60_000, 15 * 60_000, 60 * 60_000];

const MIN_SCHEDULER_SLEEP_MS: u64 = 200;

pub(crate) fn compute_error_backoff_ms(consecutive_failures: i64) -> u64 {
    let failures = consecutive_failures.max(1) as usize;
    let index = failures.saturating_sub(1).min(
        DEFAULT_CRON_ERROR_BACKOFF_SCHEDULE_MS
            .len()
            .saturating_sub(1),
    );
    DEFAULT_CRON_ERROR_BACKOFF_SCHEDULE_MS[index]
}

pub(crate) fn compute_scheduler_sleep_ms(
    now: f64,
    next_run_at: Option<f64>,
    poll_interval_ms: u64,
    max_idle_sleep_ms: u64,
) -> u64 {
    let max_idle_sleep_ms = max_idle_sleep_ms.max(MIN_SCHEDULER_SLEEP_MS);
    let overdue_sleep_ms = poll_interval_ms
        .max(MIN_SCHEDULER_SLEEP_MS)
        .min(max_idle_sleep_ms);

    match next_run_at {
        Some(next_at) => {
            let delta_ms = ((next_at - now) * 1000.0).ceil() as i64;
            if delta_ms <= 0 {
                overdue_sleep_ms
            } else {
                (delta_ms as u64).clamp(MIN_SCHEDULER_SLEEP_MS, max_idle_sleep_ms)
            }
        }
        None => max_idle_sleep_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::{compute_error_backoff_ms, compute_scheduler_sleep_ms};

    #[test]
    fn error_backoff_clamps_to_tail_bucket() {
        assert_eq!(compute_error_backoff_ms(1), 30_000);
        assert_eq!(compute_error_backoff_ms(2), 60_000);
        assert_eq!(compute_error_backoff_ms(3), 5 * 60_000);
        assert_eq!(compute_error_backoff_ms(4), 15 * 60_000);
        assert_eq!(compute_error_backoff_ms(5), 60 * 60_000);
        assert_eq!(compute_error_backoff_ms(12), 60 * 60_000);
    }

    #[test]
    fn scheduler_sleep_uses_idle_cap_for_far_future_jobs() {
        let sleep_ms = compute_scheduler_sleep_ms(100.0, Some(160.0), 1_000, 5_000);
        assert_eq!(sleep_ms, 5_000);
    }

    #[test]
    fn scheduler_sleep_wakes_quickly_for_overdue_jobs() {
        let sleep_ms = compute_scheduler_sleep_ms(100.0, Some(99.5), 1_000, 5_000);
        assert_eq!(sleep_ms, 1_000);
    }

    #[test]
    fn scheduler_sleep_uses_idle_cap_when_queue_empty() {
        let sleep_ms = compute_scheduler_sleep_ms(100.0, None, 1_000, 5_000);
        assert_eq!(sleep_ms, 5_000);
    }
}
