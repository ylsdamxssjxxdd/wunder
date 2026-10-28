use serde_json::{Map, Value};

pub(super) const MAX_EVENT_BYTES: usize = 8192;

pub(super) fn bounded(value: &Value) -> Value {
    project(value, &mut (MAX_EVENT_BYTES - 512), 0)
}

fn project(value: &Value, remaining: &mut usize, depth: usize) -> Value {
    if *remaining < 16 || depth >= 8 {
        *remaining = remaining.saturating_sub(4);
        return Value::Null;
    }
    *remaining -= 8;
    match value {
        Value::String(text) => {
            let maximum = (*remaining / 6).min(2048);
            let mut end = text.len().min(maximum);
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            *remaining -= end * 6;
            Value::String(text[..end].to_owned())
        }
        Value::Array(values) => {
            let mut output = Vec::new();
            for entry in values.iter().take(32) {
                if *remaining < 16 {
                    break;
                }
                output.push(project(entry, remaining, depth + 1));
            }
            Value::Array(output)
        }
        Value::Object(values) => {
            let mut output = Map::new();
            let priority = [
                "user_round",
                "model_round",
                "usage",
                "usage_accounted",
                "input_tokens",
                "output_tokens",
                "total_tokens",
                "reasoning_tokens",
                "decode_output_tokens",
                "prefill_duration_s",
                "decode_duration_s",
                "ttft_ms",
                "context_tokens",
                "context_occupancy_tokens",
                "session_request_count",
                "session_quota_used",
                "stage",
                "status",
                "tool",
                "model",
                "cancel_source",
            ];
            let fields = priority
                .iter()
                .filter_map(|key| values.get_key_value(*key))
                .chain(
                    values
                        .iter()
                        .take(64)
                        .filter(|(key, _)| !priority.contains(&key.as_str())),
                );
            for (key, entry) in fields.take(64) {
                let cost = key.len().saturating_mul(6).saturating_add(16);
                if cost > *remaining {
                    continue;
                }
                *remaining -= cost;
                output.insert(key.clone(), project(entry, remaining, depth + 1));
            }
            Value::Object(output)
        }
        _ => {
            *remaining = remaining.saturating_sub(32);
            value.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn diagnostic_projection_bounds_bytes_width_depth_and_unicode() {
        for text in [
            "sample 中文🙂".repeat(100_000),
            "\u{0001}\\\"".repeat(100_000),
        ] {
            let input = json!({"content":text,"results":(0..100).map(|_|json!({"output":text})).collect::<Vec<_>>()});
            let output = bounded(&input);
            assert!(serde_json::to_vec(&output).unwrap().len() <= MAX_EVENT_BYTES);
            assert!(output["content"].as_str().unwrap().len() < 2048);
        }
        let mut nested = json!("sample");
        for _ in 0..9 {
            nested = json!([nested.clone(), nested.clone(), nested.clone(), nested]);
        }
        assert!(serde_json::to_vec(&bounded(&nested)).unwrap().len() <= MAX_EVENT_BYTES);
    }
}
