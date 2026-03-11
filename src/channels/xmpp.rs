use crate::channels::adapter::{ChannelAdapter, OutboundContext};
use crate::channels::types::{
    ChannelAttachment, ChannelMessage, ChannelOutboundMessage, ChannelPeer, ChannelSender,
    ChannelThread, XmppConfig,
};
use crate::channels::xmpp_tls_connector::{XmppTlsSecurityMode, XmppTlsServerConfig};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use dashmap::DashMap;
use futures::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::future::Future;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{interval, timeout, Duration, Instant, MissedTickBehavior};
use tokio_xmpp::minidom::Element;
use tokio_xmpp::parsers::iq::{Iq, IqType};
use tokio_xmpp::parsers::jid::Jid;
use tokio_xmpp::parsers::message::{Body, Message, MessageType, Thread};
use tokio_xmpp::parsers::ns;
use tokio_xmpp::parsers::ping::Ping;
use tokio_xmpp::parsers::presence::{Presence, Type as PresenceType};
use tokio_xmpp::{AsyncClient, AsyncConfig, Event};
use tracing::warn;

pub const XMPP_CHANNEL: &str = "xmpp";

const XMPP_DEFAULT_STARTTLS_PORT: u16 = 5222;
const XMPP_DEFAULT_DIRECT_TLS_PORT: u16 = 5223;
const XMPP_RUNTIME_SEND_TIMEOUT_S: u64 = 12;
const XMPP_RUNTIME_QUEUE_SIZE: usize = 128;
const XMPP_DIRECT_SEND_TIMEOUT_S: u64 = 20;
const XMPP_HEARTBEAT_DEFAULT_INTERVAL_S: u64 = 60;
const XMPP_HEARTBEAT_DEFAULT_TIMEOUT_S: u64 = 20;
const XMPP_HEARTBEAT_MIN_INTERVAL_S: u64 = 5;
const XMPP_HEARTBEAT_MIN_TIMEOUT_S: u64 = 5;
const XMPP_NS_OOB: &str = "jabber:x:oob";
const XMPP_NS_REFERENCE: &str = "urn:xmpp:reference:0";

#[derive(Debug, Default)]
pub struct XmppAdapter;

#[async_trait]
impl ChannelAdapter for XmppAdapter {
    fn channel(&self) -> &'static str {
        XMPP_CHANNEL
    }

    async fn send_outbound(&self, context: OutboundContext<'_>) -> Result<()> {
        let config = context
            .account_config
            .xmpp
            .as_ref()
            .ok_or_else(|| anyhow!("xmpp config missing"))?;
        send_outbound(&context.account.account_id, context.outbound, config).await
    }

    async fn health_check(
        &self,
        _http: &Client,
        account_config: &crate::channels::types::ChannelAccountConfig,
    ) -> Result<Value> {
        let status = match account_config.xmpp.as_ref() {
            Some(config) if has_long_connection_credentials(config) => "configured",
            Some(_) => "missing_credentials",
            None => "not_configured",
        };
        Ok(json!({
            "status": status,
        }))
    }
}

#[derive(Debug, Clone)]
struct XmppRuntimeSettings {
    jid: Jid,
    password: String,
    server: XmppTlsServerConfig,
    login_bare: String,
    login_node: Option<String>,
    login_domain: String,
    login_resource: Option<String>,
    send_initial_presence: bool,
    status_text: Option<String>,
    muc_nick: Option<String>,
    muc_rooms: Vec<String>,
    heartbeat_enabled: bool,
    heartbeat_interval_s: u64,
    heartbeat_timeout_s: u64,
    respond_ping: bool,
}

#[derive(Clone)]
struct XmppRuntimeDispatcher {
    runtime_id: u64,
    sender: mpsc::Sender<XmppRuntimeCommand>,
}

enum XmppRuntimeCommand {
    SendOutbound {
        outbound: ChannelOutboundMessage,
        respond_to: oneshot::Sender<Result<()>>,
    },
}

static XMPP_RUNTIME_NEXT_ID: AtomicU64 = AtomicU64::new(1);
static XMPP_HEARTBEAT_SEQ: AtomicU64 = AtomicU64::new(1);
static XMPP_RUNTIME_DISPATCHERS: LazyLock<DashMap<String, XmppRuntimeDispatcher>> =
    LazyLock::new(DashMap::new);

struct PendingHeartbeatPing {
    id: String,
    sent_at: Instant,
}

enum HeartbeatIqEvent {
    PingRequest { id: String, from: Option<Jid> },
    PingResponse { id: String, is_error: bool },
}

pub fn long_connection_enabled(config: &XmppConfig) -> bool {
    config.long_connection_enabled.unwrap_or(true)
}

pub fn has_long_connection_credentials(config: &XmppConfig) -> bool {
    required_jid(config).is_some() && required_password(config).is_some()
}

pub async fn send_outbound(
    account_id: &str,
    outbound: &ChannelOutboundMessage,
    config: &XmppConfig,
) -> Result<()> {
    if let Err(runtime_err) = try_send_outbound_via_runtime(account_id, outbound).await {
        if has_runtime_dispatcher(account_id) {
            warn!(
                "xmpp runtime dispatch failed, fallback to direct connection: account_id={}, error={runtime_err}",
                account_id
            );
        }
    } else {
        return Ok(());
    }

    send_outbound_with_new_connection(outbound, config).await
}

