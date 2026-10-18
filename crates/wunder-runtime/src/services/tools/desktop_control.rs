#[cfg(feature = "desktop-control")]
mod enabled {
    pub use super::super::desktop_control_impl::*;
}

#[cfg(feature = "desktop-control")]
pub use enabled::*;

#[cfg(not(feature = "desktop-control"))]
mod disabled {
    use super::super::context::ToolContext;
    use super::super::tool_error::{build_failed_tool_result, ToolErrorMeta};
    use crate::config::Config;
    use anyhow::Result;
    use serde_json::{json, Value};

    pub const TOOL_DESKTOP_CONTROL: &str = "桌面控制";
    pub const TOOL_DESKTOP_CONTROL_ALIAS: &str = "desktop_control";
    pub const TOOL_DESKTOP_CONTROL_ALIAS_SHORT: &str = "desktop";

    pub fn is_desktop_control_tool_name(name: &str) -> bool {
        let cleaned = name.trim();
        if cleaned == TOOL_DESKTOP_CONTROL {
            return true;
        }
        matches!(
            cleaned.to_ascii_lowercase().as_str(),
            TOOL_DESKTOP_CONTROL_ALIAS | TOOL_DESKTOP_CONTROL_ALIAS_SHORT
        )
    }

    pub fn desktop_tools_enabled(_config: &Config) -> bool {
        false
    }

    pub async fn tool_desktop_control(_context: &ToolContext<'_>, args: &Value) -> Result<Value> {
        Ok(disabled_result(
            TOOL_DESKTOP_CONTROL_ALIAS,
            args.get("action").and_then(Value::as_str),
        ))
    }

    pub async fn build_followup_user_message(_result_data: &Value) -> Result<Option<Value>> {
        Ok(None)
    }

    fn disabled_result(tool: &str, request: Option<&str>) -> Value {
        build_failed_tool_result(
            "desktop-control feature is disabled; rebuild with --features desktop-control",
            json!({
                "tool": tool,
                "request": request.unwrap_or_default(),
                "phase": "feature_gate",
                "failure_summary": "desktop-control feature is disabled; rebuild with --features desktop-control",
                "next_step_hint": "Enable the desktop-control Cargo feature for local desktop screenshots and input control."
            }),
            ToolErrorMeta::new(
                "TOOL_DESKTOP_CONTROL_FEATURE_DISABLED",
                Some(
                    "Enable the desktop-control Cargo feature for local desktop control."
                        .to_string(),
                ),
                false,
                None,
            ),
            false,
        )
    }
}

#[cfg(not(feature = "desktop-control"))]
pub use disabled::*;

#[cfg(all(test, not(feature = "desktop-control")))]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn disabled_desktop_control_keeps_alias_but_hides_runtime_tool() {
        assert!(is_desktop_control_tool_name(TOOL_DESKTOP_CONTROL));
        assert!(is_desktop_control_tool_name(TOOL_DESKTOP_CONTROL_ALIAS));
        assert!(!desktop_tools_enabled(&Config::default()));
    }
}
