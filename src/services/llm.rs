// LLM 适配：支持 OpenAI 兼容的 Chat Completions 调用。
use crate::config::LlmModelConfig;
use crate::core::json_schema::normalize_tool_input_schema;
use crate::core::json_schema::normalize_tool_input_schema_for_openai;
use crate::core::tool_args::{
    normalize_tool_arguments_json as normalize_tool_arguments_json_lossy,
    sanitize_tool_call_payload,
};
use crate::schemas::TokenUsage;
use crate::tools::{extract_freeform_tool_input, is_freeform_tool_name};
use anyhow::{anyhow, Context, Result};
use futures::StreamExt;
use reqwest::header::HeaderMap;
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::future::Future;
use std::time::Duration;
use tracing::warn;
use url::{form_urlencoded::byte_serialize, Url};

const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com/v1";
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
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 32_768;
const DEFAULT_THINKING_TOKEN_BUDGET: u32 = 16_384;
const CHAT_COMPLETIONS_RESOURCE: &str = "chat/completions";
const RESPONSES_RESOURCE: &str = "responses";
const MESSAGES_RESOURCE: &str = "messages";
const EMBEDDINGS_RESOURCE: &str = "embeddings";
const ANTHROPIC_VERSION_HEADER_VALUE: &str = "2023-06-01";
const OPENAI_COMPAT_RESOURCE_SUFFIXES: [&[&str]; 4] = [
    &["chat", "completions"],
    &["responses"],
    &["embeddings"],
    &["models"],
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelType {
    Llm,
    Embedding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallMode {
    ToolCall,
    FunctionCall,
    FreeformCall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAiApiMode {
    ChatCompletions,
    Responses,
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
        return ToolCallMode::FunctionCall;
    }
    match raw.to_ascii_lowercase().replace(['-', ' '], "_").as_str() {
        "function_call" | "functioncall" | "function" | "fc" => ToolCallMode::FunctionCall,
        "freeform_call" | "freeformcall" | "freeform" | "custom_tool_call" => {
            ToolCallMode::FreeformCall
        }
        "tool_call" | "toolcall" | "tool" | "tag" | "xml" => ToolCallMode::ToolCall,
        _ => ToolCallMode::ToolCall,
    }
}

pub fn normalize_reasoning_effort(value: Option<&str>) -> Option<String> {
    let raw = value.unwrap_or("").trim();
    if raw.is_empty() {
        return None;
    }
    let normalized = raw.to_ascii_lowercase().replace(['-', ' '], "_");
    let canonical = match normalized.as_str() {
        "default" | "auto" | "inherit" => return None,
        "none" | "off" | "disable" | "disabled" => "none",
        "minimal" | "min" => "minimal",
        "low" => "low",
        "medium" | "med" | "normal" => "medium",
        "high" => "high",
        "xhigh" | "x_high" | "extra_high" | "very_high" => "xhigh",
        _ => return None,
    };
    Some(canonical.to_string())
}

fn disable_thinking_requested(config: &LlmModelConfig) -> bool {
    matches!(
        normalize_reasoning_effort(config.reasoning_effort.as_deref()).as_deref(),
        Some("none")
    )
}

fn should_emit_enable_thinking_flag(config: &LlmModelConfig) -> bool {
    matches!(
        normalize_provider(config.provider.as_deref()).as_str(),
        "openai_compatible" | "qwen" | "vllm" | "vllm_ascend"
    )
}

fn should_emit_vllm_chat_template_kwargs(config: &LlmModelConfig) -> bool {
    let provider = normalize_provider(config.provider.as_deref());
    matches!(
        provider.as_str(),
        "openai_compatible" | "vllm" | "vllm_ascend"
    )
}

fn should_emit_thinking_token_budget(config: &LlmModelConfig) -> bool {
    is_llm_model(config) && !matches!(normalize_provider(config.provider.as_deref()).as_str(), "anthropic")
}

fn resolved_max_output(config: &LlmModelConfig) -> u32 {
    config
        .max_output
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS)
}

fn resolved_thinking_token_budget(config: &LlmModelConfig) -> Option<u32> {
    if disable_thinking_requested(config) || !should_emit_thinking_token_budget(config) {
        return None;
    }
    Some(
        config
            .thinking_token_budget
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_THINKING_TOKEN_BUDGET),
    )
}

fn apply_disable_thinking_controls(payload: &mut Value, config: &LlmModelConfig) {
    if !disable_thinking_requested(config) {
        return;
    }
    if should_emit_enable_thinking_flag(config) {
        payload["enable_thinking"] = Value::Bool(false);
    }
    if should_emit_vllm_chat_template_kwargs(config) {
        let mut kwargs = payload
            .get("chat_template_kwargs")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        kwargs.insert("enable_thinking".to_string(), Value::Bool(false));
        payload["chat_template_kwargs"] = Value::Object(kwargs);
    }
}

pub fn resolve_tool_call_mode(config: &LlmModelConfig) -> ToolCallMode {
    if let Some(value) = config
        .tool_call_mode
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return normalize_tool_call_mode(Some(value));
    }
    if normalize_provider(config.provider.as_deref()) == "openai" {
        ToolCallMode::FreeformCall
    } else {
        ToolCallMode::FunctionCall
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

    fn is_anthropic_provider(&self) -> bool {
        normalize_provider(self.config.provider.as_deref()) == "anthropic"
    }

    pub async fn complete(&self, messages: &[ChatMessage]) -> Result<LlmResponse> {
        self.complete_with_tools(messages, None).await
    }

    pub async fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: Option<&[Value]>,
    ) -> Result<LlmResponse> {
        let api_mode = self.api_mode();
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
        let (content, reasoning, tool_calls) = if self.is_anthropic_provider() {
            parse_anthropic_body(&body)
        } else if matches!(api_mode, OpenAiApiMode::Responses)
            || body.get("output").is_some()
            || body.get("response").is_some()
        {
            parse_responses_body(&body)
        } else {
            parse_chat_completion_body(&body)
        };
        let usage = normalize_usage(body.get("usage")).or_else(|| {
            body.get("response")
                .and_then(|value| normalize_usage(value.get("usage")))
        });
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
                if should_fallback_stream_failure_to_non_stream(status.as_u16(), &text) {
                    return self
                        .complete_with_tools(messages, tools)
                        .await
                        .map_err(|fallback_err| {
                            anyhow!(
                                "LLM stream request failed: {status} {}; fallback request failed: {fallback_err}",
                                truncate_text(&text, 2048)
                            )
                        });
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

            if !saw_done
                && !buffer.trim().is_empty()
                && process_sse_event_block(
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

            let tool_calls = finalize_stream_tool_calls(&tool_calls_accumulator);
            let stream_payload_empty = combined.trim().is_empty()
                && reasoning_combined.trim().is_empty()
                && tool_calls.is_none();
            if !saw_done && stream_payload_empty {
                warn!("LLM stream ended without [DONE] and without payload, fallback to non-stream request");
                match self.complete_with_tools(messages, tools).await {
                    Ok(fallback) => {
                        if !fallback.content.is_empty() || !fallback.reasoning.is_empty() {
                            on_delta(fallback.content.clone(), fallback.reasoning.clone()).await?;
                        }
                        return Ok(fallback);
                    }
                    Err(err) => {
                        return Err(anyhow!(
                            "LLM stream ended without [DONE] and without payload; fallback request failed: {err}"
                        ));
                    }
                }
            }
            return Ok(LlmResponse {
                content: combined,
                reasoning: reasoning_combined,
                usage,
                tool_calls,
            });
        }
    }
    fn api_mode(&self) -> OpenAiApiMode {
        resolve_openai_api_mode(&self.config)
    }
    fn endpoint(&self) -> String {
        let base =
            resolve_base_url(&self.config).unwrap_or_else(|| DEFAULT_OPENAI_BASE_URL.to_string());
        if self.is_anthropic_provider() {
            return build_anthropic_messages_endpoint(&base)
                .unwrap_or_else(|| format!("{DEFAULT_ANTHROPIC_BASE_URL}/{MESSAGES_RESOURCE}"));
        }
        let resource = match self.api_mode() {
            OpenAiApiMode::Responses => RESPONSES_RESOURCE,
            OpenAiApiMode::ChatCompletions => CHAT_COMPLETIONS_RESOURCE,
        };
        build_openai_resource_endpoint(&base, resource)
            .unwrap_or_else(|| format!("{DEFAULT_OPENAI_BASE_URL}/{resource}"))
    }

    fn headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        let Some(api_key) = self
            .config
            .api_key
            .as_deref()
            .and_then(normalize_api_key_token)
        else {
            return headers;
        };

        if self.is_anthropic_provider() {
            if let Ok(value) = api_key.parse() {
                headers.insert(reqwest::header::HeaderName::from_static("x-api-key"), value);
            }
            if let Ok(value) = ANTHROPIC_VERSION_HEADER_VALUE.parse() {
                headers.insert(
                    reqwest::header::HeaderName::from_static("anthropic-version"),
                    value,
                );
            }
        }

        let value = format!("Bearer {api_key}");
        if let Ok(header_value) = value.parse() {
            headers.insert(reqwest::header::AUTHORIZATION, header_value);
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
        if self.is_anthropic_provider() {
            return self.build_anthropic_payload(messages, stream, include_usage, tools);
        }
        match self.api_mode() {
            OpenAiApiMode::Responses => {
                self.build_responses_payload(messages, stream, include_usage, tools)
            }
            OpenAiApiMode::ChatCompletions => {
                self.build_chat_payload(messages, stream, include_usage, tools)
            }
        }
    }

    fn build_anthropic_payload(
        &self,
        messages: &[ChatMessage],
        stream: bool,
        _include_usage: bool,
        tools: Option<&[Value]>,
    ) -> Value {
        let (system_prompt, anthropic_messages) = build_anthropic_messages(messages);
        let max_tokens = resolved_max_output(&self.config);
        let mut payload = json!({
            "model": self
                .config
                .model
                .clone()
                .unwrap_or_else(|| "claude-sonnet-4-5".to_string()),
            "messages": anthropic_messages,
            "max_tokens": max_tokens,
            "stream": stream,
        });
        if let Some(system_prompt) = system_prompt.filter(|value| !value.trim().is_empty()) {
            payload["system"] = Value::String(system_prompt);
        }
        if let Some(temperature) = self.config.temperature {
            payload["temperature"] = json!(round_f32(temperature));
        }
        if let Some(stop) = &self.config.stop {
            if !stop.is_empty() {
                payload["stop_sequences"] = json!(stop);
            }
        }
        if let Some(tool_defs) = tools {
            let anthropic_tools = tool_defs
                .iter()
                .filter_map(openai_tool_definition_to_anthropic_tool)
                .collect::<Vec<_>>();
            if !anthropic_tools.is_empty() {
                payload["tools"] = Value::Array(anthropic_tools);
                payload["tool_choice"] = json!({ "type": "auto" });
            }
        }
        payload
    }

    fn build_chat_payload(
        &self,
        messages: &[ChatMessage],
        stream: bool,
        include_usage: bool,
        tools: Option<&[Value]>,
    ) -> Value {
        let messages = sanitize_chat_messages(messages);
        let openai_top_level_schema_guard =
            should_strip_openai_tool_schema(self.config.provider.as_deref());
        let temperature = round_f32(self.config.temperature.unwrap_or(0.7));
        let mut payload = json!({
            "model": self.config.model.clone().unwrap_or_else(|| "gpt-4".to_string()),
            "messages": messages,
            "temperature": temperature,
            "stream": stream,
        });
        if let Some(reasoning_effort) =
            normalize_reasoning_effort(self.config.reasoning_effort.as_deref())
        {
            payload["reasoning_effort"] = Value::String(reasoning_effort);
        }
        apply_disable_thinking_controls(&mut payload, &self.config);
        if stream && include_usage {
            payload["stream_options"] = json!({ "include_usage": true });
        }
        payload["max_tokens"] = json!(resolved_max_output(&self.config));
        if let Some(thinking_token_budget) = resolved_thinking_token_budget(&self.config) {
            payload["thinking_token_budget"] = json!(thinking_token_budget);
            payload["thinking_budget_tokens"] = json!(thinking_token_budget);
        }
        if let Some(stop) = &self.config.stop {
            if !stop.is_empty() {
                payload["stop"] = json!(stop);
            }
        }
        if let Some(tool_defs) = tools {
            if !tool_defs.is_empty() {
                payload["tools"] = Value::Array(
                    tool_defs
                        .iter()
                        .map(|tool| {
                            normalize_chat_tool_definition(tool, openai_top_level_schema_guard)
                        })
                        .collect(),
                );
                payload["tool_choice"] = json!("auto");
            }
        }
        payload
    }

    fn build_responses_payload(
        &self,
        messages: &[ChatMessage],
        stream: bool,
        include_usage: bool,
        tools: Option<&[Value]>,
    ) -> Value {
        let openai_top_level_schema_guard =
            should_strip_openai_tool_schema(self.config.provider.as_deref());
        let temperature = round_f32(self.config.temperature.unwrap_or(0.7));
        let input = build_responses_input(messages);
        let mut payload = json!({
            "model": self.config.model.clone().unwrap_or_else(|| "gpt-4".to_string()),
            "input": input,
            "temperature": temperature,
            "stream": stream,
        });
        if let Some(reasoning_effort) =
            normalize_reasoning_effort(self.config.reasoning_effort.as_deref())
        {
            payload["reasoning"] = json!({ "effort": reasoning_effort });
        }
        apply_disable_thinking_controls(&mut payload, &self.config);
        if stream && include_usage {
            payload["stream_options"] = json!({ "include_usage": true });
        }
        payload["max_output_tokens"] = json!(resolved_max_output(&self.config));
        if let Some(thinking_token_budget) = resolved_thinking_token_budget(&self.config) {
            payload["thinking_token_budget"] = json!(thinking_token_budget);
            payload["thinking_budget_tokens"] = json!(thinking_token_budget);
        }
        if let Some(stop) = &self.config.stop {
            if !stop.is_empty() {
                payload["stop"] = json!(stop);
            }
        }
        if let Some(tool_defs) = tools {
            if !tool_defs.is_empty() {
                payload["tools"] = Value::Array(
                    tool_defs
                        .iter()
                        .map(|tool| {
                            normalize_responses_tool_definition(tool, openai_top_level_schema_guard)
                        })
                        .collect(),
                );
                payload["tool_choice"] = json!("auto");
            }
        }
        payload
    }
}

