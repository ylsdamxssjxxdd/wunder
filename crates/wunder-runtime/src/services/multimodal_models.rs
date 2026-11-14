use crate::config::{Config, LlmModelConfig};
use crate::core::command_utils::{apply_platform_spawn_options, is_not_found_error};
use crate::llm::{
    build_model_auth_headers, build_openai_model_resource_endpoint, is_asr_model, is_image_model,
    is_tts_model, is_video_model, normalize_provider, resolve_model_base_url,
};
use anyhow::{anyhow, Context, Result};
use bytes::Bytes;
use reqwest::multipart::{Form, Part};
use reqwest::Client;
use serde_json::{json, Value};
use std::io::Cursor;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::process::Command;

const AUDIO_SPEECH_RESOURCE: &str = "audio/speech";
const AUDIO_TRANSCRIPTIONS_RESOURCE: &str = "audio/transcriptions";
const IMAGES_GENERATIONS_RESOURCE: &str = "images/generations";
const IMAGES_EDITS_RESOURCE: &str = "images/edits";
const VIDEOS_RESOURCE: &str = "videos";
const AUDIO_VOICES_RESOURCE: &str = "audio/voices";
const DEFAULT_ASR_RESPONSE_FORMAT: &str = "json";
const DEFAULT_ASR_TIMEOUT_S: u64 = 120;
const DEFAULT_TTS_RESPONSE_FORMAT: &str = "wav";
const DEFAULT_TTS_TIMEOUT_S: u64 = 120;
const DEFAULT_IMAGE_TIMEOUT_S: u64 = 300;
const DEFAULT_VIDEO_TIMEOUT_S: u64 = 1800;
pub const DEFAULT_IMAGE_SIZE: &str = "1024x1024";
const WHISPER_CPP_PROVIDER: &str = "whisper_cpp";
const VLLM_OMNI_PROVIDER: &str = "vllm_omni";
/// vllm-omni / Z-Image 一类服务要求宽高都被 16 整除。
const VLLM_OMNI_IMAGE_ALIGNMENT: u32 = 16;
/// 图像输入解码上限：超过该像素数直接拒绝，避免重编码时把进程内存打爆。
const MAX_IMAGE_INPUT_PIXELS: u64 = 64 * 1024 * 1024;
const WHISPER_CPP_INFERENCE_PATH: &str = "/inference";
const FFMPEG_BIN_ENV: &str = "WUNDER_FFMPEG_BIN";

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
    pub ref_audio: Option<String>,
    pub ref_text: Option<String>,
    pub model_specific_params: Option<Value>,
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
pub struct ImageInputFile {
    pub bytes: Bytes,
    pub filename: String,
    pub content_type: String,
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
    pub input_images: Vec<ImageInputFile>,
    pub mask_image: Option<ImageInputFile>,
    pub reference_image: Option<ImageInputFile>,
    pub strength: Option<f32>,
    pub true_cfg_scale: Option<f32>,
    pub output_compression: Option<u32>,
    pub layers: Option<u32>,
    pub resolution: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ImageGenerationResponse {
    pub bytes: Bytes,
    pub content_type: String,
    /// 真正发给上游的尺寸（`None` 表示由上游按输入图自行决定）。
    pub size: Option<String>,
    /// 输入图被等比缩小时的尺寸变化，形如 `4096x4096->1024x1024`。
    pub resized_input: Option<String>,
    /// 蒙版随主图一起被缩放时的尺寸变化；调用方必须把它写进 warning。
    pub resized_mask: Option<String>,
    /// 参考图被等比缩小时的尺寸变化。
    pub resized_reference: Option<String>,
    /// 第二张及之后的输入图被等比缩小时的尺寸变化。
    pub resized_extra_inputs: Vec<String>,
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
    // 尺寸限幅与输入图缩放是纯 CPU 活，放后台线程做，避免卡住运行时。
    let prepare_config = model_config.clone();
    let (request, prepared) = tokio::task::spawn_blocking(move || {
        let mut request = request;
        let prepared = prepare_image_request(&prepare_config, &mut request)?;
        Ok::<_, anyhow::Error>((request, prepared))
    })
    .await
    .context("prepare image request")??;
    let mut response = generate_image_with_model(&model_config, &request).await?;
    // 回执给真实输出尺寸：编辑模式未显式指定时就是输入图尺寸，不再返回 null。
    response.size = prepared.reported_size.or(prepared.size);
    response.resized_input = prepared.resized_input;
    response.resized_mask = prepared.resized_mask;
    response.resized_reference = prepared.resized_reference;
    response.resized_extra_inputs = prepared.resized_extra_inputs;
    Ok(response)
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

    let upload = prepare_whisper_cpp_audio_upload(request).await?;

    let mut part = Part::bytes(upload.audio_bytes).file_name(upload.filename);
    if !upload.content_type.trim().is_empty() {
        part = part
            .mime_str(upload.content_type.trim())
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
        if let Some(ref_audio) = request
            .ref_audio
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            map.insert("ref_audio".to_string(), json!(ref_audio));
        }
        if let Some(ref_text) = request
            .ref_text
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            map.insert("ref_text".to_string(), json!(ref_text));
        }
        if let Some(extra) = request
            .model_specific_params
            .as_ref()
            .and_then(Value::as_object)
        {
            for (key, value) in extra {
                let normalized = key.trim();
                if normalized.is_empty() {
                    continue;
                }
                if matches!(
                    normalized,
                    "model"
                        | "input"
                        | "voice"
                        | "instructions"
                        | "response_format"
                        | "stream"
                        | "speed"
                        | "ref_audio"
                        | "ref_text"
                ) {
                    continue;
                }
                map.insert(normalized.to_string(), value.clone());
            }
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
    if !request.input_images.is_empty()
        || request.mask_image.is_some()
        || request.reference_image.is_some()
    {
        return edit_image_with_model(config, request).await;
    }
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
        return Err(anyhow!(format_image_http_error(
            "image request failed",
            status,
            &body
        )));
    }
    parse_image_generation_response(
        &body,
        request
            .output_format
            .as_deref()
            .or(config.image_output_format.as_deref()),
    )
}

