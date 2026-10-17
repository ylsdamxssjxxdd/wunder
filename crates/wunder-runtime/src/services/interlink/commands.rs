//! Remote command ledger (docs §4.3, §7.2, §10.1).
//!
//! Both control directions are recorded here: idempotent issue, the
//! `issued -> acked -> running -> terminal` state machine, the per-node inflight
//! bound, the bounded per-user queue for offline/busy nodes, the timeout sweep
//! and a short-lived result cache. The durable row holds digests only; the
//! argument body exists solely in the frame text handed to the tunnel.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};
use uuid::Uuid;

use wunder_core::interlink::{
    command_level, command_policy, is_queueable_when_offline, is_terminal_command_status,
    requires_hard_approval, InterlinkFrame, APPROVAL_EXPIRED, APPROVAL_NONE, APPROVAL_PENDING,
    APPROVAL_REJECTED, AUDIT_APPROVAL_DECIDE, AUDIT_COMMAND_ACK, AUDIT_COMMAND_FINISH,
    AUDIT_COMMAND_ISSUE, CMD_COMMAND_CANCEL, COMMAND_STATUS_ACKED, COMMAND_STATUS_CANCELED,
    COMMAND_STATUS_FAILED, COMMAND_STATUS_ISSUED, COMMAND_STATUS_QUEUED, COMMAND_STATUS_RUNNING,
    COMMAND_STATUS_SUCCEEDED, COMMAND_STATUS_TIMEOUT, DIRECTION_C2L, ERR_APPROVAL_EXPIRED,
    ERR_APPROVAL_REJECTED, ERR_CAP_DENIED, ERR_NODE_BUSY, ERR_NODE_OFFLINE, ERR_QUEUE_FULL,
    ERR_SERVER_RESTARTED, ERR_TIMEOUT, FRAME_COMMAND, REMOTE_FRAME_COMMAND,
};

use super::{audit, blob, digest, registry, remote, DispatchError, OutboundFrame};
use crate::core::blocking;
use crate::storage::{
    InterlinkApprovalRecord, InterlinkCommandRecord, ListInterlinkCommandsQuery, StorageBackend,
};

/// Longest time a frame body stays tracked for a server-driven retry.
const MAX_TRACKED_FRAMES: usize = 128;
const MAX_QUEUED_COMMANDS: usize = 256;
const MAX_COMMAND_ARGS_BYTES: usize = 256 * 1024;
/// Terminal result summaries kept for the polling endpoint.
const MAX_TRACKED_RESULTS: usize = 512;
const MAX_RESULT_BYTES: usize = 2 * 1024 * 1024;
const MAX_TOTAL_RESULT_BYTES: usize = 32 * 1024 * 1024;
const RESULT_TTL: Duration = Duration::from_secs(600);
/// Ledger rows scanned per sweep tick.
const SWEEP_PAGE: i64 = 200;

/// Runtime limits for the command surface (`config.interlink`).
#[derive(Debug, Clone)]
pub struct Limits {
    pub command_timeout_s: f64,
    pub inflight_per_node: usize,
    pub offline_queue_per_user: usize,
    pub require_approval_defaults: Vec<String>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            command_timeout_s: 120.0,
            inflight_per_node: 8,
            offline_queue_per_user: 32,
            require_approval_defaults: Vec::new(),
        }
    }
}

impl Limits {
    pub fn from_config(config: &wunder_core::config::InterlinkConfig) -> Self {
        Self {
            command_timeout_s: config.command_timeout_s.max(1) as f64,
            inflight_per_node: config.inflight_per_node.max(1),
            offline_queue_per_user: config.offline_queue_per_user,
            require_approval_defaults: config.require_approval_defaults.clone(),
        }
    }
}

/// Resolved target node for one issue request.
#[derive(Debug, Clone, Default)]
pub struct NodeContext {
    /// Device id for a `c2l` target; `None` for the cloud node.
    pub device_id: Option<String>,
    /// Capability set in force for the node.
    pub capabilities: Vec<String>,
    /// Admin per-device overrides.
    pub policy: digest::DevicePolicy,
}

#[derive(Debug, Clone)]
pub struct Spec<'a> {
    /// Caller-supplied idempotency key; generated when absent.
    pub command_id: Option<&'a str>,
    pub direction: &'a str,
    pub actor_user_id: &'a str,
    pub from_node: &'a str,
    /// `device:<id>` or `cloud`.
    pub to_node: &'a str,
    pub kind: &'a str,
    pub args: &'a Value,
    pub timeout_s: Option<f64>,
}

/// What happened to a freshly issued command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dispatch {
    /// Handed to the node's live tunnel.
    Sent,
    /// Held in the bounded queue (node offline or saturated).
    Queued,
    /// Target is the cloud node: the API layer executes it in-process.
    LocalTarget,
    /// Refused outright; the ledger row already carries the failure.
    Rejected(&'static str),
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub record: InterlinkCommandRecord,
    pub approval: Option<InterlinkApprovalRecord>,
    pub dispatch: Dispatch,
}

/// Issue failures the endpoint must answer distinctly (docs §13.3 10).
#[derive(Debug)]
pub enum IssueError {
    /// The id was already used: the caller returns the original state.
    Replay(InterlinkCommandRecord),
    UnknownKind(String),
    PayloadTooLarge,
    Storage(anyhow::Error),
}

impl From<anyhow::Error> for IssueError {
    fn from(value: anyhow::Error) -> Self {
        IssueError::Storage(value)
    }
}

#[derive(Debug, Clone)]
struct Queued {
    command_id: String,
    /// Normalized `device:<id>` the frame is destined for.
    target: String,
    frame: String,
}

#[derive(Debug, Default)]
struct HubState {
    /// `device_id -> command ids` currently in flight.
    inflight: HashMap<String, HashSet<String>>,
    /// `user_id -> FIFO of parked commands`.
    queued: HashMap<String, VecDeque<Queued>>,
    /// Frame bodies kept so the server can retry a busy node.
    frames: HashMap<String, String>,
    frame_order: VecDeque<String>,
    /// Result summaries for the polling endpoint.
    results: HashMap<String, CachedResult>,
    result_order: VecDeque<String>,
    result_bytes: usize,
}

#[derive(Debug)]
struct CachedResult {
    value: Value,
    bytes: usize,
    inserted: Instant,
}

// Bounded striped locks serialize progress, completion and cancellation for
// one ledger row without a growing per-command lock registry.
fn command_lock(command_id: &str) -> &'static tokio::sync::Mutex<()> {
    static LOCKS: OnceLock<[tokio::sync::Mutex<()>; 256]> = OnceLock::new();
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    command_id.hash(&mut hash);
    &LOCKS.get_or_init(|| std::array::from_fn(|_| tokio::sync::Mutex::new(())))
        [hash.finish() as usize % 256]
}