pub async fn run_long_connection_session<F, Fut>(
    account_id: &str,
    config: &XmppConfig,
    mut on_message: F,
) -> Result<()>
where
    F: FnMut(ChannelMessage) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let settings = build_runtime_settings(config)?;
    let mut client = build_client(&settings);

    let (command_tx, mut command_rx) = mpsc::channel(XMPP_RUNTIME_QUEUE_SIZE);
    let runtime_id = register_runtime_dispatcher(account_id, command_tx);

    let mut bound_jid: Option<Jid> = None;
    let mut active_muc_nick: Option<String> = settings.muc_nick.clone();
    let mut runtime_online = false;
    let mut command_rx_closed = false;
    let mut pending_heartbeat_ping: Option<PendingHeartbeatPing> = None;
    let mut heartbeat_ticker = if settings.heartbeat_enabled {
        let mut ticker = interval(Duration::from_secs(settings.heartbeat_interval_s));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        Some(ticker)
    } else {
        None
    };
    let mut heartbeat_active_ping = settings.heartbeat_enabled;

    let outcome: Result<()> = async {
        loop {
            tokio::select! {
                maybe_command = command_rx.recv(), if !command_rx_closed => {
                    match maybe_command {
                        Some(XmppRuntimeCommand::SendOutbound { outbound, respond_to }) => {
                            let result = if runtime_online {
                                send_outbound_stanza(&mut client, &settings, &outbound).await
                            } else {
                                Err(anyhow!("xmpp runtime is not online"))
                            };
                            let _ = respond_to.send(result);
                        }
                        None => {
                            command_rx_closed = true;
                        }
                    }
                }
                event = client.next() => {
                    let Some(event) = event else {
                        return Err(anyhow!("xmpp stream ended"));
                    };
                    match event {
                        Event::Online { bound_jid: online_jid, .. } => {
                            runtime_online = true;
                            bound_jid = Some(online_jid.clone());
                            pending_heartbeat_ping = None;
                            heartbeat_active_ping = settings.heartbeat_enabled;

                            if settings.send_initial_presence {
                                send_initial_presence(&mut client, settings.status_text.as_deref()).await?;
                            }

                            if !settings.muc_rooms.is_empty() {
                                let nick = resolve_muc_nick(&settings, Some(&online_jid));
                                join_muc_rooms(&mut client, &settings.muc_rooms, &nick).await?;
                                active_muc_nick = Some(nick);
                            }
                        }
                        Event::Disconnected(err) => {
                            runtime_online = false;
                            return Err(anyhow!("xmpp disconnected: {err}"));
                        }
                        Event::Stanza(stanza) => {
                            if handle_heartbeat_iq_stanza(
                                &mut client,
                                &stanza,
                                &settings,
                                &mut pending_heartbeat_ping,
                                &mut heartbeat_active_ping,
                            ).await? {
                                continue;
                            }
                            if let Some(message) = parse_stanza_message(
                                account_id,
                                stanza,
                                &settings,
                                bound_jid.as_ref(),
                                active_muc_nick.as_deref(),
                            ) {
                                on_message(message).await?;
                            }
                        }
                    }
                }
                _ = async {
                    if let Some(ticker) = heartbeat_ticker.as_mut() {
                        ticker.tick().await;
                    }
                }, if runtime_online
                    && heartbeat_active_ping
                    && pending_heartbeat_ping.is_none()
                    && heartbeat_ticker.is_some() => {
                    let ping_id = next_heartbeat_ping_id(account_id);
                    send_heartbeat_ping(&mut client, &ping_id).await?;
                    pending_heartbeat_ping = Some(PendingHeartbeatPing {
                        id: ping_id,
                        sent_at: Instant::now(),
                    });
                }
                _ = async {
                    if let Some(pending_ping) = pending_heartbeat_ping.as_ref() {
                        tokio::time::sleep_until(
                            pending_ping.sent_at + Duration::from_secs(settings.heartbeat_timeout_s),
                        ).await;
                    }
                }, if runtime_online
                    && heartbeat_active_ping
                    && pending_heartbeat_ping.is_some() => {
                    if let Some(pending_ping) = pending_heartbeat_ping.as_ref() {
                        return Err(anyhow!(
                            "xmpp heartbeat timeout: account_id={}, ping_id={}",
                            account_id,
                            pending_ping.id
                        ));
                    }
                }
            }
        }
    }
    .await;

    unregister_runtime_dispatcher(account_id, runtime_id);
    outcome
}

fn register_runtime_dispatcher(account_id: &str, sender: mpsc::Sender<XmppRuntimeCommand>) -> u64 {
    let runtime_id = XMPP_RUNTIME_NEXT_ID.fetch_add(1, Ordering::Relaxed);
    XMPP_RUNTIME_DISPATCHERS.insert(
        runtime_key(account_id),
        XmppRuntimeDispatcher { runtime_id, sender },
    );
    runtime_id
}

fn unregister_runtime_dispatcher(account_id: &str, runtime_id: u64) {
    let key = runtime_key(account_id);
    let should_remove = XMPP_RUNTIME_DISPATCHERS
        .get(&key)
        .map(|entry| entry.runtime_id == runtime_id)
        .unwrap_or(false);
    if should_remove {
        XMPP_RUNTIME_DISPATCHERS.remove(&key);
    }
}