async fn edit_image_with_model(
    config: &LlmModelConfig,
    request: &ImageGenerationRequest,
) -> Result<ImageGenerationResponse> {
    if request.input_images.is_empty() {
        return Err(anyhow!("input image is required for image editing"));
    }
    let base_url =
        resolve_model_base_url(config).ok_or_else(|| anyhow!("image base_url is required"))?;
    let endpoint = build_openai_model_resource_endpoint(&base_url, IMAGES_EDITS_RESOURCE)
        .ok_or_else(|| anyhow!("image edit base_url is invalid"))?;
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
    let form = build_image_edit_form(config, model, request)?;
    let response = client
        .post(endpoint)
        .headers(build_model_auth_headers(
            config.api_key.as_deref().unwrap_or(""),
        ))
        .multipart(form)
        .send()
        .await
        .context("send image edit request")?;
    let status = response.status();
    let body = response
        .text()
        .await
        .context("read image edit response body")?;
    if !status.is_success() {
        return Err(anyhow!(format_image_http_error(
            "image edit request failed",
            status,
            &body
        )));
    }
    parse_image_generation_response(
        &body,
        request
            .output_format
            .as_deref()
            .or(config.image_output_format.as_deref()),
    )
}

fn parse_image_generation_response(
    body: &str,
    requested_output_format: Option<&str>,
) -> Result<ImageGenerationResponse> {
    let value: Value = serde_json::from_str(body).context("parse image response json")?;
    if let Some(detail) = extract_image_provider_error_message(&value) {
        return Err(anyhow!(
            "image generation failed: {}",
            truncate_for_error(&detail, 1024)
        ));
    }
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
    // 以真实字节为准：上游可能忽略 output_format 仍返回 PNG，此时按请求格式贴
    // MIME 会把 PNG 存成 .webp。先嗅探魔数，探测不出才退回请求格式。
    let detected = detect_image_output_format(&bytes);
    let output_format = detected
        .map(ToString::to_string)
        .unwrap_or_else(|| normalize_image_output_format(requested_output_format));
    Ok(ImageGenerationResponse {
        bytes,
        content_type: image_mime_type(&output_format).to_string(),
        size: None,
        resized_input: None,
        resized_mask: None,
        resized_reference: None,
        resized_extra_inputs: Vec::new(),
    })
}

/// 从图片字节嗅探真实格式；只认本工具会写出的三种。
fn detect_image_output_format(bytes: &[u8]) -> Option<&'static str> {
    match image::guess_format(bytes).ok()? {
        image::ImageFormat::Png => Some("png"),
        image::ImageFormat::Jpeg => Some("jpeg"),
        image::ImageFormat::WebP => Some("webp"),
        _ => None,
    }
}

fn format_image_http_error(prefix: &str, status: reqwest::StatusCode, body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        if let Some(detail) = extract_image_provider_error_message(&value) {
            return format!("{prefix}: {status} {}", truncate_for_error(&detail, 1024));
        }
    }
    let fallback = summarize_raw_provider_error(body);
    format!("{prefix}: {status} {}", truncate_for_error(&fallback, 1024))
}

fn extract_image_provider_error_message(value: &Value) -> Option<String> {
    if let Some(text) = value.get("error").and_then(Value::as_str) {
        let cleaned = text.trim();
        if !cleaned.is_empty() {
            return Some(cleaned.to_string());
        }
    }
    if let Some(error) = value.get("error").and_then(Value::as_object) {
        for key in ["message", "detail", "error", "summary"] {
            if let Some(text) = error.get(key).and_then(Value::as_str) {
                let cleaned = text.trim();
                if !cleaned.is_empty() {
                    return Some(cleaned.to_string());
                }
            }
        }
    }
    for key in ["message", "detail"] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            let cleaned = text.trim();
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        }
    }
    None
}

fn summarize_raw_provider_error(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "empty provider error body".to_string();
    }
    let lines = trimmed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return trimmed.to_string();
    }
    for needle in [
        "RuntimeError:",
        "Diffusion generation failed:",
        "Height must be divisible by 16",
        "Width must be divisible by 16",
    ] {
        if let Some(line) = lines
            .iter()
            .rev()
            .find(|line| line.contains(needle))
            .copied()
        {
            return line.to_string();
        }
    }
    lines
        .last()
        .map(|line| (*line).to_string())
        .unwrap_or_else(|| trimmed.to_string())
}

/// 解析真正发给上游的图片尺寸。
///
/// - 未传尺寸：文生图用配置的默认尺寸，修图返回 `None`（由调用方按输入图尺寸决定）；
/// - 显式尺寸有一边超过 `image_max_size`：按最大边等比缩小到上限内，构图比例不变，
///   不再整张回退默认尺寸（回退会悄悄改变比例）；
/// - vllm-omni 一类要求 16 对齐的提供方，缩小结果向小对齐到 16 的倍数，
///   保证自动缩放不会反过来产出非法尺寸；
/// - 未超限的显式尺寸原样透传（含 `auto`），对齐错误仍由校验层显式报出。
pub fn resolve_requested_image_size(
    config: &LlmModelConfig,
    requested: Option<&str>,
    preserve_source_size: bool,
) -> Option<String> {
    let default_size =
        configured_image_size(config.image_default_size.as_deref()).unwrap_or(DEFAULT_IMAGE_SIZE);
    let Some(requested) = requested.map(str::trim).filter(|value| !value.is_empty()) else {
        return if preserve_source_size {
            None
        } else {
            // 默认尺寸同样受上限约束：default 配得比 max 大时不能默默突破上限。
            Some(limit_size_for_provider(config, default_size))
        };
    };
    Some(limit_size_for_provider(config, requested))
}

