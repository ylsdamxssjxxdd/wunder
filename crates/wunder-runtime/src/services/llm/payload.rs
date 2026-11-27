use super::{ChatMessage, extract_stream_text};
use crate::core::json_schema::{
    normalize_tool_input_schema, normalize_tool_input_schema_for_openai,
};
use crate::core::tool_args::normalize_tool_arguments_json as normalize_tool_arguments_json_lossy;
use crate::services::chat_attachments::parse_image_data_url;
use crate::tools::{extract_freeform_tool_input, is_freeform_tool_name};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

const CUSTOM_INPUT_KIND: &str = "input_kind";
const CUSTOM_INPUT_KIND_TEXT: &str = "text";

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

pub(super) fn build_responses_input(messages: &[ChatMessage], tools: Option<&[Value]>) -> Value {
    let declared_tools = declared_tool_kinds(tools);
    let mut input: Vec<Value> = Vec::new();
    let mut custom_tool_call_ids = HashSet::new();
    let mut fallback_call_index = 0usize;
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
            for call in calls.iter() {
                let index = fallback_call_index;
                fallback_call_index = fallback_call_index.saturating_add(1);
                if let Some(item) = tool_call_to_responses_item(call, index, &declared_tools) {
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

fn declared_tool_kinds(tools: Option<&[Value]>) -> HashMap<String, bool> {
    let mut kinds = HashMap::new();
    for tool in tools.unwrap_or(&[]) {
        let name = tool
            .get("name")
            .and_then(Value::as_str)
            .or_else(|| tool.pointer("/function/name").and_then(Value::as_str))
            .unwrap_or("")
            .trim();
        if name.is_empty() {
            continue;
        }
        let custom = tool
            .get("type")
            .and_then(Value::as_str)
            .map(|kind| kind.eq_ignore_ascii_case("custom"))
            .unwrap_or(false);
        kinds.insert(name.to_string(), custom);
    }
    kinds
}

fn declared_tool_kind(name: &str, kinds: &HashMap<String, bool>) -> Option<bool> {
    kinds.get(name).copied()
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

fn tool_call_to_responses_item(
    call: &Value,
    fallback_index: usize,
    declared_tools: &HashMap<String, bool>,
) -> Option<Value> {
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
    let raw_arguments = map
        .get("custom_input")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            map.get("function")
                .and_then(|value| value.get("arguments"))
                .and_then(Value::as_str)
        })
        .or_else(|| map.get("arguments").and_then(Value::as_str))
        .or_else(|| map.get("input").and_then(Value::as_str))
        .unwrap_or("{}")
        .to_string();
    let custom = match declared_tool_kind(&name, declared_tools) {
        Some(custom) => custom,
        None => has_custom_input_marker(map) || is_freeform_tool_name(&name),
    };
    let call_id = map
        .get("id")
        .or_else(|| map.get("call_id"))
        .or_else(|| map.get("tool_call_id"))
        .or_else(|| map.get("toolCallId"))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("call_{}", fallback_index + 1));
    if custom {
        // custom 工具的输入是补丁正文本身，不能再经过 JSON 参数修复；
        // 否则合法的 JSON 字符串会被包装成 `{"value": ...}`。
        let input = map
            .get("custom_input")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .or_else(|| extract_freeform_tool_input(&raw_arguments))
            .unwrap_or(raw_arguments);
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
        "arguments": normalize_tool_arguments_json(&raw_arguments),
    }))
}

fn has_custom_input_marker(map: &serde_json::Map<String, Value>) -> bool {
    map.get(CUSTOM_INPUT_KIND)
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case(CUSTOM_INPUT_KIND_TEXT))
}

pub(super) fn sanitize_chat_messages(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    messages
        .iter()
        .map(|message| ChatMessage {
            role: message.role.clone(),
            content: sanitize_message_content(&message.content),
            reasoning_content: message.reasoning_content.clone(),
            anthropic_thinking_blocks: message.anthropic_thinking_blocks.clone(),
            tool_calls: message
                .tool_calls
                .as_ref()
                .map(strip_internal_tool_call_fields),
            tool_call_id: message.tool_call_id.clone(),
        })
        .collect()
}

