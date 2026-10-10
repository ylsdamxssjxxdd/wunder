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
use crate::state::AppState;

/// Longest window a command id is remembered for (docs §4.3).
pub const SEEN_TTL_S: f64 = 24.0 * 3_600.0;
/// Hard bound of the idempotency set.
pub const SEEN_MAX: usize = 512;
/// Backup generations kept per workspace (docs §6.4).
pub const BACKUP_KEEP: usize = 50;
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
pub fn preflight(
    spec: &CommandSpec,
    capabilities: &[String],
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
    if !capability_allowed(&spec.kind, capabilities) {
        return Preflight::Reject(ERR_CAP_DENIED, "none");
    }
    match gate.decide_plan(&spec.kind, &spec.actor, policy, now) {
        ApprovalPlan::AutoAllow => Preflight::Allow,
        ApprovalPlan::RequirePrompt => Preflight::NeedPrompt,
        ApprovalPlan::AutoDeny(_) => Preflight::Reject(ERR_APPROVAL_REJECTED, APPROVAL_REJECTED),
    }
}

/// Kinds this node can actually run. L3 (`tool.exec`, `agent.spawn`) is not
/// declared in `hello` and has no local implementation, so it is refused here
/// as well.
pub fn is_supported_kind(kind: &str) -> bool {
    if matches!(kind, CMD_TOOL_EXEC | CMD_AGENT_SPAWN) {
        return false;
    }
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
    let user = match ctx.state.user_store.get_user_by_id(&ctx.local_user_id) {
        Ok(Some(user)) => user,
        Ok(None) => return CommandReport::failed("USER_UNAVAILABLE", "local user is not registered"),
        Err(err) => return CommandReport::failed("USER_UNAVAILABLE", sanitize(&err.to_string())),
    };
    let agent = arg_str(&spec.args, "agent").unwrap_or_default();
    let requested_thread = arg_str(&spec.args, "local_thread_id");
    let state = ctx.state.clone();
    let session_id = match requested_thread.clone() {
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
    if requested_thread.is_none() {
        let now = now_ts();
        let record = crate::storage::ChatSessionRecord {
            session_id: session_id.clone(),
            user_id: ctx.local_user_id.clone(),
            title: arg_str(&spec.args, "title")
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
            spawn_label: None,
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
    if let Err(message) = backup_before(ctx, &scope, &spec.command_id, &path).await {
        return CommandReport::failed("BACKUP_FAILED", message);
    }
    let workspace = ctx.state.workspace.clone();
    match workspace.write_file(&scope, &path, &content, true) {
        Ok(()) => {
            workspace.mark_tree_dirty(&scope);
            collector.mark_dirty(Sections::WORKSPACE);
            CommandReport::succeeded(json!({
                "path": path,
                "bytes": content.len(),
                "backup": backup_path_of(&spec.command_id),
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
    if let Err(message) = backup_before(ctx, &scope, &spec.command_id, &source).await {
        return CommandReport::failed("BACKUP_FAILED", message);
    }
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
/// only the newest [`BACKUP_KEEP`] generations survive.
async fn backup_before(
    ctx: &ExecContext,
    scope: &str,
    command_id: &str,
    relative_path: &str,
) -> Result<(), String> {
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
        if source.is_dir() {
            copy_dir_all(&source, &staged).map_err(|err| anyhow::anyhow!(err.to_string()))?;
        } else if source.exists() {
            if let Some(parent) = staged.parent().filter(|path| !path.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent).map_err(|err| anyhow::anyhow!(err.to_string()))?;
            }
            std::fs::copy(&source, &staged).map_err(|err| anyhow::anyhow!(err.to_string()))?;
        }
        let rotation = rotate_backups(&backup_root_for(&root), BACKUP_KEEP);
        Ok::<usize, anyhow::Error>(rotation.removed.len())
    })
    .await
    .map(|_| ())
    .map_err(|err| sanitize(&err.to_string()))
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