fn has_runtime_dispatcher(account_id: &str) -> bool {
    XMPP_RUNTIME_DISPATCHERS.contains_key(&runtime_key(account_id))
}

fn runtime_key(account_id: &str) -> String {
    account_id.trim().to_ascii_lowercase()
}

async fn try_send_outbound_via_runtime(
    account_id: &str,
    outbound: &ChannelOutboundMessage,
) -> Result<()> {
    let key = runtime_key(account_id);
    let dispatcher = XMPP_RUNTIME_DISPATCHERS
        .get(&key)
        .map(|entry| entry.clone())
        .ok_or_else(|| anyhow!("xmpp runtime dispatcher not found"))?;

    let (respond_to, wait_result) = oneshot::channel();
    dispatcher
        .sender
        .send(XmppRuntimeCommand::SendOutbound {
            outbound: outbound.clone(),
            respond_to,
        })
        .await
        .map_err(|_| anyhow!("xmpp runtime dispatcher send failed"))?;

    match timeout(
        Duration::from_secs(XMPP_RUNTIME_SEND_TIMEOUT_S),
        wait_result,
    )
    .await
    {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(anyhow!("xmpp runtime dispatcher response channel closed")),
        Err(_) => Err(anyhow!("xmpp runtime dispatcher timed out")),
    }
}

async fn send_outbound_with_new_connection(
    outbound: &ChannelOutboundMessage,
    config: &XmppConfig,
) -> Result<()> {
    let settings = build_runtime_settings(config)?;
    let mut client = build_client(&settings);

    let send_task = async {
        loop {
            let Some(event) = client.next().await else {
                return Err(anyhow!("xmpp stream ended before online"));
            };
            match event {
                Event::Online { .. } => {
                    send_outbound_stanza(&mut client, &settings, outbound).await?;
                    let _ = client.send_end().await;
                    return Ok(());
                }
                Event::Disconnected(err) => {
                    return Err(anyhow!("xmpp disconnected before outbound sent: {err}"));
                }
                Event::Stanza(_) => {}
            }
        }
    };

    timeout(Duration::from_secs(XMPP_DIRECT_SEND_TIMEOUT_S), send_task)
        .await
        .map_err(|_| anyhow!("xmpp outbound timed out"))?
}

fn build_client(settings: &XmppRuntimeSettings) -> AsyncClient<XmppTlsServerConfig> {
    let mut client = AsyncClient::new_with_config(AsyncConfig {
        jid: settings.jid.clone(),
        password: settings.password.clone(),
        server: settings.server.clone(),
    });
    client.set_reconnect(false);
    client
}

fn build_runtime_settings(config: &XmppConfig) -> Result<XmppRuntimeSettings> {
    let jid_raw = required_jid(config).ok_or_else(|| anyhow!("xmpp jid missing"))?;
    let password = required_password(config).ok_or_else(|| anyhow!("xmpp password missing"))?;

    let mut jid = Jid::from_str(jid_raw).map_err(|err| anyhow!("invalid xmpp jid: {err}"))?;

    if let Some(resource) = optional_trimmed(config.resource.as_deref()) {
        jid = Jid::from(
            jid.to_bare()
                .with_resource_str(resource)
                .map_err(|err| anyhow!("invalid xmpp resource: {err}"))?,
        );
    }

    let default_port = if config.direct_tls.unwrap_or(false) {
        XMPP_DEFAULT_DIRECT_TLS_PORT
    } else {
        XMPP_DEFAULT_STARTTLS_PORT
    };

    let server = {
        let host = optional_trimmed(config.host.as_deref()).map(str::to_string);
        let domain = optional_trimmed(config.domain.as_deref()).map(str::to_string);
        let security_mode = if config.trust_self_signed.unwrap_or(true) {
            XmppTlsSecurityMode::TrustSelfSigned
        } else {
            XmppTlsSecurityMode::Strict
        };
        let port = config.port.unwrap_or(default_port);
        if let Some(host) = host.or(domain) {
            XmppTlsServerConfig::manual(host, port, security_mode)
        } else if config.port.is_some() {
            XmppTlsServerConfig::manual(jid.domain().to_string(), port, security_mode)
        } else {
            XmppTlsServerConfig::use_srv(security_mode)
        }
    };

    let login_bare = jid.to_bare().to_string();
    let login_node = jid.node().map(|value| value.as_str().to_string());
    let login_domain = jid.domain().as_str().to_string();
    let login_resource = jid.resource().map(|value| value.as_str().to_string());
    let heartbeat_enabled = config.heartbeat_enabled.unwrap_or(true);
    let heartbeat_interval_s = config
        .heartbeat_interval_s
        .unwrap_or(XMPP_HEARTBEAT_DEFAULT_INTERVAL_S)
        .max(XMPP_HEARTBEAT_MIN_INTERVAL_S);
    let heartbeat_timeout_s = config
        .heartbeat_timeout_s
        .unwrap_or(XMPP_HEARTBEAT_DEFAULT_TIMEOUT_S)
        .max(XMPP_HEARTBEAT_MIN_TIMEOUT_S);

    Ok(XmppRuntimeSettings {
        jid,
        password,
        server,
        login_bare,
        login_node,
        login_domain,
        login_resource,
        send_initial_presence: config.send_initial_presence.unwrap_or(true),
        status_text: optional_trimmed(config.status_text.as_deref()).map(str::to_string),
        muc_nick: optional_trimmed(config.muc_nick.as_deref()).map(str::to_string),
        muc_rooms: normalize_rooms(&config.muc_rooms),
        heartbeat_enabled,
        heartbeat_interval_s,
        heartbeat_timeout_s,
        respond_ping: config.respond_ping.unwrap_or(true),
    })
}

