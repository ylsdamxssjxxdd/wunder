#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tokio::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::warn;

const DEFAULT_LISTEN_HOST: &str = "0.0.0.0";
const DEFAULT_LISTEN_PORT: u16 = 18661;
const DEFAULT_DISCOVERY_PORT: u16 = 18662;
const DEFAULT_DISCOVERY_INTERVAL_MS: u64 = 2_500;
const DEFAULT_PEER_TTL_MS: u64 = 15_000;
const DEFAULT_MAX_INBOUND_DEDUP: usize = 4_096;
const MAX_MAX_INBOUND_DEDUP: usize = 32_768;
const LAN_DISCOVERY_PACKET_VERSION: u32 = 1;

const DEFAULT_PRIVATE_CIDRS: [&str; 3] = ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopLanMeshSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub peer_id: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default = "default_listen_host")]
    pub listen_host: String,
    #[serde(default = "default_listen_port")]
    pub listen_port: u16,
    #[serde(default = "default_discovery_port")]
    pub discovery_port: u16,
    #[serde(default = "default_discovery_interval_ms")]
    pub discovery_interval_ms: u64,
    #[serde(default = "default_peer_ttl_ms")]
    pub peer_ttl_ms: u64,
    #[serde(default)]
    pub allow_subnets: Vec<String>,
    #[serde(default)]
    pub deny_subnets: Vec<String>,
    #[serde(default)]
    pub peer_blacklist: Vec<String>,
    #[serde(default)]
    pub shared_secret: String,
    #[serde(default = "default_max_inbound_dedup")]
    pub max_inbound_dedup: usize,
    #[serde(default)]
    pub relay_http_fallback: bool,
    #[serde(default = "default_ws_path")]
    pub peer_ws_path: String,
    #[serde(default = "default_http_path")]
    pub peer_http_path: String,
}

impl Default for DesktopLanMeshSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            peer_id: String::new(),
            display_name: String::new(),
            listen_host: default_listen_host(),
            listen_port: default_listen_port(),
            discovery_port: default_discovery_port(),
            discovery_interval_ms: default_discovery_interval_ms(),
            peer_ttl_ms: default_peer_ttl_ms(),
            allow_subnets: DEFAULT_PRIVATE_CIDRS
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            deny_subnets: Vec::new(),
            peer_blacklist: Vec::new(),
            shared_secret: String::new(),
            max_inbound_dedup: default_max_inbound_dedup(),
            relay_http_fallback: true,
            peer_ws_path: default_ws_path(),
            peer_http_path: default_http_path(),
        }
    }
}

impl DesktopLanMeshSettings {
    pub fn normalized(mut self) -> Self {
        self.listen_host = normalize_host(&self.listen_host);
        self.listen_port = normalize_port(self.listen_port, DEFAULT_LISTEN_PORT);
        self.discovery_port = normalize_port(self.discovery_port, DEFAULT_DISCOVERY_PORT);
        self.discovery_interval_ms = self.discovery_interval_ms.clamp(500, 30_000);
        self.peer_ttl_ms = self
            .peer_ttl_ms
            .max(self.discovery_interval_ms.saturating_mul(3))
            .clamp(2_000, 120_000);
        self.allow_subnets = normalize_cidr_list(&self.allow_subnets);
        if self.allow_subnets.is_empty() {
            self.allow_subnets = DEFAULT_PRIVATE_CIDRS
                .iter()
                .map(|value| (*value).to_string())
                .collect();
        }
        self.deny_subnets = normalize_cidr_list(&self.deny_subnets);
        self.peer_blacklist = normalize_text_list(&self.peer_blacklist, true);
        self.shared_secret = self.shared_secret.trim().to_string();
        self.max_inbound_dedup = self.max_inbound_dedup.clamp(128, MAX_MAX_INBOUND_DEDUP);
        self.peer_ws_path = normalize_route_path(&self.peer_ws_path, default_ws_path());
        self.peer_http_path = normalize_route_path(&self.peer_http_path, default_http_path());
        self
    }

    pub fn allows_ip(&self, ip: IpAddr) -> bool {
        let allow = normalize_cidr_list(&self.allow_subnets);
        let deny = normalize_cidr_list(&self.deny_subnets);
        is_ip_allowed(ip, &allow, &deny)
    }

