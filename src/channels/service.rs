use crate::channels::binding::{resolve_binding, BindingResolution};
use crate::channels::feishu;
use crate::channels::media::{MediaProcessingResult, MediaProcessor};
use crate::channels::outbox::{compute_retry_at, resolve_outbox_config};
use crate::channels::qqbot;
use crate::channels::rate_limit::{ChannelRateLimiter, RateLimitConfig};
use crate::channels::types::{
    ChannelAccountConfig, ChannelMessage, ChannelOutboundMessage, FeishuConfig,
};
use crate::channels::whatsapp_cloud;
use crate::config::{ChannelRateLimitConfig, Config};
use crate::monitor::MonitorState;
use crate::orchestrator::Orchestrator;
use crate::schemas::WunderRequest;
use crate::services::agent_runtime::AgentRuntime;
use crate::storage::{
    ChannelAccountRecord, ChannelBindingRecord, ChannelMessageRecord, ChannelOutboxRecord,
    ChannelSessionRecord, ChatSessionRecord, ListChannelUserBindingsQuery, StorageBackend,
    UpdateChannelOutboxStatusParams, UserAgentRecord,
};
use crate::user_store::UserStore;
use anyhow::{anyhow, Result};
use axum::http::{HeaderMap, HeaderValue as AxumHeaderValue};
use chrono::Local;
use parking_lot::Mutex;
use reqwest::header::{HeaderMap as ReqHeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tokio_stream::StreamExt;
use tracing::{error, warn};
use uuid::Uuid;

const TOOL_OVERRIDE_NONE: &str = "__no_tools__";
const DEFAULT_SESSION_TITLE: &str = "Channel Session";
const SESSION_STRATEGY_MAIN_THREAD: &str = "main_thread";
const SESSION_STRATEGY_PER_PEER: &str = "per_peer";
const SESSION_STRATEGY_HYBRID: &str = "hybrid";
const FEISHU_LONG_CONN_SUPERVISOR_INTERVAL_S: u64 = 10;
const FEISHU_LONG_CONN_RETRY_BASE_S: u64 = 3;
const FEISHU_LONG_CONN_RETRY_MAX_S: u64 = 30;
const CHANNEL_MESSAGE_DEDUPE_TTL_S: f64 = 120.0;

#[derive(Debug, Clone)]
struct FeishuLongConnTarget {
    account_id: String,
    updated_at: f64,
    inbound_token: Option<String>,
    config: FeishuConfig,
}

impl FeishuLongConnTarget {
    fn task_key(&self) -> String {
        format!("{}:{:.3}", self.account_id, self.updated_at)
    }
}

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

#[derive(Debug, Clone, Copy)]
enum ChannelCommand {
    NewThread,
    Stop,
    Help,
}

#[derive(Debug, Clone)]
enum ChannelModelResult {
    Answer(String),
    Busy,
}

impl ChannelCommand {
    fn as_str(self) -> &'static str {
        match self {
            Self::NewThread => "new",
            Self::Stop => "stop",
            Self::Help => "help",
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
    recent_inbound: Arc<Mutex<HashMap<String, f64>>>,
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
            recent_inbound: Arc::new(Mutex::new(HashMap::new())),
        };
        let worker = hub.clone();
        tokio::spawn(async move {
            worker.outbox_loop().await;
        });
        let feishu_worker = hub.clone();
        tokio::spawn(async move {
            feishu_worker.feishu_long_connection_supervisor_loop().await;
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

    fn is_duplicate_inbound(&self, message: &ChannelMessage) -> bool {
        let Some(message_id) = message
            .message_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return false;
        };
        let channel = message.channel.trim().to_ascii_lowercase();
        let account_id = message.account_id.trim().to_ascii_lowercase();
        if channel.is_empty() || account_id.is_empty() {
            return false;
        }
        let key = format!("{channel}:{account_id}:{message_id}");
        let now = now_ts();
        let mut guard = self.recent_inbound.lock();
        guard.retain(|_, ts| now - *ts < CHANNEL_MESSAGE_DEDUPE_TTL_S);
        if guard.contains_key(&key) {
            return true;
        }
        guard.insert(key, now);
        false
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
        if self.is_duplicate_inbound(&message) {
            return Ok(ChannelInboundResult {
                session_id: String::new(),
                outbox_id: None,
            });
        }
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
        let sender_fallback_id = message
            .sender
            .as_ref()
            .map(|sender| sender.id.trim().to_string())
            .filter(|value| !value.is_empty())
            .filter(|value| !value.eq_ignore_ascii_case(message.peer.id.trim()))
            .filter(|_| is_direct_peer(&message.peer.kind));
        let mut resolved_binding = resolve_binding(&bindings, &message);
        if resolved_binding.is_none() {
            if let Some(sender_id) = sender_fallback_id.as_deref() {
                let mut fallback_message = message.clone();
                fallback_message.peer.id = sender_id.to_string();
                resolved_binding = resolve_binding(&bindings, &fallback_message);
            }
        }
        let fallback_agent_id = if resolved_binding
            .as_ref()
            .and_then(|binding| binding.agent_id.as_ref())
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .is_none()
            && account_cfg
                .agent_id
                .as_ref()
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .is_none()
        {
            resolve_agent_id_by_account(&bindings, &message)
        } else {
            None
        };
        let account_agent_id = account_cfg
            .agent_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let mut resolved_agent_id = resolved_binding
            .as_ref()
            .and_then(|binding| binding.agent_id.clone())
            .or_else(|| account_agent_id.clone())
            .or(fallback_agent_id)
            .or_else(|| config.channels.default_agent_id.clone());
        if message
            .channel
            .trim()
            .eq_ignore_ascii_case(feishu::FEISHU_CHANNEL)
        {
            if let Some(account_agent) = account_agent_id {
                resolved_agent_id = Some(account_agent);
            }
        }

        let mut agent_record = match resolved_agent_id.as_ref() {
            Some(agent_id) => self.get_agent(agent_id).await,
            None => Ok(None),
        }?;

        let tool_names = resolve_tool_names(
            resolved_binding.as_ref(),
            &account_cfg,
            agent_record.as_ref(),
            &config,
        );

        let mut session_strategy = ChannelSessionStrategy::from_config(&config);
        if message
            .channel
            .trim()
            .eq_ignore_ascii_case(feishu::FEISHU_CHANNEL)
        {
            session_strategy = ChannelSessionStrategy::MainThread;
        }
        let mut bound_user_id = self
            .get_channel_user_binding(
                &message.channel,
                &message.account_id,
                &message.peer.kind,
                &message.peer.id,
            )
            .await?
            .map(|record| record.user_id);
        if bound_user_id.is_none() {
            if let Some(sender_id) = sender_fallback_id.as_deref() {
                bound_user_id = self
                    .get_channel_user_binding(
                        &message.channel,
                        &message.account_id,
                        &message.peer.kind,
                        sender_id,
                    )
                    .await?
                    .map(|record| record.user_id);
            }
        }
        if bound_user_id.is_none() {
            bound_user_id = account
                .config
                .get("owner_user_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
        }
        if bound_user_id.is_none() {
            bound_user_id = self
                .get_channel_account_owner(&message.channel, &message.account_id)
                .await?;
        }
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
        self.touch_chat_session_activity(&session_info.user_id, &session_info.session_id)
            .await;

        if resolved_agent_id.is_none() {
            if let Some(session_agent_id) = self
                .resolve_session_agent_id(&session_info.user_id, &session_info.session_id)
                .await?
            {
                resolved_agent_id = Some(session_agent_id.clone());
                if agent_record.is_none() {
                    agent_record = self.get_agent(&session_agent_id).await?;
                }
            }
        }
        if resolved_agent_id.is_none() {
            warn!(
                "channel agent unresolved: channel={}, account_id={}, peer_kind={}, peer_id={}, sender_id={}",
                message.channel,
                message.account_id,
                message.peer.kind,
                message.peer.id,
                message
                    .sender
                    .as_ref()
                    .map(|sender| sender.id.as_str())
                    .unwrap_or_default()
            );
        }

        if let Err(err) = self
            .insert_channel_message(&message, &session_info.session_id, raw_payload.clone())
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

        if let Some(command) = parse_channel_command(message.text.as_deref()) {
            return self
                .handle_channel_command(
                    command,
                    &message,
                    &session_info,
                    resolved_agent_id.as_deref(),
                    &tool_names,
                    account_cfg.tts_enabled,
                    account_cfg.tts_voice.as_deref(),
                    session_strategy,
                )
                .await;
        }

        let limiter_key = format!("{}:{}", message.channel, message.account_id);
        let rate_cfg = resolve_rate_limit(&config.channels.rate_limit, &message.channel);
        let _rate_guard = match self.rate_limiter.acquire(&limiter_key, rate_cfg) {
            Some(guard) => guard,
            None => {
                return self
                    .respond_busy(&message, &session_info, resolved_binding.as_ref())
                    .await;
            }
        };

        if message
            .channel
            .trim()
            .eq_ignore_ascii_case(feishu::FEISHU_CHANNEL)
        {
            if let Err(err) = self
                .send_processing_ack(&config, &message, &session_info, resolved_binding.as_ref())
                .await
            {
                warn!(
                    "send channel processing ack failed: channel={}, account_id={}, session_id={}, error={err}",
                    message.channel,
                    message.account_id,
                    session_info.session_id
                );
            }
        }

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
            stream: true,
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
            allow_queue: false,
            is_admin: false,
            approval_tx: None,
        };
        let response = match self
            .run_channel_request(request, &session_info.user_id, &session_info.session_id)
            .await?
        {
            ChannelModelResult::Answer(answer) => answer,
            ChannelModelResult::Busy => {
                return self
                    .respond_busy(&message, &session_info, resolved_binding.as_ref())
                    .await;
            }
        };
        if response.trim().is_empty() {
            warn!(
                "channel response empty: channel={}, account_id={}, peer_id={}",
                message.channel, message.account_id, message.peer.id
            );
        }
        let mut outbound = ChannelOutboundMessage {
            channel: message.channel.clone(),
            account_id: message.account_id.clone(),
            peer: message.peer.clone(),
            thread: message.thread.clone(),
            text: Some(response.clone()),
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
                .synthesize_tts(&response, session_info.tts_voice.as_deref())
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
                    "chan:{}:{}",
                    channel.to_lowercase(),
                    account_id.to_lowercase(),
                )
            });
        let resolved_agent = agent_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                existing
                    .as_ref()
                    .and_then(|record| record.agent_id.as_ref())
                    .map(String::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .unwrap_or_default();
        let cleaned_agent = resolved_agent.trim();
        let mut agent_value = if cleaned_agent.is_empty() {
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
            if agent_value.is_none() {
                agent_value = chat_record
                    .agent_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
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
            agent_id: agent_value.clone(),
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

    async fn resolve_session_agent_id(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<Option<String>> {
        let session = self.get_chat_session(user_id, session_id).await?;
        Ok(session
            .and_then(|record| record.agent_id)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()))
    }

    async fn run_channel_request(
        &self,
        request: WunderRequest,
        user_id: &str,
        session_id: &str,
    ) -> Result<ChannelModelResult> {
        let session_id_owned = session_id.to_string();
        let mut stream = self.orchestrator.stream(request).await?;
        let mut final_answer: Option<String> = None;
        while let Some(event) = stream.next().await {
            let event = match event {
                Ok(item) => item,
                Err(_) => continue,
            };
            let event_payload = event
                .data
                .get("data")
                .cloned()
                .unwrap_or_else(|| event.data.clone());
            if event.event == "error" {
                let code = event_payload
                    .get("code")
                    .and_then(Value::as_str)
                    .or_else(|| event.data.get("code").and_then(Value::as_str))
                    .unwrap_or_default();
                if code == "USER_BUSY" {
                    return Ok(ChannelModelResult::Busy);
                }
                let message = event_payload
                    .get("message")
                    .and_then(Value::as_str)
                    .or_else(|| event.data.get("message").and_then(Value::as_str))
                    .unwrap_or_default();
                let detail = if message.trim().is_empty() {
                    serde_json::to_string(&event_payload).unwrap_or_default()
                } else {
                    message.to_string()
                };
                return Err(anyhow!("channel stream run failed: {detail}"));
            }
            if event.event == "final" {
                let answer = event_payload
                    .get("answer")
                    .or_else(|| event_payload.get("content"))
                    .or_else(|| event_payload.get("message"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or_default()
                    .to_string();
                final_answer = Some(answer);
                break;
            }
        }
        let mut answer = match final_answer {
            Some(answer) if !answer.trim().is_empty() => answer,
            _ => self
                .load_latest_assistant_message(user_id, &session_id_owned)
                .await
                .unwrap_or_default(),
        };
        if answer.trim().is_empty() {
            answer = "Model returned an empty response. Please try again shortly.".to_string();
        }
        Ok(ChannelModelResult::Answer(answer))
    }

    async fn load_latest_assistant_message(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Option<String> {
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return None;
        }
        let storage = self.storage.clone();
        let user_id = cleaned_user.to_string();
        let session_id = cleaned_session.to_string();
        let history = tokio::task::spawn_blocking(move || {
            storage.load_chat_history(&user_id, &session_id, Some(20))
        })
        .await
        .ok()?
        .ok()?;
        for item in history.iter().rev() {
            let role = item.get("role").and_then(Value::as_str).unwrap_or("");
            if !role.eq_ignore_ascii_case("assistant") {
                continue;
            }
            if let Some(text) = extract_chat_content(item) {
                let cleaned = text.trim();
                if !cleaned.is_empty() {
                    return Some(cleaned.to_string());
                }
            }
        }
        None
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

    async fn feishu_long_connection_supervisor_loop(&self) {
        let mut workers: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();
        loop {
            workers.retain(|_, handle| !handle.is_finished());
            let config = self.config_store.get().await;
            if !channels_runtime_enabled(&config) {
                for (_, handle) in workers.drain() {
                    handle.abort();
                }
                sleep(Duration::from_secs(FEISHU_LONG_CONN_SUPERVISOR_INTERVAL_S)).await;
                continue;
            }

            match self.list_feishu_long_connection_targets().await {
                Ok(targets) => {
                    let mut desired_keys = HashSet::new();
                    for target in targets {
                        let task_key = target.task_key();
                        desired_keys.insert(task_key.clone());
                        if workers.contains_key(&task_key) {
                            continue;
                        }
                        let worker = self.clone();
                        workers.insert(
                            task_key,
                            tokio::spawn(async move {
                                worker.feishu_long_connection_worker_loop(target).await;
                            }),
                        );
                    }

                    let stale_keys = workers
                        .keys()
                        .filter(|key| !desired_keys.contains(*key))
                        .cloned()
                        .collect::<Vec<_>>();
                    for task_key in stale_keys {
                        if let Some(handle) = workers.remove(&task_key) {
                            handle.abort();
                        }
                    }
                }
                Err(err) => {
                    warn!("load feishu long connection targets failed: {err}");
                }
            }

            sleep(Duration::from_secs(FEISHU_LONG_CONN_SUPERVISOR_INTERVAL_S)).await;
        }
    }

    async fn list_feishu_long_connection_targets(&self) -> Result<Vec<FeishuLongConnTarget>> {
        let storage = self.storage.clone();
        let accounts = tokio::task::spawn_blocking(move || {
            storage.list_channel_accounts(Some(feishu::FEISHU_CHANNEL), Some("active"))
        })
        .await
        .unwrap_or_else(|err| Err(anyhow!(err)))?;

        let mut targets = Vec::new();
        for record in accounts {
            let account_cfg = ChannelAccountConfig::from_value(&record.config);
            let Some(feishu_cfg) = account_cfg.feishu else {
                continue;
            };
            if !feishu::long_connection_enabled(&feishu_cfg) {
                continue;
            }
            if !feishu::has_long_connection_credentials(&feishu_cfg) {
                warn!(
                    "skip feishu long connection target without app credentials: account_id={}",
                    record.account_id
                );
                continue;
            }
            targets.push(FeishuLongConnTarget {
                account_id: record.account_id,
                updated_at: record.updated_at,
                inbound_token: account_cfg.inbound_token,
                config: feishu_cfg,
            });
        }

        Ok(targets)
    }

    async fn feishu_long_connection_worker_loop(&self, target: FeishuLongConnTarget) {
        let mut retry_delay_s = FEISHU_LONG_CONN_RETRY_BASE_S;
        loop {
            let result = feishu::run_long_connection_session(&self.http, &target.config, {
                let worker = self.clone();
                let event_target = target.clone();
                move |payload| {
                    let worker = worker.clone();
                    let event_target = event_target.clone();
                    async move {
                        worker
                            .handle_feishu_long_connection_payload(&event_target, payload)
                            .await
                    }
                }
            })
            .await;

            match result {
                Ok(endpoint) => {
                    retry_delay_s = endpoint
                        .reconnect_interval_s
                        .clamp(FEISHU_LONG_CONN_RETRY_BASE_S, FEISHU_LONG_CONN_RETRY_MAX_S);
                    warn!(
                        "feishu long connection closed: account_id={}, retry_in={}s",
                        target.account_id, retry_delay_s
                    );
                }
                Err(err) => {
                    warn!(
                        "feishu long connection failed: account_id={}, retry_in={}s, error={err}",
                        target.account_id, retry_delay_s
                    );
                    retry_delay_s = (retry_delay_s * 2).min(FEISHU_LONG_CONN_RETRY_MAX_S);
                }
            }

            sleep(Duration::from_secs(retry_delay_s)).await;
        }
    }

    async fn handle_feishu_long_connection_payload(
        &self,
        target: &FeishuLongConnTarget,
        payload: Value,
    ) -> Result<()> {
        let resolved_payload =
            feishu::decrypt_event_if_needed(payload, target.config.encrypt_key.as_deref())?;
        if !feishu::is_message_event(&resolved_payload) {
            return Ok(());
        }
        let messages =
            feishu::extract_inbound_messages(&resolved_payload, &target.account_id, Some("user"))?;
        if messages.is_empty() {
            return Ok(());
        }
        let headers = build_internal_channel_headers(target.inbound_token.as_deref())?;
        let result = self
            .handle_inbound(
                feishu::FEISHU_CHANNEL,
                &headers,
                messages,
                Some(resolved_payload),
            )
            .await?;
        if !result.errors.is_empty() {
            return Err(anyhow!(
                "feishu long connection inbound rejected: {}",
                result.errors.join(" | ")
            ));
        }
        if result.accepted == 0 {
            return Err(anyhow!(
                "feishu long connection inbound ignored: no message accepted"
            ));
        }
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
                            warn!(
                                "deliver outbox failed: outbox_id={}, channel={}, account_id={}, retry_count={}, error={err}",
                                record.outbox_id,
                                record.channel,
                                record.account_id,
                                record.retry_count
                            );
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
        let peer_kinds = equivalent_peer_kinds(peer_kind);
        let peer_id = peer_id.to_string();
        tokio::task::spawn_blocking(move || {
            let mut peer_ids = vec![peer_id.clone()];
            if !peer_ids
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case("*"))
            {
                peer_ids.push("*".to_string());
            }
            for candidate_kind in &peer_kinds {
                for candidate_peer_id in &peer_ids {
                    if let Some(record) = storage.get_channel_user_binding(
                        &channel,
                        &account_id,
                        candidate_kind,
                        candidate_peer_id,
                    )? {
                        return Ok(Some(record));
                    }
                }
            }
            Ok(None)
        })
        .await
        .unwrap_or_else(|err| Err(anyhow!(err)))
    }

    async fn get_channel_account_owner(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<Option<String>> {
        let storage = self.storage.clone();
        let channel = channel.to_string();
        let account_id = account_id.to_string();
        tokio::task::spawn_blocking(move || {
            let (records, _) =
                storage.list_channel_user_bindings(ListChannelUserBindingsQuery {
                    channel: Some(channel.as_str()),
                    account_id: Some(account_id.as_str()),
                    peer_kind: None,
                    peer_id: None,
                    user_id: None,
                    offset: 0,
                    limit: 1,
                })?;
            Ok(records
                .first()
                .map(|record| record.user_id.trim().to_string())
                .filter(|value| !value.is_empty()))
        })
        .await
        .unwrap_or_else(|err| Err(anyhow!(err)))
    }

    async fn append_channel_chat(
        &self,
        user_id: &str,
        session_id: &str,
        role: &str,
        content: &str,
    ) {
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        let cleaned_role = role.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() || cleaned_role.is_empty() {
            return;
        }
        let payload = json!({
            "role": cleaned_role,
            "content": content,
            "session_id": cleaned_session,
            "timestamp": Local::now().to_rfc3339(),
        });
        let storage = self.storage.clone();
        let user_id = cleaned_user.to_string();
        let outcome = tokio::task::spawn_blocking(move || storage.append_chat(&user_id, &payload))
            .await
            .unwrap_or_else(|err| Err(anyhow!(err)));
        if let Err(err) = outcome {
            warn!(
                "append channel chat failed: user_id={}, session_id={}, role={}, error={err}",
                cleaned_user, cleaned_session, cleaned_role
            );
        }
    }

    async fn touch_chat_session_activity(&self, user_id: &str, session_id: &str) {
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return;
        }
        let user_store = self.user_store.clone();
        let user_id = cleaned_user.to_string();
        let session_id = cleaned_session.to_string();
        let now = now_ts();
        let outcome = tokio::task::spawn_blocking(move || {
            user_store.touch_chat_session(&user_id, &session_id, now, now)
        })
        .await
        .unwrap_or_else(|err| Err(anyhow!(err)));
        if let Err(err) = outcome {
            warn!(
                "touch channel chat session failed: user_id={}, session_id={}, error={err}",
                cleaned_user, cleaned_session
            );
        }
    }

    async fn load_latest_user_message(&self, user_id: &str, session_id: &str) -> Option<String> {
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return None;
        }
        let storage = self.storage.clone();
        let user_id = cleaned_user.to_string();
        let session_id = cleaned_session.to_string();
        let history = tokio::task::spawn_blocking(move || {
            storage.load_chat_history(&user_id, &session_id, Some(20))
        })
        .await
        .ok()?
        .ok()?;
        for item in history {
            let role = item.get("role").and_then(Value::as_str).unwrap_or("");
            if !role.eq_ignore_ascii_case("user") {
                continue;
            }
            if let Some(text) = extract_chat_content(&item) {
                return Some(text);
            }
        }
        None
    }

    async fn respond_busy(
        &self,
        message: &ChannelMessage,
        session_info: &ChannelSessionInfo,
        resolved_binding: Option<&BindingResolution>,
    ) -> Result<ChannelInboundResult> {
        let last_message = self
            .load_latest_user_message(&session_info.user_id, &session_info.session_id)
            .await
            .unwrap_or_default();
        let busy_text = if last_message.trim().is_empty() {
            "智能体在忙，请稍后再试。".to_string()
        } else {
            let preview = truncate_text(last_message.trim(), 120);
            format!("智能体在忙：{preview}")
        };
        let user_text = message_preview_text(message);
        self.append_channel_chat(
            &session_info.user_id,
            &session_info.session_id,
            "user",
            &user_text,
        )
        .await;
        self.append_channel_chat(
            &session_info.user_id,
            &session_info.session_id,
            "assistant",
            &busy_text,
        )
        .await;
        let outbound = ChannelOutboundMessage {
            channel: message.channel.clone(),
            account_id: message.account_id.clone(),
            peer: message.peer.clone(),
            thread: message.thread.clone(),
            text: Some(busy_text.clone()),
            attachments: Vec::new(),
            meta: Some(json!({
                "session_id": session_info.session_id,
                "binding_id": resolved_binding.and_then(|b| b.binding_id.clone()),
                "message_id": message.message_id,
                "busy": true,
            })),
        };
        let outbox_id = self.enqueue_outbox(&outbound).await?;
        Ok(ChannelInboundResult {
            session_id: session_info.session_id.clone(),
            outbox_id: Some(outbox_id),
        })
    }

    async fn send_processing_ack(
        &self,
        config: &Config,
        message: &ChannelMessage,
        session_info: &ChannelSessionInfo,
        resolved_binding: Option<&BindingResolution>,
    ) -> Result<()> {
        let ack_text = "已收到消息，正在处理中，请稍后。".to_string();
        let outbound = ChannelOutboundMessage {
            channel: message.channel.clone(),
            account_id: message.account_id.clone(),
            peer: message.peer.clone(),
            thread: message.thread.clone(),
            text: Some(ack_text),
            attachments: Vec::new(),
            meta: Some(json!({
                "session_id": session_info.session_id,
                "binding_id": resolved_binding.and_then(|b| b.binding_id.clone()),
                "message_id": message.message_id,
                "processing_ack": true,
            })),
        };
        let outbox_id = self.enqueue_outbox(&outbound).await?;
        if !resolve_outbox_config(config.channels.outbox.clone()).worker_enabled {
            let record = self.get_outbox(&outbox_id).await?;
            if let Some(record) = record {
                self.deliver_outbox_record(&record).await?;
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_channel_command(
        &self,
        command: ChannelCommand,
        message: &ChannelMessage,
        session_info: &ChannelSessionInfo,
        agent_id: Option<&str>,
        tool_names: &[String],
        tts_enabled: Option<bool>,
        tts_voice: Option<&str>,
        session_strategy: ChannelSessionStrategy,
    ) -> Result<ChannelInboundResult> {
        let user_id = session_info.user_id.clone();
        let command_text = message.text.as_deref().map(str::trim).unwrap_or("");
        let (target_session_id, reply_text) = match command {
            ChannelCommand::NewThread => {
                let new_session_id = format!("sess_{}", Uuid::new_v4().simple());
                let agent_key = agent_id.unwrap_or("").trim();
                if let Err(err) = self
                    .agent_runtime
                    .set_main_session(&user_id, agent_key, &new_session_id, "channel_command")
                    .await
                {
                    warn!(
                        "channel /new failed to set main session: user_id={}, agent_id={}, error={err}",
                        user_id, agent_key
                    );
                }
                let updated = self
                    .resolve_channel_session(
                        message,
                        agent_id,
                        tool_names,
                        tts_enabled,
                        tts_voice,
                        session_strategy,
                        Some(user_id.clone()),
                    )
                    .await?;
                (updated.session_id, "已创建新线程。".to_string())
            }
            ChannelCommand::Stop => {
                let cancelled = self.monitor.cancel(&session_info.session_id);
                (
                    session_info.session_id.clone(),
                    if cancelled {
                        "已请求停止当前会话。".to_string()
                    } else {
                        "当前没有可停止的会话。".to_string()
                    },
                )
            }
            ChannelCommand::Help => (
                session_info.session_id.clone(),
                "可用指令：/new 新建线程，/stop 停止当前会话。".to_string(),
            ),
        };

        if !command_text.is_empty() {
            self.append_channel_chat(&user_id, &target_session_id, "user", command_text)
                .await;
        }
        self.append_channel_chat(&user_id, &target_session_id, "assistant", &reply_text)
            .await;

        let outbound = ChannelOutboundMessage {
            channel: message.channel.clone(),
            account_id: message.account_id.clone(),
            peer: message.peer.clone(),
            thread: message.thread.clone(),
            text: Some(reply_text.clone()),
            attachments: Vec::new(),
            meta: Some(json!({
                "session_id": target_session_id,
                "command": command.as_str(),
                "message_id": message.message_id,
            })),
        };
        let outbox_id = self.enqueue_outbox(&outbound).await?;
        Ok(ChannelInboundResult {
            session_id: session_info.session_id.clone(),
            outbox_id: Some(outbox_id),
        })
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
        let peer_kinds = equivalent_peer_kinds(peer_kind);
        let peer_id = peer_id.to_string();
        let thread_id = thread_id.map(|value| value.to_string());
        tokio::task::spawn_blocking(move || {
            for candidate_kind in &peer_kinds {
                if let Some(record) = storage.get_channel_session(
                    &channel,
                    &account_id,
                    candidate_kind,
                    &peer_id,
                    thread_id.as_deref(),
                )? {
                    return Ok(Some(record));
                }
            }
            Ok(None)
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

fn build_internal_channel_headers(inbound_token: Option<&str>) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    if let Some(token) = inbound_token
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let header_value = AxumHeaderValue::from_str(token)
            .map_err(|err| anyhow!("invalid inbound token header value: {err}"))?;
        headers.insert("x-channel-token", header_value);
    }
    Ok(headers)
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

fn parse_channel_command(text: Option<&str>) -> Option<ChannelCommand> {
    let raw = text?.trim();
    if !raw.starts_with('/') {
        return None;
    }
    let token = raw
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim()
        .trim_start_matches('/');
    if token.is_empty() {
        return None;
    }
    match token.to_ascii_lowercase().as_str() {
        "new" | "reset" => Some(ChannelCommand::NewThread),
        "stop" | "cancel" => Some(ChannelCommand::Stop),
        "help" | "?" => Some(ChannelCommand::Help),
        _ => None,
    }
}

fn extract_chat_content(payload: &Value) -> Option<String> {
    let content = payload.get("content").unwrap_or(payload);
    match content {
        Value::String(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                if let Some(text) = item.as_str() {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        parts.push(trimmed.to_string());
                    }
                    continue;
                }
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        parts.push(trimmed.to_string());
                    }
                    continue;
                }
                if let Some(text) = item.get("content").and_then(Value::as_str) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        parts.push(trimmed.to_string());
                    }
                }
            }
            if !parts.is_empty() {
                return Some(parts.join(""));
            }
            Some(content.to_string())
        }
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
            if let Some(text) = map.get("content").and_then(Value::as_str) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
            Some(content.to_string())
        }
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

fn message_preview_text(message: &ChannelMessage) -> String {
    if let Some(text) = message.text.as_deref().map(str::trim) {
        if !text.is_empty() {
            return truncate_text(text, 200);
        }
    }
    if !message.attachments.is_empty() {
        if let Some(kind) = message
            .attachments
            .first()
            .map(|item| item.kind.trim())
            .filter(|value| !value.is_empty())
        {
            return format!("[{kind}]");
        }
        return "[attachment]".to_string();
    }
    let message_type = message.message_type.trim();
    if !message_type.is_empty() {
        return format!("[{message_type}]");
    }
    "[empty message]".to_string()
}

fn resolve_agent_id_by_account(
    bindings: &[ChannelBindingRecord],
    message: &ChannelMessage,
) -> Option<String> {
    let channel = message.channel.trim();
    let account_id = message.account_id.trim();
    if channel.is_empty() || account_id.is_empty() {
        return None;
    }
    let mut resolved: Option<String> = None;
    for binding in bindings {
        if !binding.enabled {
            continue;
        }
        if !binding.channel.is_empty() && !binding.channel.trim().eq_ignore_ascii_case(channel) {
            continue;
        }
        if !binding.account_id.is_empty()
            && !binding.account_id.trim().eq_ignore_ascii_case(account_id)
        {
            continue;
        }
        let Some(agent_id) = binding
            .agent_id
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        match resolved.as_ref() {
            None => resolved = Some(agent_id.to_string()),
            Some(existing) => {
                if !existing.eq_ignore_ascii_case(agent_id) {
                    return None;
                }
            }
        }
    }
    resolved
}

fn equivalent_peer_kinds(kind: &str) -> Vec<String> {
    let normalized = kind.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return vec![String::new()];
    }
    if is_direct_peer(&normalized) {
        return ["user", "dm", "direct", "single"]
            .into_iter()
            .map(|value| value.to_string())
            .collect();
    }
    vec![normalized]
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
