use super::tool_calls::{ToolCall, collect_tool_calls_from_payload};
use super::*;
use std::collections::VecDeque;

#[derive(Clone, Debug)]
pub(super) struct ContextManager;

#[derive(Clone, Debug)]
struct PendingToolCall {
    id: Option<String>,
    name: String,
}

pub(super) const MODEL_CONTEXT_INTERNAL_META_TYPE: &str = "model_context_internal";

impl ContextManager {
    pub(super) fn normalize_messages(&self, messages: Vec<Value>) -> Vec<Value> {
        if messages.is_empty() {
            return messages;
        }
        let mut output = Vec::with_capacity(messages.len());
        let mut pending: Vec<PendingToolCall> = Vec::new();
        let mut deferred: Vec<Value> = Vec::new();
        let mut queue = VecDeque::from(messages);

        while let Some(raw_message) = queue.pop_front() {
            let mut message = raw_message;
            let mut role = message
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if role == "tool" {
                let tool_call_id = extract_tool_call_id(&message);
                let matched = if let Some(id) = tool_call_id.as_deref() {
                    pending
                        .iter()
                        .position(|call| call.id.as_deref() == Some(id))
                } else {
                    pending.iter().position(|call| call.id.is_none())
                };
                if let Some(pos) = matched {
                    pending.remove(pos);
                    output.push(message);
                    if pending.is_empty() {
                        prepend_deferred_messages(&mut queue, &mut deferred);
                    }
                    continue;
                } else {
                    message = convert_orphan_tool_message_to_observation(&message);
                    role = "user".to_string();
                }
            }

            let content = message.get("content").unwrap_or(&Value::Null);
            let is_observation =
                role == "user" && Orchestrator::is_observation_message(role.as_str(), content);
            if !pending.is_empty() && role != "tool" && pending_has_native_ids(&pending) {
                // Native OpenAI-compatible tool calls require all tool result messages to
                // immediately follow the assistant message. User observations emitted by
                // recovery paths are deferred until the pending native results are closed.
                deferred.push(message);
                continue;
            }

            if !pending.is_empty() && role != "tool" && !is_observation {
                append_missing_tool_results(&mut output, &pending);
                pending.clear();
                prepend_deferred_messages(&mut queue, &mut deferred);
                queue.push_front(message);
                continue;
            }

            if is_observation {
                if let Some(pos) = pending.iter().position(|call| call.id.is_none()) {
                    pending.remove(pos);
                }
            }

            output.push(message.clone());

            if role == "assistant" {
                let calls = extract_tool_calls(&message);
                if !calls.is_empty() {
                    pending = calls
                        .into_iter()
                        .map(|call| PendingToolCall {
                            id: normalize_tool_call_id(call.id.as_deref()),
                            name: call.name,
                        })
                        .collect();
                }
            }
        }

        if !pending.is_empty() {
            append_missing_tool_results(&mut output, &pending);
        }
        if !deferred.is_empty() {
            let deferred = std::mem::take(&mut deferred);
            output.extend(self.normalize_messages(deferred));
        }
        output
    }
}

fn pending_has_native_ids(pending: &[PendingToolCall]) -> bool {
    pending.iter().any(|call| call.id.is_some())
}

fn prepend_deferred_messages(queue: &mut VecDeque<Value>, deferred: &mut Vec<Value>) {
    for message in deferred.drain(..).rev() {
        queue.push_front(message);
    }
}

pub(super) fn model_context_entries_from_messages(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .cloned()
        .filter_map(normalize_model_context_message)
        .collect()
}

