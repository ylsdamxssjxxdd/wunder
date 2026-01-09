// LLM 适配：支持 OpenAI 兼容的 Chat Completions 调用。
use crate::config::LlmModelConfig;
use crate::schemas::TokenUsage;
use anyhow::{anyhow, Result};
use futures::StreamExt;
use reqwest::header::HeaderMap;
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Value};
use std::future::Future;
use std::time::Duration;
use url::form_urlencoded::byte_serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
    pub reasoning: String,
    pub usage: Option<TokenUsage>,
}

#[derive(Clone)]
pub struct LlmClient {
    http: Client,
    config: LlmModelConfig,
}

impl LlmClient {
    pub fn new(http: Client, config: LlmModelConfig) -> Self {
        Self { http, config }
    }

    pub async fn complete(&self, messages: &[ChatMessage]) -> Result<LlmResponse> {
        let response = self
            .http
            .post(self.endpoint())
            .headers(self.headers())
            .json(&self.build_payload(messages, false, false))
            .send()
            .await?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(anyhow!("模型请求失败: {status} {body}"));
        }
        let message = body
            .get("choices")
            .and_then(|value| value.get(0))
            .and_then(|value| value.get("message"))
            .cloned()
            .unwrap_or(Value::Null);
        let content = message
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let reasoning = message
            .get("reasoning_content")
            .or_else(|| message.get("reasoning"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let usage = normalize_usage(body.get("usage"));
        Ok(LlmResponse {
            content,
            reasoning,
            usage,
        })
    }

    pub async fn stream_complete_with_callback<F, Fut>(
        &self,
        messages: &[ChatMessage],
        mut on_delta: F,
    ) -> Result<LlmResponse>
    where
        F: FnMut(String, String) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        let mut include_usage = self.config.stream_include_usage.unwrap_or(true);
        let mut usage_fallback = include_usage;
        loop {
            let response = self
                .http
                .post(self.endpoint())
                .headers(self.headers())
                .json(&self.build_payload(messages, true, include_usage))
                .send()
                .await?;
            let status = response.status();
            if !status.is_success() {
                let text = response.text().await.unwrap_or_default();
                if usage_fallback && include_usage && matches!(status.as_u16(), 400 | 422) {
                    include_usage = false;
                    usage_fallback = false;
                    continue;
                }
                return Err(anyhow!("LLM stream request failed: {status} {text}"));
            }
            let mut stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut combined = String::new();
            let mut reasoning_combined = String::new();
            let mut usage: Option<TokenUsage> = None;
            while let Some(item) = stream.next().await {
                let bytes = item?;
                let part = String::from_utf8_lossy(&bytes);
                buffer.push_str(&part);
                while let Some(pos) = buffer.find('\n') {
                    let line = buffer[..pos].trim().to_string();
                    buffer = buffer[pos + 1..].to_string();
                    if line.is_empty() || !line.starts_with("data:") {
                        continue;
                    }
                    let data = line.trim_start_matches("data:").trim();
                    if data == "[DONE]" {
                        return Ok(LlmResponse {
                            content: combined,
                            reasoning: reasoning_combined,
                            usage,
                        });
                    }
                    if let Ok(payload) = serde_json::from_str::<Value>(data) {
                        if let Some(new_usage) = normalize_usage(payload.get("usage")) {
                            usage = Some(new_usage);
                        }
                        let delta = payload
                            .get("choices")
                            .and_then(|value| value.get(0))
                            .and_then(|value| value.get("delta"))
                            .cloned()
                            .unwrap_or(Value::Null);
                        let content_delta =
                            delta.get("content").and_then(Value::as_str).unwrap_or("");
                        let reasoning_delta = delta
                            .get("reasoning_content")
                            .or_else(|| delta.get("reasoning"))
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        if !content_delta.is_empty() {
                            combined.push_str(content_delta);
                        }
                        if !reasoning_delta.is_empty() {
                            reasoning_combined.push_str(reasoning_delta);
                        }
                        if !content_delta.is_empty() || !reasoning_delta.is_empty() {
                            on_delta(content_delta.to_string(), reasoning_delta.to_string())
                                .await?;
                        }
                    }
                }
            }
            return Ok(LlmResponse {
                content: combined,
                reasoning: reasoning_combined,
                usage,
            });
        }
    }
    fn endpoint(&self) -> String {
        let base = self
            .config
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com".to_string());
        if base.ends_with("/v1") {
            format!("{base}/chat/completions")
        } else {
            format!("{base}/v1/chat/completions")
        }
    }

