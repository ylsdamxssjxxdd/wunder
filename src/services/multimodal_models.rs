use crate::config::{Config, LlmModelConfig};
use crate::llm::{
    build_model_auth_headers, build_openai_model_resource_endpoint, is_image_model, is_tts_model,
    resolve_model_base_url,
};
use anyhow::{anyhow, Context, Result};
use bytes::Bytes;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;

const AUDIO_SPEECH_RESOURCE: &str = "audio/speech";
const IMAGES_GENERATIONS_RESOURCE: &str = "images/generations";
const DEFAULT_TTS_RESPONSE_FORMAT: &str = "wav";
const DEFAULT_TTS_TIMEOUT_S: u64 = 120;

#[derive(Debug, Clone)]
pub struct SpeechSynthesisRequest {
    pub text: String,
    pub model_name: Option<String>,
    pub voice: Option<String>,
    pub instructions: Option<String>,
    pub response_format: Option<String>,
    pub speed: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct SpeechSynthesisResponse {
    pub bytes: Bytes,
    pub content_type: String,
}

pub async fn synthesize_speech(
    config: &Config,
    request: SpeechSynthesisRequest,
) -> Result<SpeechSynthesisResponse> {
    let (_name, model_config) = resolve_tts_model(config, request.model_name.as_deref())
        .ok_or_else(|| anyhow!("tts model is not configured"))?;
    synthesize_speech_with_model(&model_config, &request).await
}

pub fn list_tts_model_names(config: &Config) -> Vec<String> {
    let mut names = config
        .llm
        .models
        .iter()
        .filter(|(_, model)| is_tts_model(model))
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    names.sort();
    names
}

pub fn list_image_model_names(config: &Config) -> Vec<String> {
    let mut names = config
        .llm
        .models
        .iter()
        .filter(|(_, model)| is_image_model(model))
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    names.sort();
    names
}

pub fn resolve_tts_model(
    config: &Config,
    requested_name: Option<&str>,
) -> Option<(String, LlmModelConfig)> {
    resolve_model_by_type(
        config,
        requested_name,
        config.llm.default_tts.as_deref(),
        is_tts_model,
    )
}

pub fn resolve_image_model(
    config: &Config,
    requested_name: Option<&str>,
) -> Option<(String, LlmModelConfig)> {
    resolve_model_by_type(
        config,
        requested_name,
        config.llm.default_image.as_deref(),
        is_image_model,
    )
}

fn resolve_model_by_type(
    config: &Config,
    requested_name: Option<&str>,
    default_name: Option<&str>,
    predicate: fn(&LlmModelConfig) -> bool,
) -> Option<(String, LlmModelConfig)> {
    let requested = requested_name
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(name) = requested {
        if let Some(model) = config.llm.models.get(name).filter(|model| predicate(model)) {
            return Some((name.to_string(), model.clone()));
        }
        return None;
    }

    if let Some(name) = default_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Some(model) = config.llm.models.get(name).filter(|model| predicate(model)) {
            return Some((name.to_string(), model.clone()));
        }
    }

    config.llm.models.iter().find_map(|(name, model)| {
        if predicate(model) {
            Some((name.clone(), model.clone()))
        } else {
            None
        }
    })
}

async fn synthesize_speech_with_model(
    config: &LlmModelConfig,
    request: &SpeechSynthesisRequest,
) -> Result<SpeechSynthesisResponse> {
    let base_url =
        resolve_model_base_url(config).ok_or_else(|| anyhow!("tts base_url is required"))?;
    let endpoint = build_openai_model_resource_endpoint(&base_url, AUDIO_SPEECH_RESOURCE)
        .ok_or_else(|| anyhow!("tts base_url is invalid"))?;
    let model = config
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("tts model is required"))?;
    let response_format = normalize_tts_response_format(
        request
            .response_format
            .as_deref()
            .or(config.tts_response_format.as_deref()),
    );
    let timeout_s = config.timeout_s.unwrap_or(DEFAULT_TTS_TIMEOUT_S).max(10);
    let client = Client::builder()
        .timeout(Duration::from_secs(timeout_s))
        .build()?;
    let payload = build_speech_payload(config, model, request, &response_format);
    let response = client
        .post(endpoint)
        .headers(build_model_auth_headers(
            config.api_key.as_deref().unwrap_or(""),
        ))
        .json(&payload)
        .send()
        .await
        .context("send tts request")?;
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| tts_mime_type(&response_format).to_string());
    let bytes = response.bytes().await.context("read tts response body")?;
    if !status.is_success() {
        let detail = String::from_utf8_lossy(&bytes);
        return Err(anyhow!(
            "tts request failed: {status} {}",
            truncate_for_error(&detail, 1024)
        ));
    }
    if bytes.is_empty() {
        return Err(anyhow!("tts response is empty"));
    }
    Ok(SpeechSynthesisResponse {
        bytes,
        content_type,
    })
}

