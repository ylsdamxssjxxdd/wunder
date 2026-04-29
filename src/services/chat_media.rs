use crate::config::{ChannelAsrConfig, Config};
use crate::core::command_utils::{apply_platform_spawn_options, is_not_found_error};
use crate::schemas::AttachmentPayload;
use crate::services::chat_attachments::{
    is_supported_model_image_mime, parse_image_data_url, validate_image_attachment_bytes,
};
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
const DEFAULT_ASR_BASE_URL: &str = "https://api.openai.com/v1";
const FFMPEG_BIN_ENV: &str = "WUNDER_FFMPEG_BIN";
const FFPROBE_BIN_ENV: &str = "WUNDER_FFPROBE_BIN";

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
    Ok(ChatMediaProcessResult {
        kind: "image".to_string(),
        name: filename.clone(),
        source_public_path: public_path.clone(),
        duration_ms: None,
        requested_frame_rate: None,
        applied_frame_rate: None,
        requested_frame_step: None,
        applied_frame_step: None,
        total_frame_count: None,
        frame_count: 1,
        has_audio: false,
        attachments: vec![AttachmentPayload {
            name: Some(filename),
            content: None,
            content_type: Some(mime_type),
            public_path: Some(public_path),
        }],
        warnings: Vec::new(),
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
    source_path: &Path,
    filename: &str,
    mime_type: &str,
) -> (Option<String>, Vec<String>) {
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
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff"
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
    tokio::task::spawn_blocking(move || -> Result<()> {
        DynamicImage::ImageRgba8(image)
            .save_with_format(&output_path_for_write, ImageFormat::Png)
            .map_err(|err| anyhow!("failed to save gif frame: {err}"))?;
        Ok(())
    })
    .await
    .map_err(|err| anyhow!("gif frame save task failed: {err}"))??;
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
    let mut command = Command::new(program);
    command.args(args);
    apply_platform_spawn_options(&mut command);
    match command.output().await {
        Ok(output) => Ok(output),
        Err(err) if is_not_found_error(&err) => Err(anyhow!(
            "required executable `{program}` not found; set {FFMPEG_BIN_ENV}/{FFPROBE_BIN_ENV} or install ffmpeg"
        )),
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
        return Some(format!(
            "data:{mime_type};base64,{}",
            STANDARD.encode(bytes)
        ));
    }
    let public_path = attachment.public_path.as_deref()?.trim();
    if public_path.is_empty() {
        return None;
    }
    let workspace_id = extract_workspace_id_from_public_path(public_path)?;
    let resolved = workspace.resolve_path(&workspace_id, public_path).ok()?;
    let bytes = fs::read(resolved).await.ok()?;
    let mime = attachment
        .content_type
        .as_deref()
        .map(str::trim)
        .filter(|value| value.starts_with("image/"))
        .unwrap_or("image/jpeg");
    if !is_supported_model_image_mime(mime) {
        return None;
    }
    if !validate_image_attachment_bytes(mime, &bytes) {
        return None;
    }
    Some(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

#[cfg(test)]
mod tests {
    use super::{
        effective_frame_rate, extract_workspace_id_from_public_path, normalize_filename,
        normalize_requested_frame_rate, normalize_requested_frame_step, DEFAULT_VIDEO_FRAME_RATE,
        MAX_GIF_FRAME_STEP, MAX_VIDEO_FRAMES,
    };

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
