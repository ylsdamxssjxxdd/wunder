use serde_json::{Map, Value};
use std::borrow::Cow;

const INLINE_IMAGE_OMITTED_MARKER: &str = "wunder://omitted-inline-image";
const INLINE_IMAGE_OMITTED_TEXT: &str = "[inline image omitted from persisted context]";
// 读图 followup 用纯文本记录原图路径，既能让模型知道该重读哪个文件，
// 又不会把非标准字段带进模型的 image_url 请求结构。
const IMAGE_PATH_MARKER_PREFIX: &str = "[image_path:";
const DATA_IMAGE_JSON_PREFIX: &str = "data:image";
const BASE64_HEADER_MARKER: &str = ";base64";
const BASE64_URL_MARKER: &str = ";base64,";

pub fn sanitize_persisted_chat_payload(payload: &Value) -> Value {
    let mut sanitized = payload.clone();
    sanitize_inline_images_in_value(&mut sanitized);
    sanitized
}

pub fn sanitize_persisted_model_context_message(message: Value) -> Value {
    let mut sanitized = message;
    sanitize_inline_images_in_value(&mut sanitized);
    sanitized
}

pub fn sanitize_loaded_chat_record(mut record: Value) -> Value {
    sanitize_inline_images_in_value(&mut record);
    record
}

pub fn sanitize_inline_image_data_urls_in_json_text(text: &str) -> Cow<'_, str> {
    if text.trim().is_empty() || !contains_ascii_case_insensitive(text, DATA_IMAGE_JSON_PREFIX) {
        return Cow::Borrowed(text);
    }

    let mut output = String::with_capacity(text.len().min(8192));
    let mut cursor = 0;
    let mut changed = false;
    while cursor < text.len() {
        let Some(relative_start) =
            find_ascii_case_insensitive(&text[cursor..], DATA_IMAGE_JSON_PREFIX)
        else {
            break;
        };
        let start = cursor + relative_start;
        let Some(end) = inline_image_data_url_json_end(text, start) else {
            cursor = start + DATA_IMAGE_JSON_PREFIX.len();
            continue;
        };
        let candidate = &text[start..end];
        if !is_json_inline_image_data_url(candidate) {
            cursor = start + DATA_IMAGE_JSON_PREFIX.len();
            continue;
        }

        output.push_str(&text[cursor..start]);
        let mime = inline_image_mime_from_json_data_url(candidate)
            .unwrap_or_else(|| "image/*".to_string());
        output.push_str(&format!(
            "{INLINE_IMAGE_OMITTED_MARKER}; mime={mime}; original_chars={}",
            candidate.len()
        ));
        cursor = end;
        changed = true;
    }

    if !changed {
        return Cow::Borrowed(text);
    }
    output.push_str(&text[cursor..]);
    Cow::Owned(output)
}

pub fn parse_sanitized_persisted_chat_payload(text: &str) -> (Option<Value>, Option<String>) {
    if text.trim().is_empty() {
        return (None, None);
    }
    let sanitized_text = sanitize_inline_image_data_urls_in_json_text(text);
    let text_changed = matches!(sanitized_text, Cow::Owned(_));
    let value = serde_json::from_str::<Value>(sanitized_text.as_ref())
        .ok()
        .map(sanitize_loaded_chat_record);
    let repaired_text = if text_changed {
        value
            .as_ref()
            .and_then(|value| serde_json::to_string(value).ok())
    } else {
        None
    };
    (value, repaired_text)
}

/// 文本是否为「图片已省略」占位（持久化省略图像像素后的标记）。
pub fn is_omitted_image_placeholder_text(text: &str) -> bool {
    text.trim() == INLINE_IMAGE_OMITTED_TEXT
}

/// 从一段文本里解析 `[image_path:...]` 提示中的原始路径。占位与路径提示
/// 都是纯文本 part，恢复时按出现顺序把每个占位配给它前面最近的路径提示。
pub fn image_path_marker_in_text(text: &str) -> Option<String> {
    let start = text.find(IMAGE_PATH_MARKER_PREFIX)? + IMAGE_PATH_MARKER_PREFIX.len();
    let end = text[start..].find(']')? + start;
    let path = text[start..end].trim();
    (!path.is_empty()).then(|| path.to_string())
}

fn sanitize_inline_images_in_value(value: &mut Value) -> bool {
    match value {
        Value::String(text) => sanitize_inline_image_text(text),
        Value::Array(items) => sanitize_inline_images_in_array(items),
        Value::Object(map) => sanitize_inline_images_in_map(map),
        _ => false,
    }
}

