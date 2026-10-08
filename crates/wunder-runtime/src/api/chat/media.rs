use super::error_response;
use crate::api::attachment_convert::{build_conversion_payload, convert_multipart_list};
use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::services::chat_media::{
    process_chat_media_upload, reprocess_chat_media_source, ChatMediaUpload,
};
use crate::services::multimodal_models::{self, SpeechSynthesisRequest};
use crate::state::AppState;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use axum::{routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

const MAX_ATTACHMENT_UPLOAD_BYTES: usize = 10 * 1024 * 1024;
const MAX_MEDIA_UPLOAD_BYTES: usize = 128 * 1024 * 1024;
const MAX_TTS_INPUT_CHARS: usize = 8_000;

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/chat/attachments/convert",
            post(chat_attachment_convert).layer(DefaultBodyLimit::max(MAX_ATTACHMENT_UPLOAD_BYTES)),
        )
        .route(
            "/wunder/chat/attachments/media/process",
            post(chat_attachment_media_process)
                .layer(DefaultBodyLimit::max(MAX_MEDIA_UPLOAD_BYTES)),
        )
        .route("/wunder/chat/tts", post(synthesize_chat_tts))
}

async fn chat_attachment_convert(multipart: Multipart) -> Result<Json<Value>, Response> {
    let conversions = convert_multipart_list(multipart).await?;
    Ok(Json(json!({
        "data": build_conversion_payload(conversions),
    })))
}

#[derive(Default)]
struct ChatMediaProcessFields {
    upload: Option<ChatMediaUpload>,
    source_public_path: Option<String>,
    frame_rate: Option<f64>,
    frame_step: Option<usize>,
}

async fn chat_attachment_media_process(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let fields = parse_chat_media_process_multipart(multipart).await?;
    let config = state.config_store.get().await;
    let result = if let Some(upload) = fields.upload {
        process_chat_media_upload(
            &state.workspace,
            &config,
            &resolved.user.user_id,
            upload,
            fields.frame_rate,
            fields.frame_step,
        )
        .await
    } else if let Some(source_public_path) = fields.source_public_path.as_deref() {
        reprocess_chat_media_source(
            &state.workspace,
            &config,
            &resolved.user.user_id,
            source_public_path,
            fields.frame_rate,
            fields.frame_step,
        )
        .await
    } else {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "file or source_public_path is required".to_string(),
        ));
    }
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": result,
    })))
}

async fn parse_chat_media_process_multipart(
    mut multipart: Multipart,
) -> Result<ChatMediaProcessFields, Response> {
    let mut fields = ChatMediaProcessFields::default();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        let name = field.name().unwrap_or("").trim().to_ascii_lowercase();
        match name.as_str() {
            "file" => {
                let filename = field
                    .file_name()
                    .map(str::to_string)
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| "media".to_string());
                let content_type = field.content_type().map(str::to_string);
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
                if bytes.is_empty() {
                    return Err(error_response(
                        StatusCode::BAD_REQUEST,
                        "uploaded media file is empty".to_string(),
                    ));
                }
                fields.upload = Some(ChatMediaUpload {
                    filename,
                    content_type,
                    bytes: bytes.to_vec(),
                });
            }
            "source_public_path" | "sourcepublicpath" => {
                let value = field
                    .text()
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
                let normalized = value.trim().to_string();
                if !normalized.is_empty() {
                    fields.source_public_path = Some(normalized);
                }
            }
            "frame_rate" | "framerate" | "fps" => {
                let value = field
                    .text()
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let parsed = trimmed.parse::<f64>().map_err(|_| {
                    error_response(
                        StatusCode::BAD_REQUEST,
                        "frame_rate must be a number".to_string(),
                    )
                })?;
                fields.frame_rate = Some(parsed);
            }
            "frame_step" | "framestep" | "gif_frame_step" | "frame_interval" => {
                let value = field
                    .text()
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let parsed = trimmed.parse::<usize>().map_err(|_| {
                    error_response(
                        StatusCode::BAD_REQUEST,
                        "frame_step must be a non-negative integer".to_string(),
                    )
                })?;
                fields.frame_step = Some(parsed);
            }
            _ => {}
        }
    }
    Ok(fields)
}

#[derive(Debug, Deserialize)]
struct ChatTtsRequest {
    text: String,
    #[serde(default)]
    model_name: Option<String>,
    #[serde(default)]
    voice: Option<String>,
    #[serde(default)]
    instructions: Option<String>,
    #[serde(default)]
    response_format: Option<String>,
    #[serde(default)]
    speed: Option<f32>,
}

async fn synthesize_chat_tts(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ChatTtsRequest>,
) -> Result<Response, Response> {
    let _resolved = resolve_user(&state, &headers, None).await?;
    let text = payload.text.trim();
    if text.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.param_required"),
        ));
    }
    if text.chars().count() > MAX_TTS_INPUT_CHARS {
        return Err(error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("tts input exceeds {MAX_TTS_INPUT_CHARS} characters"),
        ));
    }

    let config = state.config_store.get().await;
    let result = multimodal_models::synthesize_speech(
        &config,
        SpeechSynthesisRequest {
            text: text.to_string(),
            model_name: payload.model_name,
            voice: payload.voice,
            instructions: payload.instructions,
            response_format: payload.response_format,
            speed: payload.speed,
            ref_audio: None,
            ref_text: None,
            model_specific_params: None,
        },
    )
    .await
    .map_err(|err| {
        error_response(
            StatusCode::BAD_REQUEST,
            format!("tts request failed: {err}"),
        )
    })?;

    let mut response = Response::new(Body::from(result.bytes));
    if let Ok(value) = HeaderValue::from_str(&result.content_type) {
        response.headers_mut().insert(header::CONTENT_TYPE, value);
    }
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, max-age=0"),
    );
    Ok(response)
}
