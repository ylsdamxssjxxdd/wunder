use crate::attachment::sanitize_filename_stem;
use crate::schemas::AttachmentPayload;
use crate::storage::USER_PRIVATE_CONTAINER_ID;
use crate::workspace::WorkspaceManager;
use anyhow::Result;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use image::ImageFormat;
use mime::Mime;
use std::path::Path;
use std::str::FromStr;
use tokio::fs;
use tracing::warn;
use uuid::Uuid;

const CHAT_ATTACHMENT_DIR: &str = "chat_attachments";
const MAX_PERSIST_BYTES: usize = 10 * 1024 * 1024;

pub async fn persist_user_chat_attachments(
    workspace: &WorkspaceManager,
    user_id: &str,
    session_id: &str,
    attachments: &mut [AttachmentPayload],
) -> Result<()> {
    if attachments.is_empty() {
        return Ok(());
    }
    let workspace_id = workspace.scoped_user_id_by_container(user_id, USER_PRIVATE_CONTAINER_ID);
    workspace.ensure_user_root(&workspace_id)?;
    let safe_session = sanitize_session_id(session_id);
    let base_dir = format!("{CHAT_ATTACHMENT_DIR}/{safe_session}");

    for attachment in attachments.iter_mut() {
        if attachment
            .public_path
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
        {
            continue;
        }
        let raw = attachment.content.as_deref().unwrap_or("").trim();
        if raw.is_empty() {
            continue;
        }
        if raw.starts_with("/workspaces/") {
            attachment.public_path = Some(raw.to_string());
            continue;
        }
        let Some((mime_type, bytes)) = parse_data_url(raw, attachment.content_type.as_deref())
        else {
            continue;
        };
        if bytes.is_empty() {
            continue;
        }
        if mime_type.trim().to_ascii_lowercase().starts_with("image/")
            && (!is_supported_model_image_mime(&mime_type)
                || !validate_image_attachment_bytes(&mime_type, &bytes))
        {
            warn!(
                "chat attachment skipped invalid image payload: user_id={}, session_id={}, mime_type={}",
                user_id, session_id, mime_type
            );
            continue;
        }
        if bytes.len() > MAX_PERSIST_BYTES {
            warn!(
                "chat attachment too large to persist: user_id={}, session_id={}, bytes={}",
                user_id,
                session_id,
                bytes.len()
            );
            continue;
        }
        if attachment
            .content_type
            .as_deref()
            .map(|value| value.trim().is_empty())
            .unwrap_or(true)
        {
            attachment.content_type = Some(mime_type.clone());
        }
        let filename = build_attachment_filename(attachment.name.as_deref(), &mime_type);
        let relative = format!("{base_dir}/{filename}");
        let dest = match workspace.resolve_path(&workspace_id, &relative) {
            Ok(path) => path,
            Err(err) => {
                warn!(
                    "chat attachment resolve path failed: user_id={}, session_id={}, error={err}",
                    user_id, session_id
                );
                continue;
            }
        };
        if let Some(parent) = dest.parent() {
            if let Err(err) = fs::create_dir_all(parent).await {
                warn!(
                    "chat attachment create dir failed: user_id={}, session_id={}, error={err}",
                    user_id, session_id
                );
                continue;
            }
        }
        if let Err(err) = fs::write(&dest, &bytes).await {
            warn!(
                "chat attachment persist failed: user_id={}, session_id={}, error={err}",
                user_id, session_id
            );
            continue;
        }
        let public_path = workspace.display_path(&workspace_id, &dest);
        attachment.public_path = Some(public_path);
    }
    Ok(())
}

fn parse_data_url(raw: &str, hint: Option<&str>) -> Option<(String, Vec<u8>)> {
    if !raw.starts_with("data:") {
        return None;
    }
    let (header, data_part) = raw.split_once(',')?;
    let meta = header.trim_start_matches("data:");
    let mut mime_type = String::new();
    let mut is_base64 = false;
    for token in meta.split(';') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        if token.eq_ignore_ascii_case("base64") {
            is_base64 = true;
        } else if token.contains('/') {
            mime_type = token.to_string();
        }
    }
    if mime_type.is_empty() {
        if let Some(hint) = hint {
            mime_type = hint.trim().to_string();
        }
    }
    let cleaned_mime = mime_type.to_ascii_lowercase();
    if !cleaned_mime.starts_with("image/") && !cleaned_mime.starts_with("audio/") {
        return None;
    }
    if !is_base64 {
        return None;
    }
    let cleaned = data_part
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    let bytes = STANDARD.decode(cleaned.as_bytes()).ok()?;
    Some((mime_type, bytes))
}