/// ChatCompletions 只认识 `type:"function"`。内部为了保留 custom 语义而附加的
/// `input_kind`/`custom_input` 必须在这里剥掉；如果将来直接存 `custom_tool_call`
/// 形状，也在这里降级成 `{"input": raw}` 的普通函数调用。
fn strip_internal_tool_call_fields(value: &Value) -> Value {
    match value {
        Value::Array(items) => {
            Value::Array(items.iter().map(strip_internal_tool_call_fields).collect())
        }
        Value::Object(map) => {
            let custom_marker = has_custom_input_marker(&map);
            let mut map = map.clone();
            map.remove(CUSTOM_INPUT_KIND);
            map.remove("custom_input");
            let item_type = map
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            if item_type == "custom_tool_call" {
                let name = map
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let input = map
                    .get("input")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let arguments = serde_json::to_string(&json!({ "input": input }))
                    .unwrap_or_else(|_| "{}".to_string());
                let mut call = json!({
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": arguments,
                    }
                });
                if let Some(id) = map
                    .get("id")
                    .or_else(|| map.get("call_id"))
                    .and_then(Value::as_str)
                {
                    call["id"] = Value::String(id.to_string());
                }
                return call;
            }
            if item_type == "function_call" {
                let name = map
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let arguments = map
                    .get("arguments")
                    .and_then(Value::as_str)
                    .unwrap_or("{}")
                    .to_string();
                let arguments = if is_freeform_tool_name(&name) {
                    chat_arguments_with_custom_input(&arguments)
                } else {
                    arguments
                };
                let mut call = json!({
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": arguments,
                    }
                });
                if let Some(id) = map
                    .get("id")
                    .or_else(|| map.get("call_id"))
                    .and_then(Value::as_str)
                {
                    call["id"] = Value::String(id.to_string());
                }
                return call;
            }
            if item_type == "function" || item_type.is_empty() {
                if let Some(function) = map.get_mut("function").and_then(Value::as_object_mut) {
                    let name = function
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    let arguments = function
                        .get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("{}")
                        .to_string();
                    if custom_marker || is_freeform_tool_name(&name) {
                        let arguments = chat_arguments_with_custom_input(&arguments);
                        function.insert("arguments".to_string(), Value::String(arguments));
                    }
                }
            }
            Value::Object(map)
        }
        other => other.clone(),
    }
}

