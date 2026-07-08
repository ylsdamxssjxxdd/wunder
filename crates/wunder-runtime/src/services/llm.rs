// LLM 适配：支持 OpenAI 兼容的 Chat Completions 调用。
use crate::config::LlmModelConfig;
use crate::schemas::TokenUsage;
use anyhow::{anyhow, Context, Result};
use futures::StreamExt;
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Value};
use std::future::Future;
use std::time::Duration;
use tracing::warn;

mod context_probe;
mod payload;
mod provider;
mod response;
mod stream_tool;
#[cfg(test)]
use context_probe::normalize_root_url;
pub use context_probe::probe_openai_context_window;
use context_probe::{build_headers, normalize_api_key_token};
use payload::{
    build_responses_input, normalize_chat_tool_definition, normalize_responses_tool_definition,
    sanitize_chat_messages,
};
use provider::{
    build_anthropic_messages_endpoint, build_openai_resource_endpoint, resolve_base_url,
    should_strip_openai_tool_schema,
};
pub use provider::{
    build_model_auth_headers, build_openai_model_resource_endpoint, is_openai_compatible_provider,
    normalize_openai_api_mode, normalize_provider, provider_default_base_url,
    resolve_model_base_url, resolve_openai_api_mode, should_disable_streaming_for_native_tools,
};
#[cfg(test)]
use response::extract_tool_calls;
use response::{
    build_anthropic_messages, extract_responses_output, extract_stream_error_message,
    has_stream_tool_activity, is_false_tool_stop_reason, normalize_usage,
    openai_tool_definition_to_anthropic_tool, parse_anthropic_body, parse_chat_completion_body,
    parse_responses_body,
};
#[cfg(test)]
use stream_tool::merge_stream_delta_field;
use stream_tool::{
    finalize_stream_tool_calls, merge_stream_tool_call_item, resolve_stream_tool_call_arguments,
    resolve_stream_tool_call_name, update_responses_tool_call_arguments,
    update_responses_tool_call_from_item, update_stream_tool_calls_delta,
    update_stream_tool_calls_snapshot, upsert_responses_tool_calls, StreamToolCall,
    StreamToolFieldMode,
};

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
const DEFAULT_VLLM_OMNI_BASE_URL: &str = "http://127.0.0.1:8000/v1";
const DEFAULT_WHISPER_CPP_BASE_URL: &str = "http://127.0.0.1:8080";
const DEFAULT_OLLAMA_BASE_URL: &str = "http://127.0.0.1:11434/v1";
const DEFAULT_LMSTUDIO_BASE_URL: &str = "http://127.0.0.1:1234/v1";
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 8_192;
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
const FINAL_RESPONSE_TOOL_NAMES: [&str; 2] = ["final_response", "最终回复"];
const FINAL_RESPONSE_STREAM_FIELD_NAMES: [&str; 3] = ["content", "answer", "message"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelType {
    Llm,
    Embedding,
    Asr,
    Tts,
    Image,
    Video,
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
        "asr"
        | "stt"
        | "speech_to_text"
        | "speech2text"
        | "audio_transcription"
        | "transcription"
        | "audio_to_text" => ModelType::Asr,
        "tts" | "speech" | "text_to_speech" | "text2speech" | "audio_speech" => ModelType::Tts,
        "image" | "draw" | "drawing" | "text_to_image" | "text2image" | "image_generation" => {
            ModelType::Image
        }
        "video" | "text_to_video" | "text2video" | "video_generation" | "movie" | "animation" => {
            ModelType::Video
        }
        _ => ModelType::Llm,
    }
}

pub fn is_embedding_model(config: &LlmModelConfig) -> bool {
    normalize_model_type(config.model_type.as_deref()) == ModelType::Embedding
}

pub fn is_llm_model(config: &LlmModelConfig) -> bool {
    normalize_model_type(config.model_type.as_deref()) == ModelType::Llm
}

pub fn is_asr_model(config: &LlmModelConfig) -> bool {
    normalize_model_type(config.model_type.as_deref()) == ModelType::Asr
}

pub fn is_tts_model(config: &LlmModelConfig) -> bool {
    normalize_model_type(config.model_type.as_deref()) == ModelType::Tts
}

pub fn is_image_model(config: &LlmModelConfig) -> bool {
    normalize_model_type(config.model_type.as_deref()) == ModelType::Image
}

pub fn is_video_model(config: &LlmModelConfig) -> bool {
    normalize_model_type(config.model_type.as_deref()) == ModelType::Video
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
        "openai_compatible" | "qwen" | "vllm" | "vllm_ascend" | "vllm_omni"
    )
}

fn should_emit_vllm_chat_template_kwargs(config: &LlmModelConfig) -> bool {
    let provider = normalize_provider(config.provider.as_deref());
    matches!(
        provider.as_str(),
        "openai_compatible" | "vllm" | "vllm_ascend" | "vllm_omni"
    )
}

