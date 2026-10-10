//! Tunnel bootstrap: node secret, channel ticket, `hello` signing and the
//! reconnect policy (docs §4.1, §9.1, §4.4).
//!
//! Everything here is transport-agnostic pure logic plus the two REST calls,
//! so the retry/handshake rules are unit-testable without a socket.
//!
//! Contract mirrored from the server (`api/interlink_ws.rs`):
//! - `POST /wunder/interlink/node_secret` `{device_id}` -> `{data:{secret, secret_version}}`
//! - `POST /wunder/interlink/channel_ticket` `{device_id}` -> `{data:{ticket, expires_in}}`
//! - `WSS /wunder/interlink/ws?ticket=itk_<uuid>` with subprotocol
//!   `wunder-interlink`, first frame `hello` signed with
//!   `hmac = hex(HMAC_SHA256(node_secret, ticket))`.

use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{json, Value};
use uuid::Uuid;

use wunder_core::interlink::{
    FRAME_HELLO, INTERLINK_PROTOCOL_VERSION, TUNNEL_WS_PROTOCOL,
};

use crate::services::cloud;
use crate::services::cloud::CloudSessionFile;
use crate::services::interlink::secret;

use super::now_ts;

/// First reconnect delay (docs §4.4: 1s -> 2s -> ... capped at 300s).
pub const BACKOFF_BASE_S: u64 = 1;
/// Reconnect delay ceiling (docs §4.4).
pub const BACKOFF_MAX_S: u64 = 300;
/// Jitter band: ±20% of the scheduled delay (docs §4.4).
pub const BACKOFF_JITTER_BPS: u64 = 2_000;
/// Time budget for the websocket connect plus the `hello_ack` round trip.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Time budget for one REST bootstrap call.
pub const REST_TIMEOUT: Duration = Duration::from_secs(15);
/// Frame id prefix of client frames.
pub fn frame_id() -> String {
    format!("frm_{}", Uuid::new_v4().simple())
}

/// Close reasons the server answers with (docs §4.1). Anything unknown is
/// treated as a plain transport close.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseReason {
    SecretRequired,
    HmacMismatch,
    HmacRequired,
    SecretVersionUnknown,
    Superseded,
    InterlinkDisabled,
    DeviceRevoked,
    DeviceUnknown,
    DeviceMismatch,
    ChannelLimit,
    UpgradeRequired,
    ProtocolUnsupported,
    StorageError,
    SendFailed,
    ClosedByPeer,
    Transport,
    Timeout,
    Unknown(String),
}

impl CloseReason {
    /// Parse the `reason` string of a `close` frame. Unknown values are kept
    /// verbatim (bounded) so the client can report them without guessing.
    pub fn parse(raw: &str) -> Self {
        let cleaned = raw.trim().to_ascii_lowercase();
        match cleaned.as_str() {
            "secret_required" => Self::SecretRequired,
            "hmac_mismatch" => Self::HmacMismatch,
            "hmac_required" => Self::HmacRequired,
            "secret_version_unknown" => Self::SecretVersionUnknown,
            "superseded" => Self::Superseded,
            "interlink_disabled" => Self::InterlinkDisabled,
            "device_revoked" => Self::DeviceRevoked,
            "device_unknown" => Self::DeviceUnknown,
            "device_mismatch" => Self::DeviceMismatch,
            "channel_limit" => Self::ChannelLimit,
            "upgrade_required" => Self::UpgradeRequired,
            "protocol_unsupported" => Self::ProtocolUnsupported,
            "storage_error" => Self::StorageError,
            "send_failed" => Self::SendFailed,
            "" => Self::ClosedByPeer,
            other => Self::Unknown(truncate(other)),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::SecretRequired => "secret_required",
            Self::HmacMismatch => "hmac_mismatch",
            Self::HmacRequired => "hmac_required",
            Self::SecretVersionUnknown => "secret_version_unknown",
            Self::Superseded => "superseded",
            Self::InterlinkDisabled => "interlink_disabled",
            Self::DeviceRevoked => "device_revoked",
            Self::DeviceUnknown => "device_unknown",
            Self::DeviceMismatch => "device_mismatch",
            Self::ChannelLimit => "channel_limit",
            Self::UpgradeRequired => "upgrade_required",
            Self::ProtocolUnsupported => "protocol_unsupported",
            Self::StorageError => "storage_error",
            Self::SendFailed => "send_failed",
            Self::ClosedByPeer => "closed",
            Self::Transport => "transport",
            Self::Timeout => "timeout",
            Self::Unknown(raw) => raw.as_str(),
        }
    }

