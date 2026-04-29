use super::{build_model_tool_success, ToolContext};
use crate::schemas::AttachmentPayload;
use crate::services::chat_media::{detect_media_kind_from_path, process_visual_media_path};
use anyhow::{anyhow, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;

const MAX_READ_VISUAL_MEDIA_BYTES: u64 = 128 * 1024 * 1024;

pub const TOOL_READ_IMAGE: &str = "\u{8bfb}\u{56fe}\u{5de5}\u{5177}";
pub const TOOL_READ_IMAGE_ALIAS: &str = "read_image";
pub const TOOL_VIEW_IMAGE_ALIAS: &str = "view_image";

#[derive(Debug, Deserialize)]
struct ReadImageArgs {
    path: String,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    frame_rate: Option<f64>,
    #[serde(default)]
    frame_step: Option<usize>,
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
    if metadata.len() > MAX_READ_VISUAL_MEDIA_BYTES {
        return Err(anyhow!(crate::i18n::t("tool.read.too_large")));
    }

    let sample = read_visual_media_sample(&resolved, 512).await?;
    let media_kind = detect_media_kind_from_path(&resolved, &sample)
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.read_image.not_image")))?;
    let result = process_visual_media_path(
        context.workspace.as_ref(),
        context.config,
        context.workspace_id,
        &resolved,
        None,
        payload.frame_rate,
        payload.frame_step,
    )
    .await?;
    Ok(build_model_tool_success(
        "read_image",
        "completed",
        format!("Prepared visual media {} for model inspection.", raw_path),
        json!({
            "path": raw_path,
            "resolved_path": resolved.to_string_lossy().to_string(),
            "media_kind": media_kind,
            "size_bytes": metadata.len(),
            "prompt": normalize_optional_prompt(payload.prompt.as_deref()),
            "result": result,
        }),
    ))
}

pub async fn build_followup_user_message(
    context: &ToolContext<'_>,
    result_data: &Value,
) -> Result<Option<Value>> {
    let Some(result) = parse_result_payload(result_data) else {
        return Ok(None);
    };
    let prompt = result
        .prompt
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| crate::i18n::t("tool.read_image.followup_prompt"));
    let attachments = result
        .result
        .get("attachments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let single_attachment = attachments.len() == 1;
    let mut content = vec![json!({ "type": "text", "text": prompt })];
    for item in attachments {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let content_type = obj
            .get("content_type")
            .or_else(|| obj.get("mime_type"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if content_type.starts_with("image/") {
            let attachment = AttachmentPayload {
                name: obj
                    .get("name")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                content: None,
                content_type: Some(content_type.clone()),
                public_path: obj
                    .get("public_path")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            };
            if let Some(image_url) = crate::services::chat_media::load_image_attachment_data_url(
                context.workspace.as_ref(),
                &attachment,
            )
            .await
            {
                content.push(json!({
                    "type": "image_url",
                    "image_url": { "url": image_url }
                }));
            } else if result.media_kind == "image" && single_attachment {
                let bytes = tokio::fs::read(&result.resolved_path)
                    .await
                    .map_err(|_| anyhow!(crate::i18n::t("tool.read.not_found")))?;
                let data_url = format!("data:{content_type};base64,{}", STANDARD.encode(bytes));
                content.push(json!({
                    "type": "image_url",
                    "image_url": { "url": data_url }
                }));
            }
            continue;
        }
        let text = obj
            .get("content")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(text) = text {
            content.push(json!({
                "type": "text",
                "text": text
            }));
        }
    }
    if content.len() <= 1 {
        return Ok(None);
    }
    Ok(Some(json!({
        "role": "user",
        "content": content
    })))
}

async fn read_visual_media_sample(path: &Path, max_bytes: usize) -> Result<Vec<u8>> {
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

fn normalize_optional_prompt(prompt: Option<&str>) -> Option<String> {
    prompt
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

struct ReadImageResultPayload {
    prompt: Option<String>,
    resolved_path: PathBuf,
    media_kind: String,
    result: Value,
}

fn parse_result_payload(data: &Value) -> Option<ReadImageResultPayload> {
    let obj = data.as_object()?;
    let resolved_path = obj
        .get("resolved_path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    Some(ReadImageResultPayload {
        prompt: normalize_optional_prompt(obj.get("prompt").and_then(Value::as_str)),
        resolved_path: PathBuf::from(resolved_path),
        media_kind: obj
            .get("media_kind")
            .and_then(Value::as_str)
            .unwrap_or("image")
            .to_string(),
        result: obj.get("result").cloned().unwrap_or(Value::Null),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_image_tool_name_supports_aliases() {
        assert!(is_read_image_tool_name(TOOL_READ_IMAGE));
        assert!(is_read_image_tool_name(TOOL_READ_IMAGE_ALIAS));
        assert!(is_read_image_tool_name(TOOL_VIEW_IMAGE_ALIAS));
        assert!(!is_read_image_tool_name("read_file"));
    }
}