fn should_emit_thinking_token_budget(config: &LlmModelConfig) -> bool {
    is_llm_model(config)
        && !matches!(
            normalize_provider(config.provider.as_deref()).as_str(),
            "anthropic"
        )
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

fn resolved_responses_thinking_token_budget(config: &LlmModelConfig) -> Option<u32> {
    if !should_emit_thinking_token_budget(config) {
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

const EMPTY_LLM_RESPONSE_ERROR: &str =
    "LLM returned empty response without content, reasoning, or tool calls";

fn llm_response_has_payload(content: &str, reasoning: &str, tool_calls: Option<&Value>) -> bool {
    if !content.trim().is_empty() || !reasoning.trim().is_empty() {
        return true;
    }
    tool_calls.is_some_and(tool_call_payload_has_items)
}

#[derive(Debug, Default)]
struct FinalResponseToolPreview {
    displayed: String,
    displayed_tail: String,
}

impl FinalResponseToolPreview {
    fn active(&self) -> bool {
        !self.displayed.is_empty()
    }

    fn sync_from_tool_calls(&mut self, tool_calls: &[StreamToolCall]) -> Option<String> {
        if let Some(next) = extract_final_response_preview_text(tool_calls) {
            if let Some(delta) = visible_text_delta(self.displayed.as_str(), next.as_str()) {
                self.displayed.push_str(delta.as_str());
                self.displayed_tail.clear();
                return Some(delta);
            }
        }
        let tail = extract_final_response_preview_tail_delta(tool_calls, self.displayed.as_str())?;
        let delta = visible_text_delta(self.displayed_tail.as_str(), tail.as_str())?;
        self.displayed_tail.push_str(delta.as_str());
        self.displayed.push_str(delta.as_str());
        Some(delta)
    }
}

fn tool_call_payload_has_items(payload: &Value) -> bool {
    match payload {
        Value::Null => false,
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
        Value::String(text) => !text.trim().is_empty(),
        _ => true,
    }
}

fn sync_visible_final_response_tool_delta(
    combined: &str,
    reasoning: &str,
    preview: &mut FinalResponseToolPreview,
    tool_calls: &[StreamToolCall],
) -> Option<String> {
    if !preview.active() && (!combined.trim().is_empty() || !reasoning.trim().is_empty()) {
        return None;
    }
    preview.sync_from_tool_calls(tool_calls)
}

fn extract_final_response_preview_text(tool_calls: &[StreamToolCall]) -> Option<String> {
    for call in tool_calls {
        let Some(name) = resolve_stream_tool_call_name(call) else {
            continue;
        };
        if !is_final_response_tool_name(name.as_str()) {
            continue;
        }
        if let Some(text) = extract_final_response_argument_text(call) {
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

fn extract_final_response_preview_tail_delta(
    tool_calls: &[StreamToolCall],
    displayed: &str,
) -> Option<String> {
    if displayed.is_empty() {
        return None;
    }
    for call in tool_calls {
        let Some(name) = resolve_stream_tool_call_name(call) else {
            continue;
        };
        if !is_final_response_tool_name(name.as_str()) {
            continue;
        }
        let delta = extract_final_response_argument_tail_fragment(call.arguments_delta.as_str())?;
        if !delta.is_empty() {
            return Some(delta);
        }
    }
    None
}

fn is_final_response_tool_name(name: &str) -> bool {
    let normalized = name.trim();
    FINAL_RESPONSE_TOOL_NAMES
        .iter()
        .any(|candidate| normalized.eq_ignore_ascii_case(candidate))
}

fn extract_final_response_argument_text(call: &StreamToolCall) -> Option<String> {
    let mut candidates: Vec<String> = Vec::new();
    let resolved = resolve_stream_tool_call_arguments(call);
    if !resolved.trim().is_empty() {
        candidates.push(resolved);
    }
    if let Some(snapshot) = call.arguments_snapshot.as_deref() {
        if !snapshot.trim().is_empty() {
            candidates.push(snapshot.to_string());
        }
    }
    if !call.arguments_delta.trim().is_empty() {
        candidates.push(call.arguments_delta.clone());
    }

    let mut best: Option<String> = None;
    for candidate in candidates {
        if let Some(text) = extract_final_response_text_from_arguments(candidate.as_str()) {
            if text.is_empty() {
                continue;
            }
            let replace = best
                .as_ref()
                .map(|current| text.chars().count() > current.chars().count())
                .unwrap_or(true);
            if replace {
                best = Some(text);
            }
        }
    }
    best
}

fn extract_final_response_argument_tail_fragment(raw: &str) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    let trimmed = raw.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return None;
    }
    if FINAL_RESPONSE_STREAM_FIELD_NAMES
        .iter()
        .any(|field| trimmed.contains(&format!("\"{field}\"")))
    {
        return None;
    }
    let decoded = decode_json_string_tail_fragment(raw);
    (!decoded.is_empty()).then_some(decoded)
}

fn decode_json_string_tail_fragment(raw: &str) -> String {
    let mut output = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => break,
            '\\' => match chars.next() {
                Some('"') => output.push('"'),
                Some('\\') => output.push('\\'),
                Some('/') => output.push('/'),
                Some('b') => output.push('\u{0008}'),
                Some('f') => output.push('\u{000c}'),
                Some('n') => output.push('\n'),
                Some('r') => output.push('\r'),
                Some('t') => output.push('\t'),
                Some('u') => {
                    let mut hex = String::new();
                    for _ in 0..4 {
                        let Some(next) = chars.peek().copied() else {
                            break;
                        };
                        if !next.is_ascii_hexdigit() {
                            break;
                        }
                        hex.push(next);
                        chars.next();
                    }
                    if hex.len() == 4 {
                        if let Ok(code) = u32::from_str_radix(hex.as_str(), 16) {
                            if let Some(decoded) = char::from_u32(code) {
                                output.push(decoded);
                            }
                        }
                    }
                }
                Some(other) => output.push(other),
                None => break,
            },
            _ => output.push(ch),
        }
    }
    output
}

fn extract_final_response_text_from_arguments(raw: &str) -> Option<String> {
    let trimmed = trim_empty_object_argument_prefix(raw);
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if let Some(text) = extract_final_response_text_value(&value) {
            return Some(text);
        }
    }
    for field in FINAL_RESPONSE_STREAM_FIELD_NAMES {
        if let Some(text) = extract_partial_json_string_field(trimmed, field) {
            return Some(text);
        }
    }
    None
}

fn trim_empty_object_argument_prefix(raw: &str) -> &str {
    let trimmed = raw.trim();
    trimmed
        .strip_prefix("{}")
        .map(str::trim_start)
        .unwrap_or(trimmed)
}

fn extract_final_response_text_value(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            for field in FINAL_RESPONSE_STREAM_FIELD_NAMES {
                if let Some(inner) = map.get(field) {
                    if let Some(text) = extract_final_response_text_value(inner) {
                        return Some(text);
                    }
                }
            }
            None
        }
        Value::String(text) => Some(text.clone()),
        _ => None,
    }
}

