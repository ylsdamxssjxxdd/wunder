//! Inbound `command` frame executor for the local node (docs §7.1, §7.2,
//! §7.3, §9.2, §4.3).
//!
//! Precedence, in order, and it is never reordered:
//! 1. **capability re-check** against `hello_ack.capabilities_granted` - the
//!    local side refuses anything outside the intersection it agreed to.
//! 2. **approval** ([`crate::services::interlink::client::approval`]) - L0 runs,
//!    L1 may be covered by a 30-minute scope memory, L2/L3 always need a live
//!    decision, and no decision inside the window is a rejection.
//! 3. **idempotency** - a command id already seen inside the 24h window is
//!    answered from the cached terminal result and never re-executed.
//! 4. execution through the *existing* engine and workspace entry points; the
//!    tunnel invents no new execution semantics (docs §2.5).
//!
//! A rejected command has no side effects at all: nothing is read, written or
//! submitted before the gates above have passed.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use futures::StreamExt;
use serde_json::{json, Value};
use tokio::sync::{Mutex, Notify};
use tokio_util::sync::CancellationToken;

use wunder_core::interlink::{
    command_level, command_policy, APPROVAL_EXPIRED, APPROVAL_REJECTED, CMD_AGENT_SPAWN,
    CMD_COMMAND_CANCEL, CMD_NODE_SUMMARY, CMD_SHADOW_REFRESH, CMD_THREAD_ANSWER,
    CMD_THREAD_CANCEL, CMD_THREAD_CREATE, CMD_THREAD_MESSAGE, CMD_THREADS_GET, CMD_THREADS_LIST,
    CMD_TOOL_EXEC, CMD_WORKSPACE_COPY, CMD_WORKSPACE_DELETE, CMD_WORKSPACE_LIST,
    CMD_WORKSPACE_MKDIR, CMD_WORKSPACE_MOVE, CMD_WORKSPACE_READ, CMD_WORKSPACE_SEARCH,
    CMD_WORKSPACE_STAT, CMD_WORKSPACE_WRITE, ERR_APPROVAL_REJECTED, ERR_CAP_DENIED,
    ERR_UNKNOWN_KIND,
};

use crate::approval_registry::ApprovalSource;
use crate::core::approval::ApprovalResponse;
use crate::core::blocking;
use crate::core::long_task;
use crate::services::interlink::client::approval::{ApprovalGate, ApprovalPlan};
use crate::services::interlink::client::shadow::{self, Sections};
use crate::services::interlink::client::stream;
use crate::services::interlink::client::{now_ts, TunnelWriter};
use crate::services::interlink::secret::sha256_hex;
use crate::services::tools::{
    execute_builtin_tool, sessions_spawn, DetachedToolContext,
};
use crate::state::AppState;

/// Longest window a command id is remembered for (docs §4.3).
pub const SEEN_TTL_S: f64 = 24.0 * 3_600.0;
/// Hard bound of the idempotency set.
pub const SEEN_MAX: usize = 512;
/// Backup generations kept per workspace (docs §6.4).
pub const BACKUP_KEEP: usize = 50;
/// Largest file the "before" digest of an L2 mutation is computed for: hashing a
/// whole huge target would turn one command into an unbounded read (docs §6.4).
pub const BACKUP_DIGEST_MAX_BYTES: u64 = 16 * 1024 * 1024;
/// Rows returned by one read-only listing command.
const LIST_LIMIT_DEFAULT: u64 = 200;
const LIST_LIMIT_MAX: u64 = 500;
/// Messages returned by `threads.get`; the transcript stays paged.
const HISTORY_LIMIT_DEFAULT: i64 = 50;
const HISTORY_LIMIT_MAX: i64 = 200;
/// In-flight commands of one node; mirrors the server's `inflight_per_node`
/// default so neither side needs a second limiter.
pub const INFLIGHT_MAX: usize = 8;

/// Everything needed to decide and run one command, parsed from the frame the
/// server sends (`services/interlink/commands.rs::command_frame_text`).
#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub command_id: String,
    pub kind: String,
    pub args: Value,
    pub from_node: String,
    pub actor: String,
    pub approval_id: Option<String>,
    pub approval_expires_at: Option<f64>,
    pub timeout_s: Option<f64>,
    /// `control == true` marks a `command.cancel` frame, not an operation.
    pub control: bool,
    pub target_command_id: Option<String>,
}

impl CommandSpec {
    /// Parse one inbound `command` payload. A frame without a correlation id or
    /// a kind is refused without side effects.
    pub fn parse(corr: Option<&str>, payload: &Value) -> Option<CommandSpec> {
        let kind = payload.get("kind").and_then(Value::as_str)?.trim().to_string();
        if kind.is_empty() {
            return None;
        }
        let command_id = corr
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)?;
        Some(Self {
            command_id,
            kind,
            args: payload.get("args").cloned().unwrap_or(Value::Null),
            from_node: payload
                .get("from")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("cloud")
                .to_string(),
            actor: payload
                .get("actor")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            approval_id: payload
                .get("approval_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            approval_expires_at: payload.get("approval_expires_at").and_then(Value::as_f64),
            timeout_s: payload.get("timeout_s").and_then(Value::as_f64),
            control: payload
                .get("control")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            target_command_id: payload
                .get("target_command_id")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }

    pub fn level(&self) -> &'static str {
        command_level(&self.kind)
    }

    pub fn risk(&self) -> &'static str {
        command_policy(self.kind.as_str()).0
    }
}

/// Outcome of the capability + approval gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preflight {
    /// Run it now.
    Allow,
    /// A human decision is required before anything runs.
    NeedPrompt,
    /// Refuse with `(error code, approval state)` and run nothing.
    Reject(&'static str, &'static str),
}

impl Preflight {
    /// Stable rejection code, used for `command_result.error.code`.
    pub fn rejection_code(&self) -> Option<&'static str> {
        match self {
            Self::Reject(code, _) => Some(code),
            _ => None,
        }
    }

    /// `approval_state` reported in the rejection ack.
    pub fn approval_state(&self) -> &'static str {
        match self {
            Self::Reject(_, state) => state,
            _ => "none",
        }
    }
}

/// Whether `kind` is inside the granted capability set (docs §9.2: either side
/// going out of bounds is a refusal, not a warning).
pub fn capability_allowed(kind: &str, capabilities: &[String]) -> bool {
    let required = command_policy(kind).1;
    capabilities.iter().any(|cap| cap == required)
}

/// Full decision table for one command, minus the live prompt itself.
///
/// `capabilities` is the set the server handed back in `hello_ack` (declared ∩
/// admin policy ∩ server permission); `declared` is this node's own capability
/// recomputed now. A kind outside either set is refused before the gate, so a
/// wrong grant cannot make the node execute L3 work it does not honour.
pub fn preflight(
    spec: &CommandSpec,
    capabilities: &[String],
    declared: &[String],
    gate: &ApprovalGate,
    policy: &str,
    now: f64,
) -> Preflight {
    if spec.control || spec.kind == CMD_COMMAND_CANCEL {
        // Control frames carry no operation and need no approval.
        return Preflight::Allow;
    }
    if !is_supported_kind(&spec.kind) {
        return Preflight::Reject(ERR_UNKNOWN_KIND, "none");
    }
    if !capability_allowed(&spec.kind, capabilities)
        || !capability_allowed(&spec.kind, declared)
    {
        return Preflight::Reject(ERR_CAP_DENIED, "none");
    }
    match gate.decide_plan(&spec.kind, &spec.actor, policy, now) {
        ApprovalPlan::AutoAllow => Preflight::Allow,
        ApprovalPlan::RequirePrompt => Preflight::NeedPrompt,
        ApprovalPlan::AutoDeny(_) => Preflight::Reject(ERR_APPROVAL_REJECTED, APPROVAL_REJECTED),
    }
}

/// Kinds this node has an implementation for. L3 (`tool.exec`, `agent.spawn`)
/// runs through the engine's own tool path, so the only remaining gate is the
/// capability intersection and the local approval (docs §7.1, §9.2).
pub fn is_supported_kind(kind: &str) -> bool {
    matches!(
        kind,
        CMD_NODE_SUMMARY
            | CMD_SHADOW_REFRESH
            | CMD_WORKSPACE_LIST
            | CMD_WORKSPACE_SEARCH
            | CMD_WORKSPACE_STAT
            | CMD_WORKSPACE_READ
            | CMD_THREADS_LIST
            | CMD_THREADS_GET
            | CMD_THREAD_CREATE
            | CMD_THREAD_MESSAGE
            | CMD_THREAD_CANCEL
            | CMD_THREAD_ANSWER
            | CMD_WORKSPACE_WRITE
            | CMD_WORKSPACE_MKDIR
            | CMD_WORKSPACE_MOVE
            | CMD_WORKSPACE_COPY
            | CMD_WORKSPACE_DELETE
            | CMD_TOOL_EXEC
            | CMD_AGENT_SPAWN
    )
}

