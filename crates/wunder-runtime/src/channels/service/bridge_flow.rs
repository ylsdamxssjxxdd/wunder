use super::support::{
    extract_bridge_meta_ids, extract_session_id, message_preview_text, now_ts,
    outbound_preview_text,
};
use super::{ChannelHub, SESSION_STRATEGY_MAIN_THREAD};
use crate::channels::types::{ChannelMessage, ChannelOutboundMessage};
use crate::core::blocking;
use crate::services::bridge::{
    log_bridge_delivery, touch_bridge_route_after_outbound, BridgeRouteResolution,
};
use crate::storage::ChannelOutboxRecord;
use anyhow::Result;
use serde_json::{json, Value};
use tracing::warn;

impl ChannelHub {
    pub(super) async fn load_channel_session_bridge_metadata(
        &self,
        message: &ChannelMessage,
    ) -> Result<Option<Value>> {
        Ok(self
            .get_channel_session(
                &message.channel,
                &message.account_id,
                &message.peer.kind,
                &message.peer.id,
                message.thread.as_ref().map(|thread| thread.id.as_str()),
            )
            .await?
            .and_then(|record| record.metadata)
            .filter(|value| extract_bridge_meta_ids(Some(value)).is_some()))
    }

    pub(super) async fn load_bridge_resolution_by_ids(
        &self,
        center_id: &str,
        center_account_id: &str,
        route_id: &str,
    ) -> Result<Option<BridgeRouteResolution>> {
        let storage = self.storage.clone();
        let center_id = center_id.trim().to_string();
        let center_account_id = center_account_id.trim().to_string();
        let route_id = route_id.trim().to_string();
        blocking::run_db("channels.bridge.load_resolution", move || {
            let Some(center) = storage.get_bridge_center(&center_id)? else {
                return Ok(None);
            };
            let Some(center_account) = storage.get_bridge_center_account(&center_account_id)?
            else {
                return Ok(None);
            };
            let Some(route) = storage.get_bridge_user_route(&route_id)? else {
                return Ok(None);
            };
            let session_strategy = center_account
                .thread_strategy
                .as_deref()
                .unwrap_or(SESSION_STRATEGY_MAIN_THREAD)
                .to_string();
            Ok(Some(BridgeRouteResolution {
                center,
                center_account,
                route,
                session_strategy,
            }))
        })
        .await
    }

    pub(super) async fn load_bridge_resolution_for_outbox(
        &self,
        record: &ChannelOutboxRecord,
    ) -> Result<Option<BridgeRouteResolution>> {
        if let Some((center_id, center_account_id, route_id)) =
            extract_bridge_meta_ids(record.payload.get("meta"))
        {
            return self
                .load_bridge_resolution_by_ids(&center_id, &center_account_id, &route_id)
                .await;
        }
        let session = self
            .get_channel_session(
                &record.channel,
                &record.account_id,
                &record.peer_kind,
                &record.peer_id,
                record.thread_id.as_deref(),
            )
            .await?;
        let Some((center_id, center_account_id, route_id)) = session
            .as_ref()
            .and_then(|item| extract_bridge_meta_ids(item.metadata.as_ref()))
        else {
            return Ok(None);
        };
        self.load_bridge_resolution_by_ids(&center_id, &center_account_id, &route_id)
            .await
    }

    pub(super) async fn on_bridge_outbound_sent(
        &self,
        resolution: Option<&BridgeRouteResolution>,
        record: &ChannelOutboxRecord,
        outbound: &ChannelOutboundMessage,
    ) {
        let Some(resolution) = resolution else {
            return;
        };
        let session_id = extract_session_id(&record.payload);
        if let Err(err) = touch_bridge_route_after_outbound(
            &self.bridge_runtime,
            &resolution.route.route_id,
            session_id.as_deref(),
            None,
        )
        .await
        {
            warn!(
                "touch bridge route outbound failed: route_id={}, outbox_id={}, error={err}",
                resolution.route.route_id, record.outbox_id
            );
        }
        if let Err(err) = log_bridge_delivery(
            &self.bridge_runtime,
            resolution,
            "outbound",
            "deliver",
            "sent",
            None,
            session_id.as_deref(),
            &outbound_preview_text(outbound),
            Some(json!({
                "outbox_id": record.outbox_id,
                "channel": record.channel,
                "account_id": record.account_id,
            })),
        )
        .await
        {
            warn!(
                "log bridge outbound sent failed: route_id={}, outbox_id={}, error={err}",
                resolution.route.route_id, record.outbox_id
            );
        }
    }

    pub(super) async fn on_bridge_outbound_failed(
        &self,
        resolution: Option<&BridgeRouteResolution>,
        record: &ChannelOutboxRecord,
        outbound: &ChannelOutboundMessage,
        error_text: &str,
    ) {
        let Some(resolution) = resolution else {
            return;
        };
        let session_id = extract_session_id(&record.payload);
        if let Err(err) = touch_bridge_route_after_outbound(
            &self.bridge_runtime,
            &resolution.route.route_id,
            session_id.as_deref(),
            Some(error_text),
        )
        .await
        {
            warn!(
                "touch bridge route outbound failure failed: route_id={}, outbox_id={}, error={err}",
                resolution.route.route_id, record.outbox_id
            );
        }
        if let Err(err) = log_bridge_delivery(
            &self.bridge_runtime,
            resolution,
            "outbound",
            "deliver",
            "failed",
            None,
            session_id.as_deref(),
            &outbound_preview_text(outbound),
            Some(json!({
                "outbox_id": record.outbox_id,
                "channel": record.channel,
                "account_id": record.account_id,
                "error": error_text,
            })),
        )
        .await
        {
            warn!(
                "log bridge outbound failed failed: route_id={}, outbox_id={}, error={err}",
                resolution.route.route_id, record.outbox_id
            );
        }
    }

    pub(super) async fn persist_bridge_inbound(
        &self,
        resolution: &BridgeRouteResolution,
        message: &ChannelMessage,
        session_id: &str,
    ) {
        let mut route = resolution.route.clone();
        let now = now_ts();
        route.last_session_id = Some(session_id.to_string());
        route.last_seen_at = now;
        route.last_inbound_at = Some(now);
        route.updated_at = now;
        let storage = self.storage.clone();
        if let Err(err) = blocking::run_db("channels.bridge.persist_inbound_route", move || {
            storage.upsert_bridge_user_route(&route)
        })
        .await
        {
            warn!(
                "persist bridge inbound route failed: route_id={}, session_id={}, error={err}",
                resolution.route.route_id, session_id
            );
        }
        if let Err(err) = log_bridge_delivery(
            &self.bridge_runtime,
            resolution,
            "inbound",
            "dispatch",
            "accepted",
            message.message_id.as_deref(),
            Some(session_id),
            &message_preview_text(message),
            Some(json!({
                "channel": message.channel,
                "account_id": message.account_id,
                "peer_id": message.peer.id,
                "peer_kind": message.peer.kind,
            })),
        )
        .await
        {
            warn!(
                "log bridge inbound failed: route_id={}, session_id={}, error={err}",
                resolution.route.route_id, session_id
            );
        }
    }
}
