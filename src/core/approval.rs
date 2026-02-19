use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    Suggest,
    AutoEdit,
    FullAuto,
}

impl ApprovalMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            ApprovalMode::Suggest => "suggest",
            ApprovalMode::AutoEdit => "auto_edit",
            ApprovalMode::FullAuto => "full_auto",
        }
    }

    pub fn from_raw(raw: Option<&str>) -> Self {
        let value = raw.unwrap_or("").trim().to_ascii_lowercase();
        match value.as_str() {
            "suggest" => ApprovalMode::Suggest,
            "auto_edit" | "auto-edit" => ApprovalMode::AutoEdit,
            _ => ApprovalMode::FullAuto,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalResponse {
    ApproveOnce,
    ApproveSession,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalRequestKind {
    Exec,
    Patch,
}

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
pub type ApprovalRequestRx = mpsc::UnboundedReceiver<ApprovalRequest>;

pub fn new_channel() -> (ApprovalRequestTx, ApprovalRequestRx) {
    mpsc::unbounded_channel()
}