async fn send_initial_presence(
    client: &mut AsyncClient<XmppTlsServerConfig>,
    status_text: Option<&str>,
) -> Result<()> {
    let mut presence = Presence::new(PresenceType::None);
    if let Some(status) = optional_trimmed(status_text) {
        presence.statuses.insert(String::new(), status.to_string());
    }
    client
        .send_stanza(presence.into())
        .await
        .map_err(|err| anyhow!("xmpp send initial presence failed: {err}"))
}

async fn join_muc_rooms(
    client: &mut AsyncClient<XmppTlsServerConfig>,
    rooms: &[String],
    nick: &str,
) -> Result<()> {
    for room in rooms {
        let room = room.trim();
        if room.is_empty() {
            continue;
        }
        let target = format!("{room}/{nick}");
        let presence = Element::builder("presence", ns::DEFAULT_NS)
            .attr("to", target)
            .append(Element::builder("x", "http://jabber.org/protocol/muc").build())
            .build();
        client
            .send_stanza(presence)
            .await
            .map_err(|err| anyhow!("xmpp join muc room failed: room={room}, error={err}"))?;
    }
    Ok(())
}

fn resolve_muc_nick(settings: &XmppRuntimeSettings, bound_jid: Option<&Jid>) -> String {
    if let Some(nick) = optional_trimmed(settings.muc_nick.as_deref()) {
        return nick.to_string();
    }
    if let Some(resource) = bound_jid.and_then(|jid| jid.resource()) {
        return resource.as_str().to_string();
    }
    if let Some(resource) = settings.login_resource.as_deref() {
        return resource.to_string();
    }
    if let Some(node) = settings.login_node.as_deref() {
        return node.to_string();
    }
    "wunder".to_string()
}

async fn send_outbound_stanza(
    client: &mut AsyncClient<XmppTlsServerConfig>,
    settings: &XmppRuntimeSettings,
    outbound: &ChannelOutboundMessage,
) -> Result<()> {
    let target = resolve_outbound_target(outbound);
    let to_jid = resolve_outbound_target_jid(&target, &settings.login_domain)?;

    let mut message = if is_group_peer_kind(&outbound.peer.kind) {
        Message::groupchat(Some(to_jid))
    } else {
        Message::chat(Some(to_jid))
    };

    let text = outbound_text(outbound)?;
    message.bodies.insert(String::new(), Body(text));

    if let Some(thread_id) = outbound
        .thread
        .as_ref()
        .and_then(|thread| optional_trimmed(Some(&thread.id)).map(str::to_string))
    {
        message.thread = Some(Thread(thread_id));
    }

    client
        .send_stanza(message.into())
        .await
        .map_err(|err| anyhow!("xmpp send stanza failed: {err}"))
}

async fn send_heartbeat_ping(
    client: &mut AsyncClient<XmppTlsServerConfig>,
    ping_id: &str,
) -> Result<()> {
    let ping = Iq::from_get(ping_id.to_string(), Ping);
    client
        .send_stanza(ping.into())
        .await
        .map_err(|err| anyhow!("xmpp send heartbeat ping failed: {err}"))
}

async fn send_heartbeat_pong(
    client: &mut AsyncClient<XmppTlsServerConfig>,
    ping_id: &str,
    from: Option<Jid>,
) -> Result<()> {
    let pong = Iq {
        from: None,
        to: from,
        id: ping_id.to_string(),
        payload: IqType::Result(None),
    };
    client
        .send_stanza(pong.into())
        .await
        .map_err(|err| anyhow!("xmpp send heartbeat pong failed: {err}"))
}

async fn handle_heartbeat_iq_stanza(
    client: &mut AsyncClient<XmppTlsServerConfig>,
    stanza: &Element,
    settings: &XmppRuntimeSettings,
    pending_heartbeat_ping: &mut Option<PendingHeartbeatPing>,
    heartbeat_active_ping: &mut bool,
) -> Result<bool> {
    let Some(event) = parse_heartbeat_iq_event(stanza) else {
        return Ok(false);
    };
    match event {
        HeartbeatIqEvent::PingRequest { id, from } => {
            if settings.respond_ping {
                send_heartbeat_pong(client, &id, from).await?;
            }
            Ok(true)
        }
        HeartbeatIqEvent::PingResponse { id, is_error } => {
            let Some(pending_ping) = pending_heartbeat_ping.as_ref() else {
                return Ok(false);
            };
            if pending_ping.id != id {
                return Ok(false);
            }
            *pending_heartbeat_ping = None;
            if is_error {
                // Some servers don't support XEP-0199 active ping; disable active ping to keep compatibility.
                *heartbeat_active_ping = false;
                warn!("xmpp heartbeat ping rejected by remote, disable active ping");
            }
            Ok(true)
        }
    }
}