pub(super) fn normalize_model_context_message(message: Value) -> Option<Value> {
    let obj = message.as_object()?;
    let role = obj
        .get("role")
        .and_then(Value::as_str)?
        .trim()
        .to_ascii_lowercase();
    if role == "system" {
        return None;
    }
    if !matches!(role.as_str(), "user" | "assistant" | "tool") {
        return None;
    }

    let content = normalize_model_context_content(obj.get("content").cloned());
    let mut normalized = serde_json::Map::new();
    normalized.insert("role".to_string(), Value::String(role.clone()));
    normalized.insert("content".to_string(), content);
    let tool_call_id = extract_tool_call_id(&Value::Object(obj.clone()));

    if role == "assistant" {
        if let Some(reasoning) = obj
            .get("reasoning_content")
            .or_else(|| obj.get("reasoning"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            normalized.insert(
                "reasoning_content".to_string(),
                Value::String(reasoning.to_string()),
            );
        }
        if let Some(tool_calls) = extract_tool_calls_payload(&Value::Object(obj.clone())) {
            normalized.insert("tool_calls".to_string(), tool_calls);
        }
        let candidate = Value::Object(normalized.clone());
        if assistant_model_context_message_is_empty(&candidate) {
            return None;
        }
    }

    if let Some(tool_call_id) = tool_call_id {
        normalized.insert("tool_call_id".to_string(), Value::String(tool_call_id));
    }

    let candidate = Value::Object(normalized.clone());
    if role == "tool" && extract_tool_call_id(&candidate).is_none() {
        return None;
    }

    Some(Value::Object(normalized))
}

fn normalize_model_context_content(content: Option<Value>) -> Value {
    match content.unwrap_or(Value::String(String::new())) {
        Value::String(text) => Value::String(text),
        Value::Array(items) => Value::Array(items),
        Value::Object(map) => Value::Object(map),
        Value::Null => Value::String(String::new()),
        other => Value::String(other.to_string()),
    }
}

fn assistant_model_context_message_is_empty(message: &Value) -> bool {
    let has_tool_calls = extract_tool_calls_payload(message).is_some();
    let has_reasoning = message
        .get("reasoning_content")
        .or_else(|| message.get("reasoning"))
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    if has_tool_calls || has_reasoning {
        return false;
    }
    match message.get("content").unwrap_or(&Value::Null) {
        Value::String(text) => text.trim().is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        Value::Null => true,
        other => other.to_string().trim().is_empty(),
    }
}

fn extract_tool_calls(message: &Value) -> Vec<ToolCall> {
    let Some(payload) = extract_tool_calls_payload(message) else {
        return Vec::new();
    };
    collect_tool_calls_from_payload(&payload)
        .into_iter()
        .filter(|call| !call.name.trim().is_empty())
        .collect()
}

fn extract_tool_calls_payload(message: &Value) -> Option<Value> {
    let obj = message.as_object()?;
    for key in [
        "tool_calls",
        "toolCalls",
        "tool_call",
        "toolCall",
        "function_call",
        "functionCall",
        "function",
    ] {
        if let Some(value) = obj.get(key) {
            if !value.is_null() {
                return Some(value.clone());
            }
        }
    }
    None
}

fn normalize_tool_call_id(id: Option<&str>) -> Option<String> {
    id.map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn extract_tool_call_id(message: &Value) -> Option<String> {
    let obj = message.as_object()?;
    for key in ["tool_call_id", "toolCallId", "call_id", "callId"] {
        if let Some(value) = obj.get(key) {
            let text = match value {
                Value::String(text) => text.clone(),
                Value::Number(num) => num.to_string(),
                _ => continue,
            };
            let cleaned = text.trim().to_string();
            if !cleaned.is_empty() {
                return Some(cleaned);
            }
        }
    }
    None
}

fn append_missing_tool_results(output: &mut Vec<Value>, pending: &[PendingToolCall]) {
    for call in pending {
        if call.name.trim().is_empty() {
            continue;
        }
        output.push(build_missing_tool_message(call));
    }
}

fn build_missing_tool_message(call: &PendingToolCall) -> Value {
    let observation = build_missing_tool_observation(call.name.trim());
    if let Some(id) = call.id.as_ref() {
        json!({
            "role": "tool",
            "tool_call_id": id,
            "content": observation,
        })
    } else {
        json!({
            "role": "user",
            "content": format!("{OBSERVATION_PREFIX}{observation}"),
        })
    }
}

fn build_missing_tool_observation(tool_name: &str) -> String {
    let payload = json!({
        "tool": tool_name,
        "ok": false,
        "error": "missing tool result",
        "data": {},
    });
    serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string())
}

fn convert_orphan_tool_message_to_observation(message: &Value) -> Value {
    let content = message.get("content").cloned().unwrap_or(Value::Null);
    let content_text = match content {
        Value::String(text) => {
            if text.trim().is_empty() {
                "{}".to_string()
            } else {
                text
            }
        }
        Value::Null => "{}".to_string(),
        other => serde_json::to_string(&other).unwrap_or_else(|_| "{}".to_string()),
    };
    json!({
        "role": "user",
        "content": format!("{OBSERVATION_PREFIX}{content_text}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_adds_missing_tool_result() {
        let manager = ContextManager;
        let messages = vec![
            json!({ "role": "system", "content": "sys" }),
            json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": { "name": "read_file", "arguments": r#"{"path":"a.txt"}"# }
                }]
            }),
            json!({ "role": "user", "content": "next" }),
        ];
        let normalized = manager.normalize_messages(messages);
        assert_eq!(normalized.len(), 4);
        assert_eq!(
            normalized[2].get("role").and_then(Value::as_str),
            Some("tool")
        );
        assert_eq!(
            normalized[2].get("tool_call_id").and_then(Value::as_str),
            Some("call_1")
        );
    }

    #[test]
    fn test_normalize_keeps_existing_tool_result() {
        let manager = ContextManager;
        let messages = vec![
            json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": { "name": "read_file", "arguments": r#"{"path":"a.txt"}"# }
                }]
            }),
            json!({
                "role": "tool",
                "tool_call_id": "call_1",
                "content": r#"{"tool":"read_file","ok":true,"data":{}}"#
            }),
            json!({ "role": "user", "content": "ok" }),
        ];
        let normalized = manager.normalize_messages(messages);
        assert_eq!(normalized.len(), 3);
    }

    #[test]
    fn test_normalize_matches_observation_for_untracked_call() {
        let manager = ContextManager;
        let observation = format!(
            "{OBSERVATION_PREFIX}{}",
            r#"{"tool":"write_file","ok":true,"data":{}}"#
        );
        let messages = vec![
            json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "type": "function",
                    "function": { "name": "write_file", "arguments": r#"{"path":"a.txt"}"# }
                }]
            }),
            json!({ "role": "user", "content": observation }),
            json!({ "role": "assistant", "content": "done" }),
        ];
        let normalized = manager.normalize_messages(messages);
        assert_eq!(normalized.len(), 3);
    }

    #[test]
    fn test_normalize_defers_observation_until_native_tool_results_close() {
        let manager = ContextManager;
        let observation = format!(
            "{OBSERVATION_PREFIX}{}",
            r#"{"type":"tool_failure_reroute_notice","instruction":"continue"}"#
        );
        let messages = vec![
            json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [
                    {
                        "id": "call_1",
                        "type": "function",
                        "function": { "name": "execute_command", "arguments": r#"{"command":"a"}"# }
                    },
                    {
                        "id": "call_2",
                        "type": "function",
                        "function": { "name": "execute_command", "arguments": r#"{"command":"b"}"# }
                    },
                    {
                        "id": "call_3",
                        "type": "function",
                        "function": { "name": "execute_command", "arguments": r#"{"command":"c"}"# }
                    }
                ]
            }),
            json!({
                "role": "tool",
                "tool_call_id": "call_1",
                "content": r#"{"tool":"execute_command","ok":false}"#
            }),
            json!({ "role": "user", "content": observation }),
            json!({
                "role": "tool",
                "tool_call_id": "call_2",
                "content": r#"{"tool":"execute_command","ok":false}"#
            }),
            json!({
                "role": "tool",
                "tool_call_id": "call_3",
                "content": r#"{"tool":"execute_command","ok":false}"#
            }),
        ];

        let normalized = manager.normalize_messages(messages);

        assert_eq!(normalized.len(), 5);
        assert_eq!(normalized[1]["role"], json!("tool"));
        assert_eq!(normalized[1]["tool_call_id"], json!("call_1"));
        assert_eq!(normalized[2]["role"], json!("tool"));
        assert_eq!(normalized[2]["tool_call_id"], json!("call_2"));
        assert_eq!(normalized[3]["role"], json!("tool"));
        assert_eq!(normalized[3]["tool_call_id"], json!("call_3"));
        assert_eq!(normalized[4]["role"], json!("user"));
        assert_eq!(normalized[4]["content"], json!(observation));
    }

    #[test]
    fn test_normalize_converts_orphan_tool_message() {
        let manager = ContextManager;
        let messages = vec![
            json!({ "role": "system", "content": "sys" }),
            json!({
                "role": "tool",
                "tool_call_id": "call_orphan",
                "content": r#"{"tool":"read_file","ok":true,"data":{"text":"hello"}}"#
            }),
            json!({ "role": "user", "content": "next" }),
        ];
        let normalized = manager.normalize_messages(messages);
        assert_eq!(normalized.len(), 3);
        assert_eq!(
            normalized[1].get("role").and_then(Value::as_str),
            Some("user")
        );
        let text = normalized[1]
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("");
        assert!(text.starts_with(OBSERVATION_PREFIX));
    }

    #[test]
    fn test_normalize_converts_mismatched_tool_message_and_keeps_pending_call() {
        let manager = ContextManager;
        let messages = vec![
            json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "id": "call_expected",
                    "type": "function",
                    "function": { "name": "read_file", "arguments": r#"{"path":"a.txt"}"# }
                }]
            }),
            json!({
                "role": "tool",
                "tool_call_id": "call_other",
                "content": r#"{"tool":"read_file","ok":true,"data":{"text":"other"}}"#
            }),
            json!({ "role": "user", "content": "continue" }),
        ];
        let normalized = manager.normalize_messages(messages);
        assert_eq!(normalized.len(), 4);
        assert_eq!(
            normalized[2].get("role").and_then(Value::as_str),
            Some("user")
        );
        assert_eq!(
            normalized[1].get("role").and_then(Value::as_str),
            Some("tool")
        );
        assert_eq!(
            normalized[1].get("tool_call_id").and_then(Value::as_str),
            Some("call_expected")
        );
    }

    #[test]
    fn test_normalize_closes_terminal_tool_calls_for_model_context() {
        let manager = ContextManager;
        let messages = vec![
            json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "id": "call_final",
                    "type": "function",
                    "function": { "name": "final_response", "arguments": r#"{"content":"ok"}"# }
                }]
            }),
            json!({ "role": "user", "content": "next" }),
        ];
        let normalized = manager.normalize_messages(messages);
        assert_eq!(normalized.len(), 3);
        assert_eq!(
            normalized[1].get("role").and_then(Value::as_str),
            Some("tool")
        );
        assert_eq!(
            normalized[1].get("tool_call_id").and_then(Value::as_str),
            Some("call_final")
        );
    }

    #[test]
    fn test_missing_tool_observation_is_deterministic() {
        let first = build_missing_tool_observation("read_file");
        let second = build_missing_tool_observation("read_file");
        assert_eq!(first, second);
        assert!(!first.contains("timestamp"));
    }

    #[test]
    fn model_context_normalization_preserves_llm_visible_tool_payload() {
        let message = json!({
            "role": "assistant",
            "content": "",
            "session_id": "session-ignored",
            "timestamp": "2026-01-01T00:00:00Z",
            "meta": { "type": "tool_call" },
            "tool_calls": [{
                "id": "call_fetch",
                "type": "function",
                "function": {
                    "name": "web_fetch",
                    "arguments": "{\"url\":\"https://example.invalid/item\",\"extract_mode\":\"markdown\"}"
                }
            }]
        });

        let normalized = normalize_model_context_message(message).expect("context message");

        assert_eq!(
            normalized,
            json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "id": "call_fetch",
                    "type": "function",
                    "function": {
                        "name": "web_fetch",
                        "arguments": "{\"url\":\"https://example.invalid/item\",\"extract_mode\":\"markdown\"}"
                    }
                }]
            })
        );
    }

    #[test]
    fn model_context_entries_exclude_system_messages() {
        let messages = vec![
            json!({ "role": "system", "content": "frozen" }),
            json!({ "role": "user", "content": "hello" }),
            json!({ "role": "assistant", "content": "hi" }),
        ];

        let entries = model_context_entries_from_messages(&messages);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["role"], json!("user"));
        assert_eq!(entries[1]["role"], json!("assistant"));
    }
}
