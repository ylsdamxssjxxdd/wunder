use crate::storage::{
    GatewayClientRecord, GatewayNodeRecord, GatewayNodeTokenRecord, StorageBackend,
};
use anyhow::{anyhow, Result};
use axum::extract::ws::Message;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex, RwLock};
use tokio::time::{timeout, Duration};
use uuid::Uuid;

pub const GATEWAY_PROTOCOL_VERSION: i32 = 1;
pub const GATEWAY_PROTOCOL_MIN_VERSION: i32 = 1;
pub const GATEWAY_PROTOCOL_MAX_VERSION: i32 = 1;
pub const GATEWAY_MAX_MESSAGE_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GatewayRole {
    Operator,
    Node,
    Channel,
    Unknown,
}

impl GatewayRole {
    pub fn from_str(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "operator" => Self::Operator,
            "node" => Self::Node,
            "channel" => Self::Channel,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Operator => "operator",
            Self::Node => "node",
            Self::Channel => "channel",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GatewayClientInfo {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GatewayAuthParams {
    #[serde(default, alias = "token")]
    pub token: Option<String>,
    #[serde(default, alias = "nodeToken", alias = "node_token")]
    pub node_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GatewayDeviceInfo {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, alias = "fingerprint")]
    pub device_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GatewayConnectParams {
    #[serde(default, alias = "protocolVersion", alias = "protocol_version")]
    pub protocol_version: Option<i32>,
    #[serde(
        default,
        alias = "minProtocol",
        alias = "min_protocol",
        alias = "minProtocolVersion",
        alias = "min_protocol_version"
    )]
    pub min_protocol: Option<i32>,
    #[serde(
        default,
        alias = "maxProtocol",
        alias = "max_protocol",
        alias = "maxProtocolVersion",
        alias = "max_protocol_version"
    )]
    pub max_protocol: Option<i32>,
    #[serde(default)]
    pub client: Option<GatewayClientInfo>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub caps: Vec<String>,
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default)]
    pub permissions: Option<Value>,
    #[serde(default)]
    pub auth: Option<GatewayAuthParams>,
    #[serde(default)]
    pub device: Option<GatewayDeviceInfo>,
    #[serde(default)]
    pub user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GatewayProtocolInfo {
    pub version: i32,
    pub min: i32,
    pub max: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct GatewayPolicy {
    pub max_message_bytes: usize,
    pub tick_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GatewayPresenceEntry {
    pub connection_id: String,
    pub role: String,
    pub user_id: Option<String>,
    pub node_id: Option<String>,
    pub scopes: Vec<String>,
    pub caps: Vec<String>,
    pub commands: Vec<String>,
    pub client: Option<GatewayClientInfo>,
    pub connected_at: f64,
    pub last_seen_at: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GatewayPresenceSnapshot {
    pub state_version: u64,
    pub items: Vec<GatewayPresenceEntry>,
}

#[derive(Debug, Clone)]
pub struct GatewayInvokeResult {
    pub ok: bool,
    pub payload: Option<Value>,
    pub error: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct GatewayNodeInvokeRequest {
    pub node_id: String,
    pub command: String,
    pub args: Option<Value>,
    pub timeout_s: f64,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct GatewayClientMeta {
    pub connection_id: String,
    pub role: GatewayRole,
    pub user_id: Option<String>,
    pub node_id: Option<String>,
    pub scopes: Vec<String>,
    pub caps: Vec<String>,
    pub commands: Vec<String>,
    pub client: Option<GatewayClientInfo>,
    pub connected_at: f64,
    pub last_seen_at: f64,
    pub device_fingerprint: Option<String>,
}

struct GatewayClientState {
    meta: GatewayClientMeta,
    sender: mpsc::Sender<Message>,
}

struct GatewayState {
    clients: HashMap<String, GatewayClientState>,
    node_index: HashMap<String, String>,
}

impl GatewayState {
    fn new() -> Self {
        Self {
            clients: HashMap::new(),
            node_index: HashMap::new(),
        }
    }
}

#[derive(Clone)]
pub struct GatewayHub {
    storage: Arc<dyn StorageBackend>,
    state: Arc<RwLock<GatewayState>>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<GatewayInvokeResult>>>>,
    state_version: Arc<AtomicU64>,
}

impl GatewayHub {
    pub fn new(storage: Arc<dyn StorageBackend>) -> Self {
        Self {
            storage,
            state: Arc::new(RwLock::new(GatewayState::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            state_version: Arc::new(AtomicU64::new(1)),
        }
    }

    pub fn protocol_info() -> GatewayProtocolInfo {
        GatewayProtocolInfo {
            version: GATEWAY_PROTOCOL_VERSION,
            min: GATEWAY_PROTOCOL_MIN_VERSION,
            max: GATEWAY_PROTOCOL_MAX_VERSION,
        }
    }

    pub fn default_policy() -> GatewayPolicy {
        GatewayPolicy {
            max_message_bytes: GATEWAY_MAX_MESSAGE_BYTES,
            tick_interval_ms: 15_000,
        }
    }

    pub async fn register_client(
        &self,
        meta: GatewayClientMeta,
        sender: mpsc::Sender<Message>,
    ) -> GatewayPresenceSnapshot {
        let mut state = self.state.write().await;
        if let Some(node_id) = meta.node_id.as_ref() {
            state
                .node_index
                .insert(node_id.clone(), meta.connection_id.clone());
        }
        state.clients.insert(
            meta.connection_id.clone(),
            GatewayClientState {
                meta: meta.clone(),
                sender,
            },
        );
        let version = self.bump_state_version();
        let snapshot = build_snapshot_locked(&state, version);
        let _ = self.persist_client_record(&meta, "connected", None);
        if meta.role == GatewayRole::Node {
            let _ = self.persist_node_record(&meta, "online");
        }
        snapshot
    }

    pub async fn unregister_client(&self, connection_id: &str) -> Option<GatewayPresenceSnapshot> {
        let mut state = self.state.write().await;
        let removed = state.clients.remove(connection_id);
        if let Some(removed) = removed {
            if let Some(node_id) = removed.meta.node_id.as_ref() {
                state.node_index.remove(node_id);
            }
            let version = self.bump_state_version();
            let snapshot = build_snapshot_locked(&state, version);
            let _ = self.persist_client_record(
                &removed.meta,
                "disconnected",
                Some(removed.meta.last_seen_at),
            );
            if removed.meta.role == GatewayRole::Node {
                let _ = self.persist_node_record(&removed.meta, "offline");
            }
            return Some(snapshot);
        }
        None
    }

    pub async fn snapshot(&self) -> GatewayPresenceSnapshot {
        let state = self.state.read().await;
        let version = self.state_version.load(Ordering::SeqCst);
        build_snapshot_locked(&state, version)
    }

    pub async fn broadcast_event(&self, event: &str, payload: Value, state_version: Option<u64>) {
        let senders = {
            let state = self.state.read().await;
            state
                .clients
                .values()
                .map(|client| client.sender.clone())
                .collect::<Vec<_>>()
        };
        let event_payload = if let Some(version) = state_version {
            json!({
                "type": "event",
                "event": event,
                "payload": payload,
                "stateVersion": version
            })
        } else {
            json!({
                "type": "event",
                "event": event,
                "payload": payload
            })
        };
        let message = Message::Text(event_payload.to_string().into());
        for sender in senders {
            let _ = sender.send(message.clone()).await;
        }
    }

    pub async fn touch_client(&self, connection_id: &str, last_seen_at: f64) {
        let mut state = self.state.write().await;
        if let Some(client) = state.clients.get_mut(connection_id) {
            client.meta.last_seen_at = last_seen_at;
        }
    }

    pub async fn handle_response(
        &self,
        request_id: &str,
        ok: bool,
        payload: Option<Value>,
        error: Option<Value>,
    ) {
        let mut pending = self.pending.lock().await;
        if let Some(tx) = pending.remove(request_id) {
            let _ = tx.send(GatewayInvokeResult { ok, payload, error });
        }
    }

    pub async fn invoke_node(
        &self,
        request: GatewayNodeInvokeRequest,
    ) -> Result<GatewayInvokeResult> {
        let sender = {
            let state = self.state.read().await;
            let conn_id = state
                .node_index
                .get(&request.node_id)
                .ok_or_else(|| anyhow!("node not connected: {}", request.node_id))?;
            let client = state
                .clients
                .get(conn_id)
                .ok_or_else(|| anyhow!("node not connected: {}", request.node_id))?;
            client.sender.clone()
        };

        let request_id = format!("gwreq_{}", Uuid::new_v4().simple());
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(request_id.clone(), tx);
        }

        let params = json!({
            "node_id": request.node_id,
            "command": request.command,
            "args": request.args,
            "metadata": request.metadata
        });
        let message = json!({
            "type": "req",
            "id": request_id,
            "method": "node.invoke",
            "params": params
        });
        if sender
            .send(Message::Text(message.to_string().into()))
            .await
            .is_err()
        {
            let mut pending = self.pending.lock().await;
            pending.remove(&request_id);
            return Err(anyhow!("node connection unavailable"));
        }

        let duration = Duration::from_secs_f64(request.timeout_s.max(1.0));
        match timeout(duration, rx).await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(_)) => Err(anyhow!("node response channel closed")),
            Err(_) => {
                let mut pending = self.pending.lock().await;
                pending.remove(&request_id);
                Err(anyhow!("node invoke timeout"))
            }
        }
    }

    fn bump_state_version(&self) -> u64 {
        self.state_version.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn persist_client_record(
        &self,
        meta: &GatewayClientMeta,
        status: &str,
        disconnected_at: Option<f64>,
    ) -> Result<()> {
        let record = GatewayClientRecord {
            connection_id: meta.connection_id.clone(),
            role: meta.role.as_str().to_string(),
            user_id: meta.user_id.clone(),
            node_id: meta.node_id.clone(),
            scopes: meta.scopes.clone(),
            caps: meta.caps.clone(),
            commands: meta.commands.clone(),
            client_info: serde_json::to_value(meta.client.clone()).ok(),
            status: status.to_string(),
            connected_at: meta.connected_at,
            last_seen_at: meta.last_seen_at,
            disconnected_at,
        };
        self.storage.upsert_gateway_client(&record)
    }

    fn persist_node_record(&self, meta: &GatewayClientMeta, status: &str) -> Result<()> {
        let Some(node_id) = meta.node_id.as_ref() else {
            return Ok(());
        };
        let record = GatewayNodeRecord {
            node_id: node_id.clone(),
            name: meta.client.as_ref().and_then(|client| client.id.clone()),
            device_fingerprint: meta.device_fingerprint.clone(),
            status: status.to_string(),
            caps: meta.caps.clone(),
            commands: meta.commands.clone(),
            permissions: None,
            metadata: None,
            created_at: meta.connected_at,
            updated_at: meta.last_seen_at,
            last_seen_at: meta.last_seen_at,
        };
        self.storage.upsert_gateway_node(&record)
    }

    pub fn validate_node_token(&self, token: &str) -> Result<Option<GatewayNodeTokenRecord>> {
        self.storage.get_gateway_node_token(token)
    }
}

fn build_snapshot_locked(state: &GatewayState, state_version: u64) -> GatewayPresenceSnapshot {
    let items = state
        .clients
        .values()
        .map(|client| GatewayPresenceEntry {
            connection_id: client.meta.connection_id.clone(),
            role: client.meta.role.as_str().to_string(),
            user_id: client.meta.user_id.clone(),
            node_id: client.meta.node_id.clone(),
            scopes: client.meta.scopes.clone(),
            caps: client.meta.caps.clone(),
            commands: client.meta.commands.clone(),
            client: client.meta.client.clone(),
            connected_at: client.meta.connected_at,
            last_seen_at: client.meta.last_seen_at,
        })
        .collect::<Vec<_>>();
    GatewayPresenceSnapshot {
        state_version,
        items,
    }
}

pub fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}
