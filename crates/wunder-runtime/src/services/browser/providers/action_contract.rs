//! Shared runtime contract for the model-facing `browser` action tool.
//!
//! Provider schemas guide the model but are not an authorization boundary:
//! callers may send raw JSON directly, so per-action fields, types and
//! constraints are enforced here before any command reaches a backend. Every
//! provider that exposes the `browser` tool must validate first; otherwise
//! unknown fields are silently dropped by the backend and the model believes
//! an action happened when it did not.

use anyhow::{anyhow, Result};
use serde_json::{Map, Value};

pub(crate) const BROWSER_ACTIONS: &[&str] = &[
    "navigate",
    "back",
    "tabs_list",
    "tabs_new",
    "tabs_select",
    "tabs_close",
    "observe_refs",
    "observe_text",
    "screenshot",
    "click",
    "type",
    "hover",
    "select",
    "drag",
    "scroll",
    "press",
    "wait",
    "console",
    "resize",
    "file_upload",
    "dialog",
    "network",
    "batch",
];

/// Validate one browser action and return its normalized name.
pub(crate) fn validate_browser_action(args: &Value) -> Result<String> {
    let map = args
        .as_object()
        .ok_or_else(|| anyhow!("browser arguments must be an object"))?;
    let action = required_str(args, "action")?;
    let allowed = browser_action_allowed_fields(&action)
        .ok_or_else(|| anyhow!("unsupported browser action '{action}'"))?;
    reject_unlisted_fields(map, allowed, &format!("browser action '{action}'"))?;
    validate_optional_strings(
        args,
        &[
            "target_id",
            "path",
            "url",
            "cursor",
            "ref",
            "selector",
            "from_ref",
            "from_selector",
            "to_ref",
            "to_selector",
            "text",
            "key",
            "prompt_text",
        ],
    )?;
    validate_optional_boolean(
        args,
        &["double_click", "submit", "full_page", "clear", "accept"],
    )?;
    validate_optional_integer(args, "max_chars", 1, 40_000)?;
    validate_optional_integer(args, "snapshot_timeout_ms", 0, 120_000)?;
    validate_optional_integer(args, "wait_ms", 0, 120_000)?;
    validate_optional_integer(args, "timeout_ms", 1, 120_000)?;
    validate_optional_integer(args, "index", 0, i64::MAX)?;
    // 滚轮增量可以为负（向上/缩小），坐标只能是视口内的正数。
    validate_optional_integer(args, "delta_x", -20_000, 20_000)?;
    validate_optional_integer(args, "delta_y", -20_000, 20_000)?;
    validate_optional_integer(args, "steps", 1, 60)?;
    for key in ["x", "y", "from_x", "from_y", "to_x", "to_y"] {
        validate_optional_integer(args, key, 0, 20_000)?;
    }
    validate_optional_integer(args, "width", 240, 3_840)?;
    validate_optional_integer(args, "height", 240, 3_840)?;
    if let Some(files) = args.get("files") {
        let items = files
            .as_array()
            .ok_or_else(|| anyhow!("files must be an array"))?;
        if items.is_empty() || items.len() > 16 {
            return Err(anyhow!("files must contain 1-16 paths"));
        }
        if items
            .iter()
            .any(|item| item.as_str().is_none_or(|text| text.trim().is_empty()))
        {
            return Err(anyhow!("files entries must be non-empty strings"));
        }
    }
    validate_optional_enum(
        args,
        "load_state",
        &["load", "domcontentloaded", "networkidle"],
    )?;
    validate_optional_enum(
        args,
        "state",
        &["visible", "hidden", "attached", "detached"],
    )?;

    match action.as_str() {
        "navigate" => {
            // 工作区页面用 path 直接预览（无需自建 HTTP 服务），外部页面用 url。
            if opt_str(args, "path").is_none() {
                required_str(args, "url")?;
            }
        }
        "tabs_select" | "tabs_close" => require_tab_target(args, &action)?,
        "type" => {
            required_str(args, "text")?;
            require_element_target(args, &action)?;
        }
        "click" | "hover" => require_click_target(args, &action)?,
        "select" => {
            require_element_target(args, &action)?;
            validate_select_value(args, "browser action select")?;
        }
        "drag" => require_drag_gesture(args)?,
        "scroll" => require_scroll_delta(args)?,
        "press" => {
            required_str(args, "key")?;
            require_optional_element_target(args, &action)?;
        }
        "resize" => {
            args.get("width")
                .ok_or_else(|| anyhow!("browser action resize requires width"))?;
            args.get("height")
                .ok_or_else(|| anyhow!("browser action resize requires height"))?;
        }
        "file_upload" => {
            if args.get("files").is_none() {
                return Err(anyhow!("browser action file_upload requires files"));
            }
            require_optional_element_target(args, &action)?;
        }
        "network" => {} // 无必填字段，clear 可选。
        "batch" => validate_batch_steps(
            args.get("steps")
                .ok_or_else(|| anyhow!("browser action batch requires steps"))?,
        )?,
        _ => {}
    }
    Ok(action)
}

