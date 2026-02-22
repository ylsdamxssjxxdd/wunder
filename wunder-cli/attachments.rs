use crate::locale;
use crate::runtime::CliRuntime;
use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use std::path::{Path, PathBuf};
use wunder_server::attachment::{
    convert_to_markdown, get_supported_extensions, sanitize_filename_stem,
};
use wunder_server::schemas::AttachmentPayload;

const MAX_ATTACHMENT_FILE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_ATTACHMENT_TEXT_CHARS: usize = 180_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttachmentKind {
    Image,
    Text,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedAttachment {
    pub source: String,
    pub payload: AttachmentPayload,
    pub kind: AttachmentKind,
    pub size_bytes: u64,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AttachAction {
    Show,
    Clear,
    Drop(usize),
    Add(String),
}

pub(crate) fn parse_attach_action(args: &str) -> Result<AttachAction> {
    let cleaned = args.trim();
    if cleaned.is_empty()
        || cleaned.eq_ignore_ascii_case("show")
        || cleaned.eq_ignore_ascii_case("list")
    {
        return Ok(AttachAction::Show);
    }
    if cleaned.eq_ignore_ascii_case("clear")
        || cleaned.eq_ignore_ascii_case("off")
        || cleaned.eq_ignore_ascii_case("none")
    {
        return Ok(AttachAction::Clear);
    }
    if let Some(rest) = cleaned.strip_prefix("drop ") {
        let index = rest
            .trim()
            .parse::<usize>()
            .map_err(|_| anyhow!("invalid /attach drop index"))?;
        if index == 0 {
            return Err(anyhow!("invalid /attach drop index"));
        }
        return Ok(AttachAction::Drop(index));
    }
    if let Some(rest) = cleaned.strip_prefix("add ") {
        let path = normalize_attachment_path_input(rest);
        if path.is_empty() {
            return Err(anyhow!("missing attachment path"));
        }
        return Ok(AttachAction::Add(path));
    }
    let path = normalize_attachment_path_input(cleaned);
    if path.is_empty() {
        return Err(anyhow!("missing attachment path"));
    }
    Ok(AttachAction::Add(path))
}

pub(crate) fn attach_usage(language: &str) -> String {
    locale::tr(
        language,
        "用法: /attach [list|clear|drop <index>|<path>]",
        "usage: /attach [list|clear|drop <index>|<path>]",
    )
}

pub(crate) fn summarize_attachment(
    item: &PreparedAttachment,
    index: usize,
    language: &str,
) -> String {
    let name = item
        .payload
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("attachment");
    let kind = match item.kind {
        AttachmentKind::Image => locale::tr(language, "图片", "image"),
        AttachmentKind::Text => locale::tr(language, "文档", "text"),
    };
    let base = format!(
        "{:>2}. {} [{}] {} bytes <- {}",
        index + 1,
        name,
        kind,
        item.size_bytes,
        item.source
    );
    if let Some(detail) = item
        .detail
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        return format!("{base} ({detail})");
    }
    base
}

pub(crate) fn to_request_attachments(
    items: &[PreparedAttachment],
) -> Option<Vec<AttachmentPayload>> {
    if items.is_empty() {
        return None;
    }
    Some(items.iter().map(|item| item.payload.clone()).collect())
}

pub(crate) async fn prepare_attachment_from_path(
    runtime: &CliRuntime,
    raw_path: &str,
) -> Result<PreparedAttachment> {
    let cleaned = normalize_attachment_path_input(raw_path);
    if cleaned.is_empty() {
        return Err(anyhow!("attachment path is empty"));
    }
    let target_path = resolve_attachment_path(runtime.launch_dir.as_path(), cleaned.as_str());
    let metadata = tokio::fs::metadata(&target_path)
        .await
        .with_context(|| format!("attachment not found: {}", target_path.display()))?;
    if !metadata.is_file() {
        return Err(anyhow!(
            "attachment path is not a file: {}",
            target_path.display()
        ));
    }
    let file_size = metadata.len();
    if file_size == 0 {
        return Err(anyhow!(
            "attachment is empty: {}",
            target_path.to_string_lossy()
        ));
    }
    if file_size > MAX_ATTACHMENT_FILE_BYTES {
        return Err(anyhow!(
            "attachment is too large (> {} bytes): {}",
            MAX_ATTACHMENT_FILE_BYTES,
            target_path.to_string_lossy()
        ));
    }

    let extension = file_extension_with_dot(target_path.as_path());
    if let Some(mime) = image_mime_by_extension(extension.as_str()) {
        let bytes = tokio::fs::read(&target_path)
            .await
            .with_context(|| format!("read image attachment failed: {}", target_path.display()))?;
        let encoded = STANDARD.encode(bytes);
        let content = format!("data:{mime};base64,{encoded}");
        let file_name = file_name_or_default(target_path.as_path(), "image");
        return Ok(PreparedAttachment {
            source: display_path(runtime.launch_dir.as_path(), target_path.as_path()),
            payload: AttachmentPayload {
                name: Some(file_name),
                content: Some(content),
                content_type: Some(mime.to_string()),
            },
            kind: AttachmentKind::Image,
            size_bytes: file_size,
            detail: Some("data-url".to_string()),
        });
    }

    let text = load_text_attachment(runtime, target_path.as_path(), extension.as_str()).await?;
    guard_text_size(text.as_str())?;
    let file_name = file_name_or_default(target_path.as_path(), "document");
    Ok(PreparedAttachment {
        source: display_path(runtime.launch_dir.as_path(), target_path.as_path()),
        payload: AttachmentPayload {
            name: Some(file_name),
            content: Some(text),
            content_type: Some("text/markdown".to_string()),
        },
        kind: AttachmentKind::Text,
        size_bytes: file_size,
        detail: None,
    })
}

fn normalize_attachment_path_input(raw: &str) -> String {
    let cleaned = raw.trim();
    if cleaned.len() >= 2 {
        let first = cleaned.chars().next().unwrap_or_default();
        let last = cleaned.chars().last().unwrap_or_default();
        if (first == '"' && last == '"') || (first == '\'' && last == '\'') {
            return cleaned[1..cleaned.len() - 1].trim().to_string();
        }
    }
    cleaned.to_string()
}

fn resolve_attachment_path(launch_dir: &Path, input: &str) -> PathBuf {
    let candidate = PathBuf::from(input);
    if candidate.is_absolute() {
        return candidate;
    }
    launch_dir.join(candidate)
}

fn display_path(root: &Path, path: &Path) -> String {
    let absolute = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if let Ok(relative) = absolute.strip_prefix(root) {
        let text = relative.to_string_lossy().to_string();
        if !text.trim().is_empty() {
            return text;
        }
    }
    absolute.to_string_lossy().to_string()
}

fn file_extension_with_dot(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{}", value.to_ascii_lowercase()))
        .unwrap_or_default()
}

fn image_mime_by_extension(extension: &str) -> Option<&'static str> {
    match extension {
        ".png" => Some("image/png"),
        ".jpg" | ".jpeg" => Some("image/jpeg"),
        ".gif" => Some("image/gif"),
        ".webp" => Some("image/webp"),
        ".bmp" => Some("image/bmp"),
        ".svg" => Some("image/svg+xml"),
        ".ico" => Some("image/x-icon"),
        ".tif" | ".tiff" => Some("image/tiff"),
        ".avif" => Some("image/avif"),
        _ => None,
    }
}

