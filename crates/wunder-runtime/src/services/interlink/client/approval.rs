//! On-device human-in-the-loop approval gate (docs §7.3).
//!
//! The server mints the ticket; every decision is taken here, on the controlled
//! node, and travels back as a `command_ack`. Rules that must never drift:
//!
//! * `L0` (read-only) executes without a prompt - that is the §7.1 default.
//! * `L1` may be auto-approved by a *scope memory* `(from_user, level)` that
//!   lasts at most [`MEMORY_TTL_S`]; the local UI can revoke it.
//! * `L2`/`L3` always require a live decision, never remembered, never
//!   auto-approved (`approvals::is_memorizable` is the single source of that
//!   rule, shared with the server side).
//! * no decision inside [`APPROVAL_TIMEOUT_S`] means rejection (fail-closed).
//! * pending prompts are bounded by [`PENDING_MAX`]; beyond that a command is
//!   refused outright instead of stacking up modals (docs §7.3 5).

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::RwLock;

use tokio::sync::oneshot;

use wunder_core::interlink::{command_level, requires_hard_approval};

use crate::services::interlink::approvals;

/// Approval timeout when the command frame carries none (docs §7.3 4).
pub const APPROVAL_TIMEOUT_S: f64 = 120.0;
/// Hard cap of concurrent prompts.
pub const PENDING_MAX: usize = 5;
/// Scope memory entries kept, L1 only.
pub const MEMORY_MAX: usize = 16;
/// A remembered L1 grant never outlives 30 minutes (docs §7.3 3).
pub const MEMORY_TTL_S: f64 = 1_800.0;
/// Longest the caller may ask a prompt to stay open.
pub const APPROVAL_TIMEOUT_MAX_S: f64 = 600.0;

/// What the local surfaces (Slint modal / TUI line) answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Approve,
    Deny,
}

/// How one command should be handled before it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalPlan {
    /// Execute immediately (L0, or a remembered L1 scope).
    AutoAllow,
    /// Ask a human; a missing answer inside the window rejects.
    RequirePrompt,
    /// Refuse outright with a stable code; nothing is executed.
    AutoDeny(&'static str),
}

/// One prompt waiting for the user.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PendingApproval {
    pub approval_id: String,
    pub command_id: String,
    pub kind: String,
    pub level: String,
    pub risk: String,
    pub from_node: String,
    /// Human-readable summary: operation, source, target. Never an argument
    /// body (docs §9.3: prompts stay free of message/file content).
    pub prompt: String,
    pub expires_at: f64,
}

/// Why a prompt could not be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateError {
    /// The bounded prompt queue is full (docs §7.3 5).
    QueueFull,
}

#[derive(Debug)]
struct Pending {
    approval: PendingApproval,
    respond: oneshot::Sender<Decision>,
}

#[derive(Debug, Default)]
struct State {
    pending: HashMap<String, Pending>,
    /// FIFO of `approval_id`, used only to evict the oldest prompt deterministically.
    order: VecDeque<String>,
    /// `(from_user, level) -> expires_at`; L1 only.
    memory: HashMap<(String, String), f64>,
    memory_order: VecDeque<(String, String)>,
    /// Commands whose prompt was already answered, so a duplicate decision is
    /// a no-op instead of a second side effect.
    decided: HashSet<String>,
    decided_order: VecDeque<String>,
}

/// Bounded prompt + scope-memory store shared by the command tasks and the UI.
#[derive(Debug, Default)]
pub struct ApprovalGate {
    state: RwLock<State>,
}

