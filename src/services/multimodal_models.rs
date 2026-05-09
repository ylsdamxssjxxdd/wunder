use crate::config::{Config, LlmModelConfig};
use crate::llm::{
    build_model_auth_headers, build_openai_model_resource_endpoint, is_asr_model, is_image_model,
    is_tts_model, is_video_model, normalize_provider, resolve_model_base_url,
};
use anyhow::{anyhow, Context, Result};
use bytes::Bytes;
use reqwest::multipart::{Form, Part};
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::Duration;

const AUDIO_SPEECH_RESOURCE: &str = "audio/speech";
const AUDIO_TRANSCRIPTIONS_RESOURCE: &str = "audio/transcriptions";
const IMAGES_GENERATIONS_RESOURCE: &str = "images/generations";
const VIDEOS_RESOURCE: &str = "videos";
const AUDIO_VOICES_RESOURCE: &str = "audio/voices";
const DEFAULT_ASR_RESPONSE_FORMAT: &str = "json";
const DEFAULT_ASR_TIMEOUT_S: u64 = 120;
const DEFAULT_TTS_RESPONSE_FORMAT: &str = "wav";
const DEFAULT_TTS_TIMEOUT_S: u64 = 120;
const DEFAULT_IMAGE_TIMEOUT_S: u64 = 300;
const DEFAULT_VIDEO_TIMEOUT_S: u64 = 1800;
const WHISPER_CPP_PROVIDER: &str = "whisper_cpp";
const WHISPER_CPP_INFERENCE_PATH: &str = "/inference";

fn tts_voice_cache() -> &'static dashmap::DashMap<String, String> {
    static CACHE: OnceLock<dashmap::DashMap<String, String>> = OnceLock::new();
    CACHE.get_or_init(dashmap::DashMap::new)
}

pub async fn probe_tts_voices(
    base_url: &str,
    api_key: &str,
    model: &str,
    timeout_s: u64,
) -> Result<Vec<String>> {
    let trimmed_base = base_url.trim();
    let trimmed_model = model.trim();
    if trimmed_base.is_empty() || trimmed_model.is_empty() {
        return Ok(Vec::new());
    }
    let cache_key = format!("{}|{}", trimmed_base.trim_end_matches('/'), trimmed_model);
    if let Some(cached) = tts_voice_cache().get(&cache_key) {
        return Ok(vec![cached.value().clone()]);
    }
    let endpoint = build_openai_model_resource_endpoint(trimmed_base, AUDIO_VOICES_RESOURCE)
        .ok_or_else(|| anyhow!("tts voices endpoint is invalid"))?;
    let client = Client::builder()
        .timeout(Duration::from_secs(timeout_s.max(5)))
        .build()?;
    let response = client
        .get(endpoint)
        .headers(build_model_auth_headers(api_key))
        .send()
        .await
        .context("send tts voices request")?;
    if !response.status().is_success() {
        let status = response.status();
        let detail = response.text().await.unwrap_or_default();
        return Err(anyhow!(
            "tts voices request failed: {status} {}",
            truncate_for_error(&detail, 512)
        ));
    }
    let body: Value = response.json().await.context("parse tts voices response")?;
    let mut voices = body
        .get("voices")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| match item {
                    Value::String(value) => Some(value.trim().to_string()),
                    Value::Object(map) => map
                        .get("name")
                        .and_then(Value::as_str)
                        .or_else(|| map.get("id").and_then(Value::as_str))
                        .map(str::trim)
                        .map(ToString::to_string),
                    _ => None,
                })
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    voices.sort();
    voices.dedup();
    if let Some(first) = voices.first() {
        tts_voice_cache().insert(cache_key, first.clone());
    }
    Ok(voices)
}

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

