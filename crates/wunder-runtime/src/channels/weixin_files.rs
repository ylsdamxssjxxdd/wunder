use crate::channels::types::{ChannelMessage, WeixinConfig};
use crate::channels::weixin;
use crate::channels::workspace_routing::resolve_channel_workspace_id;
use crate::user_store::UserStore;
use crate::workspace::WorkspaceManager;
use anyhow::Result;
use reqwest::Client;
use std::path::{Path, PathBuf};
use tokio::fs;
use uuid::Uuid;

const MAX_WEIXIN_MEDIA_BYTES: usize = 100 * 1024 * 1024;

pub async fn download_weixin_attachments_to_workspace(
    http: &Client,
    workspace: &WorkspaceManager,
    user_store: &UserStore,
    config: &WeixinConfig,
    user_id: &str,
    agent_id: Option<&str>,
    message: &mut ChannelMessage,
) -> Result<()> {
    if message.attachments.is_empty() {
        return Ok(());
    }
    let entries = weixin::extract_media_entries_from_message_meta(message.meta.as_ref());
    if entries.is_empty() {
        return Ok(());
    }

    let workspace_id = resolve_workspace_id(workspace, user_store, user_id, agent_id);
    workspace.ensure_user_root(&workspace_id)?;
    let message_id = message
        .message_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let base_dir = build_channel_inbound_dir(weixin::WEIXIN_CHANNEL, message_id);

    let total = std::cmp::min(message.attachments.len(), entries.len());
    for (index, entry) in entries.iter().enumerate().take(total) {
        let attachment = &mut message.attachments[index];

        let encrypted_url = if attachment.url.trim().is_empty() {
            weixin::build_cdn_download_url(config, &entry.encrypt_query_param)
        } else {
            attachment.url.trim().to_string()
        };

        let response = match http.get(&encrypted_url).send().await {
            Ok(value) => value,
            Err(_) => continue,
        };
        if !response.status().is_success() {
            continue;
        }
        if response.content_length().unwrap_or(0) > MAX_WEIXIN_MEDIA_BYTES as u64 {
            continue;
        }

        let raw_bytes = match response.bytes().await {
            Ok(value) => value.to_vec(),
            Err(_) => continue,
        };
        if raw_bytes.is_empty() || raw_bytes.len() > MAX_WEIXIN_MEDIA_BYTES {
            continue;
        }

        let decrypted = match weixin::decrypt_inbound_media_bytes(
            &raw_bytes,
            entry.aes_key.as_deref(),
            entry.aes_hex_key.as_deref(),
        ) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if decrypted.is_empty() || decrypted.len() > MAX_WEIXIN_MEDIA_BYTES {
            continue;
        }

        let mut display_name = entry
            .file_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                attachment
                    .name
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| default_filename(&entry.kind));
        display_name = sanitize_filename(&display_name);
        if display_name.is_empty() {
            display_name = format!("file_{}", Uuid::new_v4().simple());
        }

        let mut mime = attachment
            .mime
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| entry.mime_hint.clone())
            .or_else(|| infer_mime_from_filename(&display_name));
        let sniffed_mime = infer_mime_from_magic_bytes(&decrypted).map(str::to_string);
        if mime
            .as_deref()
            .map(str::trim)
            .map(|value| value.is_empty() || value.ends_with("/*"))
            .unwrap_or(true)
        {
            if let Some(sniffed) = sniffed_mime.clone() {
                mime = Some(sniffed);
            }
        }

        if !has_extension(&display_name) {
            if let Some(ext) =
                extension_from_mime(mime.as_deref()).or_else(|| extension_from_kind(&entry.kind))
            {
                display_name.push('.');
                display_name.push_str(ext);
            }
        }
        if mime.is_none() {
            mime = infer_mime_from_filename(&display_name);
        }

        let relative_path = format!("{base_dir}/{display_name}");
        let target = match workspace.resolve_path(&workspace_id, &relative_path) {
            Ok(path) => path,
            Err(_) => continue,
        };
        if let Some(parent) = target.parent() {
            if fs::create_dir_all(parent).await.is_err() {
                continue;
            }
        }
        let target = match ensure_unique_path(target).await {
            Ok(value) => value,
            Err(_) => continue,
        };
        if fs::write(&target, &decrypted).await.is_err() {
            continue;
        }

        let public_path = workspace.display_path(&workspace_id, &target);
        attachment.url = public_path;
        if attachment
            .name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            attachment.name = Some(display_name);
        }
        attachment.mime = mime;
        attachment.size = Some(decrypted.len() as i64);
    }

    Ok(())
}

fn resolve_workspace_id(
    workspace: &WorkspaceManager,
    user_store: &UserStore,
    user_id: &str,
    agent_id: Option<&str>,
) -> String {
    resolve_channel_workspace_id(workspace, user_store, user_id, agent_id)
}

fn default_filename(kind: &str) -> String {
    match kind.trim().to_ascii_lowercase().as_str() {
        "image" => format!("image_{}.jpg", Uuid::new_v4().simple()),
        "video" => format!("video_{}.mp4", Uuid::new_v4().simple()),
        "audio" => format!("audio_{}.silk", Uuid::new_v4().simple()),
        _ => format!("file_{}.bin", Uuid::new_v4().simple()),
    }
}

fn has_extension(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn infer_mime_from_filename(name: &str) -> Option<String> {
    let ext = Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())?;
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "silk" => "audio/silk",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "json" => "application/json",
        _ => "application/octet-stream",
    };
    Some(mime.to_string())
}

