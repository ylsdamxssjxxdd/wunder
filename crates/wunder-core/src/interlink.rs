//! Interlink (cloud <-> local) contract types.
//!
//! These types are the frozen contract for the cloud/local interlink surface:
//! node registry, unified presence, tunnel frames, remote commands, approvals
//! and workspace shadows. Both the server (`wunder-runtime` API layer) and the
//! local engine (`services/interlink`) consume them; any change must land in
//! `docs/云端本地互通方案.md` first.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Node registry & unified presence
// ---------------------------------------------------------------------------

/// Kind of an interlink node (one end instance).
pub const NODE_TYPE_WEB: &str = "web";
pub const NODE_TYPE_DESKTOP: &str = "desktop";
pub const NODE_TYPE_CLI: &str = "cli";
/// The cloud itself, presented as a symmetric node to local clients.
pub const NODE_TYPE_SERVER: &str = "server";

/// Unified node presence states (docs §5.1).
pub const NODE_STATUS_ONLINE: &str = "online";
pub const NODE_STATUS_BUSY: &str = "busy";
pub const NODE_STATUS_AWAY: &str = "away";
pub const NODE_STATUS_RECONNECTING: &str = "reconnecting";
pub const NODE_STATUS_OFFLINE: &str = "offline";

/// One node in the interlink directory (`GET /wunder/interlink/nodes`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterlinkNodeView {
    /// `device:<id>` for desktop/cli, `web:<conn>` for volatile web sessions,
    /// `cloud` for the server node.
    pub node_id: String,
    pub node_type: String,
    pub user_id: String,
    /// Display name, e.g. `desktop·pc-01`.
    pub label: String,
    pub status: String,
    pub last_seen_at: f64,
    /// Effective capability set granted for this node.
    pub capabilities: Vec<String>,
    /// Latest workspace-shadow revision (0 = never synced).
    pub shadow_revision: i64,
    /// True when the live tunnel is up (desktop/cli) or any ws is up (web).
    pub connected: bool,
    /// `os/arch/app_version` etc. for persistent nodes.
    #[serde(default)]
    pub meta: Value,
}

// ---------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------

/// Read-only node/shadow/workspace/thread queries.
pub const CAP_QUERY_BASIC: &str = "query.basic";
/// Read binary file content through the tunnel.
pub const CAP_WORKSPACE_READ_BINARY: &str = "workspace.read.binary";
/// Shadow upload with thread directory (level: full).
pub const CAP_SHADOW_FULL: &str = "shadow:full";
/// Shadow upload without thread directory/tree (level: minimal).
pub const CAP_SHADOW_MINIMAL: &str = "shadow:minimal";
/// Drive local threads (create/message/cancel/answer).
pub const CAP_THREAD_DRIVE: &str = "thread.drive";
/// Mutate local workspace (write/mkdir/move/copy/delete).
pub const CAP_WORKSPACE_WRITE: &str = "workspace.write";
/// Execute allow-listed local tools.
pub const CAP_TOOL_EXEC: &str = "tool.exec";
/// Spawn sub-agents on the node.
pub const CAP_AGENT_SPAWN: &str = "agent.spawn";

/// Default capability set for a freshly registered device.
pub fn default_device_capabilities() -> Vec<String> {
    vec![
        CAP_SHADOW_MINIMAL.to_string(),
        CAP_QUERY_BASIC.to_string(),
        CAP_THREAD_DRIVE.to_string(),
    ]
}

// ---------------------------------------------------------------------------
// Tunnel frames (Interlink protocol v1)
// ---------------------------------------------------------------------------

pub const INTERLINK_PROTOCOL_VERSION: i64 = 1;

// Frame types.
pub const FRAME_HELLO: &str = "hello";
pub const FRAME_HELLO_ACK: &str = "hello_ack";
pub const FRAME_PING: &str = "ping";
pub const FRAME_PONG: &str = "pong";
pub const FRAME_COMMAND: &str = "command";
pub const FRAME_COMMAND_ACK: &str = "command_ack";
pub const FRAME_COMMAND_EVENT: &str = "command_event";
pub const FRAME_COMMAND_RESULT: &str = "command_result";
pub const FRAME_SHADOW_FULL: &str = "shadow_full";
pub const FRAME_SHADOW_DELTA: &str = "shadow_delta";
pub const FRAME_EVENT: &str = "event";
pub const FRAME_ERROR: &str = "error";
pub const FRAME_CLOSE: &str = "close";