/// Bounded 24h idempotency ledger: replays are answered from the cached
/// terminal result and never re-executed (docs §4.3, §9.5).
#[derive(Debug, Default)]
pub struct CommandLedger {
    state: Mutex<LedgerState>,
}

#[derive(Debug, Default)]
struct LedgerState {
    /// `command_id -> (seen_at, cached terminal result payload)`.
    seen: HashMap<String, (f64, Value)>,
    order: VecDeque<String>,
    running: HashMap<String, Running>,
}

#[derive(Debug)]
struct Running {
    kind: String,
    cancel: CancellationToken,
}

/// What the caller should do with a freshly arrived command id.
#[derive(Debug, Clone, PartialEq)]
pub enum Begin {
    /// Known id, terminal result cached: answer it and execute nothing.
    Replay(Value),
    /// Known id whose command produced no result payload (control frames).
    ReplayEmpty,
    /// Same id still executing on this node.
    InFlight,
    /// Unknown id: claim it and run.
    Fresh,
}

impl CommandLedger {
    /// Classify one command id without mutating the ledger.
    pub async fn classify(&self, command_id: &str, now: f64) -> Begin {
        let mut state = self.state.lock().await;
        prune(&mut state, now);
        if state.running.contains_key(command_id) {
            return Begin::InFlight;
        }
        match state.seen.get(command_id) {
            Some((_, result)) if result.is_null() => Begin::ReplayEmpty,
            Some((_, result)) => Begin::Replay(result.clone()),
            None => Begin::Fresh,
        }
    }

    /// Claim a fresh id so a duplicate that arrives while the first is still
    /// running cannot start twice.
    pub async fn claim(&self, command_id: &str, kind: &str, now: f64) -> bool {
        let mut state = self.state.lock().await;
        prune(&mut state, now);
        if state.running.contains_key(command_id) || state.seen.contains_key(command_id) {
            return false;
        }
        state.running.insert(
            command_id.to_string(),
            Running {
                kind: kind.to_string(),
                cancel: CancellationToken::new(),
            },
        );
        true
    }

    /// Cancellation token of a running command, for `command.cancel`.
    pub async fn cancel_token_for(&self, command_id: &str) -> Option<CancellationToken> {
        self.state
            .lock()
            .await
            .running
            .get(command_id)
            .map(|running| running.cancel.clone())
    }

    /// Publish the terminal result and remember it for the rest of the window.
    pub async fn finish(&self, command_id: &str, result: Value, now: f64) {
        let mut state = self.state.lock().await;
        state.running.remove(command_id);
        if state.seen.contains_key(command_id) {
            return;
        }
        while state.seen.len() >= SEEN_MAX {
            match state.order.pop_front() {
                Some(stale) => {
                    state.seen.remove(&stale);
                }
                None => break,
            }
        }
        state.order.push_back(command_id.to_string());
        state.seen.insert(command_id.to_string(), (now, result));
    }

    /// Abort the running command with this id (docs §7.1 `command.cancel`).
    pub async fn cancel(&self, command_id: &str) -> bool {
        let mut state = self.state.lock().await;
        match state.running.get_mut(command_id) {
            Some(entry) => {
                entry.cancel.cancel();
                true
            }
            None => false,
        }
    }

    pub async fn running_ids(&self) -> Vec<String> {
        let state = self.state.lock().await;
        state.running.keys().cloned().collect()
    }

    /// Abort everything: called when the tunnel drops, because a command whose
    /// answer can no longer be delivered should not keep the node busy.
    pub async fn abort_all(&self) -> usize {
        let mut state = self.state.lock().await;
        let count = state.running.len();
        for entry in state.running.values() {
            entry.cancel.cancel();
        }
        state.running.clear();
        count
    }
}

/// Prune ids older than the 24h window so the map stays bounded.
fn prune(state: &mut LedgerState, now: f64) {
    let stale: Vec<String> = state
        .seen
        .iter()
        .filter(|(_, (seen_at, _))| now - *seen_at > SEEN_TTL_S)
        .map(|(id, _)| id.clone())
        .collect();
    for id in stale {
        state.seen.remove(&id);
        state.order.retain(|existing| existing != &id);
    }
}

/// Terminal answer of one command.
#[derive(Debug, Clone)]
pub struct CommandReport {
    pub status: &'static str,
    pub result: Value,
    pub usage: Value,
    pub error: Option<(String, String)>,
    /// Extra top-level payload keys the server reads directly (`inline`,
    /// `mime`, `max_bytes`, `stream_id`, `size`) - docs §6.4.
    pub extra: serde_json::Map<String, Value>,
}

impl CommandReport {
    pub fn succeeded(result: Value) -> Self {
        Self {
            status: wunder_core::interlink::COMMAND_STATUS_SUCCEEDED,
            result,
            usage: json!({}),
            error: None,
            extra: serde_json::Map::new(),
        }
    }

    pub fn failed(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: wunder_core::interlink::COMMAND_STATUS_FAILED,
            result: Value::Null,
            usage: json!({}),
            error: Some((code.into(), message.into())),
            extra: serde_json::Map::new(),
        }
    }

    pub fn canceled(message: impl Into<String>) -> Self {
        Self {
            status: wunder_core::interlink::COMMAND_STATUS_CANCELED,
            result: Value::Null,
            usage: json!({}),
            error: Some((
                wunder_core::interlink::COMMAND_STATUS_CANCELED.to_string(),
                message.into(),
            )),
            extra: serde_json::Map::new(),
        }
    }

    /// The work started but ran out of its window (docs §4.3 `timeout`).
    pub fn timed_out(message: impl Into<String>) -> Self {
        Self {
            status: wunder_core::interlink::COMMAND_STATUS_TIMEOUT,
            result: Value::Null,
            usage: json!({}),
            error: Some((
                wunder_core::interlink::ERR_TIMEOUT.to_string(),
                message.into(),
            )),
            extra: serde_json::Map::new(),
        }
    }

    /// Add one top-level payload key; used by the data plane.
    pub fn with_extra(mut self, key: &str, value: Value) -> Self {
        self.extra.insert(key.to_string(), value);
        self
    }

    /// `command_result` payload (docs §4.2 phase 3).
    pub fn payload(&self) -> Value {
        let mut payload = json!({
            "status": self.status,
            "result": self.result,
            "usage": self.usage,
            "error": self.error.as_ref().map(|(code, message)| json!({
                "code": code,
                "message": message.chars().take(200).collect::<String>(),
            })),
        });
        if let Some(map) = payload.as_object_mut() {
            for (key, value) in &self.extra {
                map.insert(key.clone(), value.clone());
            }
        }
        payload
    }

    /// `command_ack` payload (docs §4.2 phase 2).
    pub fn ack(accepted: bool, approval_state: &str) -> Value {
        json!({
            "accepted": accepted,
            "approval_state": approval_state,
            "eta_s": Value::Null,
        })
    }

    /// Refusal ack with the structural code (docs §9.2: an out-of-set command is
    /// refused outright). Without the code the server would read a refusal as
    /// "node busy" and keep retrying the same command.
    pub fn ack_denied(code: &str, approval_state: &str) -> Value {
        json!({
            "accepted": false,
            "approval_state": approval_state,
            "eta_s": Value::Null,
            "error_code": code,
        })
    }
}

/// The local node context one command may use.
#[derive(Clone)]
pub struct ExecContext {
    pub state: Arc<AppState>,
    /// Local engine user identity (not the cloud account id).
    pub local_user_id: String,
    /// Workspace binding of the desktop node, when it has one.
    pub workspace_id: Option<String>,
    /// Pull/write ceiling from the session file (docs §3.3).
    pub max_file_pull_bytes: u64,
    pub chunk_bytes: usize,
    pub rate_bps: u64,
    /// Origin tag recorded with anything this command creates (docs §7.2).
    pub source_tag: String,
}

