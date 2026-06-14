use super::payload::normalize_tool_arguments_json;
use super::{extract_stream_text, truncate_text, ChatMessage};
use crate::core::json_schema::normalize_tool_input_schema;
use crate::core::tool_args::sanitize_tool_call_payload;
use crate::schemas::TokenUsage;
use serde_json::{json, Value};

pub(super) fn normalize_usage(raw: Option<&Value>) -> Option<TokenUsage> {
    let raw = raw?;
    let Value::Object(map) = raw else {
        return None;
    };
    let to_u64 = |value: Option<&Value>| -> Option<u64> {
        match value {
            Some(Value::Number(num)) => num.as_u64(),
            Some(Value::String(text)) => text.trim().parse::<u64>().ok(),
            _ => None,
        }
    };
    let parse_reasoning_tokens = |value: Option<&Value>| -> Option<u64> {
        let Value::Object(details) = value? else {
            return None;
        };
        to_u64(details.get("reasoning_tokens"))
            .or_else(|| to_u64(details.get("reasoningTokens")))
            .or_else(|| {
                details
                    .get("reasoning")
                    .and_then(Value::as_object)
                    .and_then(|reasoning| {
                        to_u64(reasoning.get("tokens"))
                            .or_else(|| to_u64(reasoning.get("token_count")))
                    })
            })
    };
    let input = to_u64(map.get("input_tokens"))
        .or_else(|| to_u64(map.get("prompt_tokens")))
        .unwrap_or(0);
    let raw_output = to_u64(map.get("output_tokens"))
        .or_else(|| to_u64(map.get("completion_tokens")))
        .unwrap_or(0);
    let reasoning_tokens = to_u64(map.get("reasoning_tokens"))
        .or_else(|| to_u64(map.get("reasoningTokens")))
        .or_else(|| parse_reasoning_tokens(map.get("output_tokens_details")))
        .or_else(|| parse_reasoning_tokens(map.get("outputTokensDetails")))
        .or_else(|| parse_reasoning_tokens(map.get("completion_tokens_details")))
        .or_else(|| parse_reasoning_tokens(map.get("completionTokensDetails")))
        .unwrap_or(0);
    let output = raw_output.saturating_sub(reasoning_tokens);
    let total = to_u64(map.get("total_tokens")).unwrap_or(input.saturating_add(raw_output));
    if input == 0 && output == 0 && total == 0 {
        return None;
    }
    Some(TokenUsage {
        input,
        output,
        total,
    })
}

pub(super) fn build_anthropic_messages(messages: &[ChatMessage]) -> (Option<String>, Vec<Value>) {
    let mut system_parts = Vec::new();
    let mut output = Vec::new();

    for message in messages {
        let role = message.role.trim().to_ascii_lowercase();
        match role.as_str() {
            "system" => {
                let text = flatten_message_text(&message.content);
                if !text.trim().is_empty() {
                    system_parts.push(text);
                }
            }
            "assistant" => {
                let mut blocks = Vec::new();
                let text = flatten_message_text(&message.content);
                if !text.trim().is_empty() {
                    blocks.push(json!({
                        "type": "text",
                        "text": text,
                    }));
                }
                if let Some(tool_payload) = message.tool_calls.as_ref() {
                    for (index, call) in extract_openai_tool_calls(tool_payload).iter().enumerate()
                    {
                        if let Some(tool_use) =
                            openai_tool_call_to_anthropic_tool_use_block(call, index)
                        {
                            blocks.push(tool_use);
                        }
                    }
                }
                append_anthropic_message(&mut output, "assistant", blocks);
            }
            "tool" => {
                let tool_use_id = message
                    .tool_call_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let text = flatten_message_text(&message.content);
                let blocks = if let Some(tool_use_id) = tool_use_id {
                    vec![json!({
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": text,
                    })]
                } else if text.trim().is_empty() {
                    Vec::new()
                } else {
                    vec![json!({
                        "type": "text",
                        "text": text,
                    })]
                };
                append_anthropic_message(&mut output, "user", blocks);
            }
            _ => {
                let text = flatten_message_text(&message.content);
                if text.trim().is_empty() {
                    continue;
                }
                append_anthropic_message(
                    &mut output,
                    "user",
                    vec![json!({
                        "type": "text",
                        "text": text,
                    })],
                );
            }
        }
    }

    if output.is_empty() {
        output.push(json!({
            "role": "user",
            "content": [{
                "type": "text",
                "text": "",
            }],
        }));
    }

    let system = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n\n"))
    };
    (system, output)
}

