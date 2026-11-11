use crate::config::{ChannelAsrConfig, Config};
use crate::core::blocking;
use crate::core::command_utils::{apply_platform_spawn_options, is_not_found_error};
use crate::schemas::AttachmentPayload;
use crate::services::chat_attachments::{
    is_supported_model_image_mime, parse_image_data_url, validate_image_attachment_bytes,
};
use crate::services::image_context::{ImageContextPolicy, ImageRestoreReport};
use crate::services::multimodal_models::{self, AudioTranscriptionRequest};
use crate::storage::USER_PRIVATE_CONTAINER_ID;
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use image::codecs::gif::GifDecoder;
use image::{AnimationDecoder, DynamicImage, ImageFormat};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::multipart::{Form, Part};
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::process::Command;
use tracing::warn;
use uuid::Uuid;

const CHAT_MEDIA_SOURCE_DIR: &str = "chat_media/source";
const CHAT_MEDIA_DERIVED_DIR: &str = "chat_media/derived";
const DEFAULT_VIDEO_FRAME_RATE: f64 = 1.0;
const MIN_VIDEO_FRAME_RATE: f64 = 0.1;
const MAX_VIDEO_FRAME_RATE: f64 = 12.0;
const MAX_VIDEO_FRAMES: usize = 120;
const DEFAULT_GIF_FRAME_STEP: usize = 0;
const MIN_GIF_FRAME_STEP: usize = 0;
const MAX_GIF_FRAME_STEP: usize = 120;
const REINLINE_IMAGE_MAX_BYTES: u64 = 64 * 1024 * 1024;
const DEFAULT_ASR_BASE_URL: &str = "https://api.openai.com/v1";
const FFMPEG_BIN_ENV: &str = "WUNDER_FFMPEG_BIN";
const FFPROBE_BIN_ENV: &str = "WUNDER_FFPROBE_BIN";
const IMAGE_MAGICK_BIN_ENV: &str = "WUNDER_IMAGE_MAGICK_BIN";

#[derive(Debug, Clone)]
pub struct ChatMediaUpload {
    pub filename: String,
    pub content_type: Option<String>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatMediaProcessResult {
    pub kind: String,
    pub name: String,
    pub source_public_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_frame_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied_frame_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_frame_step: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied_frame_step: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_frame_count: Option<usize>,
    #[serde(default)]
    pub frame_count: usize,
    #[serde(default)]
    pub has_audio: bool,
    #[serde(default)]
    pub attachments: Vec<AttachmentPayload>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MediaKind {
    Image,
    Audio,
    Video,
}

#[derive(Debug, Default)]
struct MediaProbe {
    duration_ms: Option<u64>,
    has_audio: bool,
    has_video: bool,
    total_frames: Option<usize>,
}

pub async fn process_chat_media_upload(
    workspace: &WorkspaceManager,
    config: &Config,
    user_id: &str,
    upload: ChatMediaUpload,
    requested_frame_rate: Option<f64>,
    requested_frame_step: Option<usize>,
) -> Result<ChatMediaProcessResult> {
    let filename = normalize_filename(upload.filename.as_str(), "media");
    let workspace_id = workspace.scoped_user_id_by_container(user_id, USER_PRIVATE_CONTAINER_ID);
    workspace.ensure_user_root(&workspace_id)?;
    let source_path = persist_source_upload(
        workspace,
        &workspace_id,
        filename.as_str(),
        upload.bytes.as_slice(),
    )
    .await?;
    process_visual_media_path(
        workspace,
        config,
        &workspace_id,
        &source_path,
        upload.content_type.as_deref(),
        requested_frame_rate,
        requested_frame_step,
    )
    .await
}

pub async fn reprocess_chat_media_source(
    workspace: &WorkspaceManager,
    config: &Config,
    user_id: &str,
    source_public_path: &str,
    requested_frame_rate: Option<f64>,
    requested_frame_step: Option<usize>,
) -> Result<ChatMediaProcessResult> {
    let workspace_id = workspace.scoped_user_id_by_container(user_id, USER_PRIVATE_CONTAINER_ID);
    workspace.ensure_user_root(&workspace_id)?;
    let source_path =
        resolve_private_media_source_path(workspace, &workspace_id, source_public_path)?;
    process_visual_media_path(
        workspace,
        config,
        &workspace_id,
        &source_path,
        None,
        requested_frame_rate,
        requested_frame_step,
    )
    .await
}

pub async fn process_visual_media_path(
    workspace: &WorkspaceManager,
    config: &Config,
    workspace_id: &str,
    source_path: &Path,
    content_type_hint: Option<&str>,
    requested_frame_rate: Option<f64>,
    requested_frame_step: Option<usize>,
) -> Result<ChatMediaProcessResult> {
    let filename = source_path
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "media".to_string());
    let source_public_path = workspace.display_path(workspace_id, source_path);
    let media_kind = detect_media_kind(filename.as_str(), content_type_hint)
        .ok_or_else(|| anyhow!("unsupported media type, expected image/audio/video"))?;
    match media_kind {
        MediaKind::Image => {
            process_image_source(workspace, workspace_id, source_path, requested_frame_step).await
        }
        MediaKind::Audio => {
            process_audio_source(config, source_path, filename.as_str(), source_public_path).await
        }
        MediaKind::Video => {
            process_video_source(
                workspace,
                config,
                workspace_id,
                source_path,
                filename.as_str(),
                source_public_path,
                requested_frame_rate,
                requested_frame_step,
            )
            .await
        }
    }
}

async fn process_audio_source(
    config: &Config,
    source_path: &Path,
    filename: &str,
    source_public_path: String,
) -> Result<ChatMediaProcessResult> {
    let probe = probe_media(source_path).await.unwrap_or_default();
    let content_type = detect_audio_content_type(filename);
    let (transcript, mut warnings) = transcribe_audio_file(
        &config.channels.media.asr,
        config,
        source_path,
        filename,
        content_type.as_str(),
    )
    .await;
    let content = transcript
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| build_audio_placeholder(filename));
    if transcript.is_none() {
        warnings.push("Audio transcript unavailable, using placeholder text context.".to_string());
    }
    Ok(ChatMediaProcessResult {
        kind: "audio".to_string(),
        name: filename.to_string(),
        source_public_path: source_public_path.clone(),
        transcript: transcript.clone(),
        duration_ms: probe.duration_ms,
        requested_frame_rate: None,
        applied_frame_rate: None,
        requested_frame_step: None,
        applied_frame_step: None,
        total_frame_count: None,
        frame_count: 0,
        has_audio: true,
        attachments: vec![AttachmentPayload {
            name: Some(filename.to_string()),
            content: Some(content),
            content_type: Some(content_type),
            public_path: Some(source_public_path),
        }],
        warnings,
    })
}

/// 默认读图路径（无 crop/scale/stats）也按 codex 高细节口径压缩：超过上限
/// 时落一份派生图作为入模附件，原图仍在工作区供 crop/scale 取细节；未超限
/// 返回 None 沿用原图，不做任何重编码。
async fn cap_still_image_for_model(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    filename: &str,
    mime: &str,
    bytes: &[u8],
) -> Result<Option<AttachmentPayload>> {
    let Some((source_width, source_height)) = image_dimensions(bytes) else {
        return Ok(None);
    };
    let Some((width, height)) = model_view_size(source_width, source_height) else {
        return Ok(None);
    };
    let bytes = bytes.to_vec();
    let prefer_jpeg = mime.eq_ignore_ascii_case("image/jpeg");
    let encoded = blocking::run_cpu(
        "cap_model_view",
        move || -> Result<(Vec<u8>, &'static str, &'static str)> {
            let image = image::load_from_memory(&bytes)
                .map_err(|_| anyhow!(crate::i18n::t("tool.read_image.not_image")))?;
            let resized = image.resize_exact(width, height, image::imageops::FilterType::Lanczos3);
            encode_image_view_with_preference(&resized, prefer_jpeg)
        },
    )
    .await?;
    let (encoded, out_mime, extension) = encoded;
    let derived_root = persist_derived_root(workspace, workspace_id, filename).await?;
    let output_path = derived_root.join(format!("view.{extension}"));
    fs::write(&output_path, &encoded).await?;
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("image");
    Ok(Some(AttachmentPayload {
        name: Some(format!("{stem}_view.{extension}")),
        content: None,
        content_type: Some(out_mime.to_string()),
        public_path: Some(workspace.display_path(workspace_id, &output_path)),
    }))
}

async fn process_image_source(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    source_path: &Path,
    requested_frame_step: Option<usize>,
) -> Result<ChatMediaProcessResult> {
    let filename = source_path
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "image".to_string());
    if is_windows_metafile_path(source_path) {
        let preview =
            render_metafile_preview_attachment(workspace, workspace_id, source_path).await?;
        let source_public_path = workspace.display_path(workspace_id, source_path);
        return Ok(ChatMediaProcessResult {
            kind: "image".to_string(),
            name: filename,
            source_public_path,
            transcript: None,
            duration_ms: None,
            requested_frame_rate: None,
            applied_frame_rate: None,
            requested_frame_step: None,
            applied_frame_step: None,
            total_frame_count: None,
            frame_count: 1,
            has_audio: false,
            attachments: vec![preview],
            warnings: Vec::new(),
        });
    }
    let bytes = fs::read(source_path).await?;
    let mime_type = detect_supported_image_content_type(source_path, bytes.as_slice())
        .ok_or_else(|| anyhow!("unsupported image type, expected png/jpeg/gif/webp/bmp/tiff"))?;
    if !validate_image_attachment_bytes(&mime_type, bytes.as_slice()) {
        return Err(anyhow!("image file is invalid or unreadable"));
    }
    if mime_type == "image/gif" {
        return process_gif_source(
            workspace,
            workspace_id,
            source_path,
            filename.as_str(),
            bytes.as_slice(),
            requested_frame_step,
        )
        .await;
    }
    let public_path = workspace.display_path(workspace_id, source_path);
    let attachment = match cap_still_image_for_model(
        workspace,
        workspace_id,
        filename.as_str(),
        &mime_type,
        bytes.as_slice(),
    )
    .await?
    {
        Some(attachment) => attachment,
        None => AttachmentPayload {
            name: Some(filename),
            content: None,
            content_type: Some(mime_type),
            public_path: Some(public_path.clone()),
        },
    };
    Ok(ChatMediaProcessResult {
        kind: "image".to_string(),
        name: attachment
            .name
            .clone()
            .unwrap_or_else(|| "image".to_string()),
        source_public_path: public_path,
        transcript: None,
        duration_ms: None,
        requested_frame_rate: None,
        applied_frame_rate: None,
        requested_frame_step: None,
        applied_frame_step: None,
        total_frame_count: None,
        frame_count: 1,
        has_audio: false,
        attachments: vec![attachment],
        warnings: Vec::new(),
    })
}