pub(crate) fn browser_action_allowed_fields(action: &str) -> Option<&'static [&'static str]> {
    let allowed: &'static [&'static str] = match action {
        "navigate" => &["action", "url", "path", "target_id", "timeout_ms"],
        "back" => &["action", "target_id"],
        "tabs_list" => &["action"],
        "tabs_new" => &["action", "url", "target_id"],
        "tabs_select" | "tabs_close" => &["action", "target_id", "index"],
        "observe_refs" => &["action", "target_id", "max_chars", "snapshot_timeout_ms"],
        "observe_text" => &["action", "target_id", "max_chars", "cursor"],
        "screenshot" => &["action", "target_id", "path", "full_page"],
        "click" => &[
            "action",
            "target_id",
            "ref",
            "selector",
            "text",
            "double_click",
            "timeout_ms",
        ],
        "type" => &[
            "action",
            "target_id",
            "ref",
            "selector",
            "text",
            "submit",
            "timeout_ms",
        ],
        "hover" => &[
            "action",
            "target_id",
            "ref",
            "selector",
            "text",
            "timeout_ms",
        ],
        "select" => &[
            "action",
            "target_id",
            "ref",
            "selector",
            "value",
            "timeout_ms",
        ],
        "drag" => &[
            "action",
            "target_id",
            "from_ref",
            "from_selector",
            "to_ref",
            "to_selector",
            "from_x",
            "from_y",
            "to_x",
            "to_y",
            "steps",
            "timeout_ms",
        ],
        "scroll" => &[
            "action",
            "target_id",
            "delta_x",
            "delta_y",
            "x",
            "y",
            "timeout_ms",
        ],
        "console" => &["action", "target_id", "clear"],
        "resize" => &["action", "target_id", "width", "height"],
        "file_upload" => &[
            "action",
            "target_id",
            "ref",
            "selector",
            "files",
            "timeout_ms",
        ],
        "dialog" => &["action", "target_id", "accept", "prompt_text"],
        "network" => &["action", "target_id", "clear"],
        "press" => &[
            "action",
            "target_id",
            "key",
            "ref",
            "selector",
            "timeout_ms",
        ],
        "wait" => &[
            "action",
            "target_id",
            "wait_ms",
            "load_state",
            "text",
            "selector",
            "state",
            "url",
            "timeout_ms",
        ],
        "batch" => &["action", "target_id", "steps"],
        _ => return None,
    };
    Some(allowed)
}

fn reject_unlisted_fields(map: &Map<String, Value>, allowed: &[&str], subject: &str) -> Result<()> {
    if let Some(extra) = map.keys().find(|key| !allowed.contains(&key.as_str())) {
        // A flat action schema cannot express which action owns a field. When
        // the model combines them anyway, name the owning action(s) so one
        // retry can fix the call instead of dropping fields by guesswork.
        let mut owners: Vec<&str> = BROWSER_ACTIONS
            .iter()
            .copied()
            .filter(|candidate| {
                browser_action_allowed_fields(candidate)
                    .is_some_and(|fields| fields.contains(&extra.as_str()))
            })
            .collect();
        owners.sort_unstable();
        let hint = if owners.is_empty() {
            " (unknown field)".to_string()
        } else {
            format!("; use it with action(s): {}", owners.join(", "))
        };
        return Err(anyhow!("field '{extra}' is not valid for {subject}{hint}"));
    }
    Ok(())
}

fn validate_optional_strings(args: &Value, keys: &[&str]) -> Result<()> {
    for key in keys {
        if args.get(*key).is_some() {
            required_str(args, key)?;
        }
    }
    Ok(())
}

