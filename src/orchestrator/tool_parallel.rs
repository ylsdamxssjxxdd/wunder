use crate::tools::resolve_tool_name;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::OnceLock;

pub(super) fn tool_call_supports_parallel(tool_name: &str, args: &Value) -> bool {
    let canonical = resolve_tool_name(tool_name.trim());
    if canonical.trim().is_empty() {
        return false;
    }
    if is_memory_manager_tool_name(&canonical) {
        return memory_manager_action_is_read_only(args);
    }
    if canonical.contains('@') {
        return external_tool_looks_read_only(&canonical);
    }
    parallel_safe_builtin_names().contains(&canonical)
}

fn parallel_safe_builtin_names() -> &'static HashSet<String> {
    static NAMES: OnceLock<HashSet<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        [
            "read_file",
            "list_files",
            "search_content",
            "read_image",
            "web_fetch",
            "browser",
            "self_status",
            "sleep",
            "a2a_observe",
            "a2a_wait",
        ]
        .into_iter()
        .map(resolve_tool_name)
        .filter(|name| !name.trim().is_empty())
        .collect()
    })
}

fn is_memory_manager_tool_name(name: &str) -> bool {
    let canonical = resolve_tool_name("memory_manager");
    name == canonical || name.eq_ignore_ascii_case("memory_manager")
}

fn memory_manager_action_is_read_only(args: &Value) -> bool {
    let normalized = crate::core::tool_args::recover_tool_args_value(args);
    let action = normalized
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    matches!(action.as_str(), "list" | "get" | "search" | "recall")
}

fn external_tool_looks_read_only(canonical: &str) -> bool {
    let Some((namespace, tool_part)) = canonical.split_once('@') else {
        return false;
    };
    if namespace.eq_ignore_ascii_case("a2a") {
        return false;
    }
    let lower = tool_part.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return false;
    }
    let mutating_hints = [
        "write", "edit", "patch", "update", "delete", "remove", "create", "insert", "upsert",
        "set", "send", "publish", "post", "put", "exec", "run",
    ];
    if mutating_hints.iter().any(|hint| lower.contains(hint)) {
        return false;
    }
    let readonly_hints = [
        "read", "list", "search", "find", "query", "fetch", "get", "show", "describe", "status",
    ];
    readonly_hints.iter().any(|hint| lower.contains(hint))
}

#[cfg(test)]
mod tests {
    use super::tool_call_supports_parallel;
    use serde_json::json;

    #[test]
    fn read_file_supports_parallel_execution() {
        assert!(tool_call_supports_parallel(
            "read_file",
            &json!({ "path": "src/main.rs" })
        ));
    }

    #[test]
    fn apply_patch_is_forced_to_exclusive_execution() {
        assert!(!tool_call_supports_parallel(
            "apply_patch",
            &json!({ "patch": "*** Begin Patch" })
        ));
    }

    #[test]
    fn memory_recall_is_parallel_but_memory_write_is_not() {
        assert!(tool_call_supports_parallel(
            "memory_manager",
            &json!({ "action": "search", "query": "Alice" }),
        ));
        assert!(tool_call_supports_parallel(
            "memory_manager",
            &json!({ "action": "get", "memory_id": "mem_1" }),
        ));
        assert!(!tool_call_supports_parallel(
            "memory_manager",
            &json!({ "action": "add", "title": "x", "content": "y" }),
        ));
    }

    #[test]
    fn read_like_external_tool_name_can_run_in_parallel() {
        assert!(tool_call_supports_parallel(
            "extra_mcp@db_query",
            &json!({ "sql": "select 1" }),
        ));
        assert!(!tool_call_supports_parallel(
            "extra_mcp@db_update",
            &json!({ "sql": "update t set a=1" }),
        ));
    }
}