    fn headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(api_key) = &self.config.api_key {
            if !api_key.is_empty() {
                let value = format!("Bearer {api_key}");
                if let Ok(header_value) = value.parse() {
                    headers.insert(reqwest::header::AUTHORIZATION, header_value);
                }
            }
        }
        headers
    }

    pub fn build_request_payload(&self, messages: &[ChatMessage], stream: bool) -> Value {
        self.build_payload(
            messages,
            stream,
            self.config.stream_include_usage.unwrap_or(true),
        )
    }

    fn build_payload(&self, messages: &[ChatMessage], stream: bool, include_usage: bool) -> Value {
        let temperature = round_f32(self.config.temperature.unwrap_or(0.7));
        let mut payload = json!({
            "model": self.config.model.clone().unwrap_or_else(|| "gpt-4".to_string()),
            "messages": messages,
            "temperature": temperature,
            "stream": stream,
        });
        if stream && include_usage {
            payload["stream_options"] = json!({ "include_usage": true });
        }
        if let Some(max_output) = self.config.max_output {
            if max_output > 0 {
                payload["max_tokens"] = json!(max_output);
            }
        }
        if let Some(stop) = &self.config.stop {
            if !stop.is_empty() {
                payload["stop"] = json!(stop);
            }
        }
        payload
    }
}

pub fn build_llm_client(config: &LlmModelConfig, http: Client) -> LlmClient {
    LlmClient::new(http, config.clone())
}

pub fn is_llm_configured(config: &LlmModelConfig) -> bool {
    config
        .base_url
        .as_ref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
        && config
            .model
            .as_ref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
        && config
            .api_key
            .as_ref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
}

fn normalize_usage(raw: Option<&Value>) -> Option<TokenUsage> {
    let Some(raw) = raw else {
        return None;
    };
    let Value::Object(map) = raw else {
        return None;
    };
    let to_u64 = |value: Option<&Value>| -> Option<u64> {
        match value {
            Some(Value::Number(num)) => num.as_u64(),
            Some(Value::String(text)) => text.trim().parse::<u64>().ok(),
            _ => None,
        }
    };
    let input = to_u64(map.get("input_tokens"))
        .or_else(|| to_u64(map.get("prompt_tokens")))
        .unwrap_or(0);
    let output = to_u64(map.get("output_tokens"))
        .or_else(|| to_u64(map.get("completion_tokens")))
        .unwrap_or(0);
    let total = to_u64(map.get("total_tokens")).unwrap_or(input + output);
    if input == 0 && output == 0 && total == 0 {
        return None;
    }
    Some(TokenUsage {
        input,
        output,
        total,
    })
}

fn round_f32(value: f32) -> f64 {
    const DECIMALS: i32 = 6;
    let factor = 10_f64.powi(DECIMALS);
    ((value as f64) * factor).round() / factor
}

const PRIMARY_CONTEXT_KEYS: [&str; 10] = [
    "context_length",
    "context_window",
    "max_context",
    "max_context_length",
    "context_tokens",
    "max_model_len",
    "max_seq_len",
    "maxSeqLen",
    "max_sequence_length",
    "max_input_tokens",
];
const FALLBACK_CONTEXT_KEYS: [&str; 2] = ["max_total_tokens", "max_tokens"];
const LLAMA_CPP_CONTEXT_KEYS: [&str; 2] = ["n_ctx", "n_ctx_train"];
const TRITON_CONTEXT_KEYS: [&str; 2] = ["max_seq_len", "maxSeqLen"];
const CONFIG_CONTEXT_KEYS: [&str; 2] = ["maxSeqLen", "max_seq_len"];