fn validate_optional_boolean(args: &Value, keys: &[&str]) -> Result<()> {
    for key in keys {
        if args.get(*key).is_some_and(|value| !value.is_boolean()) {
            return Err(anyhow!("{key} must be a boolean"));
        }
    }
    Ok(())
}

fn validate_optional_integer(args: &Value, key: &str, min: i64, max: i64) -> Result<()> {
    let Some(value) = args.get(key) else {
        return Ok(());
    };
    let integer = value
        .as_i64()
        .ok_or_else(|| anyhow!("{key} must be an integer"))?;
    if !(min..=max).contains(&integer) {
        return Err(anyhow!("{key} is outside its allowed range"));
    }
    Ok(())
}

fn validate_optional_enum(args: &Value, key: &str, allowed: &[&str]) -> Result<()> {
    let Some(value) = args.get(key) else {
        return Ok(());
    };
    let value = value
        .as_str()
        .ok_or_else(|| anyhow!("{key} must be a string"))?;
    if !allowed.contains(&value) {
        return Err(anyhow!("{key} has an unsupported value"));
    }
    Ok(())
}

fn require_element_target(args: &Value, subject: &str) -> Result<()> {
    let has_ref = opt_str(args, "ref").is_some();
    let has_selector = opt_str(args, "selector").is_some();
    if has_ref == has_selector {
        return Err(anyhow!(
            "browser action '{subject}' requires exactly one of ref or selector"
        ));
    }
    Ok(())
}

fn require_click_target(args: &Value, subject: &str) -> Result<()> {
    let targets = ["ref", "selector", "text"]
        .into_iter()
        .filter(|key| opt_str(args, key).is_some())
        .count();
    if targets != 1 {
        return Err(anyhow!(
            "browser action '{subject}' requires exactly one of ref, selector or text"
        ));
    }
    Ok(())
}

fn require_optional_element_target(args: &Value, subject: &str) -> Result<()> {
    if opt_str(args, "ref").is_some() && opt_str(args, "selector").is_some() {
        return Err(anyhow!(
            "browser action '{subject}' accepts at most one of ref or selector"
        ));
    }
    Ok(())
}

fn require_drag_target(args: &Value, subject: &str) -> Result<()> {
    let refs = opt_str(args, "from_ref").is_some() && opt_str(args, "to_ref").is_some();
    let selectors =
        opt_str(args, "from_selector").is_some() && opt_str(args, "to_selector").is_some();
    if refs == selectors {
        return Err(anyhow!(
            "{subject} requires exactly one complete ref or selector pair"
        ));
    }
    Ok(())
}

/// 拖拽有两种合法形态：元素到元素，或在画布上按坐标拖（3D 场景靠这条旋转）。
fn require_drag_gesture(args: &Value) -> Result<()> {
    let coordinates = ["from_x", "from_y", "to_x", "to_y"]
        .into_iter()
        .all(|key| args.get(key).is_some());
    let any_coordinate = ["from_x", "from_y", "to_x", "to_y"]
        .into_iter()
        .any(|key| args.get(key).is_some());
    if coordinates {
        return Ok(());
    }
    if any_coordinate {
        return Err(anyhow!(
            "browser action drag needs all of from_x, from_y, to_x and to_y for a coordinate drag"
        ));
    }
    require_drag_target(args, "browser action drag")
}

/// 滚轮必须带一个非零增量，否则这次调用什么也不会发生。
fn require_scroll_delta(args: &Value) -> Result<()> {
    let delta = |key: &str| {
        args.get(key)
            .and_then(Value::as_i64)
            .filter(|value| *value != 0)
    };
    if delta("delta_x").is_none() && delta("delta_y").is_none() {
        return Err(anyhow!(
            "browser action scroll needs a non-zero delta_x or delta_y"
        ));
    }
    Ok(())
}

fn require_tab_target(args: &Value, action: &str) -> Result<()> {
    let has_target_id = opt_str(args, "target_id").is_some();
    let has_index = args.get("index").is_some();
    if has_target_id == has_index {
        return Err(anyhow!(
            "browser action '{action}' requires exactly one of target_id or index"
        ));
    }
    Ok(())
}

