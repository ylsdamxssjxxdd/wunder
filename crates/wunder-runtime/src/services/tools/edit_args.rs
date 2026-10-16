//! Normalize every text-edit form before routing or sandbox forwarding.
use super::{
    recover_tool_args_value,
    tool_error::{build_failed_tool_result, ToolErrorMeta},
};
use serde_json::{json, Value};

pub(super) fn normalize_edit_args(args: &Value) -> Result<Value, Value> {
    let mut args = recover_tool_args_value(args);
    let Some(map) = args.as_object_mut() else {
        return Err(invalid_args("参数必须是 JSON 对象。", json!({})));
    };
    for (primary, alias) in [
        ("file_path", "path"),
        ("old_string", "old_str"),
        ("new_string", "new_str"),
        ("input", "patch"),
    ] {
        if let (Some(first), Some(second)) = (map.get(primary), map.get(alias)) {
            if first != second {
                return Err(invalid_args(
                    &format!("{primary} 与别名 {alias} 的值冲突；只保留一个字段。"),
                    json!({"fields": [primary, alias]}),
                ));
            }
        }
        if !map.contains_key(primary) {
            if let Some(value) = map.get(alias).cloned() {
                map.insert(primary.to_string(), value);
            }
        }
        map.remove(alias);
    }
    for key in [
        "file_path",
        "old_string",
        "new_string",
        "command",
        "file_text",
        "input",
    ] {
        if map.get(key).is_some_and(|value| !value.is_string()) {
            return Err(invalid_args(
                &format!("{key} 必须是字符串；删除文本请显式传 new_string=\"\"。"),
                json!({"field": key}),
            ));
        }
    }
    for key in ["replace_all", "dry_run"] {
        if map.get(key).is_some_and(|value| !value.is_boolean()) {
            return Err(invalid_args(
                &format!("{key} 必须是布尔值 true 或 false。"),
                json!({"field": key}),
            ));
        }
    }
    if let Some(Value::String(command)) = map.get_mut("command") {
        *command = command.trim().to_string();
    }
    if super::edit_tool::has_patch_input(&args)
        && (args.get("old_string").is_some()
            || args.get("new_string").is_some()
            || args
                .get("command")
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty()))
    {
        return Err(invalid_args(
            "一次调用只能使用一种编辑方式：input 补丁，或文本替换/子命令。",
            json!({}),
        ));
    }
    Ok(args)
}

fn invalid_args(message: &str, data: Value) -> Value {
    build_failed_tool_result(
        message,
        data,
        ToolErrorMeta::new(
            "TOOL_EDIT_INVALID_ARGS",
            Some("按提示修改参数后重试；相同参数不会成功。".into()),
            false,
            None,
        ),
        false,
    )
}