pub async fn probe_openai_context_window(
    base_url: &str,
    api_key: &str,
    model: &str,
    timeout_s: u64,
) -> Result<Option<u32>> {
    let endpoint = normalize_base_url(base_url).unwrap_or_default();
    let model = model.trim();
    if endpoint.is_empty() || model.is_empty() {
        return Ok(None);
    }
    let timeout = Duration::from_secs(timeout_s.max(5));
    let client = Client::builder().timeout(timeout).build()?;
    let headers = build_headers(api_key);
    let model_encoded = encode_path_component(model);

    if let Some(payload) = fetch_json(
        &client,
        &headers,
        &format!("{endpoint}/models/{model_encoded}"),
    )
    .await
    {
        if let Some(value) = find_context_value(&payload, &PRIMARY_CONTEXT_KEYS) {
            return Ok(Some(value));
        }
        if let Some(value) = find_context_value(&payload, &FALLBACK_CONTEXT_KEYS) {
            return Ok(Some(value));
        }
    }

    if let Some(payload) = fetch_json(&client, &headers, &format!("{endpoint}/models")).await {
        let entry = select_model_entry(&payload, model).unwrap_or(&payload);
        if let Some(value) = find_context_value(entry, &PRIMARY_CONTEXT_KEYS) {
            return Ok(Some(value));
        }
        if let Some(value) = find_context_value(entry, &FALLBACK_CONTEXT_KEYS) {
            return Ok(Some(value));
        }
    }

    if let Some(props_url) = normalize_llama_props_url(base_url) {
        if let Some(payload) = fetch_json(&client, &headers, &props_url).await {
            if let Some(value) = find_context_value(&payload, &LLAMA_CPP_CONTEXT_KEYS) {
                return Ok(Some(value));
            }
        }
    }

    if let Some(root) = normalize_root_url(base_url) {
        let triton_url = format!("{root}/v2/models/{model_encoded}/config");
        if let Some(payload) = fetch_json(&client, &headers, &triton_url).await {
            if let Some(value) = find_context_value(&payload, &TRITON_CONTEXT_KEYS) {
                return Ok(Some(value));
            }
        }
    }

    if let Some(payload) = fetch_json(&client, &headers, &format!("{endpoint}/config")).await {
        if let Some(value) = find_context_value(&payload, &CONFIG_CONTEXT_KEYS) {
            return Ok(Some(value));
        }
    }

    Ok(None)
}

fn encode_path_component(value: &str) -> String {
    byte_serialize(value.as_bytes()).collect::<String>()
}

fn normalize_base_url(base_url: &str) -> Option<String> {
    let cleaned = base_url.trim().trim_end_matches('/');
    if cleaned.is_empty() {
        return None;
    }
    if cleaned.ends_with("/v1") {
        Some(cleaned.to_string())
    } else {
        Some(format!("{cleaned}/v1"))
    }
}

fn normalize_root_url(base_url: &str) -> Option<String> {
    let cleaned = base_url.trim().trim_end_matches('/');
    if cleaned.is_empty() {
        return None;
    }
    let root = if cleaned.ends_with("/v1") {
        cleaned.trim_end_matches("/v1").trim_end_matches('/')
    } else {
        cleaned
    };
    if root.is_empty() {
        None
    } else {
        Some(root.to_string())
    }
}

fn normalize_llama_props_url(base_url: &str) -> Option<String> {
    let root = normalize_root_url(base_url)?;
    Some(format!("{root}/props"))
}

fn build_headers(api_key: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return headers;
    }
    if let Ok(value) = format!("Bearer {api_key}").parse() {
        headers.insert(reqwest::header::AUTHORIZATION, value);
    }
    headers
}

async fn fetch_json(client: &Client, headers: &HeaderMap, url: &str) -> Option<Value> {
    let response = client.get(url).headers(headers.clone()).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json::<Value>().await.ok()
}

fn find_context_value(payload: &Value, keys: &[&str]) -> Option<u32> {
    match payload {
        Value::Object(map) => {
            for key in keys {
                if let Some(value) = map.get(*key) {
                    if let Some(parsed) = extract_int(value) {
                        return Some(parsed);
                    }
                }
            }
            for value in map.values() {
                if let Some(found) = find_context_value(value, keys) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(|item| find_context_value(item, keys)),
        _ => None,
    }
}

fn extract_int(value: &Value) -> Option<u32> {
    match value {
        Value::Number(num) => num.as_u64().and_then(|value| u32::try_from(value).ok()),
        Value::String(text) => text.trim().parse::<u32>().ok(),
        _ => None,
    }
}

fn select_model_entry<'a>(payload: &'a Value, model: &str) -> Option<&'a Value> {
    let candidates = payload
        .get("data")
        .or_else(|| payload.get("models"))
        .or_else(|| payload.get("result"))
        .and_then(Value::as_array)?;
    for item in candidates {
        let obj = item.as_object()?;
        let id = obj
            .get("id")
            .or_else(|| obj.get("name"))
            .or_else(|| obj.get("model"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if id == model {
            return Some(item);
        }
    }
    None
}
