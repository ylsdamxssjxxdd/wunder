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
    read_pending_files_from_metadata, write_pending_files_to_metadata, PendingChannelFile,
};
use crate::channels::qqbot;
use crate::channels::rate_limit::ChannelRateLimiter;
use crate::channels::registry::{build_default_channel_adapter_registry, ChannelAdapterRegistry};
use crate::channels::runtime_log::{
    ChannelRuntimeLogBuffer, ChannelRuntimeLogEntry, ChannelRuntimeLogLevel,
};
use crate::channels::types::{
    ChannelAccountConfig, ChannelMessage, ChannelOutboundMessage, FeishuConfig, QqBotConfig,
    WeixinConfig, XmppConfig,
};
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
use crate::monitor::MonitorState;
use crate::orchestrator::Orchestrator;
use crate::schemas::WunderRequest;
use crate::services::bridge::{
    append_bridge_meta, log_bridge_delivery, resolve_inbound_bridge_route,
    touch_bridge_route_after_outbound, BridgeRouteResolution, BridgeRuntime,
};
use crate::services::runtime::thread::ThreadRuntime;
use crate::services::stream_events::StreamEventService;
use crate::storage::{
    ChannelAccountRecord, ChannelBindingRecord, ChannelMessageRecord, ChannelOutboxRecord,
    ChannelSessionRecord, ChatSessionRecord, ListChannelUserBindingsQuery, StorageBackend,
    UpdateChannelOutboxStatusParams, UserAgentRecord,
};
use crate::user_store::UserStore;
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Result};
use axum::http::HeaderMap;
use chrono::{Local, Utc};
use futures::FutureExt;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::mpsc::Sender as TokioSender;
use tokio::time::{sleep, Duration};
use tokio_stream::StreamExt;
use tracing::{debug, warn};
use uuid::Uuid;

mod outbox;
mod support;
use support::{
    append_weixin_context_token, append_weixin_context_token_from_message,
    build_bridge_session_metadata, build_internal_channel_headers, build_session_title,
    channel_test_request_overrides, channels_runtime_enabled, enforce_allowlist,
    equivalent_peer_kinds, extract_bridge_meta_ids, extract_chat_content, extract_session_id,
    format_channel_model_error_detail, format_channel_model_error_reply,
    is_compacting_progress_event, is_direct_peer, merge_channel_request_overrides,
    merge_object_value_into, merge_object_values, message_preview_text, normalize_message,
    normalize_optional_key, now_ts, outbound_preview_text, parse_channel_approval_decision,
    parse_channel_command, resolve_agent_id_by_account, resolve_channel_actor_id,
    resolve_channel_agent_display_name, resolve_rate_limit, resolve_tool_names, should_auto_title,
    truncate_text, validate_inbound_account,
};

const TOOL_OVERRIDE_NONE: &str = "__no_tools__";
const DEFAULT_SESSION_TITLE: &str = "Channel Session";
const SESSION_STRATEGY_MAIN_THREAD: &str = "main_thread";
const SESSION_STRATEGY_PER_PEER: &str = "per_peer";
const SESSION_STRATEGY_HYBRID: &str = "hybrid";
const FEISHU_LONG_CONN_SUPERVISOR_INTERVAL_S: u64 = 10;
const FEISHU_LONG_CONN_RETRY_BASE_S: u64 = 3;
const FEISHU_LONG_CONN_RETRY_MAX_S: u64 = 30;
const QQBOT_LONG_CONN_SUPERVISOR_INTERVAL_S: u64 = 10;
const QQBOT_LONG_CONN_RETRY_BASE_S: u64 = 3;
const QQBOT_LONG_CONN_RETRY_MAX_S: u64 = 30;
const XMPP_LONG_CONN_SUPERVISOR_INTERVAL_S: u64 = 10;
const XMPP_LONG_CONN_RETRY_BASE_S: u64 = 3;
const XMPP_LONG_CONN_RETRY_MAX_S: u64 = 30;
const WEIXIN_LONG_CONN_SUPERVISOR_INTERVAL_S: u64 = 2;
const WEIXIN_LONG_CONN_RETRY_BASE_MS: u64 = 800;
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

#[derive(Debug, Clone)]
struct FeishuLongConnTarget {
    account_id: String,
    updated_at: f64,
    inbound_token: Option<String>,
    config: FeishuConfig,
}

#[derive(Debug, Clone)]
struct QqBotLongConnTarget {
    account_id: String,
    updated_at: f64,
    inbound_token: Option<String>,
    config: QqBotConfig,
}

#[derive(Debug, Clone)]
struct XmppLongConnTarget {
    account_id: String,
    updated_at: f64,
    inbound_token: Option<String>,
    config: XmppConfig,
}

#[derive(Debug, Clone)]
struct WeixinLongConnTarget {
    account_id: String,
    updated_at: f64,
    inbound_token: Option<String>,
    config: WeixinConfig,
}

impl FeishuLongConnTarget {
    fn task_key(&self) -> String {
        format!("{}:{:.3}", self.account_id, self.updated_at)
    }
}

impl XmppLongConnTarget {
    fn task_key(&self) -> String {
        format!("{}:{:.3}", self.account_id, self.updated_at)
    }
}

impl QqBotLongConnTarget {
    fn task_key(&self) -> String {
        format!("{}:{:.3}", self.account_id, self.updated_at)
    }
}

