use std::time::Duration;

const DATABASE_RETRY_MAX_MS: u64 = 30_000;

/// Return a bounded exponential delay for a transient database failure.
///
/// `failures` is one-based: the first failure uses the configured base delay,
/// and each later failure doubles it up to 30 seconds. A lower bound prevents
/// a malformed zero interval from turning recovery into a busy loop.
pub(crate) fn database_retry_delay(base_ms: u64, failures: u32) -> Duration {
    let base_ms = base_ms.max(200);
    let multiplier = 1_u64
        .checked_shl(failures.saturating_sub(1).min(16))
        .unwrap_or(u64::MAX);
    Duration::from_millis(
        base_ms
            .saturating_mul(multiplier)
            .min(DATABASE_RETRY_MAX_MS),
    )
}

#[cfg(test)]
mod tests {
    use super::database_retry_delay;
    use std::time::Duration;

    #[test]
    fn database_retry_delay_is_bounded_and_handles_invalid_base() {
        assert_eq!(database_retry_delay(1_500, 1), Duration::from_millis(1_500));
        assert_eq!(database_retry_delay(1_500, 2), Duration::from_millis(3_000));
        assert_eq!(
            database_retry_delay(1_500, 5),
            Duration::from_millis(24_000)
        );
        assert_eq!(database_retry_delay(1_500, 6), Duration::from_secs(30));
        assert_eq!(
            database_retry_delay(1_500, u32::MAX),
            Duration::from_secs(30)
        );
        assert_eq!(database_retry_delay(0, 1), Duration::from_millis(200));
    }
}
