use crate::channels::types::ChannelMessage;
use anyhow::{anyhow, Result};
use axum::http::HeaderMap;
use futures::future::BoxFuture;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc::{self, error::TrySendError, Receiver, Sender};
use tokio::sync::Semaphore;
use tokio::time::{timeout, Duration};
use tracing::warn;

pub const CHANNEL_INBOUND_QUEUE_CAPACITY: usize = 512;
pub const CHANNEL_INBOUND_MAX_IN_FLIGHT: usize = 8;
pub const CHANNEL_INBOUND_ENQUEUE_TIMEOUT_MS: u64 = 80;

#[derive(Debug)]
pub struct ChannelInboundEnvelope {
    pub provider: String,
    pub headers: HeaderMap,
    pub messages: Vec<ChannelMessage>,
    pub raw_payload: Option<Value>,
}

pub type ChannelInboundProcessor =
    Arc<dyn Fn(ChannelInboundEnvelope) -> BoxFuture<'static, Result<()>> + Send + Sync>;

pub fn new_channel(
    capacity: usize,
) -> (
    Sender<ChannelInboundEnvelope>,
    Receiver<ChannelInboundEnvelope>,
) {
    mpsc::channel(capacity.max(1))
}

pub fn spawn_dispatcher(
    mut receiver: Receiver<ChannelInboundEnvelope>,
    max_in_flight: usize,
    processor: ChannelInboundProcessor,
) {
    let semaphore = Arc::new(Semaphore::new(max_in_flight.max(1)));
    tokio::spawn(async move {
        while let Some(envelope) = receiver.recv().await {
            let permit = match Arc::clone(&semaphore).acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => break,
            };
            let processor = Arc::clone(&processor);
            tokio::spawn(async move {
                let _permit = permit;
                if let Err(err) = processor(envelope).await {
                    warn!("channel inbound dispatch failed: {err}");
                }
            });
        }
    });
}

pub async fn enqueue_with_timeout(
    sender: &Sender<ChannelInboundEnvelope>,
    envelope: ChannelInboundEnvelope,
    timeout_ms: u64,
) -> Result<()> {
    match sender.try_send(envelope) {
        Ok(()) => Ok(()),
        Err(TrySendError::Closed(_)) => Err(anyhow!("channel inbound queue closed")),
        Err(TrySendError::Full(envelope)) => {
            let wait_ms = timeout_ms.max(1);
            match timeout(Duration::from_millis(wait_ms), sender.send(envelope)).await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(_)) => Err(anyhow!("channel inbound queue closed")),
                Err(_) => Err(anyhow!("channel inbound queue busy")),
            }
        }
    }
}