fn append_anthropic_message(messages: &mut Vec<Value>, role: &str, blocks: Vec<Value>) {
    if blocks.is_empty() {
        return;
    }
    if let Some(last) = messages.last_mut() {
        let same_role = last
            .get("role")
            .and_then(Value::as_str)
            .map(|value| value == role)
            .unwrap_or(false);
        if same_role {
            if let Some(existing) = last.get_mut("content").and_then(Value::as_array_mut) {
                existing.extend(blocks);
                return;
            }
        }
    }
    messages.push(json!({
        "role": role,
        "content": blocks,
    }));
}

fn flatten_message_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.to_string(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                if let Some(text) = item.as_str() {
                    return Some(text.to_string());
                }
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    return Some(text.to_string());
                }
                if let Some(text) = item.get("content").and_then(Value::as_str) {
                    return Some(text.to_string());
                }
                None
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                return text.to_string();
            }
            if let Some(content) = map.get("content") {
                return flatten_message_text(content);
            }
            String::new()
        }
        _ => String::new(),
    }
}

fn extract_openai_tool_calls(value: &Value) -> Vec<Value> {
    match value {
        Value::Array(items) => items.clone(),
        Value::Object(map) => {
            if let Some(items) = map.get("tool_calls").and_then(Value::as_array) {
                return items.clone();
            }
            if map.get("function").is_some() {
                return vec![value.clone()];
            }
            if map.get("name").and_then(Value::as_str).is_some()
                && (map.get("arguments").is_some() || map.get("input").is_some())
            {
                return vec![value.clone()];
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn openai_tool_call_to_anthropic_tool_use_block(
    call: &Value,
    fallback_index: usize,
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
    let tool_use_id = map
        .get("id")
        .or_else(|| map.get("call_id"))
        .or_else(|| map.get("tool_call_id"))
        .or_else(|| map.get("toolCallId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("toolu_{}", fallback_index + 1));

    let args_value = map
        .get("function")
        .and_then(|value| value.get("arguments"))
        .and_then(Value::as_str)
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .or_else(|| map.get("input").cloned())
        .unwrap_or_else(|| json!({}));
    let input = if args_value.is_object() {
        args_value
    } else {
        json!({ "input": args_value })
    };

    Some(json!({
        "type": "tool_use",
        "id": tool_use_id,
        "name": name,
        "input": input,
    }))
}

pub(super) fn openai_tool_definition_to_anthropic_tool(tool: &Value) -> Option<Value> {
    let Value::Object(map) = tool else {
        return None;
    };
    if let Some(function) = map.get("function").and_then(Value::as_object) {
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        let description = function
            .get("description")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let mut input_schema = normalize_tool_input_schema(function.get("parameters"));
        if !input_schema.is_object() {
            input_schema = json!({ "type": "object", "properties": {} });
        }
        let mut payload = json!({
            "name": name,
            "input_schema": input_schema,
        });
        if let Some(description) = description {
            payload["description"] = Value::String(description);
        }
        return Some(payload);
    }

    if map.get("name").and_then(Value::as_str).is_some() && map.get("input_schema").is_some() {
        return Some(tool.clone());
    }
    None
}

fn anthropic_tool_use_block_to_openai(block: &Value, fallback_index: usize) -> Option<Value> {
    let Value::Object(map) = block else {
        return None;
    };
    let name = map
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let id = map
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("call_{}", fallback_index + 1));
    let arguments = map
        .get("input")
        .and_then(|value| serde_json::to_string(value).ok())
        .unwrap_or_else(|| "{}".to_string());
    Some(json!({
        "type": "function",
        "id": id,
        "function": {
            "name": name,
            "arguments": normalize_tool_arguments_json(&arguments),
        }
    }))
}

pub(super) fn parse_anthropic_body(body: &Value) -> (String, String, Option<Value>) {
    let mut content = String::new();
    let mut reasoning = String::new();
    let mut tool_calls = Vec::new();

    if let Some(blocks) = body.get("content").and_then(Value::as_array) {
        for (index, block) in blocks.iter().enumerate() {
            let block_type = block
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            match block_type.as_str() {
                "text" => {
                    let text = block.get("text").and_then(Value::as_str).unwrap_or("");
                    if !text.is_empty() {
                        content.push_str(text);
                    }
                }
                "thinking" => {
                    let text = block
                        .get("thinking")
                        .or_else(|| block.get("text"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if !text.is_empty() {
                        if !reasoning.is_empty() {
                            reasoning.push('\n');
                        }
                        reasoning.push_str(text);
                    }
                }
                "tool_use" => {
                    if let Some(tool_call) = anthropic_tool_use_block_to_openai(block, index) {
                        tool_calls.push(tool_call);
                    }
                }
                _ => {}
            }
        }
    }

    if content.trim().is_empty() {
        if let Some(text) = body.get("completion").and_then(Value::as_str) {
            content = text.to_string();
        }
    }

    let tool_calls = if tool_calls.is_empty() {
        None
    } else {
        Some(Value::Array(tool_calls))
    };
    (content, reasoning, tool_calls)
}

pub(super) fn parse_chat_completion_body(body: &Value) -> (String, String, Option<Value>) {
    let message = body
        .get("choices")
        .and_then(|value| value.get(0))
        .and_then(|value| value.get("message"))
        .cloned()
        .unwrap_or(Value::Null);
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let reasoning = message
        .get("reasoning_content")
        .or_else(|| message.get("reasoning"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let tool_calls = extract_tool_calls(&message);
    (content, reasoning, tool_calls)
}

pub(super) fn parse_responses_body(body: &Value) -> (String, String, Option<Value>) {
    let response = body.get("response").unwrap_or(body);
    let (content, reasoning, tool_calls) = extract_responses_output(response);
    let tool_calls = if tool_calls.is_empty() {
        None
    } else {
        Some(Value::Array(tool_calls))
    };
    (content, reasoning, tool_calls)
}

pub(super) fn extract_responses_output(response: &Value) -> (String, String, Vec<Value>) {
    let mut content = String::new();
    let mut reasoning = String::new();
    let mut tool_calls: Vec<Value> = Vec::new();

    let output_items = response
        .get("output")
        .and_then(Value::as_array)
        .or_else(|| response.as_array());
    if let Some(items) = output_items {
        for (idx, item) in items.iter().enumerate() {
            let item_type = item
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            match item_type.as_str() {
                "message" => {
                    let text = extract_response_message_text(item);
                    if !text.is_empty() {
                        content.push_str(&text);
                    }
                }
                "reasoning" => {
                    let text = extract_response_reasoning(item);
                    if !text.is_empty() {
                        if !reasoning.is_empty() {
                            reasoning.push('\n');
                        }
                        reasoning.push_str(&text);
                    }
                }
                "function_call" => {
                    if let Some(tool_call) = response_tool_call_to_openai(item, idx) {
                        tool_calls.push(tool_call);
                    }
                }
                "custom_tool_call" => {
                    if let Some(tool_call) = response_tool_call_to_openai(item, idx) {
                        tool_calls.push(tool_call);
                    }
                }
                _ => {}
            }
        }
    }

    if content.trim().is_empty() {
        if let Some(text) = response.get("output_text").and_then(Value::as_str) {
            content = text.to_string();
        } else if let Some(text) = response.get("text").and_then(Value::as_str) {
            content = text.to_string();
        }
    }

    (content, reasoning, tool_calls)
}

fn extract_response_message_text(item: &Value) -> String {
    if let Some(content) = item.get("content") {
        let text = extract_stream_text(Some(content));
        if !text.is_empty() {
            return text;
        }
    }
    item.get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn extract_response_reasoning(item: &Value) -> String {
    if let Some(summary) = item.get("summary") {
        let text = extract_stream_text(Some(summary));
        if !text.is_empty() {
            return text;
        }
    }
    item.get("text")
        .or_else(|| item.get("summary_text"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn response_tool_call_to_openai(item: &Value, fallback_index: usize) -> Option<Value> {
    let Value::Object(map) = item else {
        return None;
    };
    let name = map
        .get("name")
        .and_then(Value::as_str)
        .or_else(|| {
            map.get("function")
                .and_then(|value| value.get("name"))
                .and_then(Value::as_str)
        })
        .unwrap_or("")
        .trim()
        .to_string();
    if name.is_empty() {
        return None;
    }
    let arguments = map
        .get("arguments")
        .and_then(Value::as_str)
        .or_else(|| {
            map.get("function")
                .and_then(|value| value.get("arguments"))
                .and_then(Value::as_str)
        })
        .map(normalize_tool_arguments_json)
        .or_else(|| {
            map.get("input")
                .and_then(Value::as_str)
                .and_then(|input| serde_json::to_string(&json!({ "input": input })).ok())
        })
        .unwrap_or_default();
    let call_id = map
        .get("call_id")
        .or_else(|| map.get("id"))
        .or_else(|| map.get("tool_call_id"))
        .or_else(|| map.get("toolCallId"))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("call_{}", fallback_index + 1));
    Some(json!({
        "type": "function",
        "id": call_id,
        "function": {
            "name": name,
            "arguments": arguments,
        }
    }))
}

pub(super) fn extract_tool_calls(message: &Value) -> Option<Value> {
    let Value::Object(map) = message else {
        return None;
    };
    let payload = map
        .get("tool_calls")
        .or_else(|| map.get("tool_call"))
        .or_else(|| map.get("function_call"))
        .or_else(|| map.get("functionCall"))
        .map(sanitize_tool_call_payload)?;
    match &payload {
        Value::Array(items) if items.is_empty() => None,
        _ => Some(payload),
    }
}

pub(super) fn has_stream_tool_activity(payload: Option<&Value>) -> bool {
    payload.is_some_and(|value| extract_tool_calls(value).is_some())
}

pub(super) fn is_false_tool_stop_reason(value: Option<&Value>) -> bool {
    let Some(raw) = value.and_then(Value::as_str).map(str::trim) else {
        return false;
    };
    matches!(
        raw.to_ascii_lowercase().as_str(),
        "tool_calls" | "tool_call" | "function_call" | "tooluse" | "tool_use"
    )
}

pub(super) fn extract_stream_error_message(payload: &Value) -> Option<String> {
    let payload_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    // Responses API can report fatal failures mid-stream via structured SSE events rather than
    // terminating the HTTP request with a non-2xx status, so surface them as regular errors.
    let error_payload = if payload_type == "response.failed" {
        payload
            .get("response")
            .and_then(|response| response.get("error"))
            .or_else(|| payload.get("error"))
    } else {
        payload.get("error").or_else(|| {
            payload
                .get("response")
                .and_then(|response| response.get("error"))
        })
    }?;
    let prefix = if payload_type == "response.failed" {
        "LLM stream response failed"
    } else {
        "LLM stream payload failed"
    };
    Some(format_stream_error_message(prefix, error_payload))
}

fn format_stream_error_message(prefix: &str, error_payload: &Value) -> String {
    match error_payload {
        Value::String(text) => {
            let text = text.trim();
            if text.is_empty() {
                prefix.to_string()
            } else {
                format!("{prefix}: {text}")
            }
        }
        Value::Object(map) => {
            let code = map.get("code").and_then(Value::as_str).unwrap_or("").trim();
            let message = map
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if !code.is_empty() && !message.is_empty() {
                format!("{prefix}: {code}: {message}")
            } else if !message.is_empty() {
                format!("{prefix}: {message}")
            } else if !code.is_empty() {
                format!("{prefix}: {code}")
            } else {
                let raw = serde_json::to_string(error_payload).unwrap_or_default();
                if raw.is_empty() {
                    prefix.to_string()
                } else {
                    format!("{prefix}: {}", truncate_text(&raw, 512))
                }
            }
        }
        _ => {
            let raw = serde_json::to_string(error_payload).unwrap_or_default();
            if raw.is_empty() {
                prefix.to_string()
            } else {
                format!("{prefix}: {}", truncate_text(&raw, 512))
            }
        }
    }
}
