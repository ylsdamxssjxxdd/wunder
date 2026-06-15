use serde::{Deserialize, Serialize};

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
    Control,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_mode_parses_aliases() {
        assert_eq!(
            ApprovalMode::from_raw(Some("auto-edit")),
            ApprovalMode::AutoEdit
        );
        assert_eq!(ApprovalMode::from_raw(Some("suggest")).as_str(), "suggest");
        assert_eq!(ApprovalMode::from_raw(None), ApprovalMode::FullAuto);
    }
}
