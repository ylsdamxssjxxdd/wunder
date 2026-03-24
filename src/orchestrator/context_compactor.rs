use super::*;
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub(super) struct ContextCompactor;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObservationRole {
    UserPrefixed,
    ToolContent,
}

#[derive(Clone, Debug)]
struct FailureObservation {
    role: ObservationRole,
    tool: String,
    error: String,
    code: String,
    fingerprint: String,
}

impl ContextCompactor {
    pub(super) fn compact_messages(&self, messages: Vec<Value>) -> Vec<Value> {
        if messages.is_empty() {
            return messages;
        }
        let mut repeated_failures: HashMap<String, u32> = HashMap::new();
        let mut output = Vec::with_capacity(messages.len());

        for mut message in messages {
            if let Some(observation) = parse_failure_observation(&message) {
                let count = repeated_failures
                    .entry(observation.fingerprint.clone())
                    .and_modify(|value| *value = value.saturating_add(1))
                    .or_insert(1);
                if *count > 1 {
                    compact_failure_message(&mut message, &observation, *count);
                }
            }
            output.push(message);
        }
        output
    }
}

fn parse_failure_observation(message: &Value) -> Option<FailureObservation> {
    let role = message.get("role").and_then(Value::as_str).unwrap_or("");
    let content = message.get("content").and_then(Value::as_str)?;

    let (observation_role, payload_text) =
        if role == "user" && content.starts_with(OBSERVATION_PREFIX) {
            (
                ObservationRole::UserPrefixed,
                content.trim_start_matches(OBSERVATION_PREFIX).trim(),
            )
        } else if role == "tool" {
            (ObservationRole::ToolContent, content.trim())
        } else {
            return None;
        };
    if payload_text.is_empty() {
        return None;
    }

    let payload = serde_json::from_str::<Value>(payload_text).ok()?;
    let map = payload.as_object()?;
    if map.get("ok").and_then(Value::as_bool) != Some(false) {
        return None;
    }

    let tool = map
        .get("tool")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_string();
    let error = map
        .get("error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            map.get("data")
                .and_then(Value::as_object)
                .and_then(|data| data.get("error"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| "tool failed".to_string());
    let code = map
        .get("error_code")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            map.get("meta")
                .and_then(Value::as_object)
                .and_then(|meta| meta.get("error_code"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| "UNKNOWN".to_string());
    let fingerprint = build_failure_fingerprint(tool.as_str(), code.as_str(), error.as_str());

    Some(FailureObservation {
        role: observation_role,
        tool,
        error,
        code,
        fingerprint,
    })
}

fn compact_failure_message(
    message: &mut Value,
    observation: &FailureObservation,
    repeat_count: u32,
) {
    let compacted = json!({
        "tool": observation.tool,
        "ok": false,
        "error": observation.error.chars().take(160).collect::<String>(),
        "error_code": observation.code,
        "data": {
            "summary": "repeated failure compacted",
        },
        "meta": {
            "compacted_repeat": true,
            "repeat_count": repeat_count,
            "failure_fingerprint": observation.fingerprint,
        }
    });
    let serialized = serde_json::to_string(&compacted).unwrap_or_else(|_| "{}".to_string());
    if let Some(obj) = message.as_object_mut() {
        match observation.role {
            ObservationRole::UserPrefixed => {
                obj.insert(
                    "content".to_string(),
                    Value::String(format!("{OBSERVATION_PREFIX}{serialized}")),
                );
            }
            ObservationRole::ToolContent => {
                obj.insert("content".to_string(), Value::String(serialized));
            }
        }
    }
}

fn build_failure_fingerprint(tool: &str, code: &str, error: &str) -> String {
    let normalized = error.split_whitespace().collect::<Vec<_>>().join(" ");
    let clipped = normalized.chars().take(220).collect::<String>();
    format!("{tool}|{code}|{clipped}")
}

#[cfg(test)]
mod tests {
    use super::ContextCompactor;
    use crate::orchestrator::constants::OBSERVATION_PREFIX;
    use serde_json::{json, Value};

    #[test]
    fn compacts_repeated_failed_observations() {
        let compactor = ContextCompactor;
        let observation = format!(
            "{OBSERVATION_PREFIX}{}",
            json!({
                "tool": "execute_command",
                "ok": false,
                "error": "syntax error",
                "error_code": "SHELL_SYNTAX_ERROR",
            })
        );
        let messages = vec![
            json!({ "role": "user", "content": "start" }),
            json!({ "role": "user", "content": observation }),
            json!({ "role": "user", "content": format!("{OBSERVATION_PREFIX}{}", json!({
                "tool": "execute_command",
                "ok": false,
                "error": "syntax error",
                "error_code": "SHELL_SYNTAX_ERROR",
            })) }),
        ];
        let compacted = compactor.compact_messages(messages);
        let content = compacted[2]
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("");
        assert!(content.contains("repeated failure compacted"));
    }
}
