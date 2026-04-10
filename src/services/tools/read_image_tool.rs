use super::{build_model_tool_success, ToolContext};
use anyhow::{anyhow, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;

const MAX_READ_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

pub const TOOL_READ_IMAGE: &str = "\u{8bfb}\u{56fe}\u{5de5}\u{5177}";
pub const TOOL_READ_IMAGE_ALIAS: &str = "read_image";
pub const TOOL_VIEW_IMAGE_ALIAS: &str = "view_image";

#[derive(Debug, Deserialize)]
struct ReadImageArgs {
    path: String,
    #[serde(default)]
    prompt: Option<String>,
}

pub fn is_read_image_tool_name(name: &str) -> bool {
    let cleaned = name.trim();
    if cleaned == TOOL_READ_IMAGE {
        return true;
    }
    matches!(
        cleaned.to_ascii_lowercase().as_str(),
        TOOL_READ_IMAGE_ALIAS | TOOL_VIEW_IMAGE_ALIAS
    )
}

pub async fn tool_read_image(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: ReadImageArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let raw_path = payload.path.trim();
    if raw_path.is_empty() {
        return Err(anyhow!(crate::i18n::t("tool.read.no_path")));
    }

    let extra_roots = super::collect_read_roots(context);
    let resolved = super::resolve_tool_path(
        context.workspace.as_ref(),
        context.workspace_id,
        raw_path,
        &extra_roots,
    )?;

    let metadata = tokio::fs::metadata(&resolved)
        .await
        .map_err(|_| anyhow!(crate::i18n::t("tool.read.not_found")))?;
    if !metadata.is_file() {
        return Err(anyhow!(crate::i18n::t("tool.read_image.not_file")));
    }
    if metadata.len() > MAX_READ_IMAGE_BYTES {
        return Err(anyhow!(crate::i18n::t("tool.read.too_large")));
    }

    let sample = read_image_sample(&resolved, 512).await?;
    let mime_type = detect_image_mime(&resolved, &sample)
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.read_image.not_image")))?;

    Ok(build_model_tool_success(
        "read_image",
        "completed",
        format!("Prepared image {} for model inspection.", raw_path),
        json!({
            "path": raw_path,
            "resolved_path": resolved.to_string_lossy().to_string(),
            "mime_type": mime_type,
            "size_bytes": metadata.len(),
            "prompt": normalize_optional_prompt(payload.prompt.as_deref()),
        }),
    ))
}

pub async fn build_followup_user_message(result_data: &Value) -> Result<Option<Value>> {
    let Some(result) = parse_result_payload(result_data) else {
        return Ok(None);
    };

    let bytes = tokio::fs::read(&result.resolved_path)
        .await
        .map_err(|_| anyhow!(crate::i18n::t("tool.read.not_found")))?;
    if bytes.len() as u64 > MAX_READ_IMAGE_BYTES {
        return Err(anyhow!(crate::i18n::t("tool.read.too_large")));
    }
    let mime_type = detect_image_mime(&result.resolved_path, &bytes)
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.read_image.not_image")))?;
    let data_url = format!("data:{mime_type};base64,{}", STANDARD.encode(bytes));

    let prompt = result
        .prompt
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| crate::i18n::t("tool.read_image.followup_prompt"));

    Ok(Some(json!({
        "role": "user",
        "content": [
            { "type": "text", "text": prompt },
            { "type": "image_url", "image_url": { "url": data_url } }
        ]
    })))
}

async fn read_image_sample(path: &Path, max_bytes: usize) -> Result<Vec<u8>> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|err| anyhow!(format!("{}: {err}", crate::i18n::t("tool.read.not_found"))))?;
    let mut buffer = vec![0_u8; max_bytes];
    let read = file
        .read(&mut buffer)
        .await
        .map_err(|err| anyhow!(format!("{}: {err}", crate::i18n::t("tool.read.not_found"))))?;
    buffer.truncate(read);
    Ok(buffer)
}

fn detect_image_mime(path: &Path, bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 8 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.len() >= 3 && bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.len() >= 6 && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.len() >= 2 && bytes.starts_with(b"BM") {
        return Some("image/bmp");
    }
    if bytes.len() >= 4
        && (bytes.starts_with(&[0x49, 0x49, 0x2A, 0x00])
            || bytes.starts_with(&[0x4D, 0x4D, 0x00, 0x2A]))
    {
        return Some("image/tiff");
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        let brand = &bytes[8..12];
        if brand == b"avif" || brand == b"avis" {
            return Some("image/avif");
        }
    }
    detect_mime_by_extension(path)
}

fn detect_mime_by_extension(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.trim().to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "tif" | "tiff" => Some("image/tiff"),
        "svg" => Some("image/svg+xml"),
        "avif" => Some("image/avif"),
        _ => None,
    }
}

fn normalize_optional_prompt(prompt: Option<&str>) -> Option<String> {
    prompt
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

struct ReadImageResultPayload {
    resolved_path: PathBuf,
    prompt: Option<String>,
}

fn parse_result_payload(data: &Value) -> Option<ReadImageResultPayload> {
    let obj = data.as_object()?;
    let resolved = obj
        .get("resolved_path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    Some(ReadImageResultPayload {
        resolved_path: PathBuf::from(resolved),
        prompt: normalize_optional_prompt(obj.get("prompt").and_then(Value::as_str)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    const ONE_PIXEL_GIF: &[u8] = &[
        71, 73, 70, 56, 57, 97, 1, 0, 1, 0, 128, 0, 0, 0, 0, 0, 255, 255, 255, 33, 249, 4, 1, 0, 0,
        0, 0, 44, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 68, 1, 0, 59,
    ];

    #[test]
    fn read_image_tool_name_supports_aliases() {
        assert!(is_read_image_tool_name(TOOL_READ_IMAGE));
        assert!(is_read_image_tool_name(TOOL_READ_IMAGE_ALIAS));
        assert!(is_read_image_tool_name(TOOL_VIEW_IMAGE_ALIAS));
        assert!(!is_read_image_tool_name("read_file"));
    }

    #[tokio::test]
    async fn build_followup_user_message_with_data_url() {
        let file_path =
            std::env::temp_dir().join(format!("wunder-read-image-{}.gif", Uuid::new_v4()));
        tokio::fs::write(&file_path, ONE_PIXEL_GIF)
            .await
            .expect("write temp image");

        let result = build_followup_user_message(&json!({
            "resolved_path": file_path.to_string_lossy().to_string(),
            "prompt": "analyze this image"
        }))
        .await
        .expect("followup message should build");

        let _ = tokio::fs::remove_file(&file_path).await;

        let message = result.expect("followup message expected");
        let content = message
            .get("content")
            .and_then(Value::as_array)
            .expect("content array");
        assert_eq!(content.len(), 2);
        let image_url = content[1]
            .get("image_url")
            .and_then(Value::as_object)
            .and_then(|obj| obj.get("url"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(image_url.starts_with("data:image/gif;base64,"));
    }
}