/// Unified tunnel envelope. Carried as one JSON text websocket message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterlinkFrame {
    pub v: i64,
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub ts: f64,
    #[serde(default)]
    pub channel_id: Option<String>,
    /// Correlation id: command frames use `cmd_<uuid>`.
    #[serde(default)]
    pub corr: Option<String>,
    pub payload: Value,
}

impl InterlinkFrame {
    pub fn new(kind: &str, id: String, ts: f64, channel_id: Option<String>, payload: Value) -> Self {
        Self {
            v: INTERLINK_PROTOCOL_VERSION,
            kind: kind.to_string(),
            id,
            ts,
            channel_id,
            corr: None,
            payload,
        }
    }

    pub fn with_corr(mut self, corr: impl Into<String>) -> Self {
        self.corr = Some(corr.into());
        self
    }
}

/// `hello` payload, local -> server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterlinkHello {
    pub device_id: String,
    pub protocol: i64,
    pub capabilities: Vec<String>,
    /// HMAC-SHA256(node_secret, ticket), hex.
    pub hmac: String,
    pub secret_version: i64,
    /// Present when resuming a dropped channel.
    #[serde(default)]
    pub resume_channel_id: Option<String>,
    #[serde(default)]
    pub app_version: Option<String>,
}

/// `hello_ack` payload, server -> local.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterlinkHelloAck {
    pub channel_id: String,
    pub protocol: i64,
    pub capabilities_granted: Vec<String>,
    pub server_time: f64,
    /// Last shadow revision the server holds for this node (resume coherency).
    pub shadow_revision: i64,
}

// ---------------------------------------------------------------------------
// Remote commands
// ---------------------------------------------------------------------------

// Command kinds (docs §7.1).
pub const CMD_NODE_SUMMARY: &str = "node.summary";
pub const CMD_SHADOW_REFRESH: &str = "shadow.refresh";
pub const CMD_WORKSPACE_LIST: &str = "workspace.list";
pub const CMD_WORKSPACE_READ: &str = "workspace.read";
pub const CMD_WORKSPACE_SEARCH: &str = "workspace.search";
pub const CMD_WORKSPACE_STAT: &str = "workspace.stat";
pub const CMD_THREADS_LIST: &str = "threads.list";
pub const CMD_THREADS_GET: &str = "threads.get";
pub const CMD_THREAD_CREATE: &str = "thread.create";
pub const CMD_THREAD_MESSAGE: &str = "thread.message";
pub const CMD_THREAD_CANCEL: &str = "thread.cancel";
pub const CMD_THREAD_ANSWER: &str = "thread.answer";
pub const CMD_WORKSPACE_WRITE: &str = "workspace.write";
pub const CMD_WORKSPACE_MKDIR: &str = "workspace.mkdir";
pub const CMD_WORKSPACE_MOVE: &str = "workspace.move";
pub const CMD_WORKSPACE_COPY: &str = "workspace.copy";
pub const CMD_WORKSPACE_DELETE: &str = "workspace.delete";
pub const CMD_TOOL_EXEC: &str = "tool.exec";
pub const CMD_AGENT_SPAWN: &str = "agent.spawn";
/// Control frame kind the server uses to cancel an in-flight command; it is not
/// a user-facing operation and never appears in the §7.1 catalog.
pub const CMD_COMMAND_CANCEL: &str = "command.cancel";

// Directions.
pub const DIRECTION_C2L: &str = "c2l";
pub const DIRECTION_L2C: &str = "l2c";

// Command lifecycle states.
pub const COMMAND_STATUS_ISSUED: &str = "issued";
pub const COMMAND_STATUS_QUEUED: &str = "queued";
pub const COMMAND_STATUS_ACKED: &str = "acked";
pub const COMMAND_STATUS_RUNNING: &str = "running";
pub const COMMAND_STATUS_SUCCEEDED: &str = "succeeded";
pub const COMMAND_STATUS_FAILED: &str = "failed";
pub const COMMAND_STATUS_CANCELED: &str = "canceled";
pub const COMMAND_STATUS_TIMEOUT: &str = "timeout";