fn should_fallback_stream_failure_to_non_stream(status: u16, body_text: &str) -> bool {
    if matches!(status, 408 | 409 | 425 | 429 | 500 | 502 | 503 | 504) {
        return true;
    }
    let normalized = body_text.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }
    [
        "rate limit",
        "too many requests",
        "system is busy",
        "engineinternalerror",
        "server is busy",
        "service unavailable",
        "gateway timeout",
        "upstream",
        "tpm",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn normalize_chat_tool_definition(tool: &Value, openai_top_level_schema_guard: bool) -> Value {
    let normalize_parameters = |schema: Option<&Value>| {
        if openai_top_level_schema_guard {
            normalize_tool_input_schema_for_openai(schema)
        } else {
            normalize_tool_input_schema(schema)
        }
    };
    if let Some(function) = tool.get("function").and_then(Value::as_object) {
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if name.is_empty() {
            return tool.clone();
        }
        let description = function
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("");
        let parameters = normalize_parameters(function.get("parameters"));
        return json!({
            "type": "function",
            "function": {
                "name": name,
                "description": description,
                "parameters": parameters
            }
        });
    }

    let name = tool
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if name.is_empty() {
        return tool.clone();
    }
    let description = tool
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    let parameters = normalize_parameters(tool.get("parameters"));
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters
        }
    })
}

fn normalize_responses_tool_definition(tool: &Value, openai_top_level_schema_guard: bool) -> Value {
    let normalize_parameters = |schema: Option<&Value>| {
        if openai_top_level_schema_guard {
            normalize_tool_input_schema_for_openai(schema)
        } else {
            normalize_tool_input_schema(schema)
        }
    };
    let tool_type = tool
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if tool_type.eq_ignore_ascii_case("custom") {
        let name = tool
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if name.is_empty() {
            return tool.clone();
        }
        let description = tool
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("");
        let format = tool.get("format").cloned().unwrap_or(Value::Null);
        return json!({
            "type": "custom",
            "name": name,
            "description": description,
            "format": format,
        });
    }
    if let Some(function) = tool.get("function").and_then(Value::as_object) {
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if name.is_empty() {
            return tool.clone();
        }
        let description = function
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("");
        let parameters = normalize_parameters(function.get("parameters"));
        return json!({
            "type": "function",
            "name": name,
            "description": description,
            "parameters": parameters
        });
    }

    let name = tool
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if name.is_empty() {
        return tool.clone();
    }
    let description = tool
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    let parameters = normalize_parameters(tool.get("parameters"));
    json!({
        "type": "function",
        "name": name,
        "description": description,
        "parameters": parameters
    })
}

fn build_responses_input(messages: &[ChatMessage]) -> Value {
    let mut input: Vec<Value> = Vec::new();
    let mut custom_tool_call_ids = HashSet::new();
    for message in messages {
        let role = message.role.trim();
        if role.eq_ignore_ascii_case("tool") {
            if let Some(call_id) = message
                .tool_call_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                let output = extract_content_text(&message.content);
                let output_type = if custom_tool_call_ids.contains(call_id) {
                    "custom_tool_call_output"
                } else {
                    "function_call_output"
                };
                input.push(json!({
                    "type": output_type,
                    "call_id": call_id,
                    "output": output,
                }));
                continue;
            }
        }

        let normalized_role = normalize_responses_role(role);
        let content = convert_responses_content(&message.content);
        input.push(json!({
            "role": normalized_role,
            "content": content,
        }));

        if let Some(tool_calls) = message.tool_calls.as_ref() {
            let calls = extract_tool_calls_list(tool_calls);
            for (idx, call) in calls.iter().enumerate() {
                if let Some(item) = tool_call_to_responses_item(call, idx) {
                    if item["type"] == "custom_tool_call" {
                        if let Some(call_id) = item.get("call_id").and_then(Value::as_str) {
                            custom_tool_call_ids.insert(call_id.to_string());
                        }
                    }
                    input.push(item);
                }
            }
        }
    }
    Value::Array(input)
}

fn normalize_responses_role(role: &str) -> &'static str {
    match role.trim().to_ascii_lowercase().as_str() {
        "system" => "system",
        "assistant" => "assistant",
        "developer" => "developer",
        _ => "user",
    }
}

fn extract_content_text(content: &Value) -> String {
    let text = extract_stream_text(Some(content));
    if !text.is_empty() {
        return text;
    }
    content
        .as_str()
        .map(|value| value.to_string())
        .unwrap_or_else(|| content.to_string())
}

fn convert_responses_content(content: &Value) -> Value {
    match content {
        Value::Null => Value::String(String::new()),
        Value::String(_) => content.clone(),
        Value::Array(parts) => Value::Array(
            parts
                .iter()
                .filter_map(convert_responses_content_part)
                .collect(),
        ),
        Value::Object(_) => convert_responses_content_part(content)
            .map(|part| Value::Array(vec![part]))
            .unwrap_or_else(|| Value::String(content.to_string())),
        other => Value::String(other.to_string()),
    }
}

fn convert_responses_content_part(part: &Value) -> Option<Value> {
    match part {
        Value::String(text) => Some(json!({ "type": "input_text", "text": text })),
        Value::Object(map) => {
            let raw_type = map
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            if matches!(
                raw_type.as_str(),
                "input_text" | "input_image" | "input_file"
            ) {
                return Some(part.clone());
            }
            if matches!(raw_type.as_str(), "text" | "output_text") {
                if let Some(text) = map.get("text").and_then(Value::as_str) {
                    return Some(json!({ "type": "input_text", "text": text }));
                }
            }
            if raw_type == "image_url" || map.contains_key("image_url") || raw_type == "input_image"
            {
                let url = map
                    .get("image_url")
                    .and_then(|value| match value {
                        Value::String(text) => Some(text.to_string()),
                        Value::Object(obj) => obj
                            .get("url")
                            .and_then(Value::as_str)
                            .map(|text| text.to_string()),
                        _ => None,
                    })
                    .or_else(|| {
                        map.get("url")
                            .and_then(Value::as_str)
                            .map(|v| v.to_string())
                    });
                if let Some(url) = url {
                    let mut item = json!({ "type": "input_image", "image_url": url });
                    if let Some(detail) = map.get("detail") {
                        item["detail"] = detail.clone();
                    } else {
                        item["detail"] = json!("auto");
                    }
                    return Some(item);
                }
            }
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                return Some(json!({ "type": "input_text", "text": text }));
            }
            None
        }
        _ => None,
    }
}

fn extract_tool_calls_list(value: &Value) -> Vec<Value> {
    match value {
        Value::Array(items) => items.clone(),
        Value::Object(_) => vec![value.clone()],
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .map(|parsed| extract_tool_calls_list(&parsed))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn normalize_tool_arguments_json(arguments: &str) -> String {
    normalize_tool_arguments_json_lossy(arguments)
}

fn tool_call_to_responses_item(call: &Value, fallback_index: usize) -> Option<Value> {
    let Value::Object(map) = call else {
        return None;
    };
    let name = map
        .get("function")
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .or_else(|| map.get("name").and_then(Value::as_str))
        .unwrap_or("")
        .trim()
        .to_string();
    if name.is_empty() {
        return None;
    }
    let arguments = map
        .get("function")
        .and_then(|value| value.get("arguments"))
        .and_then(Value::as_str)
        .or_else(|| map.get("arguments").and_then(Value::as_str))
        .map(normalize_tool_arguments_json)
        .unwrap_or_else(|| "{}".to_string());
    let call_id = map
        .get("id")
        .or_else(|| map.get("call_id"))
        .or_else(|| map.get("tool_call_id"))
        .or_else(|| map.get("toolCallId"))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("call_{}", fallback_index + 1));
    if is_freeform_tool_name(&name) {
        let input = extract_freeform_tool_input(&arguments).unwrap_or(arguments);
        return Some(json!({
            "type": "custom_tool_call",
            "call_id": call_id,
            "name": name,
            "input": input,
        }));
    }
    Some(json!({
        "type": "function_call",
        "call_id": call_id,
        "name": name,
        "arguments": arguments,
    }))
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
    let endpoint = build_openai_resource_endpoint(&base_url, EMBEDDINGS_RESOURCE)
        .ok_or_else(|| anyhow!("embedding base_url is required"))?;
    let timeout = Duration::from_secs(timeout_s.max(5));
    let client = Client::builder().timeout(timeout).build()?;
    let headers = build_headers(config.api_key.as_deref().unwrap_or(""));
    let mut include_encoding_format = true;
    let (status, body_text, body) = loop {
        let payload = if include_encoding_format {
            json!({
                "model": model,
                "input": inputs,
                "encoding_format": "float",
            })
        } else {
            json!({
                "model": model,
                "input": inputs,
            })
        };
        let response = client
            .post(&endpoint)
            .headers(headers.clone())
            .json(&payload)
            .send()
            .await?;
        let status = response.status();
        let body_text = response
            .text()
            .await
            .context("read embedding response body")?;
        let body = parse_embedding_response_json(&body_text);
        if status.is_success() {
            break (status, body_text, body);
        }
        if include_encoding_format
            && should_retry_embedding_without_encoding_format(status, &body, &body_text)
        {
            warn!("Embedding endpoint rejected encoding_format; retrying without this field");
            include_encoding_format = false;
            continue;
        }
        break (status, body_text, body);
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

fn parse_embedding_response_json(body_text: &str) -> Value {
    match serde_json::from_str::<Value>(body_text) {
        Ok(value) => value,
        Err(err) => {
            warn!(
                "Embedding response json parse failed: {err}, body={}",
                truncate_text(body_text, 2048)
            );
            Value::Null
        }
    }
}

fn should_retry_embedding_without_encoding_format(
    status: reqwest::StatusCode,
    body: &Value,
    body_text: &str,
) -> bool {
    if status != reqwest::StatusCode::BAD_REQUEST {
        return false;
    }
    let mut combined = body_text.to_ascii_lowercase();
    if body != &Value::Null {
        combined.push(' ');
        combined.push_str(&body.to_string().to_ascii_lowercase());
    }
    combined.contains("encoding_format")
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
        "anthropic" => "anthropic".to_string(),
        "claude" => "anthropic".to_string(),
        "anthropic_api" => "anthropic".to_string(),
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

fn should_strip_openai_tool_schema(provider: Option<&str>) -> bool {
    normalize_provider(provider) == "openai"
}

pub fn provider_default_base_url(provider: &str) -> Option<&'static str> {
    match provider {
        "openai" => Some(DEFAULT_OPENAI_BASE_URL),
        "anthropic" => Some(DEFAULT_ANTHROPIC_BASE_URL),
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
    if normalized == "anthropic" {
        return false;
    }
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

pub fn resolve_openai_api_mode(config: &LlmModelConfig) -> OpenAiApiMode {
    if let Some(value) = config.api_mode.as_deref() {
        return normalize_openai_api_mode(Some(value));
    }
    if let Some(base_url) = config.base_url.as_deref() {
        if base_url
            .to_ascii_lowercase()
            .trim_end_matches('/')
            .contains("/responses")
        {
            return OpenAiApiMode::Responses;
        }
    }
    if should_default_to_responses_api(config) {
        return OpenAiApiMode::Responses;
    }
    OpenAiApiMode::ChatCompletions
}

fn should_default_to_responses_api(config: &LlmModelConfig) -> bool {
    let provider = normalize_provider(config.provider.as_deref());
    if provider != "openai" {
        return false;
    }
    let Some(model) = config.model.as_deref().map(str::trim) else {
        return false;
    };
    if model.is_empty() {
        return false;
    }
    let lowered = model.to_ascii_lowercase();
    lowered.starts_with("gpt-5")
        || lowered.starts_with("o1")
        || lowered.starts_with("o3")
        || lowered.starts_with("o4")
}

fn build_openai_resource_endpoint(base_url: &str, resource: &str) -> Option<String> {
    let normalized_base = normalize_base_url(base_url)?;
    let trimmed = resource.trim_matches('/');
    if trimmed.is_empty() {
        return Some(normalized_base);
    }
    Some(format!("{normalized_base}/{trimmed}"))
}

fn build_anthropic_messages_endpoint(base_url: &str) -> Option<String> {
    let normalized_base = normalize_anthropic_base_url(base_url)?;
    Some(format!("{normalized_base}/{MESSAGES_RESOURCE}"))
}

fn parse_url_without_query_fragment(value: &str) -> Option<Url> {
    let mut parsed = Url::parse(value).ok()?;
    parsed.set_query(None);
    parsed.set_fragment(None);
    Some(parsed)
}

fn parse_or_clean_base_url(base_url: &str) -> Option<(Option<Url>, String)> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(parsed) = parse_url_without_query_fragment(trimmed) {
        return Some((Some(parsed), String::new()));
    }
    let cleaned = trimmed
        .split(['?', '#'])
        .next()
        .unwrap_or(trimmed)
        .trim_end_matches('/')
        .to_string();
    if cleaned.is_empty() {
        None
    } else {
        Some((None, cleaned))
    }
}

fn collect_path_segments(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn strip_openai_resource_suffix(segments: &mut Vec<String>) {
    for suffix in OPENAI_COMPAT_RESOURCE_SUFFIXES {
        if segments.len() < suffix.len() {
            continue;
        }
        let start = segments.len() - suffix.len();
        if segments[start..]
            .iter()
            .map(String::as_str)
            .eq(suffix.iter().copied())
        {
            segments.truncate(start);
            break;
        }
    }
}

fn is_version_segment(segment: &str) -> bool {
    let Some(rest) = segment
        .strip_prefix('v')
        .or_else(|| segment.strip_prefix('V'))
    else {
        return false;
    };
    !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit())
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
    let parse_reasoning_tokens = |value: Option<&Value>| -> Option<u64> {
        let Value::Object(details) = value? else {
            return None;
        };
        to_u64(details.get("reasoning_tokens"))
            .or_else(|| to_u64(details.get("reasoningTokens")))
            .or_else(|| {
                details
                    .get("reasoning")
                    .and_then(Value::as_object)
                    .and_then(|reasoning| {
                        to_u64(reasoning.get("tokens"))
                            .or_else(|| to_u64(reasoning.get("token_count")))
                    })
            })
    };
    let input = to_u64(map.get("input_tokens"))
        .or_else(|| to_u64(map.get("prompt_tokens")))
        .unwrap_or(0);
    let raw_output = to_u64(map.get("output_tokens"))
        .or_else(|| to_u64(map.get("completion_tokens")))
        .unwrap_or(0);
    let reasoning_tokens = to_u64(map.get("reasoning_tokens"))
        .or_else(|| to_u64(map.get("reasoningTokens")))
        .or_else(|| parse_reasoning_tokens(map.get("output_tokens_details")))
        .or_else(|| parse_reasoning_tokens(map.get("outputTokensDetails")))
        .or_else(|| parse_reasoning_tokens(map.get("completion_tokens_details")))
        .or_else(|| parse_reasoning_tokens(map.get("completionTokensDetails")))
        .unwrap_or(0);
    let output = raw_output.saturating_sub(reasoning_tokens);
    let total = to_u64(map.get("total_tokens")).unwrap_or(input.saturating_add(raw_output));
    if input == 0 && output == 0 && total == 0 {
        return None;
    }
    Some(TokenUsage {
        input,
        output,
        total,
    })
}

fn build_anthropic_messages(messages: &[ChatMessage]) -> (Option<String>, Vec<Value>) {
    let mut system_parts = Vec::new();
    let mut output = Vec::new();

    for message in messages {
        let role = message.role.trim().to_ascii_lowercase();
        match role.as_str() {
            "system" => {
                let text = flatten_message_text(&message.content);
                if !text.trim().is_empty() {
                    system_parts.push(text);
                }
            }
            "assistant" => {
                let mut blocks = Vec::new();
                let text = flatten_message_text(&message.content);
                if !text.trim().is_empty() {
                    blocks.push(json!({
                        "type": "text",
                        "text": text,
                    }));
                }
                if let Some(tool_payload) = message.tool_calls.as_ref() {
                    for (index, call) in extract_openai_tool_calls(tool_payload).iter().enumerate()
                    {
                        if let Some(tool_use) =
                            openai_tool_call_to_anthropic_tool_use_block(call, index)
                        {
                            blocks.push(tool_use);
                        }
                    }
                }
                append_anthropic_message(&mut output, "assistant", blocks);
            }
            "tool" => {
                let tool_use_id = message
                    .tool_call_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let text = flatten_message_text(&message.content);
                let blocks = if let Some(tool_use_id) = tool_use_id {
                    vec![json!({
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": text,
                    })]
                } else if text.trim().is_empty() {
                    Vec::new()
                } else {
                    vec![json!({
                        "type": "text",
                        "text": text,
                    })]
                };
                append_anthropic_message(&mut output, "user", blocks);
            }
            _ => {
                let text = flatten_message_text(&message.content);
                if text.trim().is_empty() {
                    continue;
                }
                append_anthropic_message(
                    &mut output,
                    "user",
                    vec![json!({
                        "type": "text",
                        "text": text,
                    })],
                );
            }
        }
    }

    if output.is_empty() {
        output.push(json!({
            "role": "user",
            "content": [{
                "type": "text",
                "text": "",
            }],
        }));
    }

    let system = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n\n"))
    };
    (system, output)
}

fn append_anthropic_message(messages: &mut Vec<Value>, role: &str, blocks: Vec<Value>) {
    if blocks.is_empty() {
        return;
    }
    if let Some(last) = messages.last_mut() {
        let same_role = last
            .get("role")
            .and_then(Value::as_str)
            .map(|value| value == role)
            .unwrap_or(false);
        if same_role {
            if let Some(existing) = last.get_mut("content").and_then(Value::as_array_mut) {
                existing.extend(blocks);
                return;
            }
        }
    }
    messages.push(json!({
        "role": role,
        "content": blocks,
    }));
}

fn flatten_message_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.to_string(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                if let Some(text) = item.as_str() {
                    return Some(text.to_string());
                }
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    return Some(text.to_string());
                }
                if let Some(text) = item.get("content").and_then(Value::as_str) {
                    return Some(text.to_string());
                }
                None
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                return text.to_string();
            }
            if let Some(content) = map.get("content") {
                return flatten_message_text(content);
            }
            String::new()
        }
        _ => String::new(),
    }
}

