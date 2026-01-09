// 鉴权辅助：统一路径保护规则与 API Key 解析。
use axum::http::header::AUTHORIZATION;
use axum::http::HeaderMap;

pub fn is_protected_path(path: &str) -> bool {
    // 保持与 Python 版一致的鉴权豁免规则。
    if path.starts_with("/.well-known/agent-card.json") {
        return false;
    }
    if path.starts_with("/a2a") {
        return true;
    }
    if !path.starts_with("/wunder") {
        return false;
    }
    if path.starts_with("/wunder/web") {
        return false;
    }
    if path.starts_with("/wunder/ppt") {
        return false;
    }
    if path.starts_with("/wunder/i18n") {
        return false;
    }
    true
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_protected_path() {
        assert!(!is_protected_path("/"));
        assert!(!is_protected_path("/wunder/web"));
        assert!(!is_protected_path("/wunder/ppt"));
        assert!(!is_protected_path("/wunder/ppt-en"));
        assert!(!is_protected_path("/wunder/i18n"));
        assert!(!is_protected_path("/.well-known/agent-card.json"));
        assert!(is_protected_path("/wunder"));
        assert!(is_protected_path("/wunder/mcp"));
        assert!(is_protected_path("/a2a"));
    }
}
