use super::{ChannelHub, ChannelSessionInfo};
use crate::channels::adapter::OutboundContext;
use crate::channels::binding::BindingResolution;
use crate::channels::feishu_files;
use crate::channels::outbound_attachments::{
    merge_attachments_with_text_links, OutboundLinkExtractionMode,
};
use crate::channels::outbox::{compute_retry_at, resolve_outbox_config};
use crate::channels::types::{ChannelAccountConfig, ChannelMessage, ChannelOutboundMessage};
use crate::channels::weixin;
use crate::core::blocking;
use crate::storage::{ChannelOutboxRecord, UpdateChannelOutboxStatusParams};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, warn};
use uuid::Uuid;

use super::support::{
    append_weixin_context_token_from_message, build_outbound_headers, channels_runtime_enabled,
    extract_session_id, merge_object_value_into, now_ts,
    resolve_weixin_workspace_public_source_to_local, truncate_text,
};

impl ChannelHub {
    pub(super) async fn enqueue_outbox(&self, outbound: &ChannelOutboundMessage) -> Result<String> {
        let outbox_id = format!("outbox_{}", Uuid::new_v4().simple());
        let now = now_ts();
        let record = ChannelOutboxRecord {
            outbox_id: outbox_id.clone(),
            channel: outbound.channel.clone(),
            account_id: outbound.account_id.clone(),
            peer_kind: outbound.peer.kind.clone(),
            peer_id: outbound.peer.id.clone(),
            thread_id: outbound.thread.as_ref().map(|thread| thread.id.clone()),
            payload: json!(outbound),
            status: "pending".to_string(),
            retry_count: 0,
            retry_at: now,
            last_error: None,
            created_at: now,
            updated_at: now,
            delivered_at: None,
        };
        self.insert_outbox(&record).await?;
        Ok(outbox_id)
    }

    pub(super) async fn try_deliver_outbox_if_worker_disabled(&self, outbox_id: &str) {
        let config = self.config_store.get().await;
        if resolve_outbox_config(config.channels.outbox.clone()).worker_enabled {
            return;
        }
        let outbox_id = outbox_id.trim();
        if outbox_id.is_empty() {
            return;
        }
        let record = match self.get_outbox(outbox_id).await {
            Ok(value) => value,
            Err(err) => {
                warn!("load outbox failed: outbox_id={outbox_id}, error={err}");
                return;
            }
        };
        if let Some(record) = record {
            if let Err(err) = self.deliver_outbox_record(&record).await {
                warn!(
                    "deliver outbox failed (worker disabled): outbox_id={}, channel={}, account_id={}, error={err}",
                    record.outbox_id, record.channel, record.account_id
                );
            }
        }
    }

    pub(super) async fn enqueue_channel_text_reply(
        &self,
        message: &ChannelMessage,
        session_info: &ChannelSessionInfo,
        resolved_binding: Option<&BindingResolution>,
        text: &str,
        extra_meta: Option<Value>,
    ) -> Result<String> {
        let mut meta = json!({
            "session_id": session_info.session_id,
            "binding_id": resolved_binding.and_then(|item| item.binding_id.clone()),
            "message_id": message.message_id,
        });
        if let Some(bridge_meta) = self.load_channel_session_bridge_metadata(message).await? {
            merge_object_value_into(&mut meta, bridge_meta);
        }
        if let Some(extra) = extra_meta {
            merge_object_value_into(&mut meta, extra);
        }
        append_weixin_context_token_from_message(&mut meta, message);
        let outbound = ChannelOutboundMessage {
            channel: message.channel.clone(),
            account_id: message.account_id.clone(),
            peer: message.peer.clone(),
            thread: message.thread.clone(),
            text: Some(text.to_string()),
            attachments: Vec::new(),
            meta: Some(meta),
        };
        let outbox_id = self.enqueue_outbox(&outbound).await?;
        self.try_deliver_outbox_if_worker_disabled(&outbox_id).await;
        Ok(outbox_id)
    }