    pub fn is_peer_blocked(&self, peer_id: &str) -> bool {
        let key = peer_id.trim().to_ascii_lowercase();
        if key.is_empty() {
            return false;
        }
        self.peer_blacklist
            .iter()
            .any(|value| value.eq_ignore_ascii_case(&key))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopLanPeerSnapshot {
    pub peer_id: String,
    pub user_id: String,
    pub display_name: String,
    pub lan_ip: String,
    pub listen_port: u16,
    pub seen_at: f64,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopLanEnvelope {
    pub envelope_id: String,
    pub envelope_type: String,
    pub source_peer_id: String,
    pub source_user_id: String,
    #[serde(default)]
    pub target_peer_id: Option<String>,
    #[serde(default)]
    pub target_user_id: Option<String>,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub sent_at: f64,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DesktopLanDiscoveryPacket {
    version: u32,
    peer_id: String,
    user_id: String,
    display_name: String,
    listen_port: u16,
    timestamp: f64,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    signature: Option<String>,
}

#[derive(Debug)]
struct DesktopLanManagerState {
    settings: DesktopLanMeshSettings,
    peers: HashMap<String, DesktopLanPeerSnapshot>,
    group_links: HashMap<String, String>,
    conversation_links: HashMap<String, String>,
    inbound_seen: HashSet<String>,
    inbound_order: VecDeque<String>,
    bound_port: Option<u16>,
    runtime_key: Option<String>,
    runtime_cancel: Option<CancellationToken>,
}

impl Default for DesktopLanManagerState {
    fn default() -> Self {
        Self {
            settings: DesktopLanMeshSettings::default(),
            peers: HashMap::new(),
            group_links: HashMap::new(),
            conversation_links: HashMap::new(),
            inbound_seen: HashSet::new(),
            inbound_order: VecDeque::new(),
            bound_port: None,
            runtime_key: None,
            runtime_cancel: None,
        }
    }
}

pub struct DesktopLanManager {
    state: RwLock<DesktopLanManagerState>,
}

impl DesktopLanManager {
    fn new() -> Self {
        Self {
            state: RwLock::new(DesktopLanManagerState::default()),
        }
    }

    pub async fn settings(&self) -> DesktopLanMeshSettings {
        self.state.read().await.settings.clone()
    }

    pub async fn apply_settings(&self, settings: DesktopLanMeshSettings) {
        let next = settings.normalized();
        let next_runtime_key = next.enabled.then(|| build_runtime_key(&next));
        let (cancel_to_stop, should_start_runtime, runtime_settings) = {
            let mut guard = self.state.write().await;
            let dedup_cap = next.max_inbound_dedup;
            guard.settings = next.clone();
            trim_inbound_dedup_locked(&mut guard, dedup_cap);
            let peer_blacklist = guard.settings.peer_blacklist.clone();
            guard.peers.retain(|peer_id, _| {
                let key = peer_id.trim().to_ascii_lowercase();
                !peer_blacklist
                    .iter()
                    .any(|blocked| blocked.eq_ignore_ascii_case(&key))
            });

            let should_restart_runtime = if next.enabled {
                guard.runtime_key.as_deref() != next_runtime_key.as_deref()
                    || guard.runtime_cancel.is_none()
            } else {
                false
            };
            let cancel_to_stop = if !next.enabled || should_restart_runtime {
                guard.runtime_cancel.take()
            } else {
                None
            };
            if next.enabled {
                guard.runtime_key = next_runtime_key.clone();
            } else {
                guard.runtime_key = None;
                guard.peers.clear();
            }
            (
                cancel_to_stop,
                next.enabled && should_restart_runtime,
                next.clone(),
            )
        };

        if let Some(cancel) = cancel_to_stop {
            cancel.cancel();
        }
        if should_start_runtime {
            let cancel = CancellationToken::new();
            {
                let mut guard = self.state.write().await;
                guard.runtime_cancel = Some(cancel.clone());
            }
            spawn_discovery_runtime(runtime_settings, cancel);
        }
    }

    pub async fn resolve_bind_target(&self, cli_host: &str, cli_port: u16) -> (String, u16) {
        let guard = self.state.read().await;
        if !guard.settings.enabled {
            return (normalize_host(cli_host), cli_port);
        }
        let host = {
            let normalized_cli_host = normalize_host(cli_host);
            if is_loopback_host(&normalized_cli_host) {
                guard.settings.listen_host.clone()
            } else {
                normalized_cli_host
            }
        };
        let port = if cli_port == 0 {
            guard.settings.listen_port
        } else {
            cli_port
        };
        (host, port)
    }

    pub async fn set_bound_port(&self, port: u16) {
        let mut guard = self.state.write().await;
        if port == 0 {
            return;
        }
        guard.bound_port = Some(port);
    }

    pub async fn bound_port(&self) -> Option<u16> {
        self.state.read().await.bound_port
    }

    pub async fn upsert_peer(&self, mut peer: DesktopLanPeerSnapshot) -> bool {
        let mut guard = self.state.write().await;
        if peer.peer_id.trim().is_empty() {
            return false;
        }
        if guard.settings.is_peer_blocked(&peer.peer_id) {
            guard.peers.remove(peer.peer_id.trim());
            return false;
        }
        let ip = match peer.lan_ip.trim().parse::<IpAddr>() {
            Ok(value) => value,
            Err(_) => return false,
        };
        if !guard.settings.allows_ip(ip) {
            return false;
        }
        peer.peer_id = peer.peer_id.trim().to_string();
        peer.user_id = peer.user_id.trim().to_string();
        peer.display_name = peer.display_name.trim().to_string();
        peer.seen_at = now_ts();
        guard.peers.insert(peer.peer_id.clone(), peer);
        true
    }

    pub async fn remove_peer(&self, peer_id: &str) {
        let mut guard = self.state.write().await;
        guard.peers.remove(peer_id.trim());
    }

    pub async fn list_peers(&self) -> Vec<DesktopLanPeerSnapshot> {
        let mut guard = self.state.write().await;
        purge_stale_peers_locked(&mut guard);
        let mut items = guard.peers.values().cloned().collect::<Vec<_>>();
        items.sort_by(|left, right| {
            right
                .seen_at
                .total_cmp(&left.seen_at)
                .then_with(|| left.peer_id.cmp(&right.peer_id))
        });
        items
    }

    pub async fn register_group_link(&self, global_group_id: &str, conversation_id: &str) {
        let global_group_id = global_group_id.trim();
        let conversation_id = conversation_id.trim();
        if global_group_id.is_empty() || conversation_id.is_empty() {
            return;
        }
        let mut guard = self.state.write().await;
        guard
            .group_links
            .insert(global_group_id.to_string(), conversation_id.to_string());
        guard
            .conversation_links
            .insert(conversation_id.to_string(), global_group_id.to_string());
    }

    pub async fn conversation_id_by_group(&self, global_group_id: &str) -> Option<String> {
        let global_group_id = global_group_id.trim();
        if global_group_id.is_empty() {
            return None;
        }
        self.state
            .read()
            .await
            .group_links
            .get(global_group_id)
            .cloned()
    }

    pub async fn global_group_id_by_conversation(
        &self,
        conversation_id: &str,
        fallback: Option<&str>,
    ) -> Option<String> {
        let conversation_id = conversation_id.trim();
        if conversation_id.is_empty() {
            return fallback
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
        }
        self.state
            .read()
            .await
            .conversation_links
            .get(conversation_id)
            .cloned()
            .or_else(|| {
                fallback
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
    }

    pub async fn accepts_envelope(&self, envelope_id: &str) -> bool {
        let mut guard = self.state.write().await;
        let cleaned = envelope_id.trim();
        if cleaned.is_empty() || guard.inbound_seen.contains(cleaned) {
            return false;
        }
        guard.inbound_seen.insert(cleaned.to_string());
        guard.inbound_order.push_back(cleaned.to_string());
        let cap = guard.settings.max_inbound_dedup;
        trim_inbound_dedup_locked(&mut guard, cap);
        true
    }

    pub async fn clear_runtime_state(&self) {
        let mut guard = self.state.write().await;
        if let Some(cancel) = guard.runtime_cancel.take() {
            cancel.cancel();
        }
        guard.runtime_key = None;
        guard.peers.clear();
        guard.group_links.clear();
        guard.conversation_links.clear();
        guard.inbound_seen.clear();
        guard.inbound_order.clear();
    }
}

fn purge_stale_peers_locked(state: &mut DesktopLanManagerState) {
    let now = now_ts();
    let ttl_s = state.settings.peer_ttl_ms as f64 / 1_000.0;
    state
        .peers
        .retain(|_, value| now - value.seen_at <= ttl_s.max(2.0));
}

fn trim_inbound_dedup_locked(state: &mut DesktopLanManagerState, cap: usize) {
    while state.inbound_order.len() > cap {
        if let Some(oldest) = state.inbound_order.pop_front() {
            state.inbound_seen.remove(oldest.as_str());
        }
    }
}

pub fn manager() -> &'static DesktopLanManager {
    static INSTANCE: OnceLock<DesktopLanManager> = OnceLock::new();
    INSTANCE.get_or_init(DesktopLanManager::new)
}

fn build_runtime_key(settings: &DesktopLanMeshSettings) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}",
        settings.discovery_port,
        settings.discovery_interval_ms,
        settings.listen_port,
        settings.peer_id.trim().to_ascii_lowercase(),
        settings.shared_secret.trim(),
        settings
            .allow_subnets
            .iter()
            .map(|value| value.trim())
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn spawn_discovery_runtime(settings: DesktopLanMeshSettings, cancel: CancellationToken) {
    let recv_settings = settings.clone();
    let recv_cancel = cancel.clone();
    tokio::spawn(async move {
        run_discovery_receiver(recv_settings, recv_cancel).await;
    });

    tokio::spawn(async move {
        run_discovery_sender(settings, cancel).await;
    });
}

async fn run_discovery_receiver(settings: DesktopLanMeshSettings, cancel: CancellationToken) {
    let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), settings.discovery_port);
    let socket = match UdpSocket::bind(bind_addr).await {
        Ok(value) => value,
        Err(err) => {
            warn!("desktop lan receiver bind failed {bind_addr}: {err}; discovery listen disabled");
            return;
        }
    };
    let mut buffer = vec![0u8; 16 * 1024];
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                break;
            }
            result = socket.recv_from(&mut buffer) => {
                let Ok((size, source)) = result else {
                    continue;
                };
                if size == 0 {
                    continue;
                }
                if !settings.allows_ip(source.ip()) {
                    continue;
                }
                let raw = match std::str::from_utf8(&buffer[..size]) {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                let Ok(packet) = serde_json::from_str::<DesktopLanDiscoveryPacket>(raw) else {
                    continue;
                };
                if packet.version != LAN_DISCOVERY_PACKET_VERSION {
                    continue;
                }
                if packet.peer_id.trim().is_empty()
                    || packet.peer_id.trim() == settings.peer_id.trim()
                    || settings.is_peer_blocked(&packet.peer_id)
                {
                    continue;
                }
                if !verify_discovery_packet_signature(&packet, settings.shared_secret.trim()) {
                    continue;
                }
                let snapshot = DesktopLanPeerSnapshot {
                    peer_id: packet.peer_id.trim().to_string(),
                    user_id: packet.user_id.trim().to_string(),
                    display_name: packet.display_name.trim().to_string(),
                    lan_ip: source.ip().to_string(),
                    listen_port: packet.listen_port,
                    seen_at: now_ts(),
                    capabilities: packet.capabilities,
                };
                let _ = manager().upsert_peer(snapshot).await;
            }
        }
    }
}