// Approval states.
pub const APPROVAL_NONE: &str = "none";
pub const APPROVAL_PENDING: &str = "pending";
pub const APPROVAL_APPROVED: &str = "approved";
pub const APPROVAL_REJECTED: &str = "rejected";
pub const APPROVAL_EXPIRED: &str = "expired";

// Risk levels.
pub const RISK_LOW: &str = "low";
pub const RISK_MEDIUM: &str = "medium";
pub const RISK_HIGH: &str = "high";

/// Map a command kind to (risk level, capability required, default approval).
pub fn command_policy(kind: &str) -> (&'static str, &'static str, bool) {
    match kind {
        CMD_NODE_SUMMARY | CMD_SHADOW_REFRESH | CMD_WORKSPACE_LIST | CMD_WORKSPACE_READ
        | CMD_WORKSPACE_SEARCH | CMD_WORKSPACE_STAT | CMD_THREADS_LIST | CMD_THREADS_GET => {
            (RISK_LOW, CAP_QUERY_BASIC, false)
        }
        CMD_THREAD_CREATE | CMD_THREAD_MESSAGE | CMD_THREAD_CANCEL | CMD_THREAD_ANSWER => {
            (RISK_MEDIUM, CAP_THREAD_DRIVE, true)
        }
        CMD_WORKSPACE_WRITE | CMD_WORKSPACE_MKDIR | CMD_WORKSPACE_MOVE | CMD_WORKSPACE_COPY
        | CMD_WORKSPACE_DELETE => (RISK_MEDIUM, CAP_WORKSPACE_WRITE, true),
        CMD_TOOL_EXEC | CMD_AGENT_SPAWN => (RISK_HIGH, CAP_TOOL_EXEC, true),
        _ => (RISK_HIGH, CAP_TOOL_EXEC, true),
    }
}

/// Kinds that must never bypass approval regardless of local settings.
pub fn requires_hard_approval(kind: &str) -> bool {
    matches!(
        kind,
        CMD_WORKSPACE_WRITE
            | CMD_WORKSPACE_MKDIR
            | CMD_WORKSPACE_MOVE
            | CMD_WORKSPACE_COPY
            | CMD_WORKSPACE_DELETE
            | CMD_TOOL_EXEC
            | CMD_AGENT_SPAWN
    )
}

// ---------------------------------------------------------------------------
// Audit action names
// ---------------------------------------------------------------------------

pub const AUDIT_CHANNEL_OPEN: &str = "channel.open";
pub const AUDIT_CHANNEL_CLOSE: &str = "channel.close";
pub const AUDIT_COMMAND_ISSUE: &str = "command.issue";
pub const AUDIT_COMMAND_ACK: &str = "command.ack";
pub const AUDIT_COMMAND_FINISH: &str = "command.finish";
pub const AUDIT_APPROVAL_DECIDE: &str = "approval.decide";
pub const AUDIT_FILE_READ: &str = "file.read";
pub const AUDIT_FILE_WRITE: &str = "file.write";
pub const AUDIT_SHADOW_SYNC: &str = "shadow.sync";

// ---------------------------------------------------------------------------
// Event payload kinds (`event` frames, docs §4.4 / §5.3 / §7.4)
// ---------------------------------------------------------------------------

/// Lightweight application heartbeat `{status, active_threads, cpu_load}`.
pub const EVENT_PRESENCE: &str = "presence";
/// Thread event forwarded for a remote session view (docs §7.4).
pub const EVENT_THREAD: &str = "thread_event";
/// A node joined the directory; broadcast to the user's other live nodes.
pub const EVENT_NODE_JOINED: &str = "node.joined";
/// The node asks its user for an approval decision (docs §7.3).
pub const EVENT_APPROVAL_REQUEST: &str = "approval.request";

// ---------------------------------------------------------------------------
// Data-plane binary frames (docs §4.2)
//
// Layout: `stream_id(u64 be) + flags(u32 be) + offset(u32 be)` then payload.
// ---------------------------------------------------------------------------

pub const DATA_HEADER_BYTES: usize = 16;
/// Last chunk of the stream.
pub const DATA_FLAG_LAST: u32 = 0x1;
/// Producer aborted the stream; the consumer must drop the buffer.
pub const DATA_FLAG_ERROR: u32 = 0x2;

