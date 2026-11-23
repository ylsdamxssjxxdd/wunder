use serde_json::{Value, json};

#[cfg(test)]
pub(super) fn apply_anthropic_prompt_cache(payload: &mut Value) {
    apply_anthropic_prompt_cache_with_state(payload, None, false);
}

pub(super) fn apply_anthropic_prompt_cache_with_state(
    payload: &mut Value,
    previous_tail: Option<&Value>,
    static_ttl_1h: bool,
) {
    if let Some(tools) = payload.get_mut("tools").and_then(Value::as_array_mut) {
        if let Some(last_tool) = tools.last_mut() {
            mark_static_cache_control(last_tool, static_ttl_1h);
        }
    }

    if let Some(system) = payload.get_mut("system") {
        match system {
            Value::String(text) => {
                if !text.trim().is_empty() {
                    let text = std::mem::take(text);
                    let mut block = json!({ "type": "text", "text": text });
                    mark_static_cache_control(&mut block, static_ttl_1h);
                    *system = json!([block]);
                }
            }
            Value::Array(blocks) => {
                if let Some(block) = blocks
                    .iter_mut()
                    .rev()
                    .find(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                {
                    mark_static_cache_control(block, static_ttl_1h);
                }
            }
            _ => {}
        }
    }

    if let Some(messages) = payload.get_mut("messages").and_then(Value::as_array_mut) {
        // Keep the preceding write reachable when this request appended more
        // than Anthropic's 20-block lookback window.
        if let Some(previous_tail) = previous_tail {
            if let Some(previous) = messages
                .iter_mut()
                .rev()
                .skip(1)
                .find(|message| message.get("content") == Some(previous_tail))
            {
                mark_anthropic_message_cache(previous);
            }
        }
        messages
            .iter_mut()
            .rev()
            .any(|message| mark_anthropic_message_cache(message));
    }
}

pub(super) fn anthropic_cache_tail(messages: &Value) -> Option<Value> {
    messages.as_array()?.iter().rev().find_map(|message| {
        let content = message.get("content")?;
        let cacheable = match content {
            Value::String(text) => !text.trim().is_empty(),
            Value::Array(blocks) => blocks.iter().any(anthropic_cacheable_block),
            _ => false,
        };
        cacheable.then(|| content.clone())
    })
}

fn anthropic_cacheable_block(block: &Value) -> bool {
    let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");
    if matches!(block_type, "thinking" | "redacted_thinking") {
        return false;
    }
    block_type != "text"
        || block
            .get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.trim().is_empty())
}

fn mark_anthropic_message_cache(message: &mut Value) -> bool {
    let Some(content) = message.get_mut("content") else {
        return false;
    };
    match content {
        Value::String(text) => {
            if text.trim().is_empty() {
                return false;
            }
            let text = std::mem::take(text);
            let mut block = json!({ "type": "text", "text": text });
            mark_ephemeral_cache_control(&mut block);
            *content = json!([block]);
            true
        }
        Value::Array(blocks) => {
            if let Some(block) = blocks
                .iter_mut()
                .rev()
                .find(|block| anthropic_cacheable_block(block))
            {
                mark_ephemeral_cache_control(block);
                true
            } else {
                false
            }
        }
        _ => false,
    }
}

fn mark_static_cache_control(block: &mut Value, ttl_1h: bool) {
    if ttl_1h {
        let Value::Object(map) = block else {
            return;
        };
        map.entry("cache_control".to_string())
            .or_insert_with(|| json!({ "type": "ephemeral", "ttl": "1h" }));
    } else {
        mark_ephemeral_cache_control(block);
    }
}

fn mark_ephemeral_cache_control(block: &mut Value) {
    let Value::Object(map) = block else {
        return;
    };
    if !map.contains_key("cache_control") {
        map.insert("cache_control".to_string(), json!({ "type": "ephemeral" }));
    }
}
