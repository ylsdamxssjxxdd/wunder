use crate::approval::ApprovalMode;
use crate::config::Config;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecPolicyMode {
    Allow,
    Audit,
    Enforce,
}

impl ExecPolicyMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExecPolicyMode::Allow => "allow",
            ExecPolicyMode::Audit => "audit",
            ExecPolicyMode::Enforce => "enforce",
        }
    }

    pub fn from_raw(raw: Option<&str>) -> Self {
        let value = raw.unwrap_or("").trim().to_lowercase();
        match value.as_str() {
            "audit" => ExecPolicyMode::Audit,
            "enforce" => ExecPolicyMode::Enforce,
            _ => ExecPolicyMode::Allow,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecPolicyToolKind {
    Exec,
    Write,
    Control,
}

#[derive(Debug, Clone)]
pub struct ExecPolicyDecision {
    pub allowed: bool,
    pub requires_approval: bool,
    pub mode: ExecPolicyMode,
    pub approval_mode: ApprovalMode,
    pub reason: String,
}

impl ExecPolicyDecision {
    pub fn to_value(&self) -> Value {
        json!({
            "mode": self.mode.as_str(),
            "approval_mode": self.approval_mode.as_str(),
            "allowed": self.allowed,
            "requires_approval": self.requires_approval,
            "reason": self.reason,
        })
    }
}

pub fn evaluate_tool_policy(
    config: &Config,
    tool_kind: ExecPolicyToolKind,
    command: &str,
    approved: bool,
) -> Option<ExecPolicyDecision> {
    let mode = ExecPolicyMode::from_raw(config.security.exec_policy_mode.as_deref());
    let approval_mode = ApprovalMode::from_raw(config.security.approval_mode.as_deref());

    let mut requires_approval = false;
    let mut allowed = true;
    let mut reason = String::new();

    if matches!(tool_kind, ExecPolicyToolKind::Write)
        && matches!(approval_mode, ApprovalMode::Suggest)
    {
        requires_approval = !approved;
        allowed = approved;
        reason = "write_requires_approval".to_string();
    }

    if matches!(tool_kind, ExecPolicyToolKind::Exec) {
        if matches!(
            approval_mode,
            ApprovalMode::Suggest | ApprovalMode::AutoEdit
        ) && !approved
        {
            requires_approval = true;
            allowed = false;
            if reason.is_empty() {
                reason = "exec_requires_approval".to_string();
            }
        }
        if is_high_risk_command(command) {
            let high_risk_requires = !approved && !matches!(mode, ExecPolicyMode::Allow);
            let high_risk_allowed = approved || !matches!(mode, ExecPolicyMode::Enforce);
            requires_approval = requires_approval || high_risk_requires;
            allowed = allowed && high_risk_allowed;
            if reason.is_empty() && (high_risk_requires || !high_risk_allowed) {
                reason = "high_risk_command".to_string();
            }
        }
    }

    if matches!(tool_kind, ExecPolicyToolKind::Control)
        && matches!(
            approval_mode,
            ApprovalMode::Suggest | ApprovalMode::AutoEdit
        )
        && !approved
    {
        requires_approval = true;
        allowed = false;
        if reason.is_empty() {
            reason = "control_requires_approval".to_string();
        }
    }

    if !requires_approval && allowed {
        return None;
    }
    if reason.is_empty() {
        reason = "approval_required".to_string();
    }
    Some(ExecPolicyDecision {
        allowed,
        requires_approval,
        mode,
        approval_mode,
        reason,
    })
}

pub fn build_approval_signature(
    tool_kind: ExecPolicyToolKind,
    tool_name: &str,
    args: &Value,
    command: &str,
) -> String {
    match tool_kind {
        ExecPolicyToolKind::Exec => {
            let command = command.trim();
            if command.is_empty() {
                format!("tool:{tool_name}")
            } else {
                command.to_string()
            }
        }
        ExecPolicyToolKind::Write => build_write_signature(tool_name, args),
        ExecPolicyToolKind::Control => build_control_signature(tool_name, args),
    }
}

pub fn build_write_signature(tool_name: &str, args: &Value) -> String {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("");
    if !path.is_empty() {
        return format!("{tool_name}:{path}");
    }
    let compact = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
    if compact.len() > 512 {
        format!("{tool_name}:{}", truncate_signature_text(&compact, 512))
    } else {
        format!("{tool_name}:{compact}")
    }
}

pub fn build_control_signature(tool_name: &str, args: &Value) -> String {
    let mut parts = vec![tool_name.to_string()];
    if let Some(action) = args
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("action={action}"));
    }
    if let Some(desc) = args
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("desc={desc}"));
    }
    if let Some(wait_ms) = args.get("wait_ms").and_then(Value::as_i64) {
        parts.push(format!("wait_ms={wait_ms}"));
    }
    let signature = parts.join("|");
    if signature.len() > 512 {
        truncate_signature_text(&signature, 512)
    } else {
        signature
    }
}

fn truncate_signature_text(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    if end == 0 {
        String::new()
    } else {
        text[..end].to_string()
    }
}

pub fn extract_command_text(args: &Value) -> Option<String> {
    let obj = args.as_object()?;
    for key in ["content", "command", "cmd"] {
        if let Some(Value::String(text)) = obj.get(key) {
            let cleaned = text.trim();
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        }
    }
    None
}