#[derive(Debug, Clone)]
pub struct AudioTranscriptionRequest {
    pub audio_bytes: Bytes,
    pub filename: String,
    pub content_type: String,
    pub model_name: Option<String>,
    pub language: Option<String>,
    pub prompt: Option<String>,
    pub response_format: Option<String>,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct AudioTranscriptionResponse {
    pub text: String,
    pub content_type: String,
    pub raw_response: Value,
}

#[derive(Debug, Clone)]
pub struct ImageGenerationRequest {
    pub prompt: String,
    pub model_name: Option<String>,
    pub size: Option<String>,
    pub output_format: Option<String>,
    pub negative_prompt: Option<String>,
    pub num_inference_steps: Option<u32>,
    pub guidance_scale: Option<f32>,
    pub seed: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ImageGenerationResponse {
    pub bytes: Bytes,
    pub content_type: String,
}

#[derive(Debug, Clone)]
pub struct VideoGenerationRequest {
    pub prompt: String,
    pub model_name: Option<String>,
    pub size: Option<String>,
    pub seconds: Option<f32>,
    pub fps: Option<u32>,
    pub num_frames: Option<u32>,
    pub negative_prompt: Option<String>,
    pub num_inference_steps: Option<u32>,
    pub guidance_scale: Option<f32>,
    pub guidance_scale_2: Option<f32>,
    pub boundary_ratio: Option<f32>,
    pub flow_shift: Option<f32>,
    pub seed: Option<u64>,
    pub enable_frame_interpolation: Option<bool>,
    pub sync_mode: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct VideoGenerationResponse {
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

pub async fn transcribe_audio(
    config: &Config,
    request: AudioTranscriptionRequest,
) -> Result<AudioTranscriptionResponse> {
    let (_name, model_config) = resolve_asr_model(config, request.model_name.as_deref())
        .ok_or_else(|| anyhow!("asr model is not configured"))?;
    transcribe_audio_with_model(&model_config, &request).await
}

pub async fn generate_image(
    config: &Config,
    request: ImageGenerationRequest,
) -> Result<ImageGenerationResponse> {
    let (_name, model_config) = resolve_image_model(config, request.model_name.as_deref())
        .ok_or_else(|| anyhow!("image model is not configured"))?;
    generate_image_with_model(&model_config, &request).await
}

pub async fn generate_video(
    config: &Config,
    request: VideoGenerationRequest,
) -> Result<VideoGenerationResponse> {
    let (_name, model_config) = resolve_video_model(config, request.model_name.as_deref())
        .ok_or_else(|| anyhow!("video model is not configured"))?;
    generate_video_with_model(&model_config, &request).await
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

pub fn list_asr_model_names(config: &Config) -> Vec<String> {
    let mut names = config
        .llm
        .models
        .iter()
        .filter(|(_, model)| is_asr_model(model))
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

pub fn list_video_model_names(config: &Config) -> Vec<String> {
    let mut names = config
        .llm
        .models
        .iter()
        .filter(|(_, model)| is_video_model(model))
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

pub fn resolve_asr_model(
    config: &Config,
    requested_name: Option<&str>,
) -> Option<(String, LlmModelConfig)> {
    resolve_model_by_type(
        config,
        requested_name,
        config.llm.default_asr.as_deref(),
        is_asr_model,
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

pub fn resolve_video_model(
    config: &Config,
    requested_name: Option<&str>,
) -> Option<(String, LlmModelConfig)> {
    resolve_model_by_type(
        config,
        requested_name,
        config.llm.default_video.as_deref(),
        is_video_model,
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
    let resolved_voice = resolve_tts_voice(&base_url, config, request)
        .await
        .context("resolve tts voice")?;
    if resolved_voice.is_none() {
        return Err(anyhow!(
            "tts voice is required by the upstream service; configure tts_voice or expose /v1/audio/voices"
        ));
    }
    let payload = build_speech_payload(
        config,
        model,
        request,
        &response_format,
        resolved_voice.as_deref(),
    );
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

async fn transcribe_audio_with_model(
    config: &LlmModelConfig,
    request: &AudioTranscriptionRequest,
) -> Result<AudioTranscriptionResponse> {
    if normalize_provider(config.provider.as_deref()) == WHISPER_CPP_PROVIDER {
        return transcribe_audio_with_whisper_cpp(config, request).await;
    }
    let base_url =
        resolve_model_base_url(config).ok_or_else(|| anyhow!("asr base_url is required"))?;
    let endpoint = build_openai_model_resource_endpoint(&base_url, AUDIO_TRANSCRIPTIONS_RESOURCE)
        .ok_or_else(|| anyhow!("asr base_url is invalid"))?;
    let model = config
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("asr model is required"))?;
    let timeout_s = config.timeout_s.unwrap_or(DEFAULT_ASR_TIMEOUT_S).max(10);
    let client = Client::builder()
        .timeout(Duration::from_secs(timeout_s))
        .build()?;

    let response_format = normalize_asr_response_format(
        request
            .response_format
            .as_deref()
            .or(config.asr_response_format.as_deref()),
    );

    let mut part = reqwest::multipart::Part::bytes(request.audio_bytes.to_vec())
        .file_name(request.filename.clone());
    if !request.content_type.trim().is_empty() {
        part = part
            .mime_str(request.content_type.trim())
            .context("invalid asr content type")?;
    }

    let mut form = Form::new()
        .part("file", part)
        .text("model", model.to_string())
        .text("response_format", response_format.clone());
    if let Some(language) = request
        .language
        .as_deref()
        .or(config.asr_language.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        form = form.text("language", language.to_string());
    }
    if let Some(prompt) = request
        .prompt
        .as_deref()
        .or(config.asr_prompt.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        form = form.text("prompt", prompt.to_string());
    }
    if let Some(temperature) = request
        .temperature
        .or(config.asr_temperature)
        .filter(|value| value.is_finite() && *value >= 0.0)
    {
        form = form.text("temperature", temperature.to_string());
    }

    let response = client
        .post(endpoint)
        .headers(build_model_auth_headers(
            config.api_key.as_deref().unwrap_or(""),
        ))
        .multipart(form)
        .send()
        .await
        .context("send asr request")?;
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| "application/json".to_string());
    let body = response.bytes().await.context("read asr response body")?;
    if !status.is_success() {
        let detail = String::from_utf8_lossy(&body);
        return Err(anyhow!(
            "asr request failed: {status} {}",
            truncate_for_error(&detail, 1024)
        ));
    }
    if body.is_empty() {
        return Err(anyhow!("asr response is empty"));
    }
    let raw_response = if response_format == "text" {
        let text = String::from_utf8(body.to_vec()).context("decode asr text response")?;
        let trimmed = text.trim().to_string();
        json!({ "text": trimmed })
    } else {
        serde_json::from_slice::<Value>(&body).context("parse asr response json")?
    };
    let text =
        extract_asr_text(&raw_response).ok_or_else(|| anyhow!("asr response is missing text"))?;
    Ok(AudioTranscriptionResponse {
        text,
        content_type,
        raw_response,
    })
}

async fn transcribe_audio_with_whisper_cpp(
    config: &LlmModelConfig,
    request: &AudioTranscriptionRequest,
) -> Result<AudioTranscriptionResponse> {
    let base_url =
        resolve_model_base_url(config).ok_or_else(|| anyhow!("asr base_url is required"))?;
    let endpoint = build_whisper_cpp_inference_endpoint(&base_url)
        .ok_or_else(|| anyhow!("whisper.cpp base_url is invalid"))?;
    let timeout_s = config.timeout_s.unwrap_or(DEFAULT_ASR_TIMEOUT_S).max(10);
    let client = Client::builder()
        .timeout(Duration::from_secs(timeout_s))
        .build()?;

    let response_format = normalize_asr_response_format(
        request
            .response_format
            .as_deref()
            .or(config.asr_response_format.as_deref()),
    );

    let mut part = Part::bytes(request.audio_bytes.to_vec()).file_name(request.filename.clone());
    if !request.content_type.trim().is_empty() {
        part = part
            .mime_str(request.content_type.trim())
            .context("invalid asr content type")?;
    }

    let mut form = Form::new()
        .part("file", part)
        .text("response_format", response_format.clone());
    if let Some(language) = request
        .language
        .as_deref()
        .or(config.asr_language.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        form = form.text("language", language.to_string());
    }
    if let Some(prompt) = request
        .prompt
        .as_deref()
        .or(config.asr_prompt.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        form = form.text("prompt", prompt.to_string());
    }
    if let Some(temperature) = request
        .temperature
        .or(config.asr_temperature)
        .filter(|value| value.is_finite() && *value >= 0.0)
    {
        form = form.text("temperature", temperature.to_string());
    }

    let response = client
        .post(endpoint)
        .multipart(form)
        .send()
        .await
        .context("send whisper.cpp asr request")?;
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| whisper_cpp_content_type(&response_format).to_string());
    let body = response
        .bytes()
        .await
        .context("read whisper.cpp asr response body")?;
    if !status.is_success() {
        let detail = String::from_utf8_lossy(&body);
        return Err(anyhow!(
            "whisper.cpp asr request failed: {status} {}",
            truncate_for_error(&detail, 1024)
        ));
    }
    if body.is_empty() {
        return Err(anyhow!("whisper.cpp asr response is empty"));
    }

    let raw_response = if response_format == "json" || response_format == "verbose_json" {
        match serde_json::from_slice::<Value>(&body) {
            Ok(value) => value,
            Err(err) => {
                let text =
                    String::from_utf8(body.to_vec()).context("decode whisper.cpp response")?;
                let trimmed = text.trim().to_string();
                if trimmed.is_empty() {
                    return Err(anyhow!("whisper.cpp asr response parse failed: {err}"));
                }
                json!({ "text": trimmed, "format": response_format })
            }
        }
    } else {
        let text = String::from_utf8(body.to_vec()).context("decode whisper.cpp response")?;
        let trimmed = text.trim().to_string();
        json!({ "text": trimmed, "format": response_format })
    };
    let text = extract_asr_text(&raw_response)
        .ok_or_else(|| anyhow!("whisper.cpp asr response is missing text"))?;
    Ok(AudioTranscriptionResponse {
        text,
        content_type,
        raw_response,
    })
}

fn build_speech_payload(
    config: &LlmModelConfig,
    model: &str,
    request: &SpeechSynthesisRequest,
    response_format: &str,
    resolved_voice: Option<&str>,
) -> Value {
    let mut payload = json!({
        "model": model,
        "input": request.text,
        "response_format": response_format,
        "stream": false,
    });
    if let Some(map) = payload.as_object_mut() {
        if let Some(voice) = resolved_voice {
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

fn normalize_asr_response_format(value: Option<&str>) -> String {
    match value
        .unwrap_or(DEFAULT_ASR_RESPONSE_FORMAT)
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "text" | "json" | "verbose_json" | "srt" | "vtt" => value
            .unwrap_or(DEFAULT_ASR_RESPONSE_FORMAT)
            .trim()
            .to_ascii_lowercase(),
        _ => DEFAULT_ASR_RESPONSE_FORMAT.to_string(),
    }
}

fn extract_asr_text(value: &Value) -> Option<String> {
    value
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToString::to_string)
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

async fn generate_image_with_model(
    config: &LlmModelConfig,
    request: &ImageGenerationRequest,
) -> Result<ImageGenerationResponse> {
    let base_url =
        resolve_model_base_url(config).ok_or_else(|| anyhow!("image base_url is required"))?;
    let endpoint = build_openai_model_resource_endpoint(&base_url, IMAGES_GENERATIONS_RESOURCE)
        .ok_or_else(|| anyhow!("image base_url is invalid"))?;
    let model = config
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("image model is required"))?;
    let timeout_s = config.timeout_s.unwrap_or(DEFAULT_IMAGE_TIMEOUT_S).max(10);
    let client = Client::builder()
        .timeout(Duration::from_secs(timeout_s))
        .build()?;
    let payload = build_image_payload(config, model, request);
    let response = client
        .post(endpoint)
        .headers(build_model_auth_headers(
            config.api_key.as_deref().unwrap_or(""),
        ))
        .json(&payload)
        .send()
        .await
        .context("send image request")?;
    let status = response.status();
    let body = response.text().await.context("read image response body")?;
    if !status.is_success() {
        return Err(anyhow!(
            "image request failed: {status} {}",
            truncate_for_error(&body, 1024)
        ));
    }
    let value: Value = serde_json::from_str(&body).context("parse image response json")?;
    let b64 = value
        .get("data")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("b64_json"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("image response is missing b64_json"))?;
    let bytes = decode_base64_payload(b64).context("decode image base64 payload")?;
    if bytes.is_empty() {
        return Err(anyhow!("image response is empty"));
    }
    let output_format = normalize_image_output_format(
        request
            .output_format
            .as_deref()
            .or(config.image_output_format.as_deref()),
    );
    Ok(ImageGenerationResponse {
        bytes,
        content_type: image_mime_type(&output_format).to_string(),
    })
}

fn build_image_payload(
    config: &LlmModelConfig,
    model: &str,
    request: &ImageGenerationRequest,
) -> Value {
    let mut payload = json!({
        "model": model,
        "prompt": request.prompt,
        "n": 1,
        "response_format": "b64_json",
    });
    if let Some(map) = payload.as_object_mut() {
        if let Some(size) = request
            .size
            .as_deref()
            .or(config.image_size.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            map.insert("size".to_string(), json!(size));
        }
        if let Some(negative_prompt) = request
            .negative_prompt
            .as_deref()
            .or(config.image_negative_prompt.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            map.insert("negative_prompt".to_string(), json!(negative_prompt));
        }
        if let Some(num_inference_steps) = request
            .num_inference_steps
            .or(config.image_num_inference_steps)
            .filter(|value| *value > 0)
        {
            map.insert(
                "num_inference_steps".to_string(),
                json!(num_inference_steps),
            );
        }
        if let Some(guidance_scale) = request
            .guidance_scale
            .or(config.image_guidance_scale)
            .filter(|value| value.is_finite() && *value > 0.0)
        {
            map.insert("guidance_scale".to_string(), json!(guidance_scale));
        }
        if let Some(seed) = request.seed {
            map.insert("seed".to_string(), json!(seed));
        }
    }
    payload
}

async fn resolve_tts_voice(
    base_url: &str,
    config: &LlmModelConfig,
    request: &SpeechSynthesisRequest,
) -> Result<Option<String>> {
    if let Some(explicit) = request
        .voice
        .as_deref()
        .or(config.tts_voice.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(Some(explicit.to_string()));
    }
    let model = config
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("");
    let cache_key = format!("{}|{}", base_url.trim_end_matches('/'), model);
    if let Some(cached) = tts_voice_cache().get(&cache_key) {
        return Ok(Some(cached.value().clone()));
    }
    let api_key = config.api_key.as_deref().unwrap_or("");
    match probe_tts_voices(base_url, api_key, model, 10).await {
        Ok(voices) => Ok(voices.into_iter().next()),
        Err(_) => Ok(None),
    }
}

fn normalize_image_output_format(value: Option<&str>) -> String {
    match value.unwrap_or("png").trim().to_ascii_lowercase().as_str() {
        "jpeg" | "jpg" => "jpeg",
        "webp" => "webp",
        _ => "png",
    }
    .to_string()
}

fn build_whisper_cpp_inference_endpoint(base_url: &str) -> Option<String> {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    Some(format!("{trimmed}{WHISPER_CPP_INFERENCE_PATH}"))
}

fn whisper_cpp_content_type(format: &str) -> &'static str {
    match format {
        "text" => "text/html; charset=utf-8",
        "srt" => "text/plain; charset=utf-8",
        "vtt" => "text/vtt",
        _ => "application/json",
    }
}

fn image_mime_type(format: &str) -> &'static str {
    match format {
        "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/png",
    }
}

async fn generate_video_with_model(
    config: &LlmModelConfig,
    request: &VideoGenerationRequest,
) -> Result<VideoGenerationResponse> {
    let base_url =
        resolve_model_base_url(config).ok_or_else(|| anyhow!("video base_url is required"))?;
    let endpoint = build_openai_model_resource_endpoint(&base_url, &video_sync_resource_path())
        .ok_or_else(|| anyhow!("video base_url is invalid"))?;
    let model = config
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("video model is required"))?;
    let timeout_s = config.timeout_s.unwrap_or(DEFAULT_VIDEO_TIMEOUT_S).max(30);
    let client = Client::builder()
        .timeout(Duration::from_secs(timeout_s))
        .build()?;
    let form = build_video_form(config, model, request);
    let response = client
        .post(endpoint)
        .headers(build_model_auth_headers(
            config.api_key.as_deref().unwrap_or(""),
        ))
        .multipart(form)
        .send()
        .await
        .context("send video request")?;
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| "video/mp4".to_string());
    let bytes = response.bytes().await.context("read video response body")?;
    if !status.is_success() {
        let detail = String::from_utf8_lossy(&bytes);
        return Err(anyhow!(
            "video request failed: {status} {}",
            truncate_for_error(&detail, 1024)
        ));
    }
    if bytes.is_empty() {
        return Err(anyhow!("video response is empty"));
    }
    Ok(VideoGenerationResponse {
        bytes,
        content_type,
    })
}

fn video_sync_resource_path() -> String {
    format!("{VIDEOS_RESOURCE}/sync")
}

fn build_video_form(
    config: &LlmModelConfig,
    model: &str,
    request: &VideoGenerationRequest,
) -> Form {
    let mut form = Form::new()
        .text("model", model.to_string())
        .text("prompt", request.prompt.clone());
    if let Some(size) = request
        .size
        .as_deref()
        .or(config.video_size.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        form = form.text("size", size.to_string());
    }
    if let Some(seconds) = request
        .seconds
        .or(config.video_seconds)
        .filter(|value| value.is_finite() && *value > 0.0)
    {
        form = form.text("seconds", seconds.to_string());
    }
    if let Some(fps) = request.fps.or(config.video_fps).filter(|value| *value > 0) {
        form = form.text("fps", fps.to_string());
    }
    if let Some(num_frames) = request
        .num_frames
        .or(config.video_num_frames)
        .filter(|value| *value > 0)
    {
        form = form.text("num_frames", num_frames.to_string());
    }
    if let Some(negative_prompt) = request
        .negative_prompt
        .as_deref()
        .or(config.video_negative_prompt.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        form = form.text("negative_prompt", negative_prompt.to_string());
    }
    if let Some(num_inference_steps) = request
        .num_inference_steps
        .or(config.video_num_inference_steps)
        .filter(|value| *value > 0)
    {
        form = form.text("num_inference_steps", num_inference_steps.to_string());
    }
    if let Some(guidance_scale) = request
        .guidance_scale
        .or(config.video_guidance_scale)
        .filter(|value| value.is_finite() && *value > 0.0)
    {
        form = form.text("guidance_scale", guidance_scale.to_string());
    }
    if let Some(guidance_scale_2) = request
        .guidance_scale_2
        .or(config.video_guidance_scale_2)
        .filter(|value| value.is_finite() && *value > 0.0)
    {
        form = form.text("guidance_scale_2", guidance_scale_2.to_string());
    }
    if let Some(boundary_ratio) = request
        .boundary_ratio
        .or(config.video_boundary_ratio)
        .filter(|value| value.is_finite() && *value > 0.0)
    {
        form = form.text("boundary_ratio", boundary_ratio.to_string());
    }
    if let Some(flow_shift) = request
        .flow_shift
        .or(config.video_flow_shift)
        .filter(|value| value.is_finite() && *value > 0.0)
    {
        form = form.text("flow_shift", flow_shift.to_string());
    }
    if let Some(seed) = request.seed {
        form = form.text("seed", seed.to_string());
    }
    if let Some(enable_frame_interpolation) = request
        .enable_frame_interpolation
        .or(config.video_enable_frame_interpolation)
    {
        form = form.text(
            "enable_frame_interpolation",
            if enable_frame_interpolation {
                "true"
            } else {
                "false"
            }
            .to_string(),
        );
    }
    form
}

fn decode_base64_payload(value: &str) -> Result<Bytes> {
    let cleaned = if let Some((_, b64)) = value.split_once(',') {
        b64
    } else {
        value
    };
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, cleaned)
        .context("base64 decode failed")?;
    Ok(Bytes::from(bytes))
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
        build_speech_payload, list_asr_model_names, list_image_model_names, list_tts_model_names,
        list_video_model_names, normalize_asr_response_format, resolve_asr_model,
        resolve_image_model, resolve_tts_model, resolve_video_model,
    };
    use crate::config::{Config, LlmModelConfig};
    use serde_json::Value;

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
        config.llm.default_asr = Some("asr-a".to_string());
        config.llm.default_tts = Some("tts-a".to_string());
        config.llm.default_image = Some("image-a".to_string());
        config.llm.default_video = Some("video-a".to_string());
        config.llm.models.insert("chat-a".to_string(), model("llm"));
        config.llm.models.insert("asr-a".to_string(), model("asr"));
        config.llm.models.insert("tts-a".to_string(), model("tts"));
        config
            .llm
            .models
            .insert("image-a".to_string(), model("image"));
        config
            .llm
            .models
            .insert("video-a".to_string(), model("video"));

        assert_eq!(resolve_asr_model(&config, None).unwrap().0, "asr-a");
        assert_eq!(resolve_tts_model(&config, None).unwrap().0, "tts-a");
        assert_eq!(resolve_image_model(&config, None).unwrap().0, "image-a");
        assert_eq!(resolve_video_model(&config, None).unwrap().0, "video-a");
        assert_eq!(list_asr_model_names(&config), vec!["asr-a".to_string()]);
        assert_eq!(list_tts_model_names(&config), vec!["tts-a".to_string()]);
        assert_eq!(list_image_model_names(&config), vec!["image-a".to_string()]);
        assert_eq!(list_video_model_names(&config), vec!["video-a".to_string()]);
    }

