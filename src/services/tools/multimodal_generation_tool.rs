use super::{build_model_tool_success, ToolContext};
use crate::services::multimodal_models::{
    self, AudioTranscriptionRequest, ImageGenerationRequest, SpeechSynthesisRequest,
    VideoGenerationRequest,
};
use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::fs;

pub const TOOL_GENERATE_SPEECH: &str = "语音生成";
pub const TOOL_TRANSCRIBE_SPEECH: &str = "语音转文";
pub const TOOL_GENERATE_IMAGE: &str = "绘图生成";
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
    let result = multimodal_models::synthesize_speech(
        context.config,
        SpeechSynthesisRequest {
            text: text.to_string(),
            model_name: normalize_optional_string(payload.model_name.as_deref()),
            voice: normalize_optional_string(payload.voice.as_deref()),
            instructions: normalize_optional_string(payload.instructions.as_deref()),
            response_format: normalize_optional_string(payload.response_format.as_deref()),
            speed: payload.speed,
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
        }),
    ))
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
    let target = context.workspace.resolve_path(context.workspace_id, &relative)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).await?;
    }
    fs::write(&target, bytes).await?;
    context.workspace.mark_tree_dirty(context.workspace_id);
    Ok(PersistedMediaFile {
        public_path: context.workspace.display_path(context.workspace_id, &target),
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
