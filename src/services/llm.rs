// LLM 适配：支持 OpenAI 兼容的 Chat Completions 调用。
use crate::config::LlmModelConfig;
use crate::schemas::TokenUsage;
use anyhow::{anyhow, Context, Result};
use futures::StreamExt;
use reqwest::header::HeaderMap;
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Value};
use std::future::Future;
use std::time::Duration;
use tracing::warn;
use url::form_urlencoded::byte_serialize;

const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";
const DEFAULT_SILICONFLOW_BASE_URL: &str = "https://api.siliconflow.cn/v1";
const DEFAULT_DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";
const DEFAULT_MOONSHOT_BASE_URL: &str = "https://api.moonshot.ai/v1";
const DEFAULT_QWEN_BASE_URL: &str = "https://dashscope.aliyuncs.com/compatible-mode/v1";
const DEFAULT_GROQ_BASE_URL: &str = "https://api.groq.com/openai/v1";
const DEFAULT_MISTRAL_BASE_URL: &str = "https://api.mistral.ai/v1";
const DEFAULT_TOGETHER_BASE_URL: &str = "https://api.together.xyz/v1";
const DEFAULT_OLLAMA_BASE_URL: &str = "http://127.0.0.1:11434/v1";
const DEFAULT_LMSTUDIO_BASE_URL: &str = "http://127.0.0.1:1234/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelType {
    Llm,
    Embedding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallMode {
    ToolCall,
    FunctionCall,
}

pub fn normalize_model_type(value: Option<&str>) -> ModelType {
    let raw = value.unwrap_or("").trim();
    if raw.is_empty() {
        return ModelType::Llm;
    }
    match raw.to_ascii_lowercase().replace(['-', ' '], "_").as_str() {
        "embedding" | "embed" | "emb" => ModelType::Embedding,
        _ => ModelType::Llm,
    }
}

pub fn is_embedding_model(config: &LlmModelConfig) -> bool {
    normalize_model_type(config.model_type.as_deref()) == ModelType::Embedding
}

pub fn is_llm_model(config: &LlmModelConfig) -> bool {
    !is_embedding_model(config)
}