/// 单边超限时按最大边等比缩小，并把结果对齐到提供方要求的倍数。
fn limit_size_for_provider(config: &LlmModelConfig, size: &str) -> String {
    let Some((width, height)) = parse_image_dimensions(size) else {
        return size.to_string();
    };
    let Some((max_width, max_height)) = image_max_dimensions(config) else {
        return size.to_string();
    };
    if width <= max_width && height <= max_height {
        return size.to_string();
    }
    let (width, height) = fit_image_size_within(width, height, max_width, max_height);
    let (width, height) = align_for_provider(config, width, height, max_width, max_height);
    format!("{width}x{height}")
}

fn configured_image_size(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// 文生图的上限：未配置时按默认尺寸兜底，避免没填上限时请求任意放大。
fn image_max_dimensions(config: &LlmModelConfig) -> Option<(u32, u32)> {
    let max_size =
        configured_image_size(config.image_max_size.as_deref()).unwrap_or(DEFAULT_IMAGE_SIZE);
    parse_image_dimensions(max_size)
}

/// 显式配置的最大尺寸。编辑链路只在用户真的设了上限时才缩放输入图，
/// 避免没配上限的用户被悄悄降分辨率。
fn configured_image_max_dimensions(config: &LlmModelConfig) -> Option<(u32, u32)> {
    let max_size = configured_image_size(config.image_max_size.as_deref())?;
    parse_image_dimensions(max_size)
}

/// 等比缩小到给定上限内；本来就在范围内则原样返回（绝不放大）。
pub fn fit_image_size_within(
    width: u32,
    height: u32,
    max_width: u32,
    max_height: u32,
) -> (u32, u32) {
    if width <= max_width && height <= max_height {
        return (width, height);
    }
    let width_ratio = max_width as f64 / width.max(1) as f64;
    let height_ratio = max_height as f64 / height.max(1) as f64;
    let ratio = width_ratio.min(height_ratio);
    let scaled_width = ((width as f64 * ratio).round() as u32).clamp(1, max_width);
    let scaled_height = ((height as f64 * ratio).round() as u32).clamp(1, max_height);
    (scaled_width, scaled_height)
}

fn image_alignment_for(config: &LlmModelConfig) -> Option<u32> {
    if normalize_provider(config.provider.as_deref()) == VLLM_OMNI_PROVIDER {
        Some(VLLM_OMNI_IMAGE_ALIGNMENT)
    } else {
        None
    }
}

fn align_for_provider(
    config: &LlmModelConfig,
    width: u32,
    height: u32,
    max_width: u32,
    max_height: u32,
) -> (u32, u32) {
    let Some(unit) = image_alignment_for(config) else {
        return (width, height);
    };
    (
        align_down_within(width, unit, max_width),
        align_down_within(height, unit, max_height),
    )
}

/// 向小对齐到 `unit` 的倍数，同时不超过 `limit`；极限小图原样保留，避免对齐后变大。
fn align_down_within(value: u32, unit: u32, limit: u32) -> u32 {
    if value % unit == 0 {
        return value;
    }
    let aligned = value / unit * unit;
    if aligned >= unit {
        aligned.min(limit)
    } else {
        value.min(limit)
    }
}

pub(crate) fn parse_image_dimensions(size: &str) -> Option<(u32, u32)> {
    let cleaned = size.trim().to_ascii_lowercase().replace(' ', "");
    let (width, height) = cleaned.split_once('x')?;
    let width = width.parse::<u32>().ok()?;
    let height = height.parse::<u32>().ok()?;
    (width > 0 && height > 0).then_some((width, height))
}

/// 统一尺寸策略：文生图按请求/默认尺寸限幅；编辑先把输入图等比缩到上限内，
/// 再把缩小后的尺寸作为请求尺寸，输出比例与输入一致且不会突破显存上限。
///
/// 尺寸准备结果：真正下发的尺寸、可以对外回报的输出尺寸，以及输入图被等比
/// 缩小时的尺寸变化。
///
/// 编辑链路未显式给尺寸时 `size=None`（下发 `auto`），但输出尺寸其实等于输入图
/// 尺寸；`reported_size` 就是给回执用的真实值，避免同一个字段在两种模式下
/// 一会儿有值一会儿是 null。
#[derive(Debug)]
struct ImageRequestPreparation {
    size: Option<String>,
    reported_size: Option<String>,
    resized_input: Option<String>,
    resized_mask: Option<String>,
    resized_reference: Option<String>,
    resized_extra_inputs: Vec<String>,
}

impl ImageRequestPreparation {
    fn new(size: Option<String>) -> Self {
        Self {
            reported_size: size.clone(),
            size,
            resized_input: None,
            resized_mask: None,
            resized_reference: None,
            resized_extra_inputs: Vec::new(),
        }
    }

    fn report_source_size(mut self, width: u32, height: u32) -> Self {
        if self.reported_size.is_none() {
            self.reported_size = Some(size_label(width, height));
        }
        self
    }

    fn with_resized_reference(mut self, report: Option<String>) -> Self {
        self.resized_reference = report;
        self
    }
}

fn size_label(width: u32, height: u32) -> String {
    format!("{}x{}", width, height)
}

fn size_report(from_width: u32, from_height: u32, to_width: u32, to_height: u32) -> String {
    format!(
        "{}x{}->{}x{}",
        from_width, from_height, to_width, to_height
    )
}

fn prepare_image_request(
    config: &LlmModelConfig,
    request: &mut ImageGenerationRequest,
) -> Result<ImageRequestPreparation> {
    // 参考图不参与输出尺寸计算，但同样会进显卡，先独立限幅。
    // 限幅结果要带回调用方：静默缩小会让模型以为上游看到的是原图。
    let mut resized_reference = None;
    if let Some(reference) = request.reference_image.take() {
        let (capped, report) = cap_image_input(config, &reference)?;
        request.reference_image = Some(capped);
        resized_reference = report;
    }
    if request.input_images.is_empty() {
        let size = resolve_requested_image_size(config, request.size.as_deref(), false);
        request.size = size.clone();
        return Ok(ImageRequestPreparation::new(size).with_resized_reference(resized_reference));
    }
    let source = request.input_images[0].clone();
    let Some((source_width, source_height)) = image_bytes_dimensions(&source.bytes) else {
        // 拿不到尺寸（例如矢量图）时保持原有语义，交给上游判断。
        let size = resolve_requested_image_size(config, request.size.as_deref(), true);
        request.size = size.clone();
        return Ok(ImageRequestPreparation::new(size).with_resized_reference(resized_reference));
    };
    // 蒙版必须与主图同尺寸：尺寸不符时上游会直接 400，或按错位区域重绘，
    // 这里提前给出明确原因，不要靠缩放把蒙版硬拉到另一个比例。
    if let Some(mask) = request.mask_image.as_ref() {
        if let Some((mask_width, mask_height)) = image_bytes_dimensions(&mask.bytes) {
            if (mask_width, mask_height) != (source_width, source_height) {
                return Err(anyhow!(
                    "mask image size {}x{} must match the input image size {}x{}",
                    mask_width,
                    mask_height,
                    source_width,
                    source_height
                ));
            }
        }
    }
    let (target_width, target_height) = fit_size_for_provider(config, source_width, source_height);
    if (target_width, target_height) == (source_width, source_height) {
        // 输入图本来就在上限内：保留 auto 语义，除非调用方显式指定了尺寸。
        // 回执仍要说出真实输出尺寸——它就是输入图尺寸。
        let size = resolve_requested_image_size(config, request.size.as_deref(), true);
        request.size = size.clone();
        return Ok(
            ImageRequestPreparation::new(size)
                .report_source_size(source_width, source_height)
                .with_resized_reference(resized_reference),
        );
    }
    ensure_input_decodable(&source.filename, source_width, source_height)?;
    request.input_images[0] = resize_image_input(&source, target_width, target_height)?;
    // 其余输入图各自不超过上限即可，比例各自保持。
    let mut resized_extra_inputs = Vec::new();
    for index in 1..request.input_images.len() {
        let input = request.input_images[index].clone();
        let Some((width, height)) = image_bytes_dimensions(&input.bytes) else {
            continue;
        };
        let (width_target, height_target) = fit_size_for_provider(config, width, height);
        if (width_target, height_target) == (width, height) {
            continue;
        }
        ensure_input_decodable(&input.filename, width, height)?;
        request.input_images[index] = resize_image_input(&input, width_target, height_target)?;
        resized_extra_inputs.push(size_report(width, height, width_target, height_target));
    }
    // 蒙版必须与主图同尺寸，所以主图缩小后蒙版也要跟着缩；这是隐式改写，
    // 必须回报给调用方，否则模型会以为上游看到的还是原来那张蒙版。
    let mut resized_mask = None;
    if let Some(mask) = request.mask_image.take() {
        if let Some((mask_width, mask_height)) = image_bytes_dimensions(&mask.bytes) {
            resized_mask = Some(size_report(
                mask_width,
                mask_height,
                target_width,
                target_height,
            ));
        }
        request.mask_image = Some(resize_image_input(&mask, target_width, target_height)?);
    }
    let size = size_label(target_width, target_height);
    request.size = Some(size.clone());
    Ok(ImageRequestPreparation {
        size: Some(size.clone()),
        reported_size: Some(size),
        resized_input: Some(size_report(
            source_width,
            source_height,
            target_width,
            target_height,
        )),
        resized_mask,
        resized_reference,
        resized_extra_inputs,
    })
}

fn fit_size_for_provider(config: &LlmModelConfig, width: u32, height: u32) -> (u32, u32) {
    let Some((max_width, max_height)) = configured_image_max_dimensions(config) else {
        return (width, height);
    };
    let (width, height) = fit_image_size_within(width, height, max_width, max_height);
    align_for_provider(config, width, height, max_width, max_height)
}

/// 限幅单张输入图，同时回报是否真的缩过：缩放必须能被调用方看见。
fn cap_image_input(
    config: &LlmModelConfig,
    input: &ImageInputFile,
) -> Result<(ImageInputFile, Option<String>)> {
    let Some((width, height)) = image_bytes_dimensions(&input.bytes) else {
        return Ok((input.clone(), None));
    };
    let (target_width, target_height) = fit_size_for_provider(config, width, height);
    if (target_width, target_height) == (width, height) {
        return Ok((input.clone(), None));
    }
    ensure_input_decodable(&input.filename, width, height)?;
    let resized = resize_image_input(input, target_width, target_height)?;
    Ok((
        resized,
        Some(size_report(width, height, target_width, target_height)),
    ))
}

fn ensure_input_decodable(filename: &str, width: u32, height: u32) -> Result<()> {
    let pixels = width as u64 * height as u64;
    if pixels > MAX_IMAGE_INPUT_PIXELS {
        return Err(anyhow!(
            "image input {filename} is too large to resize ({width}x{height}, {pixels} pixels > limit {MAX_IMAGE_INPUT_PIXELS}); downscale it first"
        ));
    }
    Ok(())
}

fn image_bytes_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

/// 重编码为 PNG：扩展名与 MIME 一起换掉，避免上游按原扩展名解析出错。
fn resize_image_input(input: &ImageInputFile, width: u32, height: u32) -> Result<ImageInputFile> {
    let decoded = image::load_from_memory(&input.bytes)
        .with_context(|| format!("decode image input {}", input.filename))?;
    let resized = decoded.resize_exact(width, height, image::imageops::FilterType::CatmullRom);
    let mut bytes = Vec::new();
    resized
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .with_context(|| format!("encode resized image input {}", input.filename))?;
    Ok(ImageInputFile {
        bytes: Bytes::from(bytes),
        filename: png_filename(&input.filename),
        content_type: "image/png".to_string(),
    })
}

fn png_filename(filename: &str) -> String {
    match Path::new(filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(stem) => format!("{stem}.png"),
        None => "image.png".to_string(),
    }
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
        if let Some(size) = resolve_requested_image_size(config, request.size.as_deref(), false) {
            map.insert("size".to_string(), json!(size));
        }
        // 文生图以前完全不转发 output_format，导致“要 webp 拿到 png”只能靠猜。
        if let Some(output_format) = request
            .output_format
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            map.insert(
                "output_format".to_string(),
                json!(normalize_image_output_format(Some(output_format))),
            );
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
        // Default to 30 inference steps for image generation if not specified
        let num_inference_steps = request
            .num_inference_steps
            .or(config.image_num_inference_steps)
            .unwrap_or(30);
        if num_inference_steps > 0 {
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
        if let Some(true_cfg_scale) = request
            .true_cfg_scale
            .filter(|value| value.is_finite() && *value >= 0.0)
        {
            map.insert("true_cfg_scale".to_string(), json!(true_cfg_scale));
        }
        if let Some(output_compression) = request.output_compression.filter(|value| *value <= 100) {
            map.insert("output_compression".to_string(), json!(output_compression));
        }
        if let Some(layers) = request.layers.filter(|value| *value > 0) {
            map.insert("layers".to_string(), json!(layers));
        }
        if let Some(resolution) = request.resolution.filter(|value| *value > 0) {
            map.insert("resolution".to_string(), json!(resolution));
        }
    }
    payload
}

fn build_image_edit_form(
    config: &LlmModelConfig,
    model: &str,
    request: &ImageGenerationRequest,
) -> Result<Form> {
    let mut form = Form::new()
        .text("model", model.to_string())
        .text("prompt", request.prompt.clone())
        .text("response_format", "b64_json".to_string());
    let output_format = normalize_image_output_format(
        request
            .output_format
            .as_deref()
            .or(config.image_output_format.as_deref()),
    );
    form = form.text("output_format", output_format);
    let size = resolve_requested_image_size(config, request.size.as_deref(), true)
        .unwrap_or_else(|| "auto".to_string());
    form = form.text("size", size);
    let image_field = if request.input_images.len() > 1 {
        "image[]"
    } else {
        "image"
    };
    for input in &request.input_images {
        form = form.part(image_field, image_file_part(input)?);
    }
    if let Some(mask) = &request.mask_image {
        form = form.part("mask_image", image_file_part(mask)?);
    }
    if let Some(reference) = &request.reference_image {
        form = form.part("reference_image", image_file_part(reference)?);
    }
    if let Some(negative_prompt) = request
        .negative_prompt
        .as_deref()
        .or(config.image_negative_prompt.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        form = form.text("negative_prompt", negative_prompt.to_string());
    }
    let num_inference_steps = request
        .num_inference_steps
        .or(config.image_num_inference_steps)
        .unwrap_or(30);
    if num_inference_steps > 0 {
        form = form.text("num_inference_steps", num_inference_steps.to_string());
    }
    if let Some(guidance_scale) = request
        .guidance_scale
        .or(config.image_guidance_scale)
        .filter(|value| value.is_finite() && *value > 0.0)
    {
        form = form.text("guidance_scale", guidance_scale.to_string());
    }
    if let Some(seed) = request.seed {
        form = form.text("seed", seed.to_string());
    }
    // 模型没给 strength 时用模型配置里的默认值：调参项由部署方定，
    // 不该由模型每轮现猜一个数字。
    if let Some(strength) = request
        .strength
        .or(config.image_strength)
        .filter(|value| value.is_finite() && *value >= 0.0)
    {
        form = form.text("strength", strength.to_string());
    }
    if let Some(true_cfg_scale) = request
        .true_cfg_scale
        .filter(|value| value.is_finite() && *value >= 0.0)
    {
        form = form.text("true_cfg_scale", true_cfg_scale.to_string());
    }
    if let Some(output_compression) = request.output_compression.filter(|value| *value <= 100) {
        form = form.text("output_compression", output_compression.to_string());
    }
    if let Some(layers) = request.layers.filter(|value| *value > 0) {
        form = form.text("layers", layers.to_string());
    }
    if let Some(resolution) = request.resolution.filter(|value| *value > 0) {
        form = form.text("resolution", resolution.to_string());
    }
    Ok(form)
}

fn image_file_part(input: &ImageInputFile) -> Result<Part> {
    let mut part = Part::bytes(input.bytes.to_vec()).file_name(input.filename.clone());
    if !input.content_type.trim().is_empty() {
        part = part
            .mime_str(input.content_type.trim())
            .context("image input mime type is invalid")?;
    }
    Ok(part)
}

pub fn image_input_file_from_bytes(
    bytes: Bytes,
    filename: impl Into<String>,
    content_type: Option<&str>,
) -> ImageInputFile {
    let filename = filename.into();
    let resolved_content_type = content_type
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| detect_image_content_type_from_filename(&filename).map(ToString::to_string))
        .unwrap_or_else(|| "application/octet-stream".to_string());
    ImageInputFile {
        bytes,
        filename,
        content_type: resolved_content_type,
    }
}

