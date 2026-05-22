use super::tool_error::{build_failed_tool_result, ToolErrorMeta};
use super::{build_model_tool_success, collect_read_roots, ToolContext};
use crate::config::LlmModelConfig;
use crate::llm::normalize_provider;
use crate::path_utils::{is_within_root, normalize_target_path};
use crate::services::multimodal_models::{
    self, AudioTranscriptionRequest, ImageGenerationRequest, SpeechSynthesisRequest,
    VideoGenerationRequest,
};
use anyhow::{anyhow, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use tokio::fs;

pub const TOOL_GENERATE_SPEECH: &str = "语音生成";
pub const TOOL_TRANSCRIBE_SPEECH: &str = "声转文";
pub const TOOL_GENERATE_IMAGE: &str = "图像生成";
pub const TOOL_GENERATE_IMAGE_LEGACY: &str = "绘图生成";
pub const TOOL_GENERATE_VIDEO: &str = "视频生成";

pub const TOOL_GENERATE_SPEECH_ALIAS: &str = "generate_speech";
pub const TOOL_TRANSCRIBE_SPEECH_ALIAS: &str = "transcribe_speech";
pub const TOOL_GENERATE_IMAGE_ALIAS: &str = "generate_image";
pub const TOOL_GENERATE_VIDEO_ALIAS: &str = "generate_video";

const GENERATED_MEDIA_DIR: &str = "generated_media";
const VLLM_OMNI_PROVIDER: &str = "vllm_omni";
const VLLM_OMNI_IMAGE_ALIGNMENT: u32 = 16;

#[derive(Debug, Deserialize)]
pub struct GenerateSpeechArgs {
    pub text: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub model_name: Option<String>,
    #[serde(default)]
    pub voice: Option<String>,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub response_format: Option<String>,
    #[serde(default)]
    pub speed: Option<f32>,
    #[serde(default)]
    pub reference_path: Option<String>,
    #[serde(default)]
    pub ref_audio: Option<String>,
    #[serde(default)]
    pub ref_text: Option<String>,
    #[serde(default)]
    pub model_specific_params: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct TranscribeSpeechArgs {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub source_public_path: Option<String>,
    #[serde(default)]
    pub model_name: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub response_format: Option<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GenerateImageArgs {
    pub prompt: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub model_name: Option<String>,
    #[serde(default)]
    pub size: Option<String>,
    #[serde(default)]
    pub output_format: Option<String>,
    #[serde(default)]
    pub negative_prompt: Option<String>,
    #[serde(default)]
    pub num_inference_steps: Option<u32>,
    #[serde(default)]
    pub guidance_scale: Option<f32>,
    #[serde(default)]
    pub seed: Option<u64>,
    #[serde(default)]
    pub input_path: Option<String>,
    #[serde(default)]
    pub input_paths: Option<Vec<String>>,
    #[serde(default)]
    pub mask_path: Option<String>,
    #[serde(default)]
    pub reference_path: Option<String>,
    #[serde(default)]
    pub strength: Option<f32>,
    #[serde(default)]
    pub true_cfg_scale: Option<f32>,
    #[serde(default)]
    pub output_compression: Option<u32>,
    #[serde(default)]
    pub layers: Option<u32>,
    #[serde(default)]
    pub resolution: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct GenerateVideoArgs {
    pub prompt: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub model_name: Option<String>,
    #[serde(default)]
    pub size: Option<String>,
    #[serde(default)]
    pub seconds: Option<f32>,
    #[serde(default)]
    pub fps: Option<u32>,
    #[serde(default)]
    pub num_frames: Option<u32>,
    #[serde(default)]
    pub negative_prompt: Option<String>,
    #[serde(default)]
    pub num_inference_steps: Option<u32>,
    #[serde(default)]
    pub guidance_scale: Option<f32>,
    #[serde(default)]
    pub guidance_scale_2: Option<f32>,
    #[serde(default)]
    pub boundary_ratio: Option<f32>,
    #[serde(default)]
    pub flow_shift: Option<f32>,
    #[serde(default)]
    pub seed: Option<u64>,
    #[serde(default)]
    pub enable_frame_interpolation: Option<bool>,
}

pub fn speech_tool_available(config: &crate::config::Config) -> bool {
    !multimodal_models::list_tts_model_names(config).is_empty()
}

pub fn transcribe_tool_available(config: &crate::config::Config) -> bool {
    !multimodal_models::list_asr_model_names(config).is_empty()
}

pub fn image_tool_available(config: &crate::config::Config) -> bool {
    !multimodal_models::list_image_model_names(config).is_empty()
}

pub fn video_tool_available(config: &crate::config::Config) -> bool {
    !multimodal_models::list_video_model_names(config).is_empty()
}

pub async fn tool_generate_speech(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: GenerateSpeechArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let text = payload.text.trim();
    if text.is_empty() {
        return Err(anyhow!("text is required"));
    }
    let reference_path = normalize_optional_string(payload.reference_path.as_deref());
    let direct_ref_audio = normalize_optional_string(payload.ref_audio.as_deref());
    let ref_audio =
        resolve_reference_audio(context, reference_path.as_deref(), direct_ref_audio).await?;
    let ref_text = normalize_optional_string(payload.ref_text.as_deref());
    if ref_text.is_some() && ref_audio.is_none() {
        return Err(anyhow!(
            "ref_text requires reference_path or ref_audio for voice cloning"
        ));
    }
    let model_specific_params = match payload.model_specific_params {
        Some(Value::Object(map)) => Some(Value::Object(map)),
        Some(_) => return Err(anyhow!("model_specific_params must be a JSON object")),
        None => None,
    };
    let result = multimodal_models::synthesize_speech(
        context.config,
        SpeechSynthesisRequest {
            text: text.to_string(),
            model_name: normalize_optional_string(payload.model_name.as_deref()),
            voice: normalize_optional_string(payload.voice.as_deref()),
            instructions: normalize_optional_string(payload.instructions.as_deref()),
            response_format: normalize_optional_string(payload.response_format.as_deref()),
            speed: payload.speed,
            ref_audio: ref_audio.clone(),
            ref_text: ref_text.clone(),
            model_specific_params: model_specific_params.clone(),
        },
    )
    .await?;
    let extension = extension_from_content_type(&result.content_type, "wav");
    let saved = persist_generated_media(
        context,
        payload.path.as_deref(),
        "speech",
        &extension,
        &result.bytes,
    )
    .await?;
    Ok(build_model_tool_success(
        "generate_speech",
        "completed",
        format!("Generated speech and saved it to {}.", saved.public_path),
        json!({
            "text": text,
            "content_type": result.content_type,
            "path": saved.public_path,
            "workspace_relative_path": saved.workspace_relative_path,
            "bytes": saved.size_bytes,
            "reference_path": reference_path,
            "voice_clone": ref_audio.is_some(),
            "ref_text": ref_text,
            "model_specific_params": model_specific_params,
        }),
    ))
}

pub async fn tool_transcribe_speech(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: TranscribeSpeechArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let source_public_path = normalize_optional_string(payload.source_public_path.as_deref())
        .or_else(|| normalize_optional_string(payload.path.as_deref()))
        .ok_or_else(|| anyhow!("path or source_public_path is required"))?;
    let resolved_path = context
        .workspace
        .resolve_path(context.workspace_id, &source_public_path)?;
    if !resolved_path.exists() || !resolved_path.is_file() {
        return Err(anyhow!("audio source file not found"));
    }
    let filename = payload
        .filename
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            resolved_path
                .file_name()
                .and_then(|value| value.to_str())
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| "audio.wav".to_string());
    let content_type = payload
        .content_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let bytes = fs::read(&resolved_path).await?;
    let result = multimodal_models::transcribe_audio(
        context.config,
        AudioTranscriptionRequest {
            audio_bytes: bytes.into(),
            filename,
            content_type,
            model_name: normalize_optional_string(payload.model_name.as_deref()),
            language: normalize_optional_string(payload.language.as_deref()),
            prompt: normalize_optional_string(payload.prompt.as_deref()),
            response_format: normalize_optional_string(payload.response_format.as_deref()),
            temperature: payload.temperature,
        },
    )
    .await?;
    Ok(build_model_tool_success(
        "transcribe_speech",
        "completed",
        format!("Transcribed audio from {}.", source_public_path),
        json!({
            "text": result.text,
            "content_type": result.content_type,
            "source_public_path": source_public_path,
            "raw_response": result.raw_response,
        }),
    ))
}

pub async fn tool_generate_image(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: GenerateImageArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let prompt = payload.prompt.trim();
    if prompt.is_empty() {
        return Err(anyhow!("prompt is required"));
    }
    let (resolved_model_name, model_config) =
        match multimodal_models::resolve_image_model(context.config, payload.model_name.as_deref())
        {
            Some(model) => model,
            None => {
                return Ok(build_image_generation_failure_result(
                    None,
                    None,
                    "text_to_image",
                    payload.size.as_deref(),
                    None,
                    payload.mask_path.as_deref(),
                    payload.reference_path.as_deref(),
                    "image model is not configured",
                ));
            }
        };
    let effective_size = resolve_effective_image_size(&payload, &model_config);
    let input_paths = collect_image_input_paths(&payload);
    let mode = if input_paths.is_empty() {
        "text_to_image"
    } else {
        "image_edit"
    };
    if let Some(result) =
        validate_image_generation_request(&model_config, effective_size.as_deref(), mode)
    {
        return Ok(build_image_generation_failure_result(
            Some(&model_config),
            Some(resolved_model_name.as_str()),
            mode,
            effective_size.as_deref(),
            Some(&input_paths),
            payload.mask_path.as_deref(),
            payload.reference_path.as_deref(),
            &result,
        ));
    }
    let input_images = load_image_input_files(context, &input_paths).await?;
    let mask_image = load_optional_image_input_file(context, payload.mask_path.as_deref()).await?;
    let reference_image =
        load_optional_image_input_file(context, payload.reference_path.as_deref()).await?;
    let result = match multimodal_models::generate_image(
        context.config,
        ImageGenerationRequest {
            prompt: prompt.to_string(),
            model_name: Some(resolved_model_name.clone()),
            size: effective_size.clone(),
            output_format: normalize_optional_string(payload.output_format.as_deref()),
            negative_prompt: normalize_optional_string(payload.negative_prompt.as_deref()),
            num_inference_steps: payload.num_inference_steps,
            guidance_scale: payload.guidance_scale,
            seed: payload.seed,
            input_images,
            mask_image,
            reference_image,
            strength: payload.strength,
            true_cfg_scale: payload.true_cfg_scale,
            output_compression: payload.output_compression,
            layers: payload.layers,
            resolution: payload.resolution,
        },
    )
    .await
    {
        Ok(result) => result,
        Err(err) => {
            return Ok(build_image_generation_failure_result(
                Some(&model_config),
                Some(resolved_model_name.as_str()),
                mode,
                effective_size.as_deref(),
                Some(&input_paths),
                payload.mask_path.as_deref(),
                payload.reference_path.as_deref(),
                &err.to_string(),
            ));
        }
    };
    let extension = extension_from_content_type(&result.content_type, "png");
    let saved = persist_generated_media(
        context,
        payload.path.as_deref(),
        "image",
        &extension,
        &result.bytes,
    )
    .await?;
    Ok(build_model_tool_success(
        "generate_image",
        "completed",
        format!("Generated image and saved it to {}.", saved.public_path),
        json!({
            "prompt": prompt,
            "content_type": result.content_type,
            "path": saved.public_path,
            "workspace_relative_path": saved.workspace_relative_path,
            "bytes": saved.size_bytes,
            "mode": mode,
            "model_name": resolved_model_name,
            "provider": normalize_provider(model_config.provider.as_deref()),
            "size": effective_size,
            "input_paths": input_paths,
            "mask_path": normalize_optional_string(payload.mask_path.as_deref()),
            "reference_path": normalize_optional_string(payload.reference_path.as_deref()),
        }),
    ))
}

fn resolve_effective_image_size(
    payload: &GenerateImageArgs,
    model_config: &LlmModelConfig,
) -> Option<String> {
    normalize_optional_string(payload.size.as_deref())
        .or_else(|| normalize_optional_string(model_config.image_size.as_deref()))
}

fn validate_image_generation_request(
    model_config: &LlmModelConfig,
    requested_size: Option<&str>,
    mode: &str,
) -> Option<String> {
    let provider = normalize_provider(model_config.provider.as_deref());
    if provider != VLLM_OMNI_PROVIDER {
        return None;
    }
    let size = requested_size?.trim();
    if size.is_empty() {
        return None;
    }
    let (width, height) = parse_image_size(size).ok()?;
    if width % VLLM_OMNI_IMAGE_ALIGNMENT == 0 && height % VLLM_OMNI_IMAGE_ALIGNMENT == 0 {
        return None;
    }
    let mode_hint = if mode == "image_edit" {
        "图生图/编辑"
    } else {
        "文生图"
    };
    Some(format!(
        "vllm-omni {mode_hint} size `{size}` is invalid: width and height must both be divisible by {VLLM_OMNI_IMAGE_ALIGNMENT}."
    ))
}

fn parse_image_size(size: &str) -> std::result::Result<(u32, u32), ()> {
    let cleaned = size.trim().to_ascii_lowercase().replace(' ', "");
    let (width, height) = cleaned.split_once('x').ok_or(())?;
    let width = width.parse::<u32>().map_err(|_| ())?;
    let height = height.parse::<u32>().map_err(|_| ())?;
    if width == 0 || height == 0 {
        return Err(());
    }
    Ok((width, height))
}

fn build_image_generation_failure_result(
    model_config: Option<&LlmModelConfig>,
    model_name: Option<&str>,
    mode: &str,
    size: Option<&str>,
    input_paths: Option<&[String]>,
    mask_path: Option<&str>,
    reference_path: Option<&str>,
    error_text: &str,
) -> Value {
    let provider = model_config
        .map(|config| normalize_provider(config.provider.as_deref()))
        .unwrap_or_else(|| "unknown".to_string());
    let detail = summarize_image_error_detail(error_text);
    let (code, retryable, next_step_hint, alignment_suggestion) =
        classify_image_generation_failure(provider.as_str(), size, detail.as_str());
    let summary = if detail.is_empty() {
        "图像生成失败。".to_string()
    } else {
        format!("图像生成失败：{detail}")
    };
    build_failed_tool_result(
        summary,
        json!({
            "tool": "generate_image",
            "phase": "execution",
            "provider": provider,
            "model_name": model_name,
            "mode": mode,
            "size": size,
            "input_paths": input_paths,
            "mask_path": normalize_optional_string(mask_path),
            "reference_path": normalize_optional_string(reference_path),
            "failure_summary": detail,
            "error_detail_head": detail,
            "next_step_hint": next_step_hint,
            "suggested_size": alignment_suggestion,
        }),
        ToolErrorMeta::new(code, Some(next_step_hint), retryable, None),
        false,
    )
}

fn classify_image_generation_failure(
    provider: &str,
    size: Option<&str>,
    detail: &str,
) -> (&'static str, bool, String, Option<String>) {
    let lower = detail.to_ascii_lowercase();
    if contains_timeout_hint(detail) {
        return (
            "TOOL_TIMEOUT",
            true,
            "缩小生成范围、降低推理步数，或改成可拆分的生成流程后重试。".to_string(),
            None,
        );
    }
    if lower.contains("429")
        || lower.contains("too many requests")
        || lower.contains("temporarily unavailable")
        || lower.contains("service unavailable")
    {
        return (
            "IMAGE_PROVIDER_BUSY",
            true,
            "上游图像服务当前繁忙，稍后重试，或降低并发与推理步数。".to_string(),
            None,
        );
    }
    if provider == VLLM_OMNI_PROVIDER && lower.contains("divisible by 16") {
        let suggested_size = size
            .and_then(|value| parse_image_size(value).ok())
            .map(|(width, height)| suggest_aligned_image_size(width, height));
        let hint = match suggested_size.as_deref() {
            Some(candidate) => format!(
                "改用宽高都能被 16 整除的尺寸后重试，例如 `{candidate}`、`1024x1024` 或 `1344x768`。"
            ),
            None => {
                "改用宽高都能被 16 整除的尺寸后重试，例如 `1024x1024`、`1344x768` 或 `1920x1088`。"
                    .to_string()
            }
        };
        return ("IMAGE_SIZE_ALIGNMENT_INVALID", false, hint, suggested_size);
    }
    if lower.contains("image model is not configured") {
        return (
            "IMAGE_MODEL_NOT_CONFIGURED",
            false,
            "先在系统设置中配置默认图像模型，或为本次调用显式指定可用的 `model_name`。".to_string(),
            None,
        );
    }
    if lower.contains("missing b64_json")
        || lower.contains("parse image response json")
        || lower.contains("provider response")
    {
        return (
            "IMAGE_PROVIDER_RESPONSE_INVALID",
            false,
            "上游图像服务返回了非预期响应格式；请检查 vllm-omni 日志，并确认接口兼容 `/v1/images/generations` 或 `/v1/images/edits`。"
                .to_string(),
            None,
        );
    }
    if lower.contains("invalid")
        || lower.contains("unsupported")
        || lower.contains("must be")
        || lower.contains("is required")
    {
        return (
            "IMAGE_REQUEST_INVALID",
            false,
            "调整图像生成参数后重试，尤其检查 `size`、输入图、蒙版图和参考图。".to_string(),
            None,
        );
    }
    (
        "IMAGE_GENERATION_FAILED",
        false,
        "根据失败原因调整图像生成参数，必要时检查上游图像服务日志。".to_string(),
        None,
    )
}

fn summarize_image_error_detail(error_text: &str) -> String {
    let trimmed = error_text.trim();
    if trimmed.is_empty() {
        return "unknown image generation error".to_string();
    }
    let lines = trimmed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return "unknown image generation error".to_string();
    }
    for needle in [
        "RuntimeError:",
        "Diffusion generation failed:",
        "Height must be divisible by 16",
        "Width must be divisible by 16",
        "image generation failed:",
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
    if let Some(last) = lines.last() {
        return (*last).to_string();
    }
    trimmed.to_string()
}

fn contains_timeout_hint(message: &str) -> bool {
    let lower = message.trim().to_ascii_lowercase();
    lower.contains("timeout") || lower.contains("timed out") || lower.contains("time out")
}

fn suggest_aligned_image_size(width: u32, height: u32) -> String {
    let suggested_width = align_up_to_unit(width, VLLM_OMNI_IMAGE_ALIGNMENT);
    let suggested_height = align_up_to_unit(height, VLLM_OMNI_IMAGE_ALIGNMENT);
    format!("{suggested_width}x{suggested_height}")
}

fn align_up_to_unit(value: u32, unit: u32) -> u32 {
    if value % unit == 0 {
        value
    } else {
        value + (unit - value % unit)
    }
}

fn collect_image_input_paths(payload: &GenerateImageArgs) -> Vec<String> {
    let mut paths = Vec::new();
    if let Some(path) = normalize_optional_string(payload.input_path.as_deref()) {
        paths.push(path);
    }
    if let Some(items) = &payload.input_paths {
        paths.extend(
            items
                .iter()
                .filter_map(|item| normalize_optional_string(Some(item))),
        );
    }
    paths.sort();
    paths.dedup();
    paths
}

async fn load_image_input_files(
    context: &ToolContext<'_>,
    paths: &[String],
) -> Result<Vec<multimodal_models::ImageInputFile>> {
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        files.push(load_image_input_file(context, path).await?);
    }
    Ok(files)
}

async fn load_optional_image_input_file(
    context: &ToolContext<'_>,
    path: Option<&str>,
) -> Result<Option<multimodal_models::ImageInputFile>> {
    let Some(path) = normalize_optional_string(path) else {
        return Ok(None);
    };
    Ok(Some(load_image_input_file(context, &path).await?))
}

async fn load_image_input_file(
    context: &ToolContext<'_>,
    public_or_relative_path: &str,
) -> Result<multimodal_models::ImageInputFile> {
    let resolved = context
        .workspace
        .resolve_path(context.workspace_id, public_or_relative_path)?;
    if !resolved.exists() || !resolved.is_file() {
        return Err(anyhow!(
            "image input file not found: {public_or_relative_path}"
        ));
    }
    let bytes = fs::read(&resolved).await?;
    if bytes.is_empty() {
        return Err(anyhow!(
            "image input file is empty: {public_or_relative_path}"
        ));
    }
    let filename = resolved
        .file_name()
        .and_then(|value| value.to_str())
        .map(ToString::to_string)
        .unwrap_or_else(|| "image.png".to_string());
    Ok(multimodal_models::image_input_file_from_bytes(
        bytes.into(),
        filename,
        None,
    ))
}

async fn resolve_reference_audio(
    context: &ToolContext<'_>,
    reference_path: Option<&str>,
    direct_ref_audio: Option<String>,
) -> Result<Option<String>> {
    if let Some(ref_audio) = direct_ref_audio {
        if !is_direct_reference_audio_url(&ref_audio) {
            match resolve_reference_audio_path(context, &ref_audio) {
                Ok(resolved) => {
                    return build_reference_audio_data_url(&resolved, &ref_audio)
                        .await
                        .map(Some);
                }
                Err(err) if should_resolve_ref_audio_as_path(&ref_audio) => return Err(err),
                Err(_) => {}
            }
        }
        return Ok(Some(ref_audio));
    }
    let Some(reference_path) = reference_path else {
        return Ok(None);
    };
    let resolved = resolve_reference_audio_path(context, reference_path)?;
    build_reference_audio_data_url(&resolved, reference_path)
        .await
        .map(Some)
}

async fn build_reference_audio_data_url(resolved: &Path, reference_path: &str) -> Result<String> {
    let bytes = fs::read(&resolved).await?;
    if bytes.is_empty() {
        return Err(anyhow!("reference audio file is empty: {reference_path}"));
    }
    let mime = audio_content_type_from_path(&resolved);
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

fn resolve_reference_audio_path(
    context: &ToolContext<'_>,
    reference_path: &str,
) -> Result<PathBuf> {
    let read_roots = collect_read_roots(context);
    resolve_reference_audio_path_with_roots(
        context.workspace.as_ref(),
        context.workspace_id,
        reference_path,
        &read_roots,
    )
}

fn resolve_reference_audio_path_with_roots(
    workspace: &crate::workspace::WorkspaceManager,
    workspace_id: &str,
    reference_path: &str,
    read_roots: &[PathBuf],
) -> Result<PathBuf> {
    let path = normalize_reference_audio_path(reference_path);
    if path.starts_with("workspaces/") || path.starts_with("/workspaces/") {
        let resolved = workspace.resolve_path(workspace_id, &path)?;
        if resolved.exists() && resolved.is_file() {
            return Ok(resolved);
        }
        return Err(anyhow!("reference audio file not found: {reference_path}"));
    }
    if Path::new(&path).is_absolute() {
        if let Some(resolved) = resolve_existing_reference_audio_path(&path, read_roots) {
            return Ok(resolved);
        }
        return Err(anyhow!("reference audio file not found: {reference_path}"));
    }
    let resolved = workspace.resolve_path(workspace_id, &path)?;
    if resolved.exists() && resolved.is_file() {
        return Ok(resolved);
    }
    if let Some(resolved) = resolve_existing_reference_audio_path(&path, read_roots) {
        return Ok(resolved);
    }
    Err(anyhow!("reference audio file not found: {reference_path}"))
}

fn normalize_reference_audio_path(path: &str) -> String {
    let normalized = path.trim().replace('\\', "/");
    if let Some(rest) = normalized.strip_prefix("/workspaces/") {
        format!("workspaces/{rest}")
    } else {
        normalized
    }
}

fn should_resolve_ref_audio_as_path(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    if is_direct_reference_audio_url(trimmed) {
        return false;
    }
    let normalized = trimmed.replace('\\', "/");
    normalized.starts_with("/workspaces/")
        || normalized.starts_with("workspaces/")
        || normalized.starts_with("./")
        || normalized.starts_with("../")
        || normalized.contains('/')
        || Path::new(trimmed).is_absolute()
        || Path::new(trimmed)
            .extension()
            .and_then(|value| value.to_str())
            .map(|extension| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "mp3" | "wav" | "ogg" | "opus" | "aac" | "flac" | "m4a" | "webm"
                )
            })
            .unwrap_or(false)
}

fn resolve_existing_reference_audio_path(raw_path: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = Path::new(trimmed);
    if path.is_absolute() {
        for root in roots {
            if is_within_root(root, path) && path.exists() && path.is_file() {
                return Some(path.to_path_buf());
            }
        }
        return None;
    }
    let relative = sanitize_reference_audio_relative_path(trimmed)?;
    for root in roots {
        let candidate = normalize_target_path(&root.join(&relative));
        if candidate.exists() && candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn sanitize_reference_audio_relative_path(raw_path: &str) -> Option<PathBuf> {
    let normalized = raw_path.trim().replace('\\', "/");
    let stripped = normalized.strip_prefix("./").unwrap_or(&normalized);
    if stripped.is_empty() {
        return None;
    }
    let path = PathBuf::from(stripped);
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                return None;
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }
    Some(path)
}

fn is_direct_reference_audio_url(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    lower.starts_with("data:") || lower.starts_with("http://") || lower.starts_with("https://")
}

fn audio_content_type_from_path(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "opus" => "audio/opus",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        "m4a" => "audio/mp4",
        "webm" => "audio/webm",
        _ => "application/octet-stream",
    }
}

pub async fn tool_generate_video(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: GenerateVideoArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let prompt = payload.prompt.trim();
    if prompt.is_empty() {
        return Err(anyhow!("prompt is required"));
    }
    let result = multimodal_models::generate_video(
        context.config,
        VideoGenerationRequest {
            prompt: prompt.to_string(),
            model_name: normalize_optional_string(payload.model_name.as_deref()),
            size: normalize_optional_string(payload.size.as_deref()),
            seconds: payload.seconds,
            fps: payload.fps,
            num_frames: payload.num_frames,
            negative_prompt: normalize_optional_string(payload.negative_prompt.as_deref()),
            num_inference_steps: payload.num_inference_steps,
            guidance_scale: payload.guidance_scale,
            guidance_scale_2: payload.guidance_scale_2,
            boundary_ratio: payload.boundary_ratio,
            flow_shift: payload.flow_shift,
            seed: payload.seed,
            enable_frame_interpolation: payload.enable_frame_interpolation,
            sync_mode: Some(true),
        },
    )
    .await?;
    let extension = extension_from_content_type(&result.content_type, "mp4");
    let saved = persist_generated_media(
        context,
        payload.path.as_deref(),
        "video",
        &extension,
        &result.bytes,
    )
    .await?;
    Ok(build_model_tool_success(
        "generate_video",
        "completed",
        format!("Generated video and saved it to {}.", saved.public_path),
        json!({
            "prompt": prompt,
            "content_type": result.content_type,
            "path": saved.public_path,
            "workspace_relative_path": saved.workspace_relative_path,
            "bytes": saved.size_bytes,
        }),
    ))
}

struct PersistedMediaFile {
    public_path: String,
    workspace_relative_path: String,
    size_bytes: usize,
}

async fn persist_generated_media(
    context: &ToolContext<'_>,
    requested_path: Option<&str>,
    prefix: &str,
    extension: &str,
    bytes: &[u8],
) -> Result<PersistedMediaFile> {
    let relative = requested_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            let stamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
            format!("{GENERATED_MEDIA_DIR}/{prefix}_{stamp}.{extension}")
        });
    let relative = ensure_extension(relative, extension);
    let target = context
        .workspace
        .resolve_path(context.workspace_id, &relative)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).await?;
    }
    fs::write(&target, bytes).await?;
    context.workspace.mark_tree_dirty(context.workspace_id);
    Ok(PersistedMediaFile {
        public_path: context
            .workspace
            .display_path(context.workspace_id, &target),
        workspace_relative_path: relative.replace('\\', "/"),
        size_bytes: bytes.len(),
    })
}