impl ExecContext {
    /// Scope key for workspace access. An explicit `workspace_id` argument
    /// selects another bound workspace of the same local user; the fence, path
    /// resolution and size limits stay inside `WorkspaceManager`.
    pub fn scope_user(&self, args: &Value) -> String {
        let requested = args
            .get("workspace_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| self.workspace_id.clone());
        match requested {
            Some(workspace_id) => self
                .state
                .workspace
                .scoped_user_id_for_workspace(&self.local_user_id, &workspace_id),
            None if self.local_user_id.trim().is_empty() => String::new(),
            None => self.state.workspace.scoped_user_id(&self.local_user_id, None),
        }
    }
}

/// Run one approved command. The function never touches the socket directly:
/// data-plane writes go through `writer`, everything else is a report.
pub async fn execute(
    spec: &CommandSpec,
    ctx: &ExecContext,
    writer: &Arc<TunnelWriter>,
    collector: &shadow::ShadowCollector,
    shadow_wake: &Arc<Notify>,
    cancel: CancellationToken,
) -> CommandReport {
    match spec.kind.as_str() {
        CMD_NODE_SUMMARY => node_summary(ctx).await,
        CMD_SHADOW_REFRESH => {
            collector.mark_dirty(Sections::ALL);
            shadow_wake.notify_waiters();
            CommandReport::succeeded(json!({ "revision": collector.revision() }))
        }
        CMD_WORKSPACE_LIST => workspace_list(spec, ctx).await,
        CMD_WORKSPACE_SEARCH => workspace_search(spec, ctx).await,
        CMD_WORKSPACE_STAT => workspace_stat(spec, ctx).await,
        CMD_WORKSPACE_READ => stream::read_command(spec, ctx, writer, &cancel).await,
        CMD_THREADS_LIST => threads_list(spec, ctx).await,
        CMD_THREADS_GET => threads_get(spec, ctx).await,
        CMD_THREAD_CREATE | CMD_THREAD_MESSAGE => drive_thread(spec, ctx).await,
        CMD_THREAD_CANCEL => thread_cancel(spec, ctx).await,
        CMD_THREAD_ANSWER => thread_answer(spec, ctx).await,
        CMD_WORKSPACE_WRITE => workspace_write(spec, ctx, collector).await,
        CMD_WORKSPACE_MKDIR => workspace_mkdir(spec, ctx, collector).await,
        CMD_WORKSPACE_MOVE | CMD_WORKSPACE_COPY | CMD_WORKSPACE_DELETE => {
            workspace_mutate(spec, ctx, collector).await
        }
        CMD_TOOL_EXEC => tool_exec(spec, ctx, collector, &cancel).await,
        CMD_AGENT_SPAWN => agent_spawn(spec, ctx, collector).await,
        other => CommandReport::failed(ERR_UNKNOWN_KIND, format!("unsupported kind {other}")),
    }
}

async fn node_summary(ctx: &ExecContext) -> CommandReport {
    let active = ctx.state.monitor.list_sessions(true).len();
    let identity = shadow::NodeIdentity::from_session(&ctx.source_client(), &ctx.local_user_id);
    CommandReport::succeeded(shadow::summary_json(
        &identity,
        std::env::consts::OS,
        std::env::consts::ARCH,
        active,
        0,
        None,
    ))
}

impl ExecContext {
    /// Client flavour reported in the summary: the session file owns this, the
    /// executor only formats it, so keep it derived from the source tag.
    fn source_client(&self) -> &str {
        // `remote:<client>:<conn>` -> `<client>`
        self.source_tag
            .strip_prefix("remote:")
            .and_then(|rest| rest.split(':').next())
            .unwrap_or("local")
    }
}

fn bounded_limit(args: &Value, default: u64) -> u64 {
    args.get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(default)
        .clamp(1, LIST_LIMIT_MAX)
}

async fn workspace_list(spec: &CommandSpec, ctx: &ExecContext) -> CommandReport {
    let scope = ctx.scope_user(&spec.args);
    let path = spec.args.get("path").and_then(Value::as_str).unwrap_or(".");
    let keyword = spec
        .args
        .get("keyword")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let offset = spec.args.get("offset").and_then(Value::as_u64).unwrap_or(0);
    let limit = bounded_limit(&spec.args, LIST_LIMIT_DEFAULT);
    let workspace = ctx.state.workspace.clone();
    let owner = scope;
    let path = path.to_string();
    match workspace
        .list_workspace_entries_async(
            &owner,
            &path,
            keyword.as_deref(),
            offset,
            limit,
            "name",
            "asc",
        )
        .await
    {
        Ok((entries, tree_version, current_path, parent, total)) => {
            let rows = entries.len();
            let report = CommandReport::succeeded(json!({
                "path": current_path,
                "parent": parent,
                "total": total,
                "offset": offset,
                "limit": limit,
                "tree_version": tree_version,
                "entries": entries,
            }));
            CommandReport {
                usage: json!({"rows": rows}),
                ..report
            }
        }
        Err(err) => CommandReport::failed("WORKSPACE_ERROR", sanitize(&err.to_string())),
    }
}

async fn workspace_search(spec: &CommandSpec, ctx: &ExecContext) -> CommandReport {
    let scope = ctx.scope_user(&spec.args);
    let keyword = arg_str(&spec.args, "keyword").unwrap_or_default();
    if keyword.trim().is_empty() {
        return CommandReport::failed("KEYWORD_REQUIRED", "keyword is required");
    }
    let offset = spec.args.get("offset").and_then(Value::as_u64).unwrap_or(0);
    let limit = bounded_limit(&spec.args, LIST_LIMIT_DEFAULT);
    let workspace = ctx.state.workspace.clone();
    match workspace
        .search_workspace_entries_async(&scope, &keyword, offset, limit, true, true)
        .await
    {
        Ok((entries, total)) => {
            let rows = entries.len();
            let report = CommandReport::succeeded(json!({
                "total": total,
                "offset": offset,
                "limit": limit,
                "entries": entries,
            }));
            CommandReport {
                usage: json!({"rows": rows}),
                ..report
            }
        }
        Err(err) => CommandReport::failed("WORKSPACE_ERROR", sanitize(&err.to_string())),
    }
}

async fn workspace_stat(spec: &CommandSpec, ctx: &ExecContext) -> CommandReport {
    let scope = ctx.scope_user(&spec.args);
    let path = spec
        .args
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or(".")
        .to_string();
    let workspace = ctx.state.workspace.clone();
    match workspace.workspace_usage_summary_async(&scope, &path, 8).await {
        Ok(summary) => CommandReport::succeeded(json!({
            "path": path,
            "files": summary.files,
            "dirs": summary.dirs,
            "used_bytes": summary.used_bytes,
            "truncated": summary.truncated,
        })),
        Err(err) => CommandReport::failed("WORKSPACE_ERROR", sanitize(&err.to_string())),
    }
}

async fn threads_list(spec: &CommandSpec, ctx: &ExecContext) -> CommandReport {
    let limit = bounded_limit(&spec.args, LIST_LIMIT_DEFAULT).min(shadow::THREADS_MAX as u64);
    let sources = ctx.shadow_sources();
    let limits = shadow::TreeLimits {
        max_entries: 1,
        depth: 1,
        threads_max: limit.max(1) as usize,
        tasks_max: 1,
    };
    let (rows, truncated) = shadow::gather_threads(&ctx.state, &sources, &limits).await;
    CommandReport::succeeded(json!({
        "threads": rows,
        "offset": spec.args.get("offset").and_then(Value::as_u64).unwrap_or(0),
        "truncated": truncated,
    }))
}

impl ExecContext {
    /// Shadow source set for read-only commands that reuse the collector.
    pub fn shadow_sources(&self) -> shadow::ShadowSources {
        shadow::ShadowSources {
            identity: shadow::NodeIdentity::from_session(&self.source_client(), &self.local_user_id),
            local_user_id: self.local_user_id.clone(),
            workspace_id: self.workspace_id.clone(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            limits: shadow::TreeLimits::default(),
            minimal: false,
        }
    }

    /// Workspace scope of one thread that already exists: the same fence that
    /// thread's own tools see, so a remotely spawned unit inherits its parent's
    /// binding instead of falling back to the node default.
    fn scope_of_thread(&self, thread_id: &str) -> String {
        let bound = self
            .state
            .storage
            .get_chat_session(&self.local_user_id, thread_id)
            .ok()
            .flatten()
            .and_then(|record| record.workspace_id);
        match bound.or_else(|| self.workspace_id.clone()) {
            Some(workspace_id) => self
                .state
                .workspace
                .scoped_user_id_for_workspace(&self.local_user_id, &workspace_id),
            None if self.local_user_id.trim().is_empty() => String::new(),
            None => self.state.workspace.scoped_user_id(&self.local_user_id, None),
        }
    }
}

async fn threads_get(spec: &CommandSpec, ctx: &ExecContext) -> CommandReport {
    let Some(thread_id) = arg_str(&spec.args, "local_thread_id") else {
        return CommandReport::failed("THREAD_REQUIRED", "local_thread_id is required");
    };
    let limit = spec
        .args
        .get("limit")
        .and_then(Value::as_i64)
        .unwrap_or(HISTORY_LIMIT_DEFAULT)
        .clamp(1, HISTORY_LIMIT_MAX);
    let workspace = ctx.state.workspace.clone();
    // The projection the local UI consumes; the transcript crosses the tunnel
    // only while this one command runs (docs §9.3).
    match workspace.load_history_async(&ctx.local_user_id, &thread_id, limit).await {
        Ok(items) => {
            let rows = items.len();
            let report =
                CommandReport::succeeded(json!({ "local_thread_id": thread_id, "items": items, "limit": limit }));
            CommandReport {
                usage: json!({"rows": rows}),
                ..report
            }
        }
        Err(err) => CommandReport::failed("THREAD_ERROR", sanitize(&err.to_string())),
    }
}

/// `thread.create` / `thread.message`: translate into the *existing* engine
/// action (submit a turn to the thread runtime) and answer as soon as the turn
/// is admitted. Ongoing output reaches the cloud through the forwarded thread
/// event stream, never through this command (docs §7.4).
async fn drive_thread(spec: &CommandSpec, ctx: &ExecContext) -> CommandReport {
    let Some(message) = arg_str(&spec.args, "message") else {
        return CommandReport::failed("MESSAGE_REQUIRED", "message is required");
    };
    admit_turn(
        spec,
        ctx,
        arg_str(&spec.args, "local_thread_id"),
        arg_str(&spec.args, "agent").unwrap_or_default(),
        arg_str(&spec.args, "title"),
        message,
        None,
    )
    .await
}

/// Admit one turn through the engine's own admission path: a remote turn enters
/// the orchestrator queue with exactly the rights of a local one (docs §7.2),
/// and the local user keeps the right to take over or cancel at any time.
/// `thread == None` derives a new task session; `spawn_label` marks a derived
/// work unit (`agent.spawn` without a parent thread).
async fn admit_turn(
    spec: &CommandSpec,
    ctx: &ExecContext,
    thread: Option<String>,
    agent: String,
    title: Option<String>,
    message: String,
    spawn_label: Option<String>,
) -> CommandReport {
    let user = match ctx.state.user_store.get_user_by_id(&ctx.local_user_id) {
        Ok(Some(user)) => user,
        Ok(None) => return CommandReport::failed("USER_UNAVAILABLE", "local user is not registered"),
        Err(err) => return CommandReport::failed("USER_UNAVAILABLE", sanitize(&err.to_string())),
    };
    let state = ctx.state.clone();
    let session_id = match thread.clone() {
        Some(id) => id,
        None => match state.kernel.thread_runtime.create_task_session_id(
            &ctx.local_user_id,
            &agent,
        ) {
            Ok(id) => id,
            Err(err) => {
                return CommandReport::failed("THREAD_CREATE_FAILED", sanitize(&err.to_string()))
            }
        },
    };

    // A brand-new remote thread records its origin the way existing code does:
    // on the session row (`spawned_by`) plus the node's workspace binding.
    if thread.is_none() {
        let now = now_ts();
        let record = crate::storage::ChatSessionRecord {
            session_id: session_id.clone(),
            user_id: ctx.local_user_id.clone(),
            title: title
                .map(|title| title.chars().take(96).collect::<String>())
                .unwrap_or_else(|| "远程会话".to_string()),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
            last_message_at: now,
            agent_id: (!agent.is_empty()).then_some(agent.clone()),
            workspace_id: ctx.workspace_id.clone(),
            tool_overrides: Vec::new(),
            parent_session_id: None,
            parent_message_id: None,
            spawn_label,
            spawned_by: Some(ctx.source_tag.clone()),
        };
        if let Err(err) = state.user_store.upsert_chat_session(&record) {
            return CommandReport::failed("THREAD_CREATE_FAILED", sanitize(&err.to_string()));
        }
    }

    let client_message_id = Some(
        format!("remote:{}", spec.command_id)
            .chars()
            .take(128)
            .collect::<String>(),
    );
    let mut request = match crate::api::chat::build_native_chat_request(
        &state,
        &user,
        &session_id,
        message,
        client_message_id,
        Vec::new(),
        None,
    )
    .await
    {
        Ok(request) => request,
        Err(err) => return CommandReport::failed("REQUEST_REJECTED", sanitize(&err.to_string())),
    };
    // Provenance stamp on the same config-overrides channel the goal authority
    // reads: a remote turn is not a directly-observed human turn.
    let overrides = request.config_overrides.get_or_insert_with(|| json!({}));
    if let Some(map) = overrides.as_object_mut() {
        map.insert("__source".to_string(), json!(ctx.source_tag));
    }

    match state.kernel.thread_runtime.submit_user_request(request).await {
        Ok(crate::ThreadSubmitOutcome::Queued(info)) => CommandReport::succeeded(json!({
            "local_thread_id": info.session_id,
            "queued": true,
            "queue_ahead": info.queue_ahead,
        })),
        Ok(crate::ThreadSubmitOutcome::Run(request, lease)) => {
            // Drain the turn in the background; the durable thread log keeps
            // the local UI and the remote view in sync from here on.
            let orchestrator = state.kernel.orchestrator.clone();
            long_task::spawn("interlink.client.thread_run", async move {
                let _lease = lease;
                if let Ok(stream) = orchestrator.stream(*request).await {
                    futures::pin_mut!(stream);
                    while stream.next().await.is_some() {}
                }
            });
            CommandReport::succeeded(json!({
                "local_thread_id": session_id,
                "queued": false,
                "source": ctx.source_tag,
            }))
        }
        Err(err) => CommandReport::failed("SUBMIT_FAILED", sanitize(&err.to_string())),
    }
}

async fn thread_cancel(spec: &CommandSpec, ctx: &ExecContext) -> CommandReport {
    let Some(thread_id) = arg_str(&spec.args, "local_thread_id") else {
        return CommandReport::failed("THREAD_REQUIRED", "local_thread_id is required");
    };
    let state = ctx.state.clone();
    let owner = ctx.local_user_id.clone();
    let goal_cleared = state
        .kernel
        .orchestrator
        .goal_handle()
        .clear(state.storage.clone(), &owner, &thread_id)
        .await
        .is_ok();
    match state
        .kernel
        .thread_runtime
        .cancel_session_activity(&owner, &thread_id, "interlink_remote")
        .await
    {
        Ok(settlement) => CommandReport::succeeded(json!({
            "local_thread_id": thread_id,
            "cancelled": settlement.monitor_cancelled,
            "queued_tasks_cancelled": settlement.queued_tasks_cancelled,
            "child_sessions_cancelled": settlement.child_sessions_cancelled,
            "goal_cleared": goal_cleared,
        })),
        Err(err) => CommandReport::failed("CANCEL_FAILED", sanitize(&err.to_string())),
    }
}

async fn thread_answer(spec: &CommandSpec, ctx: &ExecContext) -> CommandReport {
    let Some(approval_id) = arg_str(&spec.args, "approval_id") else {
        return CommandReport::failed("APPROVAL_REQUIRED", "approval_id is required");
    };
    let Some(decision) = arg_str(&spec.args, "decision")
        .as_deref()
        .and_then(parse_decision)
    else {
        return CommandReport::failed("INVALID_APPROVAL_DECISION", "invalid approval decision");
    };
    let thread_id = arg_str(&spec.args, "local_thread_id");
    let registry = ctx.state.control.approval_registry.clone();
    let Some(snapshot) = registry.get_snapshot(&approval_id).await else {
        return CommandReport::failed("APPROVAL_NOT_FOUND", "approval request not found");
    };
    if let Some(thread_id) = thread_id.as_deref() {
        if snapshot.session_id != thread_id {
            return CommandReport::failed(
                "APPROVAL_SESSION_MISMATCH",
                "approval session mismatch",
            );
        }
    }
    // The registry owns the responder: taking the entry out first is what makes
    // a double answer impossible.
    let Some(entry) = registry.remove(&approval_id).await else {
        return CommandReport::failed("APPROVAL_NOT_FOUND", "approval request not found");
    };
    if entry.respond_to.send(decision).is_err() {
        return CommandReport::failed("APPROVAL_GONE", "approval responder is gone");
    }
    CommandReport::succeeded(json!({
        "approval_id": approval_id,
        "answered": true,
        "source": match entry.source {
            ApprovalSource::ChatWs => "chat_ws",
            ApprovalSource::Channel => "channel",
        },
    }))
}

fn parse_decision(raw: &str) -> Option<ApprovalResponse> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "approve_once" | "once" | "approve-once" => Some(ApprovalResponse::ApproveOnce),
        "approve_session" | "session" | "approve-session" => Some(ApprovalResponse::ApproveSession),
        "deny" | "reject" | "cancel" => Some(ApprovalResponse::Deny),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Workspace mutations (L2): backup first, then the existing workspace path
// ---------------------------------------------------------------------------

async fn workspace_write(
    spec: &CommandSpec,
    ctx: &ExecContext,
    collector: &shadow::ShadowCollector,
) -> CommandReport {
    let Some(path) = arg_str(&spec.args, "path") else {
        return CommandReport::failed("PATH_REQUIRED", "path is required");
    };
    let content = match spec.args.get("content") {
        Some(Value::String(text)) => text.clone(),
        None | Some(Value::Null) => {
            return CommandReport::failed("CONTENT_REQUIRED", "content is required")
        }
        Some(other) => other.to_string(),
    };
    if content.len() as u64 > ctx.max_file_pull_bytes {
        return CommandReport::failed("PAYLOAD_TOO_LARGE", "content exceeds the local ceiling");
    }
    let scope = ctx.scope_user(&spec.args);
    let before = match backup_before(ctx, &scope, &spec.command_id, &path).await {
        Ok(before) => before,
        Err(message) => return CommandReport::failed("BACKUP_FAILED", message),
    };
    let workspace = ctx.state.workspace.clone();
    match workspace.write_file(&scope, &path, &content, true) {
        Ok(()) => {
            workspace.mark_tree_dirty(&scope);
            collector.mark_dirty(Sections::WORKSPACE);
            CommandReport::succeeded(json!({
                "path": path,
                "bytes": content.len(),
                "backup": backup_path_of(&spec.command_id),
                "before_sha256": before,
                "after_sha256": sha256_hex(content.as_bytes()),
            }))
        }
        Err(err) => CommandReport::failed("WORKSPACE_ERROR", sanitize(&err.to_string())),
    }
}

async fn workspace_mkdir(
    spec: &CommandSpec,
    ctx: &ExecContext,
    collector: &shadow::ShadowCollector,
) -> CommandReport {
    let Some(path) = arg_str(&spec.args, "path") else {
        return CommandReport::failed("PATH_REQUIRED", "path is required");
    };
    let scope = ctx.scope_user(&spec.args);
    let workspace = ctx.state.workspace.clone();
    let target = match workspace.resolve_path(&scope, &path) {
        Ok(target) => target,
        Err(err) => return CommandReport::failed("PATH_REJECTED", sanitize(&err.to_string())),
    };
    if target.exists() && !target.is_dir() {
        return CommandReport::failed("TARGET_EXISTS", "target exists and is not a directory");
    }
    let created = blocking::run_fs("interlink.client.mkdir", move || {
        std::fs::create_dir_all(&target).map_err(|err| anyhow::anyhow!(err.to_string()))
    })
    .await;
    match created {
        Ok(()) => {
            workspace.refresh_workspace_tree(&scope);
            collector.mark_dirty(Sections::WORKSPACE);
            CommandReport::succeeded(json!({ "path": path, "created": true }))
        }
        Err(err) => CommandReport::failed("MKDIR_FAILED", sanitize(&err.to_string())),
    }
}

/// `workspace.move` / `workspace.copy` / `workspace.delete` share one shape:
/// resolve both ends inside the fence, back up what is about to change, then
/// apply the same filesystem operations the workspace API applies.
async fn workspace_mutate(
    spec: &CommandSpec,
    ctx: &ExecContext,
    collector: &shadow::ShadowCollector,
) -> CommandReport {
    let Some(source) = arg_str(&spec.args, "path").or_else(|| arg_str(&spec.args, "source")) else {
        return CommandReport::failed("PATH_REQUIRED", "source path is required");
    };
    let destination = arg_str(&spec.args, "destination");
    if spec.kind != CMD_WORKSPACE_DELETE && destination.is_none() {
        return CommandReport::failed("PATH_REQUIRED", "destination is required");
    }
    let scope = ctx.scope_user(&spec.args);
    let workspace = ctx.state.workspace.clone();
    let source_path = match workspace.resolve_path(&scope, &source) {
        Ok(path) => path,
        Err(err) => return CommandReport::failed("PATH_REJECTED", sanitize(&err.to_string())),
    };
    if !source_path.exists() {
        return CommandReport::failed("PATH_NOT_FOUND", "source does not exist");
    }
    let destination_path = match destination.as_deref() {
        Some(value) => match workspace.resolve_path(&scope, value) {
            Ok(path) => Some(path),
            Err(err) => return CommandReport::failed("PATH_REJECTED", sanitize(&err.to_string())),
        },
        None => None,
    };
    if let Some(destination_path) = &destination_path {
        if destination_path.exists() {
            return CommandReport::failed("TARGET_EXISTS", "destination already exists");
        }
        if source_path.is_dir() && destination_path.starts_with(&source_path) {
            return CommandReport::failed("MOVE_INTO_SELF", "cannot move a directory into itself");
        }
    }
    // Moving, copying over, or deleting all need the pre-change copy (§6.4).
    let before = match backup_before(ctx, &scope, &spec.command_id, &source).await {
        Ok(before) => before,
        Err(message) => return CommandReport::failed("BACKUP_FAILED", message),
    };
    let kind = spec.kind.clone();
    let src = source_path;
    let dst = destination_path.unwrap_or_default();
    let performed = blocking::run_fs("interlink.client.mutate", move || {
        let result = if kind == CMD_WORKSPACE_DELETE {
            if src.is_dir() {
                std::fs::remove_dir_all(&src)
            } else {
                std::fs::remove_file(&src)
            }
        } else if kind == CMD_WORKSPACE_MOVE {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(&src, &dst)
        } else if src.is_dir() {
            copy_dir_all(&src, &dst)
        } else {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&src, &dst).map(|_| ())
        };
        result.map_err(|err| anyhow::anyhow!(err.to_string()))
    })
    .await;
    match performed {
        Ok(()) => {
            workspace.refresh_workspace_tree(&scope);
            collector.mark_dirty(Sections::WORKSPACE);
            CommandReport::succeeded(json!({
                "kind": spec.kind,
                "path": source,
                "destination": destination,
                "backup": backup_path_of(&spec.command_id),
                "before_sha256": before,
            }))
        }
        Err(err) => CommandReport::failed("MUTATE_FAILED", sanitize(&err.to_string())),
    }
}

/// Recursively copy a directory, mirroring the workspace API's own helper.
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// `.interlink_backup` folder name inside the projected workspace.
pub fn backup_dir_name() -> &'static str {
    shadow::BACKUP_DIR_NAME
}