fn detect_image_content_type_from_filename(filename: &str) -> Option<&'static str> {
    let extension = Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        "bmp" => Some("image/bmp"),
        "tif" | "tiff" => Some("image/tiff"),
        _ => None,
    }
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

#[derive(Debug)]
struct WhisperCppAudioUpload {
    audio_bytes: Vec<u8>,
    filename: String,
    content_type: String,
}

async fn prepare_whisper_cpp_audio_upload(
    request: &AudioTranscriptionRequest,
) -> Result<WhisperCppAudioUpload> {
    if is_wav_audio_bytes(request.audio_bytes.as_ref()) {
        return Ok(WhisperCppAudioUpload {
            audio_bytes: request.audio_bytes.to_vec(),
            filename: ensure_extension(&request.filename, "wav"),
            content_type: "audio/wav".to_string(),
        });
    }

    let converted = convert_audio_bytes_to_wav(
        request.audio_bytes.as_ref(),
        &request.filename,
        request.content_type.as_str(),
    )
    .await
    .context("convert whisper.cpp audio input to wav")?;
    Ok(WhisperCppAudioUpload {
        audio_bytes: converted,
        filename: replace_extension(&request.filename, "wav"),
        content_type: "audio/wav".to_string(),
    })
}

fn is_wav_audio_bytes(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE"
}

