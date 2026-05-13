use crate::api::user_context::resolve_user;
use crate::services::multimodal_models::{
    self, AudioTranscriptionRequest, ImageGenerationRequest, SpeechSynthesisRequest,
    VideoGenerationRequest,
};
use crate::state::AppState;
use crate::storage::{normalize_workspace_container_id, USER_PRIVATE_CONTAINER_ID};
use axum::extract::{DefaultBodyLimit, Json, Multipart, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::{routing::post, Router};
use bytes::Bytes;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use tokio::fs;
use uuid::Uuid;

const GENERATED_MEDIA_DIR: &str = "generated_media";
const DEFAULT_DEBUG_USER_ID: &str = "admin";
const MAX_MULTIMODAL_UPLOAD_BYTES: usize = 128 * 1024 * 1024;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/admin/multimodal/transcription",
            post(admin_transcribe_audio).layer(DefaultBodyLimit::max(MAX_MULTIMODAL_UPLOAD_BYTES)),
        )
        .route(
            "/wunder/admin/multimodal/speech",
            post(admin_generate_speech),
        )
        .route("/wunder/admin/multimodal/image", post(admin_generate_image))
        .route("/wunder/admin/multimodal/video", post(admin_generate_video))
}

#[derive(Debug, Deserialize)]
struct AdminMultimodalBaseRequest {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    container_id: Option<i32>,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AdminSpeechRequest {
    #[serde(flatten)]
    base: AdminMultimodalBaseRequest,
    text: String,
    #[serde(default)]
    model_name: Option<String>,
    #[serde(default)]
    voice: Option<String>,
    #[serde(default)]
    instructions: Option<String>,
    #[serde(default, alias = "responseFormat", alias = "response_format")]
    response_format: Option<String>,
    #[serde(default)]
    speed: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct AdminImageRequest {
    #[serde(flatten)]
    base: AdminMultimodalBaseRequest,
    prompt: String,
    #[serde(default)]
    model_name: Option<String>,
    #[serde(default)]
    size: Option<String>,
    #[serde(default, alias = "outputFormat", alias = "output_format")]
    output_format: Option<String>,
    #[serde(default)]
    negative_prompt: Option<String>,
    #[serde(default)]
    num_inference_steps: Option<u32>,
    #[serde(default)]
    guidance_scale: Option<f32>,
    #[serde(default)]
    seed: Option<u64>,
    #[serde(default)]
    input_path: Option<String>,
    #[serde(default)]
    input_paths: Option<Vec<String>>,
    #[serde(default)]
    mask_path: Option<String>,
    #[serde(default)]
    reference_path: Option<String>,
    #[serde(default)]
    strength: Option<f32>,
    #[serde(default)]
    true_cfg_scale: Option<f32>,
    #[serde(default)]
    output_compression: Option<u32>,
    #[serde(default)]
    layers: Option<u32>,
    #[serde(default)]
    resolution: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct AdminVideoRequest {
    #[serde(flatten)]
    base: AdminMultimodalBaseRequest,
    prompt: String,
    #[serde(default)]
    model_name: Option<String>,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    seconds: Option<f32>,
    #[serde(default)]
    fps: Option<u32>,
    #[serde(default)]
    num_frames: Option<u32>,
    #[serde(default)]
    negative_prompt: Option<String>,
    #[serde(default)]
    num_inference_steps: Option<u32>,
    #[serde(default)]
    guidance_scale: Option<f32>,
    #[serde(default)]
    guidance_scale_2: Option<f32>,
    #[serde(default)]
    boundary_ratio: Option<f32>,
    #[serde(default)]
    flow_shift: Option<f32>,
    #[serde(default)]
    seed: Option<u64>,
    #[serde(default)]
    enable_frame_interpolation: Option<bool>,
}

#[derive(Debug, Default)]
struct AdminTranscriptionFields {
    user_id: Option<String>,
    container_id: Option<i32>,
    path: Option<String>,
    source_public_path: Option<String>,
    model_name: Option<String>,
    language: Option<String>,
    prompt: Option<String>,
    response_format: Option<String>,
    temperature: Option<f32>,
    upload_filename: Option<String>,
    upload_content_type: Option<String>,
    upload_bytes: Option<Bytes>,
}

struct PersistedMediaFile {
    public_path: String,
    workspace_relative_path: String,
    size_bytes: usize,
}

async fn admin_transcribe_audio(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Json<Value>, Response> {
    let payload = parse_admin_transcription_multipart(multipart).await?;
    let request_user_id = requested_user_id(payload.user_id.as_deref());
    let resolved = resolve_user(&state, &headers, Some(request_user_id.as_str())).await?;
    let container_id = payload
        .container_id
        .map(normalize_workspace_container_id)
        .unwrap_or(USER_PRIVATE_CONTAINER_ID);
    let workspace_id = state
        .workspace
        .scoped_user_id_by_container(&resolved.user.user_id, container_id);
    let config = state.config_store.get().await;
    let selected_model_name =
        multimodal_models::resolve_asr_model(&config, payload.model_name.as_deref())
            .map(|(name, _)| name)
            .ok_or_else(|| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    "asr model is not configured".to_string(),
                )
            })?;

    let (audio_bytes, filename, content_type, source_public_path, source_workspace_relative_path) =
        if let Some(bytes) = payload.upload_bytes {
            let filename = payload
                .upload_filename
                .clone()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "audio.wav".to_string());
            let content_type = payload
                .upload_content_type
                .clone()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| audio_content_type_from_path(filename.as_str()).to_string());
            let saved = persist_generated_media(
                state.as_ref(),
                &workspace_id,
                payload.path.as_deref(),
                "transcription_source",
                &extension_from_content_type(&content_type, "wav"),
                &bytes,
            )
            .await?;
            (
                bytes,
                filename,
                content_type,
                Some(saved.public_path),
                Some(saved.workspace_relative_path),
            )
        } else {
            let source_public_path = payload
                .source_public_path
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    error_response(
                        StatusCode::BAD_REQUEST,
                        "file or source_public_path is required".to_string(),
                    )
                })?;
            let source_path = state
                .workspace
                .resolve_path(&workspace_id, source_public_path)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            let bytes = fs::read(&source_path)
                .await
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            let filename = source_path
                .file_name()
                .and_then(|value| value.to_str())
                .map(ToString::to_string)
                .unwrap_or_else(|| "audio".to_string());
            let content_type = audio_content_type_from_path(filename.as_str()).to_string();
            (
                Bytes::from(bytes),
                filename,
                content_type,
                Some(source_public_path.to_string()),
                Some(normalize_workspace_relative_path(source_public_path)),
            )
        };

    let response_format = normalize_optional_string(payload.response_format.as_deref());
    let language = normalize_optional_string(payload.language.as_deref());
    let prompt = normalize_optional_string(payload.prompt.as_deref());
    let result = multimodal_models::transcribe_audio(
        &config,
        AudioTranscriptionRequest {
            audio_bytes,
            filename: filename.clone(),
            content_type: content_type.clone(),
            model_name: Some(selected_model_name.clone()),
            language: language.clone(),
            prompt: prompt.clone(),
            response_format: response_format.clone(),
            temperature: payload.temperature,
        },
    )
    .await
    .map_err(|err| {
        error_response(
            StatusCode::BAD_REQUEST,
            format!("asr request failed: {err}"),
        )
    })?;

    Ok(Json(json!({
        "data": {
            "kind": "transcription",
            "user_id": resolved.user.user_id,
            "container_id": container_id,
            "workspace_id": workspace_id,
            "model_name": selected_model_name,
            "content_type": result.content_type,
            "text": result.text,
            "source_public_path": source_public_path,
            "source_workspace_relative_path": source_workspace_relative_path,
            "raw_response": result.raw_response,
            "request": {
                "filename": filename,
                "content_type": content_type,
                "model_name": payload.model_name,
                "language": language,
                "prompt": prompt,
                "response_format": response_format,
                "temperature": payload.temperature,
            }
        }
    })))
}