pub fn normalize_tool_call_mode(value: Option<&str>) -> ToolCallMode {
    let raw = value.unwrap_or("").trim();
    if raw.is_empty() {
        return ToolCallMode::ToolCall;
    }
    match raw.to_ascii_lowercase().replace(['-', ' '], "_").as_str() {
        "function_call" | "functioncall" | "function" | "fc" => ToolCallMode::FunctionCall,
        "tool_call" | "toolcall" | "tool" | "tag" | "xml" => ToolCallMode::ToolCall,
        _ => ToolCallMode::ToolCall,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
    pub reasoning: String,
    pub usage: Option<TokenUsage>,
    pub tool_calls: Option<Value>,
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
        self.complete_with_tools(messages, None).await
    }

    pub async fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: Option<&[Value]>,
    ) -> Result<LlmResponse> {
        let response = self
            .http
            .post(self.endpoint())
            .headers(self.headers())
            .json(&self.build_payload(messages, false, false, tools))
            .send()
            .await?;
        let status = response.status();
        let body_text = response.text().await.context("read llm response body")?;
        let body = match serde_json::from_str::<Value>(&body_text) {
            Ok(value) => value,
            Err(err) => {
                warn!(
                    "LLM response json parse failed: {err}, body={}",
                    truncate_text(&body_text, 2048)
                );
                Value::Null
            }
        };
        if !status.is_success() {
            let detail = if body == Value::Null {
                json!({ "raw": truncate_text(&body_text, 2048) })
            } else {
                body
            };
            return Err(anyhow!("LLM request failed: {status} {detail}"));
        }
        if body == Value::Null {
            return Err(anyhow!(
                "LLM response parse failed: {}",
                truncate_text(&body_text, 2048)
            ));
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
        let tool_calls = extract_tool_calls(&message);
        let usage = normalize_usage(body.get("usage"));
        Ok(LlmResponse {
            content,
            reasoning,
            usage,
            tool_calls,
        })
    }

    pub async fn stream_complete_with_callback<F, Fut>(
        &self,
        messages: &[ChatMessage],
        on_delta: F,
    ) -> Result<LlmResponse>
    where
        F: FnMut(String, String) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        self.stream_complete_with_callback_with_tools(messages, None, on_delta)
            .await
    }

    pub async fn stream_complete_with_callback_with_tools<F, Fut>(
        &self,
        messages: &[ChatMessage],
        tools: Option<&[Value]>,
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
                .json(&self.build_payload(messages, true, include_usage, tools))
                .send()
                .await?;
            let status = response.status();
            if !status.is_success() {
                let text = match response.text().await {
                    Ok(value) => value,
                    Err(err) => {
                        return Err(anyhow!(
                            "LLM stream request failed: {status} (read body failed: {err})"
                        ));
                    }
                };
                if usage_fallback && include_usage && matches!(status.as_u16(), 400 | 422) {
                    include_usage = false;
                    usage_fallback = false;
                    continue;
                }
                return Err(anyhow!(
                    "LLM stream request failed: {status} {}",
                    truncate_text(&text, 2048)
                ));
            }
            let mut stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut combined = String::new();
            let mut reasoning_combined = String::new();
            let mut usage: Option<TokenUsage> = None;
            let mut tool_calls_accumulator: Vec<StreamToolCall> = Vec::new();
            let mut saw_done = false;
            while let Some(item) = stream.next().await {
                let bytes = item?;
                let part = String::from_utf8_lossy(&bytes);
                buffer.push_str(&part);

                while let Some(event_block) = take_next_sse_event(&mut buffer) {
                    if process_sse_event_block(
                        event_block.as_str(),
                        &mut combined,
                        &mut reasoning_combined,
                        &mut usage,
                        &mut tool_calls_accumulator,
                        &mut on_delta,
                    )
                    .await?
                    {
                        saw_done = true;
                        break;
                    }
                }

                if saw_done {
                    break;
                }
            }

            while !saw_done {
                let Some(event_block) = take_next_sse_event(&mut buffer) else {
                    break;
                };
                if process_sse_event_block(
                    event_block.as_str(),
                    &mut combined,
                    &mut reasoning_combined,
                    &mut usage,
                    &mut tool_calls_accumulator,
                    &mut on_delta,
                )
                .await?
                {
                    saw_done = true;
                    break;
                }
            }

            if !saw_done && !buffer.trim().is_empty() {
                if process_sse_event_block(
                    buffer.as_str(),
                    &mut combined,
                    &mut reasoning_combined,
                    &mut usage,
                    &mut tool_calls_accumulator,
                    &mut on_delta,
                )
                .await?
                {
                    saw_done = true;
                }
            }

            let tool_calls = finalize_stream_tool_calls(&tool_calls_accumulator);
            if !saw_done
                && combined.trim().is_empty()
                && reasoning_combined.trim().is_empty()
                && tool_calls.is_none()
            {
                warn!("LLM stream ended without [DONE] and without payload");
            }
            return Ok(LlmResponse {
                content: combined,
                reasoning: reasoning_combined,
                usage,
                tool_calls,
            });
        }
    }
    fn endpoint(&self) -> String {
        let base =
            resolve_base_url(&self.config).unwrap_or_else(|| "https://api.openai.com".to_string());
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
            None,
        )
    }

    pub fn build_request_payload_with_tools(
        &self,
        messages: &[ChatMessage],
        stream: bool,
        tools: Option<&[Value]>,
    ) -> Value {
        self.build_payload(
            messages,
            stream,
            self.config.stream_include_usage.unwrap_or(true),
            tools,
        )
    }

    fn build_payload(
        &self,
        messages: &[ChatMessage],
        stream: bool,
        include_usage: bool,
        tools: Option<&[Value]>,
    ) -> Value {
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
        if let Some(tool_defs) = tools {
            if !tool_defs.is_empty() {
                payload["tools"] = Value::Array(tool_defs.to_vec());
                payload["tool_choice"] = json!("auto");
            }
        }
        payload
    }
}

