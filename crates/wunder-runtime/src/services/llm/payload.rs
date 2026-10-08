use super::{extract_stream_text, ChatMessage};
use crate::core::json_schema::{
    normalize_tool_input_schema, normalize_tool_input_schema_for_openai,
};
use crate::core::tool_args::normalize_tool_arguments_json as normalize_tool_arguments_json_lossy;
use crate::services::chat_attachments::parse_image_data_url;
use crate::tools::{extract_freeform_tool_input, is_freeform_tool_name};
use serde_json::{json, Value};
use std::collections::HashSet;

pub(super) fn normalize_chat_tool_definition(
    tool: &Value,
    openai_top_level_schema_guard: bool,
) -> Value {
    let normalize_parameters = |schema: Option<&Value>| {
        if openai_top_level_schema_guard {
            normalize_tool_input_schema_for_openai(schema)
        } else {
            normalize_tool_input_schema(schema)
        }
    };
    if let Some(function) = tool.get("function").and_then(Value::as_object) {
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if name.is_empty() {
            return tool.clone();
        }
        let description = function
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("");
        let parameters = normalize_parameters(function.get("parameters"));
        return json!({
            "type": "function",
            "function": {
                "name": name,
                "description": description,
                "parameters": parameters
            }
        });
    }

    let name = tool
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if name.is_empty() {
        return tool.clone();
    }
    let description = tool
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    let parameters = normalize_parameters(tool.get("parameters"));
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters
        }
    })
}

pub(super) fn normalize_responses_tool_definition(
    tool: &Value,
    openai_top_level_schema_guard: bool,
) -> Value {
    let normalize_parameters = |schema: Option<&Value>| {
        if openai_top_level_schema_guard {
            normalize_tool_input_schema_for_openai(schema)
        } else {
            normalize_tool_input_schema(schema)
        }
    };
    let tool_type = tool
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if tool_type.eq_ignore_ascii_case("custom") {
        let name = tool
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if name.is_empty() {
            return tool.clone();
        }
        let description = tool
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("");
        let format = tool.get("format").cloned().unwrap_or(Value::Null);
        return json!({
            "type": "custom",
            "name": name,
            "description": description,
            "format": format,
        });
    }
    if let Some(function) = tool.get("function").and_then(Value::as_object) {
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if name.is_empty() {
            return tool.clone();
        }
        let description = function
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("");
        let parameters = normalize_parameters(function.get("parameters"));
        return json!({
            "type": "function",
            "name": name,
            "description": description,
            "parameters": parameters
        });
    }

    let name = tool
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if name.is_empty() {
        return tool.clone();
    }
    let description = tool
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    let parameters = normalize_parameters(tool.get("parameters"));
    json!({
        "type": "function",
        "name": name,
        "description": description,
        "parameters": parameters
    })
}

pub(super) fn build_responses_input(messages: &[ChatMessage]) -> Value {
    let mut input: Vec<Value> = Vec::new();
    let mut custom_tool_call_ids = HashSet::new();
    for message in messages {
        let role = message.role.trim();
        if role.eq_ignore_ascii_case("tool") {
            if let Some(call_id) = message
                .tool_call_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                let output = extract_content_text(&message.content);
                let output_type = if custom_tool_call_ids.contains(call_id) {
                    "custom_tool_call_output"
                } else {
                    "function_call_output"
                };
                input.push(json!({
                    "type": output_type,
                    "call_id": call_id,
                    "output": output,
                }));
                continue;
            }
        }

        let normalized_role = normalize_responses_role(role);
        let content = convert_responses_content(&message.content);
        input.push(json!({
            "role": normalized_role,
            "content": content,
        }));

        if let Some(tool_calls) = message.tool_calls.as_ref() {
            let calls = extract_tool_calls_list(tool_calls);
            for (idx, call) in calls.iter().enumerate() {
                if let Some(item) = tool_call_to_responses_item(call, idx) {
                    if item["type"] == "custom_tool_call" {
                        if let Some(call_id) = item.get("call_id").and_then(Value::as_str) {
                            custom_tool_call_ids.insert(call_id.to_string());
                        }
                    }
                    input.push(item);
                }
            }
        }
    }
    Value::Array(input)
}