impl WeixinLongConnTarget {
    fn task_key(&self) -> String {
        format!("{}:{:.3}", self.account_id, self.updated_at)
    }
}

#[derive(Debug, Clone, Copy)]
enum ChannelSessionStrategy {
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
        tokio::spawn(async move {
            worker.outbox_loop().await;
        });
        let feishu_worker = hub.clone();
        tokio::spawn(async move {
            feishu_worker.feishu_long_connection_supervisor_loop().await;
        });
        let qqbot_worker = hub.clone();
        tokio::spawn(async move {
            qqbot_worker.qqbot_long_connection_supervisor_loop().await;
        });
        let xmpp_worker = hub.clone();
        tokio::spawn(async move {
            xmpp_worker.xmpp_long_connection_supervisor_loop().await;
        });
        let weixin_worker = hub.clone();
        tokio::spawn(async move {
            weixin_worker.weixin_long_connection_supervisor_loop().await;
        });
        let bootstrap_worker = hub.clone();
        tokio::spawn(async move {
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
            Some(tokio::spawn(async move {
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
                    self.record_runtime_warn(
                        feishu::FEISHU_CHANNEL,
                        None,
                        "long_connection_targets_load_failed",
                        format!("load feishu long connection targets failed: {err}"),
                    );
                    debug!("load feishu long connection targets failed: {err}");
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
                self.record_runtime_warn(
                    feishu::FEISHU_CHANNEL,
                    Some(&record.account_id),
                    "long_connection_credentials_missing",
                    format!(
                        "skip feishu long connection target without app credentials: account_id={}",
                        record.account_id
                    ),
                );
                debug!(
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
                    self.record_runtime_warn(
                        feishu::FEISHU_CHANNEL,
                        Some(&target.account_id),
                        "long_connection_closed",
                        format!(
                            "feishu long connection closed: account_id={}, retry_in={}s",
                            target.account_id, retry_delay_s
                        ),
                    );
                    debug!(
                        "feishu long connection closed: account_id={}, retry_in={}s",
                        target.account_id, retry_delay_s
                    );
                }
                Err(err) => {
                    self.record_runtime_warn(
                        feishu::FEISHU_CHANNEL,
                        Some(&target.account_id),
                        "long_connection_failed",
                        format!(
                            "feishu long connection failed: account_id={}, retry_in={}s, error={err}",
                            target.account_id, retry_delay_s
                        ),
                    );
                    debug!(
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
            .enqueue_inbound(
                feishu::FEISHU_CHANNEL,
                &headers,
                messages,
                Some(resolved_payload),
            )
            .await?;
        if result.accepted == 0 {
            return Err(anyhow!(
                "feishu long connection inbound ignored: no message accepted"
            ));
        }
        Ok(())
    }

    async fn qqbot_long_connection_supervisor_loop(&self) {
        let mut workers: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();
        loop {
            workers.retain(|_, handle| !handle.is_finished());
            let config = self.config_store.get().await;
            if !channels_runtime_enabled(&config) {
                for (_, handle) in workers.drain() {
                    handle.abort();
                }
                sleep(Duration::from_secs(QQBOT_LONG_CONN_SUPERVISOR_INTERVAL_S)).await;
                continue;
            }

            match self.list_qqbot_long_connection_targets().await {
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
                                worker.qqbot_long_connection_worker_loop(target).await;
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
                    self.record_runtime_warn(
                        qqbot::QQBOT_CHANNEL,
                        None,
                        "long_connection_targets_load_failed",
                        format!("load qqbot long connection targets failed: {err}"),
                    );
                    debug!("load qqbot long connection targets failed: {err}");
                }
            }

            sleep(Duration::from_secs(QQBOT_LONG_CONN_SUPERVISOR_INTERVAL_S)).await;
        }
    }

    async fn list_qqbot_long_connection_targets(&self) -> Result<Vec<QqBotLongConnTarget>> {
        let storage = self.storage.clone();
        let accounts = tokio::task::spawn_blocking(move || {
            storage.list_channel_accounts(Some(qqbot::QQBOT_CHANNEL), Some("active"))
        })
        .await
        .unwrap_or_else(|err| Err(anyhow!(err)))?;

        let mut targets = Vec::new();
        for record in accounts {
            let account_cfg = ChannelAccountConfig::from_value(&record.config);
            let Some(qqbot_cfg) = account_cfg.qqbot else {
                continue;
            };
            if !qqbot::long_connection_enabled(&qqbot_cfg) {
                continue;
            }
            if !qqbot::has_long_connection_credentials(&qqbot_cfg) {
                self.record_runtime_warn(
                    qqbot::QQBOT_CHANNEL,
                    Some(&record.account_id),
                    "long_connection_credentials_missing",
                    format!(
                        "skip qqbot long connection target without credentials: account_id={}",
                        record.account_id
                    ),
                );
                debug!(
                    "skip qqbot long connection target without credentials: account_id={}",
                    record.account_id
                );
                continue;
            }
            targets.push(QqBotLongConnTarget {
                account_id: record.account_id,
                updated_at: record.updated_at,
                inbound_token: account_cfg.inbound_token,
                config: qqbot_cfg,
            });
        }

        Ok(targets)
    }

    async fn qqbot_long_connection_worker_loop(&self, target: QqBotLongConnTarget) {
        let intent_candidates = qqbot::resolve_long_connection_intent_candidates(&target.config);
        let mut intent_index = 0_usize;
        let mut retry_delay_s = QQBOT_LONG_CONN_RETRY_BASE_S;
        self.record_runtime_info(
            qqbot::QQBOT_CHANNEL,
            Some(&target.account_id),
            "long_connection_worker_started",
            format!(
                "qqbot long connection worker started: account_id={}, intent_candidates={:?}",
                target.account_id, intent_candidates
            ),
        );
        loop {
            let intents = intent_candidates
                .get(intent_index)
                .copied()
                .unwrap_or_else(|| qqbot::resolve_long_connection_intents(&target.config));
            self.record_runtime_info(
                qqbot::QQBOT_CHANNEL,
                Some(&target.account_id),
                "long_connection_connecting",
                format!(
                    "qqbot long connection connecting: account_id={}, intents={}, intent_level={}/{}",
                    target.account_id,
                    intents,
                    intent_index + 1,
                    intent_candidates.len()
                ),
            );

            let result = qqbot::run_long_connection_session_with_intents(
                &self.http,
                &target.config,
                intents,
                {
                    let worker = self.clone();
                    let event_target = target.clone();
                    move |payload| {
                        let worker = worker.clone();
                        let event_target = event_target.clone();
                        async move {
                            worker
                                .handle_qqbot_long_connection_payload(&event_target, payload)
                                .await
                        }
                    }
                },
            )
            .await;

            match result {
                Ok(()) => {
                    retry_delay_s = QQBOT_LONG_CONN_RETRY_BASE_S;
                    self.record_runtime_warn(
                        qqbot::QQBOT_CHANNEL,
                        Some(&target.account_id),
                        "long_connection_closed",
                        format!(
                            "qqbot long connection closed: account_id={}, retry_in={}s",
                            target.account_id, retry_delay_s
                        ),
                    );
                    debug!(
                        "qqbot long connection closed: account_id={}, retry_in={}s",
                        target.account_id, retry_delay_s
                    );
                }
                Err(err) => {
                    let err_text = err.to_string();
                    let should_try_lower_intent = intent_candidates.len() > 1
                        && intent_index + 1 < intent_candidates.len()
                        && qqbot::should_try_lower_intent_after_error(err_text.as_str());
                    if should_try_lower_intent {
                        let previous_intents = intents;
                        intent_index += 1;
                        let next_intents = intent_candidates[intent_index];
                        retry_delay_s = QQBOT_LONG_CONN_RETRY_BASE_S;
                        self.record_runtime_warn(
                            qqbot::QQBOT_CHANNEL,
                            Some(&target.account_id),
                            "long_connection_intents_downgraded",
                            format!(
                                "qqbot long connection downgrade intents: account_id={}, from={}, to={}, reason={}",
                                target.account_id, previous_intents, next_intents, err_text
                            ),
                        );
                        debug!(
                            "qqbot long connection downgrade intents: account_id={}, from={}, to={}, reason={}",
                            target.account_id, previous_intents, next_intents, err_text
                        );
                    } else {
                        self.record_runtime_warn(
                            qqbot::QQBOT_CHANNEL,
                            Some(&target.account_id),
                            "long_connection_failed",
                            format!(
                                "qqbot long connection failed: account_id={}, retry_in={}s, intents={}, error={}",
                                target.account_id, retry_delay_s, intents, err_text
                            ),
                        );
                        debug!(
                            "qqbot long connection failed: account_id={}, retry_in={}s, intents={}, error={}",
                            target.account_id, retry_delay_s, intents, err_text
                        );
                        retry_delay_s = (retry_delay_s * 2).min(QQBOT_LONG_CONN_RETRY_MAX_S);
                    }
                }
            }

            sleep(Duration::from_secs(retry_delay_s)).await;
        }
    }

    async fn handle_qqbot_long_connection_payload(
        &self,
        target: &QqBotLongConnTarget,
        payload: Value,
    ) -> Result<()> {
        let event_type = qqbot::dispatch_event_type(&payload).unwrap_or_default();
        let messages = match qqbot::extract_dispatch_messages(&payload, &target.account_id) {
            Ok(value) => value,
            Err(err) => {
                self.record_runtime_warn(
                    qqbot::QQBOT_CHANNEL,
                    Some(&target.account_id),
                    "long_connection_dispatch_parse_failed",
                    format!("qqbot long connection dispatch parse failed: event_type={event_type}, error={err}"),
                );
                return Ok(());
            }
        };
        if messages.is_empty() {
            return Ok(());
        }
        let headers = build_internal_channel_headers(target.inbound_token.as_deref())?;
        let result = match self
            .enqueue_inbound(
                qqbot::QQBOT_CHANNEL,
                &headers,
                messages,
                Some(payload.clone()),
            )
            .await
        {
            Ok(value) => value,
            Err(err) => {
                self.record_runtime_error(
                    qqbot::QQBOT_CHANNEL,
                    Some(&target.account_id),
                    "long_connection_inbound_enqueue_failed",
                    format!("qqbot long connection inbound enqueue failed: event_type={event_type}, error={err}"),
                );
                return Ok(());
            }
        };
        if result.accepted == 0 {
            self.record_runtime_warn(
                qqbot::QQBOT_CHANNEL,
                Some(&target.account_id),
                "long_connection_inbound_ignored",
                format!(
                    "qqbot long connection inbound ignored: event_type={event_type}, accepted=0"
                ),
            );
            return Ok(());
        }

        self.record_runtime_info(
            qqbot::QQBOT_CHANNEL,
            Some(&target.account_id),
            "long_connection_inbound_received",
            format!(
                "qqbot long connection inbound accepted: event_type={event_type}, accepted={}",
                result.accepted
            ),
        );
        Ok(())
    }

    async fn xmpp_long_connection_supervisor_loop(&self) {
        let mut workers: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();
        loop {
            workers.retain(|_, handle| !handle.is_finished());
            let config = self.config_store.get().await;
            if !channels_runtime_enabled(&config) {
                for (_, handle) in workers.drain() {
                    handle.abort();
                }
                sleep(Duration::from_secs(XMPP_LONG_CONN_SUPERVISOR_INTERVAL_S)).await;
                continue;
            }

            match self.list_xmpp_long_connection_targets().await {
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
                                worker.xmpp_long_connection_worker_loop(target).await;
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
                    self.record_runtime_warn(
                        xmpp::XMPP_CHANNEL,
                        None,
                        "long_connection_targets_load_failed",
                        format!("load xmpp long connection targets failed: {err}"),
                    );
                    debug!("load xmpp long connection targets failed: {err}");
                }
            }

            sleep(Duration::from_secs(XMPP_LONG_CONN_SUPERVISOR_INTERVAL_S)).await;
        }
    }

    async fn list_xmpp_long_connection_targets(&self) -> Result<Vec<XmppLongConnTarget>> {
        let storage = self.storage.clone();
        let accounts = tokio::task::spawn_blocking(move || {
            storage.list_channel_accounts(Some(xmpp::XMPP_CHANNEL), Some("active"))
        })
        .await
        .unwrap_or_else(|err| Err(anyhow!(err)))?;

        let mut targets = Vec::new();
        for record in accounts {
            let account_cfg = ChannelAccountConfig::from_value(&record.config);
            let Some(xmpp_cfg) = account_cfg.xmpp else {
                continue;
            };
            if !xmpp::long_connection_enabled(&xmpp_cfg) {
                continue;
            }
            if !xmpp::has_long_connection_credentials(&xmpp_cfg) {
                self.record_runtime_warn(
                    xmpp::XMPP_CHANNEL,
                    Some(&record.account_id),
                    "long_connection_credentials_missing",
                    format!(
                        "skip xmpp long connection target without credentials: account_id={}",
                        record.account_id
                    ),
                );
                debug!(
                    "skip xmpp long connection target without credentials: account_id={}",
                    record.account_id
                );
                continue;
            }
            targets.push(XmppLongConnTarget {
                account_id: record.account_id,
                updated_at: record.updated_at,
                inbound_token: account_cfg.inbound_token,
                config: xmpp_cfg,
            });
        }
        Ok(targets)
    }

    async fn xmpp_long_connection_worker_loop(&self, target: XmppLongConnTarget) {
        let mut retry_delay_s = XMPP_LONG_CONN_RETRY_BASE_S;
        loop {
            let result = xmpp::run_long_connection_session(&target.account_id, &target.config, {
                let worker = self.clone();
                let event_target = target.clone();
                move |message| {
                    let worker = worker.clone();
                    let event_target = event_target.clone();
                    async move {
                        worker
                            .handle_xmpp_long_connection_message(&event_target, message)
                            .await
                    }
                }
            })
            .await;

            match result {
                Ok(()) => {
                    retry_delay_s = XMPP_LONG_CONN_RETRY_BASE_S;
                    self.record_runtime_warn(
                        xmpp::XMPP_CHANNEL,
                        Some(&target.account_id),
                        "long_connection_closed",
                        format!(
                            "xmpp long connection closed: account_id={}, retry_in={}s",
                            target.account_id, retry_delay_s
                        ),
                    );
                    debug!(
                        "xmpp long connection closed: account_id={}, retry_in={}s",
                        target.account_id, retry_delay_s
                    );
                }
                Err(err) => {
                    self.record_runtime_warn(
                        xmpp::XMPP_CHANNEL,
                        Some(&target.account_id),
                        "long_connection_failed",
                        format!(
                            "xmpp long connection failed: account_id={}, retry_in={}s, error={err}",
                            target.account_id, retry_delay_s
                        ),
                    );
                    debug!(
                        "xmpp long connection failed: account_id={}, retry_in={}s, error={err}",
                        target.account_id, retry_delay_s
                    );
                    retry_delay_s = (retry_delay_s * 2).min(XMPP_LONG_CONN_RETRY_MAX_S);
                }
            }

            sleep(Duration::from_secs(retry_delay_s)).await;
        }
    }

    async fn handle_xmpp_long_connection_message(
        &self,
        target: &XmppLongConnTarget,
        message: ChannelMessage,
    ) -> Result<()> {
        let headers = build_internal_channel_headers(target.inbound_token.as_deref())?;
        let result = self
            .enqueue_inbound(xmpp::XMPP_CHANNEL, &headers, vec![message], None)
            .await?;
        if result.accepted == 0 {
            return Err(anyhow!(
                "xmpp long connection inbound ignored: no message accepted"
            ));
        }
        Ok(())
    }

    async fn weixin_long_connection_supervisor_loop(&self) {
        let mut workers: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();
        loop {
            workers.retain(|_, handle| !handle.is_finished());
            let config = self.config_store.get().await;
            if !channels_runtime_enabled(&config) {
                for (_, handle) in workers.drain() {
                    handle.abort();
                }
                sleep(Duration::from_secs(WEIXIN_LONG_CONN_SUPERVISOR_INTERVAL_S)).await;
                continue;
            }

            match self.list_weixin_long_connection_targets().await {
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
                                worker.weixin_long_connection_worker_loop(target).await;
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
                    self.record_runtime_warn(
                        weixin::WEIXIN_CHANNEL,
                        None,
                        "long_connection_targets_load_failed",
                        format!("load weixin long connection targets failed: {err}"),
                    );
                    debug!("load weixin long connection targets failed: {err}");
                }
            }

            sleep(Duration::from_secs(WEIXIN_LONG_CONN_SUPERVISOR_INTERVAL_S)).await;
        }
    }

    async fn list_weixin_long_connection_targets(&self) -> Result<Vec<WeixinLongConnTarget>> {
        let storage = self.storage.clone();
        let accounts = tokio::task::spawn_blocking(move || {
            storage.list_channel_accounts(Some(weixin::WEIXIN_CHANNEL), Some("active"))
        })
        .await
        .unwrap_or_else(|err| Err(anyhow!(err)))?;

        let mut targets = Vec::new();
        for record in accounts {
            let account_cfg = ChannelAccountConfig::from_value(&record.config);
            let Some(weixin_cfg) = account_cfg.weixin else {
                continue;
            };
            if !weixin::long_connection_enabled(&weixin_cfg) {
                continue;
            }
            if !weixin::has_long_connection_credentials(&weixin_cfg) {
                self.record_runtime_warn(
                    weixin::WEIXIN_CHANNEL,
                    Some(&record.account_id),
                    "long_connection_credentials_missing",
                    format!(
                        "skip weixin long connection target without credentials: account_id={}",
                        record.account_id
                    ),
                );
                debug!(
                    "skip weixin long connection target without credentials: account_id={}",
                    record.account_id
                );
                continue;
            }
            targets.push(WeixinLongConnTarget {
                account_id: record.account_id,
                updated_at: record.updated_at,
                inbound_token: account_cfg.inbound_token,
                config: weixin_cfg,
            });
        }
        Ok(targets)
    }

    async fn weixin_long_connection_worker_loop(&self, target: WeixinLongConnTarget) {
        let max_failures = weixin::resolve_max_consecutive_failures(&target.config);
        let backoff_ms = weixin::resolve_backoff_ms(&target.config);
        let mut next_poll_timeout_ms = weixin::resolve_poll_timeout_ms(&target.config);
        let mut get_updates_buf = String::new();
        let mut consecutive_failures = 0_u64;

        loop {
            let response = weixin::get_updates(
                &self.http,
                &target.config,
                &get_updates_buf,
                next_poll_timeout_ms,
            )
            .await;

            match response {
                Ok(result) => {
                    if let Some(timeout_ms) =
                        result.longpolling_timeout_ms.filter(|value| *value > 0)
                    {
                        next_poll_timeout_ms = timeout_ms.max(1_000);
                    }
                    if let Some(buf) = result
                        .get_updates_buf
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                    {
                        get_updates_buf = buf.to_string();
                    }

                    let errcode = result.errcode.unwrap_or(0);
                    let has_error = result.ret != 0 || errcode != 0;
                    if has_error {
                        let errmsg = result.errmsg.as_deref().unwrap_or("unknown");
                        self.record_runtime_warn(
                            weixin::WEIXIN_CHANNEL,
                            Some(&target.account_id),
                            "long_connection_poll_failed",
                            format!(
                                "weixin getupdates failed: account_id={}, ret={}, errcode={}, errmsg={}",
                                target.account_id, result.ret, errcode, errmsg
                            ),
                        );
                        if errcode == -14 || result.ret == -14 {
                            self.record_runtime_error(
                                weixin::WEIXIN_CHANNEL,
                                Some(&target.account_id),
                                "long_connection_session_expired",
                                format!(
                                    "weixin login session expired: account_id={}, retry_in={}ms",
                                    target.account_id, backoff_ms
                                ),
                            );
                            sleep(Duration::from_millis(backoff_ms)).await;
                            continue;
                        }
                        consecutive_failures = consecutive_failures.saturating_add(1);
                        if consecutive_failures >= max_failures {
                            self.record_runtime_warn(
                                weixin::WEIXIN_CHANNEL,
                                Some(&target.account_id),
                                "long_connection_backoff",
                                format!(
                                    "weixin getupdates reached failure threshold: account_id={}, failures={}, backoff_ms={}",
                                    target.account_id, consecutive_failures, backoff_ms
                                ),
                            );
                            consecutive_failures = 0;
                            sleep(Duration::from_millis(backoff_ms)).await;
                        } else {
                            sleep(Duration::from_millis(WEIXIN_LONG_CONN_RETRY_BASE_MS)).await;
                        }
                        continue;
                    }

                    consecutive_failures = 0;
                    if result.msgs.is_empty() {
                        continue;
                    }

                    let messages = weixin::extract_inbound_messages(
                        &result.msgs,
                        &target.account_id,
                        &target.config,
                    );
                    if messages.is_empty() {
                        continue;
                    }

                    let headers = match build_internal_channel_headers(
                        target.inbound_token.as_deref(),
                    ) {
                        Ok(value) => value,
                        Err(err) => {
                            self.record_runtime_error(
                                weixin::WEIXIN_CHANNEL,
                                Some(&target.account_id),
                                "long_connection_header_build_failed",
                                format!(
                                    "weixin long connection build headers failed: account_id={}, error={err}",
                                    target.account_id
                                ),
                            );
                            sleep(Duration::from_millis(WEIXIN_LONG_CONN_RETRY_BASE_MS)).await;
                            continue;
                        }
                    };

                    let raw_payload = json!({
                        "ret": result.ret,
                        "errcode": result.errcode,
                        "errmsg": result.errmsg,
                        "msgs_count": result.msgs.len(),
                        "get_updates_buf": result.get_updates_buf,
                    });
                    match self
                        .enqueue_inbound(
                            weixin::WEIXIN_CHANNEL,
                            &headers,
                            messages,
                            Some(raw_payload),
                        )
                        .await
                    {
                        Ok(outcome) => {
                            if outcome.accepted == 0 {
                                self.record_runtime_warn(
                                    weixin::WEIXIN_CHANNEL,
                                    Some(&target.account_id),
                                    "long_connection_inbound_ignored",
                                    format!(
                                        "weixin long connection inbound ignored: account_id={}, accepted=0",
                                        target.account_id
                                    ),
                                );
                            } else {
                                self.record_runtime_info(
                                    weixin::WEIXIN_CHANNEL,
                                    Some(&target.account_id),
                                    "long_connection_inbound_received",
                                    format!(
                                        "weixin long connection inbound accepted: account_id={}, accepted={}",
                                        target.account_id, outcome.accepted
                                    ),
                                );
                            }
                        }
                        Err(err) => {
                            self.record_runtime_error(
                                weixin::WEIXIN_CHANNEL,
                                Some(&target.account_id),
                                "long_connection_inbound_enqueue_failed",
                                format!(
                                    "weixin long connection inbound enqueue failed: account_id={}, error={err}",
                                    target.account_id
                                ),
                            );
                            sleep(Duration::from_millis(WEIXIN_LONG_CONN_RETRY_BASE_MS)).await;
                        }
                    }
                }
                Err(err) => {
                    consecutive_failures = consecutive_failures.saturating_add(1);
                    self.record_runtime_warn(
                        weixin::WEIXIN_CHANNEL,
                        Some(&target.account_id),
                        "long_connection_poll_error",
                        format!(
                            "weixin getupdates error: account_id={}, failures={}/{}, error={err}",
                            target.account_id, consecutive_failures, max_failures
                        ),
                    );
                    if consecutive_failures >= max_failures {
                        self.record_runtime_warn(
                            weixin::WEIXIN_CHANNEL,
                            Some(&target.account_id),
                            "long_connection_backoff",
                            format!(
                                "weixin getupdates entered backoff: account_id={}, failures={}, backoff_ms={}",
                                target.account_id, consecutive_failures, backoff_ms
                            ),
                        );
                        consecutive_failures = 0;
                        sleep(Duration::from_millis(backoff_ms)).await;
                    } else {
                        sleep(Duration::from_millis(WEIXIN_LONG_CONN_RETRY_BASE_MS)).await;
                    }
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
        let stream_event_id = self
            .append_channel_stream_event_message(
                cleaned_user,
                cleaned_session,
                cleaned_role,
                content,
            )
            .await;
        self.append_channel_chat_history(
            cleaned_user,
            cleaned_session,
            cleaned_role,
            content,
            stream_event_id,
        )
        .await;
    }

    async fn append_channel_chat_history(
        &self,
        user_id: &str,
        session_id: &str,
        role: &str,
        content: &str,
        stream_event_id: Option<i64>,
    ) {
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        let cleaned_role = role.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() || cleaned_role.is_empty() {
            return;
        }
        let mut payload = json!({
            "role": cleaned_role,
            "content": content,
            "session_id": cleaned_session,
            "timestamp": Local::now().to_rfc3339(),
        });
        if let Some(event_id) = stream_event_id {
            if let Some(payload_obj) = payload.as_object_mut() {
                payload_obj.insert("stream_event_id".to_string(), json!(event_id));
            }
        }
        let storage = self.storage.clone();
        let user_id = cleaned_user.to_string();
        let outcome = tokio::task::spawn_blocking(move || storage.append_chat(&user_id, &payload))
            .await
            .unwrap_or_else(|err| Err(anyhow!(err)));
        if let Err(err) = outcome {
            warn!(
                "append channel chat history failed: user_id={}, session_id={}, role={}, error={err}",
                cleaned_user, cleaned_session, cleaned_role
            );
        }
    }

    async fn append_channel_stream_event_message(
        &self,
        user_id: &str,
        session_id: &str,
        role: &str,
        content: &str,
    ) -> Option<i64> {
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        let cleaned_role = role.trim().to_ascii_lowercase();
        let cleaned_content = content.trim();
        if cleaned_user.is_empty()
            || cleaned_session.is_empty()
            || cleaned_content.is_empty()
            || cleaned_role.is_empty()
        {
            return None;
        }
        let payload = json!({
            "event": "channel_message",
            "data": {
                "role": cleaned_role,
                "content": cleaned_content,
                "source": "channel_inbound",
            },
            "timestamp": Utc::now().to_rfc3339(),
        });
        let stream_events = self.stream_events.clone();
        let user_id = cleaned_user.to_string();
        let session_id = cleaned_session.to_string();
        let outcome = stream_events
            .append_event(&session_id, &user_id, payload)
            .await;
        match outcome {
            Ok(event_id) => Some(event_id),
            Err(err) => {
                warn!(
                    "append channel stream event failed: user_id={}, session_id={}, role={}, error={err}",
                    cleaned_user, cleaned_session, cleaned_role
                );
                None
            }
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
        append_user_turn: bool,
        agent_display_name: Option<&str>,
        processing_ack_message_id: Option<&str>,
    ) -> Result<ChannelInboundResult> {
        let last_message = self
            .load_latest_user_message(&session_info.user_id, &session_info.session_id)
            .await
            .unwrap_or_default();
        let agent_display_name = agent_display_name
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("智能体");
        let busy_text = if last_message.trim().is_empty() {
            format!("正在忙，请稍后再试（{agent_display_name}）。")
        } else {
            let preview = truncate_text(last_message.trim(), 120);
            format!("正在忙：{preview}（{agent_display_name}）。")
        };
        let user_text = message_preview_text(message);
        if append_user_turn {
            self.append_channel_chat(
                &session_info.user_id,
                &session_info.session_id,
                "user",
                &user_text,
            )
            .await;
        } else {
            self.append_channel_chat_history(
                &session_info.user_id,
                &session_info.session_id,
                "user",
                &user_text,
                None,
            )
            .await;
        }
        self.append_channel_chat(
            &session_info.user_id,
            &session_info.session_id,
            "assistant",
            &busy_text,
        )
        .await;
        let mut outbound_meta = json!({
            "session_id": session_info.session_id,
            "binding_id": resolved_binding.and_then(|b| b.binding_id.clone()),
            "message_id": message.message_id,
            "busy": true,
        });
        if let Some(bridge_meta) = self.load_channel_session_bridge_metadata(message).await? {
            merge_object_value_into(&mut outbound_meta, bridge_meta);
        }
        if let Some(ack_message_id) = processing_ack_message_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if let Some(meta_obj) = outbound_meta.as_object_mut() {
                meta_obj.insert(
                    "processing_ack_message_id".to_string(),
                    Value::String(ack_message_id.to_string()),
                );
            }
        }
        append_weixin_context_token_from_message(&mut outbound_meta, message);
        let outbound = ChannelOutboundMessage {
            channel: message.channel.clone(),
            account_id: message.account_id.clone(),
            peer: message.peer.clone(),
            thread: message.thread.clone(),
            text: Some(busy_text.clone()),
            attachments: Vec::new(),
            meta: Some(outbound_meta),
        };
        let outbox_id = self.enqueue_outbox(&outbound).await?;
        Ok(ChannelInboundResult {
            session_id: session_info.session_id.clone(),
            outbox_id: Some(outbox_id),
        })
    }

    async fn send_processing_ack(
        &self,
        message: &ChannelMessage,
        session_info: &ChannelSessionInfo,
        resolved_binding: Option<&BindingResolution>,
        feishu_cfg: &FeishuConfig,
    ) -> Result<Option<String>> {
        let ack_text = "已收到消息，正在处理中，请稍后。".to_string();
        let mut meta = json!({
            "session_id": session_info.session_id,
            "binding_id": resolved_binding.and_then(|b| b.binding_id.clone()),
            "message_id": message.message_id,
            "processing_ack": true,
        });
        if let Some(bridge_meta) = self.load_channel_session_bridge_metadata(message).await? {
            merge_object_value_into(&mut meta, bridge_meta);
        }
        let outbound = ChannelOutboundMessage {
            channel: message.channel.clone(),
            account_id: message.account_id.clone(),
            peer: message.peer.clone(),
            thread: message.thread.clone(),
            text: Some(ack_text),
            attachments: Vec::new(),
            meta: Some(meta),
        };
        let result = feishu::send_outbound(&self.http, &outbound, feishu_cfg).await?;
        Ok(result.message_id)
    }

    async fn send_weixin_processing_ack(
        &self,
        message: &ChannelMessage,
        session_info: &ChannelSessionInfo,
        resolved_binding: Option<&BindingResolution>,
        weixin_cfg: &WeixinConfig,
        agent_display_name: &str,
    ) -> Result<()> {
        let cleaned_agent_name = agent_display_name.trim();
        let ack_text = if cleaned_agent_name.is_empty() {
            "已收到消息，正在处理中，请稍后。".to_string()
        } else {
            format!("已收到消息，{cleaned_agent_name}正在处理中，请稍后。")
        };
        let mut meta = json!({
            "session_id": session_info.session_id,
            "binding_id": resolved_binding.and_then(|b| b.binding_id.clone()),
            "message_id": message.message_id,
            "processing_ack": true,
        });
        if let Some(bridge_meta) = self.load_channel_session_bridge_metadata(message).await? {
            merge_object_value_into(&mut meta, bridge_meta);
        }
        append_weixin_context_token_from_message(&mut meta, message);
        let outbound = ChannelOutboundMessage {
            channel: message.channel.clone(),
            account_id: message.account_id.clone(),
            peer: message.peer.clone(),
            thread: message.thread.clone(),
            text: Some(ack_text),
            attachments: Vec::new(),
            meta: Some(meta),
        };
        weixin::send_outbound(&self.http, &outbound, weixin_cfg).await?;
        Ok(())
    }

    async fn send_xmpp_processing_ack(
        &self,
        message: &ChannelMessage,
        session_info: &ChannelSessionInfo,
        resolved_binding: Option<&BindingResolution>,
        xmpp_cfg: &XmppConfig,
    ) -> Result<()> {
        let ack_text = "已收到消息，正在处理中，请稍后。".to_string();
        let mut meta = json!({
            "session_id": session_info.session_id,
            "binding_id": resolved_binding.and_then(|b| b.binding_id.clone()),
            "message_id": message.message_id,
            "processing_ack": true,
        });
        if let Some(bridge_meta) = self.load_channel_session_bridge_metadata(message).await? {
            merge_object_value_into(&mut meta, bridge_meta);
        }
        let outbound = ChannelOutboundMessage {
            channel: message.channel.clone(),
            account_id: message.account_id.clone(),
            peer: message.peer.clone(),
            thread: message.thread.clone(),
            text: Some(ack_text),
            attachments: Vec::new(),
            meta: Some(meta),
        };
        xmpp::send_outbound(&message.account_id, &outbound, xmpp_cfg).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_channel_command(
        &self,
        command: ChannelCommand,
        message: &ChannelMessage,
        session_info: &ChannelSessionInfo,
        bridge_resolution: Option<&BridgeRouteResolution>,
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
                    .thread_runtime
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
                        bridge_resolution.map(build_bridge_session_metadata),
                    )
                    .await?;
                (updated.session_id, "已创建新线程。".to_string())
            }
            ChannelCommand::Stop => {
                self.clear_pending_channel_approvals(
                    Some(&session_info.session_id),
                    None,
                    ApprovalResponse::Deny,
                )
                .await;
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
                "可用指令：/new 新建线程，/stop 停止当前会话。若收到审批提示，请回复 1/2/3。"
                    .to_string(),
            ),
        };

        if !command_text.is_empty() {
            self.append_channel_chat(&user_id, &target_session_id, "user", command_text)
                .await;
        }
        self.append_channel_chat(&user_id, &target_session_id, "assistant", &reply_text)
            .await;

        let mut outbound_meta = json!({
            "session_id": target_session_id,
            "command": command.as_str(),
            "message_id": message.message_id,
        });
        if let Some(resolution) = bridge_resolution {
            append_bridge_meta(&mut outbound_meta, resolution);
        }
        append_weixin_context_token_from_message(&mut outbound_meta, message);
        let outbound = ChannelOutboundMessage {
            channel: message.channel.clone(),
            account_id: message.account_id.clone(),
            peer: message.peer.clone(),
            thread: message.thread.clone(),
            text: Some(reply_text.clone()),
            attachments: Vec::new(),
            meta: Some(outbound_meta),
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

    async fn load_pending_channel_files(
        &self,
        message: &ChannelMessage,
    ) -> Result<Vec<PendingChannelFile>> {
        let session = self
            .get_channel_session(
                &message.channel,
                &message.account_id,
                &message.peer.kind,
                &message.peer.id,
                message.thread.as_ref().map(|thread| thread.id.as_str()),
            )
            .await?;
        Ok(read_pending_files_from_metadata(
            session.as_ref().and_then(|record| record.metadata.as_ref()),
        ))
    }

    async fn save_pending_channel_files(
        &self,
        message: &ChannelMessage,
        files: &[PendingChannelFile],
    ) -> Result<()> {
        let Some(mut record) = self
            .get_channel_session(
                &message.channel,
                &message.account_id,
                &message.peer.kind,
                &message.peer.id,
                message.thread.as_ref().map(|thread| thread.id.as_str()),
            )
            .await?
        else {
            return Ok(());
        };
        record.metadata = write_pending_files_to_metadata(record.metadata, files);
        let now = now_ts();
        record.updated_at = now;
        record.last_message_at = now;
        self.upsert_channel_session(&record).await
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

    async fn load_channel_session_bridge_metadata(
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

    async fn load_bridge_resolution_by_ids(
        &self,
        center_id: &str,
        center_account_id: &str,
        route_id: &str,
    ) -> Result<Option<BridgeRouteResolution>> {
        let storage = self.storage.clone();
        let center_id = center_id.trim().to_string();
        let center_account_id = center_account_id.trim().to_string();
        let route_id = route_id.trim().to_string();
        tokio::task::spawn_blocking(move || {
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
        .unwrap_or_else(|err| Err(anyhow!(err)))
    }

    async fn load_bridge_resolution_for_outbox(
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

    async fn on_bridge_outbound_sent(
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

    async fn on_bridge_outbound_failed(
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

    async fn persist_bridge_inbound(
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
        if let Err(err) =
            tokio::task::spawn_blocking(move || storage.upsert_bridge_user_route(&route))
                .await
                .unwrap_or_else(|spawn_err| Err(anyhow!(spawn_err)))
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