pub fn build_llm_client(config: &LlmModelConfig, http: Client) -> LlmClient {
    LlmClient::new(http, config.clone())
}

pub fn is_llm_configured(config: &LlmModelConfig) -> bool {
    resolve_base_url(config)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
        && config
            .model
            .as_ref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
}

pub fn is_embedding_configured(config: &LlmModelConfig) -> bool {
    is_llm_configured(config)
}

pub async fn embed_texts(
    config: &LlmModelConfig,
    inputs: &[String],
    timeout_s: u64,
) -> Result<Vec<Vec<f32>>> {
    if inputs.is_empty() {
        return Ok(Vec::new());
    }
    let base_url =
        resolve_base_url(config).ok_or_else(|| anyhow!("embedding base_url is required"))?;
    let model = config
        .model
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("embedding model is required"))?;
    let endpoint = if base_url.ends_with("/v1") {
        format!("{base_url}/embeddings")
    } else {
        format!("{base_url}/v1/embeddings")
    };
    let timeout = Duration::from_secs(timeout_s.max(5));
    let client = Client::builder().timeout(timeout).build()?;
    let payload = json!({
        "model": model,
        "input": inputs,
    });
    let response = client
        .post(endpoint)
        .headers(build_headers(config.api_key.as_deref().unwrap_or("")))
        .json(&payload)
        .send()
        .await?;
    let status = response.status();
    let body_text = response
        .text()
        .await
        .context("read embedding response body")?;
    let body = match serde_json::from_str::<Value>(&body_text) {
        Ok(value) => value,
        Err(err) => {
            warn!(
                "Embedding response json parse failed: {err}, body={}",
                truncate_text(&body_text, 2048)
            );
            Value::Null
        }
    };
    if !status.is_success() {
        let detail = if body == Value::Null {
            json!({ "raw": truncate_text(&body_text, 2048) })
        } else {
            body
        };
        return Err(anyhow!("embedding request failed: {status} {detail}"));
    }
    if body == Value::Null {
        return Err(anyhow!(
            "embedding response parse failed: {}",
            truncate_text(&body_text, 2048)
        ));
    }
    let data = body
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| anyhow!("embedding response missing data"))?;
    let mut outputs = vec![Vec::new(); inputs.len()];
    for item in data {
        let index = item.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        let embedding = item
            .get("embedding")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("embedding response missing embedding"))?;
        let mut vector = Vec::with_capacity(embedding.len());
        for value in embedding {
            let num = value
                .as_f64()
                .ok_or_else(|| anyhow!("embedding value is not number"))?;
            vector.push(num as f32);
        }
        if index < outputs.len() {
            outputs[index] = vector;
        }
    }
    Ok(outputs)
}

pub fn normalize_provider(provider: Option<&str>) -> String {
    let raw = provider.unwrap_or("openai_compatible").trim();
    if raw.is_empty() {
        return "openai_compatible".to_string();
    }
    let normalized = raw.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    match normalized.as_str() {
        "openai_compat" => "openai_compatible".to_string(),
        "openai_native" => "openai".to_string(),
        "openai" => "openai".to_string(),
        "openai_compatible" => "openai_compatible".to_string(),
        "openrouter" => "openrouter".to_string(),
        "silicon_flow" => "siliconflow".to_string(),
        "siliconflow" => "siliconflow".to_string(),
        "deepseek" => "deepseek".to_string(),
        "moonshot" => "moonshot".to_string(),
        "kimi" => "moonshot".to_string(),
        "dashscope" => "qwen".to_string(),
        "qwen" => "qwen".to_string(),
        "groq" => "groq".to_string(),
        "mistral" => "mistral".to_string(),
        "together" => "together".to_string(),
        "ollama" => "ollama".to_string(),
        "lm_studio" => "lmstudio".to_string(),
        "lmstudio" => "lmstudio".to_string(),
        other => other.to_string(),
    }
}

