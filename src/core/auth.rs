// 鉴权辅助：统一路径保护规则与 API Key 解析。
use axum::http::header::AUTHORIZATION;
use axum::http::HeaderMap;

pub fn is_admin_path(path: &str) -> bool {
    if path.starts_with("/.well-known/agent-card.json") {
        return false;
    }
    if path.starts_with("/a2a") {
        return true;
    }
    if !path.starts_with("/wunder") {
        return false;
    }
    if path.starts_with("/wunder/ppt") {
        return false;
    }
    if path.starts_with("/wunder/i18n") {
        return false;
    }
    if path.starts_with("/wunder/doc2md") {
        return false;
    }
    if path.starts_with("/wunder/temp_dir") {
        return false;
    }
    if path.starts_with("/wunder/auth") {
        return false;
    }
    if path.starts_with("/wunder/chat") {
        return false;
    }
    if path.starts_with("/wunder/workspace") {
        return false;
    }
    if path.starts_with("/wunder/user_tools") {
        return false;
    }
    if path.starts_with("/wunder/agents") {
        return false;
    }
    if path.starts_with("/wunder/external_links") {
        return false;
    }
    true
}

pub fn is_leader_path(path: &str) -> bool {
    path.starts_with("/wunder/admin/org_units") || path.starts_with("/wunder/admin/user_accounts")
}

pub fn extract_api_key(headers: &HeaderMap) -> Option<String> {
    // 兼容 X-API-Key 与 Authorization: Bearer 的两种格式。
    if let Some(value) = headers.get("x-api-key") {
        if let Ok(text) = value.to_str() {
            let cleaned = text.trim();
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        }
    }
    if let Some(value) = headers.get(AUTHORIZATION) {
        let text = value.to_str().ok()?.trim();
        if let Some(prefix) = text.get(..7) {
            if prefix.eq_ignore_ascii_case("bearer ") {
                if let Some(raw) = text.get(7..) {
                    let cleaned = raw.trim();
                    if !cleaned.is_empty() {
                        return Some(cleaned.to_string());
                    }
                }
            }
        }
    }
    None
}

pub fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(AUTHORIZATION)?;
    let text = value.to_str().ok()?.trim();
    if let Some(prefix) = text.get(..7) {
        if prefix.eq_ignore_ascii_case("bearer ") {
            if let Some(raw) = text.get(7..) {
                let cleaned = raw.trim();
                if !cleaned.is_empty() {
                    return Some(cleaned.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_admin_path() {
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
        assert!(!is_admin_path("/wunder/user_tools/mcp"));
        assert!(!is_admin_path("/wunder/agents"));
        assert!(!is_admin_path("/wunder/external_links"));
        assert!(is_admin_path("/wunder"));
        assert!(is_admin_path("/wunder/mcp"));
        assert!(is_admin_path("/a2a"));
    }

    #[test]
    fn test_is_leader_path() {
        assert!(is_leader_path("/wunder/admin/org_units"));
        assert!(is_leader_path("/wunder/admin/org_units/root"));
        assert!(is_leader_path("/wunder/admin/user_accounts"));
        assert!(is_leader_path("/wunder/admin/user_accounts/abc"));
        assert!(!is_leader_path("/wunder/admin/users"));
        assert!(!is_leader_path("/wunder/chat/sessions"));
    }
}
