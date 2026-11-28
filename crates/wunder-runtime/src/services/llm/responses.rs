use super::{normalize_provider, resolve_base_url, DEFAULT_OPENAI_BASE_URL};
use crate::config::LlmModelConfig;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub(super) fn replay_scope(endpoint: &str, config: &LlmModelConfig) -> String {
    let identity = json!([
        normalize_provider(config.provider.as_deref()),
        endpoint,
        config.model.as_deref().unwrap_or("gpt-4"),
    ]);
    format!("{:x}", Sha256::digest(identity.to_string()))
}

pub(super) fn is_official_endpoint(config: &LlmModelConfig) -> bool {
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
pub(super) struct ReasoningItems(BTreeMap<usize, Value>);

impl ReasoningItems {
    pub(super) fn record(&mut self, index: usize, item: &Value) {
        if is_replayable_reasoning(item) {
            self.0.insert(index, item.clone());
        }
    }

    pub(super) fn record_response(&mut self, response: &Value) {
        if let Some(output) = response.get("output").and_then(Value::as_array) {
            for (index, item) in output.iter().enumerate() {
                self.record(index, item);
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

pub(super) fn replay_items<'a>(replay: &'a Value, scope: &str) -> impl Iterator<Item = &'a Value> {
    replay
        .get("scope")
        .and_then(Value::as_str)
        .filter(|value| *value == scope)
        .and_then(|_| replay.get("items").and_then(Value::as_array))
        .into_iter()
        .flatten()
        .filter(|item| is_replayable_reasoning(item))
}

fn is_replayable_reasoning(item: &Value) -> bool {
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