    /// Whether the reason means "this credential is not acceptable yet".
    pub fn is_secret_problem(&self) -> bool {
        matches!(
            self,
            Self::SecretRequired | Self::HmacMismatch | Self::SecretVersionUnknown
        )
    }
}

fn truncate(value: &str) -> String {
    value.chars().take(48).collect()
}

/// What the run loop should do after one connection attempt ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryPolicy {
    /// Re-attempt immediately without consuming a backoff slot.
    Now,
    /// Consume one backoff step and retry.
    Backoff,
    /// Stay quiet for one full (capped) window: the node is not acceptable
    /// right now, hammering the server would be a self-inflicted storm.
    Cooldown,
}

/// Map a close reason to the reconnect policy (docs §4.1/§4.4).
pub fn policy_for(reason: &CloseReason) -> RetryPolicy {
    match reason {
        // The server has no key for this device yet, or the key we hold is
        // stale: fetching a fresh one and retrying is the documented path.
        CloseReason::SecretRequired => RetryPolicy::Now,
        CloseReason::HmacMismatch | CloseReason::SecretVersionUnknown | CloseReason::HmacRequired => {
            RetryPolicy::Backoff
        }
        // A newer process took the slot. Wait out a full window so the two
        // processes cannot fight over the channel (docs §4.1).
        CloseReason::Superseded => RetryPolicy::Backoff,
        // Kill switch / revoked / too old: nothing a fast retry can fix.
        CloseReason::InterlinkDisabled
        | CloseReason::DeviceRevoked
        | CloseReason::DeviceUnknown
        | CloseReason::UpgradeRequired
        | CloseReason::ProtocolUnsupported => RetryPolicy::Cooldown,
        CloseReason::ChannelLimit
        | CloseReason::StorageError
        | CloseReason::SendFailed
        | CloseReason::DeviceMismatch
        | CloseReason::ClosedByPeer
        | CloseReason::Transport
        | CloseReason::Timeout
        | CloseReason::Unknown(_) => RetryPolicy::Backoff,
    }
}

/// Exponential backoff without jitter: `1, 2, 4, ... 300`.
pub fn backoff_delay(attempt: u32) -> Duration {
    let step = BACKOFF_BASE_S.saturating_mul(1u64.checked_shl(attempt.min(32)).unwrap_or(0));
    Duration::from_secs(step.clamp(BACKOFF_BASE_S, BACKOFF_MAX_S))
}

/// Decide the reconnect plan after one failed session: a secret-shaped close
/// reason gets at most one re-bootstrap per connection cycle (fetch the node
/// secret again), everything else follows [`policy_for`].
pub fn secret_retry_plan(flow: &mut SecretFlow, reason: &CloseReason) -> (RetryPolicy, bool) {
    match reason {
        CloseReason::SecretRequired
        | CloseReason::HmacMismatch
        | CloseReason::HmacRequired
        | CloseReason::SecretVersionUnknown => {
            if flow.on_secret_problem() == SecretAction::Bootstrap {
                (RetryPolicy::Now, true)
            } else {
                (policy_for(reason), false)
            }
        }
        _ => (policy_for(reason), false),
    }
}

/// Apply ±20% jitter deterministically from an entropy sample.
pub fn jittered(delay: Duration, sample: u64) -> Duration {
    let micros = delay.as_micros().min(u64::MAX as u128) as u64;
    if micros == 0 {
        return delay;
    }
    let band = micros.saturating_mul(BACKOFF_JITTER_BPS) / 10_000;
    // Map the sample onto [-band, +band].
    let offset = if band == 0 { 0 } else { sample % (band * 2 + 1) };
    let signed = offset as i64 - band as i64;
    let adjusted = micros as i64 + signed;
    Duration::from_micros(adjusted.max(1) as u64)
}

/// Entropy for the jitter band; no new dependency (uuid v4 is already used).
pub fn jitter_sample() -> u64 {
    let raw = Uuid::new_v4().as_u128();
    (raw ^ (raw >> 64)) as u64
}

/// Track the "fetch a fresh node secret and retry once" rule (docs §4.1:
/// `secret_required` -> bootstrap -> retry). A second failure inside the same
/// attempt must not bootstrap forever.
#[derive(Debug, Default)]
pub struct SecretFlow {
    refreshed: bool,
}