fn normalize_optional_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn ensure_extension(path: String, extension: &str) -> String {
    let normalized = path.replace('\\', "/");
    let current_ext = std::path::Path::new(&normalized)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if current_ext == extension.to_ascii_lowercase() {
        normalized
    } else if current_ext.is_empty() {
        format!("{normalized}.{extension}")
    } else {
        normalized
    }
}

fn extension_from_content_type(content_type: &str, fallback: &str) -> String {
    let normalized = content_type
        .split(';')
        .next()
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    match normalized.as_str() {
        "audio/mpeg" => "mp3",
        "audio/flac" => "flac",
        "audio/aac" => "aac",
        "audio/opus" => "opus",
        "audio/l16" => "pcm",
        "audio/wav" => "wav",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/png" => "png",
        "video/mp4" => "mp4",
        _ => fallback,
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        build_image_generation_failure_result, parse_image_size,
        resolve_reference_audio_path_with_roots, should_resolve_ref_audio_as_path,
        suggest_aligned_image_size, validate_image_generation_request, GenerateImageArgs,
        VLLM_OMNI_PROVIDER,
    };
    use crate::config::LlmModelConfig;
    use crate::path_utils::normalize_path_for_compare;
    use crate::storage::SqliteStorage;
    use serde_json::Value;
    use std::collections::HashMap;
    use std::fs;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn image_model(provider: &str) -> LlmModelConfig {
        LlmModelConfig {
            enable: Some(true),
            provider: Some(provider.to_string()),
            model: Some("demo-image".to_string()),
            model_type: Some("image".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn parse_image_size_accepts_common_format() {
        assert_eq!(parse_image_size("1920x1080"), Ok((1920, 1080)));
        assert_eq!(parse_image_size(" 1024 x 1024 "), Ok((1024, 1024)));
    }

    #[test]
    fn vllm_omni_validation_rejects_unaligned_size() {
        let config = image_model(VLLM_OMNI_PROVIDER);
        let failure =
            validate_image_generation_request(&config, Some("1920x1080"), "text_to_image")
                .expect("expected validation failure");
        assert!(failure.contains("divisible by 16"));
    }

    #[test]
    fn aligned_size_suggestion_rounds_up() {
        assert_eq!(suggest_aligned_image_size(1920, 1080), "1920x1088");
    }

    #[test]
    fn structured_failure_marks_alignment_error_non_retryable() {
        let config = image_model(VLLM_OMNI_PROVIDER);
        let payload = build_image_generation_failure_result(
            Some(&config),
            Some("demo-image"),
            "text_to_image",
            Some("1920x1080"),
            None,
            None,
            None,
            "Diffusion generation failed: Height must be divisible by 16 (got 1080).",
        );
        assert_eq!(payload.get("ok").and_then(Value::as_bool), Some(false));
        assert_eq!(
            payload.pointer("/error_meta/code").and_then(Value::as_str),
            Some("IMAGE_SIZE_ALIGNMENT_INVALID")
        );
        assert_eq!(
            payload
                .pointer("/error_meta/retryable")
                .and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            payload
                .pointer("/data/suggested_size")
                .and_then(Value::as_str),
            Some("1920x1088")
        );
    }

    #[test]
    fn non_vllm_provider_skips_alignment_validation() {
        let config = image_model("openai_compatible");
        assert!(
            validate_image_generation_request(&config, Some("1920x1080"), "text_to_image")
                .is_none()
        );
    }

    #[test]
    fn payload_size_resolution_prefers_request_then_model_default() {
        let payload = GenerateImageArgs {
            prompt: "demo".to_string(),
            path: None,
            model_name: None,
            size: Some("1536x864".to_string()),
            output_format: None,
            negative_prompt: None,
            num_inference_steps: None,
            guidance_scale: None,
            seed: None,
            input_path: None,
            input_paths: None,
            mask_path: None,
            reference_path: None,
            strength: None,
            true_cfg_scale: None,
            output_compression: None,
            layers: None,
            resolution: None,
        };
        let mut config = image_model(VLLM_OMNI_PROVIDER);
        config.image_size = Some("1024x1024".to_string());
        assert_eq!(
            super::resolve_effective_image_size(&payload, &config).as_deref(),
            Some("1536x864")
        );

        let payload_without_size = GenerateImageArgs {
            size: None,
            ..payload
        };
        assert_eq!(
            super::resolve_effective_image_size(&payload_without_size, &config).as_deref(),
            Some("1024x1024")
        );
    }

    #[test]
    fn reference_audio_path_resolves_public_workspace_path() {
        let temp = tempdir().expect("tempdir");
        let workspace_root = temp.path().join("workspace");
        let storage = Arc::new(SqliteStorage::new(
            temp.path()
                .join("state.sqlite3")
                .to_string_lossy()
                .to_string(),
        ));
        let workspace = crate::workspace::WorkspaceManager::new(
            &workspace_root.to_string_lossy(),
            storage,
            0,
            &HashMap::new(),
        );
        let reference = workspace
            .resolve_path("alice__c__1", "refs/voice.wav")
            .expect("resolve target");
        fs::create_dir_all(reference.parent().expect("parent")).expect("mkdir");
        fs::write(&reference, b"demo").expect("write reference audio");

        let resolved = resolve_reference_audio_path_with_roots(
            &workspace,
            "alice__c__1",
            "/workspaces/alice__c__1/refs/voice.wav",
            &[],
        )
        .expect("resolved");

        assert_eq!(
            normalize_path_for_compare(&resolved),
            normalize_path_for_compare(&reference)
        );
    }

    #[test]
    fn reference_audio_path_resolves_existing_read_root_relative_file() {
        let temp = tempdir().expect("tempdir");
        let workspace_root = temp.path().join("workspace");
        let read_root = temp.path().join("workdir");
        let reference = read_root.join("refs").join("voice.wav");
        fs::create_dir_all(reference.parent().expect("parent")).expect("mkdir");
        fs::write(&reference, b"demo").expect("write reference audio");
        let storage = Arc::new(SqliteStorage::new(
            temp.path()
                .join("state.sqlite3")
                .to_string_lossy()
                .to_string(),
        ));
        let workspace = crate::workspace::WorkspaceManager::new(
            &workspace_root.to_string_lossy(),
            storage,
            0,
            &HashMap::new(),
        );

        let resolved = resolve_reference_audio_path_with_roots(
            &workspace,
            "alice__c__1",
            "refs/voice.wav",
            &[read_root],
        )
        .expect("resolved");

        assert_eq!(
            normalize_path_for_compare(&resolved),
            normalize_path_for_compare(&reference)
        );
    }

    #[test]
    fn ref_audio_path_detection_keeps_urls_as_direct_inputs() {
        assert!(should_resolve_ref_audio_as_path("refs/voice.wav"));
        assert!(should_resolve_ref_audio_as_path("/workspaces/u/voice.wav"));
        assert!(!should_resolve_ref_audio_as_path(
            "data:audio/wav;base64,AAAA"
        ));
        assert!(!should_resolve_ref_audio_as_path(
            "https://example.test/voice.wav"
        ));
    }
}