fn normalize_responses_role(role: &str) -> &'static str {
    match role.trim().to_ascii_lowercase().as_str() {
        "system" => "system",
        "assistant" => "assistant",
        "developer" => "developer",
        _ => "user",
    }
}

fn extract_content_text(content: &Value) -> String {
    let text = extract_stream_text(Some(content));
    if !text.is_empty() {
        return text;
    }
    content
        .as_str()
        .map(ToString::to_string)
        .unwrap_or_else(|| content.to_string())
}

fn convert_responses_content(content: &Value) -> Value {
    match content {
        Value::Null => Value::String(String::new()),
        Value::String(_) => content.clone(),
        Value::Array(parts) => Value::Array(
            parts
                .iter()
                .filter_map(convert_responses_content_part)
                .collect(),
        ),
        Value::Object(_) => convert_responses_content_part(content)
            .map(|part| Value::Array(vec![part]))
            .unwrap_or_else(|| Value::String(content.to_string())),
        other => Value::String(other.to_string()),
    }
}

fn convert_responses_content_part(part: &Value) -> Option<Value> {
    match part {
        Value::String(text) => Some(json!({ "type": "input_text", "text": text })),
        Value::Object(map) => {
            let raw_type = map
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            if matches!(
                raw_type.as_str(),
                "input_text" | "input_image" | "input_file"
            ) {
                return Some(part.clone());
            }
            if matches!(raw_type.as_str(), "text" | "output_text") {
                if let Some(text) = map.get("text").and_then(Value::as_str) {
                    return Some(json!({ "type": "input_text", "text": text }));
                }
            }
            if raw_type == "image_url" || map.contains_key("image_url") || raw_type == "input_image"
            {
                let url = map
                    .get("image_url")
                    .and_then(|value| match value {
                        Value::String(text) => Some(text.to_string()),
                        Value::Object(obj) => obj
                            .get("url")
                            .and_then(Value::as_str)
                            .map(ToString::to_string),
                        _ => None,
                    })
                    .or_else(|| {
                        map.get("url")
                            .and_then(Value::as_str)
                            .map(ToString::to_string)
                    });
                if let Some(url) = url {
                    if url.starts_with("data:image/") && parse_image_data_url(&url, None).is_none()
                    {
                        return None;
                    }
                    let mut item = json!({ "type": "input_image", "image_url": url });
                    if let Some(detail) = map.get("detail") {
                        item["detail"] = detail.clone();
                    } else {
                        item["detail"] = json!("auto");
                    }
                    return Some(item);
                }
            }
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                return Some(json!({ "type": "input_text", "text": text }));
            }
            None
        }
        _ => None,
    }
}

