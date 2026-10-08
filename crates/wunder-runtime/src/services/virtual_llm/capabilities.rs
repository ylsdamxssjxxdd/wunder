//! Request-time provider limits, shared by chat/replay and throughput simulation.
use crate::{config::LlmModelConfig, token_utils};
use serde_json::{json, Value};
use wunder_core::virtual_model::VirtualModelOptions;

pub const DEFAULT_CONTEXT: u32 = 131_072;
pub const DEFAULT_OUTPUT: u32 = 4096;

#[derive(Debug)]
pub struct VirtualModelError {
    pub code: &'static str,
    pub param: &'static str,
    pub message: String,
}

impl VirtualModelError {
    pub(super) fn new(code: &'static str, param: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            param,
            message: message.into(),
        }
    }

    pub fn response(&self) -> Value {
        json!({"status":400,"error":{"type":"invalid_request_error","code":self.code,"param":self.param,"message":self.message}})
    }
}

impl std::fmt::Display for VirtualModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Simulated model API HTTP 400: {}", self.response())
    }
}
impl std::error::Error for VirtualModelError {}

pub fn validate_config(model: &LlmModelConfig) -> Result<(), VirtualModelError> {
    let defaults = VirtualModelOptions::default();
    let options = model.simulation.as_ref().unwrap_or(&defaults);
    if model.max_context == Some(0)
        || model.max_output == Some(0)
        || options.image_tokens == 0
        || options.audio_tokens == 0
    {
        return Err(VirtualModelError::new(
            "invalid_simulation_config",
            "simulation",
            "Context, output and media token limits must be positive.",
        ));
    }
    Ok(())
}

fn content_tokens(
    content: &Value,
    model: &LlmModelConfig,
    options: &VirtualModelOptions,
) -> Result<u64, VirtualModelError> {
    if let Some(parts) = content.as_array() {
        return parts.iter().try_fold(0u64, |total, part| {
            Ok(total.saturating_add(content_tokens(part, model, options)?))
        });
    }
    match content.get("type").and_then(Value::as_str) {
        Some("image_url" | "input_image" | "image") => {
            if model.support_vision != Some(true) {
                return Err(VirtualModelError::new(
                    "unsupported_image",
                    "messages",
                    "This model does not support image input.",
                ));
            }
            Ok(u64::from(options.image_tokens))
        }
        Some("input_audio" | "audio" | "audio_url") => {
            if model.support_hearing != Some(true) {
                return Err(VirtualModelError::new(
                    "unsupported_audio",
                    "messages",
                    "This model does not support audio input.",
                ));
            }
            Ok(u64::from(options.audio_tokens))
        }
        Some("video" | "video_url" | "input_video" | "file" | "input_file") => {
            Err(VirtualModelError::new(
                "unsupported_content",
                "messages",
                "This simulator accepts text, image and audio content only.",
            ))
        }
        _ => Ok(token_utils::estimate_content_tokens(content).max(0) as u64),
    }
}

pub fn request_input_tokens(
    model: &LlmModelConfig,
    messages: &[Value],
    tools: Option<&[Value]>,
) -> Result<u64, VirtualModelError> {
    let defaults = VirtualModelOptions::default();
    let options = model.simulation.as_ref().unwrap_or(&defaults);
    let mut total = 0u64;
    for message in messages {
        let content = message.get("content").unwrap_or(&Value::Null);
        // Replace only the media estimate, preserving reasoning, tool calls and message overhead.
        let metadata = message
            .as_object()
            .map(|map| {
                Value::Object(
                    map.iter()
                        .filter(|(key, _)| key.as_str() != "content")
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect(),
                )
            })
            .unwrap_or(Value::Null);
        let overhead = token_utils::estimate_message_tokens(&metadata).max(0) as u64;
        total = total
            .saturating_add(overhead)
            .saturating_add(content_tokens(content, model, options)?);
    }
    if let Some(tools) = tools.filter(|items| !items.is_empty()) {
        if !options.support_tools {
            return Err(VirtualModelError::new(
                "unsupported_tools",
                "tools",
                "This model does not support tool calling.",
            ));
        }
        // Tool schemas consume the same context and prefill budget as messages.
        total = total.saturating_add(
            serde_json::to_vec(tools)
                .unwrap_or_default()
                .len()
                .div_ceil(4) as u64,
        );
    }
    Ok(total)
}

pub fn validate_request(
    model: &LlmModelConfig,
    messages: &[Value],
    tools: Option<&[Value]>,
    output_tokens: u32,
) -> Result<u64, VirtualModelError> {
    validate_config(model)?;
    let max_output = model.max_output.unwrap_or(DEFAULT_OUTPUT);
    if output_tokens == 0 || output_tokens > max_output {
        return Err(VirtualModelError::new("max_tokens_exceeded", "max_tokens", format!("Requested output is {output_tokens} tokens; maximum output is {max_output} tokens.")));
    }
    let input = request_input_tokens(model, messages, tools)?;
    let context = model.max_context.unwrap_or(DEFAULT_CONTEXT);
    if input.saturating_add(u64::from(output_tokens)) > u64::from(context) {
        return Err(VirtualModelError::new("context_length_exceeded", "messages", format!("This model's maximum context length is {context} tokens, but you requested {} tokens ({input} input + {output_tokens} output).", input.saturating_add(u64::from(output_tokens)))));
    }
    Ok(input)
}

pub fn reasoning_budget(model: &LlmModelConfig, output: u32) -> u32 {
    if model
        .simulation
        .as_ref()
        .is_some_and(|options| !options.support_reasoning)
        || matches!(
            model.reasoning_effort.as_deref(),
            Some("none" | "off" | "disabled")
        )
    {
        return 0;
    }
    model
        .thinking_token_budget
        .unwrap_or(output / 4)
        .min(output.saturating_sub(1))
}
