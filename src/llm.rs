// LLM 适配：支持 OpenAI 兼容的 Chat Completions 调用。
use crate::config::LlmModelConfig;
use crate::schemas::TokenUsage;
use anyhow::{anyhow, Result};
use futures::StreamExt;
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Value};
use std::future::Future;

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
        F: FnMut(String) -> Fut,
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
                            on_delta(content_delta.to_string()).await?;
                        }
                        if !reasoning_delta.is_empty() {
                            reasoning_combined.push_str(reasoning_delta);
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
        let temperature = self.config.temperature.unwrap_or(0.7);
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
