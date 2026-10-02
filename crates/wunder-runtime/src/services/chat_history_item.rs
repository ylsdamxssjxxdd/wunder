use crate::orchestrator_constants::OBSERVATION_PREFIX;
use serde_json::Value;

pub(crate) fn is_tool_call_meta(item: &Value) -> bool {
    item.get("meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("type"))
        .and_then(Value::as_str)
        .map(|value| value == "tool_call")
        .unwrap_or(false)
}

pub(crate) fn is_tool_payload_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.starts_with(OBSERVATION_PREFIX) {
        return true;
    }
    if trimmed.contains("<tool_call")
        || trimmed.contains("</tool_call")
        || trimmed.contains("<tool>")
        || trimmed.contains("</tool>")
    {
        return true;
    }
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            return is_tool_payload(&value) && !has_visible_answer_fields(&value);
        }
    }
    trimmed.contains("\"tool_calls\"")
        || trimmed.contains("\"tool_call\"")
        || trimmed.contains("\"function_call\"")
        || trimmed.contains("\"tool_result\"")
}

fn has_visible_answer_fields(value: &Value) -> bool {
    let Some(map) = value.as_object() else {
        return false;
    };
    if has_non_empty_field(map.get("answer"))
        || has_non_empty_field(map.get("content"))
        || has_non_empty_field(map.get("message"))
    {
        return true;
    }
    let Some(data) = map.get("data").and_then(Value::as_object) else {
        return false;
    };
    has_non_empty_field(data.get("answer"))
        || has_non_empty_field(data.get("content"))
        || has_non_empty_field(data.get("message"))
}

fn has_non_empty_field(value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(text)) => !text.trim().is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
        Some(Value::Null) | None => false,
        Some(_) => true,
    }
}

pub(crate) fn is_tool_payload_value(value: &Value) -> bool {
    match value {
        Value::String(text) => is_tool_payload_text(text),
        Value::Array(items) => items.iter().any(is_tool_payload_value),
        Value::Object(_) => is_tool_payload(value) && !has_visible_answer_fields(value),
        _ => false,
    }
}

fn is_tool_payload(value: &Value) -> bool {
    is_tool_call_payload(value) || is_tool_result_payload(value)
}

fn is_tool_call_payload(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            if map.contains_key("tool_calls")
                || map.contains_key("tool_call")
                || map.contains_key("function_call")
            {
                return true;
            }
            if let Some(Value::String(kind)) = map.get("type") {
                let lowered = kind.to_lowercase();
                if lowered.contains("tool") || lowered.contains("function") {
                    return true;
                }
            }
            let has_tool = map.contains_key("tool") || map.contains_key("tool_name");
            let has_name = map.contains_key("name");
            let has_args = map.contains_key("arguments")
                || map.contains_key("args")
                || map.contains_key("parameters");
            if (has_tool || has_name) && has_args {
                return true;
            }
            let Some(Value::Object(function)) = map.get("function") else {
                return false;
            };
            let function_has_name = function.contains_key("name") || function.contains_key("tool");
            let function_has_args =
                function.contains_key("arguments") || function.contains_key("args");
            function_has_name && function_has_args
        }
        Value::Array(items) => items.iter().any(is_tool_call_payload),
        _ => false,
    }
}

fn is_tool_result_payload(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            let tool = map.get("tool").and_then(Value::as_str).unwrap_or("").trim();
            if tool.is_empty() {
                return false;
            }
            let has_ok = map.get("ok").and_then(Value::as_bool).is_some();
            let has_data = map.contains_key("data") || map.contains_key("result");
            let has_error = map.contains_key("error");
            let has_timestamp = map
                .get("timestamp")
                .and_then(Value::as_str)
                .map(|text| !text.trim().is_empty())
                .unwrap_or(false);
            (has_ok && (has_data || has_error)) || (has_timestamp && has_data)
        }
        Value::Array(items) => items.iter().any(is_tool_result_payload),
        _ => false,
    }
}

pub(crate) fn normalize_message_content(value: &Value) -> String {
    match value {
        Value::String(text) => text.to_string(),
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    if !text.trim().is_empty() {
                        parts.push(text.trim().to_string());
                    }
                }
            }
            if parts.is_empty() {
                String::new()
            } else {
                parts.join("\n")
            }
        }
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

