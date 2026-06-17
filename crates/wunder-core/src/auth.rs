pub fn is_admin_path(path: &str) -> bool {
    if path == "/.well-known/agent-card.json" {
        return false;
    }
    if path.starts_with("/a2a") {
        return true;
    }
    if !path.starts_with("/wunder") {
        return false;
    }
    if path == "/wunder/ppt-en" || path_is_prefix_or_child(path, "/wunder/ppt") {
        return false;
    }
    if path_is_prefix_or_child(path, "/wunder/i18n") {
        return false;
    }
    if path_is_prefix_or_child(path, "/wunder/doc2md") {
        return false;
    }
    if path_is_prefix_or_child(path, "/wunder/temp_dir") {
        return false;
    }
    if path_is_prefix_or_child(path, "/wunder/auth") {
        return false;
    }
    if path_is_prefix_or_child(path, "/wunder/chat") {
        return false;
    }
    if path_is_prefix_or_child(path, "/wunder/workspace") {
        return false;
    }
    if path_is_prefix_or_child(path, "/wunder/user_world") {
        return false;
    }
    if path_is_prefix_or_child(path, "/wunder/cron") {
        return false;
    }
    if path_is_prefix_or_child(path, "/wunder/channels") {
        return false;
    }
    if path_is_prefix_or_child(path, "/wunder/user_tools") {
        return false;
    }
    if path_is_prefix_or_child(path, "/wunder/prompt_templates") {
        return false;
    }
    if path_is_prefix_or_child(path, "/wunder/agents") {
        return false;
    }
    if path_is_prefix_or_child(path, "/wunder/beeroom") {
        return false;
    }
    if path_is_prefix_or_child(path, "/wunder/external_links") {
        return false;
    }
    if path_is_prefix_or_child(path, "/wunder/external/workflows") {
        return false;
    }
    if path_is_prefix_or_child(path, "/wunder/plaza") {
        return false;
    }
    if path_is_prefix_or_child(path, "/wunder/companions/global") {
        return false;
    }
    true
}

fn path_is_prefix_or_child(path: &str, prefix: &str) -> bool {
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

pub fn is_leader_path(path: &str) -> bool {
    path.starts_with("/wunder/admin/org_units") || path.starts_with("/wunder/admin/user_accounts")
}

pub fn extract_api_key_values(
    x_api_key: Option<&str>,
    authorization: Option<&str>,
) -> Option<String> {
    if let Some(value) = clean_non_empty(x_api_key) {
        return Some(value.to_string());
    }
    authorization.and_then(extract_bearer_token_value)
}

pub fn extract_bearer_token_value(authorization: &str) -> Option<String> {
    let text = authorization.trim();
    if let Some(prefix) = text.get(..7) {
        if prefix.eq_ignore_ascii_case("bearer ") {
            if let Some(raw) = text.get(7..) {
                return clean_non_empty(Some(raw)).map(str::to_string);
            }
        }
    }
    None
}

fn clean_non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_admin_path_keeps_public_routes_open() {
        assert!(!is_admin_path("/"));
        assert!(!is_admin_path("/wunder/ppt"));
        assert!(!is_admin_path("/wunder/ppt-en"));
        assert!(!is_admin_path("/wunder/i18n"));
        assert!(!is_admin_path("/wunder/doc2md/convert"));
        assert!(!is_admin_path("/wunder/temp_dir/download"));
        assert!(!is_admin_path("/.well-known/agent-card.json"));
        assert!(!is_admin_path("/wunder/auth/login"));
        assert!(!is_admin_path("/wunder/chat/sessions"));
        assert!(!is_admin_path("/wunder/workspace"));
        assert!(!is_admin_path("/wunder/user_world/contacts"));
        assert!(!is_admin_path("/wunder/user_world/ws"));
        assert!(!is_admin_path("/wunder/cron/list"));
        assert!(!is_admin_path("/wunder/channels/accounts"));
        assert!(!is_admin_path("/wunder/user_tools/mcp"));
        assert!(!is_admin_path("/wunder/prompt_templates"));
        assert!(!is_admin_path("/wunder/prompt_templates/file"));
        assert!(!is_admin_path("/wunder/agents"));
        assert!(!is_admin_path("/wunder/beeroom/groups"));
        assert!(!is_admin_path("/wunder/beeroom/groups/default"));
        assert!(!is_admin_path("/wunder/external_links"));
        assert!(!is_admin_path("/wunder/external/workflows"));
        assert!(!is_admin_path("/wunder/plaza/items"));
        assert!(!is_admin_path("/wunder/plaza/items/demo"));
        assert!(!is_admin_path("/wunder/companions/global"));
        assert!(!is_admin_path("/wunder/companions/global/abc"));
        assert!(is_admin_path("/wunder"));
        assert!(is_admin_path("/wunder/mcp"));
        assert!(is_admin_path("/a2a"));
    }

    #[test]
    fn is_leader_path_matches_org_and_user_admin_routes() {
        assert!(is_leader_path("/wunder/admin/org_units"));
        assert!(is_leader_path("/wunder/admin/org_units/root"));
        assert!(is_leader_path("/wunder/admin/user_accounts"));
        assert!(is_leader_path("/wunder/admin/user_accounts/abc"));
        assert!(!is_leader_path("/wunder/admin/users"));
        assert!(!is_leader_path("/wunder/chat/sessions"));
    }

    #[test]
    fn extracts_api_key_from_explicit_header_before_bearer() {
        assert_eq!(
            extract_api_key_values(Some(" explicit "), Some("Bearer bearer-token")),
            Some("explicit".to_string())
        );
    }

    #[test]
    fn extracts_bearer_token_case_insensitively() {
        assert_eq!(
            extract_bearer_token_value("bEaReR sample-token"),
            Some("sample-token".to_string())
        );
        assert_eq!(extract_bearer_token_value("Basic sample-token"), None);
    }

    #[test]
    fn empty_explicit_api_key_falls_back_to_bearer_token() {
        assert_eq!(
            extract_api_key_values(Some("   "), Some("Bearer fallback-token")),
            Some("fallback-token".to_string())
        );
    }

    #[test]
    fn bearer_token_requires_space_delimited_prefix() {
        assert_eq!(extract_bearer_token_value("Bearer"), None);
        assert_eq!(extract_bearer_token_value("BearerToken value"), None);
        assert_eq!(extract_bearer_token_value("Bearer    "), None);
    }

    #[test]
    fn route_prefixes_do_not_unlock_similar_admin_paths() {
        assert!(is_admin_path("/wunder/plazadmin"));
        assert!(is_admin_path("/wunder/user_tooling"));
        assert!(is_admin_path("/wunder/companions/global-admin"));
    }
}