pub fn provider_default_base_url(provider: &str) -> Option<&'static str> {
    match provider {
        "openai" => Some(DEFAULT_OPENAI_BASE_URL),
        "openrouter" => Some(DEFAULT_OPENROUTER_BASE_URL),
        "siliconflow" => Some(DEFAULT_SILICONFLOW_BASE_URL),
        "deepseek" => Some(DEFAULT_DEEPSEEK_BASE_URL),
        "moonshot" => Some(DEFAULT_MOONSHOT_BASE_URL),
        "qwen" => Some(DEFAULT_QWEN_BASE_URL),
        "groq" => Some(DEFAULT_GROQ_BASE_URL),
        "mistral" => Some(DEFAULT_MISTRAL_BASE_URL),
        "together" => Some(DEFAULT_TOGETHER_BASE_URL),
        "ollama" => Some(DEFAULT_OLLAMA_BASE_URL),
        "lmstudio" => Some(DEFAULT_LMSTUDIO_BASE_URL),
        _ => None,
    }
}

pub fn is_openai_compatible_provider(provider: &str) -> bool {
    let normalized = normalize_provider(Some(provider));
    if normalized == "openai_compatible" {
        return true;
    }
    provider_default_base_url(&normalized).is_some()
}

fn resolve_base_url(config: &LlmModelConfig) -> Option<String> {
    let inline = config
        .base_url
        .as_deref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    if let Some(value) = inline {
        return Some(value.to_string());
    }
    let provider = normalize_provider(config.provider.as_deref());
    provider_default_base_url(&provider).map(|value| value.to_string())
}

fn normalize_usage(raw: Option<&Value>) -> Option<TokenUsage> {
    let raw = raw?;
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

fn extract_tool_calls(message: &Value) -> Option<Value> {
    let Value::Object(map) = message else {
        return None;
    };
    map.get("tool_calls")
        .or_else(|| map.get("tool_call"))
        .or_else(|| map.get("function_call"))
        .or_else(|| map.get("functionCall"))
        .cloned()
}

fn extract_stream_text(value: Option<&Value>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    match value {
        Value::String(text) => text.to_string(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item.as_str()
                    .or_else(|| item.get("text").and_then(Value::as_str))
                    .or_else(|| item.get("content").and_then(Value::as_str))
            })
            .collect::<String>(),
        Value::Object(map) => map
            .get("text")
            .and_then(Value::as_str)
            .or_else(|| map.get("content").and_then(Value::as_str))
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    }
}

fn take_next_sse_event(buffer: &mut String) -> Option<String> {
    let newline_event = buffer.find("\n\n").map(|index| (index, 2));
    let crlf_event = buffer.find("\r\n\r\n").map(|index| (index, 4));
    let (index, delimiter_len) = match (newline_event, crlf_event) {
        (Some(left), Some(right)) => {
            if left.0 <= right.0 {
                left
            } else {
                right
            }
        }
        (Some(value), None) | (None, Some(value)) => value,
        (None, None) => return None,
    };

    let event = buffer[..index].to_string();
    *buffer = buffer[index + delimiter_len..].to_string();
    Some(event)
}

async fn process_sse_event_block<F, Fut>(
    block: &str,
    combined: &mut String,
    reasoning_combined: &mut String,
    usage: &mut Option<TokenUsage>,
    tool_calls_accumulator: &mut Vec<StreamToolCall>,
    on_delta: &mut F,
) -> Result<bool>
where
    F: FnMut(String, String) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let mut data_lines = Vec::new();
    for raw_line in block.lines() {
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(':') {
            continue;
        }
        if let Some(data) = trimmed.strip_prefix("data:") {
            data_lines.push(data.trim_start());
        }
    }

    if data_lines.is_empty() {
        return Ok(false);
    }

    let data = data_lines.join("\n");
    process_stream_payload(
        data.trim(),
        combined,
        reasoning_combined,
        usage,
        tool_calls_accumulator,
        on_delta,
    )
    .await
}

