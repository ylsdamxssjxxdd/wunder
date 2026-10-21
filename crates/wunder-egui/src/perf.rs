//! Lightweight counters for the event-driven path.
//!
//! These are intentionally allocation-free atomics shared by the UI thread and
//! the native worker. They never carry message text, paths or identifiers.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

const NO_EVENT: u64 = u64::MAX;
const LATENCY_BOUNDS_MS: [u64; 16] = [
    1, 2, 4, 8, 16, 33, 50, 100, 200, 500, 1_000, 2_000, 5_000, 10_000, 30_000, 60_000,
];

#[derive(Clone)]
pub struct PerfCounters {
    inner: Arc<Inner>,
}

struct Inner {
    start: Option<std::time::Instant>,
    frames: AtomicU64,
    events_applied: AtomicU64,
    wakes: AtomicU64,
    dropped_events: AtomicU64,
    queued_events: AtomicU64,
    peak_queue_depth: AtomicU64,
    pending_since_ms: AtomicU64,
    last_event_to_frame_us: AtomicU64,
    event_to_frame_total_us: AtomicU64,
    event_to_frame_samples: AtomicU64,
    latency_buckets: [AtomicU64; 16],
}

impl Default for PerfCounters {
    fn default() -> Self {
        Self::new()
    }
}

impl PerfCounters {
    pub fn new() -> Self {
        let start = {
            #[cfg(not(target_arch = "wasm32"))]
            {
                Some(std::time::Instant::now())
            }
            #[cfg(target_arch = "wasm32")]
            {
                None
            }
        };
        Self {
            inner: Arc::new(Inner {
                start,
                frames: AtomicU64::new(0),
                events_applied: AtomicU64::new(0),
                wakes: AtomicU64::new(0),
                dropped_events: AtomicU64::new(0),
                queued_events: AtomicU64::new(0),
                peak_queue_depth: AtomicU64::new(0),
                pending_since_ms: AtomicU64::new(NO_EVENT),
                last_event_to_frame_us: AtomicU64::new(0),
                event_to_frame_total_us: AtomicU64::new(0),
                event_to_frame_samples: AtomicU64::new(0),
                latency_buckets: std::array::from_fn(|_| AtomicU64::new(0)),
            }),
        }
    }

    pub fn mark_frame(&self) {
        self.inner.frames.fetch_add(1, Ordering::Relaxed);
    }

    pub fn mark_wake(&self) {
        self.inner.wakes.fetch_add(1, Ordering::Relaxed);
    }

    pub fn mark_event_queued(&self) {
        let depth = self.inner.queued_events.fetch_add(1, Ordering::Relaxed) + 1;
        self.inner
            .peak_queue_depth
            .fetch_max(depth, Ordering::Relaxed);
        // The first event in a batch owns the timestamp; later tokens in the
        // same frame should not pay for another clock read.
        if self.inner.pending_since_ms.load(Ordering::Relaxed) != NO_EVENT {
            return;
        }
        let Some(now) = self.now_ms() else {
            return;
        };
        let _ = self.inner.pending_since_ms.compare_exchange(
            NO_EVENT,
            now,
            Ordering::AcqRel,
            Ordering::Relaxed,
        );
    }

