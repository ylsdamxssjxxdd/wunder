use crate::channels::binding::{resolve_binding, BindingResolution};
use crate::channels::feishu;
use crate::channels::feishu_files;
use crate::channels::inbound_queue::{
    enqueue_with_timeout, new_channel as new_inbound_channel, spawn_dispatcher,
    ChannelInboundEnvelope, ChannelInboundProcessor, CHANNEL_INBOUND_ENQUEUE_TIMEOUT_MS,
    CHANNEL_INBOUND_MAX_IN_FLIGHT, CHANNEL_INBOUND_QUEUE_CAPACITY,
};
use crate::channels::media::MediaProcessor;
use crate::channels::pending_files::{
    build_channel_question_with_files, build_pending_files_from_attachments,
    format_pending_upload_preview, has_meaningful_channel_text, merge_pending_files,
};
use crate::channels::qqbot;
use crate::channels::rate_limit::ChannelRateLimiter;
use crate::channels::registry::{build_default_channel_adapter_registry, ChannelAdapterRegistry};
use crate::channels::runtime_log::{
    ChannelRuntimeLogBuffer, ChannelRuntimeLogEntry, ChannelRuntimeLogLevel,
};
use crate::channels::types::{ChannelAccountConfig, ChannelMessage, ChannelOutboundMessage};
use crate::channels::weixin;
use crate::channels::weixin_files;
use crate::channels::xmpp;
use crate::config::Config;
use crate::core::approval::{
    new_channel as new_approval_channel, ApprovalRequest, ApprovalRequestRx, ApprovalResponse,
};
use crate::core::approval_registry::{
    ApprovalSource, PendingApprovalEntry, PendingApprovalRegistry,
};
use crate::core::long_task;
use crate::monitor::MonitorState;
use crate::orchestrator::Orchestrator;
use crate::schemas::WunderRequest;
use crate::services::bridge::{append_bridge_meta, resolve_inbound_bridge_route, BridgeRuntime};
use crate::services::runtime::thread::ThreadRuntime;
use crate::services::stream_events::StreamEventService;
use crate::storage::{
    ChannelAccountRecord, ChannelSessionRecord, ChatSessionRecord, StorageBackend,
};
use crate::user_store::UserStore;
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Result};
use axum::http::HeaderMap;
use futures::FutureExt;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::Sender as TokioSender;
use tokio_stream::StreamExt;
use tracing::{debug, warn};
use uuid::Uuid;

mod bridge_flow;
mod long_connection;
mod outbox;
mod persistence;
mod response_flow;
mod support;
use support::{
    append_weixin_context_token, append_weixin_context_token_from_message,
    build_bridge_session_metadata, build_session_title, channel_test_request_overrides,
    channels_runtime_enabled, enforce_allowlist, format_channel_model_error_detail,
    format_channel_model_error_reply, is_compacting_progress_event, is_direct_peer,
    merge_channel_request_overrides, merge_object_value_into, merge_object_values,
    normalize_message, normalize_optional_key, now_ts, parse_channel_approval_decision,
    parse_channel_command, resolve_agent_id_by_account, resolve_channel_actor_id,
    resolve_channel_agent_display_name, resolve_rate_limit, resolve_tool_names, should_auto_title,
    truncate_text, validate_inbound_account,
};

const TOOL_OVERRIDE_NONE: &str = "__no_tools__";
const DEFAULT_SESSION_TITLE: &str = "Channel Session";
const SESSION_STRATEGY_MAIN_THREAD: &str = "main_thread";
const SESSION_STRATEGY_PER_PEER: &str = "per_peer";
const SESSION_STRATEGY_HYBRID: &str = "hybrid";
const CHANNEL_MESSAGE_DEDUPE_TTL_S: f64 = 120.0;
const CHANNEL_RUNTIME_LOG_CAPACITY: usize = 300;
const CHANNEL_RUNTIME_LOG_FLOOD_WINDOW_S: f64 = 20.0;
const CHANNEL_OPEN_APPROVAL_FOR_TEST: bool = true;
const CHANNEL_MODEL_ERROR_FALLBACK_TEXT: &str = "模型请求失败，请稍后重试。";
const CHANNEL_DISPLAY_QUESTION_OVERRIDE_KEY: &str = "_channel_display_question";
const CHANNEL_COMPACTION_NOTICE_TEXT: &str = "上下文较长，正在整理对话上下文，请稍候。";
const CHANNEL_APPROVAL_PROMPT: &str =
    "请回复数字：1 同意一次，2 同意本会话，3 拒绝（也可发送 /stop 取消当前任务）。";

#[derive(Debug, Clone)]
struct ChannelApprovalContext {
    session_id: String,
    channel: String,
    account_id: String,
    peer: crate::channels::types::ChannelPeer,
    thread: Option<crate::channels::types::ChannelThread>,
    binding_id: Option<String>,
    source_message_id: Option<String>,
    weixin_context_token: Option<String>,
    actor_id: String,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ChannelSessionStrategy {
    MainThread,
    PerPeer,
    Hybrid,
}

impl ChannelSessionStrategy {
    fn from_config(config: &Config) -> Self {
        Self::from_raw(&config.channels.session_strategy)
    }