fn extract_tool_calls_list(value: &Value) -> Vec<Value> {
    match value {
        Value::Array(items) => items.clone(),
        Value::Object(_) => vec![value.clone()],
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .map(|parsed| extract_tool_calls_list(&parsed))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

pub(super) fn normalize_tool_arguments_json(arguments: &str) -> String {
    normalize_tool_arguments_json_lossy(arguments)
}

fn tool_call_to_responses_item(call: &Value, fallback_index: usize) -> Option<Value> {
    let Value::Object(map) = call else {
        return None;
    };
    let name = map
        .get("function")
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .or_else(|| map.get("name").and_then(Value::as_str))
        .unwrap_or("")
        .trim()
        .to_string();
    if name.is_empty() {
        return None;
    }
    let arguments = map
        .get("function")
        .and_then(|value| value.get("arguments"))
        .and_then(Value::as_str)
        .or_else(|| map.get("arguments").and_then(Value::as_str))
        .map(normalize_tool_arguments_json)
        .unwrap_or_else(|| "{}".to_string());
    let call_id = map
        .get("id")
        .or_else(|| map.get("call_id"))
        .or_else(|| map.get("tool_call_id"))
        .or_else(|| map.get("toolCallId"))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("call_{}", fallback_index + 1));
    if is_freeform_tool_name(&name) {
        let input = extract_freeform_tool_input(&arguments).unwrap_or(arguments);
        return Some(json!({
            "type": "custom_tool_call",
            "call_id": call_id,
            "name": name,
            "input": input,
        }));
    }
    Some(json!({
        "type": "function_call",
        "call_id": call_id,
        "name": name,
        "arguments": arguments,
    }))
}

pub(super) fn sanitize_chat_messages(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    messages
        .iter()
        .map(|message| ChatMessage {
            role: message.role.clone(),
            content: sanitize_message_content(&message.content),
            reasoning_content: message.reasoning_content.clone(),
            tool_calls: message.tool_calls.clone(),
            tool_call_id: message.tool_call_id.clone(),
        })
        .collect()
}

fn sanitize_message_content(content: &Value) -> Value {
    match content {
        Value::Array(parts) => {
            let sanitized = parts
                .iter()
                .filter_map(|part| match part {
                    Value::Object(map) => {
                        let raw_type = map
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_ascii_lowercase();
                        if raw_type == "image_url" || map.contains_key("image_url") {
                            let data_url = map
                                .get("image_url")
                                .and_then(|value| match value {
                                    Value::String(text) => Some(text.as_str()),
                                    Value::Object(obj) => obj.get("url").and_then(Value::as_str),
                                    _ => None,
                                })
                                .or_else(|| map.get("url").and_then(Value::as_str));
                            if let Some(data_url) = data_url {
                                if data_url.starts_with("data:image/")
                                    && parse_image_data_url(data_url, None).is_none()
                                {
                                    return None;
                                }
                            }
                        }
                        Some(part.clone())
                    }
                    _ => Some(part.clone()),
                })
                .collect::<Vec<_>>();
            Value::Array(sanitized)
        }
        _ => content.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_responses_input, normalize_chat_tool_definition, normalize_responses_tool_definition,
        normalize_tool_arguments_json, sanitize_chat_messages, tool_call_to_responses_item,
    };
    use crate::services::llm::ChatMessage;
    use serde_json::{json, Value};

    #[test]
    fn sanitize_chat_messages_drops_invalid_image_parts_from_history() {
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: json!([
                {"type": "text", "text": "keep"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,invalid"}}
            ]),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        }];

        let sanitized = sanitize_chat_messages(&messages);
        let parts = sanitized[0].content.as_array().expect("array content");
        assert_eq!(parts, &[json!({"type": "text", "text": "keep"})]);
    }

    #[test]
    fn normalize_tool_arguments_json_wraps_invalid_payload_as_raw_object() {
        let normalized = normalize_tool_arguments_json("python3 -c \"print('hello')\"");
        assert_eq!(
            serde_json::from_str::<Value>(&normalized).expect("valid json"),
            json!({ "raw": "python3 -c \"print('hello')\"" })
        );
    }

    #[test]
    fn tool_call_to_responses_item_sanitizes_invalid_arguments_json() {
        let item = tool_call_to_responses_item(
            &json!({
                "id": "call_1",
                "function": {
                    "name": "execute_command",
                    "arguments": "python3 -c \"print('hello')\""
                }
            }),
            0,
        )
        .expect("responses item");

        assert_eq!(item["type"], "function_call");
        assert_eq!(item["call_id"], "call_1");
        assert_eq!(item["name"], "execute_command");
        assert_eq!(
            serde_json::from_str::<Value>(item["arguments"].as_str().unwrap()).unwrap(),
            json!({ "raw": "python3 -c \"print('hello')\"" })
        );
    }

    #[test]
    fn tool_call_to_responses_item_converts_freeform_tool_to_custom_call() {
        let item = tool_call_to_responses_item(
            &json!({
                "id": "call_patch",
                "function": {
                    "name": "apply_patch",
                    "arguments": "{\"input\":\"*** Begin Patch\\n*** End Patch\"}"
                }
            }),
            0,
        )
        .expect("responses item");

        assert_eq!(item["type"], "custom_tool_call");
        assert_eq!(item["call_id"], "call_patch");
        assert_eq!(item["input"], "*** Begin Patch\n*** End Patch");
    }

    #[test]
    fn build_responses_input_roundtrips_custom_tool_outputs() {
        let messages = vec![
            ChatMessage {
                role: "assistant".to_string(),
                content: json!(""),
                reasoning_content: None,
                tool_calls: Some(json!([{
                    "id": "call_patch",
                    "type": "function",
                    "function": {
                        "name": "apply_patch",
                        "arguments": "{\"input\":\"*** Begin Patch\\n*** End Patch\"}"
                    }
                }])),
                tool_call_id: None,
            },
            ChatMessage {
                role: "tool".to_string(),
                content: json!("applied"),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: Some("call_patch".to_string()),
            },
        ];

        let input = build_responses_input(&messages);
        let items = input.as_array().expect("responses input array");

        assert_eq!(items[1]["type"], "custom_tool_call");
        assert_eq!(items[2]["type"], "custom_tool_call_output");
        assert_eq!(items[2]["call_id"], "call_patch");
    }

    #[test]
    fn normalize_responses_tool_definition_flattens_chat_shape_and_fixes_schema() {
        let tool = json!({
            "type": "function",
            "function": {
                "name": "apply_patch",
                "description": "Apply patch",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "input": {
                            "type": "string"
                        }
                    }
                }
            }
        });
        let normalized = normalize_responses_tool_definition(&tool, false);
        assert_eq!(normalized["type"], "function");
        assert_eq!(normalized["name"], "apply_patch");
        assert_eq!(
            normalized["parameters"]["properties"]["input"]["type"],
            "string"
        );
    }

    #[test]
    fn normalize_responses_tool_definition_preserves_custom_tool_format() {
        let tool = json!({
            "type": "custom",
            "name": "apply_patch",
            "description": "Apply patch",
            "format": {
                "type": "grammar",
                "syntax": "lark",
                "definition": "start: \"ok\""
            }
        });

        let normalized = normalize_responses_tool_definition(&tool, true);
        assert_eq!(normalized["type"], "custom");
        assert_eq!(normalized["name"], "apply_patch");
        assert_eq!(normalized["format"]["syntax"], "lark");
    }

    #[test]
    fn normalize_chat_tool_definition_wraps_responses_shape_and_fixes_schema() {
        let tool = json!({
            "type": "function",
            "name": "apply_patch",
            "description": "Apply patch",
            "parameters": {
                "type": "object",
                "properties": {
                    "input": {
                        "type": "string"
                    }
                }
            }
        });
        let normalized = normalize_chat_tool_definition(&tool, false);
        assert_eq!(normalized["type"], "function");
        assert_eq!(normalized["function"]["name"], "apply_patch");
        assert_eq!(
            normalized["function"]["parameters"]["properties"]["input"]["type"],
            "string"
        );
    }

    #[test]
    fn normalize_responses_tool_definition_strips_forbidden_top_level_for_openai_only() {
        let tool = json!({
            "type": "function",
            "name": "apply_patch",
            "description": "Apply patch",
            "parameters": {
                "type": "object",
                "properties": {
                    "input": {"type": "string"}
                },
                "allOf": [
                    {"required": ["input"]}
                ]
            }
        });
        let preserved = normalize_responses_tool_definition(&tool, false);
        let stripped = normalize_responses_tool_definition(&tool, true);

        assert!(preserved["parameters"].get("allOf").is_some());
        assert!(stripped["parameters"].get("allOf").is_none());
    }
}
