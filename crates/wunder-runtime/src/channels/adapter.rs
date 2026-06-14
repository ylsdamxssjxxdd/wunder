use crate::channels::types::{ChannelAccountConfig, ChannelMessage, ChannelOutboundMessage};
use crate::storage::ChannelAccountRecord;
use anyhow::Result;
use async_trait::async_trait;
use axum::http::HeaderMap;
use reqwest::Client;
use serde_json::Value;

pub struct InboundVerifyContext<'a> {
    pub provider: &'a str,
    pub headers: &'a HeaderMap,
    pub payload: &'a Value,
}

pub struct InboundParseContext<'a> {
    pub provider: &'a str,
    pub headers: &'a HeaderMap,
    pub account_override: Option<&'a str>,
    pub payload: &'a Value,
}

pub struct OutboundContext<'a> {
    pub http: &'a Client,
    pub account: &'a ChannelAccountRecord,
    pub account_config: &'a ChannelAccountConfig,
    pub outbound: &'a ChannelOutboundMessage,
}

#[async_trait]
pub trait ChannelAdapter: Send + Sync {
    fn channel(&self) -> &'static str;

    async fn verify_inbound(&self, _context: InboundVerifyContext<'_>) -> Result<()> {
        Ok(())
    }

    async fn parse_inbound(
        &self,
        _context: InboundParseContext<'_>,
    ) -> Result<Option<Vec<ChannelMessage>>> {
        Ok(None)
    }

    async fn send_outbound(&self, context: OutboundContext<'_>) -> Result<()>;

    async fn health_check(
        &self,
        _http: &Client,
        _account_config: &ChannelAccountConfig,
    ) -> Result<Value> {
        Ok(serde_json::json!({
            "status": "unknown",
        }))
    }
}
