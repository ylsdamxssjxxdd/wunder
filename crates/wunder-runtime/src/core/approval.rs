use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

pub use wunder_core::approval::{ApprovalMode, ApprovalRequestKind, ApprovalResponse};

pub struct ApprovalRequest {
    pub id: String,
    pub kind: ApprovalRequestKind,
    pub tool: String,
    pub args: Value,
    pub summary: String,
    pub detail: Value,
    pub respond_to: oneshot::Sender<ApprovalResponse>,
}

pub type ApprovalRequestTx = mpsc::UnboundedSender<ApprovalRequest>;
#[allow(dead_code)]
pub type ApprovalRequestRx = mpsc::UnboundedReceiver<ApprovalRequest>;

#[allow(dead_code)]
pub fn new_channel() -> (ApprovalRequestTx, ApprovalRequestRx) {
    mpsc::unbounded_channel()
}