fn backup_path_of(command_id: &str) -> String {
    format!("{}/{}", backup_dir_name(), command_id)
}

/// Absolute path of the backup root for one workspace root.
pub fn backup_root_for(root: &Path) -> PathBuf {
    root.join(backup_dir_name())
}

/// Backup folder of one command, relative to the workspace root.
pub fn backup_relative_path(command_id: &str) -> PathBuf {
    PathBuf::from(backup_dir_name()).join(command_id)
}

/// Copy the target into the per-command backup folder and rotate the folder so
/// only the newest [`BACKUP_KEEP`] generations survive. Returns the SHA256 of
/// the file it replaced (`None` for a fresh file, a directory, or anything over
/// [`BACKUP_DIGEST_MAX_BYTES`]), which is the "before" half of the audit pair
/// the design asks for on L2 writes (docs §6.4).
async fn backup_before(
    ctx: &ExecContext,
    scope: &str,
    command_id: &str,
    relative_path: &str,
) -> Result<Option<String>, String> {
    let root = ctx
        .state
        .workspace
        .resolve_path(scope, ".")
        .map_err(|err| sanitize(&err.to_string()))?;
    let source = ctx
        .state
        .workspace
        .resolve_path(scope, relative_path)
        .map_err(|err| sanitize(&err.to_string()))?;
    let generation_id = sanitize_segment(command_id);
    let relative = sanitize_segment(relative_path);
    blocking::run_fs("interlink.client.backup", move || {
        let generation = backup_root_for(&root).join(&generation_id);
        std::fs::create_dir_all(&generation).map_err(|err| anyhow::anyhow!(err.to_string()))?;
        let staged = generation.join(&relative);
        let mut replaced: Option<String> = None;
        if source.is_dir() {
            copy_dir_all(&source, &staged).map_err(|err| anyhow::anyhow!(err.to_string()))?;
        } else if source.exists() {
            replaced = digest_of(&source);
            if let Some(parent) = staged.parent().filter(|path| !path.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent).map_err(|err| anyhow::anyhow!(err.to_string()))?;
            }
            std::fs::copy(&source, &staged).map_err(|err| anyhow::anyhow!(err.to_string()))?;
        }
        let _ = rotate_backups(&backup_root_for(&root), BACKUP_KEEP);
        Ok::<Option<String>, anyhow::Error>(replaced)
    })
    .await
    .map_err(|err| sanitize(&err.to_string()))
}