fn extract_partial_json_string_field(raw: &str, field: &str) -> Option<String> {
    let key = format!("\"{field}\"");
    let bytes = raw.as_bytes();
    let mut search_start = 0usize;
    while search_start < raw.len() {
        let relative = raw[search_start..].find(key.as_str())?;
        let key_end = search_start + relative + key.len();
        let mut index = skip_ascii_ws(bytes, key_end);
        if bytes.get(index) != Some(&b':') {
            search_start = key_end;
            continue;
        }
        index = skip_ascii_ws(bytes, index.saturating_add(1));
        if bytes.get(index) != Some(&b'"') {
            search_start = key_end;
            continue;
        }
        return Some(decode_partial_json_string(&raw[index.saturating_add(1)..]));
    }
    None
}

fn skip_ascii_ws(bytes: &[u8], mut index: usize) -> usize {
    while let Some(byte) = bytes.get(index) {
        if !byte.is_ascii_whitespace() {
            break;
        }
        index = index.saturating_add(1);
    }
    index
}

fn decode_partial_json_string(raw: &str) -> String {
    let mut output = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => break,
            '\\' => match chars.next() {
                Some('"') => output.push('"'),
                Some('\\') => output.push('\\'),
                Some('/') => output.push('/'),
                Some('b') => output.push('\u{0008}'),
                Some('f') => output.push('\u{000c}'),
                Some('n') => output.push('\n'),
                Some('r') => output.push('\r'),
                Some('t') => output.push('\t'),
                Some('u') => {
                    let mut hex = String::new();
                    for _ in 0..4 {
                        let Some(next) = chars.peek().copied() else {
                            break;
                        };
                        if !next.is_ascii_hexdigit() {
                            break;
                        }
                        hex.push(next);
                        chars.next();
                    }
                    if hex.len() == 4 {
                        if let Ok(code) = u32::from_str_radix(hex.as_str(), 16) {
                            if let Some(decoded) = char::from_u32(code) {
                                output.push(decoded);
                            }
                        }
                    }
                }
                Some(other) => output.push(other),
                None => break,
            },
            _ => output.push(ch),
        }
    }
    output
}

fn visible_text_delta(current: &str, next: &str) -> Option<String> {
    if next.is_empty() || next == current || current.starts_with(next) {
        return None;
    }
    if current.is_empty() {
        return Some(next.to_string());
    }
    if next.starts_with(current) {
        return Some(next[current.len()..].to_string()).filter(|delta| !delta.is_empty());
    }
    let overlap = text_overlap_len(current, next);
    if overlap == 0 || overlap >= next.len() {
        return None;
    }
    Some(next[overlap..].to_string()).filter(|delta| !delta.is_empty())
}