async fn admin_generate_speech(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<AdminSpeechRequest>,
) -> Result<Json<Value>, Response> {
    let text = payload.text.trim();
    if text.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "text is required".to_string(),
        ));
    }

    let request_user_id = requested_user_id(payload.base.user_id.as_deref());
    let resolved = resolve_user(&state, &headers, Some(request_user_id.as_str())).await?;
    let container_id = payload
        .base
        .container_id
        .map(normalize_workspace_container_id)
        .unwrap_or(USER_PRIVATE_CONTAINER_ID);
    let workspace_id = state
        .workspace
        .scoped_user_id_by_container(&resolved.user.user_id, container_id);

    let config = state.config_store.get().await;
    let selected_model_name =
        multimodal_models::resolve_tts_model(&config, payload.model_name.as_deref())
            .map(|(name, _)| name)
            .ok_or_else(|| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    "tts model is not configured".to_string(),
                )
            })?;
    let response_format = normalize_optional_string(payload.response_format.as_deref());
    let result = multimodal_models::synthesize_speech(
        &config,
        SpeechSynthesisRequest {
            text: text.to_string(),
            model_name: Some(selected_model_name.clone()),
            voice: normalize_optional_string(payload.voice.as_deref()),
            instructions: normalize_optional_string(payload.instructions.as_deref()),
            response_format: response_format.clone(),
            speed: payload.speed,
        },
    )
    .await
    .map_err(|err| {
        error_response(
            StatusCode::BAD_REQUEST,
            format!("tts request failed: {err}"),
        )
    })?;

    let saved = persist_generated_media(
        state.as_ref(),
        &workspace_id,
        payload.base.path.as_deref(),
        "speech",
        &extension_from_content_type(&result.content_type, "wav"),
        &result.bytes,
    )
    .await?;

    Ok(Json(json!({
        "data": {
            "kind": "speech",
            "user_id": resolved.user.user_id,
            "container_id": container_id,
            "workspace_id": workspace_id,
            "model_name": selected_model_name,
            "content_type": result.content_type,
            "size_bytes": saved.size_bytes,
            "workspace_relative_path": saved.workspace_relative_path,
            "public_path": saved.public_path,
            "request": {
                "text": text,
                "path": normalize_optional_string(payload.base.path.as_deref()),
                "model_name": payload.model_name,
                "voice": normalize_optional_string(payload.voice.as_deref()),
                "instructions": normalize_optional_string(payload.instructions.as_deref()),
                "response_format": response_format,
                "speed": payload.speed,
            }
        }
    })))
}