/// Content digest for the audit trail, bounded so a huge file cannot turn a
/// mutation into an unbounded read on the FS pool.
fn digest_of(path: &Path) -> Option<String> {
    let size = std::fs::metadata(path).ok()?.len();
    if size == 0 || size > BACKUP_DIGEST_MAX_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(sha256_hex(&bytes))
}

/// Outcome of a backup rotation.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BackupRotation {
    pub kept: usize,
    pub removed: Vec<String>,
}

/// Keep only the `keep` newest generations inside `.interlink_backup`.
///
/// Newest-first by directory mtime, name as a deterministic tie-break, so the
/// rotation is stable even when two writes land in the same clock tick.
pub fn rotate_backups(backup_root: &Path, keep: usize) -> BackupRotation {
    let keep = keep.max(1);
    let Ok(read) = std::fs::read_dir(backup_root) else {
        return BackupRotation::default();
    };
    let mut generations: Vec<(f64, String)> = Vec::new();
    for entry in read.flatten() {
        let Ok(metadata) = entry.metadata() else { continue };
        if !metadata.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|since| since.as_secs_f64())
            .unwrap_or(0.0);
        generations.push((mtime, name));
    }
    if generations.len() <= keep {
        return BackupRotation {
            kept: generations.len(),
            removed: Vec::new(),
        };
    }
    generations.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.1.cmp(&left.1))
    });
    let stale = generations.split_off(keep);
    let kept = generations.len();
    let mut removed = Vec::with_capacity(stale.len());
    for (_, name) in stale {
        let _ = std::fs::remove_dir_all(backup_root.join(&name));
        removed.push(name);
    }
    BackupRotation { kept, removed }
}