/// Pure precedence decision (docs §7.3 2): the kind's own tier and the local
/// policy decide, memory only ever applies to L1.
///
/// `policy` is `approval_default` from the session file:
/// * `prompt` - ask a human when no memory covers the command.
/// * `allow_readonly` - the node has no interactive surface: L0 runs, anything
///   that would need a prompt is refused.
/// * `deny_all` - refuse anything that would need a prompt.
pub fn plan(kind: &str, policy: &str, remembered: bool) -> ApprovalPlan {
    let level = command_level(kind);
    if level == "L0" {
        return ApprovalPlan::AutoAllow;
    }
    if level == "L1" && !requires_hard_approval(kind) && remembered {
        return ApprovalPlan::AutoAllow;
    }
    match policy {
        // A non-interactive node must not silently open L2/L3 to a remote peer.
        "deny_all" => ApprovalPlan::AutoDeny("DENY_ALL_POLICY"),
        "allow_readonly" => ApprovalPlan::AutoDeny("READONLY_POLICY"),
        _ => ApprovalPlan::RequirePrompt,
    }
}

/// Approval window of one command: the smaller of the frame's own expiry and
/// the documented default, clamped into a sane band.
pub fn window_seconds(expires_at: Option<f64>, timeout_s: Option<f64>, now: f64) -> f64 {
    let from_expiry = expires_at.map(|value| (value - now).max(0.0));
    let from_timeout = timeout_s.map(|value| value.clamp(1.0, APPROVAL_TIMEOUT_MAX_S));
    from_expiry
        .into_iter()
        .chain(from_timeout)
        .min_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap_or(APPROVAL_TIMEOUT_S)
        .min(APPROVAL_TIMEOUT_S)
        .max(1.0)
}

impl ApprovalGate {
    /// Whether a remembered grant currently covers `(from_user, kind)`.
    pub fn remembered(&self, from_user: &str, kind: &str, now: f64) -> bool {
        if !approvals::is_memorizable(kind) {
            return false;
        }
        let key = memory_key(from_user, kind);
        let state = self.state.read().expect("approval gate lock poisoned");
        state
            .memory
            .get(&key)
            .map(|expires_at| *expires_at > now)
            .unwrap_or(false)
    }

    /// Decide what to do with one command; consults the scope memory.
    pub fn decide_plan(
        &self,
        kind: &str,
        from_user: &str,
        policy: &str,
        now: f64,
    ) -> ApprovalPlan {
        plan(kind, policy, self.remembered(from_user, kind, now))
    }

    /// Open a prompt. Returns the ticket to show plus the receiver the command
    /// task awaits. Fails when the bounded queue is full.
    pub fn open(
        &self,
        approval: PendingApproval,
    ) -> Result<oneshot::Receiver<Decision>, GateError> {
        let approval_id = approval.approval_id.clone();
        let command_id = approval.command_id.clone();
        let (respond, receiver) = oneshot::channel();
        let mut state = self.state.write().expect("approval gate lock poisoned");
        prune_prompts(&mut state, approval.expires_at);
        if state.pending.contains_key(&approval_id) {
            // Same ticket reopened: refuse rather than show two modals.
            return Err(GateError::QueueFull);
        }
        if state.pending.len() >= PENDING_MAX {
            return Err(GateError::QueueFull);
        }
        state.order.push_back(approval_id.clone());
        state.pending.insert(
            approval_id,
            Pending {
                approval,
                respond,
            },
        );
        // A decided command never re-opens: forget the stale ticket.
        state.decided.remove(&command_id);
        Ok(receiver)
    }

    /// Snapshot of the prompt queue for the UI (bounded by [`PENDING_MAX`]).
    pub fn pending(&self) -> Vec<PendingApproval> {
        let state = self.state.read().expect("approval gate lock poisoned");
        state
            .order
            .iter()
            .filter_map(|id| state.pending.get(id))
            .map(|entry| entry.approval.clone())
            .collect()
    }

    /// Apply the user's decision. `remember` only takes effect for L1 kinds.
    /// Returns `false` when the ticket is unknown or already decided.
    pub fn decide(
        &self,
        approval_id: &str,
        decision: Decision,
        remember: bool,
        now: f64,
    ) -> bool {
        let mut state = self.state.write().expect("approval gate lock poisoned");
        let Some(entry) = state.pending.remove(approval_id) else {
            return false;
        };
        state.order.retain(|id| id != approval_id);
        if decision == Decision::Approve && remember {
            let key = memory_key(&entry.approval.from_node, &entry.approval.kind);
            if approvals::is_memorizable(&entry.approval.kind) {
                remember_grant(&mut state, key, now + MEMORY_TTL_S);
            }
        }
        mark_decided(&mut state, entry.approval.command_id.clone());
        // The command task owns the timeout; a dropped receiver is its signal
        // that the prompt vanished (node shutdown), i.e. still a rejection.
        entry.respond.send(decision).is_ok()
    }