fn sanitize_inline_images_in_array(items: &mut Vec<Value>) -> bool {
    let mut changed = false;
    for item in items.iter_mut() {
        changed |= sanitize_inline_images_in_value(item);
    }
    changed
}

fn sanitize_inline_images_in_map(map: &mut Map<String, Value>) -> bool {
    let mut changed = false;
    let is_content_image_part = map
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("image_url"));
    if let Some(image_url) = map.get_mut("image_url") {
        let image_changed = sanitize_image_url_value(image_url);
        if image_changed && is_content_image_part {
            map.clear();
            map.insert("type".to_string(), Value::String("text".to_string()));
            map.insert(
                "text".to_string(),
                Value::String(INLINE_IMAGE_OMITTED_TEXT.to_string()),
            );
            return true;
        }
        changed |= image_changed;
    }
    if let Some(url) = map.get_mut("url") {
        changed |= sanitize_inline_images_in_value(url);
    }
    for (key, value) in map.iter_mut() {
        if key == "image_url" || key == "url" {
            continue;
        }
        changed |= sanitize_inline_images_in_value(value);
    }
    if changed {
        map.entry("inline_image_omitted".to_string())
            .or_insert(Value::Bool(true));
    }
    changed
}

fn sanitize_image_url_value(value: &mut Value) -> bool {
    match value {
        Value::String(text) => sanitize_inline_image_text(text),
        Value::Object(map) => {
            let mut changed = false;
            if let Some(url) = map.get_mut("url") {
                changed |= sanitize_inline_images_in_value(url);
            }
            for (key, inner) in map.iter_mut() {
                if key == "url" {
                    continue;
                }
                changed |= sanitize_inline_images_in_value(inner);
            }
            if changed {
                map.entry("omitted".to_string())
                    .or_insert(Value::Bool(true));
            }
            changed
        }
        _ => sanitize_inline_images_in_value(value),
    }
}

fn sanitize_inline_image_text(text: &mut String) -> bool {
    let trimmed = text.trim();
    if is_omitted_inline_image_marker(trimmed) {
        return true;
    }
    if !is_inline_image_data_url(trimmed) {
        return false;
    }
    let original_len = text.len();
    let mime = inline_image_mime(trimmed).unwrap_or("image/*");
    *text = format!("{INLINE_IMAGE_OMITTED_MARKER}; mime={mime}; original_chars={original_len}");
    true
}

fn is_inline_image_data_url(value: &str) -> bool {
    let trimmed = value.trim_start();
    trimmed.len() > "data:image/".len()
        && starts_with_ascii_case_insensitive(trimmed, "data:image/")
        && contains_ascii_case_insensitive(trimmed, BASE64_URL_MARKER)
}

fn inline_image_mime(value: &str) -> Option<&str> {
    let trimmed = value.trim_start();
    let comma = trimmed.find(',')?;
    let header = &trimmed[..comma];
    let separator = header.find(';').unwrap_or(header.len());
    Some(&header[5..separator])
}

fn is_omitted_inline_image_marker(value: &str) -> bool {
    value.trim_start().starts_with(INLINE_IMAGE_OMITTED_MARKER)
}

fn inline_image_data_url_json_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut index = start;
    while index < bytes.len() {
        match bytes[index] {
            b'"' | b'}' | b']' => return Some(index),
            b'\\' => index = (index + 2).min(bytes.len()),
            byte if byte.is_ascii_whitespace() => return Some(index),
            _ => index += 1,
        }
    }
    Some(bytes.len())
}

fn is_json_inline_image_data_url(value: &str) -> bool {
    if !starts_with_json_data_image_prefix(value) {
        return false;
    }
    let Some(comma) = value.find(',') else {
        return false;
    };
    let header = &value[..comma];
    contains_ascii_case_insensitive(header, BASE64_HEADER_MARKER) && value.len() > comma + 1
}

fn starts_with_json_data_image_prefix(value: &str) -> bool {
    let prefix_len = DATA_IMAGE_JSON_PREFIX.len();
    if value.len() <= prefix_len
        || !value[..prefix_len].eq_ignore_ascii_case(DATA_IMAGE_JSON_PREFIX)
    {
        return false;
    }
    match value.as_bytes().get(prefix_len) {
        Some(b'/') => true,
        Some(b'\\') => value.as_bytes().get(prefix_len + 1) == Some(&b'/'),
        _ => false,
    }
}

