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

pub(crate) fn build_responses_freeform_tool(
    tool_name: &str,
    description: &str,
    model_tool_name: &str,
) -> Option<Value> {
    let format = freeform_tool_format(tool_name)?;
    Some(json!({
        "type": "custom",
        "name": model_tool_name,
        "description": description,
        "format": format,
    }))
}

pub(crate) fn render_prompt_tool_spec(spec: &ToolSpec, freeform_mode: bool) -> Value {
    if freeform_mode {
        if let Some(format) = freeform_tool_format(&spec.name) {
            return json!({
                "name": spec.name,
                "description": spec.description,
                "format": format,
            });
        }
    }
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
        Ok(Value::Object(map)) => map
            .get("input")
            .or_else(|| map.get("raw"))
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        Ok(Value::String(text)) => Some(text),
        _ => Some(trimmed.to_string()),
    }
}

fn is_apply_patch_tool(name: &str) -> bool {
    super::catalog::resolve_tool_name(name) == super::catalog::resolve_tool_name("apply_patch")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_patch_is_recognized_as_freeform_tool() {
        assert!(is_freeform_tool_name("apply_patch"));
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
}
