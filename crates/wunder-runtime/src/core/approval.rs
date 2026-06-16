use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tracing::warn;

pub use wunder_core::approval::{ApprovalMode, ApprovalRequestKind, ApprovalResponse};

const APPROVAL_QUEUE_CAPACITY: usize = 32;

pub struct ApprovalRequest {
    pub id: String,
    pub kind: ApprovalRequestKind,
    pub tool: String,
    pub args: Value,
    pub summary: String,
    pub detail: Value,
    pub respond_to: oneshot::Sender<ApprovalResponse>,
}

#[derive(Clone, Debug)]
pub struct ApprovalRequestTx {
    sender: mpsc::Sender<ApprovalRequest>,
}

impl ApprovalRequestTx {
    pub fn send(
        &self,
        request: ApprovalRequest,
    ) -> Result<(), mpsc::error::TrySendError<ApprovalRequest>> {
        match self.sender.try_send(request) {
            Ok(()) => Ok(()),
            Err(err) => {
                warn!(
                    queue.name = "approval.request",
                    "approval request queue rejected item: {err}"
                );
                Err(err)
            }
        }
    }
}

#[allow(dead_code)]
pub type ApprovalRequestRx = mpsc::Receiver<ApprovalRequest>;

#[allow(dead_code)]
pub fn new_channel() -> (ApprovalRequestTx, ApprovalRequestRx) {
    let (sender, receiver) = mpsc::channel(APPROVAL_QUEUE_CAPACITY);
    (ApprovalRequestTx { sender }, receiver)
}