pub fn is_high_risk_command(command: &str) -> bool {
    let lower = command.to_lowercase();
    let patterns = [
        " rm ",
        " rm-",
        "rm -",
        "rm -rf",
        "del ",
        "rmdir",
        "mkfs",
        "dd ",
        "shutdown",
        "reboot",
        "poweroff",
        "kill -9",
        "chmod 777",
        "chown ",
    ];
    patterns.iter().any(|pattern| lower.contains(pattern))
}

pub fn resolve_session_key(session_id: Option<&str>, user_id: Option<&str>) -> Option<String> {
    let session = session_id.unwrap_or("").trim();
    if !session.is_empty() {
        return Some(session.to_string());
    }
    let user = user_id.unwrap_or("").trim();
    if !user.is_empty() {
        return Some(user.to_string());
    }
    None
}

pub fn extract_approval_flag(args: &Value) -> bool {
    let Some(obj) = args.as_object() else {
        return false;
    };
    for key in ["approved", "approval"] {
        match obj.get(key) {
            Some(Value::Bool(true)) => return true,
            Some(Value::String(text)) if text.trim().eq_ignore_ascii_case("true") => return true,
            _ => {}
        }
    }
    false
}

pub fn extract_approval_token(args: &Value) -> Option<String> {
    let obj = args.as_object()?;
    for key in ["approval_key", "approval_token", "approval_id"] {
        if let Some(value) = obj.get(key) {
            let text = match value {
                Value::String(text) => text.trim().to_string(),
                Value::Number(num) => num.to_string(),
                _ => continue,
            };
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn exec_policy_requires_approval_in_enforce_mode() {
        let mut config = Config::default();
        config.security.exec_policy_mode = Some("enforce".to_string());
        let decision =
            evaluate_tool_policy(&config, ExecPolicyToolKind::Exec, "rm -rf /tmp/demo", false)
                .expect("decision");
        assert!(!decision.allowed);
        assert!(decision.requires_approval);
        assert_eq!(decision.approval_mode, ApprovalMode::FullAuto);
    }

    #[test]
    fn suggest_mode_blocks_write_file() {
        let mut config = Config::default();
        config.security.approval_mode = Some("suggest".to_string());
        let decision =
            evaluate_tool_policy(&config, ExecPolicyToolKind::Write, "", false).expect("decision");
        assert!(!decision.allowed);
        assert!(decision.requires_approval);
        assert_eq!(decision.reason, "write_requires_approval");
        assert_eq!(decision.approval_mode, ApprovalMode::Suggest);
    }

    #[test]
    fn auto_edit_mode_blocks_exec_without_approval() {
        let mut config = Config::default();
        config.security.approval_mode = Some("auto_edit".to_string());
        let decision = evaluate_tool_policy(&config, ExecPolicyToolKind::Exec, "echo ok", false)
            .expect("decision");
        assert!(!decision.allowed);
        assert!(decision.requires_approval);
        assert_eq!(decision.reason, "exec_requires_approval");
        assert_eq!(decision.approval_mode, ApprovalMode::AutoEdit);
    }

    #[test]
    fn auto_edit_mode_allows_write() {
        let mut config = Config::default();
        config.security.approval_mode = Some("auto_edit".to_string());
        assert!(evaluate_tool_policy(&config, ExecPolicyToolKind::Write, "", false).is_none());
    }

    #[test]
    fn audit_mode_allows_high_risk_exec_but_requires_approval() {
        let mut config = Config::default();
        config.security.exec_policy_mode = Some("audit".to_string());

        let decision =
            evaluate_tool_policy(&config, ExecPolicyToolKind::Exec, "rm -rf ./target", false)
                .expect("decision");

        assert!(decision.allowed);
        assert!(decision.requires_approval);
        assert_eq!(decision.reason, "high_risk_command");
        assert_eq!(decision.mode, ExecPolicyMode::Audit);
    }

    #[test]
    fn approved_high_risk_exec_is_not_blocked_in_enforce_mode() {
        let mut config = Config::default();
        config.security.exec_policy_mode = Some("enforce".to_string());

        let decision =
            evaluate_tool_policy(&config, ExecPolicyToolKind::Exec, "rm -rf ./target", true);

        assert!(decision.is_none());
    }

    #[test]
    fn build_approval_signature_uses_tool_name_for_blank_exec_command() {
        assert_eq!(
            build_approval_signature(ExecPolicyToolKind::Exec, "execute_command", &json!({}), " "),
            "tool:execute_command"
        );
    }

    #[test]
    fn extract_approval_token_accepts_numeric_ids() {
        assert_eq!(
            extract_approval_token(&json!({ "approval_id": 42 })),
            Some("42".to_string())
        );
    }

    #[test]
    fn build_write_signature_truncates_on_char_boundary() {
        let repeated = "局".repeat(300);
        let args = json!({
            "input": format!("*** Begin Patch\n*** Update File: demo.txt\n+{repeated}\n*** End Patch")
        });
        let signature = build_write_signature("apply_patch", &args);
        assert!(signature.starts_with("apply_patch:"));
        assert!(std::str::from_utf8(signature.as_bytes()).is_ok());
    }
}
