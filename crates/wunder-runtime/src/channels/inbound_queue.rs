use crate::channels::types::ChannelMessage;
use crate::core::bounded_queue;
use anyhow::{anyhow, Result};
use axum::http::HeaderMap;
use futures::future::BoxFuture;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender};

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
    bounded_queue::new_channel(capacity)
}

pub fn spawn_dispatcher(
    receiver: Receiver<ChannelInboundEnvelope>,
    max_in_flight: usize,
    processor: ChannelInboundProcessor,
) {
    bounded_queue::spawn_dispatcher("channel.inbound", receiver, max_in_flight, processor);
}

pub async fn enqueue_with_timeout(
    sender: &Sender<ChannelInboundEnvelope>,
    envelope: ChannelInboundEnvelope,
    timeout_ms: u64,
) -> Result<()> {
    bounded_queue::enqueue_with_timeout("channel.inbound", sender, envelope, timeout_ms)
        .await
        .map_err(|err| match err.to_string().as_str() {
            "bounded queue channel.inbound closed" => anyhow!("channel inbound queue closed"),
            "bounded queue channel.inbound busy" => anyhow!("channel inbound queue busy"),
            _ => err,
        })
}