fn validate_select_value(args: &Value, subject: &str) -> Result<()> {
    let value = args
        .get("value")
        .ok_or_else(|| anyhow!("{subject} requires value"))?;
    let valid = value.as_str().is_some_and(|item| !item.trim().is_empty())
        || value.as_array().is_some_and(|items| {
            !items.is_empty()
                && items.len() <= 64
                && items
                    .iter()
                    .all(|item| item.as_str().is_some_and(|text| !text.trim().is_empty()))
        });
    if !valid {
        return Err(anyhow!(
            "{subject} value must be a non-empty string or a 1-64 item non-empty string array"
        ));
    }
    Ok(())
}

fn validate_batch_steps(steps: &Value) -> Result<()> {
    let items = steps
        .as_array()
        .ok_or_else(|| anyhow!("browser action batch requires an array steps"))?;
    if items.is_empty() || items.len() > 10 {
        return Err(anyhow!("browser action batch requires 1-10 steps"));
    }
    for (index, step) in items.iter().enumerate() {
        let map = step
            .as_object()
            .ok_or_else(|| anyhow!("browser batch step {index} must be an object"))?;
        let kind = required_str(step, "kind")?;
        let allowed: &[&str] = match kind.as_str() {
            "navigate" => &["kind", "url", "timeout_ms"],
            "click" => &[
                "kind",
                "ref",
                "selector",
                "text",
                "double_click",
                "timeout_ms",
            ],
            "type" => &["kind", "ref", "selector", "text", "submit", "timeout_ms"],
            "hover" => &["kind", "ref", "selector", "text", "timeout_ms"],
            "select" => &["kind", "ref", "selector", "value", "timeout_ms"],
            "drag" => &[
                "kind",
                "from_ref",
                "from_selector",
                "to_ref",
                "to_selector",
                "timeout_ms",
            ],
            "press" => &["kind", "key", "ref", "selector", "timeout_ms"],
            "back" => &["kind"],
            "observe_text" => &["kind", "max_chars", "cursor"],
            "wait" => &[
                "kind",
                "wait_ms",
                "load_state",
                "text",
                "selector",
                "state",
                "url",
                "timeout_ms",
            ],
            _ => {
                return Err(anyhow!(
                    "browser batch step {index} has unsupported kind '{kind}'"
                ))
            }
        };
        reject_unlisted_fields(map, allowed, &format!("browser batch step {index}"))?;
        validate_optional_strings(
            step,
            &[
                "url",
                "ref",
                "selector",
                "text",
                "from_ref",
                "from_selector",
                "to_ref",
                "to_selector",
                "key",
                "cursor",
            ],
        )?;
        validate_optional_boolean(step, &["double_click", "submit"])?;
        validate_optional_integer(step, "max_chars", 1, 40_000)?;
        validate_optional_integer(step, "wait_ms", 0, 120_000)?;
        validate_optional_integer(step, "timeout_ms", 1, 120_000)?;
        validate_optional_enum(
            step,
            "load_state",
            &["load", "domcontentloaded", "networkidle"],
        )?;
        validate_optional_enum(
            step,
            "state",
            &["visible", "hidden", "attached", "detached"],
        )?;
        match kind.as_str() {
            "navigate" => {
                required_str(step, "url")?;
            }
            "click" | "hover" => require_click_target(step, &format!("batch step {index}"))?,
            "type" => {
                required_str(step, "text")?;
                require_element_target(step, &format!("batch step {index}"))?;
            }
            "select" => {
                require_element_target(step, &format!("batch step {index}"))?;
                validate_select_value(step, &format!("browser batch step {index}"))?;
            }
            "drag" => require_drag_gesture(step)?,
            "press" => {
                required_str(step, "key")?;
                require_optional_element_target(step, &format!("batch step {index}"))?;
            }
            "back" | "observe_text" | "wait" => {}
            _ => unreachable!("batch kind validated"),
        }
    }
    Ok(())
}

pub(crate) fn opt_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub(crate) fn required_str(args: &Value, key: &str) -> Result<String> {
    opt_str(args, key).ok_or_else(|| anyhow!("Missing required parameter '{key}'"))
}

#[cfg(test)]
mod tests {
    use super::{
        browser_action_allowed_fields, validate_batch_steps, validate_browser_action,
        BROWSER_ACTIONS,
    };
    use crate::services::browser::providers::playwright::TOOL_BROWSER;
    use serde_json::json;

