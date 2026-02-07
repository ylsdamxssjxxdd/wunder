use crate::channels::binding::{resolve_binding, BindingResolution};
use crate::channels::feishu;
use crate::channels::media::{MediaProcessingResult, MediaProcessor};
use crate::channels::outbox::{compute_retry_at, resolve_outbox_config};
use crate::channels::qqbot;
use crate::channels::rate_limit::{ChannelRateLimiter, RateLimitConfig};
use crate::channels::types::{ChannelAccountConfig, ChannelMessage, ChannelOutboundMessage};
use crate::channels::whatsapp_cloud;
use crate::config::{ChannelRateLimitConfig, Config};
use crate::monitor::MonitorState;
use crate::orchestrator::Orchestrator;
use crate::schemas::WunderRequest;
use crate::services::agent_runtime::AgentRuntime;
use crate::storage::{
    ChannelAccountRecord, ChannelBindingRecord, ChannelMessageRecord, ChannelOutboxRecord,
    ChannelSessionRecord, ChatSessionRecord, StorageBackend, UpdateChannelOutboxStatusParams,
    UserAgentRecord,
};
use crate::user_store::UserStore;
use anyhow::{anyhow, Result};
use axum::http::HeaderMap;
use reqwest::header::{HeaderMap as ReqHeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{error, warn};
use uuid::Uuid;

const TOOL_OVERRIDE_NONE: &str = "__no_tools__";
const DEFAULT_SESSION_TITLE: &str = "Channel Session";
const SESSION_STRATEGY_MAIN_THREAD: &str = "main_thread";
const SESSION_STRATEGY_PER_PEER: &str = "per_peer";
const SESSION_STRATEGY_HYBRID: &str = "hybrid";

fn channels_runtime_enabled(config: &Config) -> bool {
    config.channels.enabled || config.gateway.enabled
}

#[derive(Debug, Clone, Copy)]
enum ChannelSessionStrategy {
    MainThread,
    PerPeer,
    Hybrid,
}

impl ChannelSessionStrategy {
    fn from_config(config: &Config) -> Self {
        match config
            .channels
            .session_strategy
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            SESSION_STRATEGY_PER_PEER => Self::PerPeer,
            SESSION_STRATEGY_HYBRID => Self::Hybrid,
            SESSION_STRATEGY_MAIN_THREAD => Self::MainThread,
            _ => Self::MainThread,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChannelInboundResult {
    pub session_id: String,
    pub outbox_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChannelHandleResult {
    pub accepted: usize,
    pub session_ids: Vec<String>,
    pub outbox_ids: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Clone)]
pub struct ChannelHub {
    config_store: crate::config_store::ConfigStore,
    storage: Arc<dyn StorageBackend>,
    orchestrator: Arc<Orchestrator>,
    agent_runtime: Arc<AgentRuntime>,
    user_store: Arc<UserStore>,
    monitor: Arc<MonitorState>,
    rate_limiter: ChannelRateLimiter,
    http: reqwest::Client,
}

impl ChannelHub {
    pub fn new(
        config_store: crate::config_store::ConfigStore,
        storage: Arc<dyn StorageBackend>,
        orchestrator: Arc<Orchestrator>,
        agent_runtime: Arc<AgentRuntime>,
        user_store: Arc<UserStore>,
        monitor: Arc<MonitorState>,
    ) -> Self {
        let hub = Self {
            config_store,
            storage,
            orchestrator,
            agent_runtime,
            user_store,
            monitor,
            rate_limiter: ChannelRateLimiter::new(),
            http: reqwest::Client::new(),
        };
        let worker = hub.clone();
        tokio::spawn(async move {
            worker.outbox_loop().await;
        });
        hub
    }

    pub async fn handle_inbound(
        &self,
        provider: &str,
        headers: &HeaderMap,
        messages: Vec<ChannelMessage>,
        raw_payload: Option<Value>,
    ) -> Result<ChannelHandleResult> {
        let mut result = ChannelHandleResult {
            accepted: 0,
            session_ids: Vec::new(),
            outbox_ids: Vec::new(),
            errors: Vec::new(),
        };
        for message in messages {
            match self
                .handle_single(provider, headers, message, raw_payload.clone())
                .await
            {
                Ok(item) => {
                    result.accepted += 1;
                    result.session_ids.push(item.session_id);
                    if let Some(outbox_id) = item.outbox_id {
                        result.outbox_ids.push(outbox_id);
                    }
                }
                Err(err) => {
                    result.errors.push(err.to_string());
                }
            }
        }
        Ok(result)
    }

    async fn handle_single(
        &self,
        provider: &str,
        headers: &HeaderMap,
        mut message: ChannelMessage,
        raw_payload: Option<Value>,
    ) -> Result<ChannelInboundResult> {
        let config = self.config_store.get().await;
        if !channels_runtime_enabled(&config) {
            return Err(anyhow!("channels disabled"));
        }
        normalize_message(provider, &mut message)?;
        let limiter_key = format!("{}:{}", message.channel, message.account_id);
        let rate_cfg = resolve_rate_limit(&config.channels.rate_limit, &message.channel);
        let _rate_guard = self
            .rate_limiter
            .acquire(&limiter_key, rate_cfg)
            .ok_or_else(|| anyhow!("channel rate limited"))?;

        let account = self
            .load_channel_account(&message.channel, &message.account_id, &config)
            .await?;
        let account_cfg = ChannelAccountConfig::from_value(&account.config);
        validate_inbound_account(headers, &account, &account_cfg)?;
        enforce_allowlist(&message, &account_cfg)?;

        let bindings = self
            .list_channel_bindings(Some(&message.channel))
            .await
            .unwrap_or_default();
        let resolved_binding = resolve_binding(&bindings, &message);
        let resolved_agent_id = resolved_binding
            .as_ref()
            .and_then(|binding| binding.agent_id.clone())
            .or_else(|| account_cfg.agent_id.clone())
            .or_else(|| config.channels.default_agent_id.clone());

        let agent_record = match resolved_agent_id.as_ref() {
            Some(agent_id) => self.get_agent(agent_id).await,
            None => Ok(None),
        }?;

        let tool_names = resolve_tool_names(
            resolved_binding.as_ref(),
            &account_cfg,
            agent_record.as_ref(),
            &config,
        );

        let session_strategy = ChannelSessionStrategy::from_config(&config);
        let bound_user_id = self
            .get_channel_user_binding(
                &message.channel,
                &message.account_id,
                &message.peer.kind,
                &message.peer.id,
            )
            .await?
            .map(|record| record.user_id);
        let session_info = self
            .resolve_channel_session(
                &message,
                resolved_agent_id.as_deref(),
                &tool_names,
                account_cfg.tts_enabled,
                account_cfg.tts_voice.as_deref(),
                session_strategy,
                bound_user_id,
            )
            .await?;

        for attachment in &message.attachments {
            if let Err(err) = self
                .save_media_asset(&message.channel, &message.account_id, attachment)
                .await
            {
                warn!(
                    "save channel media asset failed: channel={}, account_id={}, session_id={}, error={err}",
                    message.channel,
                    message.account_id,
                    session_info.session_id
                );
            }
        }

        let media_processor = MediaProcessor::new(config.channels.media.clone());
        let allow_vision = resolve_allow_vision(&config, None);
        let MediaProcessingResult {
            text,
            attachments,
            meta,
        } = media_processor
            .process_inbound(&message, allow_vision)
            .await;

        if let Err(err) = self
            .insert_channel_message(&message, &session_info.session_id, raw_payload)
            .await
        {
            warn!(
                "insert channel message failed: channel={}, account_id={}, session_id={}, error={err}",
                message.channel,
                message.account_id,
                session_info.session_id
            );
            self.monitor.record_event(
                &session_info.session_id,
                "channel_message_save_error",
                &json!({
                    "channel": message.channel,
                    "account_id": message.account_id,
                    "error": err.to_string(),
                }),
            );
        }
        if meta.get("asr").is_some() {
            self.monitor.record_event(
                &session_info.session_id,
                "asr_done",
                &json!({ "channel": message.channel, "account_id": message.account_id }),
            );
        }
        if meta.get("ocr").is_some() {
            self.monitor.record_event(
                &session_info.session_id,
                "ocr_done",
                &json!({ "channel": message.channel, "account_id": message.account_id }),
            );
        }

        let question = if text.trim().is_empty() {
            message
                .text
                .clone()
                .unwrap_or_else(|| "[empty message]".to_string())
        } else {
            text
        };

        let agent_prompt = agent_record
            .as_ref()
            .map(|record| record.system_prompt.trim().to_string())
            .filter(|value| !value.is_empty());

        let request = WunderRequest {
            user_id: session_info.user_id.clone(),
            question,
            tool_names: tool_names.clone(),
            skip_tool_calls: false,
            stream: false,
            debug_payload: false,
            session_id: Some(session_info.session_id.clone()),
            agent_id: resolved_agent_id.clone(),
            model_name: None,
            language: Some(crate::i18n::get_language()),
            config_overrides: None,
            agent_prompt,
            attachments: if attachments.is_empty() {
                None
            } else {
                Some(attachments)
            },
            is_admin: false,
        };

        let response = self.orchestrator.run(request).await?;
        let mut outbound = ChannelOutboundMessage {
            channel: message.channel.clone(),
            account_id: message.account_id.clone(),
            peer: message.peer.clone(),
            thread: message.thread.clone(),
            text: Some(response.answer.clone()),
            attachments: Vec::new(),
            meta: Some(json!({
                "session_id": session_info.session_id,
                "binding_id": resolved_binding.as_ref().and_then(|b| b.binding_id.clone()),
                "message_id": message.message_id,
                "media": meta,
            })),
        };

        let tts_enabled = session_info.tts_enabled.unwrap_or(false);
        if tts_enabled {
            if let Ok(Some(attachment)) = media_processor
                .synthesize_tts(&response.answer, session_info.tts_voice.as_deref())
                .await
            {
                if let Err(err) = self
                    .save_media_asset(&message.channel, &message.account_id, &attachment)
                    .await
                {
                    warn!(
                        "save channel tts asset failed: channel={}, account_id={}, session_id={}, error={err}",
                        message.channel,
                        message.account_id,
                        session_info.session_id
                    );
                }
                outbound.attachments.push(attachment);
                if let Some(session_id) = outbound
                    .meta
                    .as_ref()
                    .and_then(|value| value.get("session_id"))
                    .and_then(Value::as_str)
                {
                    self.monitor.record_event(
                        session_id,
                        "tts_done",
                        &json!({ "channel": message.channel, "account_id": message.account_id }),
                    );
                }
            }
        }

        let outbox_id = self.enqueue_outbox(&outbound).await?;
        if !resolve_outbox_config(config.channels.outbox.clone()).worker_enabled {
            let record = self.get_outbox(&outbox_id).await?;
            if let Some(record) = record {
                if let Err(err) = self.deliver_outbox_record(&record).await {
                    warn!(
                        "deliver outbox failed (worker disabled): outbox_id={}, channel={}, account_id={}, error={err}",
                        record.outbox_id, record.channel, record.account_id
                    );
                }
            }
        }

        if let Some(session_id) = outbound
            .meta
            .as_ref()
            .and_then(|value| value.get("session_id"))
            .and_then(Value::as_str)
        {
            self.monitor.record_event(
                session_id,
                "channel_inbound",
                &json!({
                    "channel": message.channel,
                    "account_id": message.account_id,
                    "peer_id": message.peer.id,
                    "message_type": message.message_type,
                }),
            );
            self.monitor.record_event(
                session_id,
                "channel_bound",
                &json!({
                    "agent_id": resolved_agent_id,
                    "binding_id": resolved_binding.as_ref().and_then(|b| b.binding_id.clone()),
                }),
            );
        }

        Ok(ChannelInboundResult {
            session_id: outbound
                .meta
                .as_ref()
                .and_then(|value| value.get("session_id"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            outbox_id: Some(outbox_id),
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn resolve_channel_session(
        &self,
        message: &ChannelMessage,
        agent_id: Option<&str>,
        tool_overrides: &[String],
        tts_enabled: Option<bool>,
        tts_voice: Option<&str>,
        session_strategy: ChannelSessionStrategy,
        bound_user_id: Option<String>,
    ) -> Result<ChannelSessionInfo> {
        let channel = message.channel.clone();
        let account_id = message.account_id.clone();
        let peer_kind = message.peer.kind.clone();
        let peer_id = message.peer.id.clone();
        let thread_id = message.thread.as_ref().map(|thread| thread.id.clone());
        let existing = self
            .get_channel_session(
                &channel,
                &account_id,
                &peer_kind,
                &peer_id,
                thread_id.as_deref(),
            )
            .await?;
        let tts_enabled = tts_enabled.or_else(|| existing.as_ref().and_then(|r| r.tts_enabled));
        let tts_voice = tts_voice
            .map(|value| value.to_string())
            .or_else(|| existing.as_ref().and_then(|r| r.tts_voice.clone()));
        let now = now_ts();
        let user_id = bound_user_id
            .or_else(|| existing.as_ref().map(|record| record.user_id.clone()))
            .unwrap_or_else(|| {
                format!(
                    "chan:{}:{}:{}:{}",
                    channel.to_lowercase(),
                    account_id.to_lowercase(),
                    peer_kind.to_lowercase(),
                    peer_id
                )
            });
        let cleaned_agent = agent_id.map(|value| value.trim()).unwrap_or("");
        let agent_value = if cleaned_agent.is_empty() {
            None
        } else {
            Some(cleaned_agent.to_string())
        };
        let use_main_thread = matches!(session_strategy, ChannelSessionStrategy::MainThread)
            || (matches!(session_strategy, ChannelSessionStrategy::Hybrid)
                && is_direct_peer(&peer_kind));
        let session_id = if use_main_thread {
            if let Some(existing_main) = self
                .agent_runtime
                .resolve_main_session_id(&user_id, cleaned_agent)
                .await?
            {
                existing_main
            } else {
                self.agent_runtime
                    .resolve_or_create_main_session_id(&user_id, cleaned_agent)
                    .await?
            }
        } else if let Some(record) = existing.as_ref() {
            record.session_id.clone()
        } else {
            format!("sess_{}", Uuid::new_v4().simple())
        };

        let existing_chat = self.get_chat_session(&user_id, &session_id).await?;
        let mut title = message
            .peer
            .name
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(DEFAULT_SESSION_TITLE)
            .to_string();
        let mut resolved_tool_overrides = tool_overrides.to_vec();
        let mut created_at = existing.as_ref().map(|r| r.created_at).unwrap_or(now);
        if let Some(chat_record) = existing_chat.as_ref() {
            if use_main_thread {
                if !chat_record.title.trim().is_empty() {
                    title = chat_record.title.clone();
                }
                if !chat_record.tool_overrides.is_empty() {
                    resolved_tool_overrides = chat_record.tool_overrides.clone();
                }
            }
            created_at = chat_record.created_at;
        }
        let chat_record = ChatSessionRecord {
            session_id: session_id.clone(),
            user_id: user_id.clone(),
            title,
            created_at,
            updated_at: now,
            last_message_at: now,
            agent_id: agent_value.clone(),
            tool_overrides: resolved_tool_overrides,
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        };
        self.save_chat_session(&chat_record).await?;

        let record = ChannelSessionRecord {
            channel,
            account_id,
            peer_kind,
            peer_id,
            thread_id,
            session_id: session_id.clone(),
            agent_id: agent_value,
            user_id: user_id.clone(),
            tts_enabled,
            tts_voice: tts_voice.clone(),
            metadata: None,
            last_message_at: now,
            created_at: existing.as_ref().map(|r| r.created_at).unwrap_or(now),
            updated_at: now,
        };
        self.upsert_channel_session(&record).await?;
        Ok(ChannelSessionInfo {
            session_id,
            user_id,
            tts_enabled,
            tts_voice,
        })
    }

    async fn enqueue_outbox(&self, outbound: &ChannelOutboundMessage) -> Result<String> {
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

    async fn deliver_outbox_record(&self, record: &ChannelOutboxRecord) -> Result<()> {
        let config = self.config_store.get().await;
        let account = self
            .load_channel_account(&record.channel, &record.account_id, &config)
            .await?;
        let account_cfg = ChannelAccountConfig::from_value(&account.config);
        if record.channel.trim().eq_ignore_ascii_case("whatsapp") {
            if let Some(wa_cfg) = account_cfg.whatsapp_cloud.as_ref() {
                let has_token = wa_cfg
                    .access_token
                    .as_deref()
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false);
                if has_token {
                    let outbound: ChannelOutboundMessage =
                        serde_json::from_value(record.payload.clone())
                            .map_err(|err| anyhow!("invalid outbound payload: {err}"))?;
                    whatsapp_cloud::send_outbound(&self.http, &outbound, wa_cfg).await?;
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
                    return Ok(());
                }
            }
        }
        if record
            .channel
            .trim()
            .eq_ignore_ascii_case(feishu::FEISHU_CHANNEL)
        {
            if let Some(feishu_cfg) = account_cfg.feishu.as_ref() {
                let outbound: ChannelOutboundMessage =
                    serde_json::from_value(record.payload.clone())
                        .map_err(|err| anyhow!("invalid outbound payload: {err}"))?;
                feishu::send_outbound(&self.http, &outbound, feishu_cfg).await?;
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
                return Ok(());
            }
        }
        if record
            .channel
            .trim()
            .eq_ignore_ascii_case(qqbot::QQBOT_CHANNEL)
        {
            if let Some(qqbot_cfg) = account_cfg.qqbot.as_ref() {
                let outbound: ChannelOutboundMessage =
                    serde_json::from_value(record.payload.clone())
                        .map_err(|err| anyhow!("invalid outbound payload: {err}"))?;
                qqbot::send_outbound(&self.http, &outbound, qqbot_cfg).await?;
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
                return Ok(());
            }
        }
        let outbound_url = account_cfg
            .outbound_url
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let Some(outbound_url) = outbound_url else {
            self.update_outbox_status(record, "sent", None).await?;
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
            Ok(())
        } else {
            let body = match response.text().await {
                Ok(value) => truncate_text(&value, 2048),
                Err(err) => format!("(read body failed: {err})"),
            };
            Err(anyhow!("outbound delivery failed: {status} {body}"))
        }
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

    async fn outbox_loop(&self) {
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
                            let mut status = "retry";
                            let error_text = Some(err.to_string());
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

    async fn load_channel_account(
        &self,
        channel: &str,
        account_id: &str,
        config: &Config,
    ) -> Result<ChannelAccountRecord> {
        let channel = channel.trim();
        let account_id = account_id.trim();
        if channel.is_empty() || account_id.is_empty() {
            return Err(anyhow!("missing channel/account"));
        }
        let storage = self.storage.clone();
        let channel_key = channel.to_string();
        let account_key = account_id.to_string();
        let channel_lookup = channel_key.clone();
        let account_lookup = account_key.clone();
        let record = tokio::task::spawn_blocking(move || {
            storage.get_channel_account(&channel_lookup, &account_lookup)
        })
        .await
        .unwrap_or_else(|err| Err(anyhow!(err)))?;
        if let Some(record) = record {
            if record.status.trim().to_lowercase() != "active" {
                return Err(anyhow!("channel account disabled"));
            }
            return Ok(record);
        }
        if config.channels.allow_unknown_accounts {
            Ok(ChannelAccountRecord {
                channel: channel_key,
                account_id: account_key,
                config: json!({}),
                status: "active".to_string(),
                created_at: now_ts(),
                updated_at: now_ts(),
            })
        } else {
            Err(anyhow!("channel account not found"))
        }
    }

    async fn list_channel_bindings(
        &self,
        channel: Option<&str>,
    ) -> Result<Vec<ChannelBindingRecord>> {
        let storage = self.storage.clone();
        let channel = channel.map(|value| value.to_string());
        tokio::task::spawn_blocking(move || storage.list_channel_bindings(channel.as_deref()))
            .await
            .unwrap_or_else(|err| Err(anyhow!(err)))
    }

    async fn get_channel_user_binding(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
    ) -> Result<Option<crate::storage::ChannelUserBindingRecord>> {
        let storage = self.storage.clone();
        let channel = channel.to_string();
        let account_id = account_id.to_string();
        let peer_kind = peer_kind.to_string();
        let peer_id = peer_id.to_string();
        tokio::task::spawn_blocking(move || {
            storage.get_channel_user_binding(&channel, &account_id, &peer_kind, &peer_id)
        })
        .await
        .unwrap_or_else(|err| Err(anyhow!(err)))
    }

    async fn get_channel_session(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
        thread_id: Option<&str>,
    ) -> Result<Option<ChannelSessionRecord>> {
        let storage = self.storage.clone();
        let channel = channel.to_string();
        let account_id = account_id.to_string();
        let peer_kind = peer_kind.to_string();
        let peer_id = peer_id.to_string();
        let thread_id = thread_id.map(|value| value.to_string());
        tokio::task::spawn_blocking(move || {
            storage.get_channel_session(
                &channel,
                &account_id,
                &peer_kind,
                &peer_id,
                thread_id.as_deref(),
            )
        })
        .await
        .unwrap_or_else(|err| Err(anyhow!(err)))
    }

    async fn get_chat_session(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<Option<ChatSessionRecord>> {
        let user_store = self.user_store.clone();
        let user_id = user_id.to_string();
        let session_id = session_id.to_string();
        tokio::task::spawn_blocking(move || user_store.get_chat_session(&user_id, &session_id))
            .await
            .unwrap_or_else(|err| Err(anyhow!(err)))
    }

    async fn upsert_channel_session(&self, record: &ChannelSessionRecord) -> Result<()> {
        let storage = self.storage.clone();
        let record = record.clone();
        tokio::task::spawn_blocking(move || storage.upsert_channel_session(&record))
            .await
            .unwrap_or_else(|err| Err(anyhow!(err)))
    }

    async fn save_chat_session(&self, record: &ChatSessionRecord) -> Result<()> {
        let user_store = self.user_store.clone();
        let record = record.clone();
        tokio::task::spawn_blocking(move || user_store.upsert_chat_session(&record))
            .await
            .unwrap_or_else(|err| Err(anyhow!(err)))
    }

    async fn save_media_asset(
        &self,
        channel: &str,
        account_id: &str,
        attachment: &crate::channels::types::ChannelAttachment,
    ) -> Result<()> {
        let storage = self.storage.clone();
        let record = crate::storage::MediaAssetRecord {
            asset_id: format!("asset_{}", Uuid::new_v4().simple()),
            kind: attachment.kind.clone(),
            url: attachment.url.clone(),
            mime: attachment.mime.clone(),
            size: attachment.size,
            hash: None,
            source: Some(format!("{channel}:{account_id}")),
            created_at: now_ts(),
        };
        tokio::task::spawn_blocking(move || storage.upsert_media_asset(&record))
            .await
            .unwrap_or_else(|err| Err(anyhow!(err)))
    }

    async fn insert_channel_message(
        &self,
        message: &ChannelMessage,
        session_id: &str,
        raw_payload: Option<Value>,
    ) -> Result<()> {
        let record = ChannelMessageRecord {
            channel: message.channel.clone(),
            account_id: message.account_id.clone(),
            peer_kind: message.peer.kind.clone(),
            peer_id: message.peer.id.clone(),
            thread_id: message.thread.as_ref().map(|thread| thread.id.clone()),
            session_id: session_id.to_string(),
            message_id: message.message_id.clone(),
            sender_id: message.sender.as_ref().map(|sender| sender.id.clone()),
            message_type: message.message_type.clone(),
            payload: json!(message),
            raw_payload,
            created_at: now_ts(),
        };
        let storage = self.storage.clone();
        tokio::task::spawn_blocking(move || storage.insert_channel_message(&record))
            .await
            .unwrap_or_else(|err| Err(anyhow!(err)))
    }

    async fn insert_outbox(&self, record: &ChannelOutboxRecord) -> Result<()> {
        let storage = self.storage.clone();
        let record = record.clone();
        tokio::task::spawn_blocking(move || storage.enqueue_channel_outbox(&record))
            .await
            .unwrap_or_else(|err| Err(anyhow!(err)))
    }

    async fn get_outbox(&self, outbox_id: &str) -> Result<Option<ChannelOutboxRecord>> {
        let storage = self.storage.clone();
        let outbox_id = outbox_id.to_string();
        tokio::task::spawn_blocking(move || storage.get_channel_outbox(&outbox_id))
            .await
            .unwrap_or_else(|err| Err(anyhow!(err)))
    }

    async fn list_pending_outbox(&self, limit: i64) -> Result<Vec<ChannelOutboxRecord>> {
        let storage = self.storage.clone();
        tokio::task::spawn_blocking(move || storage.list_pending_channel_outbox(limit))
            .await
            .unwrap_or_else(|err| Err(anyhow!(err)))
    }

    async fn set_outbox_status(&self, params: UpdateChannelOutboxStatusParams<'_>) -> Result<()> {
        let storage = self.storage.clone();
        let outbox_id = params.outbox_id.to_string();
        let status = params.status.to_string();
        let last_error = params.last_error.map(|value| value.to_string());
        let retry_count = params.retry_count;
        let retry_at = params.retry_at;
        let delivered_at = params.delivered_at;
        let updated_at = params.updated_at;
        tokio::task::spawn_blocking(move || {
            storage.update_channel_outbox_status(UpdateChannelOutboxStatusParams {
                outbox_id: &outbox_id,
                status: &status,
                retry_count,
                retry_at,
                last_error: last_error.as_deref(),
                delivered_at,
                updated_at,
            })
        })
        .await
        .unwrap_or_else(|err| Err(anyhow!(err)))
    }

    async fn get_agent(&self, agent_id: &str) -> Result<Option<UserAgentRecord>> {
        let user_store = self.user_store.clone();
        let agent_id = agent_id.to_string();
        tokio::task::spawn_blocking(move || user_store.get_user_agent_by_id(&agent_id))
            .await
            .unwrap_or_else(|err| Err(anyhow!(err)))
    }
}

#[derive(Debug, Clone)]
struct ChannelSessionInfo {
    session_id: String,
    user_id: String,
    tts_enabled: Option<bool>,
    tts_voice: Option<String>,
}

fn normalize_message(provider: &str, message: &mut ChannelMessage) -> Result<()> {
    if message.channel.trim().is_empty() {
        message.channel = provider.trim().to_string();
    }
    if message.channel.trim().is_empty() {
        return Err(anyhow!("missing channel"));
    }
    if message.account_id.trim().is_empty() {
        return Err(anyhow!("missing account_id"));
    }
    if message.peer.id.trim().is_empty() {
        return Err(anyhow!("missing peer id"));
    }
    if message.peer.kind.trim().is_empty() {
        message.peer.kind = "dm".to_string();
    }
    if message.message_type.trim().is_empty() {
        message.message_type = if message.attachments.is_empty() {
            "text".to_string()
        } else {
            "mixed".to_string()
        };
    }
    Ok(())
}

fn is_direct_peer(kind: &str) -> bool {
    matches!(
        kind.trim().to_ascii_lowercase().as_str(),
        "dm" | "direct" | "single" | "user"
    )
}

fn validate_inbound_account(
    headers: &HeaderMap,
    account: &ChannelAccountRecord,
    config: &ChannelAccountConfig,
) -> Result<()> {
    if account.status.trim().to_lowercase() != "active" {
        return Err(anyhow!("channel account disabled"));
    }
    let token = config
        .inbound_token
        .as_deref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    if let Some(expected) = token {
        let provided = headers
            .get("x-channel-token")
            .and_then(|value| value.to_str().ok())
            .or_else(|| {
                headers
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
            })
            .unwrap_or("");
        let cleaned = provided.trim().trim_start_matches("Bearer ").trim();
        if cleaned != expected {
            return Err(anyhow!("invalid channel token"));
        }
    }
    Ok(())
}

fn enforce_allowlist(message: &ChannelMessage, config: &ChannelAccountConfig) -> Result<()> {
    let peer_id = message.peer.id.trim().to_lowercase();
    let sender_id = message
        .sender
        .as_ref()
        .map(|sender| sender.id.trim().to_lowercase())
        .unwrap_or_default();
    let normalize = |items: &[String]| {
        items
            .iter()
            .map(|value| value.trim().to_lowercase())
            .filter(|value| !value.is_empty())
            .collect::<HashSet<_>>()
    };
    let deny_peers = normalize(&config.deny_peers);
    if !deny_peers.is_empty() && deny_peers.contains(&peer_id) {
        return Err(anyhow!("peer blocked"));
    }
    let deny_senders = normalize(&config.deny_senders);
    if !deny_senders.is_empty() && deny_senders.contains(&sender_id) {
        return Err(anyhow!("sender blocked"));
    }
    let allow_peers = normalize(&config.allow_peers);
    if !allow_peers.is_empty() && !allow_peers.contains(&peer_id) {
        return Err(anyhow!("peer not allowed"));
    }
    let allow_senders = normalize(&config.allow_senders);
    if !allow_senders.is_empty() && !allow_senders.contains(&sender_id) {
        return Err(anyhow!("sender not allowed"));
    }
    Ok(())
}

fn resolve_rate_limit(config: &ChannelRateLimitConfig, channel: &str) -> RateLimitConfig {
    let normalized = channel.trim().to_lowercase();
    let override_cfg = config.by_channel.get(&normalized);
    let qps = override_cfg
        .and_then(|value| value.qps)
        .unwrap_or(config.default_qps);
    let concurrency = override_cfg
        .and_then(|value| value.concurrency)
        .unwrap_or(config.default_concurrency);
    RateLimitConfig { qps, concurrency }
}

fn resolve_tool_names(
    binding: Option<&BindingResolution>,
    account: &ChannelAccountConfig,
    agent: Option<&UserAgentRecord>,
    config: &Config,
) -> Vec<String> {
    let mut names = if let Some(binding) = binding {
        if !binding.tool_overrides.is_empty() {
            binding.tool_overrides.clone()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    if names.is_empty() && !account.tool_overrides.is_empty() {
        names = account.tool_overrides.clone();
    }
    if names.is_empty() {
        if let Some(agent) = agent {
            if !agent.tool_names.is_empty() {
                names = agent.tool_names.clone();
            }
        }
    }
    if names.is_empty() && !config.channels.default_tool_overrides.is_empty() {
        names = config.channels.default_tool_overrides.clone();
    }
    if names.is_empty() {
        return Vec::new();
    }
    normalize_tool_overrides(names)
}

fn normalize_tool_overrides(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    let mut has_none = false;
    for raw in values {
        let name = raw.trim().to_string();
        if name.is_empty() || seen.contains(&name) {
            continue;
        }
        if name == TOOL_OVERRIDE_NONE {
            has_none = true;
        }
        seen.insert(name.clone());
        output.push(name);
    }
    if has_none {
        vec![TOOL_OVERRIDE_NONE.to_string()]
    } else {
        output
    }
}

fn resolve_allow_vision(config: &Config, model_name: Option<&str>) -> bool {
    let fallback = config.llm.default.as_str();
    let model = model_name
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback);
    if model.trim().is_empty() {
        return false;
    }
    config
        .llm
        .models
        .get(model)
        .and_then(|model| model.support_vision)
        .unwrap_or(false)
}

fn build_outbound_headers(config: &ChannelAccountConfig) -> Result<ReqHeaderMap> {
    let mut headers = ReqHeaderMap::new();
    if let Some(Value::Object(map)) = config.outbound_headers.as_ref() {
        for (key, value) in map {
            let Some(text) = value.as_str() else {
                continue;
            };
            let name = HeaderName::from_bytes(key.as_bytes())?;
            let value = HeaderValue::from_str(text)?;
            headers.insert(name, value);
        }
    }
    if let Some(token) = config
        .outbound_token
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        headers.insert(
            AUTHORIZATION,
            format!("Bearer {token}").parse::<HeaderValue>()?,
        );
    }
    Ok(headers)
}

fn extract_session_id(payload: &Value) -> Option<String> {
    payload
        .get("meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("session_id"))
        .and_then(Value::as_str)
        .map(|value| value.to_string())
}

fn truncate_text(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut output = text[..end].to_string();
    output.push_str("...");
    output
}

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}