fn result_bytes(value: &Value) -> Option<usize> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.saturating_add(bytes.len());
            if self.0 > MAX_RESULT_BYTES {
                return Err(std::io::Error::other("result exceeds cache limit"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value).ok()?;
    Some(counter.0)
}

/// In-process command hub; the durable ledger is `interlink_commands`, this
/// holds only what a durable row cannot (frame bodies, queues, summaries).
#[derive(Debug, Default)]
pub struct CommandHub {
    state: Mutex<HubState>,
}

impl CommandHub {
    fn track_frame(&self, command_id: &str, frame: &str) {
        let mut state = self.lock();
        if state.frames.contains_key(command_id) {
            return;
        }
        state
            .frames
            .insert(command_id.to_string(), frame.to_string());
        state.frame_order.push_back(command_id.to_string());
        while state.frame_order.len() > MAX_TRACKED_FRAMES {
            if let Some(old) = state.frame_order.pop_front() {
                state.frames.remove(&old);
            }
        }
    }

    fn frame(&self, command_id: &str) -> Option<String> {
        self.lock().frames.get(command_id).cloned()
    }

    fn drop_frame(&self, command_id: &str) {
        let mut state = self.lock();
        state.frames.remove(command_id);
        state.frame_order.retain(|entry| entry != command_id);
        // Terminal/expired commands must release the bounded offline queue too,
        // even when that device never reconnects to drain it.
        Self::remove_queued_locked(&mut state, command_id);
    }

    fn remove_queued(&self, command_id: &str) {
        Self::remove_queued_locked(&mut self.lock(), command_id);
    }

    fn remove_queued_locked(state: &mut HubState, command_id: &str) {
        for queue in state.queued.values_mut() {
            queue.retain(|item| item.command_id != command_id);
        }
        state.queued.retain(|_, queue| !queue.is_empty());
    }

    fn track_result(&self, command_id: &str, result: Value) {
        let bytes = result_bytes(&result);
        let mut state = self.lock();
        if let Some(old) = state.results.remove(command_id) {
            state.result_bytes = state.result_bytes.saturating_sub(old.bytes);
            state.result_order.retain(|id| id != command_id);
        }
        let Some(bytes) = bytes else {
            return;
        };
        if !state.results.contains_key(command_id) {
            state.result_order.push_back(command_id.to_string());
        }
        state.result_bytes += bytes;
        state.results.insert(
            command_id.to_string(),
            CachedResult {
                value: result,
                bytes,
                inserted: Instant::now(),
            },
        );
        while state.result_order.len() > MAX_TRACKED_RESULTS
            || state.result_bytes > MAX_TOTAL_RESULT_BYTES
            || state
                .result_order
                .front()
                .and_then(|id| state.results.get(id))
                .is_some_and(|entry| entry.inserted.elapsed() > RESULT_TTL)
        {
            if let Some(old) = state.result_order.pop_front() {
                if let Some(entry) = state.results.remove(&old) {
                    state.result_bytes = state.result_bytes.saturating_sub(entry.bytes);
                }
            }
        }
    }

    /// Result/progress summary of one command (polling fallback, docs §3.2).
    pub fn result(&self, command_id: &str) -> Option<Value> {
        let state = self.lock();
        state
            .results
            .get(command_id)
            .filter(|entry| entry.inserted.elapsed() <= RESULT_TTL)
            .map(|entry| entry.value.clone())
    }

    #[cfg(test)]
    fn reserve(&self, device_id: &str, command_id: &str) {
        self.lock()
            .inflight
            .entry(device_id.to_string())
            .or_default()
            .insert(command_id.to_string());
    }

    fn try_reserve(&self, device_id: &str, command_id: &str, limit: usize) -> bool {
        let mut state = self.lock();
        let slots = state.inflight.entry(device_id.to_string()).or_default();
        if slots.len() >= limit.max(1) {
            return false;
        }
        slots.insert(command_id.to_string())
    }

    fn release(&self, device_id: &str, command_id: &str) {
        let mut state = self.lock();
        if let Some(set) = state.inflight.get_mut(device_id) {
            set.remove(command_id);
            if set.is_empty() {
                state.inflight.remove(device_id);
            }
        }
    }

    pub fn inflight_count(&self, device_id: &str) -> usize {
        self.lock()
            .inflight
            .get(device_id)
            .map(HashSet::len)
            .unwrap_or(0)
    }

    fn inflight_commands(&self, device_id: &str) -> Vec<String> {
        self.lock()
            .inflight
            .get(device_id)
            .map(|commands| commands.iter().cloned().collect())
            .unwrap_or_default()
    }

    fn has_capacity(&self, device_id: &str, limit: usize) -> bool {
        self.inflight_count(device_id) < limit.max(1)
    }

    /// Park a command for later delivery; false when the bound is reached.
    fn enqueue(&self, user_id: &str, item: Queued, bound: usize) -> bool {
        if bound == 0 {
            return false;
        }
        let mut state = self.lock();
        if state.queued.values().map(VecDeque::len).sum::<usize>() >= MAX_QUEUED_COMMANDS {
            return false;
        }
        let queue = state.queued.entry(user_id.to_string()).or_default();
        if queue.len() >= bound {
            return false;
        }
        queue.push_back(item);
        true
    }

    /// Keep the head and its capacity accounted for until delivery succeeds.
    /// Database/socket errors must not lose it or let another node steal the
    /// queue slot while a drainer awaits storage.
    fn next_for(&self, user_id: &str, target: &str) -> Option<Queued> {
        self.lock()
            .queued
            .get(user_id)?
            .iter()
            .find(|item| item.target == target)
            .cloned()
    }

    pub fn queued_depth(&self, user_id: Option<&str>) -> usize {
        let state = self.lock();
        match user_id {
            Some(user_id) => state.queued.get(user_id).map(VecDeque::len).unwrap_or(0),
            None => state.queued.values().map(VecDeque::len).sum(),
        }
    }

    /// Queue depth for one specific node (admin monitoring).
    pub fn queued_depth_for_target(&self, target: &str) -> usize {
        let state = self.lock();
        state
            .queued
            .values()
            .flat_map(|queue| queue.iter())
            .filter(|item| item.target == target)
            .count()
    }

    pub fn stats(&self) -> Value {
        let state = self.lock();
        json_object(vec![
            ("inflight_nodes", Value::from(state.inflight.len())),
            (
                "inflight_total",
                Value::from(state.inflight.values().map(HashSet::len).sum::<usize>()),
            ),
            (
                "queued_total",
                Value::from(state.queued.values().map(VecDeque::len).sum::<usize>()),
            ),
            ("tracked_frames", Value::from(state.frames.len())),
            ("tracked_results", Value::from(state.results.len())),
        ])
    }

    /// Release the vanished tunnel's capacity. Parked commands belong to the
    /// device, not its old channel, and must survive until reconnect/expiry.
    fn forget_node(&self, device_id: &str) {
        let mut state = self.lock();
        state.inflight.remove(device_id);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HubState> {
        self.state.lock().expect("command hub lock poisoned")
    }
}

pub fn hub() -> &'static CommandHub {
    static INSTANCE: OnceLock<CommandHub> = OnceLock::new();
    INSTANCE.get_or_init(CommandHub::default)
}

/// Issue one remote command (docs §4.3).
pub async fn issue(
    storage: Arc<dyn StorageBackend>,
    spec: Spec<'_>,
    limits: &Limits,
    node: NodeContext,
    now: f64,
) -> Result<Outcome, IssueError> {
    let target_device_id = device_of(spec.to_node);
    let _generation = match target_device_id.as_deref() {
        Some(device) => Some(super::device_gate(device).read().await),
        None => None,
    };
    let kind = spec.kind;
    if spec.command_id.is_some_and(|id| id.len() > 128)
        || serde_json::to_vec(spec.args).map_or(true, |args| args.len() > MAX_COMMAND_ARGS_BYTES)
    {
        return Err(IssueError::PayloadTooLarge);
    }
    if !is_known_kind(kind) {
        return Err(IssueError::UnknownKind(kind.to_string()));
    }
    let (risk, required_cap, default_approval) = command_policy(kind);

    let command_id = spec
        .command_id
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("cmd_{}", Uuid::new_v4().simple()));
    let _command = command_lock(&command_id).lock().await;

    // Approval precedence (docs §7.3 2): admin force > hard rule > server
    // default list > kind default.
    // Driving one's own cloud thread is ordinary authenticated chat admission.
    // The local-device approval boundary applies to C2L; cloud workspace writes
    // still require an explicit ticket, and in-thread tool approvals stay intact.
    let own_cloud_thread = spec.to_node == "cloud" && command_level(kind) == "L1";
    let requires_approval = node.policy.forces_approval(kind)
        || (!own_cloud_thread
            && (requires_hard_approval(kind)
                || limits
                    .require_approval_defaults
                    .iter()
                    .any(|item| item == kind)
                || default_approval));

    let args_digest_base = digest::digest_args(kind, spec.args);

    // Capability and policy gate first: a refusal must be a structurally failed
    // ledger row, never a silently downgraded command (docs §9.2).
    //
    // The cloud node has no L3 capability either: `tool.exec` and `agent.spawn`
    // are defined against a node with a human approver, and a ticket the issuer
    // approves themself on the server's own host is not one (docs §9.2, §13.5 17).
    let cloud_over_set = spec.to_node == "cloud" && command_level(kind) == "L3";
    if cloud_over_set
        || (spec.direction == DIRECTION_C2L
            && (node.policy.disables(kind)
                || (!node.capabilities.is_empty()
                    && !node.capabilities.iter().any(|cap| cap == required_cap))))
    {
        let summary = if cloud_over_set {
            "cloud_target_has_no_l3_capability".to_string()
        } else if node.policy.disables(kind) {
            "admin_disabled_kind".to_string()
        } else {
            format!("missing_capability:{required_cap}")
        };
        let record = InterlinkCommandRecord {
            command_id: command_id.clone(),
            direction: spec.direction.to_string(),
            actor_user_id: spec.actor_user_id.to_string(),
            from_node: spec.from_node.to_string(),
            to_node: spec.to_node.to_string(),
            kind: kind.to_string(),
            args_digest: Some(args_digest_base),
            approval_state: if requires_approval {
                APPROVAL_PENDING.to_string()
            } else {
                APPROVAL_NONE.to_string()
            },
            status: COMMAND_STATUS_FAILED.to_string(),
            created_at: now,
            acked_at: None,
            finished_at: Some(now),
            error_code: Some(ERR_CAP_DENIED.to_string()),
            error_summary: Some(summary),
        };
        match insert_ledger(storage.clone(), &record, None).await? {
            InsertOutcome::Replay(existing) => return Err(IssueError::Replay(existing)),
            InsertOutcome::Inserted => {}
        }
        write_audit(
            storage.clone(),
            AUDIT_COMMAND_ISSUE,
            &record,
            None,
            vec![("result", Value::String(ERR_CAP_DENIED.to_string()))],
        )
        .await;
        return Ok(Outcome {
            record,
            approval: None,
            dispatch: Dispatch::Rejected(ERR_CAP_DENIED),
        });
    }

    // Mint the ticket before the ledger row so the row can point at it.
    // Cloud targets decide through the user API (`commands/{id}/approval`),
    // device targets through the on-node prompt; both need a ticket.
    let approval =
        if requires_approval && (spec.direction == DIRECTION_C2L || spec.to_node == "cloud") {
            let device_id = node
                .device_id
                .clone()
                .or_else(|| device_of(spec.to_node))
                .unwrap_or_else(|| "cloud".to_string());
            Some(super::approvals::build(
                &super::approvals::ApprovalSpec {
                    command_id: &command_id,
                    device_id: &device_id,
                    user_id: spec.actor_user_id,
                    kind,
                    from_label: spec.from_node,
                    args: spec.args,
                    timeout_s: spec.timeout_s,
                },
                now,
            ))
        } else {
            None
        };

    // The digest doubles as the ledger's copy of the two structural fields the
    // table has no column for: the ticket id and the negotiated timeout.
    let mut args_digest = args_digest_base;
    {
        if let Ok(mut map) = serde_json::from_str::<Map<String, Value>>(&args_digest) {
            if let Some(ticket) = approval.as_ref() {
                map.insert(
                    "approval".to_string(),
                    Value::String(ticket.approval_id.clone()),
                );
            }
            map.insert(
                "timeout_s".to_string(),
                Value::from(spec.timeout_s.unwrap_or(limits.command_timeout_s)),
            );
            args_digest = serde_json::to_string(&Value::Object(map)).unwrap_or(args_digest);
        }
    }

    let mut record = InterlinkCommandRecord {
        command_id: command_id.clone(),
        direction: spec.direction.to_string(),
        actor_user_id: spec.actor_user_id.to_string(),
        from_node: spec.from_node.to_string(),
        to_node: spec.to_node.to_string(),
        kind: kind.to_string(),
        args_digest: Some(args_digest),
        approval_state: if requires_approval {
            APPROVAL_PENDING.to_string()
        } else {
            APPROVAL_NONE.to_string()
        },
        status: COMMAND_STATUS_ISSUED.to_string(),
        created_at: now,
        acked_at: None,
        finished_at: None,
        error_code: None,
        error_summary: None,
    };

    match insert_ledger(storage.clone(), &record, approval.as_ref()).await? {
        InsertOutcome::Inserted => {}
        InsertOutcome::Replay(existing) => return Err(IssueError::Replay(existing)),
    }

    write_audit(
        storage.clone(),
        AUDIT_COMMAND_ISSUE,
        &record,
        approval.as_ref().map(|ticket| ticket.approval_id.clone()),
        vec![("risk", Value::String(risk.to_string()))],
    )
    .await;

    // Cloud target: the API layer runs it with the existing handlers.
    if spec.to_node == "cloud" {
        return Ok(Outcome {
            record,
            approval,
            dispatch: Dispatch::LocalTarget,
        });
    }

    let device_id = node
        .device_id
        .clone()
        .or_else(|| device_of(spec.to_node))
        .unwrap_or_default();
    let frame_text = command_frame_text(&record, approval.as_ref(), spec.args, limits, risk);
    let target = format!("device:{device_id}");
    hub().track_frame(&record.command_id, &frame_text);

    let dispatch = match registry().by_device(&device_id) {
        Some(live) => {
            match if hub().queued_depth_for_target(&target) > 0
                || !hub().try_reserve(&device_id, &record.command_id, limits.inflight_per_node)
            {
                Err(DispatchError::QueueFull)
            } else {
                registry().dispatch(
                    &device_id,
                    &live.channel_id,
                    OutboundFrame::Text(frame_text.clone()),
                )
            } {
                Ok(()) => {
                    hub().track_frame(&record.command_id, &frame_text);
                    Dispatch::Sent
                }
                Err(DispatchError::QueueFull) => {
                    hub().release(&device_id, &record.command_id);
                    park(
                        storage.clone(),
                        &record,
                        spec.actor_user_id,
                        &target,
                        &frame_text,
                        limits,
                        now,
                        ERR_NODE_BUSY,
                    )
                    .await?
                }
                Err(DispatchError::Superseded | DispatchError::NoChannel) => {
                    hub().release(&device_id, &record.command_id);
                    park(
                        storage.clone(),
                        &record,
                        spec.actor_user_id,
                        &target,
                        &frame_text,
                        limits,
                        now,
                        ERR_NODE_OFFLINE,
                    )
                    .await?
                }
            }
        }
        None => {
            park(
                storage.clone(),
                &record,
                spec.actor_user_id,
                &target,
                &frame_text,
                limits,
                now,
                ERR_NODE_OFFLINE,
            )
            .await?
        }
    };

    // A refusal that flipped the row is reflected on the returned record.
    if let Dispatch::Rejected(code) = &dispatch {
        record.status = COMMAND_STATUS_FAILED.to_string();
        record.finished_at = Some(now);
        record.error_code = Some(code.to_string());
        record.error_summary = Some(code.to_string());
    } else if dispatch == Dispatch::Queued {
        record.status = COMMAND_STATUS_QUEUED.to_string();
    }

    Ok(Outcome {
        record,
        approval,
        dispatch,
    })
}