/// Responses 侧的 custom 工具把补丁正文放在 `arguments` 里，降级到
/// ChatCompletions 时必须重新包装成 `{"input": raw}`，否则 Chat 端点会收到
/// 一段不是 JSON 的 arguments。
fn chat_arguments_with_custom_input(arguments: &str) -> String {
    match serde_json::from_str::<Value>(arguments) {
        Ok(Value::Object(_)) => arguments.to_string(),
        Ok(Value::String(raw)) => json!({ "input": raw }).to_string(),
        _ => json!({ "input": arguments }).to_string(),
    }
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
    use serde_json::{Value, json};
    use std::collections::HashMap;

    #[test]
    fn sanitize_chat_messages_drops_invalid_image_parts_from_history() {
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: json!([
                {"type": "text", "text": "keep"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,invalid"}}
            ]),
            reasoning_content: None,
            anthropic_thinking_blocks: None,
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
            &HashMap::new(),
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
    fn text_edit_responses_history_keeps_structured_arguments() {
        let args = json!({"command":"insert", "file_path":"sample.txt", "insert_line":0, "new_string":"text"});
        for name in ["edit", "文本编辑", "str_replace_editor"] {
            let item = tool_call_to_responses_item(
                &json!({
                    "id":"call_edit", "function":{"name":name, "arguments":args.to_string()}
                }),
                0,
                &HashMap::new(),
            )
            .unwrap();
            assert_eq!(item["type"], "function_call");
            assert_eq!(
                serde_json::from_str::<Value>(item["arguments"].as_str().unwrap()).unwrap(),
                args
            );
        }
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
            &HashMap::new(),
        )
        .expect("responses item");

        assert_eq!(item["type"], "custom_tool_call");
        assert_eq!(item["call_id"], "call_patch");
        assert_eq!(item["input"], "*** Begin Patch\n*** End Patch");
    }

    #[test]
    fn apply_patch_call_stays_function_when_tools_declare_it_as_function() {
        let mut declared = HashMap::new();
        declared.insert("apply_patch".to_string(), false);
        let item = tool_call_to_responses_item(
            &json!({
                "id": "call_patch",
                "function": {
                    "name": "apply_patch",
                    "arguments": "{\"input\":\"*** Begin Patch\\n*** End Patch\"}"
                }
            }),
            0,
            &declared,
        )
        .expect("responses item");

        assert_eq!(item["type"], "function_call");
        assert_eq!(
            serde_json::from_str::<Value>(item["arguments"].as_str().unwrap()).unwrap(),
            json!({ "input": "*** Begin Patch\n*** End Patch" })
        );
    }

    #[test]
    fn custom_tool_call_keeps_json_string_patch_as_raw_input() {
        let mut declared = HashMap::new();
        declared.insert("apply_patch".to_string(), true);
        let arguments = serde_json::to_string("*** Begin Patch\n*** End Patch").unwrap();
        let item = tool_call_to_responses_item(
            &json!({
                "id": "call_patch",
                "function": {
                    "name": "apply_patch",
                    "arguments": arguments,
                }
            }),
            0,
            &declared,
        )
        .expect("responses item");

        assert_eq!(item["type"], "custom_tool_call");
        assert_eq!(item["input"], "*** Begin Patch\n*** End Patch");
    }

    #[test]
    fn custom_input_marker_bypasses_json_argument_repair() {
        let item = tool_call_to_responses_item(
            &json!({
                "id": "call_patch",
                "input_kind": "text",
                "function": {
                    "name": "apply_patch",
                    "arguments": "*** Begin Patch\n*** End Patch",
                }
            }),
            0,
            &HashMap::new(),
        )
        .expect("responses item");

        assert_eq!(item["type"], "custom_tool_call");
        assert_eq!(item["input"], "*** Begin Patch\n*** End Patch");
    }

    #[test]
    fn sanitize_chat_messages_strips_internal_custom_marker() {
        let messages = vec![ChatMessage {
            role: "assistant".to_string(),
            content: json!(""),
            reasoning_content: None,
            anthropic_thinking_blocks: None,
            tool_calls: Some(json!([{
                "id": "call_patch",
                "type": "function",
                "input_kind": "text",
                "function": {
                    "name": "apply_patch",
                    "arguments": "{\"input\":\"*** Begin Patch\\n*** End Patch\"}"
                }
            }])),
            tool_call_id: None,
        }];

        let sanitized = sanitize_chat_messages(&messages);
        let call = &sanitized[0]
            .tool_calls
            .as_ref()
            .unwrap()
            .as_array()
            .unwrap()[0];
        assert!(call.get("input_kind").is_none());
        assert_eq!(call["type"], "function");
    }

    #[test]
    fn sanitize_chat_messages_wraps_raw_custom_arguments_for_chat() {
        let messages = vec![ChatMessage {
            role: "assistant".to_string(),
            content: json!(""),
            reasoning_content: None,
            anthropic_thinking_blocks: None,
            tool_calls: Some(json!([{
                "id": "call_patch",
                "type": "function",
                "input_kind": "text",
                "function": {
                    "name": "apply_patch",
                    "arguments": "*** Begin Patch\n*** End Patch"
                }
            }])),
            tool_call_id: None,
        }];

        let sanitized = sanitize_chat_messages(&messages);
        let call = &sanitized[0]
            .tool_calls
            .as_ref()
            .unwrap()
            .as_array()
            .unwrap()[0];
        let arguments = call["function"]["arguments"].as_str().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(arguments).unwrap(),
            json!({ "input": "*** Begin Patch\n*** End Patch" })
        );
    }

    #[test]
    fn sanitize_chat_messages_converts_function_call_shape() {
        let messages = vec![ChatMessage {
            role: "assistant".to_string(),
            content: json!(""),
            reasoning_content: None,
            anthropic_thinking_blocks: None,
            tool_calls: Some(json!([{
                "type": "function_call",
                "call_id": "call_edit",
                "name": "edit",
                "arguments": "{\"file_path\":\"a.txt\"}"
            }])),
            tool_call_id: None,
        }];

        let sanitized = sanitize_chat_messages(&messages);
        let call = &sanitized[0]
            .tool_calls
            .as_ref()
            .unwrap()
            .as_array()
            .unwrap()[0];
        assert_eq!(call["type"], "function");
        assert_eq!(call["id"], "call_edit");
        assert_eq!(call["function"]["name"], "edit");
        assert_eq!(
            serde_json::from_str::<Value>(call["function"]["arguments"].as_str().unwrap())
                .unwrap()["file_path"],
            "a.txt"
        );
    }

    #[test]
    fn build_responses_input_roundtrips_custom_tool_outputs() {
        let messages = vec![
            ChatMessage {
                role: "assistant".to_string(),
                content: json!(""),
                reasoning_content: None,
                anthropic_thinking_blocks: None,
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
                anthropic_thinking_blocks: None,
                tool_calls: None,
                tool_call_id: Some("call_patch".to_string()),
            },
        ];

        let input = build_responses_input(&messages, None);
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
