use std::time::Duration;
use std::time::Instant;

use tokio::sync::mpsc;

const MIN_FRAME_INTERVAL: Duration = Duration::from_nanos(8_333_334);

#[derive(Clone, Debug)]
pub(crate) struct FrameRequester {
    request_tx: mpsc::UnboundedSender<Instant>,
}

#[derive(Debug)]
pub(crate) struct FrameNotifications {
    draw_rx: mpsc::UnboundedReceiver<()>,
}

#[derive(Debug, Default)]
struct FrameRateLimiter {
    last_emitted_at: Option<Instant>,
}

impl FrameRateLimiter {
    fn clamp_deadline(&self, requested: Instant) -> Instant {
        let Some(last_emitted_at) = self.last_emitted_at else {
            return requested;
        };
        let min_allowed = last_emitted_at
            .checked_add(MIN_FRAME_INTERVAL)
            .unwrap_or(last_emitted_at);
        requested.max(min_allowed)
    }

    fn mark_emitted(&mut self, emitted_at: Instant) {
        self.last_emitted_at = Some(emitted_at);
    }
}

#[derive(Debug)]
struct FrameScheduler {
    request_rx: mpsc::UnboundedReceiver<Instant>,
    draw_tx: mpsc::UnboundedSender<()>,
    limiter: FrameRateLimiter,
}

impl FrameScheduler {
    fn new(
        request_rx: mpsc::UnboundedReceiver<Instant>,
        draw_tx: mpsc::UnboundedSender<()>,
    ) -> Self {
        Self {
            request_rx,
            draw_tx,
            limiter: FrameRateLimiter::default(),
        }
    }

    async fn run(mut self) {
        let mut pending_deadline: Option<Instant> = None;

        loop {
            match pending_deadline {
                Some(deadline) => {
                    let sleep_until = tokio::time::Instant::from_std(deadline);
                    tokio::select! {
                        maybe_requested = self.request_rx.recv() => {
                            let Some(requested) = maybe_requested else {
                                break;
                            };
                            let clamped = self.limiter.clamp_deadline(requested);
                            pending_deadline = Some(deadline.min(clamped));
                        }
                        _ = tokio::time::sleep_until(sleep_until) => {
                            self.limiter.mark_emitted(Instant::now());
                            let _ = self.draw_tx.send(());
                            pending_deadline = None;
                        }
                    }
                }
                None => {
                    let Some(requested) = self.request_rx.recv().await else {
                        break;
                    };
                    pending_deadline = Some(self.limiter.clamp_deadline(requested));
                }
            }
        }
    }
}

pub(crate) fn spawn_frame_scheduler() -> (FrameRequester, FrameNotifications) {
    let (request_tx, request_rx) = mpsc::unbounded_channel();
    let (draw_tx, draw_rx) = mpsc::unbounded_channel();
    let scheduler = FrameScheduler::new(request_rx, draw_tx);
    tokio::spawn(scheduler.run());
    (
        FrameRequester { request_tx },
        FrameNotifications { draw_rx },
    )
}

impl FrameRequester {
    pub(crate) fn schedule_frame(&self) {
        let _ = self.request_tx.send(Instant::now());
    }

    pub(crate) fn schedule_frame_in(&self, delay: Duration) {
        let _ = self.request_tx.send(Instant::now() + delay);
    }
}

impl FrameNotifications {
    pub(crate) async fn recv(&mut self) -> Option<()> {
        self.draw_rx.recv().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_does_not_clamp_first_request() {
        let t0 = Instant::now();
        let limiter = FrameRateLimiter::default();
        assert_eq!(limiter.clamp_deadline(t0), t0);
    }

    #[test]
    fn limiter_clamps_requests_inside_min_interval() {
        let t0 = Instant::now();
        let mut limiter = FrameRateLimiter::default();
        limiter.mark_emitted(t0);
        let too_soon = t0 + Duration::from_millis(1);
        assert_eq!(limiter.clamp_deadline(too_soon), t0 + MIN_FRAME_INTERVAL);
    }
}