pub async fn render_metafile_preview_png(source_path: &Path) -> Result<Vec<u8>> {
    if !is_windows_metafile_path(source_path) {
        return Err(anyhow!("unsupported metafile preview type"));
    }
    let temp_dir = std::env::temp_dir()
        .join("wunder_metafile_previews")
        .join(Uuid::new_v4().simple().to_string());
    fs::create_dir_all(&temp_dir).await?;
    let output_path = temp_dir.join("preview.png");
    let result = async {
        convert_metafile_to_png(source_path, &output_path).await?;
        let bytes = fs::read(&output_path).await?;
        if bytes.is_empty() {
            return Err(anyhow!(
                "metafile preview conversion produced an empty image"
            ));
        }
        Ok(bytes)
    }
    .await;
    if temp_dir.exists() {
        if let Err(err) = fs::remove_dir_all(&temp_dir).await {
            warn!(
                "metafile preview temp cleanup failed: {}, {}",
                temp_dir.display(),
                err
            );
        }
    }
    result
}

async fn render_metafile_preview_attachment(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    source_path: &Path,
) -> Result<AttachmentPayload> {
    let preview_bytes = render_metafile_preview_png(source_path).await?;
    let source_name = source_path
        .file_stem()
        .and_then(|value| value.to_str())
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("metafile");
    let derived_root = persist_derived_root(workspace, workspace_id, source_name).await?;
    let output_path = derived_root.join("preview.png");
    fs::write(&output_path, preview_bytes).await?;
    Ok(AttachmentPayload {
        name: Some(format!("{source_name}.png")),
        content: None,
        content_type: Some("image/png".to_string()),
        public_path: Some(workspace.display_path(workspace_id, &output_path)),
    })
}

async fn process_gif_source(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    source_path: &Path,
    filename: &str,
    bytes: &[u8],
    requested_frame_step: Option<usize>,
) -> Result<ChatMediaProcessResult> {
    let requested_step = normalize_requested_frame_step(requested_frame_step)?;
    let decoder = GifDecoder::new(Cursor::new(bytes)).context("failed to decode gif frames")?;
    let frames = decoder
        .into_frames()
        .collect_frames()
        .context("failed to collect gif frames")?;
    if frames.is_empty() {
        return Err(anyhow!("gif contains no readable frames"));
    }
    let total_frame_count = frames.len();

    let public_path = workspace.display_path(workspace_id, source_path);
    if requested_step == 0 {
        let preview = render_gif_frame_attachment(
            workspace,
            workspace_id,
            filename,
            frames
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("gif contains no readable frames"))?
                .into_buffer(),
            0,
        )
        .await?;
        return Ok(ChatMediaProcessResult {
            kind: "gif".to_string(),
            name: filename.to_string(),
            source_public_path: public_path,
            transcript: None,
            duration_ms: None,
            requested_frame_rate: None,
            applied_frame_rate: None,
            requested_frame_step: Some(requested_step),
            applied_frame_step: Some(0),
            total_frame_count: Some(total_frame_count),
            frame_count: 1,
            has_audio: false,
            attachments: vec![preview],
            warnings: Vec::new(),
        });
    }

    let mut attachments = Vec::new();
    for (index, frame) in frames.into_iter().enumerate() {
        if index % requested_step != 0 {
            continue;
        }
        attachments.push(
            render_gif_frame_attachment(
                workspace,
                workspace_id,
                filename,
                frame.into_buffer(),
                index,
            )
            .await?,
        );
    }
    if attachments.is_empty() {
        return Err(anyhow!("gif frame extraction produced no images"));
    }
    Ok(ChatMediaProcessResult {
        kind: "gif".to_string(),
        name: filename.to_string(),
        source_public_path: public_path,
        transcript: None,
        duration_ms: None,
        requested_frame_rate: None,
        applied_frame_rate: None,
        requested_frame_step: Some(requested_step),
        applied_frame_step: Some(requested_step),
        total_frame_count: Some(total_frame_count),
        frame_count: attachments.len(),
        has_audio: false,
        attachments,
        warnings: Vec::new(),
    })
}

async fn process_video_source(
    workspace: &WorkspaceManager,
    config: &Config,
    workspace_id: &str,
    source_path: &Path,
    filename: &str,
    source_public_path: String,
    requested_frame_rate: Option<f64>,
    requested_frame_step: Option<usize>,
) -> Result<ChatMediaProcessResult> {
    if requested_frame_step.is_some() {
        return Err(anyhow!("frame_step is only supported for gif uploads"));
    }
    let probe = probe_media(source_path).await?;
    if !probe.has_video {
        return Err(anyhow!("video track not found in uploaded file"));
    }

    let requested = normalize_requested_frame_rate(requested_frame_rate)?;
    let applied = effective_frame_rate(requested, probe.duration_ms, MAX_VIDEO_FRAMES);
    let derived_root = persist_derived_root(workspace, workspace_id, filename).await?;

    let frame_output_pattern = derived_root.join("frame_%04d.jpg");
    extract_video_frames(source_path, &frame_output_pattern, applied).await?;

    let mut frame_paths = list_frame_paths(&derived_root).await?;
    if frame_paths.is_empty() {
        return Err(anyhow!("video frame extraction produced no images"));
    }

    let mut warnings = Vec::new();
    if applied + f64::EPSILON < requested {
        warnings.push(format!(
            "Frame rate capped to {:.3} fps to keep at most {} frames.",
            applied, MAX_VIDEO_FRAMES
        ));
    }

    frame_paths.sort();
    if frame_paths.len() > MAX_VIDEO_FRAMES {
        frame_paths.truncate(MAX_VIDEO_FRAMES);
        warnings.push(format!(
            "Frame list truncated to {} items after extraction.",
            MAX_VIDEO_FRAMES
        ));
    }
    let mut video_transcript: Option<String> = None;
    let mut attachments = Vec::with_capacity(frame_paths.len().saturating_add(1));
    for frame_path in frame_paths {
        let public_path = workspace.display_path(workspace_id, &frame_path);
        let name = frame_path
            .file_name()
            .and_then(|value| value.to_str())
            .map(ToString::to_string)
            .unwrap_or_else(|| "frame.jpg".to_string());
        attachments.push(AttachmentPayload {
            name: Some(name),
            content: None,
            content_type: Some("image/jpeg".to_string()),
            public_path: Some(public_path),
        });
    }

    let mut has_audio = false;
    if probe.has_audio {
        let audio_path = derived_root.join("audio.wav");
        match extract_video_audio(source_path, &audio_path).await {
            Ok(()) => {
                let (transcript, audio_warnings) = transcribe_audio_file(
                    &config.channels.media.asr,
                    config,
                    &audio_path,
                    audio_path
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or("audio.wav"),
                    "audio/wav",
                )
                .await;
                warnings.extend(audio_warnings);
                let content = transcript
                    .clone()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| build_audio_placeholder(filename));
                if video_transcript.is_none() {
                    video_transcript = transcript.clone().filter(|value| !value.trim().is_empty());
                }
                if transcript.is_none() {
                    warnings.push(
                        "Video audio transcript unavailable, using placeholder text context."
                            .to_string(),
                    );
                }
                attachments.push(AttachmentPayload {
                    name: Some(
                        audio_path
                            .file_name()
                            .and_then(|value| value.to_str())
                            .unwrap_or("audio.wav")
                            .to_string(),
                    ),
                    content: Some(content),
                    content_type: Some("audio/wav".to_string()),
                    public_path: Some(workspace.display_path(workspace_id, &audio_path)),
                });
                has_audio = true;
            }
            Err(err) => {
                warn!(
                    "video audio extraction failed: source={}, error={err}",
                    source_path.display()
                );
                warnings.push(format!("Video audio extraction failed: {err}"));
            }
        }
    } else {
        warnings.push("Video has no audio track.".to_string());
    }

    let frame_count = attachments
        .iter()
        .filter(|item| {
            item.content_type
                .as_deref()
                .unwrap_or("")
                .to_ascii_lowercase()
                .starts_with("image/")
        })
        .count();

    Ok(ChatMediaProcessResult {
        kind: "video".to_string(),
        name: filename.to_string(),
        source_public_path,
        transcript: video_transcript,
        duration_ms: probe.duration_ms,
        requested_frame_rate: Some(requested),
        applied_frame_rate: Some(applied),
        requested_frame_step: None,
        applied_frame_step: None,
        total_frame_count: probe.total_frames,
        frame_count,
        has_audio,
        attachments,
        warnings,
    })
}

async fn persist_source_upload(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    filename: &str,
    bytes: &[u8],
) -> Result<PathBuf> {
    let source_root = format!("{CHAT_MEDIA_SOURCE_DIR}/{}", Uuid::new_v4().simple());
    let relative = format!("{source_root}/{filename}");
    let path = workspace.resolve_path(workspace_id, &relative)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    fs::write(&path, bytes).await?;
    Ok(path)
}

