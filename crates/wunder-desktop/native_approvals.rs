//! Pending tool approvals of one native desktop turn.
//!
//! The runtime gate asks through an [`ApprovalRequest`] channel owned by the
//! turn. The Slint window answers later, so the response handle is parked here
//! under its approval id. Dropping the handle is the engine's denial signal:
//! the turn's exit path forgets everything it registered.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::oneshot;
use wunder_server::approval::{ApprovalRequest, ApprovalResponse};

#[derive(Default)]
pub(crate) struct NativeApprovals {
    pending: Mutex<HashMap<String, PendingApproval>>,
}

struct PendingApproval {
    session_id: String,
    respond_to: oneshot::Sender<ApprovalResponse>,
}

impl NativeApprovals {
    /// Park one gate request and return the frame the transcript renders.
    ///
    /// A request that cannot be parked (poisoned lock, empty id) is denied by
    /// dropping it, which is exactly what the gate reads as "no answer".
    pub(crate) fn register(&self, session_id: &str, request: ApprovalRequest) -> Option<Value> {
        let approval_id = request.id.trim().to_string();
        if approval_id.is_empty() {
            return None;
        }
        let payload = json!({
            "approval_id": approval_id,
            "kind": request.kind,
            "tool": request.tool,
            "summary": request.summary,
            "detail": request.detail,
            "session_id": session_id,
        });
        let mut pending = self.pending.lock().ok()?;
        let previous = pending.insert(
            approval_id,
            PendingApproval {
                session_id: session_id.to_string(),
                respond_to: request.respond_to,
            },
        );
        if let Some(previous) = previous {
            // Ids are freshly minted UUIDs, so this is defensive: a collision
            // must never leave two waits behind one card.
            let _ = previous.respond_to.send(ApprovalResponse::Deny);
        }
        Some(payload)
    }

    /// Resolve one parked request. The caller answers for a specific thread so
    /// a stale card in another split pane cannot approve this one.
    pub(crate) fn decide(
        &self,
        session_id: &str,
        approval_id: &str,
        response: ApprovalResponse,
    ) -> Result<()> {
        let session_id = session_id.trim();
        let approval_id = approval_id.trim();
        anyhow::ensure!(!approval_id.is_empty(), "审批标识为空");
        let entry = {
            let mut pending = self
                .pending
                .lock()
                .map_err(|_| anyhow!("审批状态暂不可用"))?;
            match pending.get(approval_id) {
                Some(entry) if entry.session_id == session_id => pending.remove(approval_id),
                Some(_) => return Err(anyhow!("该审批不属于当前线程")),
                None => return Err(anyhow!("该审批已失效")),
            }
        };
        let Some(entry) = entry else {
            return Err(anyhow!("该审批已失效"));
        };
        entry
            .respond_to
            .send(response)
            .map_err(|_| anyhow!("该审批已失效"))
    }

    /// Drop the handles a finished turn still owns. The gate sees the closed
    /// channel and records a denial instead of waiting forever.
    pub(crate) fn forget(&self, approval_ids: &[String]) {
        let Ok(mut pending) = self.pending.lock() else {
            return;
        };
        for approval_id in approval_ids {
            pending.remove(approval_id);
        }
    }

    #[cfg(test)]
    fn pending_count(&self) -> usize {
        self.pending
            .lock()
            .map(|pending| pending.len())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wunder_server::approval::ApprovalRequestKind;

    fn request(id: &str) -> (ApprovalRequest, oneshot::Receiver<ApprovalResponse>) {
        let (respond_to, receiver) = oneshot::channel();
        (
            ApprovalRequest {
                id: id.to_string(),
                kind: ApprovalRequestKind::Exec,
                tool: "execute_command".to_string(),
                args: json!({"command": "echo ok"}),
                summary: "execute_command: echo ok".to_string(),
                detail: json!({"reason": "exec_requires_approval"}),
                respond_to,
            },
            receiver,
        )
    }

    #[test]
    fn decide_delivers_the_answer_and_consumes_the_pending_entry() {
        let hub = NativeApprovals::default();
        let (request, mut receiver) = request("approval-1");
        let frame = hub.register("session-1", request).expect("approval parked");
        assert_eq!(frame["approval_id"], "approval-1");
        assert_eq!(frame["kind"], "exec");
        assert_eq!(frame["session_id"], "session-1");
        assert_eq!(hub.pending_count(), 1);

        hub.decide("session-1", "approval-1", ApprovalResponse::ApproveOnce)
            .expect("decision accepted");
        assert_eq!(receiver.try_recv(), Ok(ApprovalResponse::ApproveOnce));
        assert_eq!(hub.pending_count(), 0);
        assert!(hub
            .decide("session-1", "approval-1", ApprovalResponse::Deny)
            .is_err());
    }

    #[test]
    fn a_decision_from_another_thread_is_refused() {
        let hub = NativeApprovals::default();
        let (request, mut receiver) = request("approval-2");
        hub.register("session-1", request).expect("parked");

        assert!(hub
            .decide("session-2", "approval-2", ApprovalResponse::ApproveOnce)
            .is_err());
        assert_eq!(hub.pending_count(), 1);
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn forgetting_a_turn_drops_its_handle_and_denies_the_wait() {
        let hub = NativeApprovals::default();
        let (request, mut receiver) = request("approval-3");
        hub.register("session-1", request).expect("parked");

        hub.forget(&["approval-3".to_string()]);
        assert_eq!(hub.pending_count(), 0);
        assert!(receiver.try_recv().is_err());
    }
}
