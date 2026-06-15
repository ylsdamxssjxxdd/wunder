use crate::config::Config;
use crate::tools::resolve_tool_name;
use dashmap::DashMap;
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use wunder_core::exec_policy::{
    build_approval_signature, evaluate_tool_policy, extract_approval_flag, extract_approval_token,
    extract_command_text, resolve_session_key,
};
pub use wunder_core::exec_policy::{
    build_write_signature, ExecPolicyDecision, ExecPolicyMode, ExecPolicyToolKind,
};

const APPROVAL_CACHE_TTL_S: i64 = 600;

pub fn evaluate_tool_call(
    config: &Config,
    tool_name: &str,
    args: &Value,
    session_id: Option<&str>,
    user_id: Option<&str>,
) -> Option<ExecPolicyDecision> {
    let tool_kind = resolve_exec_policy_tool_kind(tool_name)?;
    let command = if matches!(tool_kind, ExecPolicyToolKind::Exec) {
        extract_command_text(args).unwrap_or_default()
    } else {
        String::new()
    };
    let approval_signature = build_approval_signature(tool_kind, tool_name, args, &command);

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

    evaluate_tool_policy(config, tool_kind, &command, approved)
}

fn resolve_exec_policy_tool_kind(tool_name: &str) -> Option<ExecPolicyToolKind> {
    let exec_tool_name = resolve_tool_name("execute_command");
    let ptc_tool_name = resolve_tool_name("ptc");
    let write_tool_name = resolve_tool_name("write_file");
    let edit_tool_name = resolve_tool_name("edit_file2");
    let patch_tool_name = resolve_tool_name("apply_patch");
    let controller_tool_name = resolve_tool_name("desktop_controller");
    let monitor_tool_name = resolve_tool_name("desktop_monitor");

    if tool_name == exec_tool_name || tool_name == ptc_tool_name {
        Some(ExecPolicyToolKind::Exec)
    } else if tool_name == write_tool_name
        || tool_name == edit_tool_name
        || tool_name == patch_tool_name
    {
        Some(ExecPolicyToolKind::Write)
    } else if tool_name == controller_tool_name || tool_name == monitor_tool_name {
        Some(ExecPolicyToolKind::Control)
    } else {
        None
    }
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
    use crate::core::approval::ApprovalMode;
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
        );
        assert!(decision.is_none());

        let args_repeat = json!({ "content": "rm -rf /tmp/cache" });
        let decision_repeat = evaluate_tool_call(
            &config,
            &tool_name,
            &args_repeat,
            Some("session_b"),
            Some("user_b"),
        );
        assert!(decision_repeat.is_none());

        let decision_other_session = evaluate_tool_call(
            &config,
            &tool_name,
            &args_repeat,
            Some("session_other"),
            Some("user_b"),
        )
        .expect("decision");
        assert!(!decision_other_session.allowed);
        assert!(decision_other_session.requires_approval);
    }

    #[test]
    fn test_suggest_mode_blocks_write_file() {
        let mut config = Config::default();
        config.security.approval_mode = Some("suggest".to_string());
        let tool_name = resolve_tool_name("write_file");
        let args = json!({ "path": "src/main.rs", "content": "fn main(){}" });
        let decision = evaluate_tool_call(
            &config,
            &tool_name,
            &args,
            Some("session_c"),
            Some("user_c"),
        )
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
        let decision = evaluate_tool_call(
            &config,
            &tool_name,
            &args,
            Some("session_d"),
            Some("user_d"),
        )
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
        let tool_name = resolve_tool_name("apply_patch");
        let args = json!({
            "input": "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-// old\n+// hi\n*** End Patch"
        });
        assert!(evaluate_tool_call(&config, &tool_name, &args, None, None).is_none());
    }

    #[test]
    fn test_build_write_signature_truncates_on_char_boundary() {
        let tool_name = resolve_tool_name("apply_patch");
        let repeated = "局".repeat(300);
        let args = json!({
            "input": format!("*** Begin Patch\n*** Update File: demo.txt\n+{repeated}\n*** End Patch")
        });
        let signature = build_write_signature(&tool_name, &args);
        assert!(signature.starts_with(&(tool_name + ":")));
        assert!(std::str::from_utf8(signature.as_bytes()).is_ok());
    }
}