/// A command id / relative path used as one filesystem segment: keep the
/// printable subset so a crafted id cannot escape the backup folder.
fn sanitize_segment(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .take(64)
        .collect();
    if cleaned.is_empty() || cleaned.chars().all(|c| c == '.') {
        "command".to_string()
    } else {
        cleaned
    }
}

// ---------------------------------------------------------------------------
// Execution (L3): the node's own tool path, never a second one (docs §7.1)
//
// Everything below reaches the engine through the same entry a model turn uses
// (`execute_builtin_tool`, `sessions_spawn`), so the local whitelist, exec
// policy, timeouts and output guards apply unchanged. Plaintext parameters stay
// on this node: the peer gets digests, counts and a short output head (§9.3).
// ---------------------------------------------------------------------------

/// Longest a remotely started command may run on this node.
pub const TOOL_TIMEOUT_MAX_S: f64 = 300.0;
/// Timeout applied when the request asks for none.
pub const TOOL_TIMEOUT_DEFAULT_S: f64 = 60.0;
/// Argument words accepted by one command.
pub const TOOL_ARG_MAX: usize = 32;
/// Longest output excerpt per stream that crosses the tunnel.
pub const TOOL_OUTPUT_HEAD_CHARS: usize = 400;
/// Longest `agent.spawn` may hold its command open waiting for the child run.
pub const SPAWN_WAIT_MAX_S: f64 = 300.0;

/// Structured L3 refusal codes (`command_result.error.code`).
pub const ERR_TOOL_COMMAND_REQUIRED: &str = "TOOL_COMMAND_REQUIRED";
pub const ERR_TOOL_ARGS_INVALID: &str = "TOOL_ARGS_INVALID";
pub const ERR_TOOL_NOT_ALLOWED: &str = "TOOL_NOT_ALLOWED";
pub const ERR_SPAWN_TASK_REQUIRED: &str = "SPAWN_TASK_REQUIRED";

/// A parsed, bounded `tool.exec` request.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolExecRequest {
    pub program: String,
    pub args: Vec<String>,
    pub workdir: String,
    pub timeout_s: f64,
}

impl ToolExecRequest {
    /// One shell line: `<program> <arg> ...`.
    pub fn command_line(&self) -> String {
        if self.args.is_empty() {
            return self.program.clone();
        }
        format!("{} {}", self.program, self.args.join(" "))
    }

    /// Digest prefix: the only form in which a refused command names itself
    /// back to the peer (docs §9.3).
    pub fn fingerprint(&self) -> String {
        sha256_hex(self.command_line().as_bytes())
            .chars()
            .take(16)
            .collect()
    }

    /// The argument set the node's own command tool expects.
    pub fn tool_args(&self) -> Value {
        json!({
            "content": self.command_line(),
            "workdir": self.workdir,
            "timeout_s": self.timeout_s,
        })
    }
}

/// Characters a shell reads as a second command, a redirect or a substitution.
/// The command tool splits its `content` per line whenever a whitelist is
/// active, so a newline here would smuggle an extra command past it; the rest
/// are the same class of escape on one line.
fn has_shell_control(text: &str) -> bool {
    text.contains("$(")
        || text.chars().any(|c| {
            matches!(c, '\n' | '\r' | ';' | '|' | '&' | '<' | '>' | '`')
        })
}

/// Parse and bound one `tool.exec` argument set. Every rejection is structured
/// so the peer can act on the code instead of a message string.
pub fn parse_tool_exec(
    args: &Value,
    frame_timeout_s: Option<f64>,
) -> Result<ToolExecRequest, (&'static str, String)> {
    let program = arg_str(args, "command")
        .ok_or((ERR_TOOL_COMMAND_REQUIRED, "command is required".to_string()))?;
    let mut words = Vec::new();
    match args.get("args") {
        None | Some(Value::Null) => {}
        Some(Value::Array(items)) => {
            if items.len() > TOOL_ARG_MAX {
                return Err((
                    ERR_TOOL_ARGS_INVALID,
                    format!("at most {TOOL_ARG_MAX} arguments are accepted"),
                ));
            }
            for item in items {
                let text = item
                    .as_str()
                    .ok_or((ERR_TOOL_ARGS_INVALID, "args must be strings".to_string()))?;
                words.push(text.to_string());
            }
        }
        Some(_) => {
            return Err((
                ERR_TOOL_ARGS_INVALID,
                "args must be an array of strings".to_string(),
            ))
        }
    }
    if has_shell_control(&program) || words.iter().any(|word| has_shell_control(word)) {
        return Err((
            ERR_TOOL_ARGS_INVALID,
            "shell control characters are not accepted".to_string(),
        ));
    }
    let workdir = args
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(".")
        .to_string();
    // The frame's own window bounds a local run: a remote command must not keep
    // this node busy past the answer the issuer is still waiting for.
    let ceiling = frame_timeout_s
        .unwrap_or(TOOL_TIMEOUT_MAX_S)
        .clamp(1.0, TOOL_TIMEOUT_MAX_S);
    let timeout_s = args
        .get("timeout_s")
        .and_then(Value::as_f64)
        .unwrap_or(TOOL_TIMEOUT_DEFAULT_S)
        .clamp(1.0, ceiling);
    Ok(ToolExecRequest {
        program,
        args: words,
        workdir,
        timeout_s,
    })
}

/// One output stream as the peer sees it: byte count, digest and a short head.
fn stream_summary(text: &str) -> Value {
    json!({
        "bytes": text.len(),
        "sha256": sha256_hex(text.as_bytes()).chars().take(16).collect::<String>(),
        "head": text.chars().take(TOOL_OUTPUT_HEAD_CHARS).collect::<String>(),
    })
}

/// Map the command tool's own result onto a compact command report. A non-zero
/// exit stays structured (the run did happen, the code is the answer); a
/// whitelist miss becomes `TOOL_NOT_ALLOWED` with the reason stated as a digest.
pub fn tool_report_from_result(
    result: &Value,
    elapsed_ms: u64,
    request: &ToolExecRequest,
) -> CommandReport {
    let data = result.get("data").cloned().unwrap_or(Value::Null);
    let row = data
        .get("results")
        .and_then(Value::as_array)
        .and_then(|rows| rows.last().cloned())
        .unwrap_or_else(|| data.clone());
    let exit_code = row.get("returncode").and_then(Value::as_i64);
    let meta_code = result
        .pointer("/error_meta/code")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let timed_out = meta_code == "TOOL_EXEC_TIMEOUT"
        || row.get("timed_out").and_then(Value::as_bool).unwrap_or(false);
    let summary = json!({
        "cwd": request.workdir,
        "timeout_s": request.timeout_s,
        "duration_ms": elapsed_ms,
        "command_sha256": request.fingerprint(),
        "exit_code": exit_code,
        "timed_out": timed_out,
        "stdout": stream_summary(row.get("stdout").and_then(Value::as_str).unwrap_or_default()),
        "stderr": stream_summary(row.get("stderr").and_then(Value::as_str).unwrap_or_default()),
        "truncated": row.get("truncated").and_then(Value::as_bool).unwrap_or(false),
        "output_guard": {
            "total_bytes": row.get("total_bytes").and_then(Value::as_u64).unwrap_or(0),
            "omitted_bytes": row.get("omitted_bytes").and_then(Value::as_u64).unwrap_or(0),
        },
    });
    if result.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return CommandReport {
            usage: json!({ "duration_ms": elapsed_ms }),
            ..CommandReport::succeeded(summary)
        };
    }
    if meta_code == "TOOL_EXEC_NOT_ALLOWED" {
        return CommandReport::failed(
            ERR_TOOL_NOT_ALLOWED,
            format!(
                "command {} is not in this node's security.allow_commands list",
                request.fingerprint()
            ),
        );
    }
    if timed_out {
        return CommandReport {
            result: summary,
            ..CommandReport::timed_out(format!(
                "command {} exceeded {}s",
                request.fingerprint(),
                request.timeout_s
            ))
        };
    }
    let code = if meta_code.is_empty() {
        "TOOL_EXEC_FAILED"
    } else {
        meta_code
    };
    CommandReport {
        result: summary,
        ..CommandReport::failed(
            code,
            format!(
                "command {} exited with {}",
                request.fingerprint(),
                exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "a tool failure".to_string())
            ),
        )
    }
}

