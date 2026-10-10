//! Human-in-the-loop approval tickets (docs §7.3).
//!
//! The server only mints, persists and expires tickets: every decision is taken
//! on the controlled device (蜂窝 modal / 舵机 TUI line) and arrives back as a
//! `command_ack`. Fail-closed: a missing decision is a rejection.

use serde_json::Value;
use uuid::Uuid;

use wunder_core::interlink::{APPROVAL_PENDING, RISK_HIGH, command_level};

use crate::storage::{InterlinkApprovalRecord, StorageBackend};
use super::digest::{risk_of, short_target};
use std::sync::Arc;

/// Approval timeout when the command carries none (docs §7.3 4).
pub const APPROVAL_TTL_S: f64 = 120.0;

/// Everything needed to mint a ticket for one command.
pub struct ApprovalSpec<'a> {
    pub command_id: &'a str,
    pub device_id: &'a str,
    pub user_id: &'a str,
    pub kind: &'a str,
    pub from_label: &'a str,
    pub args: &'a Value,
    pub timeout_s: Option<f64>,
}

/// Build the ticket. The prompt is what the local modal shows: operation,
/// source node, target and an argument summary - never the argument body.
pub fn build(spec: &ApprovalSpec<'_>, now: f64) -> InterlinkApprovalRecord {
    let ttl = spec
        .timeout_s
        .filter(|value| *value > 0.0)
        .unwrap_or(APPROVAL_TTL_S);
    InterlinkApprovalRecord {
        approval_id: format!("apr_{}", Uuid::new_v4().simple()),
        command_id: spec.command_id.to_string(),
        device_id: spec.device_id.to_string(),
        user_id: spec.user_id.to_string(),
        prompt: prompt_text(spec.kind, spec.from_label, spec.args),
        risk_level: risk_of(spec.kind).to_string(),
        state: APPROVAL_PENDING.to_string(),
        decided_by: None,
        decided_at: None,
        expires_at: now + ttl,
    }
}

pub fn prompt_text(kind: &str, from_label: &str, args: &Value) -> String {
    format!(
        "[{}] {} from {} -> {}",
        command_level(kind),
        kind,
        from_label,
        short_target(kind, args)
    )
}

/// Risk levels that local settings may never auto-approve (docs §7.3 3).
pub fn is_memorizable(kind: &str) -> bool {
    !matches!(risk_of(kind), RISK_HIGH) && command_level(kind) == "L1"
}

/// Persist a decision. Returns the stored ticket so callers can audit it;
/// a ticket that already left `pending` is reported unchanged (first decision
/// wins, docs §7.3 4).
pub async fn decide(
    storage: Arc<dyn StorageBackend>,
    approval_id: &str,
    state: &str,
    decided_by: &str,
    now: f64,
) -> anyhow::Result<Option<InterlinkApprovalRecord>> {
    let lookup = approval_id.to_string();
    let existing = {
        let storage = storage.clone();
        crate::core::blocking::run_db("interlink.approvals.get", move || {
            storage.get_interlink_approval(&lookup)
        })
        .await?
    };
    let Some(ticket) = existing else {
        return Ok(None);
    };
    if ticket.state != APPROVAL_PENDING {
        return Ok(Some(ticket));
    }
    let state = state.to_string();
    let decided_by = decided_by.to_string();
    let approval_id = ticket.approval_id.clone();
    {
        let storage = storage.clone();
        let state = state.clone();
        let decided_by = decided_by.clone();
        crate::core::blocking::run_db("interlink.approvals.decide", move || {
            storage.decide_interlink_approval(&approval_id, &state, &decided_by, now)
        })
        .await?;
    }
    let mut updated = ticket;
    updated.state = state;
    updated.decided_by = Some(decided_by);
    updated.decided_at = Some(now);
    Ok(Some(updated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wunder_core::interlink::{CMD_THREAD_MESSAGE, CMD_TOOL_EXEC, CMD_WORKSPACE_DELETE};

    #[test]
    fn build_sets_pending_state_with_bounded_ttl() {
        let args = json!({"message": "hello there, this is a private message body"});
        let spec = ApprovalSpec {
            command_id: "cmd_1",
            device_id: "dev-1",
            user_id: "u_1",
            kind: CMD_THREAD_MESSAGE,
            from_label: "web:conn",
            args: &args,
            timeout_s: Some(30.0),
        };
        let ticket = build(&spec, 1_000.0);
        assert_eq!(ticket.state, APPROVAL_PENDING);
        assert_eq!(ticket.risk_level, "medium");
        assert_eq!(ticket.expires_at, 1_030.0);
        assert!(ticket.prompt.contains("thread.message"));
        assert!(ticket.prompt.contains("web:conn"));
        assert!(!ticket.prompt.contains("private message body"));
        assert!(ticket.approval_id.starts_with("apr_"));

        // No explicit timeout -> the documented 120s default.
        let default_ticket = build(
            &ApprovalSpec {
                command_id: "cmd_2",
                device_id: "dev-1",
                user_id: "u_1",
                kind: CMD_TOOL_EXEC,
                from_label: "web:conn",
                args: &json!({}),
                timeout_s: None,
            },
            0.0,
        );
        assert_eq!(default_ticket.expires_at, APPROVAL_TTL_S);
        assert_eq!(default_ticket.risk_level, "high");
    }

    #[test]
    fn only_l1_tickets_can_be_remembered() {
        assert!(is_memorizable(CMD_THREAD_MESSAGE));
        assert!(!is_memorizable(CMD_WORKSPACE_DELETE));
        assert!(!is_memorizable(CMD_TOOL_EXEC));
        assert!(!is_memorizable("workspace.read"));
    }
}