    #[test]
    fn runtime_validation_rejects_cross_action_fields_and_bad_targets() {
        assert!(validate_browser_action(
            &json!({"action":"navigate","url":"https://example.test","ref":"e1"})
        )
        .is_err());
        assert!(validate_browser_action(&json!({"action":"click"})).is_err());
        assert!(validate_browser_action(
            &json!({"action":"drag","from_ref":"a","to_selector":"b"})
        )
        .is_err());
        assert!(validate_browser_action(&json!({"action":"tabs_close"})).is_err());
        assert!(
            validate_browser_action(&json!({"action":"observe_text","max_chars":40001})).is_err()
        );
        assert!(
            validate_browser_action(&json!({"action":"click","ref":"e1","selector":"#same"}))
                .is_err()
        );
        assert!(validate_browser_action(&json!({"action":"click","ref":"e1"})).is_ok());
        assert!(validate_browser_action(&json!({"action":"click","text":"Continue"})).is_ok());
        assert!(validate_browser_action(&json!({"action":"press","key":"Enter"})).is_ok());
        assert!(validate_browser_action(
            &json!({"action":"press","key":"Enter","selector":"#field","timeout_ms":50})
        )
        .is_ok());
        assert!(validate_browser_action(
            &json!({"action":"press","key":"Enter","ref":"e1","selector":"#field"})
        )
        .is_err());
        assert!(
            validate_browser_action(&json!({"action":"click","ref":"e1","text":"Continue"}))
                .is_err()
        );
        for action in ["click", "type", "hover", "select", "drag"] {
            let mut args = json!({"action":action,"selector":"#target","timeout_ms":50});
            if action == "type" {
                args["text"] = json!("sample");
            } else if action == "select" {
                args["value"] = json!("sample");
            } else if action == "drag" {
                args = json!({
                    "action":action,
                    "from_selector":"#source",
                    "to_selector":"#target",
                    "timeout_ms":50
                });
            }
            assert!(validate_browser_action(&args).is_ok(), "{action}: {args}");
        }
        assert!(validate_browser_action(&json!({"action":"tabs_select","index":0})).is_ok());
        assert!(validate_browser_action(
            &json!({"action":"tabs_select","target_id":"tab-1","index":0})
        )
        .is_err());
    }

    #[test]
    fn batch_validation_is_bounded_and_forbids_nested_batch() {
        assert!(validate_batch_steps(&json!([{"kind":"batch","steps":[]}])).is_err());
        assert!(validate_batch_steps(
            &json!([{"kind":"click","ref":"e1","url":"https://example.test"}])
        )
        .is_err());
        assert!(
            validate_batch_steps(&json!([{"kind":"type","selector":"#field","text":"x"}])).is_ok()
        );
        assert!(validate_batch_steps(&json!([{"kind":"back"}])).is_ok());
        assert!(validate_batch_steps(&json!([
            {"kind":"click","text":"Continue"},
            {"kind":"press","selector":"#field","key":"Enter","timeout_ms":50}
        ]))
        .is_ok());
        assert!(validate_batch_steps(&json!([
            {"kind":"press","ref":"e1","selector":"#field","key":"Enter"}
        ]))
        .is_err());
        assert!(validate_batch_steps(
            &json!([{"kind":"observe_text","max_chars":400,"cursor":"c1"}])
        )
        .is_ok());
        assert!(validate_batch_steps(&json!([{"kind":"observe_text","max_chars":0}])).is_err());
        assert!(validate_batch_steps(&json!([{"kind":"observe_text","cursor":true}])).is_err());
        let many: Vec<_> = (0..11)
            .map(|_| json!({"kind":"wait","wait_ms":1}))
            .collect();
        assert!(validate_batch_steps(&json!(many)).is_err());
    }

    #[test]
    fn viewport_upload_dialog_and_network_actions_validate_their_fields() {
        assert!(
            validate_browser_action(&json!({"action":"resize","width":800,"height":600})).is_ok()
        );
        assert!(validate_browser_action(&json!({"action":"resize","width":800})).is_err());
        assert!(
            validate_browser_action(&json!({"action":"resize","width":100,"height":600})).is_err()
        );
        assert!(
            validate_browser_action(&json!({"action":"file_upload","files":["notes.txt"]})).is_ok()
        );
        assert!(validate_browser_action(&json!({"action":"file_upload"})).is_err());
        assert!(validate_browser_action(
            &json!({"action":"file_upload","selector":"#file","files":[]})
        )
        .is_err());
        assert!(validate_browser_action(
            &json!({"action":"file_upload","selector":"#file","files":["notes.txt"],"width":800})
        )
        .is_err());
        assert!(validate_browser_action(
            &json!({"action":"dialog","accept":false,"prompt_text":"later"})
        )
        .is_ok());
        assert!(validate_browser_action(&json!({"action":"network","clear":true})).is_ok());
        assert!(validate_browser_action(&json!({"action":"network","accept":true})).is_err());
    }