pub fn parse_image_data_url(raw: &str, hint: Option<&str>) -> Option<(String, Vec<u8>)> {
    let (mime_type, bytes) = parse_data_url(raw, hint)?;
    if !mime_type.trim().to_ascii_lowercase().starts_with("image/") {
        return None;
    }
    if !is_supported_model_image_mime(&mime_type) {
        return None;
    }
    if !validate_image_attachment_bytes(&mime_type, &bytes) {
        return None;
    }
    Some((mime_type, bytes))
}

pub fn is_supported_model_image_mime(mime_type: &str) -> bool {
    matches!(
        normalize_model_image_mime(mime_type).as_deref(),
        Some("image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/bmp" | "image/tiff")
    )
}

pub fn validate_image_attachment_bytes(mime_type: &str, bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let Some(normalized) = normalize_model_image_mime(mime_type) else {
        return false;
    };
    let Some(expected_format) = image_format_for_model_image(&normalized) else {
        return false;
    };
    match image::guess_format(bytes).ok() {
        Some(actual) if actual == expected_format => {
            image::load_from_memory_with_format(bytes, actual).is_ok()
        }
        Some(_) => false,
        None => image::load_from_memory_with_format(bytes, expected_format).is_ok(),
    }
}

fn normalize_model_image_mime(mime_type: &str) -> Option<String> {
    let normalized = mime_type
        .split(';')
        .next()
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn image_format_for_model_image(mime_type: &str) -> Option<ImageFormat> {
    match mime_type {
        "image/png" => Some(ImageFormat::Png),
        "image/jpeg" => Some(ImageFormat::Jpeg),
        "image/gif" => Some(ImageFormat::Gif),
        "image/webp" => Some(ImageFormat::WebP),
        "image/bmp" => Some(ImageFormat::Bmp),
        "image/tiff" => Some(ImageFormat::Tiff),
        _ => None,
    }
}

fn build_attachment_filename(raw_name: Option<&str>, mime_type: &str) -> String {
    let fallback_stem = if mime_type.to_ascii_lowercase().starts_with("audio/") {
        "audio"
    } else {
        "image"
    };
    let (stem_raw, ext_raw) = split_filename(raw_name.unwrap_or(""));
    let stem_source = if stem_raw.is_empty() {
        fallback_stem
    } else {
        stem_raw.as_str()
    };
    let stem = sanitize_filename_stem(stem_source);
    let mut ext = ext_raw.or_else(|| extension_from_mime(mime_type));
    if ext.is_none() {
        ext = Some("bin".to_string());
    }
    let suffix = Uuid::new_v4().simple().to_string();
    format!(
        "{stem}_{suffix}.{}",
        ext.unwrap_or_else(|| "bin".to_string())
    )
}

fn split_filename(name: &str) -> (String, Option<String>) {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return (String::new(), None);
    }
    let path = Path::new(trimmed);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    (stem, ext)
}

fn extension_from_mime(mime_type: &str) -> Option<String> {
    let mime = Mime::from_str(mime_type).ok()?;
    let main = mime.type_().as_str();
    let sub = mime.subtype().as_str();
    if main == "image" {
        return Some(
            match sub {
                "jpeg" => "jpg",
                "png" => "png",
                "gif" => "gif",
                "webp" => "webp",
                "bmp" => "bmp",
                "svg+xml" => "svg",
                "tiff" => "tiff",
                other => other,
            }
            .to_string(),
        );
    }
    if main == "audio" {
        return Some(
            match sub {
                "mpeg" => "mp3",
                "wav" | "x-wav" => "wav",
                "ogg" => "ogg",
                "opus" => "opus",
                "aac" => "aac",
                "flac" => "flac",
                "webm" => "webm",
                "mp4" => "m4a",
                other => other,
            }
            .to_string(),
        );
    }
    None
}

fn sanitize_session_id(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return "default".to_string();
    }
    let sanitized = trimmed
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.trim().is_empty() {
        "default".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_image_data_url, validate_image_attachment_bytes};
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

    const TINY_PNG_BASE64: &str =
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+yF9sAAAAASUVORK5CYII=";

    #[test]
    fn validate_image_attachment_bytes_accepts_valid_png() {
        let bytes = STANDARD
            .decode(TINY_PNG_BASE64.as_bytes())
            .expect("decode png");
        assert!(validate_image_attachment_bytes("image/png", &bytes));
    }

    #[test]
    fn validate_image_attachment_bytes_rejects_invalid_png_payload() {
        assert!(!validate_image_attachment_bytes(
            "image/png",
            b"encrypted payload"
        ));
    }

    #[test]
    fn parse_image_data_url_rejects_invalid_png_payload() {
        let raw = format!(
            "data:image/png;base64,{}",
            STANDARD.encode("encrypted payload".as_bytes())
        );
        assert!(parse_image_data_url(&raw, Some("image/png")).is_none());
    }
}