fn extract_openai_tool_calls(value: &Value) -> Vec<Value> {
    match value {
        Value::Array(items) => items.clone(),
        Value::Object(map) => {
            if let Some(items) = map.get("tool_calls").and_then(Value::as_array) {
                return items.clone();
            }
            if map.get("function").is_some() {
                return vec![value.clone()];
            }
            if map.get("name").and_then(Value::as_str).is_some()
                && (map.get("arguments").is_some() || map.get("input").is_some())
            {
                return vec![value.clone()];
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn openai_tool_call_to_anthropic_tool_use_block(
    call: &Value,
    fallback_index: usize,
) -> Option<Value> {
    let Value::Object(map) = call else {
        return None;
    };
    let name = map
        .get("function")
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .or_else(|| map.get("name").and_then(Value::as_str))
        .unwrap_or("")
        .trim()
        .to_string();
    if name.is_empty() {
        return None;
    }
    let tool_use_id = map
        .get("id")
        .or_else(|| map.get("call_id"))
        .or_else(|| map.get("tool_call_id"))
        .or_else(|| map.get("toolCallId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("toolu_{}", fallback_index + 1));

    let args_value = map
        .get("function")
        .and_then(|value| value.get("arguments"))
        .and_then(Value::as_str)
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .or_else(|| map.get("input").cloned())
        .unwrap_or_else(|| json!({}));
    let input = if args_value.is_object() {
        args_value
    } else {
        json!({ "input": args_value })
    };

    Some(json!({
        "type": "tool_use",
        "id": tool_use_id,
        "name": name,
        "input": input,
    }))
}

fn openai_tool_definition_to_anthropic_tool(tool: &Value) -> Option<Value> {
    let Value::Object(map) = tool else {
        return None;
    };
    if let Some(function) = map.get("function").and_then(Value::as_object) {
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        let description = function
            .get("description")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let mut input_schema = normalize_tool_input_schema(function.get("parameters"));
        if !input_schema.is_object() {
            input_schema = json!({ "type": "object", "properties": {} });
        }
        let mut payload = json!({
            "name": name,
            "input_schema": input_schema,
        });
        if let Some(description) = description {
            payload["description"] = Value::String(description);
        }
        return Some(payload);
    }

    if map.get("name").and_then(Value::as_str).is_some() && map.get("input_schema").is_some() {
        return Some(tool.clone());
    }
    None
}

fn anthropic_tool_use_block_to_openai(block: &Value, fallback_index: usize) -> Option<Value> {
    let Value::Object(map) = block else {
        return None;
    };
    let name = map
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let id = map
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("call_{}", fallback_index + 1));
    let arguments = map
        .get("input")
        .and_then(|value| serde_json::to_string(value).ok())
        .unwrap_or_else(|| "{}".to_string());
    Some(json!({
        "type": "function",
        "id": id,
        "function": {
            "name": name,
            "arguments": normalize_tool_arguments_json(&arguments),
        }
    }))
}

fn parse_anthropic_body(body: &Value) -> (String, String, Option<Value>) {
    let mut content = String::new();
    let mut reasoning = String::new();
    let mut tool_calls = Vec::new();

    if let Some(blocks) = body.get("content").and_then(Value::as_array) {
        for (index, block) in blocks.iter().enumerate() {
            let block_type = block
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            match block_type.as_str() {
                "text" => {
                    let text = block.get("text").and_then(Value::as_str).unwrap_or("");
                    if !text.is_empty() {
                        content.push_str(text);
                    }
                }
                "thinking" => {
                    let text = block
                        .get("thinking")
                        .or_else(|| block.get("text"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if !text.is_empty() {
                        if !reasoning.is_empty() {
                            reasoning.push('\n');
                        }
                        reasoning.push_str(text);
                    }
                }
                "tool_use" => {
                    if let Some(tool_call) = anthropic_tool_use_block_to_openai(block, index) {
                        tool_calls.push(tool_call);
                    }
                }
                _ => {}
            }
        }
    }

    if content.trim().is_empty() {
        if let Some(text) = body.get("completion").and_then(Value::as_str) {
            content = text.to_string();
        }
    }

    let tool_calls = if tool_calls.is_empty() {
        None
    } else {
        Some(Value::Array(tool_calls))
    };
    (content, reasoning, tool_calls)
}

fn normalize_anthropic_base_url(base_url: &str) -> Option<String> {
    let (parsed, cleaned_fallback) = parse_or_clean_base_url(base_url)?;
    if let Some(mut parsed) = parsed {
        let mut segments = collect_path_segments(parsed.path());
        if segments
            .last()
            .is_some_and(|segment| segment.eq_ignore_ascii_case(MESSAGES_RESOURCE))
        {
            segments.pop();
        }
        if !segments
            .last()
            .is_some_and(|segment| is_version_segment(segment))
        {
            segments.push("v1".to_string());
        }
        parsed.set_path(&format!("/{}", segments.join("/")));
        return Some(parsed.to_string().trim_end_matches('/').to_string());
    }

    let mut base = cleaned_fallback.trim_end_matches('/').to_string();
    if let Some(stripped) = base.strip_suffix("/messages") {
        base = stripped.trim_end_matches('/').to_string();
    }
    if base.is_empty() {
        return None;
    }
    if base
        .rsplit('/')
        .next()
        .is_none_or(|segment| !is_version_segment(segment))
    {
        base = format!("{base}/v1");
    }
    Some(base)
}

fn parse_chat_completion_body(body: &Value) -> (String, String, Option<Value>) {
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
    (content, reasoning, tool_calls)
}

fn parse_responses_body(body: &Value) -> (String, String, Option<Value>) {
    let response = body.get("response").unwrap_or(body);
    let (content, reasoning, tool_calls) = extract_responses_output(response);
    let tool_calls = if tool_calls.is_empty() {
        None
    } else {
        Some(Value::Array(tool_calls))
    };
    (content, reasoning, tool_calls)
}

fn extract_responses_output(response: &Value) -> (String, String, Vec<Value>) {
    let mut content = String::new();
    let mut reasoning = String::new();
    let mut tool_calls: Vec<Value> = Vec::new();

    let output_items = response
        .get("output")
        .and_then(Value::as_array)
        .or_else(|| response.as_array());
    if let Some(items) = output_items {
        for (idx, item) in items.iter().enumerate() {
            let item_type = item
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            match item_type.as_str() {
                "message" => {
                    let text = extract_response_message_text(item);
                    if !text.is_empty() {
                        content.push_str(&text);
                    }
                }
                "reasoning" => {
                    let text = extract_response_reasoning(item);
                    if !text.is_empty() {
                        if !reasoning.is_empty() {
                            reasoning.push('\n');
                        }
                        reasoning.push_str(&text);
                    }
                }
                "function_call" => {
                    if let Some(tool_call) = response_tool_call_to_openai(item, idx) {
                        tool_calls.push(tool_call);
                    }
                }
                "custom_tool_call" => {
                    if let Some(tool_call) = response_tool_call_to_openai(item, idx) {
                        tool_calls.push(tool_call);
                    }
                }
                _ => {}
            }
        }
    }

    if content.trim().is_empty() {
        if let Some(text) = response.get("output_text").and_then(Value::as_str) {
            content = text.to_string();
        } else if let Some(text) = response.get("text").and_then(Value::as_str) {
            content = text.to_string();
        }
    }

    (content, reasoning, tool_calls)
}

fn extract_response_message_text(item: &Value) -> String {
    if let Some(content) = item.get("content") {
        let text = extract_stream_text(Some(content));
        if !text.is_empty() {
            return text;
        }
    }
    item.get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn extract_response_reasoning(item: &Value) -> String {
    if let Some(summary) = item.get("summary") {
        let text = extract_stream_text(Some(summary));
        if !text.is_empty() {
            return text;
        }
    }
    item.get("text")
        .or_else(|| item.get("summary_text"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn response_tool_call_to_openai(item: &Value, fallback_index: usize) -> Option<Value> {
    let Value::Object(map) = item else {
        return None;
    };
    let name = map
        .get("name")
        .and_then(Value::as_str)
        .or_else(|| {
            map.get("function")
                .and_then(|value| value.get("name"))
                .and_then(Value::as_str)
        })
        .unwrap_or("")
        .trim()
        .to_string();
    if name.is_empty() {
        return None;
    }
    let arguments = map
        .get("arguments")
        .and_then(Value::as_str)
        .or_else(|| {
            map.get("function")
                .and_then(|value| value.get("arguments"))
                .and_then(Value::as_str)
        })
        .map(normalize_tool_arguments_json)
        .or_else(|| {
            map.get("input")
                .and_then(Value::as_str)
                .and_then(|input| serde_json::to_string(&json!({ "input": input })).ok())
        })
        .unwrap_or_default();
    let call_id = map
        .get("call_id")
        .or_else(|| map.get("id"))
        .or_else(|| map.get("tool_call_id"))
        .or_else(|| map.get("toolCallId"))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("call_{}", fallback_index + 1));
    Some(json!({
        "type": "function",
        "id": call_id,
        "function": {
            "name": name,
            "arguments": arguments,
        }
    }))
}

fn extract_tool_calls(message: &Value) -> Option<Value> {
    let Value::Object(map) = message else {
        return None;
    };
    let payload = map
        .get("tool_calls")
        .or_else(|| map.get("tool_call"))
        .or_else(|| map.get("function_call"))
        .or_else(|| map.get("functionCall"))
        .map(sanitize_tool_call_payload)?;
    match &payload {
        Value::Array(items) if items.is_empty() => None,
        _ => Some(payload),
    }
}

fn has_stream_tool_activity(payload: Option<&Value>) -> bool {
    payload.is_some_and(|value| extract_tool_calls(value).is_some())
}

fn is_false_tool_stop_reason(value: Option<&Value>) -> bool {
    let Some(raw) = value.and_then(Value::as_str).map(str::trim) else {
        return false;
    };
    matches!(
        raw.to_ascii_lowercase().as_str(),
        "tool_calls" | "tool_call" | "function_call" | "tooluse" | "tool_use"
    )
}

fn sanitize_chat_messages(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    messages
        .iter()
        .map(|message| ChatMessage {
            role: message.role.clone(),
            content: message.content.clone(),
            reasoning_content: message.reasoning_content.clone(),
            tool_calls: message.tool_calls.as_ref().map(sanitize_tool_call_payload),
            tool_call_id: message.tool_call_id.clone(),
        })
        .collect()
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
        let fallback = block.trim();
        if fallback.starts_with('{') || fallback.starts_with('[') {
            return process_stream_payload(
                fallback,
                combined,
                reasoning_combined,
                usage,
                tool_calls_accumulator,
                on_delta,
            )
            .await;
        }
        return Ok(false);
    }

    if data_lines.len() > 1 {
        let mut is_line_delimited_payload = true;
        for data in &data_lines {
            let payload = data.trim();
            if payload.is_empty() || payload == "[DONE]" {
                continue;
            }
            if serde_json::from_str::<Value>(payload).is_err() {
                is_line_delimited_payload = false;
                break;
            }
        }
        if is_line_delimited_payload {
            for data in data_lines {
                if process_stream_payload(
                    data.trim(),
                    combined,
                    reasoning_combined,
                    usage,
                    tool_calls_accumulator,
                    on_delta,
                )
                .await?
                {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
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
            if let Some(error_message) = extract_stream_error_message(&payload) {
                return Err(anyhow!(error_message));
            }
            if is_anthropic_stream_payload(&payload) {
                return process_anthropic_stream_payload(
                    &payload,
                    combined,
                    reasoning_combined,
                    usage,
                    tool_calls_accumulator,
                    on_delta,
                )
                .await;
            }
            if is_responses_stream_payload(&payload) {
                return process_responses_stream_payload(
                    &payload,
                    combined,
                    reasoning_combined,
                    usage,
                    tool_calls_accumulator,
                    on_delta,
                )
                .await;
            }
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
            let tool_activity = has_stream_tool_activity(Some(&delta))
                || has_stream_tool_activity(choice.and_then(|value| value.get("message")))
                || has_stream_tool_activity(choice)
                || has_stream_tool_activity(Some(&payload));
            let false_tool_stop =
                is_false_tool_stop_reason(choice.and_then(|value| value.get("finish_reason")))
                    || is_false_tool_stop_reason(payload.get("finish_reason"))
                    || is_false_tool_stop_reason(payload.get("stop_reason"))
                    || is_false_tool_stop_reason(payload.get("stopReason"));
            update_stream_tool_calls(tool_calls_accumulator, &delta);
            if let Some(message) = choice.and_then(|value| value.get("message")) {
                update_stream_tool_calls(tool_calls_accumulator, message);
            }
            if let Some(choice_payload) = choice {
                update_stream_tool_calls(tool_calls_accumulator, choice_payload);
            }
            if let Some(payload_tool_calls) = payload.get("tool_calls") {
                update_stream_tool_calls(tool_calls_accumulator, payload_tool_calls);
            }
            if let Some(payload_function_call) = payload.get("function_call") {
                update_stream_tool_calls(tool_calls_accumulator, payload_function_call);
            }
            if !content_delta.is_empty() {
                combined.push_str(content_delta.as_str());
            }
            if !reasoning_delta.is_empty() {
                reasoning_combined.push_str(reasoning_delta.as_str());
            }
            if !content_delta.is_empty() || !reasoning_delta.is_empty() {
                on_delta(content_delta, reasoning_delta).await?;
            } else if tool_activity && !false_tool_stop {
                on_delta(String::new(), String::new()).await?;
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

fn is_responses_stream_payload(payload: &Value) -> bool {
    payload.get("type").is_some()
        || payload.get("output").is_some()
        || payload.get("response").is_some()
}

fn is_anthropic_stream_payload(payload: &Value) -> bool {
    let payload_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        payload_type.as_str(),
        "message"
            | "message_start"
            | "message_delta"
            | "message_stop"
            | "content_block_start"
            | "content_block_delta"
            | "content_block_stop"
            | "ping"
            | "error"
    )
}

async fn process_anthropic_stream_payload<F, Fut>(
    payload: &Value,
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
    let payload_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    match payload_type.as_str() {
        "message_start" => {
            if let Some(new_usage) =
                normalize_usage(payload.get("message").and_then(|value| value.get("usage")))
            {
                *usage = Some(new_usage);
            }
        }
        "message_delta" => {
            if let Some(new_usage) = normalize_usage(payload.get("usage")) {
                *usage = Some(new_usage);
            }
        }
        "content_block_start" => {
            let index = payload.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            if let Some(block) = payload.get("content_block") {
                let block_type = block
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_ascii_lowercase();
                match block_type.as_str() {
                    "text" => {
                        let delta = block.get("text").and_then(Value::as_str).unwrap_or("");
                        if !delta.is_empty() {
                            combined.push_str(delta);
                            on_delta(delta.to_string(), String::new()).await?;
                        }
                    }
                    "thinking" => {
                        let delta = block
                            .get("thinking")
                            .or_else(|| block.get("text"))
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        if !delta.is_empty() {
                            reasoning_combined.push_str(delta);
                            on_delta(String::new(), delta.to_string()).await?;
                        }
                    }
                    "tool_use" => {
                        let call = json!({
                            "index": index,
                            "id": block.get("id").cloned().unwrap_or(Value::Null),
                            "function": {
                                "name": block.get("name").cloned().unwrap_or(Value::Null),
                                "arguments": block
                                    .get("input")
                                    .and_then(|value| serde_json::to_string(value).ok())
                                    .unwrap_or_else(|| "{}".to_string()),
                            }
                        });
                        merge_stream_tool_call_item(tool_calls_accumulator, &call);
                        on_delta(String::new(), String::new()).await?;
                    }
                    _ => {}
                }
            }
        }
        "content_block_delta" => {
            let index = payload.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            if let Some(delta) = payload.get("delta") {
                let delta_type = delta
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_ascii_lowercase();
                match delta_type.as_str() {
                    "text_delta" => {
                        let text = delta.get("text").and_then(Value::as_str).unwrap_or("");
                        if !text.is_empty() {
                            combined.push_str(text);
                            on_delta(text.to_string(), String::new()).await?;
                        }
                    }
                    "thinking_delta" => {
                        let text = delta
                            .get("thinking")
                            .or_else(|| delta.get("text"))
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        if !text.is_empty() {
                            reasoning_combined.push_str(text);
                            on_delta(String::new(), text.to_string()).await?;
                        }
                    }
                    "input_json_delta" => {
                        let partial = delta
                            .get("partial_json")
                            .or_else(|| delta.get("text"))
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        if !partial.is_empty() {
                            let call = json!({
                                "index": index,
                                "function": {
                                    "arguments": partial,
                                }
                            });
                            merge_stream_tool_call_item(tool_calls_accumulator, &call);
                            on_delta(String::new(), String::new()).await?;
                        }
                    }
                    _ => {}
                }
            }
        }
        "message" => {
            if let Some(new_usage) = normalize_usage(payload.get("usage")) {
                *usage = Some(new_usage);
            }
            let (text, reasoning, tool_calls) = parse_anthropic_body(payload);
            if combined.is_empty() && !text.is_empty() {
                combined.push_str(&text);
                on_delta(text, String::new()).await?;
            }
            if reasoning_combined.is_empty() && !reasoning.is_empty() {
                reasoning_combined.push_str(&reasoning);
                on_delta(String::new(), reasoning).await?;
            }
            if let Some(Value::Array(items)) = tool_calls {
                upsert_responses_tool_calls(tool_calls_accumulator, &items);
                if combined.is_empty() && reasoning_combined.is_empty() {
                    on_delta(String::new(), String::new()).await?;
                }
            }
            return Ok(true);
        }
        "message_stop" => {
            return Ok(true);
        }
        _ => {}
    }
    Ok(false)
}

async fn process_responses_stream_payload<F, Fut>(
    payload: &Value,
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
    if let Some(new_usage) = normalize_usage(payload.get("usage")) {
        *usage = Some(new_usage);
    }

    let payload_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    match payload_type.as_str() {
        "response.output_text.delta" => {
            let delta = payload.get("delta").and_then(Value::as_str).unwrap_or("");
            if !delta.is_empty() {
                combined.push_str(delta);
                on_delta(delta.to_string(), String::new()).await?;
            }
        }
        "response.output_text.done" => {
            let text = payload.get("text").and_then(Value::as_str).unwrap_or("");
            if !text.is_empty() && combined.is_empty() {
                combined.push_str(text);
                on_delta(text.to_string(), String::new()).await?;
            }
        }
        "response.reasoning_summary_part.added" => {
            if !reasoning_combined.is_empty() {
                reasoning_combined.push_str("\n\n");
            }
        }
        "response.reasoning_summary_text.delta" => {
            let delta = payload.get("delta").and_then(Value::as_str).unwrap_or("");
            if !delta.is_empty() {
                reasoning_combined.push_str(delta);
                on_delta(String::new(), delta.to_string()).await?;
            }
        }
        "response.output_item.added" => {
            if let Some(item) = payload.get("item") {
                update_responses_tool_call_from_item(tool_calls_accumulator, item);
                on_delta(String::new(), String::new()).await?;
            }
        }
        "response.function_call_arguments.delta" => {
            update_responses_tool_call_arguments(tool_calls_accumulator, payload);
            on_delta(String::new(), String::new()).await?;
        }
        "response.function_call_arguments.done" => {
            update_responses_tool_call_arguments(tool_calls_accumulator, payload);
            on_delta(String::new(), String::new()).await?;
        }
        "response.completed" => {
            if let Some(response) = payload.get("response") {
                if let Some(new_usage) = normalize_usage(response.get("usage")) {
                    *usage = Some(new_usage);
                }
                let (text, reasoning, tool_calls) = extract_responses_output(response);
                if combined.is_empty() && !text.is_empty() {
                    combined.push_str(&text);
                    on_delta(text, String::new()).await?;
                }
                if reasoning_combined.is_empty() && !reasoning.is_empty() {
                    reasoning_combined.push_str(&reasoning);
                    on_delta(String::new(), reasoning).await?;
                }
                if !tool_calls.is_empty() {
                    upsert_responses_tool_calls(tool_calls_accumulator, &tool_calls);
                    if combined.is_empty() && reasoning_combined.is_empty() {
                        on_delta(String::new(), String::new()).await?;
                    }
                }
            }
            return Ok(true);
        }
        _ => {}
    }

    if payload_type.is_empty() && payload.get("output").is_some() {
        let (text, reasoning, tool_calls) = extract_responses_output(payload);
        if combined.is_empty() && !text.is_empty() {
            combined.push_str(&text);
            on_delta(text, String::new()).await?;
        }
        if reasoning_combined.is_empty() && !reasoning.is_empty() {
            reasoning_combined.push_str(&reasoning);
            on_delta(String::new(), reasoning).await?;
        }
        if !tool_calls.is_empty() {
            upsert_responses_tool_calls(tool_calls_accumulator, &tool_calls);
            if combined.is_empty() && reasoning_combined.is_empty() {
                on_delta(String::new(), String::new()).await?;
            }
        }
        if let Some(new_usage) = normalize_usage(payload.get("usage")) {
            *usage = Some(new_usage);
        }
        return Ok(true);
    }

    Ok(false)
}

fn extract_stream_error_message(payload: &Value) -> Option<String> {
    let payload_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    // Responses API can report fatal failures mid-stream via structured SSE events rather than
    // terminating the HTTP request with a non-2xx status, so surface them as regular errors.
    let error_payload = if payload_type == "response.failed" {
        payload
            .get("response")
            .and_then(|response| response.get("error"))
            .or_else(|| payload.get("error"))
    } else {
        payload.get("error").or_else(|| {
            payload
                .get("response")
                .and_then(|response| response.get("error"))
        })
    }?;
    let prefix = if payload_type == "response.failed" {
        "LLM stream response failed"
    } else {
        "LLM stream payload failed"
    };
    Some(format_stream_error_message(prefix, error_payload))
}

fn format_stream_error_message(prefix: &str, error_payload: &Value) -> String {
    match error_payload {
        Value::String(text) => {
            let text = text.trim();
            if text.is_empty() {
                prefix.to_string()
            } else {
                format!("{prefix}: {text}")
            }
        }
        Value::Object(map) => {
            let code = map.get("code").and_then(Value::as_str).unwrap_or("").trim();
            let message = map
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if !code.is_empty() && !message.is_empty() {
                format!("{prefix}: {code}: {message}")
            } else if !message.is_empty() {
                format!("{prefix}: {message}")
            } else if !code.is_empty() {
                format!("{prefix}: {code}")
            } else {
                let raw = serde_json::to_string(error_payload).unwrap_or_default();
                if raw.is_empty() {
                    prefix.to_string()
                } else {
                    format!("{prefix}: {}", truncate_text(&raw, 512))
                }
            }
        }
        _ => {
            let raw = serde_json::to_string(error_payload).unwrap_or_default();
            if raw.is_empty() {
                prefix.to_string()
            } else {
                format!("{prefix}: {}", truncate_text(&raw, 512))
            }
        }
    }
}

#[derive(Debug, Default, Clone)]
struct StreamToolCall {
    id: Option<String>,
    source_id: Option<String>,
    name: String,
    arguments: String,
}

fn update_stream_tool_calls(acc: &mut Vec<StreamToolCall>, payload: &Value) {
    match payload {
        Value::Array(items) => {
            if items.is_empty() {
                return;
            }
            for item in items {
                merge_stream_tool_call_item(acc, item);
            }
        }
        Value::Object(map) => {
            if let Some(tool_calls) = map.get("tool_calls").or_else(|| map.get("tool_call")) {
                update_stream_tool_calls(acc, tool_calls);
            } else if looks_like_stream_tool_call_item(map) {
                merge_stream_tool_call_item(acc, payload);
            }

            if let Some(function_call) = map.get("function_call") {
                if acc.is_empty() {
                    acc.push(StreamToolCall::default());
                }
                apply_function_delta(&mut acc[0], function_call);
            }
        }
        _ => {}
    }
}

fn update_responses_tool_call_from_item(acc: &mut Vec<StreamToolCall>, item: &Value) {
    let Value::Object(map) = item else {
        return;
    };
    let item_type = map
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    if item_type != "function_call" && item_type != "custom_tool_call" {
        return;
    }
    let item_id = map
        .get("id")
        .or_else(|| map.get("item_id"))
        .and_then(Value::as_str);
    let call_id = map
        .get("call_id")
        .or_else(|| map.get("callId"))
        .and_then(Value::as_str);
    let name = map.get("name").and_then(Value::as_str).or_else(|| {
        map.get("function")
            .and_then(|value| value.get("name"))
            .and_then(Value::as_str)
    });
    let arguments = map.get("arguments").and_then(Value::as_str).or_else(|| {
        map.get("function")
            .and_then(|value| value.get("arguments"))
            .and_then(Value::as_str)
    });
    let custom_input = map.get("input").and_then(Value::as_str);
    let arguments = if item_type == "custom_tool_call" {
        custom_input.or(arguments)
    } else {
        arguments
    };
    upsert_response_tool_call(acc, item_id, call_id, name, arguments);
}

fn update_responses_tool_call_arguments(acc: &mut Vec<StreamToolCall>, payload: &Value) {
    let item_id = payload.get("item_id").and_then(Value::as_str);
    let call_id = payload
        .get("call_id")
        .or_else(|| payload.get("callId"))
        .and_then(Value::as_str);
    let arguments = payload
        .get("delta")
        .or_else(|| payload.get("arguments"))
        .and_then(Value::as_str);
    if arguments.is_none() && call_id.is_none() && item_id.is_none() {
        return;
    }
    upsert_response_tool_call(acc, item_id, call_id, None, arguments);
}

fn upsert_responses_tool_calls(acc: &mut Vec<StreamToolCall>, tool_calls: &[Value]) {
    for call in tool_calls {
        let Value::Object(map) = call else {
            continue;
        };
        let call_id = map
            .get("id")
            .or_else(|| map.get("call_id"))
            .or_else(|| map.get("tool_call_id"))
            .or_else(|| map.get("toolCallId"))
            .and_then(Value::as_str);
        let function = map.get("function").or_else(|| map.get("function_call"));
        let name = function
            .and_then(|value| value.get("name"))
            .and_then(Value::as_str);
        let arguments = function
            .and_then(|value| value.get("arguments"))
            .and_then(Value::as_str);
        if name.is_none() && arguments.is_none() && call_id.is_none() {
            continue;
        }
        upsert_response_tool_call(acc, None, call_id, name, arguments);
    }
}

fn upsert_response_tool_call(
    acc: &mut Vec<StreamToolCall>,
    item_id: Option<&str>,
    call_id: Option<&str>,
    name: Option<&str>,
    arguments: Option<&str>,
) {
    let index = find_response_tool_call_index(acc, item_id, call_id).unwrap_or_else(|| {
        acc.push(StreamToolCall::default());
        acc.len().saturating_sub(1)
    });
    let slot = &mut acc[index];
    if let Some(item_id) = item_id {
        let trimmed = item_id.trim();
        if !trimmed.is_empty() {
            slot.source_id = Some(trimmed.to_string());
        }
    }
    if let Some(call_id) = call_id {
        let trimmed = call_id.trim();
        if !trimmed.is_empty() {
            slot.id = Some(trimmed.to_string());
        }
    }
    if let Some(name) = name {
        merge_stream_text_field(&mut slot.name, name);
    }
    if let Some(arguments) = arguments {
        merge_stream_text_field(&mut slot.arguments, arguments);
    }
}

fn find_response_tool_call_index(
    acc: &[StreamToolCall],
    item_id: Option<&str>,
    call_id: Option<&str>,
) -> Option<usize> {
    if let Some(item_id) = item_id {
        if let Some(index) = acc
            .iter()
            .position(|call| call.source_id.as_deref() == Some(item_id))
        {
            return Some(index);
        }
    }
    if let Some(call_id) = call_id {
        if let Some(index) = acc
            .iter()
            .position(|call| call.id.as_deref() == Some(call_id))
        {
            return Some(index);
        }
    }
    None
}

pub fn normalize_openai_api_mode(value: Option<&str>) -> OpenAiApiMode {
    let raw = value.unwrap_or("").trim();
    if raw.is_empty() {
        return OpenAiApiMode::ChatCompletions;
    }
    match raw
        .to_ascii_lowercase()
        .replace(['-', ' ', '/'], "_")
        .as_str()
    {
        "responses" | "response" | "response_api" | "v1_responses" => OpenAiApiMode::Responses,
        "chat" | "chat_completions" | "chatcompletion" | "chat_completions_api" => {
            OpenAiApiMode::ChatCompletions
        }
        _ => OpenAiApiMode::ChatCompletions,
    }
}

fn looks_like_stream_tool_call_item(map: &serde_json::Map<String, Value>) -> bool {
    if map.contains_key("function") {
        return true;
    }

    let has_name = map.get("name").is_some_and(Value::is_string);
    let has_arguments = map.contains_key("arguments");
    if has_name && has_arguments {
        return true;
    }

    let has_index_or_id = map.contains_key("index")
        || map.contains_key("id")
        || map.contains_key("tool_call_id")
        || map.contains_key("toolCallId")
        || map.contains_key("call_id")
        || map.contains_key("callId");
    let is_function_type = map
        .get("type")
        .and_then(Value::as_str)
        .map(|value| value.eq_ignore_ascii_case("function"))
        .unwrap_or(false);
    has_arguments && (has_index_or_id || is_function_type)
}

fn merge_stream_tool_call_item(acc: &mut Vec<StreamToolCall>, item: &Value) {
    let Value::Object(map) = item else {
        return;
    };

    let index = map.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
    while acc.len() <= index {
        acc.push(StreamToolCall::default());
    }

    let slot = &mut acc[index];
    if let Some(id) = map
        .get("id")
        .or_else(|| map.get("call_id"))
        .or_else(|| map.get("callId"))
        .or_else(|| map.get("tool_call_id"))
        .or_else(|| map.get("toolCallId"))
        .and_then(Value::as_str)
        .map(str::trim)
    {
        if !id.is_empty() {
            slot.id = Some(id.to_string());
        }
    }

    if let Some(function) = map.get("function") {
        apply_function_delta(slot, function);
    } else {
        apply_function_delta(slot, item);
    }
}

fn apply_function_delta(slot: &mut StreamToolCall, function: &Value) {
    if let Value::Object(map) = function {
        if let Some(name) = map.get("name").and_then(Value::as_str) {
            merge_stream_text_field(&mut slot.name, name);
        }
        if let Some(arguments) = map.get("arguments").and_then(Value::as_str) {
            merge_stream_text_field(&mut slot.arguments, arguments);
        }
    }
}

fn merge_stream_text_field(target: &mut String, fragment: &str) {
    if fragment.is_empty() {
        return;
    }

    if target.is_empty() {
        target.push_str(fragment);
        return;
    }

    if target.as_str() == fragment || target.ends_with(fragment) {
        return;
    }

    if fragment.starts_with(target.as_str()) {
        target.clear();
        target.push_str(fragment);
        return;
    }

    if should_replace_empty_json_stream_payload(target, fragment) {
        target.clear();
        target.push_str(fragment);
        return;
    }

    if should_replace_stream_json_payload(target, fragment) {
        target.clear();
        target.push_str(fragment);
        return;
    }

    let overlap = stream_text_overlap_len(target, fragment).filter(|overlap| *overlap > 1);
    let Some(overlap) = overlap else {
        target.push_str(fragment);
        return;
    };
    target.push_str(&fragment[overlap..]);
}

fn should_replace_empty_json_stream_payload(current: &str, next: &str) -> bool {
    let current = current.trim();
    let next = next.trim();
    if current != "{}" {
        return false;
    }
    if next.is_empty() {
        return false;
    }
    if next == "{" {
        return true;
    }
    if !next.starts_with('{') {
        return false;
    }
    strict_parse_partial_json_fragment(next)
}

fn should_replace_stream_json_payload(current: &str, next: &str) -> bool {
    let current = current.trim();
    let next = next.trim();
    if current.is_empty() || next.is_empty() {
        return false;
    }
    serde_json::from_str::<Value>(current).is_ok() && serde_json::from_str::<Value>(next).is_ok()
}

fn stream_text_overlap_len(target: &str, fragment: &str) -> Option<usize> {
    let max_len = target.len().min(fragment.len());
    for len in (1..=max_len).rev() {
        let target_start = target.len() - len;
        if !target.is_char_boundary(target_start) || !fragment.is_char_boundary(len) {
            continue;
        }
        if target[target_start..] == fragment[..len] {
            return Some(len);
        }
    }
    None
}

fn strict_parse_partial_json_fragment(fragment: &str) -> bool {
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escaped = false;

    for ch in fragment.chars() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }

    !in_string && depth == 0 && serde_json::from_str::<Value>(fragment).is_ok()
}

fn finalize_stream_tool_calls(acc: &[StreamToolCall]) -> Option<Value> {
    let mut output = Vec::new();
    for call in acc {
        if call.name.trim().is_empty() {
            continue;
        }
        let arguments = if is_freeform_tool_name(&call.name) {
            let input = extract_freeform_tool_input(&call.arguments)
                .unwrap_or_else(|| call.arguments.clone());
            serde_json::to_string(&json!({ "input": input })).unwrap_or_else(|_| "{}".to_string())
        } else {
            normalize_tool_arguments_json(&call.arguments)
        };
        let mut payload = json!({
            "type": "function",
            "function": {
                "name": call.name,
                "arguments": arguments,
            }
        });
        if let Some(id) = call.id.as_ref().or(call.source_id.as_ref()) {
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
    let (parsed, cleaned_fallback) = parse_or_clean_base_url(base_url)?;
    if let Some(mut parsed) = parsed {
        let mut segments = collect_path_segments(parsed.path());
        strip_openai_resource_suffix(&mut segments);
        if !segments
            .last()
            .is_some_and(|segment| is_version_segment(segment))
        {
            segments.push("v1".to_string());
        }
        parsed.set_path(&format!("/{}", segments.join("/")));
        return Some(parsed.to_string().trim_end_matches('/').to_string());
    }

    let mut base = cleaned_fallback;
    for suffix in ["/chat/completions", "/responses", "/embeddings", "/models"] {
        if let Some(stripped) = base.strip_suffix(suffix) {
            base = stripped.trim_end_matches('/').to_string();
            break;
        }
    }
    if base.is_empty() {
        return None;
    }
    if base.rsplit('/').next().is_some_and(is_version_segment) {
        Some(base)
    } else {
        Some(format!("{base}/v1"))
    }
}

fn normalize_root_url(base_url: &str) -> Option<String> {
    let normalized = normalize_base_url(base_url)?;
    if let Some(mut parsed) = parse_url_without_query_fragment(&normalized) {
        let mut segments = collect_path_segments(parsed.path());
        if segments
            .last()
            .is_some_and(|segment| is_version_segment(segment))
        {
            segments.pop();
        }
        if segments.is_empty() {
            parsed.set_path("/");
        } else {
            parsed.set_path(&format!("/{}", segments.join("/")));
        }
        return Some(parsed.to_string().trim_end_matches('/').to_string());
    }

    let cleaned = normalized.trim_end_matches('/');
    let (prefix, segment) = cleaned.rsplit_once('/').unwrap_or(("", cleaned));
    let root = if is_version_segment(segment) {
        prefix.trim_end_matches('/')
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

fn strip_ascii_prefix_case_insensitive<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let head = value.get(..prefix.len())?;
    if !head.eq_ignore_ascii_case(prefix) {
        return None;
    }
    value.get(prefix.len()..)
}

fn normalize_api_key_token(raw: &str) -> Option<&str> {
    let mut token = raw.trim();
    if token.is_empty() {
        return None;
    }
    if let Some(rest) = strip_ascii_prefix_case_insensitive(token, "authorization:") {
        token = rest.trim();
    }
    if let Some(rest) = strip_ascii_prefix_case_insensitive(token, "bearer ") {
        token = rest.trim();
    }
    token = token.trim_matches(|ch| ch == '"' || ch == '\'').trim();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

fn build_headers(api_key: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let Some(token) = normalize_api_key_token(api_key) else {
        return headers;
    };
    if let Ok(value) = format!("Bearer {token}").parse() {
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
    #[test]
    fn normalize_base_url_keeps_existing_version_segment() {
        let normalized = normalize_base_url("https://open.bigmodel.cn/api/paas/v4/")
            .expect("normalized base url");
        assert_eq!(normalized, "https://open.bigmodel.cn/api/paas/v4");
    }

    #[test]
    fn normalize_base_url_strips_resource_suffixes() {
        let normalized = normalize_base_url("https://example.com/v1/chat/completions")
            .expect("normalized base url");
        assert_eq!(normalized, "https://example.com/v1");
    }

    #[test]
    fn build_openai_resource_endpoint_uses_detected_version_path() {
        let endpoint = build_openai_resource_endpoint(
            "https://open.bigmodel.cn/api/paas/v4/",
            CHAT_COMPLETIONS_RESOURCE,
        )
        .expect("chat endpoint");
        assert_eq!(
            endpoint,
            "https://open.bigmodel.cn/api/paas/v4/chat/completions"
        );
    }

    #[test]
    fn normalize_provider_maps_anthropic_aliases() {
        assert_eq!(normalize_provider(Some("anthropic")), "anthropic");
        assert_eq!(normalize_provider(Some("claude")), "anthropic");
        assert_eq!(normalize_provider(Some("anthropic_api")), "anthropic");
    }

    #[test]
    fn is_openai_compatible_provider_excludes_anthropic() {
        assert!(!is_openai_compatible_provider("anthropic"));
        assert!(!is_openai_compatible_provider("claude"));
        assert!(is_openai_compatible_provider("openai_compatible"));
    }

    #[test]
    fn normalize_anthropic_base_url_adds_v1_and_strips_messages_suffix() {
        let normalized =
            normalize_anthropic_base_url("https://aiproxy.xin/cosphere").expect("anthropic base");
        assert_eq!(normalized, "https://aiproxy.xin/cosphere/v1");

        let normalized = normalize_anthropic_base_url("https://api.anthropic.com/v1/messages")
            .expect("anthropic messages endpoint");
        assert_eq!(normalized, "https://api.anthropic.com/v1");
    }

    #[test]
    fn build_anthropic_messages_converts_system_tools_and_tool_results() {
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: Value::String("You are a test assistant.".to_string()),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: Value::String("list files".to_string()),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: Value::String(String::new()),
                reasoning_content: None,
                tool_calls: Some(json!([{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "list_files",
                        "arguments": "{\"path\":\".\"}"
                    }
                }])),
                tool_call_id: None,
            },
            ChatMessage {
                role: "tool".to_string(),
                content: Value::String("[\"src\",\"Cargo.toml\"]".to_string()),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: Some("call_1".to_string()),
            },
        ];

        let (system, anthropic_messages) = build_anthropic_messages(&messages);
        assert_eq!(system, Some("You are a test assistant.".to_string()));
        assert_eq!(anthropic_messages[0]["role"], "user");
        assert_eq!(anthropic_messages[1]["role"], "assistant");
        assert_eq!(anthropic_messages[1]["content"][0]["type"], "tool_use");
        assert_eq!(anthropic_messages[1]["content"][0]["name"], "list_files");
        assert_eq!(anthropic_messages[2]["role"], "user");
        assert_eq!(anthropic_messages[2]["content"][0]["type"], "tool_result");
        assert_eq!(anthropic_messages[2]["content"][0]["tool_use_id"], "call_1");
    }

    #[test]
    fn parse_anthropic_body_extracts_text_reasoning_and_tool_calls() {
        let body = json!({
            "content": [
                { "type": "thinking", "thinking": "step 1" },
                { "type": "text", "text": "done" },
                { "type": "tool_use", "id": "toolu_1", "name": "read_file", "input": { "path": "README.md" } }
            ]
        });
        let (content, reasoning, tool_calls) = parse_anthropic_body(&body);
        assert_eq!(content, "done");
        assert_eq!(reasoning, "step 1");
        let tool_calls = tool_calls.expect("tool calls");
        assert_eq!(tool_calls[0]["function"]["name"], "read_file");
        assert_eq!(
            serde_json::from_str::<Value>(
                tool_calls[0]["function"]["arguments"]
                    .as_str()
                    .unwrap_or("")
            )
            .expect("tool arguments"),
            json!({ "path": "README.md" })
        );
    }

    #[test]
    fn normalize_api_key_token_accepts_prefixed_values() {
        assert_eq!(normalize_api_key_token("sk-test"), Some("sk-test"));
        assert_eq!(normalize_api_key_token("Bearer sk-test"), Some("sk-test"));
        assert_eq!(
            normalize_api_key_token("Authorization: Bearer sk-test"),
            Some("sk-test")
        );
        assert_eq!(normalize_api_key_token(""), None);
    }

    #[test]
    fn build_headers_avoids_duplicate_bearer_prefix() {
        let headers = build_headers("Bearer sk-test");
        let auth = headers
            .get(reqwest::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok());
        assert_eq!(auth, Some("Bearer sk-test"));
    }

    #[test]
    fn anthropic_headers_include_x_api_key_and_normalized_authorization() {
        let config = LlmModelConfig {
            enable: Some(true),
            provider: Some("anthropic".to_string()),
            api_mode: None,
            base_url: Some("https://api.anthropic.com/v1".to_string()),
            api_key: Some("Bearer sk-test".to_string()),
            model: Some("claude-sonnet-4-5-20250929".to_string()),
            temperature: None,
            timeout_s: None,
            max_rounds: None,
            max_context: None,
            max_output: None,
            thinking_token_budget: None,
            support_vision: None,
            support_hearing: None,
            stream: Some(true),
            stream_include_usage: Some(true),
            history_compaction_ratio: None,
            tool_call_mode: Some("function_call".to_string()),
            reasoning_effort: None,
            model_type: Some("llm".to_string()),
            stop: None,
            mock_if_unconfigured: None,
        };
        let headers = LlmClient::new(Client::new(), config).headers();
        let auth = headers
            .get(reqwest::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok());
        let x_api_key = headers
            .get("x-api-key")
            .and_then(|value| value.to_str().ok());
        assert_eq!(auth, Some("Bearer sk-test"));
        assert_eq!(x_api_key, Some("sk-test"));
    }

    #[test]
    fn normalize_root_url_trims_version_segment_only() {
        let root =
            normalize_root_url("https://open.bigmodel.cn/api/paas/v4/").expect("normalized root");
        assert_eq!(root, "https://open.bigmodel.cn/api/paas");
    }

    #[tokio::test]
    async fn embed_texts_retries_without_encoding_format_when_rejected() {
        use axum::routing::post;
        use axum::Router;
        use tokio::net::TcpListener;

        let app = Router::new().route(
            "/v1/embeddings",
            post(
                |axum::extract::Json(payload): axum::extract::Json<Value>| async move {
                    if payload.get("encoding_format").is_some() {
                        return (
                            axum::http::StatusCode::BAD_REQUEST,
                            axum::Json(json!({
                                "error": { "message": "unknown field encoding_format" }
                            })),
                        );
                    }
                    (
                        axum::http::StatusCode::OK,
                        axum::Json(json!({
                            "data": [{ "index": 0, "embedding": [0.1, 0.2, 0.3] }]
                        })),
                    )
                },
            ),
        );

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let config = LlmModelConfig {
            enable: Some(true),
            provider: Some("openai_compatible".to_string()),
            api_mode: None,
            base_url: Some(format!("http://{addr}/v1")),
            api_key: Some("test-key".to_string()),
            model: Some("test-embed-model".to_string()),
            temperature: None,
            timeout_s: None,
            max_rounds: None,
            max_context: None,
            max_output: None,
            thinking_token_budget: None,
            support_vision: None,
            support_hearing: None,
            stream: Some(false),
            stream_include_usage: Some(false),
            history_compaction_ratio: None,
            tool_call_mode: Some("function_call".to_string()),
            reasoning_effort: None,
            model_type: Some("embedding".to_string()),
            stop: None,
            mock_if_unconfigured: None,
        };
        let outputs = embed_texts(&config, &["hello".to_string()], 10)
            .await
            .expect("embed should succeed after retry");
        assert_eq!(outputs, vec![vec![0.1_f32, 0.2_f32, 0.3_f32]]);
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
    async fn process_sse_event_block_supports_line_delimited_json_without_event_separator() {
        let mut combined = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tool_calls = Vec::new();
        let mut on_delta = |_content: String, _reasoning: String| async { Ok(()) };

        let block = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"A\"}}]}\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"B\"}}]}"
        );
        let done = process_sse_event_block(
            block,
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut on_delta,
        )
        .await
        .expect("process line-delimited block");

        assert!(!done);
        assert_eq!(combined, "AB");
    }

    #[tokio::test]
    async fn process_sse_event_block_supports_anthropic_message_payload() {
        let mut combined = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tool_calls = Vec::new();
        let mut on_delta = |_content: String, _reasoning: String| async { Ok(()) };

        let done = process_sse_event_block(
            "data: {\"type\":\"message\",\"content\":[{\"type\":\"text\",\"text\":\"anthropic-ok\"}]}",
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut on_delta,
        )
        .await
        .expect("process anthropic message block");

        assert!(done);
        assert_eq!(combined, "anthropic-ok");
        assert!(reasoning.is_empty());
    }

    #[tokio::test]
    async fn process_sse_event_block_reads_tool_calls_from_message_payload() {
        let mut combined = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tool_calls = Vec::new();
        let mut on_delta = |_content: String, _reasoning: String| async { Ok(()) };

        let done = process_sse_event_block(
            "data: {\"choices\":[{\"delta\":{},\"message\":{\"tool_calls\":[{\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"notes.txt\\\"}\"}}]}}]}",
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut on_delta,
        )
        .await
        .expect("process tool-call message block");

        assert!(!done);
        let finalized = finalize_stream_tool_calls(&tool_calls).expect("tool calls should exist");
        assert_eq!(finalized[0]["function"]["name"], "read_file");
        assert_eq!(
            finalized[0]["function"]["arguments"],
            "{\"path\":\"notes.txt\"}"
        );
    }

    #[tokio::test]
    async fn process_sse_event_block_ignores_empty_tool_calls_array() {
        let mut combined = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tool_calls = Vec::new();
        let mut on_delta = |_content: String, _reasoning: String| async { Ok(()) };

        let done = process_sse_event_block(
            "data: {\"choices\":[{\"delta\":{},\"message\":{\"tool_calls\":[]},\"finish_reason\":\"tool_calls\"}]}",
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut on_delta,
        )
        .await
        .expect("process empty tool-calls block");

        assert!(!done);
        assert!(combined.is_empty());
        assert!(reasoning.is_empty());
        assert!(finalize_stream_tool_calls(&tool_calls).is_none());
    }

    #[tokio::test]
    async fn process_sse_event_block_ignores_assistant_name_metadata_in_delta() {
        let mut combined = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tool_calls = Vec::new();
        let mut on_delta = |_content: String, _reasoning: String| async { Ok(()) };

        let done = process_sse_event_block(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hello\",\"role\":\"assistant\",\"name\":\"MiniMax AI\",\"audio_content\":\"\"}}]}",
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut on_delta,
        )
        .await
        .expect("process minimax-style delta");

        assert!(!done);
        assert_eq!(combined, "hello");
        assert!(reasoning.is_empty());
        assert!(usage.is_none());
        assert!(finalize_stream_tool_calls(&tool_calls).is_none());
    }

    #[test]
    fn update_stream_tool_calls_merges_delta_and_snapshot_without_duplicates() {
        let mut acc = Vec::new();
        update_stream_tool_calls(
            &mut acc,
            &json!({
                "tool_calls": [{
                    "index": 0,
                    "function": {
                        "name": "read_",
                        "arguments": "{\"path\":\""
                    }
                }]
            }),
        );
        update_stream_tool_calls(
            &mut acc,
            &json!({
                "tool_calls": [{
                    "index": 0,
                    "function": {
                        "name": "file",
                        "arguments": "demo.txt\"}"
                    }
                }]
            }),
        );
        update_stream_tool_calls(
            &mut acc,
            &json!({
                "tool_calls": [{
                    "index": 0,
                    "function": {
                        "name": "read_file",
                        "arguments": "{\"path\":\"demo.txt\"}"
                    }
                }]
            }),
        );

        let finalized = finalize_stream_tool_calls(&acc).expect("tool calls should exist");
        assert_eq!(finalized[0]["function"]["name"], "read_file");
        assert_eq!(
            finalized[0]["function"]["arguments"],
            "{\"path\":\"demo.txt\"}"
        );
    }

    #[test]
    fn extract_tool_calls_ignores_empty_array_payload() {
        let payload = json!({
            "tool_calls": []
        });
        assert!(extract_tool_calls(&payload).is_none());
    }

    #[test]
    fn is_false_tool_stop_reason_matches_known_variants() {
        assert!(is_false_tool_stop_reason(Some(&json!("tool_calls"))));
        assert!(is_false_tool_stop_reason(Some(&json!("toolUse"))));
        assert!(is_false_tool_stop_reason(Some(&json!("tool_use"))));
        assert!(!is_false_tool_stop_reason(Some(&json!("stop"))));
    }

    #[test]
    fn merge_stream_text_field_preserves_whitespace_between_fragments() {
        let mut merged = String::new();
        merge_stream_text_field(&mut merged, "import ");
        merge_stream_text_field(&mut merged, "json\nfrom datetime import ");
        merge_stream_text_field(&mut merged, "datetime, timedelta");

        assert_eq!(
            merged,
            "import json\nfrom datetime import datetime, timedelta"
        );
    }

    #[test]
    fn merge_stream_text_field_preserves_single_character_prefix_overlap() {
        let mut merged = "plt.savefig(path, b".to_string();
        merge_stream_text_field(&mut merged, "box_inches='tight')");

        assert_eq!(merged, "plt.savefig(path, bbox_inches='tight')");
    }

    #[test]
    fn update_stream_tool_calls_preserves_whitespace_in_arguments() {
        let mut acc = Vec::new();
        update_stream_tool_calls(
            &mut acc,
            &json!({
                "tool_calls": [{
                    "index": 0,
                    "function": {
                        "name": "ptc",
                        "arguments": "{\"content\":\"import "
                    }
                }]
            }),
        );
        update_stream_tool_calls(
            &mut acc,
            &json!({
                "tool_calls": [{
                    "index": 0,
                    "function": {
                        "arguments": "json\\nfrom datetime import "
                    }
                }]
            }),
        );
        update_stream_tool_calls(
            &mut acc,
            &json!({
                "tool_calls": [{
                    "index": 0,
                    "function": {
                        "arguments": "datetime, timedelta\"}"
                    }
                }]
            }),
        );

        let finalized = finalize_stream_tool_calls(&acc).expect("tool calls should exist");
        assert_eq!(finalized[0]["function"]["name"], "ptc");
        assert_eq!(
            finalized[0]["function"]["arguments"],
            "{\"content\":\"import json\\nfrom datetime import datetime, timedelta\"}"
        );
    }

    #[test]
    fn update_stream_tool_calls_ignores_assistant_name_metadata() {
        let mut acc = Vec::new();
        update_stream_tool_calls(
            &mut acc,
            &json!({
                "content": "<think>...</think>",
                "role": "assistant",
                "name": "MiniMax AI",
                "audio_content": ""
            }),
        );
        assert!(finalize_stream_tool_calls(&acc).is_none());
    }

    #[test]
    fn normalize_tool_arguments_json_wraps_invalid_payload_as_raw_object() {
        let normalized = normalize_tool_arguments_json("python3 -c \"print('hello')\"");
        assert_eq!(
            serde_json::from_str::<Value>(&normalized).expect("normalized json"),
            json!({ "raw": "python3 -c \"print('hello')\"" })
        );
    }

    #[test]
    fn tool_call_to_responses_item_sanitizes_invalid_arguments_json() {
        let item = tool_call_to_responses_item(
            &json!({
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "execute_command",
                    "arguments": "python3 -c \"print('hello')\""
                }
            }),
            0,
        )
        .expect("tool call item");

        assert_eq!(
            item["arguments"],
            Value::String("{\"raw\":\"python3 -c \\\"print('hello')\\\"\"}".to_string())
        );
        assert_eq!(
            serde_json::from_str::<Value>(item["arguments"].as_str().unwrap_or(""))
                .expect("sanitized args json"),
            json!({ "raw": "python3 -c \"print('hello')\"" })
        );
    }

    #[test]
    fn tool_call_to_responses_item_converts_freeform_tool_to_custom_call() {
        let item = tool_call_to_responses_item(
            &json!({
                "id": "call_patch",
                "type": "function",
                "function": {
                    "name": "apply_patch",
                    "arguments": "{\"input\":\"*** Begin Patch\\n*** End Patch\"}"
                }
            }),
            0,
        )
        .expect("tool call item");

        assert_eq!(item["type"], "custom_tool_call");
        assert_eq!(item["call_id"], "call_patch");
        assert_eq!(item["name"], "apply_patch");
        assert_eq!(item["input"], "*** Begin Patch\n*** End Patch");
    }

    #[test]
    fn build_responses_input_roundtrips_custom_tool_outputs() {
        let messages = vec![
            ChatMessage {
                role: "assistant".to_string(),
                content: json!(""),
                reasoning_content: None,
                tool_calls: Some(json!([{
                    "id": "call_patch",
                    "type": "function",
                    "function": {
                        "name": "apply_patch",
                        "arguments": "{\"input\":\"*** Begin Patch\\n*** End Patch\"}"
                    }
                }])),
                tool_call_id: None,
            },
            ChatMessage {
                role: "tool".to_string(),
                content: json!("applied"),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: Some("call_patch".to_string()),
            },
        ];

        let input = build_responses_input(&messages);
        let items = input.as_array().expect("responses input array");

        assert_eq!(items[1]["type"], "custom_tool_call");
        assert_eq!(items[2]["type"], "custom_tool_call_output");
        assert_eq!(items[2]["call_id"], "call_patch");
    }

    #[test]
    fn finalize_stream_tool_calls_repairs_custom_tool_input_stream() {
        let mut acc = Vec::new();
        update_responses_tool_call_from_item(
            &mut acc,
            &json!({
                "type": "custom_tool_call",
                "id": "item_patch",
                "call_id": "call_patch",
                "name": "apply_patch",
                "input": ""
            }),
        );
        update_responses_tool_call_from_item(
            &mut acc,
            &json!({
                "type": "custom_tool_call",
                "id": "item_patch",
                "call_id": "call_patch",
                "name": "apply_patch",
                "input": "*** Begin Patch\n*** Add File: hello.txt\n+hello world\n*** End Patch"
            }),
        );

        let finalized = finalize_stream_tool_calls(&acc).expect("tool calls should exist");
        assert_eq!(finalized[0]["function"]["name"], "apply_patch");
        assert_eq!(
            serde_json::from_str::<Value>(
                finalized[0]["function"]["arguments"].as_str().unwrap_or(""),
            )
            .expect("custom tool arguments json"),
            json!({
                "input": "*** Begin Patch\n*** Add File: hello.txt\n+hello world\n*** End Patch"
            })
        );
    }

    #[test]
    fn merge_stream_text_field_replaces_later_complete_json_payload() {
        let mut merged = "{\"content\":\"hello\"}".to_string();
        merge_stream_text_field(&mut merged, "{\"content\":\"hello world\"}");
        assert_eq!(merged, "{\"content\":\"hello world\"}");
    }

    #[test]
    fn merge_stream_text_field_replaces_empty_json_before_complete_payload() {
        let mut merged = "{}".to_string();
        merge_stream_text_field(
            &mut merged,
            "{\"filename\":\"draw_heart.py\",\"content\":\"print('ok')\"}",
        );
        assert_eq!(
            merged,
            "{\"filename\":\"draw_heart.py\",\"content\":\"print('ok')\"}"
        );
    }

    #[test]
    fn finalize_stream_tool_calls_replaces_empty_json_seed_from_anthropic_tool_use() {
        let mut acc = Vec::new();
        merge_stream_tool_call_item(
            &mut acc,
            &json!({
                "index": 0,
                "id": "call_1",
                "function": {
                    "name": "ptc",
                    "arguments": "{}"
                }
            }),
        );
        merge_stream_tool_call_item(
            &mut acc,
            &json!({
                "index": 0,
                "function": {
                    "arguments": "{\"filename\":\"draw_heart.py\",\"content\":\"print('ok')\"}"
                }
            }),
        );

        let finalized = finalize_stream_tool_calls(&acc).expect("tool calls should exist");
        assert_eq!(finalized[0]["function"]["name"], "ptc");
        assert_eq!(
            finalized[0]["function"]["arguments"],
            "{\"filename\":\"draw_heart.py\",\"content\":\"print('ok')\"}"
        );
    }

    #[test]
    fn finalize_stream_tool_calls_replaces_empty_json_seed_from_incremental_anthropic_fragments()
    {
        let mut acc = Vec::new();
        merge_stream_tool_call_item(
            &mut acc,
            &json!({
                "index": 0,
                "id": "call_1",
                "function": {
                    "name": "ptc",
                    "arguments": "{}"
                }
            }),
        );
        for fragment in [
            "{",
            "\"filename\": ",
            "\"demo",
            ".py",
            "\"",
            ", ",
            "\"content\": ",
            "\"print",
            "(",
            "1",
            ")",
            "\"",
            "}",
        ] {
            merge_stream_tool_call_item(
                &mut acc,
                &json!({
                    "index": 0,
                    "function": {
                        "arguments": fragment
                    }
                }),
            );
        }

        let finalized = finalize_stream_tool_calls(&acc).expect("tool calls should exist");
        assert_eq!(finalized[0]["function"]["name"], "ptc");
        let arguments = finalized[0]["function"]["arguments"]
            .as_str()
            .expect("arguments string");
        assert_eq!(
            serde_json::from_str::<Value>(arguments).expect("incremental anthropic args json"),
            json!({
                "filename": "demo.py",
                "content": "print(1)"
            })
        );
    }

    #[tokio::test]
    async fn process_anthropic_stream_tool_use_replaces_empty_input_seed() {
        let mut combined = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tool_calls = Vec::new();
        let mut on_delta = |_content: String, _reasoning: String| async { Ok(()) };

        let done = process_sse_event_block(
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"call_1\",\"name\":\"ptc\",\"input\":{}}}",
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut on_delta,
        )
        .await
        .expect("process anthropic tool_use start");
        assert!(!done);

        let done = process_sse_event_block(
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"filename\\\":\\\"draw_heart.py\\\",\\\"content\\\":\\\"print('ok')\\\"}\"}}",
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut on_delta,
        )
        .await
        .expect("process anthropic input json delta");
        assert!(!done);

        let finalized = finalize_stream_tool_calls(&tool_calls).expect("tool calls should exist");
        assert_eq!(finalized[0]["function"]["name"], "ptc");
        assert_eq!(
            finalized[0]["function"]["arguments"],
            "{\"filename\":\"draw_heart.py\",\"content\":\"print('ok')\"}"
        );
    }

    #[test]
    fn extract_tool_calls_repairs_multiline_arguments_json() {
        let message = json!({
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "execute_command",
                    "arguments": "{\"content\": \"python3 -c \\\"\nprint('Map saved')\n\\\", \"workdir\": \".\"}"
                }
            }]
        });

        let tool_calls = extract_tool_calls(&message).expect("tool calls");
        assert_eq!(
            tool_calls,
            json!([{
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "execute_command",
                    "arguments": "{\"content\":\"python3 -c \\\"\\nprint('Map saved')\\n\\\"\",\"workdir\":\".\"}"
                }
            }])
        );
    }

    #[tokio::test]
    async fn process_sse_event_block_accepts_raw_json_without_data_prefix() {
        let mut combined = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tool_calls = Vec::new();
        let mut on_delta = |_content: String, _reasoning: String| async { Ok(()) };

        let done = process_sse_event_block(
            r#"{"choices":[{"message":{"content":"raw-json"}}]}"#,
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut on_delta,
        )
        .await
        .expect("process raw json block");

        assert!(!done);
        assert_eq!(combined, "raw-json");
        assert!(reasoning.is_empty());
    }

    #[tokio::test]
    async fn process_sse_event_block_marks_openai_tool_stream_as_activity() {
        let mut combined = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tool_calls = Vec::new();
        let callbacks = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, String)>::new()));
        let callbacks_for_closure = std::sync::Arc::clone(&callbacks);
        let mut on_delta = move |content: String, reasoning: String| {
            let callbacks = std::sync::Arc::clone(&callbacks_for_closure);
            async move {
                callbacks
                    .lock()
                    .expect("lock callbacks")
                    .push((content, reasoning));
                Ok(())
            }
        };

        let done = process_sse_event_block(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"execute_command\",\"arguments\":\"{\\\"command\\\":\\\"echo hi\\\"}\"}}]}}]}",
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut on_delta,
        )
        .await
        .expect("process tool-call stream block");

        assert!(!done);
        assert!(combined.is_empty());
        assert!(reasoning.is_empty());
        assert_eq!(
            callbacks.lock().expect("lock callbacks").as_slice(),
            &[(String::new(), String::new())]
        );
        let finalized = finalize_stream_tool_calls(&tool_calls).expect("tool calls should exist");
        assert_eq!(finalized[0]["function"]["name"], "execute_command");
    }

    #[tokio::test]
    async fn process_sse_event_block_marks_responses_tool_stream_as_activity() {
        let mut combined = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tool_calls = Vec::new();
        let callbacks = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, String)>::new()));
        let callbacks_for_closure = std::sync::Arc::clone(&callbacks);
        let mut on_delta = move |content: String, reasoning: String| {
            let callbacks = std::sync::Arc::clone(&callbacks_for_closure);
            async move {
                callbacks
                    .lock()
                    .expect("lock callbacks")
                    .push((content, reasoning));
                Ok(())
            }
        };

        let done = process_sse_event_block(
            "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"item_1\",\"call_id\":\"call_1\",\"name\":\"execute_command\",\"arguments\":\"{\\\"command\\\":\\\"echo hi\\\"}\"}}",
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut on_delta,
        )
        .await
        .expect("process responses tool stream block");

        assert!(!done);
        assert!(combined.is_empty());
        assert!(reasoning.is_empty());
        assert_eq!(
            callbacks.lock().expect("lock callbacks").as_slice(),
            &[(String::new(), String::new())]
        );
        let finalized = finalize_stream_tool_calls(&tool_calls).expect("tool calls should exist");
        assert_eq!(finalized[0]["function"]["name"], "execute_command");
    }

    #[tokio::test]
    async fn stream_complete_accepts_tail_event_without_done_or_newline() {
        use axum::body::Body;
        use axum::http::StatusCode;
        use axum::response::Response;
        use axum::routing::post;
        use axum::Router;
        use bytes::Bytes;
        use futures::stream;
        use tokio::net::TcpListener;

        let app = Router::new().route(
            "/v1/chat/completions",
            post(|| async {
                let stream = stream::iter(vec![Ok::<_, std::convert::Infallible>(Bytes::from(
                    r#"data: {"choices":[{"delta":{"content":"tail"}}]}"#,
                ))]);
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .body(Body::from_stream(stream))
                    .expect("build sse response")
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let config = LlmModelConfig {
            enable: Some(true),
            provider: Some("openai_compatible".to_string()),
            api_mode: None,
            base_url: Some(format!("http://{addr}/v1")),
            api_key: Some("test-key".to_string()),
            model: Some("test-model".to_string()),
            temperature: None,
            timeout_s: None,
            max_rounds: None,
            max_context: None,
            max_output: None,
            thinking_token_budget: None,
            support_vision: None,
            support_hearing: None,
            stream: Some(true),
            stream_include_usage: Some(true),
            history_compaction_ratio: None,
            tool_call_mode: Some("tool_call".to_string()),
            reasoning_effort: None,
            model_type: Some("llm".to_string()),
            stop: None,
            mock_if_unconfigured: None,
        };
        let client = LlmClient::new(Client::new(), config);
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: Value::String("hello".to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        }];

        let response = client
            .stream_complete_with_callback(&messages, |_delta, _reasoning| async { Ok(()) })
            .await
            .expect("stream complete");

        assert_eq!(response.content, "tail");
        assert!(response.reasoning.is_empty());
    }

    #[tokio::test]
    async fn stream_complete_falls_back_to_non_stream_after_retryable_http_failure() {
        use axum::extract::State;
        use axum::http::StatusCode;
        use axum::routing::post;
        use axum::{Json, Router};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::net::TcpListener;

        #[derive(Clone)]
        struct AppState {
            stream_calls: Arc<AtomicUsize>,
            non_stream_calls: Arc<AtomicUsize>,
        }

        let state = AppState {
            stream_calls: Arc::new(AtomicUsize::new(0)),
            non_stream_calls: Arc::new(AtomicUsize::new(0)),
        };
        let app = Router::new().route(
            "/v1/chat/completions",
            post(
                |State(state): State<AppState>,
                 Json(payload): Json<Value>| async move {
                    if payload.get("stream").and_then(Value::as_bool) == Some(true) {
                        state.stream_calls.fetch_add(1, Ordering::SeqCst);
                        return (
                            StatusCode::TOO_MANY_REQUESTS,
                            Json(json!({
                                "error": {
                                    "message": "Rate limit reached for TPM"
                                }
                            })),
                        );
                    }
                    state.non_stream_calls.fetch_add(1, Ordering::SeqCst);
                    (
                        StatusCode::OK,
                        Json(json!({
                            "choices": [
                                {
                                    "message": {
                                        "content": "fallback-ok"
                                    }
                                }
                            ],
                            "usage": {
                                "prompt_tokens": 12,
                                "completion_tokens": 3,
                                "total_tokens": 15
                            }
                        })),
                    )
                },
            ),
        )
        .with_state(state.clone());

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let config = LlmModelConfig {
            enable: Some(true),
            provider: Some("openai_compatible".to_string()),
            api_mode: Some("chat_completions".to_string()),
            base_url: Some(format!("http://{addr}/v1")),
            api_key: Some("test-key".to_string()),
            model: Some("test-model".to_string()),
            temperature: None,
            timeout_s: None,
            max_rounds: None,
            max_context: None,
            max_output: None,
            thinking_token_budget: None,
            support_vision: None,
            support_hearing: None,
            stream: Some(true),
            stream_include_usage: Some(true),
            history_compaction_ratio: None,
            tool_call_mode: Some("tool_call".to_string()),
            reasoning_effort: None,
            model_type: Some("llm".to_string()),
            stop: None,
            mock_if_unconfigured: None,
        };
        let client = LlmClient::new(Client::new(), config);
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: Value::String("hello".to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        }];

        let response = client
            .stream_complete_with_callback(&messages, |_delta, _reasoning| async { Ok(()) })
            .await
            .expect("fallback response");

        assert_eq!(response.content, "fallback-ok");
        assert_eq!(
            state.stream_calls.load(Ordering::SeqCst),
            1,
            "stream request should be attempted once"
        );
        assert_eq!(
            state.non_stream_calls.load(Ordering::SeqCst),
            1,
            "non-stream fallback should be attempted once"
        );
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

    #[tokio::test]
    async fn process_sse_event_block_returns_context_window_error_from_response_failed() {
        let mut combined = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tool_calls = Vec::new();
        let mut on_delta = |_content: String, _reasoning: String| async { Ok(()) };

        let err = process_sse_event_block(
            "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"context_length_exceeded\",\"message\":\"Your input exceeds the context window of this model. Please adjust your input and try again.\"}}}",
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut on_delta,
        )
        .await
        .expect_err("context window error should bubble up");

        let message = err.to_string();
        assert!(message.contains("context_length_exceeded"));
        assert!(message.contains("context window"));
    }

    #[tokio::test]
    async fn process_sse_event_block_returns_top_level_error_payload() {
        let mut combined = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tool_calls = Vec::new();
        let mut on_delta = |_content: String, _reasoning: String| async { Ok(()) };

        let err = process_sse_event_block(
            "data: {\"error\":{\"code\":\"context_length_exceeded\",\"message\":\"Prompt is too long.\"}}",
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut on_delta,
        )
        .await
        .expect_err("top-level stream error should bubble up");

        assert!(err.to_string().contains("Prompt is too long"));
    }

    #[test]
    fn normalize_tool_call_mode_accepts_freeform_call_aliases() {
        assert_eq!(
            normalize_tool_call_mode(Some("freeform_call")),
            ToolCallMode::FreeformCall
        );
        assert_eq!(
            normalize_tool_call_mode(Some("freeform")),
            ToolCallMode::FreeformCall
        );
        assert_eq!(
            normalize_tool_call_mode(Some("custom_tool_call")),
            ToolCallMode::FreeformCall
        );
    }

    #[test]
    fn normalize_tool_call_mode_defaults_to_function_call() {
        assert_eq!(normalize_tool_call_mode(None), ToolCallMode::FunctionCall);
    }

    #[test]
    fn normalize_reasoning_effort_accepts_codex_levels() {
        assert_eq!(
            normalize_reasoning_effort(Some("minimal")),
            Some("minimal".to_string())
        );
        assert_eq!(
            normalize_reasoning_effort(Some("x_high")),
            Some("xhigh".to_string())
        );
        assert_eq!(
            normalize_reasoning_effort(Some("disabled")),
            Some("none".to_string())
        );
        assert_eq!(normalize_reasoning_effort(Some("default")), None);
        assert_eq!(normalize_reasoning_effort(Some("unknown")), None);
    }

    #[test]
    fn build_chat_payload_includes_reasoning_effort_when_configured() {
        let config = LlmModelConfig {
            enable: Some(true),
            provider: Some("openai_compatible".to_string()),
            api_mode: Some("chat_completions".to_string()),
            base_url: Some("http://127.0.0.1:18000/v1".to_string()),
            api_key: Some("test-key".to_string()),
            model: Some("test-model".to_string()),
            temperature: Some(0.7),
            timeout_s: Some(15),
            max_rounds: Some(4),
            max_context: Some(16_384),
            max_output: Some(256),
            thinking_token_budget: None,
            support_vision: Some(false),
            support_hearing: Some(false),
            stream: Some(false),
            stream_include_usage: Some(false),
            history_compaction_ratio: None,
            tool_call_mode: Some("tool_call".to_string()),
            reasoning_effort: Some("x_high".to_string()),
            model_type: Some("llm".to_string()),
            stop: None,
            mock_if_unconfigured: None,
        };
        let client = LlmClient::new(Client::new(), config);
        let payload = client.build_request_payload(
            &[ChatMessage {
                role: "user".to_string(),
                content: Value::String("hello".to_string()),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            false,
        );
        assert_eq!(
            payload.get("reasoning_effort"),
            Some(&Value::String("xhigh".to_string()))
        );
        assert_eq!(payload["max_tokens"], 256);
        assert_eq!(payload["thinking_token_budget"], 16_384);
        assert_eq!(payload["thinking_budget_tokens"], 16_384);
    }

    #[test]
    fn build_responses_payload_includes_reasoning_effort_when_configured() {
        let config = LlmModelConfig {
            enable: Some(true),
            provider: Some("openai".to_string()),
            api_mode: Some("responses".to_string()),
            base_url: Some("https://api.openai.com/v1".to_string()),
            api_key: Some("test-key".to_string()),
            model: Some("gpt-5.2".to_string()),
            temperature: Some(0.7),
            timeout_s: Some(15),
            max_rounds: Some(4),
            max_context: Some(16_384),
            max_output: Some(256),
            thinking_token_budget: None,
            support_vision: Some(false),
            support_hearing: Some(false),
            stream: Some(false),
            stream_include_usage: Some(false),
            history_compaction_ratio: None,
            tool_call_mode: Some("freeform_call".to_string()),
            reasoning_effort: Some("high".to_string()),
            model_type: Some("llm".to_string()),
            stop: None,
            mock_if_unconfigured: None,
        };
        let client = LlmClient::new(Client::new(), config);
        let payload = client.build_request_payload(
            &[ChatMessage {
                role: "user".to_string(),
                content: Value::String("hello".to_string()),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            false,
        );
        assert_eq!(payload["reasoning"]["effort"], "high");
        assert_eq!(payload["max_output_tokens"], 256);
        assert_eq!(payload["thinking_token_budget"], 16_384);
        assert_eq!(payload["thinking_budget_tokens"], 16_384);
    }

    #[test]
    fn build_chat_payload_disables_thinking_for_qwen_compatible_requests() {
        let config = LlmModelConfig {
            enable: Some(true),
            provider: Some("qwen".to_string()),
            api_mode: Some("chat_completions".to_string()),
            base_url: Some("https://dashscope.aliyuncs.com/compatible-mode/v1".to_string()),
            api_key: Some("test-key".to_string()),
            model: Some("qwen3.5-32b".to_string()),
            temperature: Some(0.7),
            timeout_s: Some(15),
            max_rounds: Some(4),
            max_context: Some(16_384),
            max_output: Some(256),
            thinking_token_budget: None,
            support_vision: Some(false),
            support_hearing: Some(false),
            stream: Some(false),
            stream_include_usage: Some(false),
            history_compaction_ratio: None,
            tool_call_mode: Some("tool_call".to_string()),
            reasoning_effort: Some("disabled".to_string()),
            model_type: Some("llm".to_string()),
            stop: None,
            mock_if_unconfigured: None,
        };
        let client = LlmClient::new(Client::new(), config);
        let payload = client.build_request_payload(
            &[ChatMessage {
                role: "user".to_string(),
                content: Value::String("hello".to_string()),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            false,
        );

        assert_eq!(payload["reasoning_effort"], "none");
        assert_eq!(payload["enable_thinking"], false);
        assert!(payload.get("chat_template_kwargs").is_none());
        assert!(payload.get("thinking_token_budget").is_none());
        assert!(payload.get("thinking_budget_tokens").is_none());
    }

    #[test]
    fn build_chat_payload_disables_thinking_for_local_vllm_requests() {
        let config = LlmModelConfig {
            enable: Some(true),
            provider: Some("openai_compatible".to_string()),
            api_mode: Some("chat_completions".to_string()),
            base_url: Some("http://127.0.0.1:8000/v1".to_string()),
            api_key: Some("test-key".to_string()),
            model: Some("Qwen/Qwen3-8B".to_string()),
            temperature: Some(0.7),
            timeout_s: Some(15),
            max_rounds: Some(4),
            max_context: Some(16_384),
            max_output: Some(256),
            thinking_token_budget: None,
            support_vision: Some(false),
            support_hearing: Some(false),
            stream: Some(false),
            stream_include_usage: Some(false),
            history_compaction_ratio: None,
            tool_call_mode: Some("tool_call".to_string()),
            reasoning_effort: Some("none".to_string()),
            model_type: Some("llm".to_string()),
            stop: None,
            mock_if_unconfigured: None,
        };
        let client = LlmClient::new(Client::new(), config);
        let payload = client.build_request_payload(
            &[ChatMessage {
                role: "user".to_string(),
                content: Value::String("hello".to_string()),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            false,
        );

        assert_eq!(payload["reasoning_effort"], "none");
        assert_eq!(payload["enable_thinking"], false);
        assert_eq!(payload["chat_template_kwargs"]["enable_thinking"], false);
        assert!(payload.get("thinking_token_budget").is_none());
        assert!(payload.get("thinking_budget_tokens").is_none());
    }

    #[test]
    fn build_chat_payload_disables_thinking_for_remote_openai_compatible_requests() {
        let config = LlmModelConfig {
            enable: Some(true),
            provider: Some("openai_compatible".to_string()),
            api_mode: Some("chat_completions".to_string()),
            base_url: Some("https://dashscope.aliyuncs.com/compatible-mode/v1".to_string()),
            api_key: Some("test-key".to_string()),
            model: Some("qwen3.5-35b-a3b".to_string()),
            temperature: Some(0.7),
            timeout_s: Some(15),
            max_rounds: Some(4),
            max_context: Some(16_384),
            max_output: Some(256),
            thinking_token_budget: None,
            support_vision: Some(false),
            support_hearing: Some(false),
            stream: Some(false),
            stream_include_usage: Some(false),
            history_compaction_ratio: None,
            tool_call_mode: Some("tool_call".to_string()),
            reasoning_effort: Some("none".to_string()),
            model_type: Some("llm".to_string()),
            stop: None,
            mock_if_unconfigured: None,
        };
        let client = LlmClient::new(Client::new(), config);
        let payload = client.build_request_payload(
            &[ChatMessage {
                role: "user".to_string(),
                content: Value::String("hello".to_string()),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            false,
        );

        assert_eq!(payload["reasoning_effort"], "none");
        assert_eq!(payload["enable_thinking"], false);
        assert_eq!(payload["chat_template_kwargs"]["enable_thinking"], false);
        assert!(payload.get("thinking_token_budget").is_none());
        assert!(payload.get("thinking_budget_tokens").is_none());
    }

    #[test]
    fn build_responses_payload_keeps_openai_disable_thinking_standard_only() {
        let config = LlmModelConfig {
            enable: Some(true),
            provider: Some("openai".to_string()),
            api_mode: Some("responses".to_string()),
            base_url: Some("https://api.openai.com/v1".to_string()),
            api_key: Some("test-key".to_string()),
            model: Some("gpt-5.2".to_string()),
            temperature: Some(0.7),
            timeout_s: Some(15),
            max_rounds: Some(4),
            max_context: Some(16_384),
            max_output: Some(256),
            thinking_token_budget: None,
            support_vision: Some(false),
            support_hearing: Some(false),
            stream: Some(false),
            stream_include_usage: Some(false),
            history_compaction_ratio: None,
            tool_call_mode: Some("freeform_call".to_string()),
            reasoning_effort: Some("disabled".to_string()),
            model_type: Some("llm".to_string()),
            stop: None,
            mock_if_unconfigured: None,
        };
        let client = LlmClient::new(Client::new(), config);
        let payload = client.build_request_payload(
            &[ChatMessage {
                role: "user".to_string(),
                content: Value::String("hello".to_string()),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            false,
        );

        assert_eq!(payload["reasoning"]["effort"], "none");
        assert!(payload.get("enable_thinking").is_none());
        assert!(payload.get("chat_template_kwargs").is_none());
        assert_eq!(payload["thinking_token_budget"], 16_384);
        assert_eq!(payload["thinking_budget_tokens"], 16_384);
    }

    #[test]
    fn build_chat_payload_uses_global_defaults_when_max_output_is_unset() {
        let config = LlmModelConfig {
            enable: Some(true),
            provider: Some("openai_compatible".to_string()),
            api_mode: Some("chat_completions".to_string()),
            base_url: Some("http://127.0.0.1:18000/v1".to_string()),
            api_key: Some("test-key".to_string()),
            model: Some("test-model".to_string()),
            temperature: Some(0.7),
            timeout_s: Some(15),
            max_rounds: Some(4),
            max_context: Some(16_384),
            max_output: None,
            thinking_token_budget: None,
            support_vision: Some(false),
            support_hearing: Some(false),
            stream: Some(false),
            stream_include_usage: Some(false),
            history_compaction_ratio: None,
            tool_call_mode: Some("tool_call".to_string()),
            reasoning_effort: None,
            model_type: Some("llm".to_string()),
            stop: None,
            mock_if_unconfigured: None,
        };
        let client = LlmClient::new(Client::new(), config);
        let payload = client.build_request_payload(
            &[ChatMessage {
                role: "user".to_string(),
                content: Value::String("hello".to_string()),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            false,
        );

        assert_eq!(payload["max_tokens"], 32_768);
        assert_eq!(payload["thinking_token_budget"], 16_384);
        assert_eq!(payload["thinking_budget_tokens"], 16_384);
    }

    #[test]
    fn build_anthropic_payload_does_not_include_thinking_budget_fields() {
        let config = LlmModelConfig {
            enable: Some(true),
            provider: Some("anthropic".to_string()),
            api_mode: None,
            base_url: Some("https://api.anthropic.com/v1".to_string()),
            api_key: Some("sk-test".to_string()),
            model: Some("claude-sonnet-4-5-20250929".to_string()),
            temperature: Some(0.7),
            timeout_s: Some(15),
            max_rounds: Some(4),
            max_context: Some(16_384),
            max_output: None,
            thinking_token_budget: None,
            support_vision: Some(false),
            support_hearing: Some(false),
            stream: Some(false),
            stream_include_usage: Some(false),
            history_compaction_ratio: None,
            tool_call_mode: Some("function_call".to_string()),
            reasoning_effort: None,
            model_type: Some("llm".to_string()),
            stop: None,
            mock_if_unconfigured: None,
        };
        let client = LlmClient::new(Client::new(), config);
        let payload = client.build_request_payload(
            &[ChatMessage {
                role: "user".to_string(),
                content: Value::String("hello".to_string()),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            false,
        );

        assert!(payload.get("thinking_token_budget").is_none());
        assert!(payload.get("thinking_budget_tokens").is_none());
        assert_eq!(payload["max_tokens"], 32_768);
    }

    #[test]
    fn resolve_tool_call_mode_defaults_by_provider() {
        let mut config = LlmModelConfig {
            enable: Some(true),
            provider: Some("openai".to_string()),
            api_mode: None,
            base_url: None,
            api_key: None,
            model: Some("gpt-4.1".to_string()),
            temperature: None,
            timeout_s: None,
            max_rounds: None,
            max_context: None,
            max_output: None,
            thinking_token_budget: None,
            support_vision: None,
            support_hearing: None,
            stream: Some(true),
            stream_include_usage: Some(true),
            history_compaction_ratio: None,
            tool_call_mode: None,
            reasoning_effort: None,
            model_type: Some("llm".to_string()),
            stop: None,
            mock_if_unconfigured: None,
        };
        assert_eq!(resolve_tool_call_mode(&config), ToolCallMode::FreeformCall);

        config.provider = Some("openai_compatible".to_string());
        assert_eq!(resolve_tool_call_mode(&config), ToolCallMode::FunctionCall);
    }

    #[test]
    fn resolve_openai_api_mode_defaults_to_responses_for_gpt5_on_openai_provider() {
        let config = LlmModelConfig {
            enable: Some(true),
            provider: Some("openai".to_string()),
            api_mode: None,
            base_url: Some("https://api.openai.com/v1".to_string()),
            api_key: Some("test-key".to_string()),
            model: Some("gpt-5.2".to_string()),
            temperature: None,
            timeout_s: None,
            max_rounds: None,
            max_context: None,
            max_output: None,
            thinking_token_budget: None,
            support_vision: None,
            support_hearing: None,
            stream: Some(true),
            stream_include_usage: Some(true),
            history_compaction_ratio: None,
            tool_call_mode: Some("tool_call".to_string()),
            reasoning_effort: None,
            model_type: Some("llm".to_string()),
            stop: None,
            mock_if_unconfigured: None,
        };
        assert_eq!(resolve_openai_api_mode(&config), OpenAiApiMode::Responses);
    }

    #[test]
    fn normalize_responses_tool_definition_flattens_chat_shape_and_fixes_schema() {
        let tool = json!({
            "type": "function",
            "function": {
                "name": "apply_patch",
                "description": "Apply patch",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "input": {
                            "type": "string"
                        }
                    }
                }
            }
        });
        let normalized = normalize_responses_tool_definition(&tool, false);
        assert_eq!(normalized["type"], "function");
        assert_eq!(normalized["name"], "apply_patch");
        assert_eq!(
            normalized["parameters"]["properties"]["input"]["type"],
            "string"
        );
    }

    #[test]
    fn normalize_responses_tool_definition_preserves_custom_tool_format() {
        let tool = json!({
            "type": "custom",
            "name": "apply_patch",
            "description": "Apply patch",
            "format": {
                "type": "grammar",
                "syntax": "lark",
                "definition": "start: \"ok\""
            }
        });

        let normalized = normalize_responses_tool_definition(&tool, true);
        assert_eq!(normalized["type"], "custom");
        assert_eq!(normalized["name"], "apply_patch");
        assert_eq!(normalized["format"]["syntax"], "lark");
    }

    #[test]
    fn normalize_chat_tool_definition_wraps_responses_shape_and_fixes_schema() {
        let tool = json!({
            "type": "function",
            "name": "apply_patch",
            "description": "Apply patch",
            "parameters": {
                "type": "object",
                "properties": {
                    "input": {
                        "type": "string"
                    }
                }
            }
        });
        let normalized = normalize_chat_tool_definition(&tool, false);
        assert_eq!(normalized["type"], "function");
        assert_eq!(normalized["function"]["name"], "apply_patch");
        assert_eq!(
            normalized["function"]["parameters"]["properties"]["input"]["type"],
            "string"
        );
    }

    #[test]
    fn normalize_responses_tool_definition_strips_forbidden_top_level_for_openai_only() {
        let tool = json!({
            "type": "function",
            "name": "apply_patch",
            "description": "Apply patch",
            "parameters": {
                "type": "object",
                "properties": {
                    "input": {"type": "string"}
                },
                "allOf": [
                    {"required": ["input"]}
                ]
            }
        });
        let preserved = normalize_responses_tool_definition(&tool, false);
        let stripped = normalize_responses_tool_definition(&tool, true);

        assert!(preserved["parameters"].get("allOf").is_some());
        assert!(stripped["parameters"].get("allOf").is_none());
    }

    #[test]
    fn should_strip_openai_tool_schema_only_for_openai_provider() {
        assert!(should_strip_openai_tool_schema(Some("openai")));
        assert!(!should_strip_openai_tool_schema(Some("openai_compatible")));
        assert!(!should_strip_openai_tool_schema(Some("groq")));
    }

    #[test]
    fn normalize_usage_excludes_reasoning_tokens_from_output() {
        let usage = normalize_usage(Some(&json!({
            "input_tokens": 120,
            "output_tokens": 80,
            "total_tokens": 200,
            "output_tokens_details": {
                "reasoning_tokens": 30
            }
        })))
        .expect("usage should be parsed");
        assert_eq!(usage.input, 120);
        assert_eq!(usage.output, 50);
        assert_eq!(usage.total, 200);
    }

    #[test]
    fn normalize_usage_estimates_total_with_raw_output_when_missing_total() {
        let usage = normalize_usage(Some(&json!({
            "prompt_tokens": 20,
            "completion_tokens": 14,
            "completion_tokens_details": {
                "reasoning_tokens": 4
            }
        })))
        .expect("usage should be parsed");
        assert_eq!(usage.input, 20);
        assert_eq!(usage.output, 10);
        assert_eq!(usage.total, 34);
    }
}