async fn persist_derived_root(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    filename: &str,
) -> Result<PathBuf> {
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("media");
    let safe_stem = crate::attachment::sanitize_filename_stem(stem);
    let relative = format!(
        "{CHAT_MEDIA_DERIVED_DIR}/{}_{}",
        safe_stem,
        Uuid::new_v4().simple()
    );
    let path = workspace.resolve_path(workspace_id, &relative)?;
    fs::create_dir_all(&path).await?;
    Ok(path)
}

fn resolve_private_media_source_path(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    public_path: &str,
) -> Result<PathBuf> {
    let normalized = public_path.trim();
    if normalized.is_empty() {
        return Err(anyhow!("source_public_path is required"));
    }
    let extracted_workspace = extract_workspace_id_from_public_path(normalized)
        .ok_or_else(|| anyhow!("invalid source_public_path"))?;
    if extracted_workspace != workspace_id {
        return Err(anyhow!(
            "source_public_path is outside the current user private workspace"
        ));
    }
    let resolved = workspace.resolve_path(workspace_id, normalized)?;
    if !resolved.is_file() {
        return Err(anyhow!("source_public_path is not a file"));
    }
    Ok(resolved)
}

async fn list_frame_paths(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut reader = fs::read_dir(dir).await?;
    let mut output = Vec::new();
    while let Some(entry) = reader.next_entry().await? {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if name.starts_with("frame_") && name.to_ascii_lowercase().ends_with(".jpg") {
            output.push(path);
        }
    }
    Ok(output)
}

async fn extract_video_frames(
    source_path: &Path,
    output_pattern: &Path,
    frame_rate: f64,
) -> Result<()> {
    let binary = resolve_binary(FFMPEG_BIN_ENV, "ffmpeg");
    let fps_value = format!("{frame_rate:.6}");
    let output = run_command(
        &binary,
        vec![
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
            "-i".to_string(),
            source_path.to_string_lossy().to_string(),
            "-vf".to_string(),
            format!("fps={fps_value}"),
            "-q:v".to_string(),
            "3".to_string(),
            output_pattern.to_string_lossy().to_string(),
        ],
    )
    .await?;
    if output.status.success() {
        return Ok(());
    }
    Err(anyhow!(
        "ffmpeg frame extraction failed: {}",
        stderr_to_text(&output.stderr)
    ))
}

async fn extract_video_audio(source_path: &Path, output_path: &Path) -> Result<()> {
    let binary = resolve_binary(FFMPEG_BIN_ENV, "ffmpeg");
    let output = run_command(
        &binary,
        vec![
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
            "-i".to_string(),
            source_path.to_string_lossy().to_string(),
            "-vn".to_string(),
            "-ac".to_string(),
            "1".to_string(),
            "-ar".to_string(),
            "16000".to_string(),
            "-y".to_string(),
            output_path.to_string_lossy().to_string(),
        ],
    )
    .await?;
    if output.status.success() {
        return Ok(());
    }
    Err(anyhow!(
        "ffmpeg audio extraction failed: {}",
        stderr_to_text(&output.stderr)
    ))
}

async fn probe_media(source_path: &Path) -> Result<MediaProbe> {
    let binary = resolve_binary(FFPROBE_BIN_ENV, "ffprobe");
    let output = run_command(
        &binary,
        vec![
            "-v".to_string(),
            "error".to_string(),
            "-show_entries".to_string(),
            "format=duration:stream=codec_type,nb_frames".to_string(),
            "-of".to_string(),
            "json".to_string(),
            source_path.to_string_lossy().to_string(),
        ],
    )
    .await?;
    if !output.status.success() {
        return Err(anyhow!(
            "ffprobe failed: {}",
            stderr_to_text(&output.stderr)
        ));
    }
    let payload: Value =
        serde_json::from_slice(&output.stdout).context("failed to parse ffprobe json output")?;
    let duration_ms = payload
        .get("format")
        .and_then(|value| value.get("duration"))
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| (value * 1000.0).round() as u64);
    let mut probe = MediaProbe {
        duration_ms,
        has_audio: false,
        has_video: false,
        total_frames: None,
    };
    if let Some(streams) = payload.get("streams").and_then(Value::as_array) {
        for stream in streams {
            match stream
                .get("codec_type")
                .and_then(Value::as_str)
                .unwrap_or("")
            {
                "audio" => probe.has_audio = true,
                "video" => {
                    probe.has_video = true;
                    if probe.total_frames.is_none() {
                        probe.total_frames = stream
                            .get("nb_frames")
                            .and_then(Value::as_str)
                            .and_then(|value| value.parse::<usize>().ok())
                            .filter(|value| *value > 0);
                    }
                }
                _ => {}
            }
        }
    }
    Ok(probe)
}

async fn transcribe_audio_file(
    config: &ChannelAsrConfig,
    app_config: &Config,
    source_path: &Path,
    filename: &str,
    mime_type: &str,
) -> (Option<String>, Vec<String>) {
    if multimodal_models::resolve_asr_model(app_config, None).is_some() {
        match fs::read(source_path).await {
            Ok(bytes) => match multimodal_models::transcribe_audio(
                app_config,
                AudioTranscriptionRequest {
                    audio_bytes: bytes.into(),
                    filename: filename.to_string(),
                    content_type: mime_type.to_string(),
                    model_name: None,
                    language: None,
                    prompt: None,
                    response_format: None,
                    temperature: None,
                },
            )
            .await
            {
                Ok(result) => return (Some(result.text), Vec::new()),
                Err(err) => {
                    return (
                        None,
                        vec![format!("Audio transcription request failed: {err}")],
                    );
                }
            },
            Err(err) => {
                return (
                    None,
                    vec![format!("Audio transcription failed to read file: {}", err)],
                );
            }
        }
    }
    if !config.enabled {
        return (
            None,
            vec!["Audio transcription is disabled in channels.media.asr.".to_string()],
        );
    }
    let provider = config
        .provider
        .as_deref()
        .unwrap_or("openai")
        .trim()
        .to_ascii_lowercase();
    if provider != "openai" && provider != "openai_compatible" {
        return (
            None,
            vec![format!(
                "Audio transcription provider `{provider}` is not supported for chat uploads."
            )],
        );
    }
    let api_key = config.api_key.as_deref().unwrap_or("").trim();
    if api_key.is_empty() {
        return (
            None,
            vec!["Audio transcription API key is not configured.".to_string()],
        );
    }
    let bytes = match fs::read(source_path).await {
        Ok(bytes) => bytes,
        Err(err) => {
            return (
                None,
                vec![format!("Audio transcription failed to read file: {}", err)],
            );
        }
    };
    let max_bytes = if config.max_bytes == 0 {
        25 * 1024 * 1024
    } else {
        config.max_bytes
    };
    if bytes.len() > max_bytes {
        return (
            None,
            vec![format!(
                "Audio transcription skipped because file is larger than {} bytes.",
                max_bytes
            )],
        );
    }

    let client = Client::new();
    let base_url = config
        .base_url
        .as_deref()
        .unwrap_or(DEFAULT_ASR_BASE_URL)
        .trim()
        .trim_end_matches('/');
    let model = config.model.as_deref().unwrap_or("whisper-1").trim();
    let timeout = std::time::Duration::from_secs(config.timeout_s.max(10));

    let mut headers = HeaderMap::new();
    let auth_value = match format!("Bearer {api_key}").parse::<HeaderValue>() {
        Ok(value) => value,
        Err(err) => {
            return (
                None,
                vec![format!("Audio transcription auth header is invalid: {err}")],
            );
        }
    };
    headers.insert(AUTHORIZATION, auth_value);

    let part = match Part::bytes(bytes)
        .file_name(filename.to_string())
        .mime_str(mime_type)
    {
        Ok(part) => part,
        Err(err) => {
            return (
                None,
                vec![format!("Audio transcription mime type is invalid: {err}")],
            );
        }
    };
    let form = Form::new()
        .part("file", part)
        .text("model", model.to_string());

    match client
        .post(format!("{base_url}/audio/transcriptions"))
        .headers(headers)
        .timeout(timeout)
        .multipart(form)
        .send()
        .await
    {
        Ok(response) => {
            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return (
                    None,
                    vec![format!(
                        "Audio transcription request failed with {status}: {}",
                        body.trim()
                    )],
                );
            }
            match response.json::<Value>().await {
                Ok(payload) => {
                    let text = payload
                        .get("text")
                        .and_then(Value::as_str)
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty());
                    if text.is_none() {
                        return (
                            None,
                            vec!["Audio transcription returned empty text.".to_string()],
                        );
                    }
                    (text, Vec::new())
                }
                Err(err) => (
                    None,
                    vec![format!("Audio transcription response parse failed: {err}")],
                ),
            }
        }
        Err(err) => (
            None,
            vec![format!("Audio transcription request failed: {err}")],
        ),
    }
}

fn detect_media_kind(filename: &str, content_type: Option<&str>) -> Option<MediaKind> {
    let lowered_mime = content_type.unwrap_or("").trim().to_ascii_lowercase();
    if lowered_mime.starts_with("image/") {
        return Some(MediaKind::Image);
    }
    if lowered_mime.starts_with("audio/") {
        return Some(MediaKind::Audio);
    }
    if lowered_mime.starts_with("video/") {
        return Some(MediaKind::Video);
    }
    let extension = Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(
        extension.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "wmf" | "emf"
    ) {
        return Some(MediaKind::Image);
    }
    if matches!(
        extension.as_str(),
        "mp3" | "wav" | "ogg" | "opus" | "aac" | "flac" | "m4a"
    ) {
        return Some(MediaKind::Audio);
    }
    if matches!(
        extension.as_str(),
        "mp4" | "mov" | "mkv" | "avi" | "webm" | "mpeg" | "mpg" | "m4v"
    ) {
        return Some(MediaKind::Video);
    }
    None
}

