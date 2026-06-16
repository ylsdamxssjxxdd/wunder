use anyhow::{anyhow, Result};
use futures::future::BoxFuture;
use std::sync::Arc;
use tokio::sync::mpsc::{self, error::TrySendError, Receiver, Sender};
use tokio::sync::Semaphore;
use tokio::time::{timeout, Duration};
use tracing::warn;

use super::long_task;
use super::runtime_metrics;

pub type QueueProcessor<T> = Arc<dyn Fn(T) -> BoxFuture<'static, Result<()>> + Send + Sync>;

pub fn new_channel<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    mpsc::channel(capacity.max(1))
}

pub fn spawn_dispatcher<T>(
    queue_name: &'static str,
    mut receiver: Receiver<T>,
    max_in_flight: usize,
    processor: QueueProcessor<T>,
) where
    T: Send + 'static,
{
    let semaphore = Arc::new(Semaphore::new(max_in_flight.max(1)));
    runtime_metrics::record_queue_config(queue_name, 0, max_in_flight);
    long_task::spawn(queue_name, async move {
        while let Some(item) = receiver.recv().await {
            let permit = match Arc::clone(&semaphore).acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => break,
            };
            let processor = Arc::clone(&processor);
            long_task::spawn(queue_name, async move {
                let _permit = permit;
                runtime_metrics::record_queue_item_started(queue_name);
                let result = processor(item).await;
                runtime_metrics::record_queue_item_finished(queue_name, result.is_ok());
                if let Err(err) = result {
                    warn!(queue.name = queue_name, "bounded queue item failed: {err}");
                }
            });
        }
    });
}

pub async fn enqueue_with_timeout<T>(
    queue_name: &'static str,
    sender: &Sender<T>,
    item: T,
    timeout_ms: u64,
) -> Result<()> {
    match sender.try_send(item) {
        Ok(()) => {
            runtime_metrics::record_queue_enqueued(queue_name, false, 0);
            Ok(())
        }
        Err(TrySendError::Closed(_)) => {
            runtime_metrics::record_queue_closed(queue_name);
            Err(anyhow!("bounded queue {queue_name} closed"))
        }
        Err(TrySendError::Full(item)) => {
            let wait_ms = timeout_ms.max(1);
            let started = std::time::Instant::now();
            match timeout(Duration::from_millis(wait_ms), sender.send(item)).await {
                Ok(Ok(())) => {
                    runtime_metrics::record_queue_enqueued(
                        queue_name,
                        true,
                        runtime_metrics::elapsed_ms_u64(started),
                    );
                    Ok(())
                }
                Ok(Err(_)) => {
                    runtime_metrics::record_queue_closed(queue_name);
                    Err(anyhow!("bounded queue {queue_name} closed"))
                }
                Err(_) => {
                    runtime_metrics::record_queue_busy(queue_name);
                    Err(anyhow!("bounded queue {queue_name} busy"))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{enqueue_with_timeout, new_channel};

    #[tokio::test]
    async fn bounded_enqueue_reports_busy_queue() {
        let (sender, _receiver) = new_channel(1);
        enqueue_with_timeout("test", &sender, 1, 1)
            .await
            .expect("first enqueue should fit");
        let result = enqueue_with_timeout("test", &sender, 2, 1).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("busy"));
    }
}