fn parse_heartbeat_iq_event(stanza: &Element) -> Option<HeartbeatIqEvent> {
    let iq = Iq::try_from(stanza.clone()).ok()?;
    let Iq {
        from, id, payload, ..
    } = iq;
    let id = id.trim();
    if id.is_empty() {
        return None;
    }
    match payload {
        IqType::Get(payload) => {
            if payload.is("ping", ns::PING) {
                Some(HeartbeatIqEvent::PingRequest {
                    id: id.to_string(),
                    from,
                })
            } else {
                None
            }
        }
        IqType::Result(_) => Some(HeartbeatIqEvent::PingResponse {
            id: id.to_string(),
            is_error: false,
        }),
        IqType::Error(_) => Some(HeartbeatIqEvent::PingResponse {
            id: id.to_string(),
            is_error: true,
        }),
        IqType::Set(_) => None,
    }
}

fn next_heartbeat_ping_id(account_id: &str) -> String {
    let seq = XMPP_HEARTBEAT_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("wunder-hb:{}:{seq}", runtime_key(account_id))
}

fn resolve_outbound_target(outbound: &ChannelOutboundMessage) -> String {
    let meta_target = outbound
        .meta
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("xmpp_to"))
        .and_then(Value::as_str)
        .and_then(|value| optional_trimmed(Some(value)).map(str::to_string));
    meta_target.unwrap_or_else(|| outbound.peer.id.trim().to_string())
}

fn resolve_outbound_target_jid(target: &str, login_domain: &str) -> Result<Jid> {
    let target =
        optional_trimmed(Some(target)).ok_or_else(|| anyhow!("xmpp outbound target missing"))?;
    if !target.contains('@') && !target.contains('/') && !target.contains('.') {
        let fallback = format!("{target}@{login_domain}");
        return Jid::from_str(&fallback)
            .map_err(|err| anyhow!("invalid xmpp outbound target: {err}"));
    }
    Jid::from_str(target).map_err(|err| anyhow!("invalid xmpp outbound target: {err}"))
}

fn outbound_text(outbound: &ChannelOutboundMessage) -> Result<String> {
    if let Some(text) = outbound
        .text
        .as_deref()
        .and_then(|value| optional_trimmed(Some(value)).map(str::to_string))
    {
        return Ok(text);
    }

    if let Some(item) = outbound.attachments.first() {
        let kind = item.kind.trim();
        let url = item.url.trim();
        if !kind.is_empty() && !url.is_empty() {
            return Ok(format!("[{kind}] {url}"));
        }
        if !url.is_empty() {
            return Ok(url.to_string());
        }
    }

    Err(anyhow!("xmpp outbound text is empty"))
}

fn parse_stanza_message(
    account_id: &str,
    stanza: Element,
    settings: &XmppRuntimeSettings,
    bound_jid: Option<&Jid>,
    active_muc_nick: Option<&str>,
) -> Option<ChannelMessage> {
    let attachments = extract_stanza_attachments(&stanza);
    let message = Message::try_from(stanza).ok()?;
    parse_inbound_message(
        account_id,
        message,
        attachments,
        settings,
        bound_jid,
        active_muc_nick,
    )
}

fn parse_inbound_message(
    account_id: &str,
    message: Message,
    attachments: Vec<ChannelAttachment>,
    settings: &XmppRuntimeSettings,
    bound_jid: Option<&Jid>,
    active_muc_nick: Option<&str>,
) -> Option<ChannelMessage> {
    let Message {
        from,
        to,
        id,
        type_,
        bodies,
        thread,
        ..
    } = message;

    if matches!(type_, MessageType::Error | MessageType::Headline) {
        return None;
    }

    let from = from?;

    if is_self_message(&from, &type_, settings, bound_jid, active_muc_nick) {
        return None;
    }

    let text = select_message_body(&bodies);
    if text.is_none() && attachments.is_empty() {
        return None;
    }

    let is_group = matches!(type_, MessageType::Groupchat);
    let peer_kind = if is_group { "group" } else { "user" };
    let peer_id = from.to_bare().to_string();

    let sender = if is_group {
        let sender_id = from
            .resource()
            .map(|value| value.as_str().to_string())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| from.to_string());
        Some(ChannelSender {
            id: sender_id.clone(),
            name: Some(sender_id),
        })
    } else {
        let sender_id = from.to_bare().to_string();
        Some(ChannelSender {
            id: sender_id,
            name: from.resource().map(|value| value.as_str().to_string()),
        })
    };

    let thread = thread
        .map(|value| value.0)
        .and_then(normalize_owned_string)
        .map(|id| ChannelThread { id, topic: None });

    let message_id = id.and_then(normalize_owned_string);

    let message_type = if attachments.is_empty() {
        "text".to_string()
    } else if text.is_some() {
        "mixed".to_string()
    } else {
        attachments
            .first()
            .map(|item| item.kind.clone())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "file".to_string())
    };

    Some(ChannelMessage {
        channel: XMPP_CHANNEL.to_string(),
        account_id: account_id.to_string(),
        peer: ChannelPeer {
            kind: peer_kind.to_string(),
            id: peer_id,
            name: None,
        },
        thread,
        message_id,
        sender,
        message_type,
        text,
        attachments,
        location: None,
        ts: None,
        meta: Some(json!({
            "xmpp": {
                "from": from.to_string(),
                "to": to.map(|jid| jid.to_string()),
                "type": xmpp_message_type_name(&type_),
            }
        })),
    })
}