// ---------------------------------------------------------------------------
// Structural error codes (docs §4.3 / §10.3)
// ---------------------------------------------------------------------------

pub const ERR_NODE_OFFLINE: &str = "NODE_OFFLINE";
pub const ERR_NODE_BUSY: &str = "NODE_BUSY";
pub const ERR_QUEUE_FULL: &str = "QUEUE_FULL";
pub const ERR_CAP_DENIED: &str = "CAP_DENIED";
pub const ERR_APPROVAL_REQUIRED: &str = "APPROVAL_REQUIRED";
pub const ERR_APPROVAL_REJECTED: &str = "APPROVAL_REJECTED";
pub const ERR_APPROVAL_EXPIRED: &str = "APPROVAL_EXPIRED";
pub const ERR_TIMEOUT: &str = "TIMEOUT";
pub const ERR_CHANNEL_SUPERSEDED: &str = "CHANNEL_SUPERSEDED";
pub const ERR_UNKNOWN_KIND: &str = "UNKNOWN_KIND";

/// Terminal command states: the first one to arrive wins (docs §4.3).
pub fn is_terminal_command_status(status: &str) -> bool {
    matches!(
        status,
        COMMAND_STATUS_SUCCEEDED
            | COMMAND_STATUS_FAILED
            | COMMAND_STATUS_CANCELED
            | COMMAND_STATUS_TIMEOUT
    )
}

/// Operation tier of a command kind (docs §7.1). `L0` is read-only, `L2`/`L3`
/// mutate or execute locally and can never bypass on-device approval.
pub fn command_level(kind: &str) -> &'static str {
    match kind {
        CMD_NODE_SUMMARY | CMD_SHADOW_REFRESH | CMD_WORKSPACE_LIST | CMD_WORKSPACE_READ
        | CMD_WORKSPACE_SEARCH | CMD_WORKSPACE_STAT | CMD_THREADS_LIST | CMD_THREADS_GET => "L0",
        CMD_THREAD_CREATE | CMD_THREAD_MESSAGE | CMD_THREAD_CANCEL | CMD_THREAD_ANSWER => "L1",
        CMD_WORKSPACE_WRITE | CMD_WORKSPACE_MKDIR | CMD_WORKSPACE_MOVE | CMD_WORKSPACE_COPY
        | CMD_WORKSPACE_DELETE => "L2",
        CMD_TOOL_EXEC | CMD_AGENT_SPAWN => "L3",
        _ => "L3",
    }
}

/// Offline targets only accept non-mutating commands (docs §4.3: the offline
/// queue never holds L2/L3, they require the user to be present).
pub fn is_queueable_when_offline(kind: &str) -> bool {
    matches!(command_level(kind), "L0" | "L1")
}

/// Remote session view frame types on `WS /wunder/interlink/remote_ws`.
pub const REMOTE_FRAME_SNAPSHOT: &str = "snapshot";
pub const REMOTE_FRAME_DELTA: &str = "delta";
pub const REMOTE_FRAME_ERROR: &str = "error";
pub const REMOTE_FRAME_CLOSE: &str = "close";
pub const REMOTE_WS_PROTOCOL: &str = "wunder-interlink-remote";
pub const TUNNEL_WS_PROTOCOL: &str = "wunder-interlink";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_serializes_with_type_field() {
        let frame = InterlinkFrame::new(
            FRAME_PING,
            "frm_1".to_string(),
            10.0,
            Some("ch_1".to_string()),
            serde_json::json!({}),
        );
        let json = serde_json::to_value(&frame).expect("serialize");
        assert_eq!(json["type"], "ping");
        assert_eq!(json["v"], 1);
    }

    #[test]
    fn command_policy_levels_match_doc() {
        assert_eq!(command_policy(CMD_WORKSPACE_READ).0, RISK_LOW);
        assert_eq!(command_policy(CMD_THREAD_MESSAGE).0, RISK_MEDIUM);
        assert!(command_policy(CMD_WORKSPACE_DELETE).2);
        assert!(requires_hard_approval(CMD_TOOL_EXEC));
        assert!(!requires_hard_approval(CMD_THREADS_LIST));
    }
}