/// Park a command in the bounded per-user queue, or fail it structurally.
///
/// Docs §4.3: an offline target only accepts non-mutating commands, and a
/// depth-0 queue means "reject immediately".
async fn park(
    storage: Arc<dyn StorageBackend>,
    record: &InterlinkCommandRecord,
    user_id: &str,
    target: &str,
    frame_text: &str,
    limits: &Limits,
    now: f64,
    reason: &'static str,
) -> anyhow::Result<Dispatch> {
    let node_live = device_of(target)
        .map(|id| registry().by_device(&id).is_some())
        .unwrap_or(false);
    let queueable = is_queueable_when_offline(&record.kind);
    if (!node_live && !queueable) || limits.offline_queue_per_user == 0 {
        let code = if node_live {
            ERR_NODE_BUSY
        } else {
            ERR_NODE_OFFLINE
        };
        mark_status(
            storage,
            &record.command_id,
            COMMAND_STATUS_FAILED,
            None,
            Some(now),
            Some(code),
            Some(code),
        )
        .await?;
        hub().drop_frame(&record.command_id);
        return Ok(Dispatch::Rejected(code));
    }
    let item = Queued {
        command_id: record.command_id.clone(),
        target: target.to_string(),
        frame: frame_text.to_string(),
    };
    if hub().enqueue(user_id, item, limits.offline_queue_per_user) {
        hub().track_frame(&record.command_id, frame_text);
        mark_status(
            storage.clone(),
            &record.command_id,
            COMMAND_STATUS_QUEUED,
            None,
            None,
            Some(reason),
            None,
        )
        .await?;
        // A parked command whose channel is live is transient backpressure, not
        // an offline node: the backlog only drains on an event edge (a channel
        // opening, a command finishing), so without this retry the command would
        // sit until the timeout sweep fails it (docs §13.4 11).
        if node_live {
            if let Some(device_id) = device_of(target) {
                retry_parked_queue(storage, user_id.to_string(), device_id, limits.clone());
            }
        }
        return Ok(Dispatch::Queued);
    }
    mark_status(
        storage,
        &record.command_id,
        COMMAND_STATUS_FAILED,
        None,
        Some(now),
        Some(ERR_QUEUE_FULL),
        Some(reason),
    )
    .await?;
    hub().drop_frame(&record.command_id);
    Ok(Dispatch::Rejected(ERR_QUEUE_FULL))
}

/// One bounded retry of the backlog shortly after parking. A single attempt: if
/// the outbound queue is still full the drain retains the head and stops,
/// and the janitor backstop (`drain_backlogs`) covers the longer waits.
fn retry_parked_queue(
    storage: Arc<dyn StorageBackend>,
    user_id: String,
    device_id: String,
    limits: Limits,
) {
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let _ = drain(storage, &user_id, &device_id, &limits).await;
    });
}

/// Build the `command` frame body handed to the tunnel (docs §4.2).
pub fn command_frame_text(
    record: &InterlinkCommandRecord,
    approval: Option<&InterlinkApprovalRecord>,
    args: &Value,
    limits: &Limits,
    risk: &str,
) -> String {
    let payload = json_object(vec![
        ("kind", Value::String(record.kind.clone())),
        ("risk", Value::String(risk.to_string())),
        (
            "level",
            Value::String(command_level(&record.kind).to_string()),
        ),
        (
            "requires_approval",
            Value::Bool(record.approval_state != APPROVAL_NONE),
        ),
        (
            "approval_id",
            approval
                .map(|ticket| Value::String(ticket.approval_id.clone()))
                .unwrap_or(Value::Null),
        ),
        (
            "approval_expires_at",
            approval
                .map(|ticket| Value::from(ticket.expires_at))
                .unwrap_or(Value::Null),
        ),
        ("timeout_s", Value::from(record_timeout_s(record, limits))),
        (
            "capability",
            Value::String(command_policy(&record.kind).1.to_string()),
        ),
        ("from", Value::String(record.from_node.clone())),
        ("actor", Value::String(record.actor_user_id.clone())),
        ("args", args.clone()),
    ]);
    let frame = InterlinkFrame::new(
        FRAME_COMMAND,
        format!("frm_{}", Uuid::new_v4().simple()),
        record.created_at,
        None,
        payload,
    )
    .with_corr(record.command_id.clone());
    serde_json::to_string(&frame).unwrap_or_else(|_| "{}".to_string())
}

/// A control frame cancelling an in-flight command.
pub fn cancel_frame_text(command_id: &str, reason: &str, now: f64) -> String {
    let payload = json_object(vec![
        ("kind", Value::String(CMD_COMMAND_CANCEL.to_string())),
        ("control", Value::Bool(true)),
        ("target_command_id", Value::String(command_id.to_string())),
        ("reason", Value::String(reason.to_string())),
    ]);
    let frame = InterlinkFrame::new(
        FRAME_COMMAND,
        format!("frm_{}", Uuid::new_v4().simple()),
        now,
        None,
        payload,
    )
    .with_corr(command_id.to_string());
    serde_json::to_string(&frame).unwrap_or_else(|_| "{}".to_string())
}

/// The timeout of one command: the ledger keeps it inside its digest so a
/// retry or sweep uses the value the issuer asked for.
fn record_timeout_s(record: &InterlinkCommandRecord, limits: &Limits) -> f64 {
    digest_field(record, "timeout_s")
        .and_then(|value| value.as_f64())
        .unwrap_or(limits.command_timeout_s)
}

fn digest_field(record: &InterlinkCommandRecord, key: &str) -> Option<Value> {
    serde_json::from_str::<Value>(record.args_digest.as_deref()?)
        .ok()
        .and_then(|value| value.get(key).cloned())
}

/// Outcome of a `command_ack` frame (docs §4.2 phase 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AckOutcome {
    Acked,
    /// Approval refused (or expired) on the device: no execution happened.
    Refused(String),
    /// Node busy: parked again for a server-driven retry.
    Retried,
    /// The command had already reached a terminal state.
    Late(String),
    Unknown,
}

/// Handle a `command_ack` frame from the node.
pub async fn on_ack(
    storage: Arc<dyn StorageBackend>,
    command_id: &str,
    payload: &Value,
    limits: &Limits,
    now: f64,
) -> anyhow::Result<AckOutcome> {
    let _guard = command_lock(command_id).lock().await;
    let Some(record) = load(storage.clone(), command_id).await? else {
        return Ok(AckOutcome::Unknown);
    };
    if is_terminal_command_status(&record.status) {
        return Ok(AckOutcome::Late(record.status));
    }

    let accepted = payload
        .get("accepted")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let approval_state = payload
        .get("approval_state")
        .and_then(Value::as_str)
        .unwrap_or(record.approval_state.as_str())
        .to_string();
    let device_id = target_device(&record);

    if !accepted {
        // The node names why it refused. Anything but its own busy signal is
        // terminal: re-queueing a command the node will not run is a storm.
        let refusal = payload
            .get("error_code")
            .and_then(Value::as_str)
            .filter(|code| *code != ERR_NODE_BUSY)
            .map(str::to_string);
        let approval_refused = matches!(
            approval_state.as_str(),
            APPROVAL_REJECTED | APPROVAL_EXPIRED
        );
        if approval_refused || refusal.is_some() {
            let code = match (approval_refused, refusal) {
                (true, _) if approval_state == APPROVAL_EXPIRED => ERR_APPROVAL_EXPIRED.to_string(),
                (true, _) => ERR_APPROVAL_REJECTED.to_string(),
                (false, Some(code)) => code,
                (false, None) => ERR_APPROVAL_REJECTED.to_string(),
            };
            if approval_refused {
                set_approval(storage.clone(), command_id, &approval_state).await?;
                decide_ticket(storage.clone(), &record, &approval_state, "device", now).await?;
            }
            finalize_unlocked(
                storage.clone(),
                command_id,
                COMMAND_STATUS_FAILED,
                Some(now),
                Some(code.as_str()),
                Some(code.as_str()),
            )
            .await?;
            if let Some(device_id) = &device_id {
                hub().release(device_id, command_id);
            }
            return Ok(AckOutcome::Refused(approval_state));
        }
        // Capacity refusal: the server owns the retry, the client never re-sends.
        let eta_ms = payload.get("eta_s").and_then(Value::as_f64);
        let _ = eta_ms;
        if let (Some(frame), Some(target)) = (hub().frame(command_id), device_of(&record.to_node)) {
            if record.status == COMMAND_STATUS_QUEUED {
                return Ok(AckOutcome::Retried);
            }
            if let Some(device_id) = &device_id {
                hub().release(device_id, command_id);
            }
            let item = Queued {
                command_id: command_id.to_string(),
                target: format!("device:{target}"),
                frame,
            };
            if hub().enqueue(
                &record.actor_user_id,
                item,
                limits.offline_queue_per_user.max(1),
            ) {
                mark_status(
                    storage.clone(),
                    command_id,
                    COMMAND_STATUS_QUEUED,
                    None,
                    None,
                    Some(ERR_NODE_BUSY),
                    None,
                )
                .await?;
                retry_parked_queue(
                    storage.clone(),
                    record.actor_user_id.clone(),
                    target,
                    limits.clone(),
                );
                return Ok(AckOutcome::Retried);
            }
        }
        finalize_unlocked(
            storage.clone(),
            command_id,
            COMMAND_STATUS_FAILED,
            Some(now),
            Some(ERR_NODE_BUSY),
            None,
        )
        .await?;
        return Ok(AckOutcome::Refused(ERR_NODE_BUSY.to_string()));
    }

    if record.approval_state == APPROVAL_PENDING {
        set_approval(storage.clone(), command_id, &approval_state).await?;
        decide_ticket(storage.clone(), &record, &approval_state, "device", now).await?;
    }
    update_status(
        storage.clone(),
        command_id,
        if record.status == COMMAND_STATUS_RUNNING {
            COMMAND_STATUS_RUNNING
        } else {
            COMMAND_STATUS_ACKED
        },
        Some(now),
        None,
        None,
        None,
    )
    .await?;
    write_audit(
        storage.clone(),
        AUDIT_COMMAND_ACK,
        &record,
        approval_id_of(&record),
        vec![("approval_state", Value::String(approval_state))],
    )
    .await;
    if let Some(device_id) = &device_id {
        remote::hub().publish_control(
            device_id,
            &lifecycle_json(&record.kind, COMMAND_STATUS_ACKED, command_id),
        );
    }
    Ok(AckOutcome::Acked)
}