/// Action for one secret-related failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretAction {
    /// Fetch (or re-fetch) the node secret, then retry without backoff.
    Bootstrap,
    /// Already re-bootstrapped once: fall back to the normal backoff.
    GiveUp,
}

impl SecretFlow {
    /// Returns `Bootstrap` at most once per connection cycle.
    pub fn on_secret_problem(&mut self) -> SecretAction {
        if self.refreshed {
            return SecretAction::GiveUp;
        }
        self.refreshed = true;
        SecretAction::Bootstrap
    }

    pub fn reset(&mut self) {
        self.refreshed = false;
    }

    pub fn refreshed(&self) -> bool {
        self.refreshed
    }
}

/// `ws://`/`wss://` tunnel URL for one ticket. Rejects anything that is not a
/// plain http(s) origin or a ticket outside the server's alphabet, so no
/// credential or path can leak into the query string.
pub fn tunnel_url(server: &str, ticket: &str) -> Option<String> {
    if !is_ticket_shape(ticket) {
        return None;
    }
    let trimmed = server.trim().trim_end_matches('/');
    let lowered = trimmed.to_ascii_lowercase();
    let scheme = if lowered.starts_with("https://") {
        "wss"
    } else if lowered.starts_with("http://") {
        "ws"
    } else {
        return None;
    };
    let host = trimmed.split_once("://")?.1;
    if host.is_empty() || host.contains(' ') || host.contains('?') || host.contains('#') {
        return None;
    }
    Some(format!(
        "{scheme}://{host}/wunder/interlink/ws?ticket={ticket}"
    ))
}

/// Ticket alphabet the server mints: `itk_<uuid simple>`.
pub fn is_ticket_shape(ticket: &str) -> bool {
    let Some(body) = ticket.strip_prefix("itk_") else {
        return false;
    };
    !body.is_empty()
        && body.len() <= 64
        && body
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Node secret plus the version the server issued it under. The secret is a
/// credential: it is kept in the 0600 session file and in this process only,
/// never in a log, an error string or a frame other than the derived HMAC.
#[derive(Debug, Clone)]
pub struct NodeSecret {
    pub secret: String,
    pub version: i64,
}

/// Failures of the pre-socket phase. Every variant maps to a reconnect policy
/// and carries no credential material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandshakeError {
    /// The server has no interlink surface for this call (404) or failed it
    /// (5xx): keep the cloud channel working, back off.
    Unavailable(u16),
    /// Non-success status the node cannot fix by retrying (400/403).
    Rejected(u16),
    /// The session is gone or expired: wait for a re-login.
    Expired,
    /// Network-level failure (sanitised, no url).
    Network,
    /// The websocket could not be established.
    Transport,
    /// The hello/hello_ack exchange did not complete.
    Timeout,
    /// Response shape or ticket/alphabet unexpected: protocol mismatch.
    Protocol(&'static str),
    /// The server closed the tunnel with an explicit reason.
    Closed(CloseReason),
}

impl HandshakeError {
    /// Short, log-safe summary.
    pub fn summary(&self) -> String {
        match self {
            Self::Unavailable(status) => format!("interlink endpoint unavailable ({status})"),
            Self::Rejected(status) => format!("interlink endpoint rejected ({status})"),
            Self::Closed(reason) => format!("tunnel closed ({})", reason.as_str()),
            Self::Expired => "cloud session expired".to_string(),
            Self::Network => "network request failed".to_string(),
            Self::Transport => "websocket connect failed".to_string(),
            Self::Timeout => "handshake timeout".to_string(),
            Self::Protocol(code) => format!("protocol mismatch ({code})"),
        }
    }

    /// Reconnect policy for this failure.
    pub fn policy(&self) -> RetryPolicy {
        match self {
            Self::Expired => RetryPolicy::Cooldown,
            Self::Closed(reason) => policy_for(reason),
            Self::Unavailable(_) | Self::Rejected(_) | Self::Network | Self::Transport
            | Self::Timeout => RetryPolicy::Backoff,
            Self::Protocol(_) => RetryPolicy::Cooldown,
        }
    }
}
pub fn build_hello(
    session: &CloudSessionFile,
    secret: &NodeSecret,
    ticket: &str,
    capabilities: &[String],
    resume_channel_id: Option<&str>,
    app_version: &str,
) -> Value {
    let hmac = secret::channel_hmac(&secret.secret, ticket);
    json!({
        "v": INTERLINK_PROTOCOL_VERSION,
        "type": FRAME_HELLO,
        "id": frame_id(),
        "ts": now_ts(),
        "channel_id": Value::Null,
        "corr": Value::Null,
        "payload": {
            "device_id": session.device_id,
            "protocol": INTERLINK_PROTOCOL_VERSION,
            "capabilities": capabilities,
            "hmac": hmac,
            "secret_version": secret.version,
            "resume_channel_id": resume_channel_id,
            "app_version": app_version,
        }
    })
}

/// Websocket subprotocol the tunnel negotiates.
pub fn subprotocol() -> &'static str {
    TUNNEL_WS_PROTOCOL
}

fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(REST_TIMEOUT)
            .build()
            .unwrap_or_default()
    })
}

/// `POST {server}/wunder/interlink/node_secret` - bootstrap the per-device key.
/// The secret appears exactly once, in this response; it is never logged.
pub async fn fetch_node_secret(session: &CloudSessionFile) -> Result<NodeSecret, HandshakeError> {
    let body = authed_post(
        session,
        "/wunder/interlink/node_secret",
        &json!({ "device_id": session.device_id }),
    )
    .await?;
    let data = body.get("data").cloned().unwrap_or(Value::Null);
    let secret = data["secret"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(HandshakeError::Protocol("node_secret_response"))?
        .to_string();
    let version = data["secret_version"].as_i64().unwrap_or(1).max(1);
    Ok(NodeSecret { secret, version })
}

/// `POST {server}/wunder/interlink/channel_ticket` - one-time tunnel ticket.
pub async fn fetch_channel_ticket(
    session: &CloudSessionFile,
) -> Result<String, HandshakeError> {
    let body = authed_post(
        session,
        "/wunder/interlink/channel_ticket",
        &json!({ "device_id": session.device_id }),
    )
    .await?;
    let data = body.get("data").cloned().unwrap_or(Value::Null);
    let ticket = data["ticket"]
        .as_str()
        .map(str::trim)
        .filter(|value| is_ticket_shape(value))
        .ok_or(HandshakeError::Protocol("ticket_response"))?
        .to_string();
    Ok(ticket)
}

/// One Bearer POST with the documented 401 -> single-flight refresh -> retry
/// once path, driven through the public `CloudService` surface so token
/// rotation and the connection state machine stay in one place.
async fn authed_post(
    session: &CloudSessionFile,
    path: &str,
    body: &Value,
) -> Result<Value, HandshakeError> {
    let service = cloud::shared();
    let url = format!("{}{}", session.server.trim_end_matches('/'), path);
    let stale_token = session.token.clone();
    let first = send_post(&url, &stale_token, &session.device_id, body).await;
    let response = match first {
        Ok(response) if response.status().as_u16() != 401 => response,
        Ok(response) => {
            drop(response);
            match service.recover_auth(&stale_token).await {
                Ok(true) => {
                    let refreshed = service
                        .session()
                        .ok_or(HandshakeError::Expired)?
                        .clone();
                    send_post(&url, &refreshed.token, &refreshed.device_id, body)
                        .await
                        .map_err(|err| network_error(&err))?
                }
                Ok(false) => return Err(HandshakeError::Expired),
                Err(err) => {
                    let _ = err;
                    return Err(HandshakeError::Network);
                }
            }
        }
        Err(err) => return Err(network_error(&err)),
    };
    let status = response.status();
    if status.as_u16() == 404 || status.is_server_error() {
        // Older server without the interlink surface, or a transient failure:
        // the caller backs off instead of spinning.
        return Err(HandshakeError::Unavailable(status.as_u16()));
    }
    if !status.is_success() {
        return Err(HandshakeError::Rejected(status.as_u16()));
    }
    let parsed: Value = response
        .json()
        .await
        .map_err(|_| HandshakeError::Protocol("bad_json_response"))?;
    Ok(parsed)
}

async fn send_post(
    url: &str,
    token: &str,
    device_id: &str,
    body: &Value,
) -> Result<reqwest::Response, reqwest::Error> {
    http()
        .post(url)
        .bearer_auth(token)
        .header("x-wunder-device-id", device_id)
        .json(body)
        .send()
        .await
}

fn network_error(err: &reqwest::Error) -> HandshakeError {
    // Reuse the cloud channel's sanitisation: class only, no url or token.
    let _ = cloud::network_error_summary(err);
    HandshakeError::Network
}
