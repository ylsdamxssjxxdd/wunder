const PRE_SAMPLING_OCCUPANCY_RATIO: f64 = 0.92;
const PRE_SAMPLING_MIN_HEADROOM_TOKENS: i64 = 128;
const PRE_SAMPLING_MAX_HEADROOM_TOKENS: i64 = 1024;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct CompactionDecision {
    pub(super) by_history: bool,
    pub(super) by_overflow: bool,
    pub(super) by_presampling: bool,
    pub(super) presampling_limit: i64,
}

impl CompactionDecision {
    pub(super) fn should_compact(self) -> bool {
        self.by_history || self.by_overflow || self.by_presampling
    }

    pub(super) fn trigger(self) -> &'static str {
        if self.by_history {
            "history"
        } else if self.by_overflow {
            "overflow"
        } else if self.by_presampling {
            "pre_sampling"
        } else {
            "none"
        }
    }
}

pub(super) fn resolve_pre_sampling_trigger_limit(limit: i64) -> i64 {
    let limit = limit.max(1);
    let max_headroom = (limit / 2).max(1);
    let occupancy_trigger = ((limit as f64) * PRE_SAMPLING_OCCUPANCY_RATIO).round() as i64;
    let adaptive_headroom = (limit / 16)
        .clamp(
            PRE_SAMPLING_MIN_HEADROOM_TOKENS,
            PRE_SAMPLING_MAX_HEADROOM_TOKENS,
        )
        .min(max_headroom);
    let headroom_trigger = limit.saturating_sub(adaptive_headroom);
    occupancy_trigger.max(headroom_trigger).clamp(1, limit)
}

pub(super) fn should_compact_by_context(
    projected_request_tokens: i64,
    limit: i64,
    history_threshold: Option<i64>,
) -> CompactionDecision {
    let limit = limit.max(1);
    let presampling_limit = resolve_pre_sampling_trigger_limit(limit);
    let by_history = history_threshold
        .map(|threshold| projected_request_tokens >= threshold)
        .unwrap_or(false);
    let by_overflow = projected_request_tokens >= limit;
    // Trigger compaction before hard overflow so we keep request headroom stable.
    let by_presampling = !by_overflow && projected_request_tokens >= presampling_limit;
    CompactionDecision {
        by_history,
        by_overflow,
        by_presampling,
        presampling_limit,
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_pre_sampling_trigger_limit, should_compact_by_context};

    #[test]
    fn pre_sampling_trigger_stays_within_limit() {
        let limit = 8192;
        let trigger = resolve_pre_sampling_trigger_limit(limit);
        assert!(trigger > 0);
        assert!(trigger < limit);
    }

    #[test]
    fn near_limit_without_overflow_uses_pre_sampling_trigger() {
        let decision = should_compact_by_context(7600, 8000, None);
        assert!(decision.by_presampling);
        assert!(decision.should_compact());
        assert_eq!(decision.trigger(), "pre_sampling");
    }

    #[test]
    fn overflow_still_reports_overflow_trigger() {
        let decision = should_compact_by_context(9000, 8000, None);
        assert!(decision.by_overflow);
        assert!(decision.should_compact());
        assert_eq!(decision.trigger(), "overflow");
    }
}