    fn from_raw(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            SESSION_STRATEGY_PER_PEER => Self::PerPeer,
            SESSION_STRATEGY_HYBRID => Self::Hybrid,
            SESSION_STRATEGY_MAIN_THREAD => Self::MainThread,
            _ => Self::MainThread,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ChannelCommand {
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
    pub(super) fn as_str(self) -> &'static str {
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
pub struct ChannelHubSharedState {
    pub monitor: Arc<MonitorState>,
    pub approval_registry: Arc<PendingApprovalRegistry>,
    pub bridge_runtime: BridgeRuntime,
}

#[derive(Clone)]
pub struct ChannelHub {
    config_store: crate::config_store::ConfigStore,
    storage: Arc<dyn StorageBackend>,
    orchestrator: Arc<Orchestrator>,
    thread_runtime: Arc<ThreadRuntime>,
    user_store: Arc<UserStore>,
    workspace: Arc<WorkspaceManager>,
    monitor: Arc<MonitorState>,
    bridge_runtime: BridgeRuntime,
    rate_limiter: ChannelRateLimiter,
    adapter_registry: ChannelAdapterRegistry,
    http: reqwest::Client,
    recent_inbound: Arc<Mutex<HashMap<String, f64>>>,
    approval_registry: Arc<PendingApprovalRegistry>,
    runtime_logs: Arc<Mutex<ChannelRuntimeLogBuffer>>,
    inbound_queue_tx: TokioSender<ChannelInboundEnvelope>,
    stream_events: Arc<StreamEventService>,
}

impl ChannelHub {
    pub fn new(
        config_store: crate::config_store::ConfigStore,
        storage: Arc<dyn StorageBackend>,
        orchestrator: Arc<Orchestrator>,
        thread_runtime: Arc<ThreadRuntime>,
        user_store: Arc<UserStore>,
        workspace: Arc<WorkspaceManager>,
        shared_state: ChannelHubSharedState,
    ) -> Self {
        let (inbound_queue_tx, inbound_queue_rx) =
            new_inbound_channel(CHANNEL_INBOUND_QUEUE_CAPACITY);
        let stream_events = Arc::new(StreamEventService::new(storage.clone()));
        let hub = Self {
            config_store,
            storage,
            orchestrator,
            thread_runtime,
            user_store,
            workspace,
            monitor: shared_state.monitor,
            bridge_runtime: shared_state.bridge_runtime,
            rate_limiter: ChannelRateLimiter::new(),
            adapter_registry: build_default_channel_adapter_registry(),
            http: reqwest::Client::new(),
            recent_inbound: Arc::new(Mutex::new(HashMap::new())),
            approval_registry: shared_state.approval_registry,
            runtime_logs: Arc::new(Mutex::new(ChannelRuntimeLogBuffer::new(
                CHANNEL_RUNTIME_LOG_CAPACITY,
                CHANNEL_RUNTIME_LOG_FLOOD_WINDOW_S,
            ))),
            inbound_queue_tx,
            stream_events,
        };
        let inbound_worker = hub.clone();
        let inbound_processor: ChannelInboundProcessor = Arc::new(move |envelope| {
            let inbound_worker = inbound_worker.clone();
            async move { inbound_worker.process_inbound_envelope(envelope).await }.boxed()
        });
        spawn_dispatcher(
            inbound_queue_rx,
            CHANNEL_INBOUND_MAX_IN_FLIGHT,
            inbound_processor,
        );
        let worker = hub.clone();
        long_task::spawn("channels.outbox.loop", async move {
            worker.outbox_loop().await;
        });
        let feishu_worker = hub.clone();
        long_task::spawn("channels.long_connection.feishu.supervisor", async move {
            feishu_worker.feishu_long_connection_supervisor_loop().await;
        });
        let qqbot_worker = hub.clone();
        long_task::spawn("channels.long_connection.qqbot.supervisor", async move {
            qqbot_worker.qqbot_long_connection_supervisor_loop().await;
        });
        let xmpp_worker = hub.clone();
        long_task::spawn("channels.long_connection.xmpp.supervisor", async move {
            xmpp_worker.xmpp_long_connection_supervisor_loop().await;
        });
        let weixin_worker = hub.clone();
        long_task::spawn("channels.long_connection.weixin.supervisor", async move {
            weixin_worker.weixin_long_connection_supervisor_loop().await;
        });
        let bootstrap_worker = hub.clone();
        long_task::spawn("channels.runtime.bootstrap_log", async move {
            bootstrap_worker.runtime_bootstrap_log_once().await;
        });
        hub
    }

    pub fn adapter_registry(&self) -> ChannelAdapterRegistry {
        self.adapter_registry.clone()
    }

    pub fn force_xmpp_reconnect(&self, account_id: &str) -> Result<()> {
        let cleaned = account_id.trim();
        if cleaned.is_empty() {
            return Err(anyhow!("xmpp account_id is empty"));
        }
        let current = self
            .storage
            .get_channel_account(xmpp::XMPP_CHANNEL, cleaned)?
            .ok_or_else(|| anyhow!("xmpp account not found: {cleaned}"))?;
        let refreshed = ChannelAccountRecord {
            updated_at: now_ts(),
            ..current
        };
        self.storage.upsert_channel_account(&refreshed)?;
        self.record_runtime_info(
            xmpp::XMPP_CHANNEL,
            Some(cleaned),
            "reconnect_requested",
            format!("xmpp reconnect requested: account_id={cleaned}"),
        );
        Ok(())
    }

    pub fn record_runtime_info(
        &self,
        channel: &str,
        account_id: Option<&str>,
        event: &str,
        message: impl Into<String>,
    ) {
        self.record_runtime_log(
            ChannelRuntimeLogLevel::Info,
            channel,
            account_id,
            event,
            message.into(),
        );
    }

    pub fn record_runtime_warn(
        &self,
        channel: &str,
        account_id: Option<&str>,
        event: &str,
        message: impl Into<String>,
    ) {
        self.record_runtime_log(
            ChannelRuntimeLogLevel::Warn,
            channel,
            account_id,
            event,
            message.into(),
        );
    }

    pub fn record_runtime_error(
        &self,
        channel: &str,
        account_id: Option<&str>,
        event: &str,
        message: impl Into<String>,
    ) {
        self.record_runtime_log(
            ChannelRuntimeLogLevel::Error,
            channel,
            account_id,
            event,
            message.into(),
        );
    }

    pub fn list_runtime_logs(
        &self,
        channel: Option<&str>,
        account_id: Option<&str>,
        limit: usize,
    ) -> Vec<ChannelRuntimeLogEntry> {
        self.runtime_logs.lock().list(channel, account_id, limit)
    }

    fn record_runtime_log(
        &self,
        level: ChannelRuntimeLogLevel,
        channel: &str,
        account_id: Option<&str>,
        event: &str,
        message: String,
    ) {
        self.runtime_logs.lock().push(
            level,
            channel,
            account_id.unwrap_or_default(),
            event,
            &message,
            now_ts(),
        );
    }

    async fn process_inbound_envelope(&self, envelope: ChannelInboundEnvelope) -> Result<()> {
        let ChannelInboundEnvelope {
            provider,
            headers,
            messages,
            raw_payload,
        } = envelope;
        let result = self
            .handle_inbound(&provider, &headers, messages, raw_payload)
            .await?;
        if !result.errors.is_empty() {
            for item in &result.errors {
                self.record_runtime_warn(
                    &provider,
                    None,
                    "inbound_worker_rejected",
                    format!("channel inbound worker rejected message: {item}"),
                );
            }
            warn!(
                "channel inbound worker rejected messages: provider={}, errors={}",
                provider,
                result.errors.join(" | ")
            );
        }
        Ok(())
    }

    pub async fn enqueue_inbound(
        &self,
        provider: &str,
        headers: &HeaderMap,
        messages: Vec<ChannelMessage>,
        raw_payload: Option<Value>,
    ) -> Result<ChannelHandleResult> {
        if messages.is_empty() {
            return Ok(ChannelHandleResult {
                accepted: 0,
                session_ids: Vec::new(),
                outbox_ids: Vec::new(),
                errors: Vec::new(),
            });
        }
        let accepted = messages.len();
        let envelope = ChannelInboundEnvelope {
            provider: provider.to_string(),
            headers: headers.clone(),
            messages,
            raw_payload,
        };
        enqueue_with_timeout(
            &self.inbound_queue_tx,
            envelope,
            CHANNEL_INBOUND_ENQUEUE_TIMEOUT_MS,
        )
        .await?;
        Ok(ChannelHandleResult {
            accepted,
            session_ids: Vec::new(),
            outbox_ids: Vec::new(),
            errors: Vec::new(),
        })
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
        let public_base_url = feishu_files::resolve_public_base_url(headers, &config);
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
        let bridge_resolution = if resolved_binding.is_none() && bound_user_id.is_none() {
            resolve_inbound_bridge_route(&self.bridge_runtime, &message).await?
        } else {
            None
        };
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
            .or_else(|| {
                bridge_resolution
                    .as_ref()
                    .map(|route| route.route.agent_id.clone())
            })
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
        if let Some(route) = bridge_resolution.as_ref() {
            session_strategy = ChannelSessionStrategy::from_raw(&route.session_strategy);
        }
        if message
            .channel
            .trim()
            .eq_ignore_ascii_case(feishu::FEISHU_CHANNEL)
        {
            session_strategy = ChannelSessionStrategy::MainThread;
        }
        if bound_user_id.is_none() {
            bound_user_id = bridge_resolution
                .as_ref()
                .map(|route| route.route.wunder_user_id.clone());
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
                bridge_resolution
                    .as_ref()
                    .map(build_bridge_session_metadata),
            )
            .await?;
        self.touch_chat_session_activity(&session_info.user_id, &session_info.session_id)
            .await;
        if let Some(route) = bridge_resolution.as_ref() {
            self.persist_bridge_inbound(route, &message, &session_info.session_id)
                .await;
        }

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
            debug!(
                "channel agent fallback: channel={}, account_id={}, peer_kind={}, peer_id={}, sender_id={}, session_id={}, user_id={}",
                message.channel,
                message.account_id,
                message.peer.kind,
                message.peer.id,
                message
                    .sender
                    .as_ref()
                    .map(|sender| sender.id.as_str())
                    .unwrap_or_default(),
                session_info.session_id,
                session_info.user_id
            );
        }
        let agent_display_name =
            resolve_channel_agent_display_name(agent_record.as_ref(), resolved_agent_id.as_deref());

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
                    bridge_resolution.as_ref(),
                    resolved_agent_id.as_deref(),
                    &tool_names,
                    account_cfg.tts_enabled,
                    account_cfg.tts_voice.as_deref(),
                    session_strategy,
                )
                .await;
        }

        if let Some(result) = self
            .handle_channel_approval_response(&message, &session_info, resolved_binding.as_ref())
            .await?
        {
            return Ok(result);
        }
        let inbound_has_meaningful_text = has_meaningful_channel_text(message.text.as_deref());

        let limiter_key = format!("{}:{}", message.channel, message.account_id);
        let rate_cfg = resolve_rate_limit(&config.channels.rate_limit, &message.channel);
        let _rate_guard = match self.rate_limiter.acquire(&limiter_key, rate_cfg) {
            Some(guard) => guard,
            None => {
                return self
                    .respond_busy(
                        &message,
                        &session_info,
                        resolved_binding.as_ref(),
                        true,
                        Some(agent_display_name.as_str()),
                        None,
                    )
                    .await;
            }
        };
        let display_question = message
            .text
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if let Some(content) = display_question.as_deref() {
            // Push the inbound user turn to the live session stream before model execution
            // starts so channel-originated messages render immediately in the active thread.
            let _ = self
                .append_channel_stream_event_message(
                    &session_info.user_id,
                    &session_info.session_id,
                    "user",
                    content,
                )
                .await;
        }

        let mut processing_ack_message_id = None;
        if message
            .channel
            .trim()
            .eq_ignore_ascii_case(feishu::FEISHU_CHANNEL)
        {
            if let Some(feishu_cfg) = account_cfg.feishu.as_ref() {
                match self
                    .send_processing_ack(
                        &message,
                        &session_info,
                        resolved_binding.as_ref(),
                        feishu_cfg,
                    )
                    .await
                {
                    Ok(message_id) => {
                        processing_ack_message_id = message_id;
                    }
                    Err(err) => {
                        warn!(
                            "send channel processing ack failed: channel={}, account_id={}, session_id={}, error={err}",
                            message.channel,
                            message.account_id,
                            session_info.session_id
                        );
                    }
                }
            }
        }
        if message
            .channel
            .trim()
            .eq_ignore_ascii_case(weixin::WEIXIN_CHANNEL)
        {
            if let Some(weixin_cfg) = account_cfg.weixin.as_ref() {
                if inbound_has_meaningful_text {
                    if let Err(err) = self
                        .send_weixin_processing_ack(
                            &message,
                            &session_info,
                            resolved_binding.as_ref(),
                            weixin_cfg,
                            agent_display_name.as_str(),
                        )
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
                if let Err(err) = weixin_files::download_weixin_attachments_to_workspace(
                    &self.http,
                    &self.workspace,
                    &self.user_store,
                    weixin_cfg,
                    &session_info.user_id,
                    resolved_agent_id.as_deref(),
                    &mut message,
                )
                .await
                {
                    warn!(
                        "download weixin attachments failed: channel={}, account_id={}, session_id={}, error={err}",
                        message.channel, message.account_id, session_info.session_id
                    );
                }
            }
        }
        if message
            .channel
            .trim()
            .eq_ignore_ascii_case(xmpp::XMPP_CHANNEL)
        {
            if let Some(xmpp_cfg) = account_cfg.xmpp.as_ref() {
                if let Err(err) = self
                    .send_xmpp_processing_ack(
                        &message,
                        &session_info,
                        resolved_binding.as_ref(),
                        xmpp_cfg,
                    )
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
        if message
            .channel
            .trim()
            .eq_ignore_ascii_case(feishu::FEISHU_CHANNEL)
        {
            if let Some(feishu_cfg) = account_cfg.feishu.as_ref() {
                if let Err(err) = feishu_files::download_feishu_attachments_to_workspace(
                    &self.http,
                    &self.workspace,
                    &self.user_store,
                    feishu_cfg,
                    &session_info.user_id,
                    resolved_agent_id.as_deref(),
                    &mut message,
                )
                .await
                {
                    warn!(
                        "download feishu attachments failed: channel={}, account_id={}, session_id={}, error={err}",
                        message.channel, message.account_id, session_info.session_id
                    );
                }
            }
        }
        if message
            .channel
            .trim()
            .eq_ignore_ascii_case(xmpp::XMPP_CHANNEL)
        {
            if let Err(err) = feishu_files::download_remote_attachments_to_workspace(
                &self.http,
                &self.workspace,
                &self.user_store,
                &session_info.user_id,
                resolved_agent_id.as_deref(),
                xmpp::XMPP_CHANNEL,
                &mut message,
            )
            .await
            {
                warn!(
                    "download xmpp attachments failed: channel={}, account_id={}, session_id={}, error={err}",
                    message.channel, message.account_id, session_info.session_id
                );
            }
        }
        if message
            .channel
            .trim()
            .eq_ignore_ascii_case(qqbot::QQBOT_CHANNEL)
        {
            if let Err(err) = feishu_files::download_remote_attachments_to_workspace(
                &self.http,
                &self.workspace,
                &self.user_store,
                &session_info.user_id,
                resolved_agent_id.as_deref(),
                qqbot::QQBOT_CHANNEL,
                &mut message,
            )
            .await
            {
                warn!(
                    "download qqbot attachments failed: channel={}, account_id={}, session_id={}, error={err}",
                    message.channel, message.account_id, session_info.session_id
                );
            }
        }

        let media_processor = MediaProcessor::new(config.channels.media.clone());
        let mut pending_files = match self.load_pending_channel_files(&message).await {
            Ok(value) => value,
            Err(err) => {
                warn!(
                    "load pending channel files failed: channel={}, account_id={}, session_id={}, error={err}",
                    message.channel, message.account_id, session_info.session_id
                );
                Vec::new()
            }
        };
        let incoming_files = build_pending_files_from_attachments(&message.attachments, now_ts());
        if !incoming_files.is_empty() {
            pending_files = merge_pending_files(pending_files, incoming_files.clone());
            if let Err(err) = self
                .save_pending_channel_files(&message, &pending_files)
                .await
            {
                warn!(
                    "save pending channel files failed: channel={}, account_id={}, session_id={}, error={err}",
                    message.channel, message.account_id, session_info.session_id
                );
            }
        }

        let has_user_text = inbound_has_meaningful_text;
        if !has_user_text {
            if !incoming_files.is_empty() {
                let user_text = format_pending_upload_preview(&incoming_files);
                self.append_channel_chat(
                    &session_info.user_id,
                    &session_info.session_id,
                    "user",
                    &user_text,
                )
                .await;
                self.monitor.record_event(
                    &session_info.session_id,
                    "channel_file_buffered",
                    &json!({
                        "channel": message.channel,
                        "account_id": message.account_id,
                        "count": incoming_files.len(),
                        "pending_total": pending_files.len(),
                    }),
                );
            }
            return Ok(ChannelInboundResult {
                session_id: session_info.session_id.clone(),
                outbox_id: None,
            });
        }

        let question = build_channel_question_with_files(message.text.as_deref(), &pending_files);
        let config_overrides = merge_channel_request_overrides(
            channel_test_request_overrides(),
            display_question.as_deref(),
        );
        let mut meta_probe_message = message.clone();
        meta_probe_message.attachments.clear();
        meta_probe_message.location = None;
        let media_probe = media_processor
            .process_inbound(&meta_probe_message, false)
            .await;
        let _probe_text_len = media_probe.text.len();
        let _probe_attachment_count = media_probe.attachments.len();
        let media_meta = media_probe.meta;
        let mut meta = json!({
            "pending_files_total": pending_files.len(),
            "incoming_files": incoming_files.len(),
        });
        merge_object_value_into(&mut meta, media_meta);

        let agent_prompt = agent_record
            .as_ref()
            .map(|record| record.system_prompt.trim().to_string())
            .filter(|value| !value.is_empty());
        let preview_skill = agent_record
            .as_ref()
            .map(|record| record.preview_skill)
            .unwrap_or(false);

        let mut request = WunderRequest {
            user_id: session_info.user_id.clone(),
            question,
            client_message_id: None,
            tool_names: tool_names.clone(),
            skip_tool_calls: false,
            stream: true,
            debug_payload: false,
            session_id: Some(session_info.session_id.clone()),
            agent_id: resolved_agent_id.clone(),
            workspace_container_id: None,
            model_name: None,
            language: Some(crate::i18n::get_language()),
            config_overrides,
            agent_prompt,
            preview_skill,
            attachments: None,
            allow_queue: false,
            is_admin: false,
            enforce_runtime_queue: false,
            approval_tx: None,
        };
        let approval_task = if CHANNEL_OPEN_APPROVAL_FOR_TEST {
            None
        } else {
            let approval_context = ChannelApprovalContext {
                session_id: session_info.session_id.clone(),
                channel: message.channel.clone(),
                account_id: message.account_id.clone(),
                peer: message.peer.clone(),
                thread: message.thread.clone(),
                binding_id: resolved_binding
                    .as_ref()
                    .and_then(|item| item.binding_id.clone()),
                source_message_id: message.message_id.clone(),
                weixin_context_token: weixin::extract_inbound_context_token(&message),
                actor_id: resolve_channel_actor_id(&message),
            };
            let (approval_tx, approval_rx) = new_approval_channel();
            request.approval_tx = Some(approval_tx);
            let approval_hub = self.clone();
            let approval_context_clone = approval_context.clone();
            Some(long_task::spawn("channels.approval.forward", async move {
                approval_hub
                    .forward_channel_approval_requests(approval_rx, approval_context_clone)
                    .await;
            }))
        };
        let response = match self
            .run_channel_request(
                request,
                &session_info.user_id,
                &session_info.session_id,
                &message,
                &session_info,
                resolved_binding.as_ref(),
            )
            .await
        {
            Ok(ChannelModelResult::Answer(answer)) => {
                if !pending_files.is_empty() {
                    if let Err(err) = self.save_pending_channel_files(&message, &[]).await {
                        warn!(
                            "clear pending channel files failed: channel={}, account_id={}, session_id={}, error={err}",
                            message.channel, message.account_id, session_info.session_id
                        );
                    } else {
                        pending_files.clear();
                    }
                }
                answer
            }
            Ok(ChannelModelResult::Busy) => {
                if let Some(task) = approval_task.as_ref() {
                    task.abort();
                }
                return self
                    .respond_busy(
                        &message,
                        &session_info,
                        resolved_binding.as_ref(),
                        false,
                        Some(agent_display_name.as_str()),
                        processing_ack_message_id.as_deref(),
                    )
                    .await;
            }
            Err(err) => {
                if let Some(task) = approval_task.as_ref() {
                    task.abort();
                }
                return self
                    .respond_channel_model_error(
                        &message,
                        &session_info,
                        resolved_binding.as_ref(),
                        &err,
                        processing_ack_message_id.as_deref(),
                    )
                    .await;
            }
        };
        if let Some(task) = approval_task.as_ref() {
            task.abort();
        }
        if response.trim().is_empty() {
            warn!(
                "channel response empty: channel={}, account_id={}, peer_id={}",
                message.channel, message.account_id, message.peer.id
            );
        }
        let mut outbound_meta = json!({
            "session_id": session_info.session_id,
            "binding_id": resolved_binding.as_ref().and_then(|b| b.binding_id.clone()),
            "message_id": message.message_id,
            "media": meta,
        });
        if let Some(resolution) = bridge_resolution.as_ref() {
            append_bridge_meta(&mut outbound_meta, resolution);
        }
        if let Some(meta_obj) = outbound_meta.as_object_mut() {
            meta_obj.insert(
                "user_id".to_string(),
                Value::String(session_info.user_id.clone()),
            );
            if let Some(agent_id) = resolved_agent_id.as_ref() {
                meta_obj.insert("agent_id".to_string(), Value::String(agent_id.clone()));
            }
            if !public_base_url.trim().is_empty() {
                meta_obj.insert(
                    "public_base_url".to_string(),
                    Value::String(public_base_url.clone()),
                );
            }
        }
        if let Some(ack_message_id) = processing_ack_message_id.as_deref() {
            if let Some(meta_obj) = outbound_meta.as_object_mut() {
                meta_obj.insert(
                    "processing_ack_message_id".to_string(),
                    Value::String(ack_message_id.to_string()),
                );
            }
        }
        append_weixin_context_token_from_message(&mut outbound_meta, &message);
        let mut outbound = ChannelOutboundMessage {
            channel: message.channel.clone(),
            account_id: message.account_id.clone(),
            peer: message.peer.clone(),
            thread: message.thread.clone(),
            text: Some(response.clone()),
            attachments: Vec::new(),
            meta: Some(outbound_meta),
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
        self.try_deliver_outbox_if_worker_disabled(&outbox_id).await;

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
        session_metadata: Option<Value>,
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
        let session_metadata = merge_object_values(
            existing.as_ref().and_then(|record| record.metadata.clone()),
            session_metadata,
        );
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
                .thread_runtime
                .resolve_main_session_id(&user_id, cleaned_agent)
                .await?
            {
                existing_main
            } else {
                self.thread_runtime
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
        if should_auto_title(&title) {
            if let Some(auto_title) = build_session_title(message.text.as_deref()) {
                title = auto_title;
            }
        }
        let chat_record = ChatSessionRecord {
            session_id: session_id.clone(),
            user_id: user_id.clone(),
            title,
            status: "active".to_string(),
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
        if !use_main_thread {
            if let Some(agent_key) = agent_value
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                match self
                    .thread_runtime
                    .resolve_main_session_id(&user_id, agent_key)
                    .await
                {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        if let Err(err) = self
                            .thread_runtime
                            .set_main_session(
                                &user_id,
                                agent_key,
                                &session_id,
                                "channel_inbound_auto_create",
                            )
                            .await
                        {
                            warn!(
                                "channel inbound failed to set main session: user_id={}, agent_id={}, session_id={}, error={err}",
                                user_id, agent_key, session_id
                            );
                        }
                    }
                    Err(err) => {
                        warn!(
                            "channel inbound failed to resolve main session: user_id={}, agent_id={}, session_id={}, error={err}",
                            user_id, agent_key, session_id
                        );
                    }
                }
            }
        }

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
            metadata: session_metadata,
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

    async fn forward_channel_approval_requests(
        &self,
        mut approval_rx: ApprovalRequestRx,
        context: ChannelApprovalContext,
    ) {
        while let Some(request) = approval_rx.recv().await {
            if let Err(err) = self
                .register_channel_approval_request(request, &context)
                .await
            {
                warn!(
                    "channel approval register failed: session_id={}, channel={}, account_id={}, error={err}",
                    context.session_id, context.channel, context.account_id
                );
            }
        }
    }

    async fn register_channel_approval_request(
        &self,
        request: ApprovalRequest,
        context: &ChannelApprovalContext,
    ) -> Result<()> {
        let ApprovalRequest {
            id,
            kind,
            tool,
            summary,
            respond_to,
            ..
        } = request;
        let approval_id = id.trim().to_string();
        if approval_id.is_empty() {
            let _ = respond_to.send(ApprovalResponse::Deny);
            return Ok(());
        }

        let mut replaced = self
            .approval_registry
            .remove_matching(|entry| {
                entry.source == ApprovalSource::Channel
                    && entry.session_id == context.session_id
                    && entry
                        .channel
                        .as_deref()
                        .map(|channel| channel.eq_ignore_ascii_case(context.channel.as_str()))
                        .unwrap_or(false)
                    && entry.account_id.as_deref() == Some(context.account_id.as_str())
                    && entry.peer_id.as_deref() == Some(context.peer.id.as_str())
                    && normalize_optional_key(entry.thread_id.as_deref())
                        == normalize_optional_key(
                            context.thread.as_ref().map(|item| item.id.as_str()),
                        )
            })
            .await;
        if let Some(previous) = self
            .approval_registry
            .upsert(PendingApprovalEntry {
                approval_id: approval_id.clone(),
                source: ApprovalSource::Channel,
                session_id: context.session_id.clone(),
                request_id: None,
                channel: Some(context.channel.clone()),
                account_id: Some(context.account_id.clone()),
                peer_id: Some(context.peer.id.clone()),
                thread_id: context.thread.as_ref().map(|item| item.id.clone()),
                actor_id: Some(context.actor_id.clone()).filter(|value| !value.trim().is_empty()),
                tool: tool.clone(),
                summary: summary.clone(),
                kind,
                created_at: now_ts(),
                respond_to,
            })
            .await
        {
            replaced.push(previous);
        }
        for stale in replaced {
            let _ = stale.respond_to.send(ApprovalResponse::Deny);
        }

        let summary_preview = truncate_text(summary.trim(), 180);
        let approval_text = if summary_preview.trim().is_empty() {
            format!("检测到敏感操作需要审批。\n{CHANNEL_APPROVAL_PROMPT}")
        } else {
            format!("检测到敏感操作需要审批：{summary_preview}\n{CHANNEL_APPROVAL_PROMPT}")
        };
        let mut outbound_meta = json!({
            "session_id": context.session_id.clone(),
            "binding_id": context.binding_id.clone(),
            "message_id": context.source_message_id.clone(),
            "approval_id": approval_id.clone(),
            "approval_kind": kind,
            "approval_tool": tool,
            "approval_request": true,
        });
        append_weixin_context_token(&mut outbound_meta, context.weixin_context_token.as_deref());
        let outbound = ChannelOutboundMessage {
            channel: context.channel.clone(),
            account_id: context.account_id.clone(),
            peer: context.peer.clone(),
            thread: context.thread.clone(),
            text: Some(approval_text),
            attachments: Vec::new(),
            meta: Some(outbound_meta),
        };
        let outbox_id = match self.enqueue_outbox(&outbound).await {
            Ok(value) => value,
            Err(err) => {
                self.clear_pending_channel_approvals(
                    None,
                    Some(&approval_id),
                    ApprovalResponse::Deny,
                )
                .await;
                return Err(err);
            }
        };
        self.try_deliver_outbox_if_worker_disabled(&outbox_id).await;
        Ok(())
    }

    async fn handle_channel_approval_response(
        &self,
        message: &ChannelMessage,
        session_info: &ChannelSessionInfo,
        resolved_binding: Option<&BindingResolution>,
    ) -> Result<Option<ChannelInboundResult>> {
        let actor_id = resolve_channel_actor_id(message);
        let matched_entries = self
            .approval_registry
            .find_snapshots(|entry| {
                entry.source == ApprovalSource::Channel
                    && entry.session_id == session_info.session_id
                    && entry
                        .channel
                        .as_deref()
                        .map(|channel| channel.eq_ignore_ascii_case(message.channel.trim()))
                        .unwrap_or(false)
                    && entry.account_id.as_deref() == Some(message.account_id.as_str())
                    && entry.peer_id.as_deref() == Some(message.peer.id.as_str())
                    && normalize_optional_key(entry.thread_id.as_deref())
                        == normalize_optional_key(
                            message.thread.as_ref().map(|item| item.id.as_str()),
                        )
            })
            .await;

        if matched_entries.is_empty() {
            return Ok(None);
        }

        let selected_id = matched_entries
            .iter()
            .filter_map(|entry| {
                let actor_matches = actor_id.is_empty()
                    || entry
                        .actor_id
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .is_none()
                    || entry.actor_id.as_deref() == Some(actor_id.as_str());
                actor_matches.then_some((entry.approval_id.clone(), entry.created_at))
            })
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|item| item.0);

        if selected_id.is_none() {
            let reply = "当前审批仅允许消息发起人处理，请联系发起人回复 1/2/3。".to_string();
            let outbox_id = self
                .enqueue_channel_text_reply(message, session_info, resolved_binding, &reply, None)
                .await?;
            return Ok(Some(ChannelInboundResult {
                session_id: session_info.session_id.clone(),
                outbox_id: Some(outbox_id),
            }));
        }

        let Some(approval_id) = selected_id else {
            return Ok(None);
        };
        let Some(decision) = parse_channel_approval_decision(message.text.as_deref()) else {
            let summary = self
                .approval_registry
                .get_snapshot(&approval_id)
                .await
                .map(|entry| truncate_text(entry.summary.trim(), 120))
                .unwrap_or_default();
            let reply = if summary.trim().is_empty() {
                CHANNEL_APPROVAL_PROMPT.to_string()
            } else {
                format!("当前待审批操作：{summary}\n{CHANNEL_APPROVAL_PROMPT}")
            };
            let outbox_id = self
                .enqueue_channel_text_reply(message, session_info, resolved_binding, &reply, None)
                .await?;
            return Ok(Some(ChannelInboundResult {
                session_id: session_info.session_id.clone(),
                outbox_id: Some(outbox_id),
            }));
        };

        let entry = self.approval_registry.remove(&approval_id).await;
        if let Some(item) = entry {
            let _ = item.respond_to.send(decision);
            let reply = match decision {
                ApprovalResponse::ApproveOnce => "已同意一次，正在继续执行。".to_string(),
                ApprovalResponse::ApproveSession => {
                    "已同意本会话同类操作，正在继续执行。".to_string()
                }
                ApprovalResponse::Deny => "已拒绝本次操作。".to_string(),
            };
            let outbox_id = self
                .enqueue_channel_text_reply(
                    message,
                    session_info,
                    resolved_binding,
                    &reply,
                    Some(json!({
                        "approval_id": item.approval_id,
                        "approval_tool": item.tool,
                        "approval_response": true,
                    })),
                )
                .await?;
            return Ok(Some(ChannelInboundResult {
                session_id: session_info.session_id.clone(),
                outbox_id: Some(outbox_id),
            }));
        }

        Ok(None)
    }

    async fn clear_pending_channel_approvals(
        &self,
        session_id: Option<&str>,
        approval_id: Option<&str>,
        decision: ApprovalResponse,
    ) {
        let target_session = session_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let target_approval = approval_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let removed = self
            .approval_registry
            .remove_matching(|entry| {
                if entry.source != ApprovalSource::Channel {
                    return false;
                }
                let session_match = match target_session.as_deref() {
                    Some(session) => entry.session_id == session,
                    None => true,
                };
                let approval_match = match target_approval.as_deref() {
                    Some(approval) => entry.approval_id == approval,
                    None => true,
                };
                session_match && approval_match
            })
            .await;
        for entry in removed {
            let _ = entry.respond_to.send(decision);
        }
    }

    async fn run_channel_request(
        &self,
        request: WunderRequest,
        user_id: &str,
        session_id: &str,
        message: &ChannelMessage,
        session_info: &ChannelSessionInfo,
        resolved_binding: Option<&BindingResolution>,
    ) -> Result<ChannelModelResult> {
        let session_id_owned = session_id.to_string();
        let mut stream = self.orchestrator.stream(request).await?;
        let mut final_answer: Option<String> = None;
        let mut compaction_notice_sent = false;
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
            if !compaction_notice_sent
                && is_compacting_progress_event(&event.event, &event_payload, &event.data)
            {
                if let Err(err) = self
                    .send_channel_compaction_notice(message, session_info, resolved_binding)
                    .await
                {
                    warn!(
                        "send channel compaction notice failed: channel={}, account_id={}, session_id={}, error={err}",
                        message.channel, message.account_id, session_info.session_id
                    );
                }
                compaction_notice_sent = true;
            }
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

    async fn send_channel_compaction_notice(
        &self,
        message: &ChannelMessage,
        session_info: &ChannelSessionInfo,
        resolved_binding: Option<&BindingResolution>,
    ) -> Result<()> {
        let extra_meta = Some(json!({
            "progress_notice": true,
            "progress_stage": "compacting",
            "compaction_notice": true,
        }));
        self.enqueue_channel_text_reply(
            message,
            session_info,
            resolved_binding,
            CHANNEL_COMPACTION_NOTICE_TEXT,
            extra_meta,
        )
        .await?;
        Ok(())
    }

    async fn respond_channel_model_error(
        &self,
        message: &ChannelMessage,
        session_info: &ChannelSessionInfo,
        resolved_binding: Option<&BindingResolution>,
        err: &anyhow::Error,
        processing_ack_message_id: Option<&str>,
    ) -> Result<ChannelInboundResult> {
        let detail = format_channel_model_error_detail(err);
        let reply = format_channel_model_error_reply(err);
        warn!(
            "channel model request failed: channel={}, account_id={}, session_id={}, error={detail}",
            message.channel, message.account_id, session_info.session_id
        );
        self.monitor.record_event(
            &session_info.session_id,
            "channel_model_error",
            &json!({
                "channel": message.channel,
                "account_id": message.account_id,
                "message_id": message.message_id,
                "error": detail.clone(),
            }),
        );
        self.append_channel_chat(
            &session_info.user_id,
            &session_info.session_id,
            "assistant",
            &reply,
        )
        .await;
        let mut extra_meta = json!({
            "model_error": true,
            "error_detail": detail.clone(),
        });
        if let Some(ack_message_id) = processing_ack_message_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if let Some(meta_obj) = extra_meta.as_object_mut() {
                meta_obj.insert(
                    "processing_ack_message_id".to_string(),
                    Value::String(ack_message_id.to_string()),
                );
            }
        }
        let outbox_id = self
            .enqueue_channel_text_reply(
                message,
                session_info,
                resolved_binding,
                &reply,
                Some(extra_meta),
            )
            .await?;
        Ok(ChannelInboundResult {
            session_id: session_info.session_id.clone(),
            outbox_id: Some(outbox_id),
        })
    }
}

#[derive(Debug, Clone)]
pub(super) struct ChannelSessionInfo {
    pub(super) session_id: String,
    pub(super) user_id: String,
    tts_enabled: Option<bool>,
    tts_voice: Option<String>,
}
