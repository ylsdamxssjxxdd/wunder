use crate::schemas::ToolSpec;
use serde_json::{json, Value};

const APPLY_PATCH_LARK_GRAMMAR: &str = include_str!("tool_apply_patch.lark");

pub(crate) fn is_freeform_tool_name(name: &str) -> bool {
    is_apply_patch_tool(name)
}

pub(crate) fn freeform_tool_format(name: &str) -> Option<Value> {
    if !is_apply_patch_tool(name) {
        return None;
    }
    Some(json!({
        "type": "grammar",
        "syntax": "lark",
        "definition": APPLY_PATCH_LARK_GRAMMAR,
    }))
}

pub(crate) fn build_freeform_tool(
    tool_name: &str,
    description: &str,
    model_tool_name: &str,
    responses_api: bool,
) -> Option<Value> {
    let format = freeform_tool_format(tool_name)?;
    if responses_api {
        return Some(json!({
            "type": "custom",
            "name": model_tool_name,
            "description": description,
            "format": format,
        }));
    }
    // ChatCompletions 没有 custom/grammar 工具类型：同一个语法工具退化成只带一个
    // 字符串参数的普通函数，输入依旧是补丁正文本身。
    Some(json!({
        "type": "function",
        "function": {
            "name": model_tool_name,
            "description": format!(
                "{description}\n\nThe `input` argument is the raw patch body (from `*** Begin Patch` to `*** End Patch`), not a JSON-encoded patch description."
            ),
            "parameters": {
                "type": "object",
                "properties": {
                    "input": {
                        "type": "string",
                        "description": "Raw patch body, `*** Begin Patch` ... `*** End Patch`."
                    },
                    "dry_run": {
                        "type": "boolean",
                        "description": "Preview the patch without writing files."
                    }
                },
                "required": ["input"],
                "additionalProperties": false
            }
        }
    }))
}

pub(crate) fn render_prompt_tool_spec(spec: &ToolSpec) -> Value {
    json!({
        "name": spec.name,
        "description": spec.description,
        "arguments": spec.input_schema,
    })
}

pub(crate) fn extract_freeform_tool_input(arguments: &str) -> Option<String> {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return None;
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Object(map)) => ["input", "raw", "patch", "content", "value"]
            .iter()
            .find_map(|key| map.get(*key).and_then(Value::as_str))
            .map(|value| value.to_string()),
        Ok(Value::String(text)) => Some(text),
        _ => Some(trimmed.to_string()),
    }
}

fn is_apply_patch_tool(name: &str) -> bool {
    // The merged editor accepts structured commands as well as patches.
    // Resolving aliases here would force every edit call through patch-only
    // grammar and hide its JSON schema from the model.
    matches!(name.trim(), "apply_patch" | "应用补丁" | "patch")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_patch_is_recognized_as_freeform_tool() {
        assert!(is_freeform_tool_name("apply_patch"));
    }

    #[test]
    fn merged_text_edit_keeps_json_parameters_even_in_freeform_mode() {
        for name in ["文本编辑", "edit", "编辑", "str_replace_editor"] {
            assert!(!is_freeform_tool_name(name));
            assert!(build_freeform_tool(name, "edit", "edit", true).is_none());
            assert!(build_freeform_tool(name, "edit", "edit", false).is_none());
        }
    }

    #[test]
    fn freeform_tool_degrades_to_input_function_on_chat_completions() {
        let tool = build_freeform_tool("apply_patch", "patch tool", "apply_patch", false)
            .expect("chat freeform tool");
        assert_eq!(tool["type"], "function");
        assert_eq!(tool["function"]["name"], "apply_patch");
        assert_eq!(
            tool["function"]["parameters"]["required"],
            json!(["input"])
        );
        assert_eq!(
            tool["function"]["parameters"]["properties"]["input"]["type"],
            "string"
        );
        assert_eq!(
            tool["function"]["parameters"]["properties"]["dry_run"]["type"],
            "boolean"
        );
        assert!(tool["function"]["parameters"]["additionalProperties"] == json!(false));
    }

    #[test]
    fn extract_freeform_tool_input_prefers_input_field() {
        let input = extract_freeform_tool_input(r#"{"input":"*** Begin Patch\n*** End Patch"}"#);
        assert_eq!(input, Some("*** Begin Patch\n*** End Patch".to_string()));
    }

    #[test]
    fn extract_freeform_tool_input_falls_back_to_raw_field() {
        let input = extract_freeform_tool_input(r#"{"raw":"patch body"}"#);
        assert_eq!(input, Some("patch body".to_string()));
    }

    #[test]
    fn extract_freeform_tool_input_accepts_patch_content_and_value_fields() {
        for key in ["patch", "content", "value"] {
            let mut object = serde_json::Map::new();
            object.insert(
                key.to_string(),
                Value::String("*** Begin Patch\n*** End Patch".to_string()),
            );
            let payload = Value::Object(object).to_string();
            assert_eq!(
                extract_freeform_tool_input(&payload),
                Some("*** Begin Patch\n*** End Patch".to_string()),
                "key={key}"
            );
        }
    }
}