async fn process_stream_payload<F, Fut>(
    data: &str,
    combined: &mut String,
    reasoning_combined: &mut String,
    usage: &mut Option<TokenUsage>,
    tool_calls_accumulator: &mut Vec<StreamToolCall>,
    on_delta: &mut F,
) -> Result<bool>
where
    F: FnMut(String, String) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    if data.is_empty() {
        return Ok(false);
    }
    if data == "[DONE]" {
        return Ok(true);
    }

    match serde_json::from_str::<Value>(data) {
        Ok(payload) => {
            if let Some(new_usage) = normalize_usage(payload.get("usage")) {
                *usage = Some(new_usage);
            }
            let choice = payload.get("choices").and_then(|value| value.get(0));
            let delta = choice
                .and_then(|value| value.get("delta"))
                .cloned()
                .unwrap_or(Value::Null);
            let mut content_delta = extract_stream_text(delta.get("content"));
            if content_delta.is_empty() {
                content_delta = extract_stream_text(
                    choice
                        .and_then(|value| value.get("message"))
                        .and_then(|value| value.get("content")),
                );
            }
            let mut reasoning_delta = extract_stream_text(
                delta
                    .get("reasoning_content")
                    .or_else(|| delta.get("reasoning")),
            );
            if reasoning_delta.is_empty() {
                reasoning_delta =
                    extract_stream_text(choice.and_then(|value| value.get("message")).and_then(
                        |value| {
                            value
                                .get("reasoning_content")
                                .or_else(|| value.get("reasoning"))
                        },
                    ));
            }
            update_stream_tool_calls(tool_calls_accumulator, &delta);
            if delta.is_null() {
                if let Some(message) = choice.and_then(|value| value.get("message")) {
                    update_stream_tool_calls(tool_calls_accumulator, message);
                }
            }
            if !content_delta.is_empty() {
                combined.push_str(content_delta.as_str());
            }
            if !reasoning_delta.is_empty() {
                reasoning_combined.push_str(reasoning_delta.as_str());
            }
            if !content_delta.is_empty() || !reasoning_delta.is_empty() {
                on_delta(content_delta, reasoning_delta).await?;
            }
        }
        Err(err) => {
            warn!(
                "LLM stream json parse failed: {err}, data={}",
                truncate_text(data, 512)
            );
        }
    }

    Ok(false)
}

#[derive(Debug, Default, Clone)]
struct StreamToolCall {
    id: Option<String>,
    name: String,
    arguments: String,
}

fn update_stream_tool_calls(acc: &mut Vec<StreamToolCall>, delta: &Value) {
    let tool_calls_raw = delta.get("tool_calls").or_else(|| delta.get("tool_call"));
    let tool_calls = match tool_calls_raw {
        Some(Value::Array(items)) => Some(items.as_slice()),
        Some(Value::Object(_)) => tool_calls_raw.map(std::slice::from_ref),
        _ => None,
    };
    if let Some(items) = tool_calls {
        for item in items {
            if let Value::Object(map) = item {
                let index = map.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                while acc.len() <= index {
                    acc.push(StreamToolCall::default());
                }
                let slot = &mut acc[index];
                if let Some(id) = map.get("id").and_then(Value::as_str) {
                    slot.id = Some(id.to_string());
                }
                if let Some(function) = map.get("function") {
                    apply_function_delta(slot, function);
                }
            }
        }
    }

    if let Some(function_call) = delta.get("function_call") {
        if acc.is_empty() {
            acc.push(StreamToolCall::default());
        }
        apply_function_delta(&mut acc[0], function_call);
    }
}