async fn load_text_attachment(
    runtime: &CliRuntime,
    input_path: &Path,
    extension: &str,
) -> Result<String> {
    let supported = get_supported_extensions();
    if supported
        .iter()
        .any(|item| item.eq_ignore_ascii_case(extension))
    {
        return convert_with_doc2md(runtime, input_path, extension).await;
    }

    let bytes = tokio::fs::read(input_path)
        .await
        .with_context(|| format!("read attachment failed: {}", input_path.display()))?;
    String::from_utf8(bytes).map_err(|_| {
        anyhow!(
            "unsupported binary attachment type: {}",
            input_path.to_string_lossy()
        )
    })
}

async fn convert_with_doc2md(
    runtime: &CliRuntime,
    input_path: &Path,
    extension: &str,
) -> Result<String> {
    let stem = input_path
        .file_stem()
        .and_then(|value| value.to_str())
        .map(sanitize_filename_stem)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "document".to_string());
    let output_dir = runtime.temp_root.join("attachments");
    tokio::fs::create_dir_all(&output_dir)
        .await
        .with_context(|| {
            format!(
                "create attachment temp dir failed: {}",
                output_dir.display()
            )
        })?;
    let output_path = output_dir.join(format!("{stem}_{}.md", uuid::Uuid::new_v4().simple()));
    let conversion = convert_to_markdown(input_path, &output_path, extension)
        .await
        .with_context(|| format!("convert attachment failed: {}", input_path.display()))?;
    let text = tokio::fs::read_to_string(&output_path)
        .await
        .with_context(|| {
            format!(
                "read converted attachment failed: {}",
                output_path.display()
            )
        })?;
    if let Err(err) = tokio::fs::remove_file(&output_path).await {
        tracing::debug!(
            "remove converted attachment temp failed: {}, {}",
            output_path.display(),
            err
        );
    }
    if text.trim().is_empty() {
        return Err(anyhow!("attachment conversion produced empty text"));
    }
    if !conversion.warnings.is_empty() {
        let warnings = conversion.warnings.join("; ");
        return Ok(format!("{text}\n\n> [converter-warning] {warnings}"));
    }
    Ok(text)
}

fn guard_text_size(text: &str) -> Result<()> {
    let count = text.chars().count();
    if count > MAX_ATTACHMENT_TEXT_CHARS {
        return Err(anyhow!(
            "attachment text is too long (> {} chars)",
            MAX_ATTACHMENT_TEXT_CHARS
        ));
    }
    Ok(())
}

fn file_name_or_default(path: &Path, fallback: &str) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| fallback.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_attach_action_defaults_to_show() {
        assert_eq!(parse_attach_action("").unwrap(), AttachAction::Show);
        assert_eq!(parse_attach_action("list").unwrap(), AttachAction::Show);
    }

    #[test]
    fn parse_attach_action_supports_drop_and_add() {
        assert_eq!(
            parse_attach_action("drop 2").unwrap(),
            AttachAction::Drop(2)
        );
        assert_eq!(
            parse_attach_action("add ./foo.md").unwrap(),
            AttachAction::Add("./foo.md".to_string())
        );
        assert_eq!(
            parse_attach_action("\"C:\\\\demo\\\\a.md\"").unwrap(),
            AttachAction::Add("C:\\\\demo\\\\a.md".to_string())
        );
    }

    #[test]
    fn image_mime_resolution_works() {
        assert_eq!(image_mime_by_extension(".png"), Some("image/png"));
        assert_eq!(image_mime_by_extension(".jpg"), Some("image/jpeg"));
        assert_eq!(image_mime_by_extension(".txt"), None);
    }
}