async fn run_discovery_sender(settings: DesktopLanMeshSettings, cancel: CancellationToken) {
    let socket = match UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)).await
    {
        Ok(value) => value,
        Err(err) => {
            warn!("desktop lan sender bind failed: {err}");
            return;
        }
    };
    if let Err(err) = socket.set_broadcast(true) {
        warn!("desktop lan sender set_broadcast failed: {err}");
        return;
    }

    let mut ticker = tokio::time::interval(Duration::from_millis(
        settings.discovery_interval_ms.max(500),
    ));
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                break;
            }
            _ = ticker.tick() => {
                let current = manager().settings().await;
                if !current.enabled {
                    continue;
                }
                let source_user_id = std::env::var("WUNDER_DESKTOP_USER_ID")
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "desktop_user".to_string());
                let listen_port = manager()
                    .bound_port()
                    .await
                    .unwrap_or(current.listen_port);
                let mut packet = DesktopLanDiscoveryPacket {
                    version: LAN_DISCOVERY_PACKET_VERSION,
                    peer_id: current.peer_id.trim().to_string(),
                    user_id: source_user_id.clone(),
                    display_name: if current.display_name.trim().is_empty() {
                        source_user_id
                    } else {
                        current.display_name.trim().to_string()
                    },
                    listen_port,
                    timestamp: now_ts(),
                    capabilities: vec!["user_world".to_string()],
                    signature: None,
                };
                if !current.shared_secret.trim().is_empty() {
                    packet.signature = Some(sign_discovery_packet(&packet, current.shared_secret.trim()));
                }
                let bytes = match serde_json::to_vec(&packet) {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                let targets = discovery_targets(&current);
                for target in targets {
                    let _ = socket.send_to(&bytes, target).await;
                }
            }
        }
    }
}