    #[test]
    fn normalize_asr_response_format_falls_back_to_json() {
        assert_eq!(normalize_asr_response_format(Some("text")), "text");
        assert_eq!(
            normalize_asr_response_format(Some("verbose_json")),
            "verbose_json"
        );
        assert_eq!(normalize_asr_response_format(Some("unknown")), "json");
    }

    #[test]
    fn requested_tts_model_must_match_type() {
        let mut config = Config::default();
        config.llm.models.insert("chat-a".to_string(), model("llm"));

        assert!(resolve_tts_model(&config, Some("chat-a")).is_none());
    }

    #[test]
    fn tts_payload_omits_voice_without_resolution() {
        let config = model("tts");
        let request = super::SpeechSynthesisRequest {
            text: "hello".to_string(),
            model_name: None,
            voice: None,
            instructions: None,
            response_format: None,
            speed: None,
        };

        let payload = build_speech_payload(&config, "tts-model", &request, "wav", None);

        assert_eq!(payload.get("voice"), None);
    }

    #[test]
    fn tts_payload_uses_resolved_voice() {
        let config = model("tts");
        let request = super::SpeechSynthesisRequest {
            text: "hello".to_string(),
            model_name: None,
            voice: None,
            instructions: None,
            response_format: None,
            speed: None,
        };

        let payload = build_speech_payload(&config, "tts-model", &request, "wav", Some("Vivian"));

        assert_eq!(payload.get("voice").and_then(Value::as_str), Some("Vivian"));
    }
}