fn apply_function_delta(slot: &mut StreamToolCall, function: &Value) {
    if let Value::Object(map) = function {
        if let Some(name) = map.get("name").and_then(Value::as_str) {
            slot.name.push_str(name);
        }
        if let Some(arguments) = map.get("arguments").and_then(Value::as_str) {
            slot.arguments.push_str(arguments);
        }
    }
}

fn finalize_stream_tool_calls(acc: &[StreamToolCall]) -> Option<Value> {
    let mut output = Vec::new();
    for call in acc {
        if call.name.trim().is_empty() {
            continue;
        }
        let mut payload = json!({
            "type": "function",
            "function": {
                "name": call.name,
                "arguments": call.arguments,
            }
        });
        if let Some(id) = &call.id {
            if let Value::Object(ref mut map) = payload {
                map.insert("id".to_string(), Value::String(id.clone()));
            }
        }
        output.push(payload);
    }
    if output.is_empty() {
        None
    } else {
        Some(Value::Array(output))
    }
}

fn round_f32(value: f32) -> f64 {
    const DECIMALS: i32 = 6;
    let factor = 10_f64.powi(DECIMALS);
    ((value as f64) * factor).round() / factor
}

fn truncate_text(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut output = text[..end].to_string();
    output.push_str("...");
    output
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_next_sse_event_handles_crlf_and_lf_delimiters() {
        let mut buffer = "data: {\"x\":1}\r\n\r\ndata: {\"y\":2}\n\n".to_string();
        let first = take_next_sse_event(&mut buffer).expect("first event");
        assert_eq!(first, "data: {\"x\":1}");
        let second = take_next_sse_event(&mut buffer).expect("second event");
        assert_eq!(second, "data: {\"y\":2}");
        assert!(take_next_sse_event(&mut buffer).is_none());
    }

    #[tokio::test]
    async fn process_sse_event_block_parses_delta_without_done() {
        let mut combined = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tool_calls = Vec::new();
        let mut on_delta = |_content: String, _reasoning: String| async { Ok(()) };

        let done = process_sse_event_block(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}",
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut on_delta,
        )
        .await
        .expect("process sse block");

        assert!(!done);
        assert_eq!(combined, "hello");
        assert!(reasoning.is_empty());
        assert!(usage.is_none());
    }

    #[tokio::test]
    async fn process_sse_event_block_uses_message_content_fallback() {
        let mut combined = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tool_calls = Vec::new();
        let mut on_delta = |_content: String, _reasoning: String| async { Ok(()) };

        let done = process_sse_event_block(
            "data: {\"choices\":[{\"message\":{\"content\":\"fallback\"}}]}",
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut on_delta,
        )
        .await
        .expect("process sse block");

        assert!(!done);
        assert_eq!(combined, "fallback");
    }

    #[tokio::test]
    async fn process_sse_event_block_supports_multiline_json_data() {
        let mut combined = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tool_calls = Vec::new();
        let mut on_delta = |_content: String, _reasoning: String| async { Ok(()) };

        let block = "event: message\n\
                     data: {\n\
                     data: \"choices\": [{\"delta\": {\"content\": \"ok\"}}]\n\
                     data: }";
        let done = process_sse_event_block(
            block,
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut on_delta,
        )
        .await
        .expect("process sse block");

        assert!(!done);
        assert_eq!(combined, "ok");
    }

    #[tokio::test]
    async fn process_sse_event_block_stops_on_done_payload() {
        let mut combined = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tool_calls = Vec::new();
        let mut on_delta = |_content: String, _reasoning: String| async { Ok(()) };

        let done = process_sse_event_block(
            "data: [DONE]",
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut on_delta,
        )
        .await
        .expect("process sse block");

        assert!(done);
        assert!(combined.is_empty());
        assert!(reasoning.is_empty());
    }
}