fn is_windows_metafile_path(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(extension.as_str(), "wmf" | "emf")
}

async fn convert_metafile_to_png(source_path: &Path, output_path: &Path) -> Result<()> {
    let configured = std::env::var(IMAGE_MAGICK_BIN_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let mut programs = Vec::new();
    if let Some(program) = configured {
        programs.push(program);
    } else {
        programs.push("magick".to_string());
        if !cfg!(windows) {
            programs.push("convert".to_string());
        }
    }

    let mut failures = Vec::new();
    for program in programs {
        let output_target = format!("png:{}", output_path.to_string_lossy());
        let args = vec![source_path.to_string_lossy().to_string(), output_target];
        match run_command_with_missing_hint(
            &program,
            args,
            &format!(
                "required executable `{program}` not found; set {IMAGE_MAGICK_BIN_ENV} or install ImageMagick"
            ),
        )
        .await
        {
            Ok(output) if output.status.success() => return Ok(()),
            Ok(output) => failures.push(format!("{program}: {}", stderr_to_text(&output.stderr))),
            Err(err) => failures.push(format!("{program}: {err}")),
        }
    }

    Err(anyhow!(
        "metafile preview conversion failed; install ImageMagick or set {IMAGE_MAGICK_BIN_ENV}: {}",
        failures.join("; ")
    ))
}

pub fn detect_media_kind_from_path(path: &Path, sample: &[u8]) -> Option<String> {
    let content_type = detect_supported_image_content_type(path, sample)
        .or_else(|| detect_video_content_type(path))
        .or_else(|| detect_audio_content_type_from_path(path));
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let kind = detect_media_kind(filename, content_type.as_deref())?;
    Some(
        match kind {
            MediaKind::Image => {
                if content_type.as_deref() == Some("image/gif") {
                    "gif"
                } else {
                    "image"
                }
            }
            MediaKind::Audio => "audio",
            MediaKind::Video => "video",
        }
        .to_string(),
    )
}

fn detect_video_content_type(path: &Path) -> Option<String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "mp4" | "m4v" => Some("video/mp4".to_string()),
        "mov" => Some("video/quicktime".to_string()),
        "mkv" => Some("video/x-matroska".to_string()),
        "avi" => Some("video/x-msvideo".to_string()),
        "webm" => Some("video/webm".to_string()),
        "mpeg" | "mpg" => Some("video/mpeg".to_string()),
        _ => None,
    }
}

fn detect_audio_content_type_from_path(path: &Path) -> Option<String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "mp3" => Some("audio/mpeg".to_string()),
        "wav" => Some("audio/wav".to_string()),
        "ogg" => Some("audio/ogg".to_string()),
        "opus" => Some("audio/opus".to_string()),
        "aac" => Some("audio/aac".to_string()),
        "flac" => Some("audio/flac".to_string()),
        "m4a" => Some("audio/mp4".to_string()),
        _ => None,
    }
}

fn detect_supported_image_content_type(path: &Path, bytes: &[u8]) -> Option<String> {
    let guessed = image::guess_format(bytes)
        .ok()
        .and_then(|format| match format {
            image::ImageFormat::Png => Some("image/png"),
            image::ImageFormat::Jpeg => Some("image/jpeg"),
            image::ImageFormat::Gif => Some("image/gif"),
            image::ImageFormat::WebP => Some("image/webp"),
            image::ImageFormat::Bmp => Some("image/bmp"),
            image::ImageFormat::Tiff => Some("image/tiff"),
            _ => None,
        });
    if guessed.is_some() {
        return guessed.map(|value| value.to_string());
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" => Some("image/png".to_string()),
        "jpg" | "jpeg" => Some("image/jpeg".to_string()),
        "gif" => Some("image/gif".to_string()),
        "webp" => Some("image/webp".to_string()),
        "bmp" => Some("image/bmp".to_string()),
        "tif" | "tiff" => Some("image/tiff".to_string()),
        "wmf" => Some("image/wmf".to_string()),
        "emf" => Some("image/emf".to_string()),
        _ => None,
    }
}

fn normalize_requested_frame_rate(value: Option<f64>) -> Result<f64> {
    let requested = value.unwrap_or(DEFAULT_VIDEO_FRAME_RATE);
    if !requested.is_finite() {
        return Err(anyhow!("frame_rate must be a finite number"));
    }
    if !(MIN_VIDEO_FRAME_RATE..=MAX_VIDEO_FRAME_RATE).contains(&requested) {
        return Err(anyhow!(
            "frame_rate must be between {MIN_VIDEO_FRAME_RATE} and {MAX_VIDEO_FRAME_RATE}"
        ));
    }
    Ok(requested)
}

fn normalize_requested_frame_step(value: Option<usize>) -> Result<usize> {
    let requested = value.unwrap_or(DEFAULT_GIF_FRAME_STEP);
    if !(MIN_GIF_FRAME_STEP..=MAX_GIF_FRAME_STEP).contains(&requested) {
        return Err(anyhow!(
            "frame_step must be between {MIN_GIF_FRAME_STEP} and {MAX_GIF_FRAME_STEP}"
        ));
    }
    Ok(requested)
}

async fn render_gif_frame_attachment(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    source_name: &str,
    image: image::RgbaImage,
    index: usize,
) -> Result<AttachmentPayload> {
    let derived_root = persist_derived_root(workspace, workspace_id, source_name).await?;
    let output_path = derived_root.join(format!("frame_{:04}.png", index + 1));
    let output_path_for_write = output_path.clone();
    blocking::run_fs(
        "services.chat_media.save_gif_frame",
        move || -> Result<()> {
            DynamicImage::ImageRgba8(image)
                .save_with_format(&output_path_for_write, ImageFormat::Png)
                .map_err(|err| anyhow!("failed to save gif frame: {err}"))?;
            Ok(())
        },
    )
    .await
    .map_err(|err| anyhow!("gif frame save task failed: {err}"))?;
    let public_path = workspace.display_path(workspace_id, &output_path);
    let name = output_path
        .file_name()
        .and_then(|value| value.to_str())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("frame_{:04}.png", index + 1));
    Ok(AttachmentPayload {
        name: Some(name),
        content: None,
        content_type: Some("image/png".to_string()),
        public_path: Some(public_path),
    })
}

fn effective_frame_rate(requested: f64, duration_ms: Option<u64>, max_frames: usize) -> f64 {
    let Some(duration_ms) = duration_ms.filter(|value| *value > 0) else {
        return requested;
    };
    let duration_s = duration_ms as f64 / 1000.0;
    if duration_s <= 0.0 {
        return requested;
    }
    let max_rate = max_frames as f64 / duration_s;
    requested.min(max_rate.max(MIN_VIDEO_FRAME_RATE))
}

fn detect_audio_content_type(filename: &str) -> String {
    let extension = Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "opus" => "audio/opus",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        "m4a" => "audio/mp4",
        "webm" => "audio/webm",
        _ => "audio/mpeg",
    }
    .to_string()
}

fn build_audio_placeholder(filename: &str) -> String {
    format!("[Audio attachment: {filename}]")
}

fn normalize_filename(raw: &str, fallback_stem: &str) -> String {
    let trimmed = raw.trim();
    let path = Path::new(trimmed);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .trim();
    let stem = if stem.is_empty() {
        fallback_stem.to_string()
    } else {
        let sanitized = crate::attachment::sanitize_filename_stem(stem);
        let sanitized = sanitized
            .split_whitespace()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("_");
        if sanitized.trim().is_empty() {
            fallback_stem.to_string()
        } else {
            sanitized
        }
    };
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    match extension {
        Some(extension) => format!("{stem}.{extension}"),
        None => stem,
    }
}

fn extract_workspace_id_from_public_path(raw: &str) -> Option<String> {
    let normalized = raw.trim().replace('\\', "/");
    let marker_index = normalized.find("/workspaces/")?;
    let rest = &normalized[marker_index + "/workspaces/".len()..];
    let workspace_id = rest.split('/').next()?.trim();
    if workspace_id.is_empty() {
        None
    } else {
        Some(workspace_id.to_string())
    }
}