async fn admin_generate_image(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<AdminImageRequest>,
) -> Result<Json<Value>, Response> {
    let prompt = payload.prompt.trim();
    if prompt.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "prompt is required".to_string(),
        ));
    }

    let request_user_id = requested_user_id(payload.base.user_id.as_deref());
    let resolved = resolve_user(&state, &headers, Some(request_user_id.as_str())).await?;
    let container_id = payload
        .base
        .container_id
        .map(normalize_workspace_container_id)
        .unwrap_or(USER_PRIVATE_CONTAINER_ID);
    let workspace_id = state
        .workspace
        .scoped_user_id_by_container(&resolved.user.user_id, container_id);

    let config = state.config_store.get().await;
    let selected_model_name =
        multimodal_models::resolve_image_model(&config, payload.model_name.as_deref())
            .map(|(name, _)| name)
            .ok_or_else(|| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    "image model is not configured".to_string(),
                )
            })?;
    let output_format = normalize_optional_string(payload.output_format.as_deref());
    let negative_prompt = normalize_optional_string(payload.negative_prompt.as_deref());
    let size = normalize_optional_string(payload.size.as_deref());
    let input_paths = collect_admin_image_input_paths(&payload);
    let input_images = load_admin_image_input_files(state.as_ref(), &workspace_id, &input_paths)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mask_image = load_optional_admin_image_input_file(
        state.as_ref(),
        &workspace_id,
        payload.mask_path.as_deref(),
    )
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let reference_image = load_optional_admin_image_input_file(
        state.as_ref(),
        &workspace_id,
        payload.reference_path.as_deref(),
    )
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let result = multimodal_models::generate_image(
        &config,
        ImageGenerationRequest {
            prompt: prompt.to_string(),
            model_name: Some(selected_model_name.clone()),
            size: size.clone(),
            output_format: output_format.clone(),
            negative_prompt: negative_prompt.clone(),
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
    .map_err(|err| {
        error_response(
            StatusCode::BAD_REQUEST,
            format!("image request failed: {err}"),
        )
    })?;

    let saved = persist_generated_media(
        state.as_ref(),
        &workspace_id,
        payload.base.path.as_deref(),
        "image",
        &extension_from_content_type(&result.content_type, "png"),
        &result.bytes,
    )
    .await?;

    Ok(Json(json!({
        "data": {
            "kind": "image",
            "user_id": resolved.user.user_id,
            "container_id": container_id,
            "workspace_id": workspace_id,
            "model_name": selected_model_name,
            "content_type": result.content_type,
            "size_bytes": saved.size_bytes,
            "workspace_relative_path": saved.workspace_relative_path,
            "public_path": saved.public_path,
            "request": {
                "prompt": prompt,
                "path": normalize_optional_string(payload.base.path.as_deref()),
                "model_name": payload.model_name,
                "size": size,
                "output_format": output_format,
                "negative_prompt": negative_prompt,
                "num_inference_steps": payload.num_inference_steps,
                "guidance_scale": payload.guidance_scale,
                "seed": payload.seed,
                "mode": if input_paths.is_empty() { "text_to_image" } else { "image_edit" },
                "input_paths": input_paths,
                "mask_path": normalize_optional_string(payload.mask_path.as_deref()),
                "reference_path": normalize_optional_string(payload.reference_path.as_deref()),
                "strength": payload.strength,
                "true_cfg_scale": payload.true_cfg_scale,
                "output_compression": payload.output_compression,
                "layers": payload.layers,
                "resolution": payload.resolution,
            }
        }
    })))
}

async fn admin_generate_video(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<AdminVideoRequest>,
) -> Result<Json<Value>, Response> {
    let prompt = payload.prompt.trim();
    if prompt.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "prompt is required".to_string(),
        ));
    }

    let request_user_id = requested_user_id(payload.base.user_id.as_deref());
    let resolved = resolve_user(&state, &headers, Some(request_user_id.as_str())).await?;
    let container_id = payload
        .base
        .container_id
        .map(normalize_workspace_container_id)
        .unwrap_or(USER_PRIVATE_CONTAINER_ID);
    let workspace_id = state
        .workspace
        .scoped_user_id_by_container(&resolved.user.user_id, container_id);

    let config = state.config_store.get().await;
    let selected_model_name =
        multimodal_models::resolve_video_model(&config, payload.model_name.as_deref())
            .map(|(name, _)| name)
            .ok_or_else(|| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    "video model is not configured".to_string(),
                )
            })?;
    let size = normalize_optional_string(payload.size.as_deref());
    let negative_prompt = normalize_optional_string(payload.negative_prompt.as_deref());
    let result = multimodal_models::generate_video(
        &config,
        VideoGenerationRequest {
            prompt: prompt.to_string(),
            model_name: Some(selected_model_name.clone()),
            size: size.clone(),
            seconds: payload.seconds,
            fps: payload.fps,
            num_frames: payload.num_frames,
            negative_prompt: negative_prompt.clone(),
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
    .await
    .map_err(|err| {
        error_response(
            StatusCode::BAD_REQUEST,
            format!("video request failed: {err}"),
        )
    })?;

    let saved = persist_generated_media(
        state.as_ref(),
        &workspace_id,
        payload.base.path.as_deref(),
        "video",
        &extension_from_content_type(&result.content_type, "mp4"),
        &result.bytes,
    )
    .await?;

    Ok(Json(json!({
        "data": {
            "kind": "video",
            "user_id": resolved.user.user_id,
            "container_id": container_id,
            "workspace_id": workspace_id,
            "model_name": selected_model_name,
            "content_type": result.content_type,
            "size_bytes": saved.size_bytes,
            "workspace_relative_path": saved.workspace_relative_path,
            "public_path": saved.public_path,
            "request": {
                "prompt": prompt,
                "path": normalize_optional_string(payload.base.path.as_deref()),
                "model_name": payload.model_name,
                "size": size,
                "seconds": payload.seconds,
                "fps": payload.fps,
                "num_frames": payload.num_frames,
                "negative_prompt": negative_prompt,
                "num_inference_steps": payload.num_inference_steps,
                "guidance_scale": payload.guidance_scale,
                "guidance_scale_2": payload.guidance_scale_2,
                "boundary_ratio": payload.boundary_ratio,
                "flow_shift": payload.flow_shift,
                "seed": payload.seed,
                "enable_frame_interpolation": payload.enable_frame_interpolation,
            }
        }
    })))
}

async fn persist_generated_media(
    state: &AppState,
    workspace_id: &str,
    requested_path: Option<&str>,
    prefix: &str,
    extension: &str,
    bytes: &Bytes,
) -> Result<PersistedMediaFile, Response> {
    state
        .workspace
        .ensure_user_root(workspace_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let relative = requested_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            let stamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
            let suffix = Uuid::new_v4().simple().to_string();
            format!(
                "{GENERATED_MEDIA_DIR}/{prefix}_{stamp}_{}.{}",
                &suffix[..6],
                extension
            )
        });
    let relative = ensure_extension(relative, extension);
    let target = state
        .workspace
        .resolve_path(workspace_id, &relative)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    fs::write(&target, bytes)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    state.workspace.mark_tree_dirty(workspace_id);

    Ok(PersistedMediaFile {
        public_path: state.workspace.display_path(workspace_id, &target),
        workspace_relative_path: relative.replace('\\', "/"),
        size_bytes: bytes.len(),
    })
}