    pub fn mark_events_drained(&self, count: usize) {
        if count == 0 {
            return;
        }
        let _ =
            self.inner
                .queued_events
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |depth| {
                    Some(depth.saturating_sub(count as u64))
                });
    }

    pub fn mark_events_applied(&self, count: usize) {
        if count == 0 {
            return;
        }
        self.inner
            .events_applied
            .fetch_add(count as u64, Ordering::Relaxed);
        let Some(now) = self.now_ms() else {
            return;
        };
        let since = self.inner.pending_since_ms.swap(NO_EVENT, Ordering::AcqRel);
        if since != NO_EVENT && now >= since {
            let micros = (now - since) * 1_000;
            self.inner
                .last_event_to_frame_us
                .store(micros, Ordering::Relaxed);
            self.inner
                .event_to_frame_total_us
                .fetch_add(micros, Ordering::Relaxed);
            self.inner
                .event_to_frame_samples
                .fetch_add(1, Ordering::Relaxed);
            let bucket = latency_bucket(now - since);
            self.inner.latency_buckets[bucket].fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn record_dropped_events(&self, count: usize) {
        self.inner
            .dropped_events
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> PerfSnapshot {
        let samples = self.inner.event_to_frame_samples.load(Ordering::Relaxed);
        let total = self.inner.event_to_frame_total_us.load(Ordering::Relaxed);
        PerfSnapshot {
            frames: self.inner.frames.load(Ordering::Relaxed),
            events_applied: self.inner.events_applied.load(Ordering::Relaxed),
            wakes: self.inner.wakes.load(Ordering::Relaxed),
            dropped_events: self.inner.dropped_events.load(Ordering::Relaxed),
            queue_depth: self.inner.queued_events.load(Ordering::Relaxed),
            peak_queue_depth: self.inner.peak_queue_depth.load(Ordering::Relaxed),
            event_to_frame_samples: samples,
            last_event_to_frame_ms: self.inner.last_event_to_frame_us.load(Ordering::Relaxed)
                as f64
                / 1_000.0,
            average_event_to_frame_ms: if samples == 0 {
                0.0
            } else {
                total as f64 / samples as f64 / 1_000.0
            },
            event_to_frame_p95_ms: self.latency_percentile(95),
        }
    }

    fn latency_percentile(&self, percentile: u64) -> f64 {
        let samples: u64 = self
            .inner
            .latency_buckets
            .iter()
            .map(|bucket| bucket.load(Ordering::Relaxed))
            .sum();
        if samples == 0 {
            return 0.0;
        }
        // Nearest-rank percentile over the fixed buckets.
        let target = (samples * percentile).div_ceil(100);
        let mut seen = 0;
        for (index, bound) in LATENCY_BOUNDS_MS.iter().enumerate() {
            seen += self.inner.latency_buckets[index].load(Ordering::Relaxed);
            if seen >= target {
                return *bound as f64;
            }
        }
        LATENCY_BOUNDS_MS[LATENCY_BOUNDS_MS.len() - 1] as f64
    }

    fn now_ms(&self) -> Option<u64> {
        self.inner
            .start
            .map(|start| start.elapsed().as_millis() as u64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PerfSnapshot {
    pub frames: u64,
    pub events_applied: u64,
    pub wakes: u64,
    pub dropped_events: u64,
    pub queue_depth: u64,
    pub peak_queue_depth: u64,
    pub event_to_frame_samples: u64,
    pub last_event_to_frame_ms: f64,
    pub average_event_to_frame_ms: f64,
    pub event_to_frame_p95_ms: f64,
}

fn latency_bucket(millis: u64) -> usize {
    LATENCY_BOUNDS_MS
        .iter()
        .position(|bound| millis <= *bound)
        .unwrap_or(LATENCY_BOUNDS_MS.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_are_shared_and_monotonic() {
        let perf = PerfCounters::new();
        let worker = perf.clone();
        worker.mark_wake();
        worker.mark_wake();
        perf.mark_frame();
        let snapshot = perf.snapshot();
        assert_eq!(snapshot.wakes, 2);
        assert_eq!(snapshot.frames, 1);
    }

    #[test]
    fn one_pending_timestamp_covers_a_batch() {
        let perf = PerfCounters::new();
        perf.mark_event_queued();
        perf.mark_event_queued();
        perf.mark_events_applied(3);
        let snapshot = perf.snapshot();
        assert_eq!(snapshot.events_applied, 3);
        assert_eq!(snapshot.event_to_frame_samples, 1);
        assert!(snapshot.last_event_to_frame_ms >= 0.0);
        assert!(snapshot.event_to_frame_p95_ms >= 0.0);
    }

    #[test]
    fn queue_depth_tracks_sends_and_drains() {
        let perf = PerfCounters::new();
        for _ in 0..3 {
            perf.mark_event_queued();
        }
        assert_eq!(perf.snapshot().queue_depth, 3);
        assert_eq!(perf.snapshot().peak_queue_depth, 3);
        perf.mark_events_drained(2);
        assert_eq!(perf.snapshot().queue_depth, 1);
        perf.mark_events_drained(5);
        assert_eq!(perf.snapshot().queue_depth, 0);
        assert_eq!(perf.snapshot().peak_queue_depth, 3);
    }

    #[test]
    fn latency_buckets_are_monotonic() {
        assert_eq!(latency_bucket(0), 0);
        assert_eq!(latency_bucket(1), 0);
        assert_eq!(latency_bucket(2), 1);
        assert_eq!(latency_bucket(17), 5);
        assert_eq!(latency_bucket(u64::MAX), LATENCY_BOUNDS_MS.len() - 1);
    }
}