/// Handle a `command_event` progress frame.
pub async fn on_event(
    storage: Arc<dyn StorageBackend>,
    command_id: &str,
    payload: &Value,
    now: f64,
) -> anyhow::Result<()> {
    let _guard = command_lock(command_id).lock().await;
    let Some(record) = load(storage.clone(), command_id).await? else {
        return Ok(());
    };
    if is_terminal_command_status(&record.status) {
        return Ok(());
    }
    if record.status != COMMAND_STATUS_RUNNING {
        update_status(
            storage,
            command_id,
            COMMAND_STATUS_RUNNING,
            Some(now),
            None,
            None,
            None,
        )
        .await?;
    }
    hub().track_result(
        command_id,
        json_object(vec![
            ("progress", payload.clone()),
            ("at", Value::from(now)),
        ]),
    );
    Ok(())
}

/// Handle a `command_result` terminal frame (docs §4.3: first terminal wins).
pub async fn on_result(
    storage: Arc<dyn StorageBackend>,
    command_id: &str,
    payload: &Value,
    _limits: &Limits,
    now: f64,
) -> anyhow::Result<bool> {
    let _guard = command_lock(command_id).lock().await;
    let Some(record) = load(storage.clone(), command_id).await? else {
        return Ok(false);
    };
    let requested = payload
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or(COMMAND_STATUS_SUCCEEDED);
    let status = match requested {
        COMMAND_STATUS_SUCCEEDED
        | COMMAND_STATUS_FAILED
        | COMMAND_STATUS_CANCELED
        | COMMAND_STATUS_TIMEOUT => requested,
        _ => COMMAND_STATUS_FAILED,
    };
    let error_code = payload
        .pointer("/error/code")
        .and_then(Value::as_str)
        .or_else(|| payload.get("error_code").and_then(Value::as_str));
    let error_summary = payload
        .pointer("/error/message")
        .and_then(Value::as_str)
        .map(|text| text.chars().take(200).collect::<String>());

    if is_terminal_command_status(&record.status) {
        // Duplicate terminal: audited for the trail, ledger untouched.
        write_audit(
            storage.clone(),
            AUDIT_COMMAND_FINISH,
            &record,
            approval_id_of(&record),
            vec![
                ("status", Value::String(status.to_string())),
                ("duplicate", Value::Bool(true)),
            ],
        )
        .await;
        return Ok(false);
    }

    // The HTTP record exposes the operation payload directly; status/error
    // belong to the ledger and transport metadata has its own stable fields.
    let mut result = payload.get("result").cloned().unwrap_or_else(|| json!({}));
    if let Some(map) = result.as_object_mut() {
        for key in ["usage", "inline", "stream_id", "size", "mime", "max_bytes"] {
            if let Some(value) = payload.get(key) {
                map.insert(key.to_string(), value.clone());
            }
        }
    }
    hub().track_result(command_id, result);

    let device_id = target_device(&record);
    finalize_unlocked(
        storage.clone(),
        command_id,
        status,
        Some(now),
        error_code,
        error_summary.as_deref(),
    )
    .await?;
    drop(_guard);
    if let Some(device_id) = &device_id {
        hub().release(device_id, command_id);
        remote::hub().publish_control(device_id, &lifecycle_json(&record.kind, status, command_id));
    }
    if status != COMMAND_STATUS_SUCCEEDED {
        blob::store().drop_command(command_id);
    }
    Ok(true)
}