fn extension_from_mime(mime: Option<&str>) -> Option<&'static str> {
    let mime = mime?.trim().to_ascii_lowercase();
    if mime == "image/*" {
        return Some("jpg");
    }
    if mime == "video/*" {
        return Some("mp4");
    }
    if mime == "audio/*" {
        return Some("silk");
    }
    match mime.as_str() {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "video/mp4" => Some("mp4"),
        "video/quicktime" => Some("mov"),
        "audio/mpeg" => Some("mp3"),
        "audio/wav" => Some("wav"),
        "audio/silk" => Some("silk"),
        "application/pdf" => Some("pdf"),
        "text/plain" => Some("txt"),
        "application/json" => Some("json"),
        _ => None,
    }
}

fn extension_from_kind(kind: &str) -> Option<&'static str> {
    match kind.trim().to_ascii_lowercase().as_str() {
        "image" => Some("jpg"),
        "video" => Some("mp4"),
        "audio" => Some("silk"),
        "file" => Some("bin"),
        _ => None,
    }
}

fn infer_mime_from_magic_bytes(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 8 && bytes[..8] == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        return Some("image/png");
    }
    if bytes.len() >= 3 && bytes[..3] == [0xFF, 0xD8, 0xFF] {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.starts_with(b"%PDF-") {
        return Some("application/pdf");
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Some("video/mp4");
    }
    None
}

fn sanitize_filename(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_control() || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            sanitized.push('_');
        } else {
            sanitized.push(ch);
        }
    }
    let mut normalized = sanitized.trim().to_string();
    while normalized.ends_with('.') || normalized.ends_with(' ') {
        normalized.pop();
    }
    if normalized == "." || normalized == ".." {
        String::new()
    } else {
        normalized
    }
}

async fn ensure_unique_path(path: PathBuf) -> Result<PathBuf> {
    if fs::metadata(&path).await.is_err() {
        return Ok(path);
    }
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("file");
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let suffix = Uuid::new_v4().simple().to_string();
    let filename = if ext.is_empty() {
        format!("{stem}_{suffix}")
    } else {
        format!("{stem}_{suffix}.{ext}")
    };
    Ok(path.with_file_name(filename))
}

fn build_channel_inbound_dir(channel: &str, message_id: Option<&str>) -> String {
    let suffix = message_id
        .map(sanitize_path_component)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
    let safe_channel = sanitize_path_component(channel);
    if safe_channel.is_empty() {
        format!("inbox/files/{suffix}")
    } else {
        format!("inbox/{safe_channel}/{suffix}")
    }
}

fn sanitize_path_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn sanitize_filename_replaces_unsafe_characters() {
        let value = sanitize_filename("report 2026/03?.pdf");
        assert_eq!(value, "report 2026_03_.pdf");

        let chinese = sanitize_filename("\u{6D4B}\u{8BD5}\u{6587}\u{6863}.doc");
        assert_eq!(chinese, "\u{6D4B}\u{8BD5}\u{6587}\u{6863}.doc");
    }

    #[test]
    fn sanitize_path_component_replaces_unsafe_characters() {
        let value = sanitize_path_component("weixin/channel#A");
        assert_eq!(value, "weixin_channel_A");
    }

    #[test]
    fn build_channel_inbound_dir_uses_channel_and_message_id() {
        let with_message = build_channel_inbound_dir("wei/xin", Some("msg/01"));
        assert_eq!(with_message, "inbox/wei_xin/msg_01");

        let without_channel = build_channel_inbound_dir("", Some("msg-02"));
        assert_eq!(without_channel, "inbox/files/msg-02");
    }

    #[test]
    fn build_channel_inbound_dir_generates_suffix_without_message_id() {
        let dir = build_channel_inbound_dir("weixin", None);
        assert!(dir.starts_with("inbox/weixin/"));
        let suffix = dir.trim_start_matches("inbox/weixin/");
        assert_eq!(suffix.len(), 32);
        assert!(suffix.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn extension_from_mime_maps_known_types() {
        assert_eq!(extension_from_mime(Some(" image/jpeg ")), Some("jpg"));
        assert_eq!(extension_from_mime(Some("image/*")), Some("jpg"));
        assert_eq!(extension_from_mime(Some("application/pdf")), Some("pdf"));
        assert_eq!(extension_from_mime(Some("application/x-custom")), None);
        assert_eq!(extension_from_mime(None), None);
    }

    #[test]
    fn infer_mime_from_magic_bytes_detects_png() {
        let bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00];
        assert_eq!(infer_mime_from_magic_bytes(&bytes), Some("image/png"));
    }

    #[test]
    fn infer_mime_from_filename_maps_extensions() {
        assert_eq!(
            infer_mime_from_filename("report.PDF").as_deref(),
            Some("application/pdf")
        );
        assert_eq!(
            infer_mime_from_filename("unknown.xyz").as_deref(),
            Some("application/octet-stream")
        );
        assert_eq!(infer_mime_from_filename("no_extension"), None);
    }

    #[test]
    fn has_extension_detects_non_empty_extension() {
        assert!(has_extension("a.txt"));
        assert!(has_extension("archive.tar.gz"));
        assert!(!has_extension("a."));
        assert!(!has_extension("folder/noext"));
    }

    #[tokio::test]
    async fn ensure_unique_path_returns_same_when_missing_and_suffix_when_exists() {
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("report.txt");

        let missing = ensure_unique_path(target.clone())
            .await
            .expect("missing path check");
        assert_eq!(missing, target);

        fs::write(&target, b"content").await.expect("write file");
        let deduplicated = ensure_unique_path(target.clone())
            .await
            .expect("existing path check");
        assert_ne!(deduplicated, target);
        assert_eq!(
            deduplicated.extension().and_then(|v| v.to_str()),
            Some("txt")
        );
        let stem = deduplicated
            .file_stem()
            .and_then(|v| v.to_str())
            .unwrap_or_default();
        assert!(stem.starts_with("report_"));
    }
}
