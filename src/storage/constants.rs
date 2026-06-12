pub(crate) const TOOL_LOG_SKILL_READ_MARKER: &str = "\"source\":\"skill_read\"";

pub(crate) const TOOL_LOG_EXCLUDED_NAMES: &[&str] = &[
    "final_response",
    "最终回复",
    "update_plan",
    "sessions_yield",
    "yield",
    "会话让出",
    "计划面板",
    "question_panel",
    "ask_panel",
    "问询面板",
    "a2ui",
    "a2a_observe",
    "a2a_wait",
    "a2a观察",
    "a2a等待",
    "performance_log",
];

pub const USER_PRIVATE_CONTAINER_ID: i32 = 0;
pub const DEFAULT_SANDBOX_CONTAINER_ID: i32 = 1;
pub const MIN_SANDBOX_CONTAINER_ID: i32 = 1;
pub const MAX_SANDBOX_CONTAINER_ID: i32 = 10;
pub const DEFAULT_HIVE_ID: &str = "default";

pub fn normalize_sandbox_container_id(value: i32) -> i32 {
    value.clamp(MIN_SANDBOX_CONTAINER_ID, MAX_SANDBOX_CONTAINER_ID)
}

pub fn normalize_workspace_container_id(value: i32) -> i32 {
    value.clamp(USER_PRIVATE_CONTAINER_ID, MAX_SANDBOX_CONTAINER_ID)
}

pub fn normalize_hive_id(value: &str) -> String {
    let cleaned = value.trim();
    if cleaned.is_empty() {
        return DEFAULT_HIVE_ID.to_string();
    }
    let mut output = String::with_capacity(cleaned.len());
    for ch in cleaned.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            output.push(ch.to_ascii_lowercase());
        }
    }
    if output.is_empty() {
        DEFAULT_HIVE_ID.to_string()
    } else {
        output
    }
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_hive_id, normalize_sandbox_container_id, normalize_workspace_container_id,
        DEFAULT_HIVE_ID, DEFAULT_SANDBOX_CONTAINER_ID, MAX_SANDBOX_CONTAINER_ID,
        MIN_SANDBOX_CONTAINER_ID, USER_PRIVATE_CONTAINER_ID,
    };

    #[test]
    fn normalize_sandbox_container_id_clamps_to_range() {
        assert_eq!(
            normalize_sandbox_container_id(MIN_SANDBOX_CONTAINER_ID - 1),
            MIN_SANDBOX_CONTAINER_ID
        );
        assert_eq!(
            normalize_sandbox_container_id(MAX_SANDBOX_CONTAINER_ID + 1),
            MAX_SANDBOX_CONTAINER_ID
        );
    }

    #[test]
    fn normalize_sandbox_container_id_keeps_default_in_range() {
        assert_eq!(
            normalize_sandbox_container_id(DEFAULT_SANDBOX_CONTAINER_ID),
            DEFAULT_SANDBOX_CONTAINER_ID
        );
    }

    #[test]
    fn normalize_workspace_container_id_allows_user_private_container() {
        assert_eq!(
            normalize_workspace_container_id(USER_PRIVATE_CONTAINER_ID),
            USER_PRIVATE_CONTAINER_ID
        );
        assert_eq!(
            normalize_workspace_container_id(USER_PRIVATE_CONTAINER_ID - 1),
            USER_PRIVATE_CONTAINER_ID
        );
        assert_eq!(
            normalize_workspace_container_id(MAX_SANDBOX_CONTAINER_ID + 1),
            MAX_SANDBOX_CONTAINER_ID
        );
    }

    #[test]
    fn normalize_hive_id_falls_back_to_default_when_empty_or_invalid() {
        assert_eq!(normalize_hive_id(""), DEFAULT_HIVE_ID);
        assert_eq!(normalize_hive_id("   "), DEFAULT_HIVE_ID);
        assert_eq!(normalize_hive_id("@@@"), DEFAULT_HIVE_ID);
    }

    #[test]
    fn normalize_hive_id_keeps_safe_characters_and_lowercases() {
        assert_eq!(normalize_hive_id("Hive_A-01"), "hive_a-01");
        assert_eq!(normalize_hive_id(" hive-Main "), "hive-main");
    }
}