fn extract_stanza_attachments(stanza: &Element) -> Vec<ChannelAttachment> {
    // Support URL-style attachments commonly carried by XEP-0066 OOB and xep-0372 references.
    let mut attachments = Vec::new();
    let mut seen_urls = HashSet::new();

    for node in stanza.children() {
        if node.is("x", XMPP_NS_OOB) {
            if let Some((url, desc)) = extract_oob_url_and_desc(node) {
                push_attachment_from_url(&mut attachments, &mut seen_urls, &url, desc.as_deref());
            }
            continue;
        }
        if node.is("reference", XMPP_NS_REFERENCE) {
            if let Some(url) = node
                .attr("uri")
                .and_then(|value| normalize_owned_string(value.to_string()))
            {
                push_attachment_from_url(&mut attachments, &mut seen_urls, &url, None);
            }
        }
    }

    attachments
}

fn extract_oob_url_and_desc(node: &Element) -> Option<(String, Option<String>)> {
    let mut url = None;
    let mut desc = None;
    for child in node.children() {
        if child.name() == "url" {
            url = normalize_owned_string(child.text());
        } else if child.name() == "desc" {
            desc = normalize_owned_string(child.text());
        }
    }
    url.map(|value| (value, desc))
}

fn push_attachment_from_url(
    attachments: &mut Vec<ChannelAttachment>,
    seen_urls: &mut HashSet<String>,
    url: &str,
    desc: Option<&str>,
) {
    let cleaned = url.trim();
    if !is_attachment_url(cleaned) || !seen_urls.insert(cleaned.to_string()) {
        return;
    }

    let name = desc
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.contains("://"))
        .map(str::to_string)
        .or_else(|| infer_attachment_name_from_url(cleaned));

    attachments.push(ChannelAttachment {
        kind: infer_attachment_kind_from_url(cleaned),
        url: cleaned.to_string(),
        mime: None,
        size: None,
        name,
    });
}

fn is_attachment_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

fn infer_attachment_kind_from_url(value: &str) -> String {
    let lowered = value
        .split('#')
        .next()
        .unwrap_or(value)
        .split('?')
        .next()
        .unwrap_or(value)
        .to_ascii_lowercase();
    if lowered.ends_with(".png")
        || lowered.ends_with(".jpg")
        || lowered.ends_with(".jpeg")
        || lowered.ends_with(".gif")
        || lowered.ends_with(".webp")
        || lowered.ends_with(".bmp")
        || lowered.ends_with(".svg")
    {
        return "image".to_string();
    }
    if lowered.ends_with(".mp3")
        || lowered.ends_with(".wav")
        || lowered.ends_with(".ogg")
        || lowered.ends_with(".opus")
        || lowered.ends_with(".m4a")
        || lowered.ends_with(".aac")
    {
        return "audio".to_string();
    }
    if lowered.ends_with(".mp4")
        || lowered.ends_with(".mov")
        || lowered.ends_with(".avi")
        || lowered.ends_with(".mkv")
        || lowered.ends_with(".webm")
    {
        return "video".to_string();
    }
    "file".to_string()
}

fn infer_attachment_name_from_url(value: &str) -> Option<String> {
    let head = value.split('#').next().unwrap_or(value);
    let head = head.split('?').next().unwrap_or(head);
    let filename = head.rsplit('/').next()?.trim();
    if filename.is_empty() || filename.contains("://") {
        return None;
    }
    Some(filename.to_string())
}

fn select_message_body(bodies: &BTreeMap<String, Body>) -> Option<String> {
    let preferred = ["", "en", "zh-CN", "zh"];
    for key in preferred {
        if let Some(body) = bodies.get(key) {
            if let Some(value) = normalize_owned_string(body.0.clone()) {
                return Some(value);
            }
        }
    }
    for body in bodies.values() {
        if let Some(value) = normalize_owned_string(body.0.clone()) {
            return Some(value);
        }
    }
    None
}

fn is_self_message(
    from: &Jid,
    message_type: &MessageType,
    settings: &XmppRuntimeSettings,
    bound_jid: Option<&Jid>,
    active_muc_nick: Option<&str>,
) -> bool {
    if matches!(message_type, MessageType::Groupchat) {
        let Some(sender_nick) = from.resource().map(|value| value.as_str()) else {
            return false;
        };

        // MUC self messages come from room/nick, so compare nick against known local nick variants.
        if active_muc_nick
            .and_then(|nick| optional_trimmed(Some(nick)))
            .is_some_and(|nick| sender_nick.eq_ignore_ascii_case(nick))
        {
            return true;
        }

        if settings
            .muc_nick
            .as_deref()
            .and_then(|nick| optional_trimmed(Some(nick)))
            .is_some_and(|nick| sender_nick.eq_ignore_ascii_case(nick))
        {
            return true;
        }

        if settings
            .login_resource
            .as_deref()
            .and_then(|resource| optional_trimmed(Some(resource)))
            .is_some_and(|resource| sender_nick.eq_ignore_ascii_case(resource))
        {
            return true;
        }

        if settings
            .login_node
            .as_deref()
            .and_then(|node| optional_trimmed(Some(node)))
            .is_some_and(|node| sender_nick.eq_ignore_ascii_case(node))
        {
            return true;
        }

        if bound_jid
            .and_then(|jid| jid.resource())
            .map(|resource| resource.as_str())
            .is_some_and(|resource| sender_nick.eq_ignore_ascii_case(resource))
        {
            return true;
        }

        return false;
    }

    let from_bare = from.to_bare().to_string();
    if from_bare.eq_ignore_ascii_case(&settings.login_bare) {
        return true;
    }

    bound_jid
        .map(|jid| jid.to_bare().to_string())
        .is_some_and(|jid| from_bare.eq_ignore_ascii_case(&jid))
}