    async fn deliver_outbox_record(&self, record: &ChannelOutboxRecord) -> Result<()> {
        let config = self.config_store.get().await;
        let account = self
            .load_channel_account(&record.channel, &record.account_id, &config)
            .await?;
        let account_cfg = ChannelAccountConfig::from_value(&account.config);
        let outbound: ChannelOutboundMessage = serde_json::from_value(record.payload.clone())
            .map_err(|err| anyhow!("invalid outbound payload: {err}"))?;
        let mut outbound = outbound;
        let bridge_resolution = match self.load_bridge_resolution_for_outbox(record).await {
            Ok(value) => value,
            Err(err) => {
                warn!(
                    "load bridge outbox resolution failed: outbox_id={}, channel={}, account_id={}, error={err}",
                    record.outbox_id, record.channel, record.account_id
                );
                None
            }
        };
        if record
            .channel
            .trim()
            .eq_ignore_ascii_case(weixin::WEIXIN_CHANNEL)
        {
            let rewritten = self.rewrite_weixin_workspace_paths_to_local(&mut outbound);
            if rewritten > 0 {
                debug!(
                    "rewrite weixin workspace paths to local: outbox_id={}, rewritten={}",
                    record.outbox_id, rewritten
                );
            }
        } else if let Err(err) = feishu_files::append_temp_dir_links_for_outbound(
            &self.workspace,
            &self.user_store,
            &config,
            &record.channel,
            &mut outbound,
        )
        .await
        {
            warn!(
                "rewrite outbound workspace links failed: channel={}, account_id={}, outbox_id={}, error={err}",
                record.channel, record.account_id, record.outbox_id
            );
        }
        if let Some(adapter) = self.adapter_registry.get(&record.channel) {
            let context = OutboundContext {
                http: &self.http,
                account: &account,
                account_config: &account_cfg,
                outbound: &outbound,
            };
            if let Err(err) = adapter.send_outbound(context).await {
                self.on_bridge_outbound_failed(
                    bridge_resolution.as_ref(),
                    record,
                    &outbound,
                    &err.to_string(),
                )
                .await;
                return Err(err);
            }
            self.update_outbox_status(record, "sent", None).await?;
            if let Some(session_id) = extract_session_id(&record.payload) {
                self.monitor.record_event(
                    &session_id,
                    "channel_outbound",
                    &json!({
                        "channel": record.channel,
                        "account_id": record.account_id,
                        "outbox_id": record.outbox_id,
                    }),
                );
            }
            self.on_bridge_outbound_sent(bridge_resolution.as_ref(), record, &outbound)
                .await;
            return Ok(());
        }
        let outbound_url = account_cfg
            .outbound_url
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let Some(outbound_url) = outbound_url else {
            self.update_outbox_status(record, "sent", None).await?;
            self.on_bridge_outbound_sent(bridge_resolution.as_ref(), record, &outbound)
                .await;
            return Ok(());
        };
        let payload = record.payload.clone();
        let headers = build_outbound_headers(&account_cfg)?;
        let timeout = account_cfg.timeout_s.unwrap_or(10);
        let response = self
            .http
            .post(outbound_url)
            .headers(headers)
            .timeout(Duration::from_secs(timeout.max(1)))
            .json(&payload)
            .send()
            .await?;
        let status = response.status();
        if status.is_success() {
            self.update_outbox_status(record, "sent", None).await?;
            if let Some(session_id) = extract_session_id(&payload) {
                self.monitor.record_event(
                    &session_id,
                    "channel_outbound",
                    &json!({
                        "channel": record.channel,
                        "account_id": record.account_id,
                        "outbox_id": record.outbox_id,
                    }),
                );
            }
            self.on_bridge_outbound_sent(bridge_resolution.as_ref(), record, &outbound)
                .await;
            Ok(())
        } else {
            let body = match response.text().await {
                Ok(value) => truncate_text(&value, 2048),
                Err(err) => format!("(read body failed: {err})"),
            };
            let error = anyhow!("outbound delivery failed: {status} {body}");
            self.on_bridge_outbound_failed(
                bridge_resolution.as_ref(),
                record,
                &outbound,
                &error.to_string(),
            )
            .await;
            Err(error)
        }
    }

    fn rewrite_weixin_workspace_paths_to_local(
        &self,
        outbound: &mut ChannelOutboundMessage,
    ) -> usize {
        let mut rewritten_count = 0usize;
        let mut replacements: HashMap<String, String> = HashMap::new();
        let mut merged = merge_attachments_with_text_links(
            &outbound.attachments,
            outbound.text.as_deref(),
            OutboundLinkExtractionMode::WorkspaceResource,
        );
        for attachment in &mut merged {
            let source = attachment.url.trim();
            if source.is_empty() {
                continue;
            }
            let replacement = replacements
                .entry(source.to_string())
                .or_insert_with(|| {
                    resolve_weixin_workspace_public_source_to_local(&self.workspace, source)
                        .unwrap_or_else(|| source.to_string())
                })
                .clone();
            if replacement != source {
                attachment.url = replacement;
                rewritten_count = rewritten_count.saturating_add(1);
            }
        }
        if !merged.is_empty() {
            outbound.attachments = merged;
        }
        if let Some(text) = outbound.text.as_ref() {
            let mut rewritten = text.to_string();
            for (source, target) in &replacements {
                if source == target || !rewritten.contains(source) {
                    continue;
                }
                rewritten = rewritten.replace(source, target);
            }
            if rewritten != *text {
                outbound.text = Some(rewritten);
                rewritten_count = rewritten_count.saturating_add(1);
            }
        }
        rewritten_count
    }