    #[test]
    fn cross_action_field_error_names_the_owning_action() {
        let error = validate_browser_action(&json!({
            "action": "navigate",
            "url": "https://example.test",
            "load_state": "domcontentloaded"
        }))
        .expect_err("cross-action field must be rejected");
        let message = error.to_string();
        assert!(message.contains("load_state"), "{message}");
        assert!(message.contains("action(s): wait"), "{message}");
    }

    #[test]
    fn model_visible_browser_fields_are_accepted_by_at_least_one_action() {
        let spec = crate::tools::builtin_tool_specs()
            .into_iter()
            .find(|spec| spec.name == TOOL_BROWSER)
            .expect("browser spec");
        let projected = crate::tools::prompt_tool_spec(&spec);
        let properties = projected.input_schema["properties"]
            .as_object()
            .expect("browser properties");
        assert!(properties.contains_key("value"));
        assert!(properties.contains_key("timeout_ms"));
        assert!(projected.input_schema["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum")
            .contains(&json!("select")));
        for field in properties.keys() {
            let accepted = BROWSER_ACTIONS.iter().any(|action| {
                browser_action_allowed_fields(action)
                    .is_some_and(|fields| fields.contains(&field.as_str()))
            });
            assert!(
                accepted,
                "model-visible field '{field}' is dead in every action"
            );
        }
    }

    #[test]
    fn coordinate_drag_and_wheel_are_valid_without_a_locator() {
        // 画布上的拖拽和滚轮是 3D 页面的主交互，二者都不该要求 ref/selector。
        assert!(validate_browser_action(&json!({
            "action": "drag",
            "from_x": 40,
            "from_y": 120,
            "to_x": 260,
            "to_y": 180
        }))
        .is_ok());
        assert!(validate_browser_action(&json!({
            "action": "scroll",
            "delta_y": -120,
            "x": 120,
            "y": 80
        }))
        .is_ok());
        // 只给一半坐标或零增量必须当场失败，而不是静默什么都不做。
        let partial = validate_browser_action(&json!({
            "action": "drag",
            "from_x": 10,
            "to_x": 40
        }))
        .expect_err("partial coordinates must be rejected");
        assert!(partial.to_string().contains("from_x"), "{partial}");
        let empty = validate_browser_action(&json!({
            "action": "scroll",
            "delta_x": 0,
            "delta_y": 0
        }))
        .expect_err("zero delta must be rejected");
        assert!(empty.to_string().contains("delta"), "{empty}");
    }

    #[test]
    fn console_action_needs_no_locator_and_takes_clear() {
        assert!(validate_browser_action(&json!({"action": "console"})).is_ok());
        assert!(validate_browser_action(&json!({"action": "console", "clear": true})).is_ok());
    }

    #[test]
    fn navigate_accepts_a_workspace_path_instead_of_a_url() {
        assert!(validate_browser_action(&json!({
            "action": "navigate",
            "path": "pages/index.html"
        }))
        .is_ok());
        let missing = validate_browser_action(&json!({"action": "navigate"}))
            .expect_err("navigate needs url or path");
        assert!(missing.to_string().contains("url"), "{missing}");
    }

    #[test]
    fn select_value_rejects_empty_and_non_string_entries() {
        assert!(validate_browser_action(
            &json!({"action":"select","selector":"#s","value":["a","b"]})
        )
        .is_ok());
        assert!(
            validate_browser_action(&json!({"action":"select","selector":"#s","value":""}))
                .is_err()
        );
        assert!(validate_browser_action(
            &json!({"action":"select","selector":"#s","value":["a",""]})
        )
        .is_err());
        // `label` 不是共享契约字段：模型应按可见文字传 value，校验必须明说。
        let error =
            validate_browser_action(&json!({"action":"select","selector":"#s","label":"蓝色"}))
                .expect_err("unknown select field must be rejected");
        assert!(error.to_string().contains("label"), "{error}");
    }
}
