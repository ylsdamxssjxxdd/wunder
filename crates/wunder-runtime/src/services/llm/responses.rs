use super::provider::is_openai_responses_model;
use super::{extract_stream_text, normalize_provider, resolve_base_url, DEFAULT_OPENAI_BASE_URL};
use crate::config::LlmModelConfig;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const MAX_REPLAY_TOOL_ARGUMENT_BYTES: usize = 8 * 1024;

pub(super) fn replay_scope(endpoint: &str, config: &LlmModelConfig) -> String {
    let identity = json!([
        normalize_provider(config.provider.as_deref()),
        endpoint,
        config.model.as_deref().unwrap_or("gpt-4"),
    ]);
    format!("{:x}", Sha256::digest(identity.to_string()))
}

pub(super) fn uses_openai_options(config: &LlmModelConfig) -> bool {
    let provider = normalize_provider(config.provider.as_deref());
    if provider == "openai"
        || (provider == "openai_compatible" && is_openai_responses_model(config.model.as_deref()))
    {
        return true;
    }
    let base = resolve_base_url(config).unwrap_or_else(|| DEFAULT_OPENAI_BASE_URL.to_string());
    reqwest::Url::parse(&base)
        .is_ok_and(|url| url.scheme() == "https" && url.host_str() == Some("api.openai.com"))
}

pub(super) fn supports_temperature(model: Option<&str>, effort: Option<&str>) -> bool {
    let model = model.unwrap_or_default().trim().to_ascii_lowercase();
    if ["o1", "o3", "o4"].iter().any(|family| {
        model
            .strip_prefix(family)
            .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with('-'))
    }) {
        return false;
    }
    if !["gpt-5"].iter().any(|family| {
        model
            .strip_prefix(family)
            .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with(['-', '.']))
    }) {
        return true;
    }
    effort == Some("none")
        && ["gpt-5.1", "gpt-5.2", "gpt-5.4"].iter().any(|family| {
            model.strip_prefix(family).is_some_and(|suffix| {
                suffix.is_empty() || suffix.strip_prefix('-').is_some_and(is_dated_snapshot)
            })
        })
}

fn is_dated_snapshot(suffix: &str) -> bool {
    suffix.len() == 10
        && suffix.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 4 | 7) {
                byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
}

pub(crate) const REPLAY_FIELD: &str = "responses_reasoning";

#[derive(Debug, Default)]
pub(super) struct OutputItems(BTreeMap<usize, Value>);

impl OutputItems {
    pub(super) fn record(&mut self, index: usize, item: &Value) {
        let fields: &[&str] = match item_kind(item) {
            Some("reasoning") if is_replayable_reasoning(item) => &[
                "type",
                "id",
                "summary",
                "content",
                "encrypted_content",
                "status",
            ],
            Some("message") if is_replayable_message(item) => {
                &["type", "id", "role", "content", "phase", "status"]
            }
            Some("function_call") if is_replayable_call(item, "arguments") => {
                &["type", "id", "name", "call_id", "arguments", "status"]
            }
            Some("custom_tool_call") if is_replayable_call(item, "input") => {
                &["type", "id", "name", "call_id", "input"]
            }
            _ => return,
        };
        let mut output = serde_json::Map::new();
        for field in fields {
            if let Some(value) = item.get(*field) {
                output.insert((*field).to_string(), value.clone());
            }
        }
        self.0.insert(index, Value::Object(output));
    }

    pub(super) fn record_response(&mut self, response: &Value) {
        if let Some(output) = response
            .get("output")
            .and_then(Value::as_array)
            .filter(|output| !output.is_empty())
        {
            let mut streamed = std::mem::take(&mut self.0);
            for (index, item) in output.iter().enumerate() {
                if item_kind(item) == Some("reasoning") && !is_replayable_reasoning(item) {
                    if let Some(previous) = streamed.remove(&index).filter(|previous| {
                        is_replayable_reasoning(previous) && previous["id"] == item["id"]
                    }) {
                        self.0.insert(index, previous);
                    }
                } else {
                    self.record(index, item);
                }
            }
        }
    }

    pub(super) fn into_replay(self, scope: String) -> Option<Value> {
        (!self.0.is_empty()).then(|| {
            json!({
                "scope": scope,
                "items": self.0.into_values().collect::<Vec<_>>(),
            })
        })
    }

    pub(super) fn from_body(body: &Value, scope: String) -> Option<Value> {
        let mut items = Self::default();
        items.record_response(body.get("response").unwrap_or(body));
        items.into_replay(scope)
    }
}

pub(super) fn replay_items<'a>(replay: &'a Value, scope: &str) -> &'a [Value] {
    replay
        .get("scope")
        .and_then(Value::as_str)
        .filter(|value| *value == scope)
        .and_then(|_| replay.get("items").and_then(Value::as_array))
        .map_or(&[], Vec::as_slice)
}

pub(super) fn matches_output(items: &[Value], content: &Value, calls: &[Value]) -> bool {
    let expected_text = match content {
        Value::String(text) => text.as_str(),
        Value::Null => "",
        _ => return false,
    };
    let mut remaining = expected_text;
    let mut calls = calls.iter();
    let mut has_output = false;
    for item in items {
        match item_kind(item) {
            Some("reasoning") if is_replayable_reasoning(item) => {}
            Some("message") if is_replayable_message(item) => {
                let text = extract_stream_text(Some(&item["content"]));
                let Some(rest) = remaining.strip_prefix(text.as_str()) else {
                    return false;
                };
                remaining = rest;
                has_output = true;
            }
            Some(kind @ ("function_call" | "custom_tool_call")) => {
                let Some(projected) = calls.next() else {
                    return false;
                };
                let args_match = if kind == "function_call" {
                    item["arguments"]
                        .as_str()
                        .is_some_and(|raw| projected["arguments"].as_str() == Some(raw.trim()))
                } else {
                    item["input"].is_string() && item["input"] == projected["input"]
                };
                if projected["type"] != kind
                    || item["call_id"] != projected["call_id"]
                    || item["name"] != projected["name"]
                    || !args_match
                {
                    return false;
                }
                has_output = true;
            }
            _ => return false,
        }
    }
    has_output && remaining.is_empty() && calls.next().is_none()
}

fn item_kind(item: &Value) -> Option<&str> {
    item.get("type").and_then(Value::as_str)
}

fn is_replayable_message(item: &Value) -> bool {
    item["role"] == "assistant"
        && item["content"].as_array().is_some_and(|parts| {
            !parts.is_empty()
                && parts
                    .iter()
                    .all(|part| item_kind(part) == Some("output_text") && part["text"].is_string())
        })
}

fn is_replayable_call(item: &Value, argument_key: &str) -> bool {
    ["call_id", "name"].iter().all(|key| {
        item.get(*key)
            .and_then(Value::as_str)
            .is_some_and(|value| !value.is_empty())
    }) && item[argument_key]
        .as_str()
        .is_some_and(|arguments| arguments.len() <= MAX_REPLAY_TOOL_ARGUMENT_BYTES)
}

pub(super) fn is_replayable_reasoning(item: &Value) -> bool {
    item["type"] == "reasoning"
        && item
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| !id.is_empty())
        && item
            .get("encrypted_content")
            .and_then(Value::as_str)
            .is_some_and(|content| !content.is_empty())
}