    async fn update_outbox_status(
        &self,
        record: &ChannelOutboxRecord,
        status: &str,
        error: Option<String>,
    ) -> Result<()> {
        let now = now_ts();
        let mut retry_count = record.retry_count;
        let mut retry_at = record.retry_at;
        let last_error = error;
        let mut delivered_at = record.delivered_at;
        if status == "sent" {
            delivered_at = Some(now);
        } else if status == "retry" {
            retry_count += 1;
            let cfg = resolve_outbox_config(self.config_store.get().await.channels.outbox.clone());
            retry_at = compute_retry_at(now, retry_count, &cfg);
        }
        self.set_outbox_status(UpdateChannelOutboxStatusParams {
            outbox_id: &record.outbox_id,
            status,
            retry_count,
            retry_at,
            last_error: last_error.as_deref(),
            delivered_at,
            updated_at: now,
        })
        .await?;
        Ok(())
    }

    pub(super) async fn outbox_loop(&self) {
        loop {
            let config = self.config_store.get().await;
            if !channels_runtime_enabled(&config) {
                sleep(Duration::from_millis(1000)).await;
                continue;
            }
            let outbox_cfg = resolve_outbox_config(config.channels.outbox.clone());
            if !outbox_cfg.worker_enabled {
                sleep(Duration::from_millis(outbox_cfg.poll_interval_ms)).await;
                continue;
            }
            let pending = self.list_pending_outbox(outbox_cfg.max_batch as i64).await;
            match pending {
                Ok(items) => {
                    if items.is_empty() {
                        sleep(Duration::from_millis(outbox_cfg.poll_interval_ms)).await;
                        continue;
                    }
                    for record in items {
                        let outcome = self.deliver_outbox_record(&record).await;
                        if let Err(err) = outcome {
                            let error_text = err.to_string();
                            warn!(
                                "deliver outbox failed: outbox_id={}, channel={}, account_id={}, retry_count={}, error={err}",
                                record.outbox_id,
                                record.channel,
                                record.account_id,
                                record.retry_count
                            );
                            self.record_runtime_error(
                                &record.channel,
                                Some(&record.account_id),
                                "outbound_delivery_failed",
                                format!(
                                    "deliver outbox failed: outbox_id={}, retry_count={}, error={}",
                                    record.outbox_id, record.retry_count, error_text
                                ),
                            );
                            let mut status = "retry";
                            let error_text = Some(error_text);
                            if record.retry_count as u32 >= outbox_cfg.max_retries {
                                status = "failed";
                            }
                            if let Err(update_err) = self
                                .update_outbox_status(&record, status, error_text.clone())
                                .await
                            {
                                error!("outbox update failed: {update_err}");
                            }
                            if let Some(session_id) = extract_session_id(&record.payload) {
                                self.monitor.record_event(
                                    &session_id,
                                    "channel_outbound_error",
                                    &json!({
                                        "channel": record.channel,
                                        "account_id": record.account_id,
                                        "outbox_id": record.outbox_id,
                                        "error": error_text.as_deref().unwrap_or_default(),
                                    }),
                                );
                            }
                        }
                    }
                }
                Err(err) => {
                    error!("load pending outbox failed: {err}");
                    sleep(Duration::from_millis(outbox_cfg.poll_interval_ms)).await;
                }
            }
        }
    }

    pub(super) async fn runtime_bootstrap_log_once(&self) {
        let storage = self.storage.clone();
        let records = blocking::run_db("channels.outbox.bootstrap_accounts", move || {
            storage.list_channel_accounts(None, Some("active"))
        })
        .await;
        let records = match records {
            Ok(items) => items,
            Err(err) => {
                debug!("runtime bootstrap skipped: load active channel accounts failed: {err}");
                return;
            }
        };
        for record in records {
            let channel = record.channel.trim().to_ascii_lowercase();
            let account_id = record.account_id.trim().to_string();
            if channel.is_empty() || account_id.is_empty() {
                continue;
            }
            self.record_runtime_info(
                &channel,
                Some(&account_id),
                "runtime_bootstrap",
                format!(
                    "channel runtime collector ready: channel={channel}, account_id={account_id}"
                ),
            );
        }
    }
}