    /// Drop the prompt attached to one command (cancel, disconnect, timeout).
    pub fn dismiss_command(&self, command_id: &str) -> usize {
        let mut state = self.state.write().expect("approval gate lock poisoned");
        let ids: Vec<String> = state
            .pending
            .iter()
            .filter(|(_, entry)| entry.approval.command_id == command_id)
            .map(|(id, _)| id.clone())
            .collect();
        let removed = ids.len();
        for id in ids {
            state.pending.remove(&id);
            state.order.retain(|existing| existing != &id);
        }
        removed
    }

    /// Forget every prompt: called when the tunnel drops, because a decision
    /// for a command the server no longer tracks is meaningless.
    pub fn clear(&self) {
        let mut state = self.state.write().expect("approval gate lock poisoned");
        state.pending.clear();
        state.order.clear();
    }

    /// Revoke remembered grants (UI switch, or `deny_all` taking effect).
    pub fn forget_memory(&self) {
        let mut state = self.state.write().expect("approval gate lock poisoned");
        state.memory.clear();
        state.memory_order.clear();
    }

    pub fn memory_grants(&self, now: f64) -> Vec<(String, String, f64)> {
        let state = self.state.read().expect("approval gate lock poisoned");
        state
            .memory
            .iter()
            .filter(|(_, expires_at)| **expires_at > now)
            .map(|((from_user, level), expires_at)| {
                (from_user.clone(), level.clone(), *expires_at)
            })
            .collect()
    }
}

fn memory_key(from_user: &str, kind: &str) -> (String, String) {
    (
        from_user.trim().to_ascii_lowercase(),
        command_level(kind).to_string(),
    )
}

fn remember_grant(state: &mut State, key: (String, String), expires_at: f64) {
    if !state.memory.contains_key(&key) {
        while state.memory.len() >= MEMORY_MAX {
            match state.memory_order.pop_front() {
                Some(stale) => {
                    state.memory.remove(&stale);
                }
                None => break,
            }
        }
        state.memory_order.push_back(key.clone());
    }
    // A grant is only ever extended, never shortened, inside its window.
    let current = state.memory.entry(key).or_insert(expires_at);
    if expires_at > *current {
        *current = expires_at;
    }
}

fn mark_decided(state: &mut State, command_id: String) {
    const DECIDED_MAX: usize = 256;
    if state.decided.contains(&command_id) {
        return;
    }
    while state.decided.len() >= DECIDED_MAX {
        match state.decided_order.pop_front() {
            Some(stale) => {
                state.decided.remove(&stale);
            }
            None => break,
        }
    }
    state.decided_order.push_back(command_id.clone());
    state.decided.insert(command_id);
}

/// Drop prompts whose window already passed. Expired prompts are simply
/// removed: the awaiting command task has its own timeout and answers the
/// server with `expired`.
fn prune_prompts(state: &mut State, now: f64) {
    let expired: Vec<String> = state
        .pending
        .iter()
        .filter(|(_, entry)| entry.approval.expires_at <= now)
        .map(|(id, _)| id.clone())
        .collect();
    for id in expired {
        state.pending.remove(&id);
        state.order.retain(|existing| existing != &id);
    }
}

/// Prompt text shown to the local user. The wording is produced by the shared
/// approval helper (`services/interlink/approvals.rs`), so the node and the
/// server's ticket carry the same text: tier, kind, source node and a target
/// summary. Argument bodies never reach it (docs §9.3).
pub fn prompt_text(kind: &str, from_node: &str, args: &serde_json::Value) -> String {
    let label = if from_node.trim().is_empty() {
        "cloud"
    } else {
        from_node.trim()
    };
    approvals::prompt_text(kind, label, args)
}
