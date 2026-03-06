use serde_json::{json, Map, Value};

pub struct ToolArgsRecovery {
    pub value: Value,
    pub repair: Option<Value>,
}

pub struct ToolCallPayloadSanitization {
    pub value: Value,
    pub repair: Option<Value>,
}

pub fn recover_tool_args_value(args: &Value) -> Value {
    recover_tool_args_value_with_meta(args).value
}

pub fn recover_tool_args_value_with_meta(args: &Value) -> ToolArgsRecovery {
    match args {
        Value::Object(map) => {
            let Some(raw) = map.get("raw").and_then(Value::as_str).map(str::trim) else {
                return ToolArgsRecovery {
                    value: args.clone(),
                    repair: None,
                };
            };
            if raw.is_empty() {
                return ToolArgsRecovery {
                    value: args.clone(),
                    repair: None,
                };
            }

            if let Some(Value::Object(mut parsed)) = strict_parse_json_value(raw) {
                for (key, value) in map {
                    if key == "raw" {
                        continue;
                    }
                    parsed.entry(key.clone()).or_insert_with(|| value.clone());
                }
                return ToolArgsRecovery {
                    value: Value::Object(parsed),
                    repair: None,
                };
            }

            if let Some(Value::Object(mut parsed)) = parse_json_value_lossy(raw) {
                for (key, value) in map {
                    if key == "raw" {
                        continue;
                    }
                    parsed.entry(key.clone()).or_insert_with(|| value.clone());
                }
                return ToolArgsRecovery {
                    value: Value::Object(parsed),
                    repair: Some(build_repair_meta("tool_args", "raw", "lossy_json_string_repair")),
                };
            }

            ToolArgsRecovery {
                value: args.clone(),
                repair: None,
            }
        }
        Value::String(raw) => {
            let trimmed = raw.trim();
            if let Some(Value::Object(parsed)) = strict_parse_json_value(trimmed) {
                return ToolArgsRecovery {
                    value: Value::Object(parsed),
                    repair: None,
                };
            }
            if let Some(Value::Object(parsed)) = parse_json_value_lossy(trimmed) {
                return ToolArgsRecovery {
                    value: Value::Object(parsed),
                    repair: Some(build_repair_meta(
                        "tool_args",
                        "string",
                        "lossy_json_string_repair",
                    )),
                };
            }
            ToolArgsRecovery {
                value: args.clone(),
                repair: None,
            }
        }
        _ => ToolArgsRecovery {
            value: args.clone(),
            repair: None,
        },
    }
}

#[cfg(test)]
pub fn normalize_tool_arguments_value(arguments: &str) -> Value {
    normalize_tool_arguments_json_with_meta(arguments)
        .0
        .parse::<Value>()
        .ok()
        .unwrap_or_else(|| json!({ "raw": arguments }))
}

pub fn normalize_tool_arguments_json(arguments: &str) -> String {
    normalize_tool_arguments_json_with_meta(arguments).0
}

pub fn normalize_tool_arguments_json_with_meta(arguments: &str) -> (String, Option<Value>) {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return ("{}".to_string(), None);
    }

    if let Some(Value::Object(_)) = strict_parse_json_value(trimmed) {
        return (trimmed.to_string(), None);
    }

    if let Some(value) = strict_parse_json_value(trimmed) {
        return (
            serde_json::to_string(&json!({ "value": value }))
                .unwrap_or_else(|_| "{\"value\":null}".to_string()),
            Some(build_repair_meta(
                "tool_arguments",
                "arguments",
                "non_object_arguments_wrapped",
            )),
        );
    }

    if let Some(value) = parse_json_value_lossy(trimmed) {
        return match value {
            Value::Object(map) => (
                serde_json::to_string(&Value::Object(map))
                    .unwrap_or_else(|_| "{}".to_string()),
                Some(build_repair_meta(
                    "tool_arguments",
                    "arguments",
                    "lossy_json_string_repair",
                )),
            ),
            other => (
                serde_json::to_string(&json!({ "value": other }))
                    .unwrap_or_else(|_| "{\"value\":null}".to_string()),
                Some(build_repair_meta(
                    "tool_arguments",
                    "arguments",
                    "non_object_arguments_wrapped",
                )),
            ),
        };
    }

    (
        serde_json::to_string(&json!({ "raw": arguments }))
            .unwrap_or_else(|_| "{\"raw\":\"\"}".to_string()),
        Some(build_repair_meta(
            "tool_arguments",
            "arguments",
            "raw_arguments_wrapped",
        )),
    )
}

pub fn sanitize_tool_call_payload(payload: &Value) -> Value {
    sanitize_tool_call_payload_with_meta(payload).value
}

pub fn sanitize_tool_call_payload_with_meta(payload: &Value) -> ToolCallPayloadSanitization {
    let mut repaired_count = 0usize;
    let value = sanitize_tool_call_payload_inner(payload, &mut repaired_count);
    let repair = (repaired_count > 0).then(|| {
        json!({
            "kind": "tool_call_payload",
            "source": "function.arguments",
            "strategy": "sanitize_before_request",
            "count": repaired_count,
        })
    });
    ToolCallPayloadSanitization { value, repair }
}

