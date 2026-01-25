use super::tool_calls::{collect_tool_calls_from_payload, ToolCall};
use super::*;

#[derive(Clone, Debug)]
pub(super) struct ContextManager;

#[derive(Clone, Debug)]
struct PendingToolCall {
    id: Option<String>,
    name: String,
}

impl ContextManager {
    pub(super) fn normalize_messages(&self, messages: Vec<Value>) -> Vec<Value> {
        if messages.is_empty() {
            return messages;
        }
        let mut output = Vec::with_capacity(messages.len());
        let mut pending: Vec<PendingToolCall> = Vec::new();

        for message in messages {
            let role = message.get("role").and_then(Value::as_str).unwrap_or("");
            let content = message.get("content").unwrap_or(&Value::Null);
            let is_observation =
                role == "user" && Orchestrator::is_observation_message(role, content);

            if !pending.is_empty() && role != "tool" && !is_observation {
                append_missing_tool_results(&mut output, &pending);
                pending.clear();
            }

            if role == "tool" {
                let tool_call_id = extract_tool_call_id(&message);
                if let Some(id) = tool_call_id.as_deref() {
                    if let Some(pos) = pending
                        .iter()
                        .position(|call| call.id.as_deref() == Some(id))
                    {
                        pending.remove(pos);
                    }
                } else if let Some(pos) = pending.iter().position(|call| call.id.is_none()) {
                    pending.remove(pos);
                }
            } else if is_observation {
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
        output
    }

    pub(super) fn estimate_context_tokens(&self, messages: &[Value]) -> i64 {
        estimate_messages_tokens(messages).max(0)
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
        "timestamp": Local::now().to_rfc3339(),
    });
    serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string())
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
}