fn resolve_binary(env_name: &str, fallback: &str) -> String {
    std::env::var(env_name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

async fn run_command(program: &str, args: Vec<String>) -> Result<std::process::Output> {
    run_command_with_missing_hint(
        program,
        args,
        &format!(
            "required executable `{program}` not found; set {FFMPEG_BIN_ENV}/{FFPROBE_BIN_ENV} or install ffmpeg"
        ),
    )
    .await
}

async fn run_command_with_missing_hint(
    program: &str,
    args: Vec<String>,
    missing_hint: &str,
) -> Result<std::process::Output> {
    let mut command = Command::new(program);
    command.args(args);
    apply_platform_spawn_options(&mut command);
    match command.output().await {
        Ok(output) => Ok(output),
        Err(err) if is_not_found_error(&err) => Err(anyhow!(missing_hint.to_string())),
        Err(err) => Err(anyhow!("failed to start `{program}`: {err}")),
    }
}

fn stderr_to_text(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr).trim().to_string()
}

pub async fn load_image_attachment_data_url(
    workspace: &WorkspaceManager,
    attachment: &AttachmentPayload,
) -> Option<String> {
    let raw_content = attachment.content.as_deref().unwrap_or("").trim();
    if let Some((mime_type, bytes)) =
        parse_image_data_url(raw_content, attachment.content_type.as_deref())
    {
        return Some(encode_model_ready_data_url(bytes, mime_type).await);
    }
    let public_path = attachment.public_path.as_deref()?.trim();
    if public_path.is_empty() {
        return None;
    }
    let resolved = match extract_workspace_id_from_public_path(public_path) {
        Some(workspace_id) => workspace.resolve_path(&workspace_id, public_path).ok()?,
        None => {
            // 本地形态（桌面/CLI）的 display_path 就是绝对文件路径，直接读；
            // 否则裁切/抽帧的派生附件会回退成原图，模型看到的不是同一帧。
            let path = Path::new(public_path);
            if !path.is_absolute() {
                return None;
            }
            path.to_path_buf()
        }
    };
    let bytes = fs::read(resolved).await.ok()?;
    let mime = attachment
        .content_type
        .as_deref()
        .map(str::trim)
        .filter(|value| value.starts_with("image/"))
        .unwrap_or("image/jpeg")
        .to_string();
    if !is_supported_model_image_mime(&mime) {
        return None;
    }
    if !validate_image_attachment_bytes(&mime, &bytes) {
        return None;
    }
    Some(encode_model_ready_data_url(bytes, mime).await)
}

/// Re-encode PNG/JPEG inputs to the model-view size before they enter the
/// request. Non-PNG formats (GIF animation, WebP/TIFF/BMP) keep their original
/// bytes so frame semantics are not silently changed.
pub(crate) async fn encode_model_ready_data_url(bytes: Vec<u8>, mime: String) -> String {
    if !matches!(mime.as_str(), "image/png" | "image/jpeg") {
        return format!("data:{mime};base64,{}", STANDARD.encode(bytes));
    }
    let original = bytes.clone();
    let mime_hint = mime.clone();
    let encoded = blocking::run_cpu("model_image_view", move || {
        encode_model_ready_image_bytes(&bytes, &mime_hint)
    })
    .await;
    let (bytes, mime) = match encoded {
        Ok(encoded) => encoded,
        Err(_) => (original, mime),
    };
    format!("data:{mime};base64,{}", STANDARD.encode(bytes))
}

/// Re-inline a previously persisted image from its local path. Used when a
/// resumed turn's history still carries the "analyze this image" instruction
/// but its pixels were dropped by persistence.
pub async fn load_image_data_url_for_path(path: &str) -> Option<String> {
    let path = Path::new(path.trim());
    let metadata = fs::metadata(path).await.ok()?;
    if !metadata.is_file() {
        return None;
    }
    // Re-inlining is a convenience for continuation; never pull a huge file
    // into the request just because history mentioned it.
    if metadata.len() > REINLINE_IMAGE_MAX_BYTES {
        return None;
    }
    let bytes = fs::read(path).await.ok()?;
    let mime = detect_supported_image_content_type(path, &bytes)?;
    if !is_supported_model_image_mime(&mime) || !validate_image_attachment_bytes(&mime, &bytes) {
        return None;
    }
    // 恢复历史时按入模上限重新压缩：磁盘原图可能远大于首轮看到的视图，
    // 直接内联会让跨轮常驻成本超出单图估价。
    let (bytes, mime) = blocking::run_cpu("reinstate_image_view", move || {
        encode_model_ready_image_bytes(&bytes, &mime)
    })
    .await
    .ok()?;
    Some(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

/// 加载持久化历史时，把被省略的图像按路径提示从磁盘重新内联。像素写库时
/// 被省略以控制体积，但磁盘原图与 `[image_path:]` 提示仍在；对齐 codex 在构建
/// prompt 时解析图像引用的做法，让最近的图像跨轮常驻，模型不必反复重读同一帧。
/// 占位与路径提示都是纯文本 part，按出现顺序把每个占位配给它前面最近的路径。
///
/// 只按“最新优先”恢复预算内的图片：同一路径最多保留最近若干个版本，整体
/// 不超过策略的图片数与字节数；更早的占位保留路径提示，需要时由模型重读。
/// 这样既保留视觉上下文，又不会把几十帧原图再次塞满请求。
pub async fn restore_omitted_inline_images(
    messages: &mut [Value],
    policy: &ImageContextPolicy,
) -> ImageRestoreReport {
    use crate::services::chat_payload_sanitizer::{
        image_path_marker_in_text, is_omitted_image_placeholder_text,
    };

    let mut candidates: Vec<(usize, usize, String)> = Vec::new();
    let mut report = ImageRestoreReport::default();
    for (message_index, message) in messages.iter().enumerate() {
        let Some(parts) = message.get("content").and_then(Value::as_array) else {
            continue;
        };
        let mut last_path: Option<String> = None;
        for (part_index, part) in parts.iter().enumerate() {
            let Some(text) = part.get("text").and_then(Value::as_str) else {
                continue;
            };
            if let Some(path) = image_path_marker_in_text(text) {
                last_path = Some(path);
            }
            if is_omitted_image_placeholder_text(text) {
                report.placeholders = report.placeholders.saturating_add(1);
                match last_path.as_ref() {
                    Some(path) => candidates.push((message_index, part_index, path.clone())),
                    None => {
                        report.skipped_missing = report.skipped_missing.saturating_add(1);
                    }
                }
            }
        }
    }

    // Newest first: the latest frame of each path wins, older versions stay as
    // path-bearing placeholders and can be re-read on demand.
    let mut per_path: HashMap<String, usize> = HashMap::new();
    let keep_versions = policy.max_versions_per_path.max(1);
    for (message_index, part_index, path) in candidates.into_iter().rev() {
        let seen = per_path.entry(path.clone()).or_insert(0);
        if *seen >= keep_versions {
            report.skipped_by_path_limit = report.skipped_by_path_limit.saturating_add(1);
            continue;
        }
        if report.restored >= policy.max_images {
            report.skipped_by_budget = report.skipped_by_budget.saturating_add(1);
            continue;
        }
        let Some(data_url) = load_image_data_url_for_path(&path).await else {
            report.skipped_missing = report.skipped_missing.saturating_add(1);
            continue;
        };
        let data_url_len = data_url.len();
        if report.restored_bytes.saturating_add(data_url_len) > policy.max_total_bytes {
            report.skipped_by_budget = report.skipped_by_budget.saturating_add(1);
            continue;
        }
        let replaced = messages
            .get_mut(message_index)
            .and_then(|message| message.get_mut("content"))
            .and_then(Value::as_array_mut)
            .and_then(|parts| parts.get_mut(part_index))
            .map(|part| {
                *part = serde_json::json!({
                    "type": "image_url",
                    "image_url": { "url": data_url }
                });
            })
            .is_some();
        if replaced {
            *seen += 1;
            report.restored = report.restored.saturating_add(1);
            report.restored_bytes = report.restored_bytes.saturating_add(data_url_len);
        }
    }
    report
}

const IMAGE_VIEW_MAX_DIMENSION: u32 = 4096;
const IMAGE_VIEW_MAX_ENCODED_BYTES: usize = 8 * 1024 * 1024;
const IMAGE_VIEW_MAX_SCALE: f64 = 16.0;
const IMAGE_VIEW_EDGE_SAMPLE_WIDTH: u32 = 480;
const IMAGE_VIEW_EDGE_THRESHOLD: u8 = 6;
const IMAGE_VIEW_BACKGROUND_TOLERANCE: u8 = 24;
const IMAGE_VIEW_CORNER_PATCH: u32 = 8;
/// codex 高细节口径：默认入模视图压到最长边 ≤2048 且 ≤2500 个 32px patch，
/// 让常驻图像的单图成本可预测（约 1844 token）。显式 crop/scale 是细节请求，
/// 只按 4096 长边兜底，不做 patch 上限。
const MODEL_VIEW_MAX_DIMENSION: u32 = 2048;
const MODEL_VIEW_MAX_PATCHES: u32 = 2500;
const MODEL_VIEW_PATCH_SIZE: u32 = 32;

/// codex 高细节上限内则返回 None（沿用原图），超限返回缩放后的目标尺寸。
fn model_view_size(width: u32, height: u32) -> Option<(u32, u32)> {
    if width == 0 || height == 0 {
        return None;
    }
    let long = width.max(height);
    let area = f64::from(width) * f64::from(height);
    let max_area = f64::from(MODEL_VIEW_MAX_PATCHES)
        * f64::from(MODEL_VIEW_PATCH_SIZE)
        * f64::from(MODEL_VIEW_PATCH_SIZE);
    let mut scale = 1.0_f64;
    if long > MODEL_VIEW_MAX_DIMENSION {
        scale = f64::from(MODEL_VIEW_MAX_DIMENSION) / f64::from(long);
    }
    if area * scale * scale > max_area {
        scale = (max_area / area).sqrt();
    }
    if scale >= 1.0 {
        return None;
    }
    let scaled_width = ((f64::from(width) * scale).floor() as u32).max(1);
    let scaled_height = ((f64::from(height) * scale).floor() as u32).max(1);
    (scaled_width < width || scaled_height < height).then_some((scaled_width, scaled_height))
}

fn max_dimension_size(width: u32, height: u32, max: u32) -> Option<(u32, u32)> {
    let long = width.max(height);
    if long == 0 || long <= max {
        return None;
    }
    let scale = f64::from(max) / f64::from(long);
    Some((
        ((f64::from(width) * scale).floor() as u32).max(1),
        ((f64::from(height) * scale).floor() as u32).max(1),
    ))
}

fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

/// 读图工具的裁切区域，单位是源图像素。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageCrop {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl ImageCrop {
    pub fn parse(raw: &str) -> Result<Self> {
        let invalid = || anyhow!(crate::i18n::t("tool.read_image.invalid_crop"));
        let parts: Vec<&str> = raw.split(',').map(str::trim).collect();
        if parts.len() != 4 {
            return Err(invalid());
        }
        let mut values = [0_u32; 4];
        for (slot, part) in values.iter_mut().zip(parts) {
            *slot = part.parse::<u32>().map_err(|_| invalid())?;
        }
        let crop = Self {
            x: values[0],
            y: values[1],
            width: values[2],
            height: values[3],
        };
        if crop.width == 0 || crop.height == 0 {
            return Err(invalid());
        }
        Ok(crop)
    }
}

/// 区域数值检查：替代「裁切→存图→肉眼看」的纯数值证据，随结果文本持久留存。
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct ImageViewStats {
    pub width: u32,
    pub height: u32,
    pub mean_luma: f64,
    pub background_rgb: [u8; 3],
    pub non_background_ratio: f64,
    pub edge_ratio: f64,
}

pub struct ImageView {
    pub attachment: Option<AttachmentPayload>,
    pub stats: Option<ImageViewStats>,
    pub source_width: u32,
    pub source_height: u32,
    pub view_width: u32,
    pub view_height: u32,
}

struct RenderedImageView {
    encoded: Option<(Vec<u8>, &'static str, &'static str)>,
    stats: Option<ImageViewStats>,
    source_width: u32,
    source_height: u32,
    view_width: u32,
    view_height: u32,
    view_changed: bool,
}

/// 读图工具的裁切/缩放/统计：派生图落盘为工作区附件，统计在缩放前的裁切
/// 区域上计算（缩放不改变亮度与占比语义）。stats_only 不解码输出像素。
pub async fn derive_image_view(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    source_path: &Path,
    crop: Option<ImageCrop>,
    scale: Option<f64>,
    want_stats: bool,
    stats_only: bool,
) -> Result<ImageView> {
    if let Some(scale) = scale {
        if !scale.is_finite() || scale <= 0.0 || scale > IMAGE_VIEW_MAX_SCALE {
            return Err(anyhow!(crate::i18n::t("tool.read_image.invalid_scale")));
        }
    }
    let bytes = fs::read(source_path).await?;
    let filename = source_path
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "image".to_string());
    let source_mime = detect_supported_image_content_type(source_path, &bytes);
    let rendered = blocking::run_cpu("read_image_view", move || {
        render_image_view(&bytes, crop, scale, want_stats, stats_only)
    })
    .await?;
    let attachment = if stats_only {
        None
    } else if rendered.view_changed {
        let (encoded, mime, extension) = rendered
            .encoded
            .ok_or_else(|| anyhow!("image view encoding missing"))?;
        let derived_root = persist_derived_root(workspace, workspace_id, &filename).await?;
        let output_path = derived_root.join(format!("view.{extension}"));
        fs::write(&output_path, &encoded).await?;
        let stem = Path::new(&filename)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("image");
        Some(AttachmentPayload {
            name: Some(format!("{stem}_view.{extension}")),
            content: None,
            content_type: Some(mime.to_string()),
            public_path: Some(workspace.display_path(workspace_id, &output_path)),
        })
    } else {
        let mime =
            source_mime.ok_or_else(|| anyhow!(crate::i18n::t("tool.read_image.not_image")))?;
        Some(AttachmentPayload {
            name: Some(filename),
            content: None,
            content_type: Some(mime),
            public_path: Some(workspace.display_path(workspace_id, source_path)),
        })
    };
    Ok(ImageView {
        attachment,
        stats: rendered.stats,
        source_width: rendered.source_width,
        source_height: rendered.source_height,
        view_width: rendered.view_width,
        view_height: rendered.view_height,
    })
}

fn render_image_view(
    bytes: &[u8],
    crop: Option<ImageCrop>,
    scale: Option<f64>,
    want_stats: bool,
    stats_only: bool,
) -> Result<RenderedImageView> {
    let image = image::load_from_memory(bytes)
        .map_err(|_| anyhow!(crate::i18n::t("tool.read_image.not_image")))?;
    let (source_width, source_height) = (image.width(), image.height());
    let cropped = match crop {
        Some(crop) => {
            if crop.x.saturating_add(crop.width) > source_width
                || crop.y.saturating_add(crop.height) > source_height
            {
                return Err(anyhow!(crate::i18n::t_with_params(
                    "tool.read_image.crop_out_of_bounds",
                    &std::collections::HashMap::from([
                        ("width".to_string(), source_width.to_string()),
                        ("height".to_string(), source_height.to_string()),
                    ]),
                )));
            }
            image.crop_imm(crop.x, crop.y, crop.width, crop.height)
        }
        None => image,
    };
    let stats = want_stats.then(|| compute_image_view_stats(&cropped));
    let prefer_jpeg = matches!(image::guess_format(bytes), Ok(ImageFormat::Jpeg));
    let effective_scale = scale.unwrap_or(1.0);
    let explicit_detail = crop.is_some() || (effective_scale - 1.0).abs() > f64::EPSILON;
    let mut resized = false;
    let scaled = if (effective_scale - 1.0).abs() > f64::EPSILON {
        let width = (f64::from(cropped.width()) * effective_scale)
            .round()
            .max(1.0) as u32;
        let height = (f64::from(cropped.height()) * effective_scale)
            .round()
            .max(1.0) as u32;
        if width > IMAGE_VIEW_MAX_DIMENSION || height > IMAGE_VIEW_MAX_DIMENSION {
            let max_scale = (f64::from(IMAGE_VIEW_MAX_DIMENSION) / f64::from(cropped.width()))
                .min(f64::from(IMAGE_VIEW_MAX_DIMENSION) / f64::from(cropped.height()));
            return Err(anyhow!(crate::i18n::t_with_params(
                "tool.read_image.scale_too_large",
                &std::collections::HashMap::from([(
                    "max_scale".to_string(),
                    format!("{max_scale:.2}"),
                )]),
            )));
        }
        resized = true;
        cropped.resize_exact(width, height, image::imageops::FilterType::Lanczos3)
    } else if crop.is_some() {
        // 裁切是细节请求：只兜底超长边，不套 patch 上限。
        match max_dimension_size(cropped.width(), cropped.height(), IMAGE_VIEW_MAX_DIMENSION) {
            Some((width, height)) => {
                resized = true;
                cropped.resize_exact(width, height, image::imageops::FilterType::Lanczos3)
            }
            None => cropped,
        }
    } else {
        // 默认视图（如仅 stats）：按 codex 高细节上限压缩，控制常驻成本。
        match model_view_size(cropped.width(), cropped.height()) {
            Some((width, height)) => {
                resized = true;
                cropped.resize_exact(width, height, image::imageops::FilterType::Lanczos3)
            }
            None => cropped,
        }
    };
    let view_changed = explicit_detail || resized;
    let encoded = if stats_only || !view_changed {
        None
    } else {
        Some(encode_image_view_with_preference(&scaled, prefer_jpeg)?)
    };
    Ok(RenderedImageView {
        encoded,
        stats,
        source_width,
        source_height,
        view_width: scaled.width(),
        view_height: scaled.height(),
        view_changed,
    })
}

/// 工具 followup 截图入模前的统一上限：超过 codex 高细节口径（2048 长边 /
/// 2500 个 32px patch）才缩放重编码，未超限原样返回，避免无谓的解码重编码。
pub fn encode_model_ready_image_bytes(bytes: &[u8], mime_hint: &str) -> Result<(Vec<u8>, String)> {
    let normalized = mime_hint.trim().to_ascii_lowercase();
    let mime = if normalized.starts_with("image/") {
        normalized
    } else {
        "image/png".to_string()
    };
    let Some((width, height)) = image_dimensions(bytes) else {
        return Ok((bytes.to_vec(), mime));
    };
    let Some((target_width, target_height)) = model_view_size(width, height) else {
        return Ok((bytes.to_vec(), mime));
    };
    let image = image::load_from_memory(bytes)
        .map_err(|_| anyhow!(crate::i18n::t("tool.read_image.not_image")))?;
    let resized = image.resize_exact(
        target_width,
        target_height,
        image::imageops::FilterType::Lanczos3,
    );
    let prefer_jpeg = mime == "image/jpeg";
    let (encoded, out_mime, _) = encode_image_view_with_preference(&resized, prefer_jpeg)?;
    Ok((encoded, out_mime.to_string()))
}

/// 照片类输入保留 JPEG（体积远小于同尺寸 PNG），其余走 PNG/JPEG 兜底。
fn encode_image_view_with_preference(
    image: &DynamicImage,
    prefer_jpeg: bool,
) -> Result<(Vec<u8>, &'static str, &'static str)> {
    if prefer_jpeg {
        use image::ImageEncoder;
        let rgb = image.to_rgb8();
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85)
            .write_image(
                rgb.as_raw(),
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|err| anyhow!("encode image view failed: {err}"))?;
        if jpeg.len() <= IMAGE_VIEW_MAX_ENCODED_BYTES {
            return Ok((jpeg, "image/jpeg", "jpg"));
        }
    }
    encode_image_view(image)
}