/// Cancel a command that has not reached a terminal state.
pub async fn cancel(
    storage: Arc<dyn StorageBackend>,
    command_id: &str,
    actor: &str,
    now: f64,
) -> anyhow::Result<CancelOutcome> {
    let _guard = command_lock(command_id).lock().await;
    let Some(record) = load(storage.clone(), command_id).await? else {
        return Ok(CancelOutcome::Unknown);
    };
    if is_terminal_command_status(&record.status) {
        return Ok(CancelOutcome::AlreadyTerminal(record.status));
    }
    if record.actor_user_id != actor {
        return Ok(CancelOutcome::Forbidden);
    }
    if let Some(device_id) = target_device(&record) {
        if let Some(live) = registry().by_device(&device_id) {
            let _ = registry().dispatch(
                &device_id,
                &live.channel_id,
                OutboundFrame::Text(cancel_frame_text(command_id, "user_cancel", now)),
            );
            hub().release(&device_id, command_id);
        }
    }
    finalize_unlocked(
        storage.clone(),
        command_id,
        COMMAND_STATUS_CANCELED,
        Some(now),
        Some("CANCELED"),
        Some("user_cancel"),
    )
    .await?;
    Ok(CancelOutcome::Canceled)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancelOutcome {
    Canceled,
    AlreadyTerminal(String),
    /// Only the issuer may cancel their own command.
    Forbidden,
    Unknown,
}

/// Push parked commands onto a node that just became available (docs §4.3).
pub async fn drain(
    storage: Arc<dyn StorageBackend>,
    user_id: &str,
    device_id: &str,
    limits: &Limits,
) -> anyhow::Result<usize> {
    // One bounded drainer owns FIFO delivery; replacement and admission wait.
    let _generation = super::device_gate(device_id).write().await;
    let Some(live) = registry().by_device(device_id) else {
        return Ok(0);
    };
    let target = format!("device:{device_id}");
    let mut delivered = 0usize;
    while hub().has_capacity(device_id, limits.inflight_per_node) {
        let Some(item) = hub().next_for(user_id, &target) else {
            break;
        };
        let _command = command_lock(&item.command_id).lock().await;
        if load(storage.clone(), &item.command_id)
            .await?
            .is_none_or(|record| is_terminal_command_status(&record.status))
        {
            hub().drop_frame(&item.command_id);
            continue;
        }
        if !hub().try_reserve(device_id, &item.command_id, limits.inflight_per_node) {
            break;
        }
        // Persist before publishing: an immediate acknowledgement must not be
        // overwritten by a slower `queued -> issued` database update.
        if let Err(error) = update_status(
            storage.clone(),
            &item.command_id,
            COMMAND_STATUS_ISSUED,
            None,
            None,
            None,
            None,
        )
        .await
        {
            hub().release(device_id, &item.command_id);
            return Err(error);
        }
        match registry().dispatch(
            device_id,
            &live.channel_id,
            OutboundFrame::Text(item.frame.clone()),
        ) {
            Ok(()) => {
                hub().track_frame(&item.command_id, &item.frame);
                hub().remove_queued(&item.command_id);
                delivered += 1;
            }
            Err(_) => {
                hub().release(device_id, &item.command_id);
                update_status(
                    storage.clone(),
                    &item.command_id,
                    COMMAND_STATUS_QUEUED,
                    None,
                    None,
                    Some(ERR_NODE_BUSY),
                    None,
                )
                .await?;
                break;
            }
        }
    }
    Ok(delivered)
}

/// Backstop for parked commands. The offline queue drains on the event edges
/// only (a channel opening, a command finishing), so a command that was parked
/// while its channel was actually live would sit there until the timeout sweep
/// fails it (docs §13.4 11). One pass per janitor tick, and only for nodes that
/// really hold a backlog.
pub async fn drain_backlogs(storage: Arc<dyn StorageBackend>, limits: &Limits) -> usize {
    let mut delivered = 0usize;
    for live in registry().snapshot() {
        if hub().queued_depth(Some(live.user_id.as_str())) == 0 {
            continue;
        }
        if let Ok(count) = drain(storage.clone(), &live.user_id, &live.device_id, limits).await {
            delivered += count;
        }
    }
    delivered
}

/// Timeout + approval-expiry sweep (docs §4.3, §7.3 4).
pub async fn sweep(
    storage: Arc<dyn StorageBackend>,
    limits: &Limits,
    now: f64,
) -> anyhow::Result<usize> {
    let mut swept = 0usize;
    for status in [
        COMMAND_STATUS_ISSUED,
        COMMAND_STATUS_QUEUED,
        COMMAND_STATUS_ACKED,
        COMMAND_STATUS_RUNNING,
    ] {
        let rows = {
            let storage = storage.clone();
            blocking::run_db("interlink.command.sweep_list", move || {
                storage.list_interlink_commands(ListInterlinkCommandsQuery {
                    user_id: None,
                    device_id: None,
                    kind: None,
                    status: Some(status),
                    direction: None,
                    offset: 0,
                    limit: SWEEP_PAGE,
                })
            })
            .await
            .map(|(rows, _total)| rows)?
        };
        for record in rows {
            let _guard = command_lock(&record.command_id).lock().await;
            let Some(record) = load(storage.clone(), &record.command_id).await? else {
                continue;
            };
            if is_terminal_command_status(&record.status) {
                continue;
            }
            let timeout = record_timeout_s(&record, limits);
            if now - record.created_at <= timeout {
                continue;
            }
            if let Some(device_id) = target_device(&record) {
                if let Some(live) = registry().by_device(&device_id) {
                    let _ = registry().dispatch(
                        &device_id,
                        &live.channel_id,
                        OutboundFrame::Text(cancel_frame_text(&record.command_id, "timeout", now)),
                    );
                }
                hub().release(&device_id, &record.command_id);
            }
            hub().drop_frame(&record.command_id);
            let approval_expired = record.approval_state == APPROVAL_PENDING;
            let code = if approval_expired {
                ERR_APPROVAL_EXPIRED
            } else {
                ERR_TIMEOUT
            };
            update_status(
                storage.clone(),
                &record.command_id,
                COMMAND_STATUS_TIMEOUT,
                None,
                Some(now),
                Some(code),
                Some(code),
            )
            .await?;
            decide_ticket(
                storage.clone(),
                &record,
                APPROVAL_EXPIRED,
                "system:expiry",
                now,
            )
            .await?;
            write_audit(
                storage.clone(),
                AUDIT_COMMAND_FINISH,
                &record,
                approval_id_of(&record),
                vec![
                    ("status", Value::String(COMMAND_STATUS_TIMEOUT.to_string())),
                    ("code", Value::String(code.to_string())),
                ],
            )
            .await;
            swept += 1;
        }
    }
    Ok(swept)
}

/// Fail commands handed to the retired generation, including pending device
/// prompts. Parked commands survive for reconnect. Acquires the device write
/// gate so a replacement cannot reserve slots during settlement.
pub async fn fail_device_commands(
    storage: Arc<dyn StorageBackend>,
    device_id: &str,
    now: f64,
) -> anyhow::Result<usize> {
    let _generation = super::device_gate(device_id).write().await;
    fail_device_commands_locked(storage, device_id, now).await
}

pub(crate) async fn fail_device_commands_locked(
    storage: Arc<dyn StorageBackend>,
    device_id: &str,
    now: f64,
) -> anyhow::Result<usize> {
    // Only the retired generation's actual reservations are relevant. A recent
    // history page can hide an older active command behind completed rows.
    // Lifecycle callers exclude admission/draining with the device write gate.
    let commands = hub().inflight_commands(device_id);
    let mut failed = 0usize;
    for command_id in commands {
        let _guard = command_lock(&command_id).lock().await;
        let Some(record) = load(storage.clone(), &command_id).await? else {
            hub().release(device_id, &command_id);
            continue;
        };
        if is_terminal_command_status(&record.status) {
            hub().release(device_id, &command_id);
            continue;
        }
        // Only a command the node actually got is "in flight". The parked backlog
        // exists to survive this exact moment (docs §4.3: it drains on reconnect),
        // while a prompt already sent down this tunnel cannot be recovered.
        if record.status == COMMAND_STATUS_QUEUED {
            continue;
        }
        if record.approval_state == APPROVAL_PENDING {
            decide_ticket(
                storage.clone(),
                &record,
                APPROVAL_EXPIRED,
                "system:disconnect",
                now,
            )
            .await?;
            set_approval(storage.clone(), &command_id, APPROVAL_EXPIRED).await?;
        }
        finalize_unlocked(
            storage.clone(),
            &command_id,
            COMMAND_STATUS_FAILED,
            Some(now),
            Some(ERR_NODE_OFFLINE),
            Some(ERR_NODE_OFFLINE),
        )
        .await?;
        hub().release(device_id, &command_id);
        blob::store().drop_command(&command_id);
        failed += 1;
    }
    hub().forget_node(device_id);
    Ok(failed)
}

/// One recovery page per maintenance pass. The durable ledger deliberately
/// contains no arguments, so a process restart settles orphaned commands with
/// an explicit reason instead of reconstructing and replaying their effects.
pub async fn recover_after_restart(
    storage: Arc<dyn StorageBackend>,
    before: f64,
    now: f64,
) -> anyhow::Result<usize> {
    let rows = {
        let storage = storage.clone();
        blocking::run_db("interlink.command.restart_page", move || {
            storage.list_interlink_unfinished_before(before, SWEEP_PAGE)
        })
        .await?
    };
    let mut recovered = 0;
    for row in rows {
        let _guard = command_lock(&row.command_id).lock().await;
        let Some(record) = load(storage.clone(), &row.command_id).await? else {
            continue;
        };
        if is_terminal_command_status(&record.status) || record.created_at >= before {
            continue;
        }
        if record.approval_state == APPROVAL_PENDING {
            decide_ticket(
                storage.clone(),
                &record,
                APPROVAL_EXPIRED,
                "system:restart",
                now,
            )
            .await?;
            set_approval(storage.clone(), &record.command_id, APPROVAL_EXPIRED).await?;
        }
        finalize_unlocked(
            storage.clone(),
            &record.command_id,
            COMMAND_STATUS_FAILED,
            Some(now),
            Some(ERR_SERVER_RESTARTED),
            Some("server restarted; execution outcome may be unknown; operation was not replayed"),
        )
        .await?;
        if let Some(device) = target_device(&record) {
            hub().release(&device, &record.command_id);
        }
        blob::store().drop_command(&record.command_id);
        recovered += 1;
    }
    Ok(recovered)
}

/// Persist a terminal state and audit it.
pub async fn finalize(
    storage: Arc<dyn StorageBackend>,
    command_id: &str,
    status: &str,
    finished_at: Option<f64>,
    error_code: Option<&str>,
    error_summary: Option<&str>,
) -> anyhow::Result<()> {
    let _guard = command_lock(command_id).lock().await;
    finalize_unlocked(
        storage,
        command_id,
        status,
        finished_at,
        error_code,
        error_summary,
    )
    .await
}

async fn finalize_unlocked(
    storage: Arc<dyn StorageBackend>,
    command_id: &str,
    status: &str,
    finished_at: Option<f64>,
    error_code: Option<&str>,
    error_summary: Option<&str>,
) -> anyhow::Result<()> {
    let Some(record) = load(storage.clone(), command_id).await? else {
        return Ok(());
    };
    if is_terminal_command_status(&record.status) {
        return Ok(());
    }
    // Audit first, status second: a reader that can already see the terminal
    // state must also find the `command.finish` row in the trail. The other
    // order lets an export taken right after a command finishes miss its last
    // row (docs §13.4 19). The trail is append-only, so a retried finalize can
    // add a second row rather than lose one.
    write_audit(
        storage.clone(),
        AUDIT_COMMAND_FINISH,
        &record,
        approval_id_of(&record),
        vec![
            ("status", Value::String(status.to_string())),
            (
                "code",
                error_code
                    .map(|code| Value::String(code.to_string()))
                    .unwrap_or(Value::Null),
            ),
        ],
    )
    .await;
    update_status(
        storage.clone(),
        command_id,
        status,
        None,
        finished_at,
        error_code,
        error_summary,
    )
    .await?;
    if let Some(device_id) = target_device(&record) {
        remote::hub().publish_control(
            &device_id,
            &lifecycle_json(&record.kind, status, command_id),
        );
    }
    Ok(())
}

/// Record a terminal state for a command the cloud node executed in-process.
pub async fn finish_local(
    storage: Arc<dyn StorageBackend>,
    command_id: &str,
    status: &str,
    result: Value,
    error_code: Option<&str>,
    now: f64,
) -> anyhow::Result<()> {
    let _guard = command_lock(command_id).lock().await;
    if load(storage.clone(), command_id)
        .await?
        .is_none_or(|row| is_terminal_command_status(&row.status))
    {
        return Ok(());
    }
    hub().track_result(command_id, result);
    finalize_unlocked(storage, command_id, status, Some(now), error_code, None).await
}

async fn decide_ticket(
    storage: Arc<dyn StorageBackend>,
    record: &InterlinkCommandRecord,
    state: &str,
    decided_by: &str,
    now: f64,
) -> anyhow::Result<()> {
    let Some(approval_id) = approval_id_of(record) else {
        return Ok(());
    };
    if let Some(ticket) =
        super::approvals::decide(storage.clone(), &approval_id, state, decided_by, now).await?
    {
        write_audit(
            storage,
            AUDIT_APPROVAL_DECIDE,
            record,
            Some(ticket.approval_id),
            vec![("state", Value::String(state.to_string()))],
        )
        .await;
    }
    Ok(())
}

async fn update_status(
    storage: Arc<dyn StorageBackend>,
    command_id: &str,
    status: &str,
    acked_at: Option<f64>,
    finished_at: Option<f64>,
    error_code: Option<&str>,
    error_summary: Option<&str>,
) -> anyhow::Result<()> {
    let lookup = command_id.to_string();
    let terminal = is_terminal_command_status(status);
    let status = status.to_string();
    let error_code = error_code.map(str::to_string);
    let error_summary = error_summary.map(str::to_string);
    blocking::run_db("interlink.command.status", move || {
        storage.update_interlink_command_status(
            &lookup,
            &status,
            acked_at,
            finished_at,
            error_code.as_deref(),
            error_summary.as_deref(),
        )
    })
    .await?;
    if terminal {
        hub().drop_frame(command_id);
    }
    Ok(())
}

async fn mark_status(
    storage: Arc<dyn StorageBackend>,
    command_id: &str,
    status: &str,
    acked_at: Option<f64>,
    finished_at: Option<f64>,
    error_code: Option<&str>,
    error_summary: Option<&str>,
) -> anyhow::Result<()> {
    update_status(
        storage,
        command_id,
        status,
        acked_at,
        finished_at,
        error_code,
        error_summary,
    )
    .await
}

async fn set_approval(
    storage: Arc<dyn StorageBackend>,
    command_id: &str,
    state: &str,
) -> anyhow::Result<()> {
    let lookup = command_id.to_string();
    let state = state.to_string();
    blocking::run_db("interlink.command.approval", move || {
        storage.set_interlink_command_approval(&lookup, &state)
    })
    .await
}

enum InsertOutcome {
    Inserted,
    Replay(InterlinkCommandRecord),
}

async fn insert_ledger(
    storage: Arc<dyn StorageBackend>,
    record: &InterlinkCommandRecord,
    approval: Option<&InterlinkApprovalRecord>,
) -> anyhow::Result<InsertOutcome> {
    let owned = record.clone();
    let inserted = {
        let storage = storage.clone();
        blocking::run_db("interlink.command.insert", move || {
            storage.insert_interlink_command(&owned)
        })
        .await?
    };
    if inserted {
        if let Some(ticket) = approval {
            let ticket = ticket.clone();
            let storage = storage.clone();
            blocking::run_db("interlink.approval.insert", move || {
                storage.insert_interlink_approval(&ticket)
            })
            .await?;
        }
        return Ok(InsertOutcome::Inserted);
    }
    let lookup = record.command_id.clone();
    let existing = {
        let storage = storage.clone();
        blocking::run_db("interlink.command.replay", move || {
            storage.get_interlink_command(&lookup)
        })
        .await?
    };
    match existing {
        Some(record) => Ok(InsertOutcome::Replay(record)),
        None => Err(anyhow::anyhow!("command id collision without a ledger row")),
    }
}

async fn load(
    storage: Arc<dyn StorageBackend>,
    command_id: &str,
) -> anyhow::Result<Option<InterlinkCommandRecord>> {
    let lookup = command_id.to_string();
    blocking::run_db("interlink.command.get", move || {
        storage.get_interlink_command(&lookup)
    })
    .await
}

async fn write_audit(
    storage: Arc<dyn StorageBackend>,
    action: &str,
    record: &InterlinkCommandRecord,
    approval_id: Option<String>,
    detail: Vec<(&str, Value)>,
) {
    let row = audit::record(
        action,
        &record.actor_user_id,
        Some(&record.from_node),
        Some(&record.to_node),
        Some(&record.command_id),
        approval_id.as_deref(),
        detail,
    );
    let _ = blocking::run_db("interlink.audit.write", move || {
        storage.insert_interlink_audit(&row)
    })
    .await;
}

/// `device:<id>` -> `Some(<id>)`; the cloud node has no tunnel target.
pub fn target_device(record: &InterlinkCommandRecord) -> Option<String> {
    device_of(&record.to_node)
}

pub fn device_of(node: &str) -> Option<String> {
    node.strip_prefix("device:")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// The approval id travels inside the args digest JSON (docs §3.1 keeps no
/// dedicated column; the ticket links back through its `command_id`).
pub fn approval_id_of(record: &InterlinkCommandRecord) -> Option<String> {
    digest_field(record, "approval").and_then(|value| value.as_str().map(str::to_string))
}

fn lifecycle_json(kind: &str, status: &str, command_id: &str) -> String {
    serde_json::to_string(&json_object(vec![
        ("type", Value::String(REMOTE_FRAME_COMMAND.to_string())),
        ("kind", Value::String(kind.to_string())),
        ("status", Value::String(status.to_string())),
        ("command_id", Value::String(command_id.to_string())),
    ]))
    .unwrap_or_else(|_| "{}".to_string())
}

fn json_object(entries: Vec<(&str, Value)>) -> Value {
    let mut map = Map::new();
    for (key, value) in entries {
        map.insert(key.to_string(), value);
    }
    Value::Object(map)
}

/// The frozen §7.1 kind directory; anything else is a structural refusal.
pub fn is_known_kind(kind: &str) -> bool {
    matches!(
        kind,
        wunder_core::interlink::CMD_NODE_SUMMARY
            | wunder_core::interlink::CMD_SHADOW_REFRESH
            | wunder_core::interlink::CMD_WORKSPACE_LIST
            | wunder_core::interlink::CMD_WORKSPACE_READ
            | wunder_core::interlink::CMD_WORKSPACE_SEARCH
            | wunder_core::interlink::CMD_WORKSPACE_STAT
            | wunder_core::interlink::CMD_THREADS_LIST
            | wunder_core::interlink::CMD_THREADS_GET
            | wunder_core::interlink::CMD_THREAD_CREATE
            | wunder_core::interlink::CMD_THREAD_MESSAGE
            | wunder_core::interlink::CMD_THREAD_CANCEL
            | wunder_core::interlink::CMD_THREAD_ANSWER
            | wunder_core::interlink::CMD_WORKSPACE_WRITE
            | wunder_core::interlink::CMD_WORKSPACE_MKDIR
            | wunder_core::interlink::CMD_WORKSPACE_MOVE
            | wunder_core::interlink::CMD_WORKSPACE_COPY
            | wunder_core::interlink::CMD_WORKSPACE_DELETE
            | wunder_core::interlink::CMD_TOOL_EXEC
            | wunder_core::interlink::CMD_AGENT_SPAWN
            | CMD_COMMAND_CANCEL
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wunder_core::interlink::{
        CMD_THREAD_MESSAGE, CMD_WORKSPACE_DELETE, CMD_WORKSPACE_READ, RISK_MEDIUM,
    };

    fn record(kind: &str, to_node: &str) -> InterlinkCommandRecord {
        InterlinkCommandRecord {
            command_id: "cmd_test".to_string(),
            direction: DIRECTION_C2L.to_string(),
            actor_user_id: "u_1".to_string(),
            from_node: "web:conn".to_string(),
            to_node: to_node.to_string(),
            kind: kind.to_string(),
            args_digest: Some(digest::digest_args(kind, &json!({"path": "a.md"}))),
            approval_state: APPROVAL_NONE.to_string(),
            status: COMMAND_STATUS_ISSUED.to_string(),
            created_at: 100.0,
            acked_at: None,
            finished_at: None,
            error_code: None,
            error_summary: None,
        }
    }

    #[test]
    fn concurrent_reservations_never_exceed_the_node_limit() {
        let hub = Arc::new(CommandHub::default());
        let workers: Vec<_> = (0..64)
            .map(|index| {
                let hub = hub.clone();
                std::thread::spawn(move || {
                    hub.try_reserve("fixture_device", &format!("fixture_{index}"), 8)
                })
            })
            .collect();
        let admitted = workers
            .into_iter()
            .map(|worker| usize::from(worker.join().unwrap()))
            .sum::<usize>();
        assert_eq!(admitted, 8);
        assert_eq!(hub.inflight_count("fixture_device"), 8);
    }

    #[tokio::test]
    async fn restart_recovery_is_paged_and_preserves_new_commands_and_terminal_history() {
        use wunder_core::interlink::APPROVAL_APPROVED;
        let root = tempfile::tempdir().unwrap();
        let path = root
            .path()
            .join("restart.db")
            .to_string_lossy()
            .into_owned();
        {
            let storage: Arc<dyn StorageBackend> =
                Arc::new(crate::storage::SqliteStorage::new(path.clone()));
            for index in 0..205 {
                let mut row = record(CMD_WORKSPACE_READ, "device:fixture_restart");
                row.command_id = format!("fixture_restart_{index:03}");
                row.status = if index % 2 == 0 {
                    COMMAND_STATUS_QUEUED
                } else {
                    COMMAND_STATUS_RUNNING
                }
                .into();
                storage.insert_interlink_command(&row).unwrap();
            }
            let mut pending = record(CMD_THREAD_MESSAGE, "cloud");
            pending.command_id = "fixture_restart_pending".into();
            pending.approval_state = APPROVAL_PENDING.into();
            pending.args_digest = Some(json!({"approval": "fixture_restart_ticket"}).to_string());
            storage.insert_interlink_command(&pending).unwrap();
            storage
                .insert_interlink_approval(&InterlinkApprovalRecord {
                    approval_id: "fixture_restart_ticket".into(),
                    command_id: pending.command_id,
                    device_id: "cloud".into(),
                    user_id: "u_1".into(),
                    prompt: "Fixture approval".into(),
                    risk_level: "medium".into(),
                    state: APPROVAL_PENDING.into(),
                    decided_by: None,
                    decided_at: None,
                    expires_at: 400.0,
                })
                .unwrap();
            let mut recent = record(CMD_WORKSPACE_READ, "cloud");
            recent.command_id = "fixture_restart_new".into();
            recent.created_at = 300.0;
            storage.insert_interlink_command(&recent).unwrap();
            recent.command_id = "fixture_restart_finished".into();
            recent.created_at = 10.0;
            recent.status = COMMAND_STATUS_SUCCEEDED.into();
            recent.finished_at = Some(20.0);
            recent.approval_state = APPROVAL_APPROVED.into();
            storage.insert_interlink_command(&recent).unwrap();
        }
        // Reopen only the durable database: none of the old bodies or queues
        // exist in this service generation, and recovery must not invent them.
        let storage: Arc<dyn StorageBackend> = Arc::new(crate::storage::SqliteStorage::new(path));
        assert_eq!(
            storage
                .list_interlink_unfinished_before(200.0, 1000)
                .unwrap()
                .len(),
            200
        );
        assert_eq!(
            recover_after_restart(storage.clone(), 200.0, 300.0)
                .await
                .unwrap(),
            200
        );
        assert_eq!(
            recover_after_restart(storage.clone(), 200.0, 300.0)
                .await
                .unwrap(),
            6
        );
        assert_eq!(
            recover_after_restart(storage.clone(), 200.0, 300.0)
                .await
                .unwrap(),
            0
        );
        for id in [
            "fixture_restart_000",
            "fixture_restart_204",
            "fixture_restart_pending",
        ] {
            let row = storage.get_interlink_command(id).unwrap().unwrap();
            assert_eq!(row.status, COMMAND_STATUS_FAILED);
            assert_eq!(row.error_code.as_deref(), Some(ERR_SERVER_RESTARTED));
            assert_eq!(row.finished_at, Some(300.0));
            assert!(hub().frame(id).is_none());
        }
        assert_eq!(
            storage
                .get_interlink_approval("fixture_restart_ticket")
                .unwrap()
                .unwrap()
                .state,
            APPROVAL_EXPIRED
        );
        assert_eq!(
            storage
                .get_interlink_command("fixture_restart_pending")
                .unwrap()
                .unwrap()
                .approval_state,
            APPROVAL_EXPIRED
        );
        assert_eq!(
            storage
                .get_interlink_command("fixture_restart_new")
                .unwrap()
                .unwrap()
                .status,
            COMMAND_STATUS_ISSUED
        );
        assert_eq!(
            storage
                .get_interlink_command("fixture_restart_finished")
                .unwrap()
                .unwrap()
                .status,
            COMMAND_STATUS_SUCCEEDED
        );
    }

    #[test]
    fn frames_carry_approval_and_timeout_without_the_body() {
        let mut record = record(CMD_THREAD_MESSAGE, "device:dev-1");
        record.approval_state = APPROVAL_PENDING.to_string();
        let limits = Limits {
            command_timeout_s: 45.0,
            ..Limits::default()
        };
        let ticket = InterlinkApprovalRecord {
            approval_id: "apr_1".to_string(),
            command_id: record.command_id.clone(),
            device_id: "dev-1".to_string(),
            user_id: "u_1".to_string(),
            prompt: "p".to_string(),
            risk_level: RISK_MEDIUM.to_string(),
            state: APPROVAL_PENDING.to_string(),
            decided_by: None,
            decided_at: None,
            expires_at: 145.0,
        };
        let text = command_frame_text(
            &record,
            Some(&ticket),
            &json!({"message": "secret text"}),
            &limits,
            RISK_MEDIUM,
        );
        let value: Value = serde_json::from_str(&text).expect("frame json");
        assert_eq!(value["type"], FRAME_COMMAND);
        assert_eq!(value["corr"], "cmd_test");
        assert_eq!(value["payload"]["kind"], CMD_THREAD_MESSAGE);
        assert_eq!(value["payload"]["requires_approval"], true);
        assert_eq!(value["payload"]["approval_id"], "apr_1");
        assert_eq!(value["payload"]["timeout_s"], 45.0);
        assert_eq!(value["payload"]["args"]["message"], "secret text");

        // The durable row never holds the body.
        assert!(!record
            .args_digest
            .as_deref()
            .unwrap()
            .contains("secret text"));
    }

    #[test]
    fn cancel_frame_is_a_control_command_bound_to_the_id() {
        let text = cancel_frame_text("cmd_9", "timeout", 10.0);
        let value: Value = serde_json::from_str(&text).expect("json");
        assert_eq!(value["payload"]["kind"], CMD_COMMAND_CANCEL);
        assert_eq!(value["payload"]["control"], true);
        assert_eq!(value["payload"]["target_command_id"], "cmd_9");
        assert_eq!(value["corr"], "cmd_9");
    }

    #[test]
    fn approval_id_and_timeout_round_trip_through_the_digest() {
        let mut stamped = record(CMD_WORKSPACE_READ, "device:dev-1");
        stamped.args_digest = Some(json!({"approval": "apr_7", "timeout_s": 33.0}).to_string());
        assert_eq!(approval_id_of(&stamped).as_deref(), Some("apr_7"));
        assert_eq!(record_timeout_s(&stamped, &Limits::default()), 33.0);
        // Absent fields fall back to the configured default.
        let plain = record(CMD_WORKSPACE_READ, "device:dev-1");
        assert_eq!(approval_id_of(&plain), None);
        assert_eq!(record_timeout_s(&plain, &Limits::default()), 120.0);
    }

    #[test]
    fn node_targets_are_normalized_and_cloud_has_none() {
        let device = record(CMD_WORKSPACE_READ, "device:dev-9");
        assert_eq!(target_device(&device).as_deref(), Some("dev-9"));
        let cloud = record(CMD_WORKSPACE_READ, "cloud");
        assert_eq!(target_device(&cloud), None);
        assert_eq!(device_of("device:"), None);
    }

    #[test]
    fn kind_directory_rejects_invented_kinds() {
        assert!(is_known_kind(CMD_THREAD_MESSAGE));
        assert!(is_known_kind(CMD_WORKSPACE_DELETE));
        assert!(!is_known_kind("shell.exec"));
    }

    #[test]
    fn hub_tracks_frames_results_and_queue_depth_with_hard_bounds() {
        let hub = CommandHub::default();
        hub.track_frame("cmd_1", "frame-1");
        hub.track_frame("cmd_1", "frame-ignored");
        assert_eq!(hub.frame("cmd_1").as_deref(), Some("frame-1"));
        hub.track_result("cmd_1", json!({"status": "succeeded"}));
        assert_eq!(hub.result("cmd_1").expect("result")["status"], "succeeded");

        for index in 0..(MAX_TRACKED_FRAMES + 10) {
            hub.track_frame(&format!("cmd_{index}"), "f");
        }
        assert_eq!(hub.lock().frames.len(), MAX_TRACKED_FRAMES);

        for index in 0..(MAX_TRACKED_RESULTS + 10) {
            hub.track_result(&format!("r_{index}"), json!({}));
        }
        assert_eq!(hub.lock().results.len(), MAX_TRACKED_RESULTS);
    }

    #[test]
    fn result_cache_enforces_bytes_expiry_and_oversized_replacement() {
        let hub = CommandHub::default();
        hub.track_result("oversized", json!({"progress":1}));
        hub.track_result("oversized", json!("x".repeat(MAX_RESULT_BYTES)));
        assert!(hub.result("oversized").is_none());
        let value = json!("x".repeat(MAX_RESULT_BYTES / 2));
        for index in 0..40 {
            hub.track_result(&format!("bytes_{index}"), value.clone());
        }
        assert!(hub.lock().result_bytes <= MAX_TOTAL_RESULT_BYTES);
        assert!(hub.result("bytes_0").is_none());
        hub.track_result("expired", json!({}));
        hub.lock().results.get_mut("expired").unwrap().inserted =
            Instant::now() - RESULT_TTL - Duration::from_secs(1);
        assert!(hub.result("expired").is_none());
    }

    #[tokio::test]
    async fn busy_retry_keeps_target_identity_and_cancel_releases_the_queue() {
        let root = tempfile::tempdir().unwrap();
        let storage: Arc<dyn StorageBackend> = Arc::new(crate::storage::SqliteStorage::new(
            root.path()
                .join("fixture.db")
                .to_string_lossy()
                .into_owned(),
        ));
        let mut row = record(CMD_WORKSPACE_READ, "device:fixture_busy");
        row.command_id = "fixture_busy_command".to_string();
        storage.insert_interlink_command(&row).unwrap();
        hub().track_frame(&row.command_id, "fixture_frame");
        hub().reserve("fixture_busy", &row.command_id);
        let outcome = on_ack(
            storage.clone(),
            &row.command_id,
            &json!({"accepted": false, "error_code": ERR_NODE_BUSY}),
            &Limits::default(),
            110.0,
        )
        .await
        .unwrap();
        assert_eq!(outcome, AckOutcome::Retried);
        assert_eq!(hub().queued_depth_for_target("device:fixture_busy"), 1);
        assert_eq!(hub().inflight_count("fixture_busy"), 0);
        assert_eq!(
            on_ack(
                storage.clone(),
                &row.command_id,
                &json!({"accepted": false, "error_code": ERR_NODE_BUSY}),
                &Limits::default(),
                111.0,
            )
            .await
            .unwrap(),
            AckOutcome::Retried
        );
        assert_eq!(hub().queued_depth_for_target("device:fixture_busy"), 1);
        assert_eq!(
            cancel(storage.clone(), &row.command_id, &row.actor_user_id, 120.0)
                .await
                .unwrap(),
            CancelOutcome::Canceled
        );
        assert_eq!(hub().queued_depth_for_target("device:fixture_busy"), 0);
        assert!(hub().frame(&row.command_id).is_none());
    }

    #[tokio::test]
    async fn drain_preserves_queue_on_storage_errors_and_outbound_backpressure() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("queue.db");
        let storage: Arc<dyn StorageBackend> = Arc::new(crate::storage::SqliteStorage::new(
            path.to_string_lossy().into_owned(),
        ));
        let device = "fixture_drain_failure";
        let target = format!("device:{device}");
        let mut row = record(CMD_WORKSPACE_READ, &target);
        row.command_id = "fixture_drain_command".into();
        row.status = COMMAND_STATUS_QUEUED.into();
        storage.insert_interlink_command(&row).unwrap();
        assert!(hub().enqueue(
            &row.actor_user_id,
            Queued {
                command_id: row.command_id.clone(),
                target,
                frame: "fixture_frame".into(),
            },
            1,
        ));
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        sender
            .try_send(OutboundFrame::Text("occupied".into()))
            .unwrap();
        registry().register(
            super::super::LiveChannel {
                device_id: device.into(),
                user_id: row.actor_user_id.clone(),
                client: "desktop".into(),
                channel_id: "fixture_drain_channel".into(),
                connection_id: "fixture_drain_connection".into(),
                protocol_version: 1,
                capabilities: vec![],
                connected_at: 100.0,
                last_seen_at: 100.0,
                rtt_ms: None,
                presence_status: None,
                active_threads: 0,
                resumed: false,
            },
            sender,
        );
        let connection = rusqlite::Connection::open(&path).unwrap();
        let limits = Limits::default();

        // A failed load cannot consume the queue head.
        connection
            .execute_batch("ALTER TABLE interlink_commands RENAME TO fixture_commands")
            .unwrap();
        assert!(drain(storage.clone(), &row.actor_user_id, device, &limits)
            .await
            .is_err());
        assert_eq!(hub().queued_depth_for_target(&row.to_node), 1);
        assert_eq!(hub().inflight_count(device), 0);
        connection
            .execute_batch("ALTER TABLE fixture_commands RENAME TO interlink_commands")
            .unwrap();

        // Nor can a failed queued -> issued update consume it or its slot.
        connection.execute_batch("CREATE TRIGGER fixture_issue_failure BEFORE UPDATE ON interlink_commands WHEN NEW.status = 'issued' BEGIN SELECT RAISE(FAIL, 'fixture storage failure'); END;").unwrap();
        assert!(drain(storage.clone(), &row.actor_user_id, device, &limits)
            .await
            .is_err());
        assert_eq!(hub().queued_depth_for_target(&row.to_node), 1);
        assert_eq!(hub().inflight_count(device), 0);
        connection
            .execute_batch("DROP TRIGGER fixture_issue_failure")
            .unwrap();

        // A full socket plus a failed rollback also leaves the body available.
        connection.execute_batch("CREATE TRIGGER fixture_queue_failure BEFORE UPDATE ON interlink_commands WHEN NEW.status = 'queued' BEGIN SELECT RAISE(FAIL, 'fixture storage failure'); END;").unwrap();
        assert!(drain(storage.clone(), &row.actor_user_id, device, &limits)
            .await
            .is_err());
        assert_eq!(hub().queued_depth_for_target(&row.to_node), 1);
        assert_eq!(hub().inflight_count(device), 0);
        connection
            .execute_batch("DROP TRIGGER fixture_queue_failure")
            .unwrap();
        assert_eq!(
            drain(storage.clone(), &row.actor_user_id, device, &limits)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            storage
                .get_interlink_command(&row.command_id)
                .unwrap()
                .unwrap()
                .status,
            COMMAND_STATUS_QUEUED
        );
        receiver.try_recv().unwrap();

        assert_eq!(
            drain(storage.clone(), &row.actor_user_id, device, &limits)
                .await
                .unwrap(),
            1
        );
        assert_eq!(hub().queued_depth_for_target(&row.to_node), 0);
        assert_eq!(hub().inflight_count(device), 1);
        assert!(
            matches!(receiver.try_recv().unwrap(), OutboundFrame::Text(text) if text == "fixture_frame")
        );
        assert!(receiver.try_recv().is_err());
        fail_device_commands(storage, device, 110.0).await.unwrap();
        registry().unregister(device, "fixture_drain_channel");
    }

    #[tokio::test]
    async fn late_progress_and_completion_preserve_first_terminal_result() {
        let root = tempfile::tempdir().unwrap();
        let storage: Arc<dyn StorageBackend> = Arc::new(crate::storage::SqliteStorage::new(
            root.path().join("ledger.db").to_string_lossy().into_owned(),
        ));
        let mut row = record(CMD_WORKSPACE_READ, "cloud");
        row.command_id = "fixture_terminal_result".into();
        storage.insert_interlink_command(&row).unwrap();
        finish_local(
            storage.clone(),
            &row.command_id,
            COMMAND_STATUS_SUCCEEDED,
            json!({"value":"first"}),
            None,
            120.0,
        )
        .await
        .unwrap();
        let progress = json!({"progress":true});
        let (progress_result, completion_result) = tokio::join!(
            on_event(storage.clone(), &row.command_id, &progress, 121.0),
            finish_local(
                storage.clone(),
                &row.command_id,
                COMMAND_STATUS_FAILED,
                json!({"value":"late"}),
                Some("LATE"),
                122.0
            )
        );
        progress_result.unwrap();
        completion_result.unwrap();
        assert_eq!(
            storage
                .get_interlink_command(&row.command_id)
                .unwrap()
                .unwrap()
                .status,
            COMMAND_STATUS_SUCCEEDED
        );
        assert_eq!(
            hub().result(&row.command_id).unwrap(),
            json!({"value":"first"})
        );
    }

    #[test]
    fn inflight_capacity_and_release_are_per_node() {
        let hub = CommandHub::default();
        assert!(hub.has_capacity("dev-1", 2));
        hub.reserve("dev-1", "cmd_1");
        hub.reserve("dev-1", "cmd_2");
        assert!(!hub.has_capacity("dev-1", 2));
        // Another node is unaffected.
        assert!(hub.has_capacity("dev-2", 2));
        hub.release("dev-1", "cmd_1");
        assert_eq!(hub.inflight_count("dev-1"), 1);
        assert!(hub.has_capacity("dev-1", 2));
        hub.release("dev-1", "cmd_2");
        assert_eq!(hub.inflight_count("dev-1"), 0);
    }

    #[test]
    fn queue_is_bounded_fifo_and_scoped_to_one_target() {
        let hub = CommandHub::default();
        let item = |id: &str, target: &str| Queued {
            command_id: id.to_string(),
            target: target.to_string(),
            frame: format!("frame-{id}"),
        };
        assert!(
            !hub.enqueue("u_1", item("cmd_1", "device:d1"), 0),
            "bound 0 parks nothing"
        );
        assert!(hub.enqueue("u_1", item("cmd_1", "device:d1"), 2));
        assert!(hub.enqueue("u_1", item("cmd_2", "device:d2"), 2));
        assert!(!hub.enqueue("u_1", item("cmd_3", "device:d1"), 2));
        assert_eq!(hub.queued_depth(Some("u_1")), 2);
        assert_eq!(hub.queued_depth(None), 2);
        assert_eq!(hub.queued_depth_for_target("device:d1"), 1);

        // Only the target's head is served; the other stays parked.
        let taken = hub.next_for("u_1", "device:d2").expect("item");
        assert_eq!(taken.command_id, "cmd_2");
        assert_eq!(hub.queued_depth(Some("u_1")), 2);
        assert!(!hub.enqueue("u_1", item("cmd_3", "device:d2"), 2));
        hub.remove_queued(&taken.command_id);
        assert!(hub.next_for("u_1", "device:unknown").is_none());
        assert_eq!(
            hub.next_for("u_1", "device:d1").expect("item").command_id,
            "cmd_1"
        );
        hub.remove_queued("cmd_1");
        assert_eq!(hub.queued_depth(Some("u_1")), 0);

        hub.enqueue("u_1", item("cmd_a", "device:d1"), 4);
        hub.enqueue("u_1", item("cmd_b", "device:d1"), 4);
        assert_eq!(
            hub.next_for("u_1", "device:d1").expect("head").command_id,
            "cmd_a"
        );
        hub.remove_queued("cmd_a");
        assert_eq!(
            hub.next_for("u_1", "device:d1").unwrap().command_id,
            "cmd_b"
        );
    }

    #[test]
    fn closing_a_tunnel_releases_slots_and_preserves_device_backlogs() {
        let hub = CommandHub::default();
        hub.reserve("dev-1", "cmd_1");
        hub.enqueue(
            "u_1",
            Queued {
                command_id: "cmd_2".to_string(),
                target: "device:dev-1".to_string(),
                frame: "f".to_string(),
            },
            4,
        );
        hub.enqueue(
            "u_1",
            Queued {
                command_id: "cmd_3".to_string(),
                target: "device:dev-2".to_string(),
                frame: "f".to_string(),
            },
            4,
        );
        hub.forget_node("dev-1");
        assert_eq!(hub.inflight_count("dev-1"), 0);
        assert_eq!(hub.queued_depth_for_target("device:dev-1"), 1);
        assert_eq!(hub.queued_depth_for_target("device:dev-2"), 1);
        assert_eq!(
            hub.next_for("u_1", "device:dev-1").unwrap().command_id,
            "cmd_2"
        );
    }

    #[test]
    fn dropping_a_terminal_frame_releases_its_offline_queue_slot() {
        let hub = CommandHub::default();
        hub.track_frame("fixture_command", "fixture_frame");
        assert!(hub.enqueue(
            "fixture_user",
            Queued {
                command_id: "fixture_command".to_string(),
                target: "device:fixture_device".to_string(),
                frame: "fixture_frame".to_string(),
            },
            1
        ));
        hub.forget_node("fixture_device");
        assert_eq!(hub.queued_depth(None), 1);
        hub.drop_frame("fixture_command");
        assert_eq!(hub.queued_depth(None), 0);
        assert!(hub.frame("fixture_command").is_none());
    }

    #[test]
    fn stats_report_the_bounded_totals() {
        let hub = CommandHub::default();
        hub.reserve("dev-1", "cmd_1");
        hub.track_frame("cmd_1", "f");
        hub.track_result("cmd_1", json!({}));
        hub.enqueue(
            "u_1",
            Queued {
                command_id: "cmd_2".to_string(),
                target: "device:dev-1".to_string(),
                frame: "f".to_string(),
            },
            4,
        );
        let stats = hub.stats();
        assert_eq!(stats["inflight_total"], 1);
        assert_eq!(stats["queued_total"], 1);
        assert_eq!(stats["tracked_frames"], 1);
        assert_eq!(stats["tracked_results"], 1);
    }

    #[test]
    fn limits_come_from_the_configuration_defaults() {
        let config = wunder_core::config::InterlinkConfig::default();
        let limits = Limits::from_config(&config);
        assert_eq!(limits.command_timeout_s, 120.0);
        assert_eq!(limits.inflight_per_node, 8);
        assert_eq!(limits.offline_queue_per_user, 32);
        assert!(limits
            .require_approval_defaults
            .iter()
            .any(|kind| kind == CMD_THREAD_MESSAGE));
    }

    /// A `command_result` frame must settle the ledger, not just feed the
    /// in-memory result the polling endpoint merges into its response. When the
    /// row stays `acked` the caller sees a finished result on a command that
    /// never leaves the open states - exactly the shape the loopback test hit.
    #[test]
    fn a_result_frame_settles_the_ledger_row() {
        use crate::storage::SqliteStorage;

        let root = tempfile::tempdir().unwrap();
        let storage: Arc<dyn StorageBackend> = Arc::new(SqliteStorage::new(
            root.path().join("state.db").to_string_lossy().into_owned(),
        ));
        let target = "device:dev-result";
        let mut row = record(CMD_WORKSPACE_READ, target);
        row.command_id = "cmd_result".to_string();
        row.status = COMMAND_STATUS_ACKED.to_string();
        row.acked_at = Some(100.0);
        storage.insert_interlink_command(&row).unwrap();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let settled = runtime
            .block_on(on_result(
                Arc::clone(&storage),
                "cmd_result",
                &json!({"status": "succeeded", "result": {"ok": true}}),
                &Limits::default(),
                120.0,
            ))
            .expect("result handled");

        assert!(settled, "a first terminal frame is accepted");
        let stored = storage
            .get_interlink_command("cmd_result")
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, COMMAND_STATUS_SUCCEEDED);
        assert_eq!(stored.finished_at, Some(120.0));
    }

    /// Docs §13.2 7: the commands a vanished node was actually handed answer
    /// `NODE_OFFLINE` at once, while commands still parked for delivery survive.
    #[test]
    fn a_vanished_node_fails_only_the_commands_it_was_handed() {
        use crate::storage::{ListInterlinkAuditQuery, SqliteStorage};
        use wunder_core::interlink::APPROVAL_APPROVED;

        let root = tempfile::tempdir().unwrap();
        let storage: Arc<dyn StorageBackend> = Arc::new(SqliteStorage::new(
            root.path().join("state.db").to_string_lossy().into_owned(),
        ));
        let device = "dev-settle-7";
        let target = format!("device:{device}");
        let row = |id: &str, status: &str, approval: &str| InterlinkCommandRecord {
            command_id: id.to_string(),
            to_node: target.clone(),
            status: status.to_string(),
            approval_state: approval.to_string(),
            ..record(CMD_WORKSPACE_READ, &target)
        };

        let delivered = [
            ("cmd_issued", COMMAND_STATUS_ISSUED),
            ("cmd_acked", COMMAND_STATUS_ACKED),
            ("cmd_running", COMMAND_STATUS_RUNNING),
        ];
        for (id, status) in delivered {
            storage
                .insert_interlink_command(&row(id, status, APPROVAL_NONE))
                .unwrap();
            hub().reserve(device, id);
        }
        // Completed history must not hide an older reservation from cleanup.
        for index in 0..205 {
            let mut completed = row(
                &format!("fixture_history_{index}"),
                COMMAND_STATUS_SUCCEEDED,
                APPROVAL_NONE,
            );
            completed.created_at = 150.0;
            completed.finished_at = Some(151.0);
            storage.insert_interlink_command(&completed).unwrap();
        }
        storage
            .insert_interlink_command(&row("cmd_queued", COMMAND_STATUS_QUEUED, APPROVAL_NONE))
            .unwrap();
        assert!(hub().enqueue(
            "u1",
            Queued {
                command_id: "cmd_queued".to_string(),
                target: target.clone(),
                frame: "fixture".to_string(),
            },
            4
        ));
        storage
            .insert_interlink_command(&row("cmd_pending", COMMAND_STATUS_ISSUED, APPROVAL_PENDING))
            .unwrap();
        hub().reserve(device, "cmd_pending");
        storage
            .insert_interlink_command(&row(
                "cmd_done",
                COMMAND_STATUS_SUCCEEDED,
                APPROVAL_APPROVED,
            ))
            .unwrap();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let failed = runtime
            .block_on(fail_device_commands(Arc::clone(&storage), device, 200.0))
            .expect("settle");
        assert_eq!(
            failed, 4,
            "all handed-off commands, including a pending prompt, settle"
        );
        assert_eq!(
            storage
                .get_interlink_command("cmd_pending")
                .unwrap()
                .unwrap()
                .approval_state,
            APPROVAL_EXPIRED
        );

        for (id, _) in delivered {
            let settled = storage.get_interlink_command(id).unwrap().unwrap();
            assert_eq!(settled.status, COMMAND_STATUS_FAILED, "{id}");
            assert_eq!(
                settled.error_code.as_deref(),
                Some(ERR_NODE_OFFLINE),
                "{id}"
            );
            assert_eq!(settled.finished_at, Some(200.0), "{id}");
        }
        for (id, status, approval) in [
            ("cmd_queued", COMMAND_STATUS_QUEUED, APPROVAL_NONE),
            ("cmd_done", COMMAND_STATUS_SUCCEEDED, APPROVAL_APPROVED),
        ] {
            let kept = storage.get_interlink_command(id).unwrap().unwrap();
            assert_eq!(kept.status, status, "{id} keeps its state");
            assert_eq!(kept.approval_state, approval, "{id}");
        }

        // The node's in-flight slots are released with it, so its next tunnel does
        // not inherit the dead session's capacity.
        assert_eq!(hub().inflight_count(device), 0);
        assert_eq!(hub().queued_depth_for_target(&target), 1);
        assert_eq!(
            hub().next_for("u1", &target).unwrap().command_id,
            "cmd_queued"
        );
        hub().drop_frame("cmd_queued");

        // Each settle is an auditable finish row, not a silent update.
        let (trail, _) = storage
            .list_interlink_audit(ListInterlinkAuditQuery {
                user_id: None,
                device_id: None,
                action: Some(AUDIT_COMMAND_FINISH),
                since: None,
                until: None,
                offset: 0,
                limit: 100,
            })
            .unwrap();
        let finished: Vec<&str> = trail
            .iter()
            .filter_map(|item| item.command_id.as_deref())
            .collect();
        for id in ["cmd_issued", "cmd_acked", "cmd_running"] {
            assert!(finished.contains(&id), "{id} has no command.finish row");
        }
        assert!(
            !finished.contains(&"cmd_queued"),
            "the parked backlog was settled"
        );
    }
}