fn discovery_targets(settings: &DesktopLanMeshSettings) -> Vec<SocketAddr> {
    let mut targets = HashSet::new();
    for cidr in &settings.allow_subnets {
        let Some(parsed) = parse_cidr(cidr) else {
            continue;
        };
        let ParsedCidr::V4 { network, prefix } = parsed else {
            continue;
        };
        let mask = if prefix == 0 {
            0
        } else if prefix >= 32 {
            u32::MAX
        } else {
            u32::MAX << (32 - prefix)
        };
        let network_raw = u32::from(network);
        let broadcast = Ipv4Addr::from(network_raw | !mask);
        targets.insert(SocketAddr::new(
            IpAddr::V4(broadcast),
            settings.discovery_port,
        ));
    }
    targets.insert(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255)),
        settings.discovery_port,
    ));
    targets.into_iter().collect()
}

fn verify_discovery_packet_signature(
    packet: &DesktopLanDiscoveryPacket,
    shared_secret: &str,
) -> bool {
    let secret = shared_secret.trim();
    if secret.is_empty() {
        return true;
    }
    let provided = packet
        .signature
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(provided) = provided else {
        return false;
    };
    let expected = sign_discovery_packet(packet, secret);
    provided.eq_ignore_ascii_case(&expected)
}

fn sign_discovery_packet(packet: &DesktopLanDiscoveryPacket, shared_secret: &str) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let content = format!(
        "{}|{}|{}|{}|{}|{}",
        packet.version,
        packet.peer_id.trim(),
        packet.user_id.trim(),
        packet.display_name.trim(),
        packet.listen_port,
        packet.timestamp
    );
    let mut mac = match Hmac::<Sha256>::new_from_slice(shared_secret.trim().as_bytes()) {
        Ok(value) => value,
        Err(_) => return String::new(),
    };
    mac.update(content.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn normalize_host(raw: &str) -> String {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return DEFAULT_LISTEN_HOST.to_string();
    }
    cleaned.to_string()
}