fn encode_image_view(image: &DynamicImage) -> Result<(Vec<u8>, &'static str, &'static str)> {
    let mut png = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .map_err(|err| anyhow!("encode image view failed: {err}"))?;
    if png.len() <= IMAGE_VIEW_MAX_ENCODED_BYTES {
        return Ok((png, "image/png", "png"));
    }
    let rgb = DynamicImage::ImageRgb8(image.to_rgb8());
    let mut jpeg = Vec::new();
    rgb.write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg)
        .map_err(|err| anyhow!("encode image view failed: {err}"))?;
    if jpeg.len() > IMAGE_VIEW_MAX_ENCODED_BYTES {
        return Err(anyhow!(crate::i18n::t("tool.read_image.view_too_large")));
    }
    Ok((jpeg, "image/jpeg", "jpg"))
}

fn compute_image_view_stats(image: &DynamicImage) -> ImageViewStats {
    let rgb = image.to_rgb8();
    let (width, height) = rgb.dimensions();
    let pixels = rgb.as_raw();
    let count = u64::from(width) * u64::from(height);
    let mut luma_sum = 0_u64;
    for px in pixels.chunks_exact(3) {
        luma_sum += (u64::from(px[0]) * 77 + u64::from(px[1]) * 150 + u64::from(px[2]) * 29) >> 8;
    }
    let mean_luma = luma_sum as f64 / count.max(1) as f64;

    // 背景估计：四角色块的逐通道中位数，单角有内容时不至于被带偏。
    let patch = IMAGE_VIEW_CORNER_PATCH.min(width).min(height).max(1);
    let mut background = [0_u8; 3];
    for (channel, slot) in background.iter_mut().enumerate() {
        let mut samples: Vec<u8> = Vec::with_capacity((patch * patch * 4) as usize);
        for (x0, y0) in [
            (0, 0),
            (width - patch, 0),
            (0, height - patch),
            (width - patch, height - patch),
        ] {
            for y in y0..y0 + patch {
                for x in x0..x0 + patch {
                    samples.push(pixels[((y * width + x) * 3) as usize + channel]);
                }
            }
        }
        samples.sort_unstable();
        *slot = samples[samples.len() / 2];
    }
    let mut non_background = 0_u64;
    for px in pixels.chunks_exact(3) {
        let diff = px[0]
            .abs_diff(background[0])
            .max(px[1].abs_diff(background[1]))
            .max(px[2].abs_diff(background[2]));
        if diff > IMAGE_VIEW_BACKGROUND_TOLERANCE {
            non_background += 1;
        }
    }

    // 边缘占比在降采样亮度图上算，颗粒噪声被平均掉，与技能的安全区像素
    // 检查同口径；渐变背景靠它而不是颜色差判断。
    let luma = image.to_luma8();
    let sample_width = width.min(IMAGE_VIEW_EDGE_SAMPLE_WIDTH).max(1);
    let sample_height =
        ((u64::from(height) * u64::from(sample_width)) / u64::from(width).max(1)).max(1) as u32;
    let small = image::imageops::resize(
        &luma,
        sample_width,
        sample_height,
        image::imageops::FilterType::Triangle,
    );
    let raw = small.as_raw();
    let mut edge_hits = 0_u64;
    let mut edge_total = 0_u64;
    for y in 0..sample_height {
        for x in 0..sample_width {
            let index = (y * sample_width + x) as usize;
            if x + 1 < sample_width {
                edge_total += 1;
                if raw[index].abs_diff(raw[index + 1]) > IMAGE_VIEW_EDGE_THRESHOLD {
                    edge_hits += 1;
                }
            }
            if y + 1 < sample_height {
                edge_total += 1;
                if raw[index].abs_diff(raw[index + sample_width as usize])
                    > IMAGE_VIEW_EDGE_THRESHOLD
                {
                    edge_hits += 1;
                }
            }
        }
    }
    let round = |value: f64, places: f64| (value * places).round() / places;
    ImageViewStats {
        width,
        height,
        mean_luma: round(mean_luma, 10.0),
        background_rgb: background,
        non_background_ratio: round(non_background as f64 / count.max(1) as f64, 10_000.0),
        edge_ratio: round(edge_hits as f64 / edge_total.max(1) as f64, 10_000.0),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        detect_media_kind_from_path, effective_frame_rate, extract_workspace_id_from_public_path,
        load_image_data_url_for_path, normalize_filename, normalize_requested_frame_rate,
        normalize_requested_frame_step, DEFAULT_VIDEO_FRAME_RATE, MAX_GIF_FRAME_STEP,
        MAX_VIDEO_FRAMES,
    };
    use std::path::Path;

    #[test]
    fn reinline_image_reads_a_real_png_from_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("frame.png");
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
        image.save(&path).expect("write png");

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let url = runtime
            .block_on(load_image_data_url_for_path(path.to_str().expect("path")))
            .expect("data url");
        assert!(url.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn reinline_image_rejects_non_images() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("frame.bin");
        std::fs::write(&path, b"not an image").expect("write file");

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        assert!(runtime
            .block_on(load_image_data_url_for_path(path.to_str().expect("path")))
            .is_none());
    }

    fn png_bytes(image: &image::DynamicImage) -> Vec<u8> {
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("encode png");
        bytes
    }

    /// 100x80 黑底，(50,50) 处一块 10x10 白色矩形。
    fn fixture_image() -> image::DynamicImage {
        let mut image = image::RgbImage::from_pixel(100, 80, image::Rgb([0, 0, 0]));
        for y in 50..60 {
            for x in 50..60 {
                image.put_pixel(x, y, image::Rgb([255, 255, 255]));
            }
        }
        image::DynamicImage::ImageRgb8(image)
    }

    #[test]
    fn image_crop_parse_validates_shape() {
        let crop = super::ImageCrop::parse("10, 20,30,40").expect("parse crop");
        assert_eq!((crop.x, crop.y, crop.width, crop.height), (10, 20, 30, 40));
        for bad in ["1,2,3", "1,2,0,4", "a,b,c,d", "-1,2,3,4", "1,2,3,4,5"] {
            assert!(super::ImageCrop::parse(bad).is_err(), "accepted {bad}");
        }
    }

    #[test]
    fn attachment_data_url_reads_local_absolute_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("view.png");
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 255, 0, 255]));
        image.save(&path).expect("write png");
        let attachment = crate::schemas::AttachmentPayload {
            name: Some("view.png".to_string()),
            content: None,
            content_type: Some("image/png".to_string()),
            public_path: Some(path.to_string_lossy().to_string()),
        };
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let workspace = crate::workspace::WorkspaceManager::new(
            dir.path().to_string_lossy().as_ref(),
            std::sync::Arc::new(crate::storage::SqliteStorage::new(
                dir.path().join("state.db").to_string_lossy().to_string(),
            )),
            0,
            &std::collections::HashMap::new(),
        );
        let url = runtime
            .block_on(super::load_image_attachment_data_url(
                &workspace,
                &attachment,
            ))
            .expect("data url from absolute path");
        assert!(url.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn render_image_view_crops_and_computes_region_stats() {
        let bytes = png_bytes(&fixture_image());
        let crop = super::ImageCrop {
            x: 40,
            y: 40,
            width: 20,
            height: 20,
        };
        let view =
            super::render_image_view(&bytes, Some(crop), None, true, false).expect("render view");
        assert_eq!((view.source_width, view.source_height), (100, 80));
        assert_eq!((view.view_width, view.view_height), (20, 20));
        let stats = view.stats.expect("stats");
        assert_eq!((stats.width, stats.height), (20, 20));
        assert_eq!(stats.background_rgb, [0, 0, 0]);
        // 白色矩形 100/400 像素：占比 25%，平均亮度 255*0.25。
        assert!((stats.non_background_ratio - 0.25).abs() < 0.001);
        assert!((stats.mean_luma - 63.8).abs() < 0.2);
        assert!(stats.edge_ratio > 0.0);
        let (encoded, mime, extension) = view.encoded.expect("encoded view");
        assert_eq!(mime, "image/png");
        assert_eq!(extension, "png");
        let decoded = image::load_from_memory(&encoded).expect("decode view");
        assert_eq!((decoded.width(), decoded.height()), (20, 20));
    }

    #[test]
    fn render_image_view_stats_only_skips_encoding() {
        let bytes = png_bytes(&fixture_image());
        let view = super::render_image_view(&bytes, None, None, true, true).expect("render view");
        assert!(view.encoded.is_none());
        assert!(view.stats.is_some());
        assert!(!view.view_changed);
    }

    #[test]
    fn render_image_view_plain_read_reuses_original() {
        let bytes = png_bytes(&fixture_image());
        let view = super::render_image_view(&bytes, None, None, false, false).expect("render view");
        assert!(view.encoded.is_none());
        assert!(!view.view_changed);
        assert!(view.stats.is_none());
    }

    #[test]
    fn render_image_view_scale_zooms_after_crop() {
        let bytes = png_bytes(&fixture_image());
        let crop = super::ImageCrop {
            x: 50,
            y: 50,
            width: 10,
            height: 10,
        };
        let view = super::render_image_view(&bytes, Some(crop), Some(2.0), false, false)
            .expect("render view");
        assert_eq!((view.view_width, view.view_height), (20, 20));
        assert!(view.view_changed);
        assert!(view.encoded.is_some());
    }

    #[test]
    fn render_image_view_rejects_out_of_bounds_crop() {
        let bytes = png_bytes(&fixture_image());
        let crop = super::ImageCrop {
            x: 90,
            y: 70,
            width: 20,
            height: 20,
        };
        assert!(super::render_image_view(&bytes, Some(crop), None, false, false).is_err());
    }

    #[test]
    fn render_image_view_rejects_oversized_scale() {
        let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            400,
            300,
            image::Rgb([0, 0, 0]),
        ));
        let bytes = png_bytes(&image);
        assert!(super::render_image_view(&bytes, None, Some(16.0), false, false).is_err());
    }

    #[test]
    fn model_view_size_matches_codex_high_detail_limits() {
        // 1080p 与常见的 1290x1210 拼图不压缩。
        assert!(super::model_view_size(1920, 1080).is_none());
        assert!(super::model_view_size(1290, 1210).is_none());
        // 4K 按最长边 2048 压。
        assert_eq!(super::model_view_size(3840, 2160), Some((2048, 1152)));
        // 方形大图按 2500 个 32px patch 的面积上限压。
        let (width, height) = super::model_view_size(3000, 3000).expect("resize");
        assert!(width.div_ceil(32) * height.div_ceil(32) <= 2500);
    }

    #[test]
    fn model_ready_image_bytes_cap_oversized_screenshots_only() {
        let large = png_bytes(&image::DynamicImage::ImageRgb8(
            image::RgbImage::from_pixel(2600, 1300, image::Rgb([30, 30, 30])),
        ));
        let (capped, mime) =
            super::encode_model_ready_image_bytes(&large, "image/png").expect("cap screenshot");
        assert_eq!(mime, "image/png");
        let decoded = image::load_from_memory(&capped).expect("decode capped");
        assert!(decoded.width() <= 2048 && decoded.height() <= 2048);
        assert!(decoded.width().div_ceil(32) * decoded.height().div_ceil(32) <= 2500);

        let small = png_bytes(&fixture_image());
        let (unchanged, mime) =
            super::encode_model_ready_image_bytes(&small, "image/png").expect("keep small");
        assert_eq!(mime, "image/png");
        assert_eq!(unchanged, small);
    }

    #[test]
    fn render_image_view_default_stats_view_is_capped_but_stats_keep_original_region() {
        let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            2600,
            2600,
            image::Rgb([10, 10, 10]),
        ));
        let bytes = png_bytes(&image);
        let view = super::render_image_view(&bytes, None, None, true, false).expect("render");
        assert!(view.view_width <= 2048 && view.view_height <= 2048);
        assert!(view.view_changed);
        let stats = view.stats.expect("stats");
        // 统计在缩放前的原始区域上计算，坐标语义不随入模压缩改变。
        assert_eq!((stats.width, stats.height), (2600, 2600));
    }

    #[tokio::test]
    async fn cap_still_image_for_model_resizes_large_and_skips_small() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = crate::workspace::WorkspaceManager::new(
            dir.path().to_string_lossy().as_ref(),
            std::sync::Arc::new(crate::storage::SqliteStorage::new(
                dir.path().join("state.db").to_string_lossy().to_string(),
            )),
            0,
            &std::collections::HashMap::new(),
        );
        let large = png_bytes(&image::DynamicImage::ImageRgb8(
            image::RgbImage::from_pixel(2600, 2600, image::Rgb([0, 0, 0])),
        ));
        let attachment =
            super::cap_still_image_for_model(&workspace, "user", "big.png", "image/png", &large)
                .await
                .expect("cap large")
                .expect("large image is capped");
        let path = workspace
            .resolve_path("user", attachment.public_path.as_deref().unwrap())
            .expect("resolve derived");
        let decoded = image::open(&path).expect("open derived");
        assert!(decoded.width() <= 2048 && decoded.height() <= 2048);

        let small = png_bytes(&fixture_image());
        assert!(super::cap_still_image_for_model(
            &workspace,
            "user",
            "small.png",
            "image/png",
            &small
        )
        .await
        .expect("cap small")
        .is_none());
    }

    #[tokio::test]
    async fn restore_omitted_images_reinlines_from_path_hints() {
        let dir = tempfile::tempdir().expect("tempdir");
        let frame_a = dir.path().join("a.png");
        let frame_b = dir.path().join("b.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
            .save(&frame_a)
            .expect("write a");
        image::RgbImage::from_pixel(2600, 1300, image::Rgb([0, 0, 255]))
            .save(&frame_b)
            .expect("write b");
        let placeholder = "[inline image omitted from persisted context]";
        let mut messages = vec![
            serde_json::json!({
                "role": "user",
                "content": [
                    {"type": "text", "text": "prompt"},
                    {"type": "text", "text": format!("[image_path:{}]", frame_a.display())},
                    {"type": "text", "text": placeholder},
                    {"type": "text", "text": format!("[image_path:{}]", frame_b.display())},
                    {"type": "text", "text": placeholder}
                ]
            }),
            // 文件已删除：占位保留，不阻断加载。
            serde_json::json!({
                "role": "user",
                "content": [
                    {"type": "text", "text": format!("[image_path:{}]", dir.path().join("gone.png").display())},
                    {"type": "text", "text": placeholder}
                ]
            }),
        ];

        let policy = crate::services::image_context::ImageContextPolicy {
            max_images: 24,
            max_total_bytes: 24 * 1024 * 1024,
            max_versions_per_path: 2,
            max_request_bytes: 32 * 1024 * 1024,
        };
        super::restore_omitted_inline_images(&mut messages, &policy).await;

        let parts = messages[0]["content"].as_array().expect("parts");
        assert_eq!(parts[2]["type"], "image_url");
        assert!(parts[2]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));
        assert_eq!(parts[4]["type"], "image_url");
        // 两张图各自配对自己的路径，恢复出的字节不同。
        assert_ne!(parts[2]["image_url"]["url"], parts[4]["image_url"]["url"]);
        // 大图恢复时按入模上限压缩，跨轮常驻成本与首轮一致。
        use base64::Engine as _;
        let url = parts[4]["image_url"]["url"].as_str().expect("url");
        let encoded = url.split_once(',').expect("data url payload").1;
        let restored_bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("decode restored");
        let restored = image::load_from_memory(&restored_bytes).expect("restored image");
        assert!(restored.width() <= 2048 && restored.height() <= 2048);
        assert!(restored.width().div_ceil(32) * restored.height().div_ceil(32) <= 2500);
        let missing = messages[1]["content"].as_array().expect("parts");
        assert_eq!(missing[1]["text"], placeholder);
    }

    #[tokio::test]
    async fn restore_omitted_images_keeps_newest_version_per_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let frame = dir.path().join("frame.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 255, 0, 255]))
            .save(&frame)
            .expect("write frame");
        let placeholder = "[inline image omitted from persisted context]";
        let hint = format!("[image_path:{}]", frame.display());
        let mut messages = vec![
            serde_json::json!({
                "role": "user",
                "content": [
                    {"type": "text", "text": hint.clone()},
                    {"type": "text", "text": placeholder},
                ]
            }),
            serde_json::json!({
                "role": "user",
                "content": [
                    {"type": "text", "text": hint.clone()},
                    {"type": "text", "text": placeholder},
                ]
            }),
        ];
        let policy = crate::services::image_context::ImageContextPolicy {
            max_images: 24,
            max_total_bytes: 24 * 1024 * 1024,
            max_versions_per_path: 1,
            max_request_bytes: 32 * 1024 * 1024,
        };

        let report = super::restore_omitted_inline_images(&mut messages, &policy).await;

        assert_eq!(report.placeholders, 2);
        assert_eq!(report.restored, 1);
        assert_eq!(report.skipped_by_path_limit, 1);
        assert_eq!(messages[0]["content"][1]["type"], "text");
        assert_eq!(messages[1]["content"][1]["type"], "image_url");
    }

    #[tokio::test]
    async fn restore_omitted_images_respects_budget() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = dir.path().join("first.png");
        let second = dir.path().join("second.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
            .save(&first)
            .expect("write first");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 255, 255]))
            .save(&second)
            .expect("write second");
        let placeholder = "[inline image omitted from persisted context]";
        let mut messages = vec![
            serde_json::json!({
                "role": "user",
                "content": [
                    {"type": "text", "text": format!("[image_path:{}]", first.display())},
                    {"type": "text", "text": placeholder},
                ]
            }),
            serde_json::json!({
                "role": "user",
                "content": [
                    {"type": "text", "text": format!("[image_path:{}]", second.display())},
                    {"type": "text", "text": placeholder},
                ]
            }),
        ];
        let policy = crate::services::image_context::ImageContextPolicy {
            max_images: 1,
            max_total_bytes: 24 * 1024 * 1024,
            max_versions_per_path: 2,
            max_request_bytes: 32 * 1024 * 1024,
        };

        let report = super::restore_omitted_inline_images(&mut messages, &policy).await;

        assert_eq!(report.restored, 1);
        assert_eq!(report.skipped_by_budget, 1);
        // Newest first: the second message wins the only slot.
        assert_eq!(messages[0]["content"][1]["type"], "text");
        assert_eq!(messages[1]["content"][1]["type"], "image_url");
    }

    #[test]
    fn normalize_filename_preserves_extension() {
        assert_eq!(
            normalize_filename(" demo clip.MP4 ", "media"),
            "demo_clip.mp4"
        );
    }

    #[test]
    fn extract_workspace_id_supports_full_public_path() {
        assert_eq!(
            extract_workspace_id_from_public_path("/workspaces/alice__c__1/media/a.wav"),
            Some("alice__c__1".to_string())
        );
    }

    #[test]
    fn detect_media_kind_accepts_windows_metafiles() {
        assert_eq!(
            detect_media_kind_from_path(Path::new("preview.wmf"), &[]),
            Some("image".to_string())
        );
        assert_eq!(
            detect_media_kind_from_path(Path::new("preview.emf"), &[]),
            Some("image".to_string())
        );
    }

    #[test]
    fn normalize_requested_frame_rate_uses_default() {
        let rate = normalize_requested_frame_rate(None).expect("default rate");
        assert!((rate - DEFAULT_VIDEO_FRAME_RATE).abs() < f64::EPSILON);
    }

    #[test]
    fn effective_frame_rate_caps_long_video() {
        let capped = effective_frame_rate(1.0, Some(5 * 60 * 1000), MAX_VIDEO_FRAMES);
        assert!(capped < 1.0);
        assert!(capped > 0.0);
    }

    #[test]
    fn normalize_requested_frame_step_uses_first_frame_by_default() {
        let step = normalize_requested_frame_step(None).expect("default frame step");
        assert_eq!(step, 0);
    }

    #[test]
    fn normalize_requested_frame_step_accepts_interval_sampling() {
        let step = normalize_requested_frame_step(Some(2)).expect("interval frame step");
        assert_eq!(step, 2);
    }

    #[test]
    fn normalize_requested_frame_step_rejects_large_values() {
        let err = normalize_requested_frame_step(Some(MAX_GIF_FRAME_STEP + 1))
            .expect_err("frame step beyond max should fail");
        assert!(err.to_string().contains("frame_step must be between"));
    }
}