fn inline_image_mime_from_json_data_url(value: &str) -> Option<String> {
    let comma = value.find(',')?;
    let header = value[..comma].replace("\\/", "/");
    let separator = header.find(';').unwrap_or(header.len());
    let mime = header.get(5..separator)?.trim();
    if mime.is_empty() {
        None
    } else {
        Some(mime.to_string())
    }
}

fn contains_ascii_case_insensitive(haystack: &str, needle: &str) -> bool {
    find_ascii_case_insensitive(haystack, needle).is_some()
}

fn starts_with_ascii_case_insensitive(value: &str, prefix: &str) -> bool {
    value
        .as_bytes()
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    let needle = needle.as_bytes();
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn persisted_chat_payload_sanitizes_inline_image_parts() {
        let payload = json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "inspect"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}
            ],
            "meta": {"type": "model_context_internal"}
        });

        let sanitized = sanitize_persisted_chat_payload(&payload);
        let serialized = sanitized.to_string();

        assert!(!serialized.contains("data:image/png;base64"));
        assert!(serialized.contains("inline image omitted"));
    }

    #[test]
    fn image_path_marker_parses_hint_with_trailing_prose() {
        assert_eq!(
            image_path_marker_in_text("[image_path:/tmp/a.png] older frames keep only this path")
                .as_deref(),
            Some("/tmp/a.png")
        );
        assert_eq!(
            image_path_marker_in_text("[image_path:review/s_0005.00.png]").as_deref(),
            Some("review/s_0005.00.png")
        );
        assert!(image_path_marker_in_text("plain text").is_none());
        assert!(image_path_marker_in_text("[image_path:]").is_none());
    }

    #[test]
    fn omitted_placeholder_text_is_detected_exactly() {
        assert!(is_omitted_image_placeholder_text(INLINE_IMAGE_OMITTED_TEXT));
        assert!(is_omitted_image_placeholder_text(&format!(
            "  {INLINE_IMAGE_OMITTED_TEXT}  "
        )));
        assert!(!is_omitted_image_placeholder_text(
            "[image_path:/tmp/a.png]"
        ));
        assert!(!is_omitted_image_placeholder_text("inline image omitted"));
    }

    #[test]
    fn sanitized_followup_keeps_path_hint_for_restore() {
        let mut message = json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "prompt"},
                {"type": "text", "text": "[image_path:/tmp/a.png] older frames keep only this path"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}
            ]
        });

        sanitize_inline_images_in_value(&mut message);

        let parts = message["content"].as_array().expect("content array");
        assert!(is_omitted_image_placeholder_text(
            parts[2]["text"].as_str().unwrap()
        ));
        assert_eq!(
            image_path_marker_in_text(parts[1]["text"].as_str().unwrap()).as_deref(),
            Some("/tmp/a.png")
        );
        assert!(!message.to_string().contains("data:image/png;base64"));
    }

    #[test]
    fn json_text_sanitizer_replaces_inline_data_before_parse() {
        let payload = r#"{"content":[{"type":"image_url","image_url":{"url":"data:image/png;base64,AAAA"}}],"tail":true}"#;

        let sanitized = sanitize_inline_image_data_urls_in_json_text(payload);

        assert!(matches!(sanitized, Cow::Owned(_)));
        assert!(!sanitized.contains("data:image/png;base64"));
        let parsed = serde_json::from_str::<Value>(sanitized.as_ref()).expect("valid json");
        assert_eq!(parsed["tail"], json!(true));
        assert!(parsed.to_string().contains("omitted-inline-image"));
    }

    #[test]
    fn json_text_sanitizer_replaces_escaped_slash_data_urls() {
        let payload = r#"{"url":"data:image\/png;base64,AAAA"}"#;

        let sanitized = sanitize_inline_image_data_urls_in_json_text(payload);

        assert!(matches!(sanitized, Cow::Owned(_)));
        assert!(!sanitized.contains("data:image"));
        let parsed = serde_json::from_str::<Value>(sanitized.as_ref()).expect("valid json");
        assert!(parsed.to_string().contains("image/png"));
    }

    #[test]
    fn json_text_sanitizer_leaves_plain_json_borrowed() {
        let payload = r#"{"content":"plain"}"#;

        let sanitized = sanitize_inline_image_data_urls_in_json_text(payload);

        assert!(matches!(sanitized, Cow::Borrowed(_)));
    }
}
