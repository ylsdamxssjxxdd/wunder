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
    use crate::schemas::ToolSpec;
    use anyhow::Result;
    use serde_json::{json, Value};

    pub const TOOL_DESKTOP_CONTROL: &str = "桌面控制";
    pub const TOOL_DESKTOP_CONTROL_ALIAS: &str = "desktop_control";
    pub const TOOL_DESKTOP_CONTROL_ALIAS_SHORT: &str = "desktop";

    pub fn is_desktop_control_tool_name(name: &str) -> bool {
        let cleaned = name.trim();
        if matches!(cleaned, TOOL_DESKTOP_CONTROL | "桌面控制器" | "桌面监视器") {
            return true;
        }
        matches!(
            cleaned.to_ascii_lowercase().as_str(),
            TOOL_DESKTOP_CONTROL_ALIAS
                | TOOL_DESKTOP_CONTROL_ALIAS_SHORT
                | "desktop_controller"
                | "desktop_monitor"
        )
    }

    pub fn desktop_tools_enabled(_config: &Config) -> bool {
        false
    }

    pub fn desktop_control_tool_spec(language: &str) -> ToolSpec {
        let description =
            crate::i18n::t_in_language("tool.spec.desktop_control.description", language);
        let bbox = json!({
            "type": "array",
            "items": {"type": "integer", "minimum": 0, "maximum": 10000},
            "anyOf": [
                {"type": "array", "items": {"type": "integer", "minimum": 0, "maximum": 10000}, "minItems": 2, "maxItems": 2},
                {"type": "array", "items": {"type": "integer", "minimum": 0, "maximum": 10000}, "minItems": 4, "maxItems": 4}
            ]
        });
        let branch = |action: Value,
                      mut properties: serde_json::Map<String, Value>,
                      required: &[&str]| {
            properties.insert("action".to_string(), action);
            json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
        };
        let base = || {
            let mut properties = serde_json::Map::new();
            properties.insert("bbox".to_string(), bbox.clone());
            properties
        };
        let mut scroll = base();
        scroll.insert(
            "scroll_steps".to_string(),
            json!({"type":"integer","minimum":1,"maximum":100}),
        );
        let mut press = base();
        press.insert(
            "key".to_string(),
            json!({"type":"string","minLength":1,"maxLength":128}),
        );
        let mut typing = base();
        typing.insert(
            "text".to_string(),
            json!({"type":"string","minLength":1,"maxLength":16384}),
        );
        let mut wait = base();
        wait.insert(
            "delay_ms".to_string(),
            json!({"type":"integer","minimum":0,"maximum":30000}),
        );
        let mut move_mouse = base();
        move_mouse.insert(
            "duration_ms".to_string(),
            json!({"type":"integer","minimum":0,"maximum":30000}),
        );
        let mut drag = base();
        drag.insert("to_bbox".to_string(), bbox.clone());
        drag.insert(
            "duration_ms".to_string(),
            json!({"type":"integer","minimum":0,"maximum":30000}),
        );
        ToolSpec {
            name: TOOL_DESKTOP_CONTROL.to_string(),
            title: None,
            description,
            input_schema: json!({"oneOf":[
                branch(json!({"enum":["left_click","left_double_click","right_click","middle_click","left_hold","right_hold","middle_hold","left_release","right_release","middle_release"]}), base(), &["action","bbox"]),
                branch(json!({"enum":["scroll_down","scroll_up"]}), scroll, &["action","bbox"]),
                branch(json!({"const":"press_key"}), press, &["action","bbox","key"]),
                branch(json!({"const":"type_text"}), typing, &["action","bbox","text"]),
                branch(json!({"const":"wait"}), wait, &["action","bbox"]),
                branch(json!({"const":"move_mouse"}), move_mouse, &["action","bbox"]),
                branch(json!({"const":"drag_drop"}), drag, &["action","bbox","to_bbox"])
            ]}),
        }
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