fn collect_admin_image_input_paths(payload: &AdminImageRequest) -> Vec<String> {
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

async fn load_admin_image_input_files(
    state: &AppState,
    workspace_id: &str,
    paths: &[String],
) -> anyhow::Result<Vec<multimodal_models::ImageInputFile>> {
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        files.push(load_admin_image_input_file(state, workspace_id, path).await?);
    }
    Ok(files)
}

async fn load_optional_admin_image_input_file(
    state: &AppState,
    workspace_id: &str,
    path: Option<&str>,
) -> anyhow::Result<Option<multimodal_models::ImageInputFile>> {
    let Some(path) = normalize_optional_string(path) else {
        return Ok(None);
    };
    Ok(Some(
        load_admin_image_input_file(state, workspace_id, &path).await?,
    ))
}

async fn load_admin_image_input_file(
    state: &AppState,
    workspace_id: &str,
    public_or_relative_path: &str,
) -> anyhow::Result<multimodal_models::ImageInputFile> {
    let resolved = state
        .workspace
        .resolve_path(workspace_id, public_or_relative_path)?;
    if !resolved.exists() || !resolved.is_file() {
        return Err(anyhow::anyhow!(
            "image input file not found: {public_or_relative_path}"
        ));
    }
    let bytes = fs::read(&resolved).await?;
    if bytes.is_empty() {
        return Err(anyhow::anyhow!(
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

fn requested_user_id(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .unwrap_or(DEFAULT_DEBUG_USER_ID)
        .to_string()
}

async fn parse_admin_transcription_multipart(
    mut multipart: Multipart,
) -> Result<AdminTranscriptionFields, Response> {
    let mut fields = AdminTranscriptionFields::default();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        let name = field.name().unwrap_or("").trim().to_ascii_lowercase();
        match name.as_str() {
            "file" => {
                fields.upload_filename = field
                    .file_name()
                    .map(str::to_string)
                    .filter(|value| !value.trim().is_empty());
                fields.upload_content_type = field.content_type().map(str::to_string);
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
                if !bytes.is_empty() {
                    fields.upload_bytes = Some(bytes);
                }
            }
            "user_id" | "userid" => {
                let value = field
                    .text()
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
                fields.user_id = normalize_optional_string(Some(value.as_str()));
            }
            "container_id" | "containerid" => {
                let value = field
                    .text()
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
                fields.container_id = value.trim().parse::<i32>().ok();
            }
            "path" => {
                let value = field
                    .text()
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
                fields.path = normalize_optional_string(Some(value.as_str()));
            }
            "source_public_path" | "sourcepublicpath" => {
                let value = field
                    .text()
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
                fields.source_public_path = normalize_optional_string(Some(value.as_str()));
            }
            "model_name" | "modelname" => {
                let value = field
                    .text()
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
                fields.model_name = normalize_optional_string(Some(value.as_str()));
            }
            "language" => {
                let value = field
                    .text()
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
                fields.language = normalize_optional_string(Some(value.as_str()));
            }
            "prompt" => {
                let value = field
                    .text()
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
                fields.prompt = normalize_optional_string(Some(value.as_str()));
            }
            "response_format" | "responseformat" => {
                let value = field
                    .text()
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
                fields.response_format = normalize_optional_string(Some(value.as_str()));
            }
            "temperature" => {
                let value = field
                    .text()
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
                fields.temperature = value.trim().parse::<f32>().ok();
            }
            _ => {}
        }
    }
    Ok(fields)
}

fn normalize_optional_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
}

fn normalize_workspace_relative_path(public_path: &str) -> String {
    let normalized = public_path.trim().replace('\\', "/");
    if let Some(without_root) = normalized.strip_prefix("/workspaces/") {
        if let Some((_, relative)) = without_root.split_once('/') {
            return relative.to_string();
        }
    }
    if let Some(without_root) = normalized.strip_prefix("workspaces/") {
        if let Some((_, relative)) = without_root.split_once('/') {
            return relative.to_string();
        }
    }
    normalized
}

fn audio_content_type_from_path(path: &str) -> &'static str {
    let ext = Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "aac" => "audio/aac",
        "ogg" => "audio/ogg",
        "opus" => "audio/opus",
        "m4a" => "audio/mp4",
        _ => "application/octet-stream",
    }
}

fn ensure_extension(path: String, extension: &str) -> String {
    let normalized = path.replace('\\', "/");
    let current_ext = Path::new(&normalized)
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

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}