fn is_loopback_host(host: &str) -> bool {
    let cleaned = host.trim().to_ascii_lowercase();
    matches!(cleaned.as_str(), "127.0.0.1" | "localhost" | "::1")
}

fn normalize_port(raw: u16, fallback: u16) -> u16 {
    if raw == 0 {
        fallback
    } else {
        raw
    }
}

fn normalize_route_path(raw: &str, fallback: String) -> String {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return fallback;
    }
    if cleaned.starts_with('/') {
        cleaned.to_string()
    } else {
        format!("/{cleaned}")
    }
}

fn normalize_text_list(values: &[String], lowercase: bool) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for item in values {
        let mut cleaned = item.trim().to_string();
        if cleaned.is_empty() {
            continue;
        }
        if lowercase {
            cleaned.make_ascii_lowercase();
        }
        if seen.insert(cleaned.clone()) {
            output.push(cleaned);
        }
    }
    output
}

fn normalize_cidr_list(values: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for item in values {
        let cleaned = item.trim();
        if cleaned.is_empty() {
            continue;
        }
        if parse_cidr(cleaned).is_none() {
            continue;
        }
        let key = cleaned.to_ascii_lowercase();
        if seen.insert(key.clone()) {
            output.push(key);
        }
    }
    output
}

fn is_ip_allowed(ip: IpAddr, allow_subnets: &[String], deny_subnets: &[String]) -> bool {
    for cidr in deny_subnets {
        if let Some(parsed) = parse_cidr(cidr) {
            if parsed.contains(ip) {
                return false;
            }
        }
    }

    if allow_subnets.is_empty() {
        return true;
    }

    allow_subnets
        .iter()
        .filter_map(|cidr| parse_cidr(cidr))
        .any(|cidr| cidr.contains(ip))
}

#[derive(Debug, Clone, Copy)]
enum ParsedCidr {
    V4 { network: Ipv4Addr, prefix: u8 },
    V6 { network: Ipv6Addr, prefix: u8 },
}

