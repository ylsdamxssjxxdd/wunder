use super::{build_model_tool_success, ToolContext};
use crate::services::multimodal_models::{
    self, AudioTranscriptionRequest, ImageGenerationRequest, SpeechSynthesisRequest,
    VideoGenerationRequest,
};
use anyhow::{anyhow, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
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
    let input_paths = collect_image_input_paths(&payload);
    let input_images = load_image_input_files(context, &input_paths).await?;
    let mask_image = load_optional_image_input_file(context, payload.mask_path.as_deref()).await?;
    let reference_image =
        load_optional_image_input_file(context, payload.reference_path.as_deref()).await?;
    let result = multimodal_models::generate_image(
        context.config,
        ImageGenerationRequest {
            prompt: prompt.to_string(),
            model_name: normalize_optional_string(payload.model_name.as_deref()),
            size: normalize_optional_string(payload.size.as_deref()),
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
    .await?;
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
            "mode": if input_paths.is_empty() { "text_to_image" } else { "image_edit" },
            "input_paths": input_paths,
            "mask_path": normalize_optional_string(payload.mask_path.as_deref()),
            "reference_path": normalize_optional_string(payload.reference_path.as_deref()),
        }),
    ))
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
        return Ok(Some(ref_audio));
    }
    let Some(reference_path) = reference_path else {
        return Ok(None);
    };
    let resolved = context
        .workspace
        .resolve_path(context.workspace_id, reference_path)?;
    if !resolved.exists() || !resolved.is_file() {
        return Err(anyhow!("reference audio file not found: {reference_path}"));
    }
    let bytes = fs::read(&resolved).await?;
    if bytes.is_empty() {
        return Err(anyhow!("reference audio file is empty: {reference_path}"));
    }
    let mime = audio_content_type_from_path(&resolved);
    Ok(Some(format!(
        "data:{mime};base64,{}",
        STANDARD.encode(bytes)
    )))
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