fn sanitize_tool_call_payload_inner(payload: &Value, repaired_count: &mut usize) -> Value {
    match payload {
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| sanitize_tool_call_payload_inner(item, repaired_count))
                .collect(),
        ),
        Value::Object(map) => {
            let mut sanitized = Map::with_capacity(map.len());
            for (key, value) in map {
                if key == "arguments" {
                    let normalized = match value {
                        Value::String(text) => {
                            let (normalized, repair) = normalize_tool_arguments_json_with_meta(text);
                            if repair.is_some() {
                                *repaired_count = repaired_count.saturating_add(1);
                            }
                            Value::String(normalized)
                        }
                        other => sanitize_tool_call_payload_inner(other, repaired_count),
                    };
                    sanitized.insert(key.clone(), normalized);
                    continue;
                }
                sanitized.insert(
                    key.clone(),
                    sanitize_tool_call_payload_inner(value, repaired_count),
                );
            }
            Value::Object(sanitized)
        }
        _ => payload.clone(),
    }
}

fn strict_parse_json_value(raw: &str) -> Option<Value> {
    serde_json::from_str::<Value>(raw).ok()
}

fn parse_json_value_lossy(raw: &str) -> Option<Value> {
    let repaired = repair_json_string_syntax(raw);
    (repaired != raw)
        .then(|| serde_json::from_str::<Value>(&repaired).ok())
        .flatten()
}

fn build_repair_meta(kind: &str, source: &str, strategy: &str) -> Value {
    json!({
        "kind": kind,
        "source": source,
        "strategy": strategy,
    })
}

fn repair_json_string_syntax(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let mut output = String::with_capacity(raw.len() + 16);
    let mut in_string = false;
    let mut escaped = false;

    for (index, ch) in chars.iter().copied().enumerate() {
        if !in_string {
            if ch == '"' {
                in_string = true;
            }
            output.push(ch);
            continue;
        }

        if escaped {
            output.push(ch);
            if ch == '"'
                && matches!(next_non_whitespace_char(&chars, index + 1), Some(',') | Some('}'))
            {
                output.push('"');
                in_string = false;
            }
            escaped = false;
            continue;
        }

        match ch {
            '\\' => {
                output.push(ch);
                escaped = true;
            }
            '"' => {
                output.push(ch);
                in_string = false;
            }
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            other if other.is_control() => {
                let code = other as u32;
                output.push_str(&format!("\\u{code:04x}"));
            }
            _ => output.push(ch),
        }
    }

    output
}

fn next_non_whitespace_char(chars: &[char], start: usize) -> Option<char> {
    chars
        .iter()
        .copied()
        .skip(start)
        .find(|ch| !ch.is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_tool_arguments_json_with_meta, normalize_tool_arguments_value,
        recover_tool_args_value, recover_tool_args_value_with_meta,
        sanitize_tool_call_payload, sanitize_tool_call_payload_with_meta,
    };
    use serde_json::json;

    #[test]
    fn recover_tool_args_value_repairs_multiline_raw_json() {
        let args = json!({
            "raw": "{\"content\": \"python3 -c \\\"\nprint('Map saved')\n\\\", \"timeout_s\": 30, \"workdir\": \".\"}"
        });

        assert_eq!(
            recover_tool_args_value(&args),
            json!({
                "content": "python3 -c \"\nprint('Map saved')\n\"",
                "timeout_s": 30,
                "workdir": "."
            })
        );
    }

    #[test]
    fn recover_tool_args_value_with_meta_marks_repaired_raw_payload() {
        let args = json!({
            "raw": "{\"content\": \"python3 -c \\\"\nprint('Map saved')\n\\\", \"timeout_s\": 30}"
        });

        let repaired = recover_tool_args_value_with_meta(&args);
        assert_eq!(
            repaired.repair,
            Some(json!({
                "kind": "tool_args",
                "source": "raw",
                "strategy": "lossy_json_string_repair"
            }))
        );
    }

    #[test]
    fn normalize_tool_arguments_value_repairs_literal_newlines() {
        let value = normalize_tool_arguments_value(
            "{\"content\": \"python3 -c \\\"\nprint('hi')\n\\\"\", \"workdir\": \".\"}",
        );

        assert_eq!(
            value,
            json!({
                "content": "python3 -c \"\nprint('hi')\n\"",
                "workdir": "."
            })
        );
    }

    #[test]
    fn normalize_tool_arguments_json_with_meta_marks_wrapped_raw_arguments() {
        let (_, repair) = normalize_tool_arguments_json_with_meta("python3 -c \"print('hi')\"");
        assert_eq!(
            repair,
            Some(json!({
                "kind": "tool_arguments",
                "source": "arguments",
                "strategy": "raw_arguments_wrapped"
            }))
        );
    }

    #[test]
    fn sanitize_tool_call_payload_normalizes_nested_arguments() {
        let payload = json!([
            {
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "execute_command",
                    "arguments": "{\"content\": \"python3 -c \\\"\nprint('hi')\n\\\"\"}"
                }
            }
        ]);

        let sanitized = sanitize_tool_call_payload(&payload);
        assert_eq!(
            sanitized,
            json!([
                {
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "execute_command",
                        "arguments": "{\"content\":\"python3 -c \\\"\\nprint('hi')\\n\\\"\"}"
                    }
                }
            ])
        );
    }

    #[test]
    fn sanitize_tool_call_payload_with_meta_counts_repairs() {
        let payload = json!([
            {
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "execute_command",
                    "arguments": "{\"content\": \"python3 -c \\\"\nprint('hi')\n\\\"\"}"
                }
            }
        ]);

        let sanitized = sanitize_tool_call_payload_with_meta(&payload);
        assert_eq!(
            sanitized.repair,
            Some(json!({
                "kind": "tool_call_payload",
                "source": "function.arguments",
                "strategy": "sanitize_before_request",
                "count": 1
            }))
        );
    }
}