fn text_overlap_len(current: &str, next: &str) -> usize {
    let limit = current.len().min(next.len()).min(1024);
    let mut size = limit;
    while size > 0 {
        if current.is_char_boundary(current.len() - size)
            && next.is_char_boundary(size)
            && current.ends_with(&next[..size])
        {
            return size;
        }
        size -= 1;
    }
    0
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
        if !llm_response_has_payload(&content, &reasoning, tool_calls.as_ref()) {
            return Err(anyhow!(EMPTY_LLM_RESPONSE_ERROR));
        }
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
            let mut final_response_preview = FinalResponseToolPreview::default();
            let mut saw_done = false;
            while let Some(item) = stream.next().await {
                let bytes = item?;
                let part = String::from_utf8_lossy(&bytes);
                buffer.push_str(&part);

                while let Some(event_block) = take_next_sse_event(&mut buffer) {
                    if process_sse_event_block_with_preview(
                        event_block.as_str(),
                        &mut combined,
                        &mut reasoning_combined,
                        &mut usage,
                        &mut tool_calls_accumulator,
                        &mut final_response_preview,
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
                if process_sse_event_block_with_preview(
                    event_block.as_str(),
                    &mut combined,
                    &mut reasoning_combined,
                    &mut usage,
                    &mut tool_calls_accumulator,
                    &mut final_response_preview,
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
                && process_sse_event_block_with_preview(
                    buffer.as_str(),
                    &mut combined,
                    &mut reasoning_combined,
                    &mut usage,
                    &mut tool_calls_accumulator,
                    &mut final_response_preview,
                    &mut on_delta,
                )
                .await?
            {
                saw_done = true;
            }

            let tool_calls = finalize_stream_tool_calls(&tool_calls_accumulator);
            let stream_payload_empty =
                !llm_response_has_payload(&combined, &reasoning_combined, tool_calls.as_ref());
            if stream_payload_empty {
                let empty_reason = if saw_done {
                    "LLM stream finished with [DONE] but without payload"
                } else {
                    "LLM stream ended without [DONE] and without payload"
                };
                warn!("{empty_reason}, fallback to non-stream request");
                match self.complete_with_tools(messages, tools).await {
                    Ok(fallback) => {
                        if !fallback.content.is_empty() || !fallback.reasoning.is_empty() {
                            on_delta(fallback.content.clone(), fallback.reasoning.clone()).await?;
                        }
                        return Ok(fallback);
                    }
                    Err(err) => {
                        return Err(anyhow!("{empty_reason}; fallback request failed: {err}"));
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
        if let Some(thinking_token_budget) = resolved_responses_thinking_token_budget(&self.config)
        {
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

pub fn build_llm_client(config: &LlmModelConfig, http: Client) -> LlmClient {
    LlmClient::new(http, config.clone())
}

pub fn is_llm_configured(config: &LlmModelConfig) -> bool {
    if crate::services::virtual_llm::is_virtual_replay_provider(config.provider.as_deref()) {
        return true;
    }
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

pub(super) fn extract_stream_text(value: Option<&Value>) -> String {
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

async fn process_sse_event_block_with_preview<F, Fut>(
    block: &str,
    combined: &mut String,
    reasoning_combined: &mut String,
    usage: &mut Option<TokenUsage>,
    tool_calls_accumulator: &mut Vec<StreamToolCall>,
    final_response_preview: &mut FinalResponseToolPreview,
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
                final_response_preview,
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
                    final_response_preview,
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
        final_response_preview,
        on_delta,
    )
    .await
}

#[cfg(test)]
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
    let mut final_response_preview = FinalResponseToolPreview::default();
    process_sse_event_block_with_preview(
        block,
        combined,
        reasoning_combined,
        usage,
        tool_calls_accumulator,
        &mut final_response_preview,
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
    final_response_preview: &mut FinalResponseToolPreview,
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
                    final_response_preview,
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
                    final_response_preview,
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
            update_stream_tool_calls_delta(tool_calls_accumulator, &delta);
            if let Some(message) = choice.and_then(|value| value.get("message")) {
                update_stream_tool_calls_snapshot(tool_calls_accumulator, message);
            }
            if let Some(choice_payload) = choice {
                if choice_payload.get("message").is_none() {
                    update_stream_tool_calls_snapshot(tool_calls_accumulator, choice_payload);
                }
            }
            if let Some(payload_tool_calls) = payload.get("tool_calls") {
                update_stream_tool_calls_snapshot(tool_calls_accumulator, payload_tool_calls);
            }
            if let Some(payload_function_call) = payload.get("function_call") {
                update_stream_tool_calls_snapshot(tool_calls_accumulator, payload_function_call);
            }
            let preview_delta = if content_delta.is_empty() && reasoning_delta.is_empty() {
                sync_visible_final_response_tool_delta(
                    combined,
                    reasoning_combined,
                    final_response_preview,
                    tool_calls_accumulator,
                )
            } else {
                None
            };
            if !content_delta.is_empty() {
                combined.push_str(content_delta.as_str());
            }
            if !reasoning_delta.is_empty() {
                reasoning_combined.push_str(reasoning_delta.as_str());
            }
            if !content_delta.is_empty() || !reasoning_delta.is_empty() {
                on_delta(content_delta, reasoning_delta).await?;
            } else if let Some(preview_delta) = preview_delta {
                on_delta(preview_delta, String::new()).await?;
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
    final_response_preview: &mut FinalResponseToolPreview,
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
                        merge_stream_tool_call_item(
                            tool_calls_accumulator,
                            &call,
                            StreamToolFieldMode::Snapshot,
                        );
                        if let Some(preview_delta) = sync_visible_final_response_tool_delta(
                            combined,
                            reasoning_combined,
                            final_response_preview,
                            tool_calls_accumulator,
                        ) {
                            on_delta(preview_delta, String::new()).await?;
                        } else {
                            on_delta(String::new(), String::new()).await?;
                        }
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
                            merge_stream_tool_call_item(
                                tool_calls_accumulator,
                                &call,
                                StreamToolFieldMode::Delta,
                            );
                            if let Some(slot) = tool_calls_accumulator.get_mut(index) {
                                if let Some(snapshot) = slot.arguments_snapshot.as_mut() {
                                    if snapshot == "{}" || snapshot.starts_with("{}") {
                                        snapshot.push_str(partial);
                                    }
                                }
                            }
                            if let Some(preview_delta) = sync_visible_final_response_tool_delta(
                                combined,
                                reasoning_combined,
                                final_response_preview,
                                tool_calls_accumulator,
                            ) {
                                on_delta(preview_delta, String::new()).await?;
                            } else {
                                on_delta(String::new(), String::new()).await?;
                            }
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
                if let Some(preview_delta) = sync_visible_final_response_tool_delta(
                    combined,
                    reasoning_combined,
                    final_response_preview,
                    tool_calls_accumulator,
                ) {
                    on_delta(preview_delta, String::new()).await?;
                } else if combined.is_empty() && reasoning_combined.is_empty() {
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
    final_response_preview: &mut FinalResponseToolPreview,
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
                if let Some(preview_delta) = sync_visible_final_response_tool_delta(
                    combined,
                    reasoning_combined,
                    final_response_preview,
                    tool_calls_accumulator,
                ) {
                    on_delta(preview_delta, String::new()).await?;
                } else {
                    on_delta(String::new(), String::new()).await?;
                }
            }
        }
        "response.function_call_arguments.delta" => {
            update_responses_tool_call_arguments(tool_calls_accumulator, payload);
            if let Some(preview_delta) = sync_visible_final_response_tool_delta(
                combined,
                reasoning_combined,
                final_response_preview,
                tool_calls_accumulator,
            ) {
                on_delta(preview_delta, String::new()).await?;
            } else {
                on_delta(String::new(), String::new()).await?;
            }
        }
        "response.function_call_arguments.done" => {
            update_responses_tool_call_arguments(tool_calls_accumulator, payload);
            if let Some(preview_delta) = sync_visible_final_response_tool_delta(
                combined,
                reasoning_combined,
                final_response_preview,
                tool_calls_accumulator,
            ) {
                on_delta(preview_delta, String::new()).await?;
            } else {
                on_delta(String::new(), String::new()).await?;
            }
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
                    if let Some(preview_delta) = sync_visible_final_response_tool_delta(
                        combined,
                        reasoning_combined,
                        final_response_preview,
                        tool_calls_accumulator,
                    ) {
                        on_delta(preview_delta, String::new()).await?;
                    } else if combined.is_empty() && reasoning_combined.is_empty() {
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
            if let Some(preview_delta) = sync_visible_final_response_tool_delta(
                combined,
                reasoning_combined,
                final_response_preview,
                tool_calls_accumulator,
            ) {
                on_delta(preview_delta, String::new()).await?;
            } else if combined.is_empty() && reasoning_combined.is_empty() {
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
            ..Default::default()
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
    fn virtual_replay_is_configured_without_model_or_base_url() {
        let config = LlmModelConfig {
            provider: Some("virtual_replay".to_string()),
            model: None,
            base_url: None,
            api_key: None,
            ..Default::default()
        };

        assert!(is_llm_configured(&config));
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
            ..Default::default()
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
    async fn process_sse_event_block_merges_openai_delta_and_message_snapshot_without_duplication()
    {
        let mut combined = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tool_calls = Vec::new();
        let mut on_delta = |_content: String, _reasoning: String| async { Ok(()) };

        let done = process_sse_event_block(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"read_\",\"arguments\":\"{\\\"path\\\":\\\"\"}}]}}]}",
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut on_delta,
        )
        .await
        .expect("process first delta");
        assert!(!done);

        let done = process_sse_event_block(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"file\",\"arguments\":\"notes.txt\\\"}\"}}]},\"message\":{\"tool_calls\":[{\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"notes.txt\\\"}\"}}]}}]}",
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut on_delta,
        )
        .await
        .expect("process delta plus snapshot");
        assert!(!done);

        let finalized = finalize_stream_tool_calls(&tool_calls).expect("tool calls should exist");
        assert_eq!(finalized.as_array().map(Vec::len), Some(1));
        assert_eq!(finalized[0]["id"], "call_1");
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
    fn update_stream_tool_calls_merges_delta_then_snapshot_without_duplication() {
        let mut acc = Vec::new();
        update_stream_tool_calls_delta(
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
        update_stream_tool_calls_delta(
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
        update_stream_tool_calls_snapshot(
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

        assert_eq!(acc.len(), 1);
        assert_eq!(
            resolve_stream_tool_call_name(&acc[0]).as_deref(),
            Some("read_file")
        );
        assert_eq!(
            resolve_stream_tool_call_arguments(&acc[0]),
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
    fn merge_stream_delta_field_preserves_whitespace_between_fragments() {
        let mut merged = String::new();
        merge_stream_delta_field(&mut merged, "import ");
        merge_stream_delta_field(&mut merged, "json\nfrom datetime import ");
        merge_stream_delta_field(&mut merged, "datetime, timedelta");

        assert_eq!(
            merged,
            "import json\nfrom datetime import datetime, timedelta"
        );
    }

    #[test]
    fn merge_stream_delta_field_preserves_whitespace_only_fragment() {
        let mut merged = String::new();
        for fragment in ["which", " ", "python3"] {
            merge_stream_delta_field(&mut merged, fragment);
        }

        assert_eq!(merged, "which python3");
    }

    #[test]
    fn update_stream_tool_calls_preserves_whitespace_in_arguments() {
        let mut acc = Vec::new();
        update_stream_tool_calls_delta(
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
        update_stream_tool_calls_delta(
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
        update_stream_tool_calls_delta(
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
    fn update_stream_tool_calls_preserves_incremental_fragments() {
        let mut acc = Vec::new();
        for (index, fragment) in [
            "{",
            "\"content\":\"",
            "for",
            " i",
            " in",
            " range",
            "(12):",
            "\\n  ",
            "  print",
            "(i)",
            "\"",
            ",\"filename\":\"script.py\"",
            "}",
        ]
        .into_iter()
        .enumerate()
        {
            let mut function = json!({ "arguments": fragment });
            if index == 0 {
                function["name"] = json!("programmatic_tool_call");
            }
            update_stream_tool_calls_delta(
                &mut acc,
                &json!({
                    "tool_calls": [{
                        "index": 0,
                        "function": function,
                    }]
                }),
            );
        }

        let finalized = finalize_stream_tool_calls(&acc).expect("tool calls should exist");
        assert_eq!(finalized[0]["function"]["name"], "programmatic_tool_call");
        assert_eq!(
            finalized[0]["function"]["arguments"],
            "{\"content\":\"for i in range(12):\\n    print(i)\",\"filename\":\"script.py\"}"
        );
    }

    #[test]
    fn update_stream_tool_calls_preserves_whitespace_only_argument_fragments() {
        let mut acc = Vec::new();
        update_stream_tool_calls_delta(
            &mut acc,
            &json!({
                "tool_calls": [{
                    "index": 0,
                    "function": {
                        "name": "execute_command",
                        "arguments": "{\"content\":\"which"
                    }
                }]
            }),
        );
        update_stream_tool_calls_delta(
            &mut acc,
            &json!({
                "tool_calls": [{
                    "index": 0,
                    "function": {
                        "arguments": " "
                    }
                }]
            }),
        );
        update_stream_tool_calls_delta(
            &mut acc,
            &json!({
                "tool_calls": [{
                    "index": 0,
                    "function": {
                        "arguments": "python3\"}"
                    }
                }]
            }),
        );

        let finalized = finalize_stream_tool_calls(&acc).expect("tool calls should exist");
        assert_eq!(finalized[0]["function"]["name"], "execute_command");
        assert_eq!(
            finalized[0]["function"]["arguments"],
            "{\"content\":\"which python3\"}"
        );
    }

    #[test]
    fn update_stream_tool_calls_ignores_assistant_name_metadata() {
        let mut acc = Vec::new();
        update_stream_tool_calls_delta(
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
    fn update_responses_tool_call_from_item_keeps_latest_custom_tool_snapshot() {
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

        assert_eq!(acc.len(), 1);
        assert_eq!(
            resolve_stream_tool_call_name(&acc[0]).as_deref(),
            Some("apply_patch")
        );
        assert_eq!(
            resolve_stream_tool_call_arguments(&acc[0]),
            "*** Begin Patch\n*** Add File: hello.txt\n+hello world\n*** End Patch"
        );
    }

    #[test]
    fn resolve_stream_tool_call_arguments_merges_snapshot_with_delta_data() {
        let call = StreamToolCall {
            arguments_delta: "{\"content\":\"hello\"}".to_string(),
            arguments_snapshot: Some("{\"raw\":\"tail\",\"path\":\"draw_heart.py\"}".to_string()),
            ..Default::default()
        };
        assert_eq!(
            serde_json::from_str::<Value>(&resolve_stream_tool_call_arguments(&call))
                .expect("valid json"),
            json!({
                "content": "hello",
                "raw": "tail",
                "path": "draw_heart.py"
            })
        );
    }

    #[test]
    fn resolve_stream_tool_call_arguments_prefers_snapshot_for_complete_object() {
        let call = StreamToolCall {
            arguments_delta: "{}".to_string(),
            arguments_snapshot: Some(
                "{\"filename\":\"draw_heart.py\",\"content\":\"print('ok')\"}".to_string(),
            ),
            ..Default::default()
        };
        assert_eq!(
            serde_json::from_str::<Value>(&resolve_stream_tool_call_arguments(&call))
                .expect("valid json"),
            json!({
                "filename": "draw_heart.py",
                "content": "print('ok')"
            })
        );
    }

    #[test]
    fn merge_stream_tool_call_item_replaces_empty_json_seed_from_anthropic_tool_use() {
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
            StreamToolFieldMode::Snapshot,
        );
        merge_stream_tool_call_item(
            &mut acc,
            &json!({
                "index": 0,
                "function": {
                    "arguments": "{\"filename\":\"draw_heart.py\",\"content\":\"print('ok')\"}"
                }
            }),
            StreamToolFieldMode::Snapshot,
        );

        assert_eq!(acc.len(), 1);
        assert_eq!(
            resolve_stream_tool_call_name(&acc[0]).as_deref(),
            Some("ptc")
        );
        assert_eq!(
            resolve_stream_tool_call_arguments(&acc[0]),
            "{\"filename\":\"draw_heart.py\",\"content\":\"print('ok')\"}"
        );
    }

    #[test]
    fn merge_stream_tool_call_item_replaces_empty_json_seed_and_merges_incremental_fragments() {
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
            StreamToolFieldMode::Snapshot,
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
                StreamToolFieldMode::Delta,
            );
        }

        assert_eq!(acc.len(), 1);
        assert_eq!(
            resolve_stream_tool_call_name(&acc[0]).as_deref(),
            Some("ptc")
        );
        assert_eq!(
            resolve_stream_tool_call_arguments(&acc[0]),
            "{\"filename\": \"demo.py\", \"content\": \"print(1)\"}"
        );
    }

    #[tokio::test]
    async fn process_anthropic_stream_tool_use_appends_empty_input_seed() {
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

        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].name_snapshot.as_deref(), Some("ptc"));
        assert_eq!(
            tool_calls[0].arguments_snapshot.as_deref(),
            Some("{}{\"filename\":\"draw_heart.py\",\"content\":\"print('ok')\"}")
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
    async fn process_sse_event_block_streams_final_response_tool_arguments_as_visible_delta() {
        let mut combined = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tool_calls = Vec::new();
        let mut final_response_preview = FinalResponseToolPreview::default();
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

        let done = process_sse_event_block_with_preview(
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_final","type":"function","function":{"name":"final_response","arguments":"{\"content\":\"Hel"}}]}}]}"#,
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut final_response_preview,
            &mut on_delta,
        )
        .await
        .expect("process first final response argument delta");
        assert!(!done);

        let done = process_sse_event_block_with_preview(
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_final","type":"function","function":{"arguments":"lo\"}"}}]}}]}"#,
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut final_response_preview,
            &mut on_delta,
        )
        .await
        .expect("process second final response argument delta");
        assert!(!done);

        assert!(combined.is_empty());
        assert!(reasoning.is_empty());
        assert_eq!(
            callbacks.lock().expect("lock callbacks").as_slice(),
            &[
                ("Hel".to_string(), String::new()),
                ("lo".to_string(), String::new())
            ]
        );
        let finalized = finalize_stream_tool_calls(&tool_calls).expect("tool calls should exist");
        assert_eq!(finalized[0]["function"]["name"], "final_response");
        assert_eq!(
            serde_json::from_str::<Value>(
                finalized[0]["function"]["arguments"].as_str().unwrap_or("")
            )
            .expect("final response args"),
            json!({ "content": "Hello" })
        );
    }

    #[tokio::test]
    async fn process_sse_event_block_prefers_cumulative_final_response_arguments() {
        let mut combined = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tool_calls = Vec::new();
        let mut final_response_preview = FinalResponseToolPreview::default();
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

        process_sse_event_block_with_preview(
            r#"data: {"choices":[{"message":{"tool_calls":[{"index":0,"id":"call_final","type":"function","function":{"name":"final_response","arguments":"{\"content\":\"Hel\"}"}}]}}]}"#,
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut final_response_preview,
            &mut on_delta,
        )
        .await
        .expect("process initial final response snapshot");

        process_sse_event_block_with_preview(
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_final","type":"function","function":{"arguments":"lo\"}"}}]}}]}"#,
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut final_response_preview,
            &mut on_delta,
        )
        .await
        .expect("process cumulative final response delta");

        assert!(combined.is_empty());
        assert_eq!(
            callbacks.lock().expect("lock callbacks").as_slice(),
            &[
                ("Hel".to_string(), String::new()),
                ("lo".to_string(), String::new())
            ]
        );
    }

    #[tokio::test]
    async fn process_sse_event_block_streams_responses_final_response_arguments() {
        let mut combined = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tool_calls = Vec::new();
        let mut final_response_preview = FinalResponseToolPreview::default();
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

        process_sse_event_block_with_preview(
            r#"data: {"type":"response.output_item.added","item":{"type":"function_call","id":"item_final","call_id":"call_final","name":"final_response","arguments":"{}"}}"#,
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut final_response_preview,
            &mut on_delta,
        )
        .await
        .expect("process responses final tool start");

        process_sse_event_block_with_preview(
            r#"data: {"type":"response.function_call_arguments.delta","item_id":"item_final","call_id":"call_final","delta":"{\"answer\":\"H"}"#,
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut final_response_preview,
            &mut on_delta,
        )
        .await
        .expect("process first responses final argument delta");

        process_sse_event_block_with_preview(
            r#"data: {"type":"response.function_call_arguments.delta","item_id":"item_final","call_id":"call_final","delta":"i\"}"}"#,
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut final_response_preview,
            &mut on_delta,
        )
        .await
        .expect("process second responses final argument delta");

        assert!(combined.is_empty());
        assert_eq!(
            callbacks.lock().expect("lock callbacks").as_slice(),
            &[
                (String::new(), String::new()),
                ("H".to_string(), String::new()),
                ("i".to_string(), String::new())
            ]
        );
        let finalized = finalize_stream_tool_calls(&tool_calls).expect("tool calls should exist");
        assert_eq!(finalized[0]["function"]["name"], "final_response");
        assert_eq!(
            serde_json::from_str::<Value>(
                finalized[0]["function"]["arguments"].as_str().unwrap_or("")
            )
            .expect("final response args"),
            json!({ "answer": "Hi" })
        );
    }

    #[tokio::test]
    async fn process_sse_event_block_streams_anthropic_final_response_input_json() {
        let mut combined = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tool_calls = Vec::new();
        let mut final_response_preview = FinalResponseToolPreview::default();
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

        process_sse_event_block_with_preview(
            r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"call_final","name":"final_response","input":{}}}"#,
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut final_response_preview,
            &mut on_delta,
        )
        .await
        .expect("process anthropic final tool start");

        process_sse_event_block_with_preview(
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"message\":\"Hel"}}"#,
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut final_response_preview,
            &mut on_delta,
        )
        .await
        .expect("process first anthropic final input delta");

        process_sse_event_block_with_preview(
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"lo\"}"}}"#,
            &mut combined,
            &mut reasoning,
            &mut usage,
            &mut tool_calls,
            &mut final_response_preview,
            &mut on_delta,
        )
        .await
        .expect("process second anthropic final input delta");

        assert!(combined.is_empty());
        assert_eq!(
            callbacks.lock().expect("lock callbacks").as_slice(),
            &[
                (String::new(), String::new()),
                ("Hel".to_string(), String::new()),
                ("lo".to_string(), String::new())
            ]
        );
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
            ..Default::default()
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
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
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
        let app = Router::new()
            .route(
                "/v1/chat/completions",
                post(
                    |State(state): State<AppState>, Json(payload): Json<Value>| async move {
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
            ..Default::default()
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
    async fn complete_with_tools_rejects_empty_success_body() {
        use axum::http::StatusCode;
        use axum::routing::post;
        use axum::{Json, Router};
        use tokio::net::TcpListener;

        let app = Router::new().route(
            "/v1/chat/completions",
            post(|| async {
                (
                    StatusCode::OK,
                    Json(json!({
                        "choices": [
                            {
                                "message": {
                                    "content": ""
                                }
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 12,
                            "completion_tokens": 0,
                            "total_tokens": 12
                        }
                    })),
                )
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
            api_mode: Some("chat_completions".to_string()),
            base_url: Some(format!("http://{addr}/v1")),
            api_key: Some("test-key".to_string()),
            model: Some("test-model".to_string()),
            stream: Some(false),
            stream_include_usage: Some(false),
            model_type: Some("llm".to_string()),
            ..Default::default()
        };
        let client = LlmClient::new(Client::new(), config);
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: Value::String("hello".to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        }];

        let err = client
            .complete_with_tools(&messages, None)
            .await
            .expect_err("empty success body should be rejected");

        assert!(err.to_string().contains(EMPTY_LLM_RESPONSE_ERROR));
    }

    #[tokio::test]
    async fn stream_complete_falls_back_and_rejects_when_fallback_is_empty() {
        use axum::extract::State;
        use axum::http::StatusCode;
        use axum::routing::post;
        use axum::{Json, Router};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
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
        let app = Router::new()
            .route(
                "/v1/chat/completions",
                post(
                    |State(state): State<AppState>, Json(payload): Json<Value>| async move {
                        if payload.get("stream").and_then(Value::as_bool) == Some(true) {
                            state.stream_calls.fetch_add(1, Ordering::SeqCst);
                            return (StatusCode::OK, "data: [DONE]\n\n".to_string());
                        }
                        state.non_stream_calls.fetch_add(1, Ordering::SeqCst);
                        (
                            StatusCode::OK,
                            serde_json::to_string(&json!({
                                "choices": [
                                    {
                                        "message": {
                                            "content": ""
                                        }
                                    }
                                ],
                                "usage": {
                                    "prompt_tokens": 12,
                                    "completion_tokens": 0,
                                    "total_tokens": 12
                                }
                            }))
                            .expect("serialize response"),
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
            stream: Some(true),
            stream_include_usage: Some(true),
            model_type: Some("llm".to_string()),
            ..Default::default()
        };
        let client = LlmClient::new(Client::new(), config);
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: Value::String("hello".to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        }];

        let err = client
            .stream_complete_with_callback(&messages, |_delta, _reasoning| async { Ok(()) })
            .await
            .expect_err("empty stream and empty fallback should be rejected");

        let message = err.to_string();
        assert!(message.contains("without payload"));
        assert!(message.contains(EMPTY_LLM_RESPONSE_ERROR));
        assert_eq!(state.stream_calls.load(Ordering::SeqCst), 1);
        assert_eq!(state.non_stream_calls.load(Ordering::SeqCst), 1);
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
    fn keeps_native_tool_streaming_for_private_openai_compatible_backends() {
        let config = LlmModelConfig {
            provider: Some("openai_compatible".to_string()),
            base_url: Some("http://192.168.1.88:8000/v1".to_string()),
            ..Default::default()
        };

        assert!(!should_disable_streaming_for_native_tools(&config, true));
        assert!(!should_disable_streaming_for_native_tools(&config, false));
    }

    #[test]
    fn keeps_native_tool_streaming_for_public_openai_compatible_backends() {
        let config = LlmModelConfig {
            provider: Some("openai_compatible".to_string()),
            base_url: Some("https://api.openrouter.ai/v1".to_string()),
            ..Default::default()
        };

        assert!(!should_disable_streaming_for_native_tools(&config, true));
    }

    #[test]
    fn keeps_native_tool_streaming_for_vllm_ascend_provider_until_retry_recovery() {
        let config = LlmModelConfig {
            provider: Some("vllm_ascend".to_string()),
            base_url: Some("http://10.0.0.8:8000/v1".to_string()),
            ..Default::default()
        };

        assert!(!should_disable_streaming_for_native_tools(&config, true));
    }

    #[test]
    fn keeps_native_tool_streaming_for_official_openai_provider() {
        let config = LlmModelConfig {
            provider: Some("openai".to_string()),
            base_url: Some("https://api.openai.com/v1".to_string()),
            ..Default::default()
        };

        assert!(!should_disable_streaming_for_native_tools(&config, true));
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
            ..Default::default()
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
            ..Default::default()
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

        assert_eq!(payload["max_tokens"], 8_192);
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
            ..Default::default()
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
        assert_eq!(payload["max_tokens"], 8_192);
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
            ..Default::default()
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
            ..Default::default()
        };
        assert_eq!(resolve_openai_api_mode(&config), OpenAiApiMode::Responses);
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