fn build_speech_payload(
    config: &LlmModelConfig,
    model: &str,
    request: &SpeechSynthesisRequest,
    response_format: &str,
) -> Value {
    let mut payload = json!({
        "model": model,
        "input": request.text,
        "response_format": response_format,
        "stream": false,
    });
    if let Some(map) = payload.as_object_mut() {
        if let Some(voice) = request
            .voice
            .as_deref()
            .or(config.tts_voice.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            map.insert("voice".to_string(), json!(voice));
        }
        if let Some(instructions) = request
            .instructions
            .as_deref()
            .or(config.tts_instructions.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            map.insert("instructions".to_string(), json!(instructions));
        }
        let speed = request.speed.or(config.tts_speed);
        if let Some(speed) = speed.filter(|value| value.is_finite() && *value > 0.0) {
            map.insert("speed".to_string(), json!(speed.clamp(0.25, 4.0)));
        }
    }
    payload
}

fn normalize_tts_response_format(value: Option<&str>) -> String {
    match value
        .unwrap_or(DEFAULT_TTS_RESPONSE_FORMAT)
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "pcm" => "pcm",
        "flac" => "flac",
        "mp3" => "mp3",
        "aac" => "aac",
        "opus" => "opus",
        _ => DEFAULT_TTS_RESPONSE_FORMAT,
    }
    .to_string()
}

fn tts_mime_type(format: &str) -> &'static str {
    match format {
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "aac" => "audio/aac",
        "opus" => "audio/opus",
        "pcm" => "audio/L16",
        _ => "audio/wav",
    }
}

fn truncate_for_error(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    let mut output = value.chars().take(limit).collect::<String>();
    output.push_str("...");
    output
}

#[allow(dead_code)]
pub fn image_generation_resource_path() -> &'static str {
    IMAGES_GENERATIONS_RESOURCE
}

#[cfg(test)]
mod tests {
    use super::{
        list_image_model_names, list_tts_model_names, resolve_image_model, resolve_tts_model,
    };
    use crate::config::{Config, LlmModelConfig};

    fn model(model_type: &str) -> LlmModelConfig {
        LlmModelConfig {
            enable: Some(true),
            provider: Some("openai_compatible".to_string()),
            api_mode: None,
            base_url: Some("http://127.0.0.1:8000/v1".to_string()),
            api_key: None,
            model: Some(format!("{model_type}-model")),
            temperature: None,
            timeout_s: None,
            max_rounds: None,
            max_context: None,
            max_output: None,
            thinking_token_budget: None,
            support_vision: None,
            support_hearing: None,
            stream: None,
            stream_include_usage: None,
            history_compaction_ratio: None,
            tool_call_mode: None,
            reasoning_effort: None,
            model_type: Some(model_type.to_string()),
            stop: None,
            mock_if_unconfigured: None,
            ..Default::default()
        }
    }

    #[test]
    fn resolves_default_tts_and_image_models_by_type() {
        let mut config = Config::default();
        config.llm.default = "chat-a".to_string();
        config.llm.default_tts = Some("tts-a".to_string());
        config.llm.default_image = Some("image-a".to_string());
        config.llm.models.insert("chat-a".to_string(), model("llm"));
        config.llm.models.insert("tts-a".to_string(), model("tts"));
        config
            .llm
            .models
            .insert("image-a".to_string(), model("image"));

        assert_eq!(resolve_tts_model(&config, None).unwrap().0, "tts-a");
        assert_eq!(resolve_image_model(&config, None).unwrap().0, "image-a");
        assert_eq!(list_tts_model_names(&config), vec!["tts-a".to_string()]);
        assert_eq!(list_image_model_names(&config), vec!["image-a".to_string()]);
    }

    #[test]
    fn requested_tts_model_must_match_type() {
        let mut config = Config::default();
        config.llm.models.insert("chat-a".to_string(), model("llm"));

        assert!(resolve_tts_model(&config, Some("chat-a")).is_none());
    }
}
