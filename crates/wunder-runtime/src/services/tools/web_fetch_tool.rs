#[cfg(feature = "web-fetch")]
mod enabled {
    pub use super::super::web_fetch_tool_impl::*;
}

#[cfg(feature = "web-fetch")]
pub use enabled::*;

#[cfg(not(feature = "web-fetch"))]
mod disabled {
    use super::super::context::ToolContext;
    use super::super::tool_error::{build_failed_tool_result, ToolErrorMeta};
    use crate::config::Config;
    use anyhow::Result;
    use serde_json::{json, Value};

    pub const TOOL_WEB_FETCH: &str = "网页抓取";
    pub const TOOL_WEB_FETCH_ALIAS: &str = "web_fetch";

    pub fn is_web_fetch_tool_name(name: &str) -> bool {
        let cleaned = name.trim();
        if cleaned == TOOL_WEB_FETCH {
            return true;
        }
        cleaned.eq_ignore_ascii_case(TOOL_WEB_FETCH_ALIAS)
    }

    pub fn web_fetch_enabled(_config: &Config) -> bool {
        false
    }

    pub async fn tool_web_fetch(_context: &ToolContext<'_>, args: &Value) -> Result<Value> {
        let url = args
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        Ok(build_failed_tool_result(
            "web_fetch feature is disabled; rebuild with --features web-fetch",
            json!({
                "url": url,
                "phase": "feature_gate",
                "failure_summary": "web_fetch feature is disabled; rebuild with --features web-fetch",
                "next_step_hint": "Enable the web-fetch Cargo feature for direct webpage fetching, or remove web_fetch from the enabled tool set."
            }),
            ToolErrorMeta::new(
                "TOOL_WEB_FETCH_FEATURE_DISABLED",
                Some("Enable the web-fetch Cargo feature for direct webpage fetching.".to_string()),
                false,
                None,
            ),
            false,
        ))
    }
}

#[cfg(not(feature = "web-fetch"))]
pub use disabled::*;

#[cfg(all(test, not(feature = "web-fetch")))]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn disabled_web_fetch_is_hidden_from_runtime_availability() {
        assert!(!web_fetch_enabled(&Config::default()));
        assert!(is_web_fetch_tool_name(TOOL_WEB_FETCH));
        assert!(is_web_fetch_tool_name(TOOL_WEB_FETCH_ALIAS));
    }
}
