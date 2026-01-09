// Token 估算工具：用于近似计算上下文占用并进行裁剪。
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;
use tracing::error;

const APPROX_BYTES_PER_TOKEN: f64 = 4.0;
const MESSAGE_TOKEN_OVERHEAD: i64 = 4;
const IMAGE_TOKEN_ESTIMATE: i64 = 256;

pub fn approx_token_count(text: &str) -> i64 {
    if text.is_empty() {
        return 0;
    }
    ((text.len() as f64) / APPROX_BYTES_PER_TOKEN).ceil() as i64
}

pub fn trim_text_to_tokens(text: &str, max_tokens: i64, suffix: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    if max_tokens <= 0 {
        return suffix.to_string();
    }
    if approx_token_count(text) <= max_tokens {
        return text.to_string();
    }
    let suffix_text = suffix;
    let suffix_tokens = approx_token_count(suffix_text);
    if max_tokens <= suffix_tokens {
        let max_chars = (max_tokens.max(1) as f64 * APPROX_BYTES_PER_TOKEN) as usize;
        return suffix_text.chars().take(max_chars).collect();
    }
    let max_chars = (max_tokens as f64 * APPROX_BYTES_PER_TOKEN) as usize;
    let trimmed: String = text
        .chars()
        .take(max_chars.saturating_sub(suffix_text.len()))
        .collect();
    format!("{trimmed}{suffix_text}")
}

pub fn estimate_message_tokens(message: &Value) -> i64 {
    if !message.is_object() {
        return 0;
    }
    let content_tokens = estimate_content_tokens(message.get("content").unwrap_or(&Value::Null));
    let reasoning = message
        .get("reasoning_content")
        .or_else(|| message.get("reasoning"))
        .unwrap_or(&Value::Null);
    let reasoning_tokens = match reasoning {
        Value::String(text) => approx_token_count(text),
        Value::Array(_) | Value::Object(_) => approx_token_count(&reasoning.to_string()),
        _ => 0,
    };
    content_tokens + reasoning_tokens + MESSAGE_TOKEN_OVERHEAD
}

pub fn estimate_messages_tokens(messages: &[Value]) -> i64 {
    messages.iter().map(estimate_message_tokens).sum()
}

pub fn trim_messages_to_budget(messages: &[Value], max_tokens: i64) -> Vec<Value> {
    if messages.is_empty() {
        return Vec::new();
    }
    if max_tokens <= 0 {
        return vec![messages[messages.len() - 1].clone()];
    }
    let mut selected: Vec<Value> = Vec::new();
    let mut remaining = max_tokens;
    for message in messages.iter().rev() {
        let cost = estimate_message_tokens(message);
        if cost <= remaining {
            selected.push(message.clone());
            remaining -= cost;
            continue;
        }
        if selected.is_empty() {
            selected.push(message.clone());
        }
        break;
    }
    selected.reverse();
    selected
}

pub fn estimate_content_tokens(content: &Value) -> i64 {
    match content {
        Value::Null => 0,
        Value::String(text) => estimate_string_tokens(text),
        Value::Array(items) => items.iter().map(estimate_content_tokens).sum(),
        Value::Object(map) => {
            let part_type = map
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_lowercase();
            if part_type == "text" {
                return approx_token_count(map.get("text").and_then(Value::as_str).unwrap_or(""));
            }
            if part_type == "image_url" || map.contains_key("image_url") {
                return IMAGE_TOKEN_ESTIMATE;
            }
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                return approx_token_count(text);
            }
            approx_token_count(&content.to_string())
        }
        _ => approx_token_count(&content.to_string()),
    }
}

fn estimate_string_tokens(text: &str) -> i64 {
    if text.starts_with("data:image/") {
        return IMAGE_TOKEN_ESTIMATE;
    }
    if text.contains("data:image/") {
        let Some(re) = data_url_regex() else {
            return approx_token_count(text);
        };
        let matches = re.find_iter(text).count() as i64;
        let stripped = re.replace_all(text, "[image]");
        return approx_token_count(&stripped) + matches * IMAGE_TOKEN_ESTIMATE;
    }
    approx_token_count(text)
}

fn data_url_regex() -> Option<&'static Regex> {
    static REGEX: OnceLock<Option<Regex>> = OnceLock::new();
    REGEX
        .get_or_init(|| {
            match Regex::new(r"data:image/[a-zA-Z0-9+.-]+;base64,[A-Za-z0-9+/=\r\n]+") {
                Ok(regex) => Some(regex),
                Err(err) => {
                    error!("invalid token_utils data url regex: {err}");
                    None
                }
            }
        })
        .as_ref()
}
