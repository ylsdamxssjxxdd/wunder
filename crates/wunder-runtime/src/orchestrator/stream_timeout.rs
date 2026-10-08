use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use std::{future::Future, sync::Arc, time::Duration};
use tokio::time::Instant;

#[derive(Clone)]
pub(super) struct StreamActivity(Arc<Mutex<Instant>>);

impl StreamActivity {
    pub(super) fn new() -> Self {
        Self(Arc::new(Mutex::new(Instant::now())))
    }

    pub(super) fn touch(&self) {
        *self.0.lock() = Instant::now();
    }

    pub(super) async fn with_idle_timeout<T>(
        &self,
        timeout: Duration,
        future: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        if timeout.is_zero() {
            return future.await;
        }
        tokio::pin!(future);
        loop {
            let deadline = *self.0.lock() + timeout;
            tokio::select! {
                result = &mut future => return result,
                _ = tokio::time::sleep_until(deadline) => {
                    // Content, reasoning and tool argument chunks all keep the call alive.
                    if Instant::now().duration_since(*self.0.lock()) >= timeout {
                        return Err(anyhow!("LLM stream idle timeout"));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn active_stream_outlives_timeout_window() {
        let activity = StreamActivity::new();
        let result = activity
            .with_idle_timeout(Duration::from_millis(80), async {
                for _ in 0..6 {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    activity.touch();
                }
                Ok("completed")
            })
            .await;
        assert_eq!(result.unwrap(), "completed");
    }

    #[tokio::test]
    async fn silent_stream_times_out() {
        let activity = StreamActivity::new();
        let result = activity
            .with_idle_timeout(
                Duration::from_millis(20),
                std::future::pending::<Result<()>>(),
            )
            .await;
        assert_eq!(result.unwrap_err().to_string(), "LLM stream idle timeout");
    }
}