/// `tool.exec`: run one allow-listed command with the node's own command tool.
async fn tool_exec(
    spec: &CommandSpec,
    ctx: &ExecContext,
    collector: &shadow::ShadowCollector,
    cancel: &CancellationToken,
) -> CommandReport {
    let request = match parse_tool_exec(&spec.args, spec.timeout_s) {
        Ok(request) => request,
        Err((code, reason)) => return CommandReport::failed(code, reason),
    };
    let scope = ctx.scope_user(&spec.args);
    if scope.trim().is_empty() {
        return CommandReport::failed("USER_UNAVAILABLE", "the node has no local user scope");
    }
    let detached = DetachedToolContext::load(&ctx.state).await;
    let session_id = format!("interlink_{}", sanitize_segment(&spec.command_id));
    let tool_context = detached.bind(&ctx.local_user_id, &session_id, &scope);
    let tool_args = request.tool_args();
    let started = Instant::now();
    let outcome = tokio::select! {
        result = execute_builtin_tool(&tool_context, "execute_command", &tool_args) => Some(result),
        _ = cancel.cancelled() => None,
    };
    let elapsed_ms = started.elapsed().as_millis() as u64;
    // Whatever happened, the command may have touched the workspace; the delta
    // window re-projects it (docs §6.2).
    collector.mark_dirty(shadow::command_sections(&spec.kind));
    match outcome {
        None => CommandReport::canceled("command canceled on this node"),
        Some(Err(err)) => CommandReport::failed("TOOL_EXEC_FAILED", sanitize(&err.to_string())),
        Some(Ok(result)) => tool_report_from_result(&result, elapsed_ms, &request),
    }
}

/// `agent.spawn` with a parent thread: derive a work unit through the engine's
/// own subagent path. The child answer stays local - the peer follows it with
/// the remote session view or `threads.get`, so no message body crosses here.
async fn spawn_child_unit(
    ctx: &ExecContext,
    parent: &str,
    task: String,
    label: Option<String>,
    agent: String,
    wait_s: f64,
) -> CommandReport {
    let scope = ctx.scope_of_thread(parent);
    if scope.trim().is_empty() {
        return CommandReport::failed("USER_UNAVAILABLE", "the node has no local user scope");
    }
    let detached = DetachedToolContext::load(&ctx.state).await;
    let tool_context = detached.bind(&ctx.local_user_id, parent, &scope);
    let args = json!({
        "task": task,
        "label": label,
        "agent_id": (!agent.is_empty()).then_some(agent),
        "run_timeout_seconds": (wait_s > 0.0).then_some(wait_s),
    });
    match sessions_spawn(&tool_context, &args).await {
        Ok(result) => spawn_report(&result),
        Err(err) => CommandReport::failed("SPAWN_FAILED", sanitize(&err.to_string())),
    }
}

fn spawn_report(result: &Value) -> CommandReport {
    let state = result
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let thread_id = result
        .pointer("/data/session_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if thread_id.is_empty() {
        return CommandReport::failed("SPAWN_FAILED", "the node returned no thread id");
    }
    let payload = json!({
        "local_thread_id": thread_id,
        "run_id": result.pointer("/data/run_id").and_then(Value::as_str),
        "state": state,
    });
    match state.as_str() {
        "accepted" | "running" | "completed" => CommandReport::succeeded(payload),
        "timeout" => CommandReport {
            result: payload,
            ..CommandReport::timed_out("the child run did not finish inside its window")
        },
        other => CommandReport {
            result: payload,
            ..CommandReport::failed("SPAWN_NOT_COMPLETED", other.to_string())
        },
    }
}

/// `agent.spawn`: one new work unit on the local engine, either as a subagent of
/// a thread the caller names or as an independent task thread. The result is a
/// `local_thread_id`, so the unit is driven, watched, taken over or canceled
/// exactly like a `thread.drive` session (docs §7.2).
async fn agent_spawn(
    spec: &CommandSpec,
    ctx: &ExecContext,
    collector: &shadow::ShadowCollector,
) -> CommandReport {
    let Some(task) = arg_str(&spec.args, "task") else {
        return CommandReport::failed(ERR_SPAWN_TASK_REQUIRED, "task is required");
    };
    let label = arg_str(&spec.args, "label");
    let agent = arg_str(&spec.args, "agent").unwrap_or_default();
    let wait_s = spec
        .args
        .get("wait_s")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, SPAWN_WAIT_MAX_S);
    let report = match arg_str(&spec.args, "parent_thread_id") {
        Some(parent) => {
            spawn_child_unit(ctx, &parent, task, label.clone(), agent, wait_s).await
        }
        None => {
            admit_turn(
                spec,
                ctx,
                None,
                agent,
                arg_str(&spec.args, "title"),
                task,
                label,
            )
            .await
        }
    };
    collector.mark_dirty(shadow::command_sections(&spec.kind));
    report
}

fn arg_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Trim and keep error text log-safe: no workspace root, no token material.
pub fn sanitize(text: &str) -> String {
    let first = text.lines().next().unwrap_or_default();
    let cleaned: String = first.chars().take(180).collect();
    if cleaned.trim().is_empty() {
        "operation failed".to_string()
    } else {
        cleaned
    }
}

/// Approval state reported when no decision arrived in time.
pub fn expired_state(now: f64, expires_at: Option<f64>) -> &'static str {
    match expires_at {
        Some(value) if now >= value => APPROVAL_EXPIRED,
        _ => APPROVAL_REJECTED,
    }
}

/// Bounded in-flight table, consulted by `command.cancel` and the busy check.
#[derive(Debug, Default)]
pub struct InFlightTable {
    ids: RwLock<HashSet<String>>,
}

impl InFlightTable {
    pub fn try_add(&self, command_id: &str, limit: usize) -> bool {
        let mut ids = self.ids.write().expect("in-flight table lock poisoned");
        if ids.len() >= limit {
            return false;
        }
        ids.insert(command_id.to_string())
    }

    pub fn remove(&self, command_id: &str) {
        let mut ids = self.ids.write().expect("in-flight table lock poisoned");
        ids.remove(command_id);
    }

