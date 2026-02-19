use crate::approval::ApprovalMode;
use crate::config::Config;
use crate::tools::resolve_tool_name;
use dashmap::DashMap;
use serde_json::{json, Value};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecPolicyMode {
    Allow,
    Audit,
    Enforce,
}

const APPROVAL_CACHE_TTL_S: i64 = 600;

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

pub fn evaluate_tool_call(
    config: &Config,
    tool_name: &str,
    args: &Value,
    session_id: Option<&str>,
    user_id: Option<&str>,
) -> Option<ExecPolicyDecision> {
    let mode = ExecPolicyMode::from_raw(config.security.exec_policy_mode.as_deref());
    let approval_mode = ApprovalMode::from_raw(config.security.approval_mode.as_deref());

    let exec_tool_name = resolve_tool_name("execute_command");
    let ptc_tool_name = resolve_tool_name("ptc");
    let write_tool_name = resolve_tool_name("write_file");
    let edit_tool_name = resolve_tool_name("edit_file");
    let replace_tool_name = resolve_tool_name("replace_text");
    let is_exec_tool = tool_name == exec_tool_name || tool_name == ptc_tool_name;
    let is_write_tool =
        tool_name == write_tool_name || tool_name == edit_tool_name || tool_name == replace_tool_name;
    if !is_exec_tool && !is_write_tool {
        return None;
    }

    let command = if is_exec_tool {
        extract_command_text(args).unwrap_or_default()
    } else {
        String::new()
    };
    let approval_signature = if is_exec_tool {
        if command.is_empty() {
            format!("tool:{tool_name}")
        } else {
            command.clone()
        }
    } else {
        build_write_signature(tool_name, args)
    };

    let session_key = resolve_session_key(session_id, user_id);
    let approval_flag = extract_approval_flag(args);
    let approval_token = extract_approval_token(args);
    let mut approved = approval_flag || approval_token.is_some();
    if let Some(session_key) = session_key.as_deref() {
        if approved {
            remember_approval(session_key, tool_name, &approval_signature);
        } else if is_approval_cached(session_key, tool_name, &approval_signature) {
            approved = true;
        }
    }

    let mut requires_approval = false;
    let mut allowed = true;
    let mut reason = String::new();

    if is_write_tool && matches!(approval_mode, ApprovalMode::Suggest) {
        requires_approval = !approved;
        allowed = approved;
        reason = "write_requires_approval".to_string();
    }

    if is_exec_tool {
        if matches!(approval_mode, ApprovalMode::Suggest | ApprovalMode::AutoEdit) && !approved {
            requires_approval = true;
            allowed = false;
            if reason.is_empty() {
                reason = "exec_requires_approval".to_string();
            }
        }
        if is_high_risk_command(&command) {
            let high_risk_requires = !approved && !matches!(mode, ExecPolicyMode::Allow);
            let high_risk_allowed = approved || !matches!(mode, ExecPolicyMode::Enforce);
            requires_approval = requires_approval || high_risk_requires;
            allowed = allowed && high_risk_allowed;
            if reason.is_empty() && (high_risk_requires || !high_risk_allowed) {
                reason = "high_risk_command".to_string();
            }
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

fn build_write_signature(tool_name: &str, args: &Value) -> String {
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
        format!("{tool_name}:{}", &compact[..512])
    } else {
        format!("{tool_name}:{compact}")
    }
}

fn extract_command_text(args: &Value) -> Option<String> {
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

fn is_high_risk_command(command: &str) -> bool {
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

fn resolve_session_key(session_id: Option<&str>, user_id: Option<&str>) -> Option<String> {
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

fn extract_approval_flag(args: &Value) -> bool {
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

fn extract_approval_token(args: &Value) -> Option<String> {
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

fn approval_cache() -> &'static DashMap<String, i64> {
    static CACHE: OnceLock<DashMap<String, i64>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn build_approval_cache_key(session_key: &str, tool_name: &str, command: &str) -> String {
    let tool = tool_name.trim();
    let hash = hash_command(command);
    if tool.is_empty() {
        format!("{session_key}:{hash}")
    } else {
        format!("{session_key}:{tool}:{hash}")
    }
}

fn is_approval_cached(session_key: &str, tool_name: &str, command: &str) -> bool {
    if APPROVAL_CACHE_TTL_S <= 0 {
        return false;
    }
    let cache_key = build_approval_cache_key(session_key, tool_name, command);
    let now = now_ts();
    if let Some(entry) = approval_cache().get(&cache_key) {
        if *entry > now {
            return true;
        }
    }
    approval_cache().remove(&cache_key);
    false
}

fn remember_approval(session_key: &str, tool_name: &str, command: &str) {
    if APPROVAL_CACHE_TTL_S <= 0 {
        return;
    }
    let cache_key = build_approval_cache_key(session_key, tool_name, command);
    let expires_at = now_ts().saturating_add(APPROVAL_CACHE_TTL_S.max(0));
    approval_cache().insert(cache_key, expires_at);
}

fn hash_command(command: &str) -> String {
    let mut hasher = DefaultHasher::new();
    command.trim().hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_exec_policy_requires_approval_in_enforce_mode() {
        let mut config = Config::default();
        config.security.exec_policy_mode = Some("enforce".to_string());
        let tool_name = resolve_tool_name("execute_command");
        let args = json!({ "content": "rm -rf /tmp/demo" });
        let decision = evaluate_tool_call(
            &config,
            &tool_name,
            &args,
            Some("session_a"),
            Some("user_a"),
        )
        .expect("decision");
        assert!(!decision.allowed);
        assert!(decision.requires_approval);
        assert_eq!(decision.approval_mode, ApprovalMode::FullAuto);
    }

    #[test]
    fn test_exec_policy_approval_cache_allows_repeat() {
        let mut config = Config::default();
        config.security.exec_policy_mode = Some("enforce".to_string());
        let tool_name = resolve_tool_name("execute_command");
        let args = json!({ "content": "rm -rf /tmp/cache", "approved": true });
        let decision = evaluate_tool_call(
            &config,
            &tool_name,
            &args,
            Some("session_b"),
            Some("user_b"),
        )
        .expect("decision");
        assert!(decision.allowed);
        assert!(!decision.requires_approval);

        let args_repeat = json!({ "content": "rm -rf /tmp/cache" });
        let decision_repeat = evaluate_tool_call(
            &config,
            &tool_name,
            &args_repeat,
            Some("session_b"),
            Some("user_b"),
        )
        .expect("decision");
        assert!(decision_repeat.allowed);
        assert!(!decision_repeat.requires_approval);
    }

    #[test]
    fn test_suggest_mode_blocks_write_file() {
        let mut config = Config::default();
        config.security.approval_mode = Some("suggest".to_string());
        let tool_name = resolve_tool_name("write_file");
        let args = json!({ "path": "src/main.rs", "content": "fn main(){}" });
        let decision =
            evaluate_tool_call(&config, &tool_name, &args, Some("session_c"), Some("user_c"))
                .expect("decision");
        assert!(!decision.allowed);
        assert!(decision.requires_approval);
        assert_eq!(decision.reason, "write_requires_approval");
        assert_eq!(decision.approval_mode, ApprovalMode::Suggest);
    }

    #[test]
    fn test_auto_edit_mode_blocks_exec_without_approval() {
        let mut config = Config::default();
        config.security.approval_mode = Some("auto_edit".to_string());
        let tool_name = resolve_tool_name("execute_command");
        let args = json!({ "content": "echo hello" });
        let decision =
            evaluate_tool_call(&config, &tool_name, &args, Some("session_d"), Some("user_d"))
                .expect("decision");
        assert!(!decision.allowed);
        assert!(decision.requires_approval);
        assert_eq!(decision.reason, "exec_requires_approval");
        assert_eq!(decision.approval_mode, ApprovalMode::AutoEdit);
    }

    #[test]
    fn test_auto_edit_mode_allows_write() {
        let mut config = Config::default();
        config.security.approval_mode = Some("auto_edit".to_string());
        let tool_name = resolve_tool_name("edit_file");
        let args = json!({
            "path": "src/lib.rs",
            "edits": [{ "action": "replace", "start_line": 1, "end_line": 1, "new_content": "// hi" }]
        });
        assert!(evaluate_tool_call(&config, &tool_name, &args, None, None).is_none());
    }
}