fn ensure_extension(filename: &str, extension: &str) -> String {
    let trimmed = filename.trim();
    if Path::new(trimmed)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case(extension))
        .unwrap_or(false)
    {
        return trimmed.to_string();
    }
    replace_extension(trimmed, extension)
}

fn replace_extension(filename: &str, extension: &str) -> String {
    let trimmed = filename.trim();
    let stem = Path::new(trimmed)
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("audio");
    format!("{stem}.{extension}")
}

async fn convert_audio_bytes_to_wav(
    bytes: &[u8],
    filename: &str,
    content_type: &str,
) -> Result<Vec<u8>> {
    let temp_root =
        std::env::temp_dir().join(format!("wunder_asr_{}", uuid::Uuid::new_v4().simple()));
    tokio::fs::create_dir_all(&temp_root)
        .await
        .context("create asr temp dir")?;
    let input_extension = audio_extension_for_upload(filename, content_type);
    let input_path = temp_root.join(format!("input.{input_extension}"));
    let output_path = temp_root.join("output.wav");
    let cleanup_root = temp_root.clone();
    let result = async {
        tokio::fs::write(&input_path, bytes)
            .await
            .context("write asr temp input")?;
        let binary = resolve_ffmpeg_binary();
        let mut command = Command::new(&binary);
        command.args(vec![
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
            "-y".to_string(),
            "-i".to_string(),
            input_path.to_string_lossy().to_string(),
            "-vn".to_string(),
            "-ac".to_string(),
            "1".to_string(),
            "-ar".to_string(),
            "16000".to_string(),
            "-f".to_string(),
            "wav".to_string(),
            output_path.to_string_lossy().to_string(),
        ]);
        apply_platform_spawn_options(&mut command);
        let output = command.output().await.map_err(|err| {
            if is_not_found_error(&err) {
                anyhow!(
                    "required executable `{binary}` not found; set {FFMPEG_BIN_ENV} or install ffmpeg"
                )
            } else {
                anyhow!("failed to start `{binary}`: {err}")
            }
        })?;
        if !output.status.success() {
            return Err(anyhow!(
                "ffmpeg audio conversion failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let converted = tokio::fs::read(&output_path)
            .await
            .context("read converted asr wav")?;
        if !is_wav_audio_bytes(&converted) {
            return Err(anyhow!("converted asr audio is not wav"));
        }
        Ok(converted)
    }
    .await;
    let _ = tokio::fs::remove_dir_all(cleanup_root).await;
    result
}

fn resolve_ffmpeg_binary() -> String {
    std::env::var(FFMPEG_BIN_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "ffmpeg".to_string())
}

fn audio_extension_for_upload(filename: &str, content_type: &str) -> &'static str {
    let lowered_mime = content_type.trim().to_ascii_lowercase();
    if lowered_mime.contains("webm") {
        return "webm";
    }
    if lowered_mime.contains("ogg") || lowered_mime.contains("opus") {
        return "ogg";
    }
    if lowered_mime.contains("mp4") || lowered_mime.contains("m4a") {
        return "m4a";
    }
    if lowered_mime.contains("flac") {
        return "flac";
    }
    if lowered_mime.contains("aac") {
        return "aac";
    }
    if lowered_mime.contains("mpeg") || lowered_mime.contains("mp3") {
        return "mp3";
    }
    match Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "webm" => "webm",
        "ogg" | "opus" => "ogg",
        "m4a" | "mp4" => "m4a",
        "flac" => "flac",
        "aac" => "aac",
        "mp3" => "mp3",
        _ => "audio",
    }
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
        build_image_payload, build_speech_payload, image_input_file_from_bytes,
        list_asr_model_names, list_image_model_names, list_tts_model_names, list_video_model_names,
        normalize_asr_response_format, resolve_asr_model, resolve_image_model, resolve_tts_model,
        resolve_video_model,
    };
    use crate::config::{Config, LlmModelConfig};
    use bytes::Bytes;
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
            ref_audio: None,
            ref_text: None,
            model_specific_params: None,
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
            ref_audio: None,
            ref_text: None,
            model_specific_params: None,
        };

        let payload = build_speech_payload(&config, "tts-model", &request, "wav", Some("Vivian"));

        assert_eq!(payload.get("voice").and_then(Value::as_str), Some("Vivian"));
    }

    #[test]
    fn tts_payload_forwards_voice_clone_and_model_specific_params() {
        let config = model("tts");
        let request = super::SpeechSynthesisRequest {
            text: "hello".to_string(),
            model_name: None,
            voice: Some("default".to_string()),
            instructions: None,
            response_format: Some("wav".to_string()),
            speed: Some(1.25),
            ref_audio: Some("data:audio/wav;base64,AAAA".to_string()),
            ref_text: Some("reference".to_string()),
            model_specific_params: Some(serde_json::json!({
                "task_type": "Base",
                "x_vector_only_mode": false,
                "voice": "should_be_ignored"
            })),
        };

        let payload = build_speech_payload(&config, "tts-model", &request, "wav", Some("Vivian"));

        assert_eq!(
            payload.get("ref_audio").and_then(Value::as_str),
            Some("data:audio/wav;base64,AAAA")
        );
        assert_eq!(
            payload.get("ref_text").and_then(Value::as_str),
            Some("reference")
        );
        assert_eq!(
            payload.get("task_type").and_then(Value::as_str),
            Some("Base")
        );
        assert_eq!(
            payload.get("x_vector_only_mode").and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(payload.get("voice").and_then(Value::as_str), Some("Vivian"));
    }

    #[test]
    fn image_payload_forwards_omni_extensions_without_inputs() {
        let mut config = model("image");
        config.image_default_size = Some("768x512".to_string());
        let request = super::ImageGenerationRequest {
            prompt: "draw".to_string(),
            model_name: None,
            size: None,
            output_format: Some("webp".to_string()),
            negative_prompt: Some("blur".to_string()),
            num_inference_steps: Some(12),
            guidance_scale: Some(5.5),
            seed: Some(42),
            input_images: Vec::new(),
            mask_image: None,
            reference_image: None,
            strength: Some(0.7),
            true_cfg_scale: Some(3.0),
            output_compression: Some(90),
            layers: Some(4),
            resolution: Some(1024),
        };

        let payload = build_image_payload(&config, "image-model", &request);

        assert_eq!(payload["model"], "image-model");
        assert_eq!(payload["prompt"], "draw");
        assert_eq!(payload["size"], "768x512");
        assert_eq!(payload["negative_prompt"], "blur");
        assert_eq!(payload["num_inference_steps"], 12);
        assert_eq!(payload["guidance_scale"], 5.5);
        assert_eq!(payload["seed"], 42);
        assert_eq!(payload["true_cfg_scale"], 3.0);
        assert_eq!(payload["output_compression"], 90);
        assert_eq!(payload["layers"], 4);
        assert_eq!(payload["resolution"], 1024);
    }

    #[test]
    fn image_payload_scales_request_when_it_exceeds_max() {
        let mut config = model("image");
        config.image_default_size = Some("768x512".to_string());
        let request = super::ImageGenerationRequest {
            prompt: "draw".to_string(),
            model_name: None,
            size: Some("1536x864".to_string()),
            output_format: None,
            negative_prompt: None,
            num_inference_steps: None,
            guidance_scale: None,
            seed: None,
            input_images: Vec::new(),
            mask_image: None,
            reference_image: None,
            strength: None,
            true_cfg_scale: None,
            output_compression: None,
            layers: None,
            resolution: None,
        };

        let payload = build_image_payload(&config, "image-model", &request);

        // 默认上限 1024x1024，1536x864 等比缩到 1024x576，比例不变。
        assert_eq!(payload["size"], "1024x576");
    }

    #[test]
    fn image_size_resolution_uses_default_and_max() {
        let mut config = model("image");
        assert_eq!(
            super::resolve_requested_image_size(&config, None, false).as_deref(),
            Some("1024x1024")
        );
        assert_eq!(
            super::resolve_requested_image_size(&config, None, true),
            None
        );
        assert_eq!(
            super::resolve_requested_image_size(&config, Some("768x512"), false).as_deref(),
            Some("768x512")
        );
        // 默认上限 1024x1024：超限时按最大边等比缩小，比例不变。
        assert_eq!(
            super::resolve_requested_image_size(&config, Some("1536x864"), false).as_deref(),
            Some("1024x576")
        );
        assert_eq!(
            super::resolve_requested_image_size(&config, Some("1024x1280"), false).as_deref(),
            Some("819x1024")
        );
        config.image_default_size = Some("768x512".to_string());
        config.image_max_size = Some("2048x2048".to_string());
        assert_eq!(
            super::resolve_requested_image_size(&config, Some("1536x864"), false).as_deref(),
            Some("1536x864")
        );
        assert_eq!(
            super::resolve_requested_image_size(&config, Some("4096x864"), false).as_deref(),
            Some("2048x432")
        );
        assert_eq!(
            super::resolve_requested_image_size(&config, Some("auto"), false).as_deref(),
            Some("auto")
        );
    }

    #[test]
    fn default_size_above_the_configured_max_is_scaled_down() {
        let mut config = model("image");
        config.image_default_size = Some("2048x1024".to_string());
        config.image_max_size = Some("1024x1024".to_string());
        assert_eq!(
            super::resolve_requested_image_size(&config, None, false).as_deref(),
            Some("1024x512")
        );
    }

    #[test]
    fn image_size_scaling_keeps_aspect_and_never_upscales() {
        assert_eq!(
            super::fit_image_size_within(1536, 864, 1024, 1024),
            (1024, 576)
        );
        assert_eq!(
            super::fit_image_size_within(800, 600, 1024, 1024),
            (800, 600)
        );
        assert_eq!(
            super::fit_image_size_within(4096, 4096, 1024, 1024),
            (1024, 1024)
        );
        assert_eq!(
            super::fit_image_size_within(2000, 400, 1024, 1024),
            (1024, 205)
        );
    }

    #[test]
    fn vllm_omni_scaled_size_stays_aligned_and_within_max() {
        let mut config = model("image");
        config.provider = Some("vllm_omni".to_string());
        config.image_max_size = Some("1920x1080".to_string());
        // 等比缩到 1920x1080 后再向小对齐到 16 的倍数，仍然不超过上限。
        assert_eq!(
            super::resolve_requested_image_size(&config, Some("3840x2160"), false).as_deref(),
            Some("1920x1072")
        );
    }

    #[test]
    fn prepare_image_request_downscales_edit_inputs_within_max() {
        let mut config = model("image");
        config.image_max_size = Some("1024x1024".to_string());
        let source = image_input_file_from_bytes(
            Bytes::from(encode_solid_png(3000, 1500)),
            "source.png",
            Some("image/png"),
        );
        let mask = image_input_file_from_bytes(
            Bytes::from(encode_solid_png(3000, 1500)),
            "mask.png",
            Some("image/png"),
        );
        let mut request = super::ImageGenerationRequest {
            prompt: "remove the background".to_string(),
            model_name: None,
            size: None,
            output_format: None,
            negative_prompt: None,
            num_inference_steps: None,
            guidance_scale: None,
            seed: None,
            input_images: vec![source],
            mask_image: Some(mask),
            reference_image: None,
            strength: None,
            true_cfg_scale: None,
            output_compression: None,
            layers: None,
            resolution: None,
        };

        let prepared = super::prepare_image_request(&config, &mut request).expect("prepare");

        // 3000x1500 按最大边缩到 1024x512，比例保持。
        assert_eq!(prepared.size.as_deref(), Some("1024x512"));
        assert_eq!(
            prepared.resized_input.as_deref(),
            Some("3000x1500->1024x512")
        );
        assert_eq!(request.size.as_deref(), Some("1024x512"));
        let resized = image::load_from_memory(&request.input_images[0].bytes).expect("decode");
        assert_eq!((resized.width(), resized.height()), (1024, 512));
        assert_eq!(request.input_images[0].content_type, "image/png");
        let mask = request.mask_image.expect("mask");
        let mask_image = image::load_from_memory(&mask.bytes).expect("decode mask");
        assert_eq!((mask_image.width(), mask_image.height()), (1024, 512));
        // 蒙版被隐式缩放必须回报，否则模型以为上游看到的是原尺寸蒙版。
        assert_eq!(
            prepared.resized_mask.as_deref(),
            Some("3000x1500->1024x512")
        );
    }

    #[test]
    fn prepare_image_request_keeps_small_edit_inputs_untouched() {
        let mut config = model("image");
        config.image_max_size = Some("1024x1024".to_string());
        let source = image_input_file_from_bytes(
            Bytes::from(encode_solid_png(640, 480)),
            "small.png",
            Some("image/png"),
        );
        let original_bytes = source.bytes.clone();
        let mut request = super::ImageGenerationRequest {
            prompt: "sharpen".to_string(),
            model_name: None,
            size: None,
            output_format: None,
            negative_prompt: None,
            num_inference_steps: None,
            guidance_scale: None,
            seed: None,
            input_images: vec![source],
            mask_image: None,
            reference_image: None,
            strength: None,
            true_cfg_scale: None,
            output_compression: None,
            layers: None,
            resolution: None,
        };

        let prepared = super::prepare_image_request(&config, &mut request).expect("prepare");

        // 未超限：不改字节、不改尺寸语义（保持 auto）。
        assert_eq!(prepared.size, None);
        assert_eq!(prepared.resized_input, None);
        assert_eq!(request.input_images[0].bytes, original_bytes);
        assert_eq!(request.size, None);
    }

    #[test]
    fn prepare_image_request_rejects_mask_with_mismatched_size() {
        let mut config = model("image");
        config.image_max_size = Some("1024x1024".to_string());
        let source = image_input_file_from_bytes(
            Bytes::from(encode_solid_png(2000, 1000)),
            "source.png",
            Some("image/png"),
        );
        let mask = image_input_file_from_bytes(
            Bytes::from(encode_solid_png(800, 800)),
            "mask.png",
            Some("image/png"),
        );
        let mut request = super::ImageGenerationRequest {
            prompt: "remove the sign".to_string(),
            model_name: None,
            size: None,
            output_format: None,
            negative_prompt: None,
            num_inference_steps: None,
            guidance_scale: None,
            seed: None,
            input_images: vec![source],
            mask_image: Some(mask),
            reference_image: None,
            strength: None,
            true_cfg_scale: None,
            output_compression: None,
            layers: None,
            resolution: None,
        };

        let err = super::prepare_image_request(&config, &mut request)
            .expect_err("mask size mismatch must be rejected");

        assert!(
            err.to_string().contains("must match the input image size"),
            "{err}"
        );
    }

    #[test]
    fn prepare_image_request_without_configured_max_keeps_source_size() {
        let config = model("image");
        assert!(config.image_max_size.is_none());
        let source = image_input_file_from_bytes(
            Bytes::from(encode_solid_png(2000, 1000)),
            "big.png",
            Some("image/png"),
        );
        let original_bytes = source.bytes.clone();
        let mut request = super::ImageGenerationRequest {
            prompt: "add a hat".to_string(),
            model_name: None,
            size: None,
            output_format: None,
            negative_prompt: None,
            num_inference_steps: None,
            guidance_scale: None,
            seed: None,
            input_images: vec![source],
            mask_image: None,
            reference_image: None,
            strength: None,
            true_cfg_scale: None,
            output_compression: None,
            layers: None,
            resolution: None,
        };

        let prepared = super::prepare_image_request(&config, &mut request).expect("prepare");

        // 没配上限就不动输入图，也不改尺寸语义。
        assert_eq!(prepared.size, None);
        assert_eq!(prepared.resized_input, None);
        assert_eq!(request.input_images[0].bytes, original_bytes);
    }

    fn encode_solid_png(width: u32, height: u32) -> Vec<u8> {
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            width,
            height,
            image::Rgba([200, 120, 60, 255]),
        ));
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("encode png");
        bytes
    }

    #[test]
    fn image_input_file_infers_mime_from_filename() {
        let file = image_input_file_from_bytes(Bytes::from_static(b"abc"), "source.webp", None);

        assert_eq!(file.filename, "source.webp");
        assert_eq!(file.content_type, "image/webp");
        assert_eq!(file.bytes, Bytes::from_static(b"abc"));
    }

    #[test]
    fn parse_image_generation_response_surfaces_provider_error() {
        let err = super::parse_image_generation_response(
            r#"{"error":{"message":"Diffusion generation failed: Height must be divisible by 16 (got 1080)."}} "#,
            Some("png"),
        )
        .expect_err("expected provider error");
        assert!(err.to_string().contains("Height must be divisible by 16"));
    }

    #[test]
    fn format_image_http_error_prefers_actionable_tail() {
        let message = super::format_image_http_error(
            "image request failed",
            reqwest::StatusCode::BAD_REQUEST,
            "Traceback...\nRuntimeError: Diffusion generation failed: Height must be divisible by 16 (got 1080).",
        );
        assert!(message.contains("Height must be divisible by 16"));
    }
}