    pub fn len(&self) -> usize {
        self.ids.read().expect("in-flight table lock poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn snapshot(&self) -> Vec<String> {
        self.ids
            .read()
            .expect("in-flight table lock poisoned")
            .iter()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wunder_core::interlink::{
        CAP_AGENT_SPAWN, CAP_QUERY_BASIC, CAP_TOOL_EXEC, COMMAND_STATUS_FAILED,
        COMMAND_STATUS_SUCCEEDED, COMMAND_STATUS_TIMEOUT, ERR_APPROVAL_REJECTED, RISK_HIGH,
        is_queueable_when_offline,
    };

    fn spec(kind: &str, args: Value) -> CommandSpec {
        CommandSpec::parse(
            Some("cmd_1"),
            &json!({"kind": kind, "args": args, "from": "web:conn", "actor": "u_1"}),
        )
        .expect("command spec")
    }

    #[test]
    fn l3_kinds_are_supported_and_carry_the_high_risk_tier() {
        for kind in [CMD_TOOL_EXEC, CMD_AGENT_SPAWN] {
            assert!(is_supported_kind(kind));
            assert_eq!(command_level(kind), "L3");
            assert_eq!(spec(kind, json!({})).risk(), RISK_HIGH);
            // A command that may touch anything re-projects every section.
            assert_eq!(shadow::command_sections(kind), Sections::ALL);
            assert!(!is_queueable_when_offline(kind));
        }
        assert!(!is_supported_kind("shell.exec"));
    }

    #[test]
    fn preflight_refuses_l3_outside_the_effective_set() {
        let tool = spec(CMD_TOOL_EXEC, json!({"command": "echo", "args": ["ready"]}));
        let spawn = spec(CMD_AGENT_SPAWN, json!({"task": "collect the open notes"}));
        let both = vec![CAP_TOOL_EXEC.to_string(), CAP_AGENT_SPAWN.to_string()];
        let basic = vec![CAP_QUERY_BASIC.to_string()];
        let only_tool = vec![CAP_TOOL_EXEC.to_string()];
        let gate = ApprovalGate::default();

        // Out of the server grant or out of what this node declares: either way
        // the node refuses, and no prompt is raised.
        assert_eq!(
            preflight(&tool, &basic, &both, &gate, "prompt", 0.0),
            Preflight::Reject(ERR_CAP_DENIED, "none")
        );
        assert_eq!(
            preflight(&tool, &both, &basic, &gate, "prompt", 0.0),
            Preflight::Reject(ERR_CAP_DENIED, "none")
        );
        // Each L3 kind carries its own capability: one never implies the other.
        assert_eq!(
            preflight(&spawn, &only_tool, &only_tool, &gate, "prompt", 0.0),
            Preflight::Reject(ERR_CAP_DENIED, "none")
        );
        // Inside the intersection: always a live prompt, never an auto-allow.
        assert_eq!(
            preflight(&tool, &both, &both, &gate, "prompt", 0.0),
            Preflight::NeedPrompt
        );
        assert_eq!(
            preflight(&spawn, &both, &both, &gate, "prompt", 0.0),
            Preflight::NeedPrompt
        );
        // A node without an interactive surface refuses instead of running blind.
        assert_eq!(
            preflight(&tool, &both, &both, &gate, "deny_all", 0.0),
            Preflight::Reject(ERR_APPROVAL_REJECTED, APPROVAL_REJECTED)
        );
        assert_eq!(
            preflight(&spec("unknown.kind", json!({})), &both, &both, &gate, "prompt", 0.0),
            Preflight::Reject(ERR_UNKNOWN_KIND, "none")
        );
    }

    #[test]
    fn denial_ack_and_timeout_reports_are_structured() {
        let ack = CommandReport::ack_denied(ERR_CAP_DENIED, "none");
        assert_eq!(ack["accepted"], json!(false));
        assert_eq!(ack["error_code"], json!(ERR_CAP_DENIED));
        assert_eq!(ack["approval_state"], json!("none"));

        let report = CommandReport::timed_out("window closed");
        assert_eq!(report.status, COMMAND_STATUS_TIMEOUT);
        assert_eq!(
            report.payload()["error"]["code"].as_str(),
            Some(wunder_core::interlink::ERR_TIMEOUT)
        );
    }

    #[test]
    fn tool_exec_request_is_bounded_and_single_line() {
        let request = parse_tool_exec(
            &json!({"command": "echo", "args": ["a", "b"], "cwd": "notes", "timeout_s": 5.0}),
            Some(30.0),
        )
        .expect("parse");
        assert_eq!(request.command_line(), "echo a b");
        assert_eq!(request.tool_args()["content"], json!("echo a b"));
        assert_eq!(request.tool_args()["workdir"], json!("notes"));
        assert_eq!(request.timeout_s, 5.0);
        assert_eq!(request.fingerprint().len(), 16);

        assert_eq!(
            parse_tool_exec(&json!({}), None).unwrap_err().0,
            ERR_TOOL_COMMAND_REQUIRED
        );
        // A newline is split into an extra command by the tool's own line
        // parser: exactly the whitelist escape this layer refuses. The rest are
        // the shell control characters of the same class.
        for args in [
            json!({"command": "echo", "args": ["a\nb"]}),
            json!({"command": "echo; x"}),
            json!({"command": "echo", "args": ["$(x)"]}),
            json!({"command": "echo", "args": ["x>out"]}),
            json!({"command": "echo", "args": ["x|y"]}),
        ] {
            assert_eq!(
                parse_tool_exec(&args, None).unwrap_err().0,
                ERR_TOOL_ARGS_INVALID,
                "{args}"
            );
        }
        assert_eq!(
            parse_tool_exec(&json!({"command": "echo", "args": "not-an-array"}), None)
                .unwrap_err()
                .0,
            ERR_TOOL_ARGS_INVALID
        );
        let too_many = (0..TOOL_ARG_MAX + 1)
            .map(|_| Value::from("x"))
            .collect::<Vec<_>>();
        assert_eq!(
            parse_tool_exec(&json!({"command": "echo", "args": too_many}), None)
                .unwrap_err()
                .0,
            ERR_TOOL_ARGS_INVALID
        );
        // The frame's own window caps a local run.
        assert_eq!(
            parse_tool_exec(&json!({"command": "echo", "timeout_s": 3_600.0}), Some(5.0))
                .expect("capped")
                .timeout_s,
            5.0
        );
        assert_eq!(
            parse_tool_exec(&json!({"command": "echo"}), None)
                .expect("default")
                .timeout_s,
            TOOL_TIMEOUT_DEFAULT_S
        );
    }

    #[test]
    fn whitelist_miss_comes_back_as_a_structured_code_without_plaintext() {
        let request =
            parse_tool_exec(&json!({"command": "echo", "args": ["keep this local"]}), None)
                .expect("parse");
        let refused = json!({
            "ok": false,
            "error": "tool.exec.not_allowed",
            "data": {"command": "echo keep this local"},
            "error_meta": {"code": "TOOL_EXEC_NOT_ALLOWED"},
        });
        let report = tool_report_from_result(&refused, 12, &request);
        assert_eq!(report.status, COMMAND_STATUS_FAILED);
        let (code, reason) = report.error.clone().expect("error pair");
        assert_eq!(code, ERR_TOOL_NOT_ALLOWED);
        assert!(reason.contains(&request.fingerprint()));
        assert!(!reason.contains("keep this local"));
        // Nothing in the answer carries the command line either (docs §9.3).
        assert!(!report.payload().to_string().contains("keep this local"));
    }

    #[test]
    fn a_finished_run_reports_code_duration_and_digests() {
        let request =
            parse_tool_exec(&json!({"command": "echo", "args": ["ok"], "cwd": "notes"}), None)
                .expect("parse");
        let result = json!({
            "ok": true,
            "state": "completed",
            "data": {"results": [{
                "command": "echo ok",
                "returncode": 0,
                "stdout": "ok\n",
                "stderr": "",
                "truncated": false,
                "total_bytes": 3,
                "omitted_bytes": 0,
            }]},
        });
        let report = tool_report_from_result(&result, 42, &request);
        assert_eq!(report.status, COMMAND_STATUS_SUCCEEDED);
        assert_eq!(report.result["exit_code"], json!(0));
        assert_eq!(report.result["duration_ms"], json!(42));
        assert_eq!(report.result["cwd"], json!("notes"));
        assert_eq!(report.result["stdout"]["head"], json!("ok\n"));
        assert_eq!(report.result["stdout"]["bytes"], json!(3));
        assert!(report.result["stdout"]["sha256"].as_str().is_some());
        assert!(report.result.get("command").is_none());
        assert_eq!(report.usage["duration_ms"], json!(42));
        assert!(!report.payload().to_string().contains("echo ok"));
    }

    #[test]
    fn non_zero_exit_and_timeout_keep_the_summary_and_their_own_code() {
        let request = parse_tool_exec(&json!({"command": "x"}), Some(9.0)).expect("parse");
        let failed = tool_report_from_result(
            &json!({
                "ok": false,
                "data": {"results": [{"command": "x", "returncode": 3, "stdout": "", "stderr": "nope"}]},
                "error_meta": {"code": "TOOL_EXEC_NON_ZERO_EXIT"},
            }),
            7,
            &request,
        );
        assert_eq!(failed.status, COMMAND_STATUS_FAILED);
        assert_eq!(
            failed.error.as_ref().expect("error").0,
            "TOOL_EXEC_NON_ZERO_EXIT"
        );
        assert_eq!(failed.result["exit_code"], json!(3));

        let timed_out = tool_report_from_result(
            &json!({
                "ok": false,
                "data": {"results": [{"command": "x", "timed_out": true}]},
                "error_meta": {"code": "TOOL_EXEC_TIMEOUT"},
            }),
            9_000,
            &request,
        );
        assert_eq!(timed_out.status, COMMAND_STATUS_TIMEOUT);
        assert_eq!(timed_out.result["timed_out"], json!(true));
    }

    #[test]
    fn before_digest_is_content_sha_and_stays_bounded() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("note.txt");
        std::fs::write(&file, b"hello").expect("write");
        assert_eq!(
            digest_of(&file).as_deref(),
            Some(sha256_hex(b"hello").as_str())
        );

        // An empty file has no meaningful "before", and anything past the cap is
        // skipped rather than read whole.
        let empty = dir.path().join("empty.txt");
        std::fs::write(&empty, b"").expect("write empty");
        assert_eq!(digest_of(&empty), None);
        let big = dir.path().join("big.bin");
        std::fs::write(&big, vec![0u8; (BACKUP_DIGEST_MAX_BYTES + 1) as usize])
            .expect("write big");
        assert_eq!(digest_of(&big), None);
        assert_eq!(digest_of(&dir.path().join("missing.txt")), None);
    }

    #[test]
    fn output_head_is_bounded() {
        let long = "x".repeat(TOOL_OUTPUT_HEAD_CHARS + 200);
        let summary = stream_summary(&long);
        let head = summary["head"].as_str().expect("head");
        assert_eq!(head.chars().count(), TOOL_OUTPUT_HEAD_CHARS);
        assert_eq!(summary["bytes"].as_u64(), Some(long.len() as u64));
        assert_eq!(summary["sha256"].as_str().map(str::len), Some(16));
        assert_eq!(stream_summary("")["bytes"].as_u64(), Some(0));
    }

    #[test]
    fn spawn_report_returns_the_thread_handle_and_no_child_output() {
        let accepted = spawn_report(&json!({
            "ok": true,
            "state": "accepted",
            "data": {"session_id": "th_child_1", "run_id": "run_1", "reply": "private body"},
        }));
        assert_eq!(accepted.status, COMMAND_STATUS_SUCCEEDED);
        assert_eq!(accepted.result["local_thread_id"], json!("th_child_1"));
        assert_eq!(accepted.result["run_id"], json!("run_1"));
        assert!(!accepted.payload().to_string().contains("private body"));

        assert_eq!(
            spawn_report(&json!({"ok": true, "state": "accepted", "data": {}}))
                .error
                .as_ref()
                .expect("error")
                .0,
            "SPAWN_FAILED"
        );
        let timeout = spawn_report(&json!({
            "ok": true, "state": "timeout", "data": {"session_id": "th_child_2"},
        }));
        assert_eq!(timeout.status, COMMAND_STATUS_TIMEOUT);
        assert_eq!(timeout.result["local_thread_id"], json!("th_child_2"));
    }
}