impl ParsedCidr {
    fn contains(self, ip: IpAddr) -> bool {
        match (self, ip) {
            (ParsedCidr::V4 { network, prefix }, IpAddr::V4(value)) => {
                ipv4_network(value, prefix) == ipv4_network(network, prefix)
            }
            (ParsedCidr::V6 { network, prefix }, IpAddr::V6(value)) => {
                ipv6_network(value, prefix) == ipv6_network(network, prefix)
            }
            _ => false,
        }
    }
}

fn parse_cidr(value: &str) -> Option<ParsedCidr> {
    let cleaned = value.trim();
    let (ip_part, prefix_part) = cleaned.split_once('/')?;
    let ip_part = ip_part.trim();
    let prefix = prefix_part.trim().parse::<u8>().ok()?;

    if let Ok(ipv4) = ip_part.parse::<Ipv4Addr>() {
        if prefix <= 32 {
            return Some(ParsedCidr::V4 {
                network: ipv4_network(ipv4, prefix),
                prefix,
            });
        }
        return None;
    }

    let ipv6 = ip_part.parse::<Ipv6Addr>().ok()?;
    if prefix > 128 {
        return None;
    }
    Some(ParsedCidr::V6 {
        network: ipv6_network(ipv6, prefix),
        prefix,
    })
}

fn ipv4_network(ip: Ipv4Addr, prefix: u8) -> Ipv4Addr {
    if prefix == 0 {
        return Ipv4Addr::UNSPECIFIED;
    }
    let raw = u32::from(ip);
    let mask = if prefix == 32 {
        u32::MAX
    } else {
        u32::MAX << (32 - prefix)
    };
    Ipv4Addr::from(raw & mask)
}

fn ipv6_network(ip: Ipv6Addr, prefix: u8) -> Ipv6Addr {
    if prefix == 0 {
        return Ipv6Addr::UNSPECIFIED;
    }
    let raw = u128::from_be_bytes(ip.octets());
    let mask = if prefix == 128 {
        u128::MAX
    } else {
        u128::MAX << (128 - prefix)
    };
    Ipv6Addr::from(raw & mask)
}

fn default_listen_host() -> String {
    DEFAULT_LISTEN_HOST.to_string()
}

const fn default_listen_port() -> u16 {
    DEFAULT_LISTEN_PORT
}

const fn default_discovery_port() -> u16 {
    DEFAULT_DISCOVERY_PORT
}

const fn default_discovery_interval_ms() -> u64 {
    DEFAULT_DISCOVERY_INTERVAL_MS
}

const fn default_peer_ttl_ms() -> u64 {
    DEFAULT_PEER_TTL_MS
}

const fn default_max_inbound_dedup() -> usize {
    DEFAULT_MAX_INBOUND_DEDUP
}

fn default_ws_path() -> String {
    "/wunder/desktop/lan/ws".to_string()
}

fn default_http_path() -> String {
    "/wunder/desktop/lan/envelope".to_string()
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::{is_ip_allowed, DesktopLanMeshSettings};
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn normalize_settings_dedupes_and_clamps() {
        let settings = DesktopLanMeshSettings {
            allow_subnets: vec![
                "192.168.1.0/24".to_string(),
                "192.168.1.0/24".to_string(),
                "invalid".to_string(),
            ],
            peer_blacklist: vec![" Alice ".to_string(), "alice".to_string()],
            discovery_interval_ms: 100,
            peer_ttl_ms: 100,
            max_inbound_dedup: 100_000,
            ..DesktopLanMeshSettings::default()
        }
        .normalized();

        assert_eq!(settings.allow_subnets, vec!["192.168.1.0/24"]);
        assert_eq!(settings.peer_blacklist, vec!["alice"]);
        assert!(settings.discovery_interval_ms >= 500);
        assert!(settings.peer_ttl_ms >= settings.discovery_interval_ms * 3);
        assert!(settings.max_inbound_dedup <= 32_768);
    }

    #[test]
    fn ip_allowlist_and_denylist_work() {
        let allow = vec!["192.168.0.0/16".to_string()];
        let deny = vec!["192.168.99.0/24".to_string()];
        let inside = IpAddr::V4(Ipv4Addr::new(192, 168, 10, 8));
        let blocked = IpAddr::V4(Ipv4Addr::new(192, 168, 99, 20));
        let outside = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));

        assert!(is_ip_allowed(inside, &allow, &deny));
        assert!(!is_ip_allowed(blocked, &allow, &deny));
        assert!(!is_ip_allowed(outside, &allow, &deny));
    }
}