fn xmpp_message_type_name(message_type: &MessageType) -> &'static str {
    match message_type {
        MessageType::Chat => "chat",
        MessageType::Error => "error",
        MessageType::Groupchat => "groupchat",
        MessageType::Headline => "headline",
        MessageType::Normal => "normal",
    }
}

fn is_group_peer_kind(peer_kind: &str) -> bool {
    matches!(
        peer_kind.trim().to_ascii_lowercase().as_str(),
        "group" | "room" | "muc" | "channel"
    )
}

fn required_jid(config: &XmppConfig) -> Option<&str> {
    optional_trimmed(config.jid.as_deref())
}

fn required_password(config: &XmppConfig) -> Option<String> {
    if let Some(password) = optional_trimmed(config.password.as_deref()) {
        return Some(password.to_string());
    }

    // OpenFang-compatible fallback: allow storing only env var name.
    let env_name = optional_trimmed(config.password_env.as_deref())?;
    std::env::var(env_name)
        .ok()
        .and_then(normalize_owned_string)
}

fn normalize_rooms(raw: &[String]) -> Vec<String> {
    raw.iter()
        .filter_map(|room| optional_trimmed(Some(room)).map(str::to_string))
        .collect()
}

fn normalize_owned_string(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn optional_trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static XMPP_ENV_TEST_SEQ: AtomicU64 = AtomicU64::new(1);

    fn build_test_runtime_settings() -> XmppRuntimeSettings {
        XmppRuntimeSettings {
            jid: Jid::from_str("bot@example.com/wunder").unwrap(),
            password: "secret".to_string(),
            server: XmppTlsServerConfig::use_srv(XmppTlsSecurityMode::TrustSelfSigned),
            login_bare: "bot@example.com".to_string(),
            login_node: Some("bot".to_string()),
            login_domain: "example.com".to_string(),
            login_resource: Some("wunder".to_string()),
            send_initial_presence: true,
            status_text: None,
            muc_nick: Some("botnick".to_string()),
            muc_rooms: vec![],
            heartbeat_enabled: true,
            heartbeat_interval_s: 60,
            heartbeat_timeout_s: 20,
            respond_ping: true,
        }
    }

    #[test]
    fn long_connection_enabled_defaults_true() {
        let config = XmppConfig::default();
        assert!(long_connection_enabled(&config));

        let disabled = XmppConfig {
            long_connection_enabled: Some(false),
            ..XmppConfig::default()
        };
        assert!(!long_connection_enabled(&disabled));
    }

    #[test]
    fn credentials_check_requires_jid_and_password() {
        let empty = XmppConfig::default();
        assert!(!has_long_connection_credentials(&empty));

        let only_jid = XmppConfig {
            jid: Some("bot@example.com".to_string()),
            ..XmppConfig::default()
        };
        assert!(!has_long_connection_credentials(&only_jid));

        let full = XmppConfig {
            jid: Some("bot@example.com".to_string()),
            password: Some("secret".to_string()),
            ..XmppConfig::default()
        };
        assert!(has_long_connection_credentials(&full));
    }

    #[test]
    fn credentials_check_supports_password_env_fallback() {
        let env_key = format!(
            "WUNDER_XMPP_TEST_PASSWORD_{}",
            XMPP_ENV_TEST_SEQ.fetch_add(1, Ordering::Relaxed)
        );
        let config = XmppConfig {
            jid: Some("bot@example.com".to_string()),
            password_env: Some(env_key.clone()),
            ..XmppConfig::default()
        };
        assert!(!has_long_connection_credentials(&config));

        std::env::set_var(&env_key, "secret");
        assert!(has_long_connection_credentials(&config));
        std::env::remove_var(&env_key);
    }

    #[test]
    fn resolve_short_target_uses_login_domain() {
        let jid = resolve_outbound_target_jid("alice", "example.com").unwrap();
        assert_eq!(jid.to_string(), "alice@example.com");
    }

    #[test]
    fn parse_group_message_filters_self_by_muc_nick() {
        let settings = build_test_runtime_settings();

        let stanza = Element::builder("message", ns::DEFAULT_NS)
            .attr("from", "room@conference.example.com/botnick")
            .attr("type", "groupchat")
            .append(
                Element::builder("body", ns::DEFAULT_NS)
                    .append("hello")
                    .build(),
            )
            .build();

        let parsed = parse_stanza_message("acc1", stanza, &settings, None, Some("botnick"));
        assert!(parsed.is_none());
    }

    #[test]
    fn parse_oob_attachment_without_body_is_supported() {
        let settings = build_test_runtime_settings();
        let stanza = Element::builder("message", ns::DEFAULT_NS)
            .attr("from", "alice@example.com/mobile")
            .attr("type", "chat")
            .append(
                Element::builder("x", XMPP_NS_OOB)
                    .append(
                        Element::builder("url", XMPP_NS_OOB)
                            .append("https://example.com/files/report.pdf")
                            .build(),
                    )
                    .append(
                        Element::builder("desc", XMPP_NS_OOB)
                            .append("report.pdf")
                            .build(),
                    )
                    .build(),
            )
            .build();

        let parsed = parse_stanza_message("acc1", stanza, &settings, None, None)
            .expect("oob file should be parsed");
        assert!(parsed.text.is_none());
        assert_eq!(parsed.message_type, "file");
        assert_eq!(parsed.attachments.len(), 1);
        assert_eq!(
            parsed.attachments[0].url,
            "https://example.com/files/report.pdf"
        );
        assert_eq!(parsed.attachments[0].name.as_deref(), Some("report.pdf"));
    }

    #[test]
    fn parse_oob_attachment_with_body_marks_mixed_message() {
        let settings = build_test_runtime_settings();
        let stanza = Element::builder("message", ns::DEFAULT_NS)
            .attr("from", "alice@example.com/mobile")
            .attr("type", "chat")
            .append(
                Element::builder("body", ns::DEFAULT_NS)
                    .append("see attachment")
                    .build(),
            )
            .append(
                Element::builder("x", XMPP_NS_OOB)
                    .append(
                        Element::builder("url", XMPP_NS_OOB)
                            .append("https://example.com/images/a.png")
                            .build(),
                    )
                    .build(),
            )
            .build();

        let parsed = parse_stanza_message("acc1", stanza, &settings, None, None)
            .expect("mixed message should be parsed");
        assert_eq!(parsed.text.as_deref(), Some("see attachment"));
        assert_eq!(parsed.message_type, "mixed");
        assert_eq!(parsed.attachments.len(), 1);
        assert_eq!(parsed.attachments[0].kind, "image");
    }

    #[test]
    fn parse_heartbeat_iq_event_supports_ping_request() {
        let stanza = Element::builder("iq", ns::DEFAULT_NS)
            .attr("id", "p1")
            .attr("type", "get")
            .append(Element::builder("ping", ns::PING).build())
            .build();

        let event = parse_heartbeat_iq_event(&stanza).unwrap();
        match event {
            HeartbeatIqEvent::PingRequest { id, from } => {
                assert_eq!(id, "p1");
                assert!(from.is_none());
            }
            _ => panic!("expected ping request"),
        }
    }

    #[test]
    fn parse_heartbeat_iq_event_supports_ping_result() {
        let stanza = Element::builder("iq", ns::DEFAULT_NS)
            .attr("id", "p2")
            .attr("type", "result")
            .build();

        let event = parse_heartbeat_iq_event(&stanza).unwrap();
        match event {
            HeartbeatIqEvent::PingResponse { id, is_error } => {
                assert_eq!(id, "p2");
                assert!(!is_error);
            }
            _ => panic!("expected ping response"),
        }
    }

    #[test]
    fn build_runtime_settings_applies_heartbeat_defaults() {
        let config = XmppConfig {
            jid: Some("bot@example.com".to_string()),
            password: Some("secret".to_string()),
            ..XmppConfig::default()
        };
        let settings = build_runtime_settings(&config).unwrap();
        assert!(settings.heartbeat_enabled);
        assert_eq!(
            settings.heartbeat_interval_s,
            XMPP_HEARTBEAT_DEFAULT_INTERVAL_S
        );
        assert_eq!(
            settings.heartbeat_timeout_s,
            XMPP_HEARTBEAT_DEFAULT_TIMEOUT_S
        );
        assert!(settings.respond_ping);
        match settings.server {
            XmppTlsServerConfig::UseSrv { security_mode } => {
                assert_eq!(security_mode, XmppTlsSecurityMode::TrustSelfSigned);
            }
            _ => panic!("expected srv connector"),
        }
    }

    #[test]
    fn build_runtime_settings_uses_password_env_when_password_missing() {
        let env_key = format!(
            "WUNDER_XMPP_TEST_PASSWORD_{}",
            XMPP_ENV_TEST_SEQ.fetch_add(1, Ordering::Relaxed)
        );
        std::env::set_var(&env_key, "secret-from-env");

        let config = XmppConfig {
            jid: Some("bot@example.com".to_string()),
            password_env: Some(env_key.clone()),
            ..XmppConfig::default()
        };
        let settings = build_runtime_settings(&config).unwrap();
        assert_eq!(settings.password, "secret-from-env");

        std::env::remove_var(&env_key);
    }

    #[test]
    fn build_runtime_settings_respects_strict_tls_mode() {
        let config = XmppConfig {
            jid: Some("bot@example.com".to_string()),
            password: Some("secret".to_string()),
            trust_self_signed: Some(false),
            ..XmppConfig::default()
        };
        let settings = build_runtime_settings(&config).unwrap();
        match settings.server {
            XmppTlsServerConfig::UseSrv { security_mode } => {
                assert_eq!(security_mode, XmppTlsSecurityMode::Strict);
            }
            _ => panic!("expected srv connector"),
        }
    }
}
