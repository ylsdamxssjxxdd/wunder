use crate::channels::feishu;
use crate::channels::types::{
    ChannelAttachment, ChannelMessage, ChannelOutboundMessage, FeishuConfig,
};
use crate::channels::workspace_routing::resolve_channel_workspace_id;
use crate::config::Config;
use crate::user_store::UserStore;
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Result};
use axum::http::{header, HeaderMap};
use bytes::Bytes;
use reqwest::{Client, Url};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tokio::fs;
use uuid::Uuid;

const MAX_REMOTE_ATTACHMENT_BYTES: usize = 20 * 1024 * 1024;

pub fn resolve_public_base_url(headers: &HeaderMap, config: &Config) -> String {
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("http");

    let mut host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .unwrap_or_else(|| {
            let host = if config.server.host == "0.0.0.0" {
                "127.0.0.1".to_string()
            } else {
                config.server.host.clone()
            };
            format!("{host}:{}", config.server.port)
        });
    if !host.contains(':') {
        if let Some(port) = headers
            .get("x-forwarded-port")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            host = format!("{host}:{port}");
        }
    }

    format!("{scheme}://{host}")
        .trim_end_matches('/')
        .to_string()
}

pub async fn download_feishu_attachments_to_workspace(
    http: &Client,
    workspace: &WorkspaceManager,
    user_store: &UserStore,
    config: &FeishuConfig,
    user_id: &str,
    agent_id: Option<&str>,
    message: &mut ChannelMessage,
) -> Result<()> {
    if message.attachments.is_empty() {
        return Ok(());
    }
    let workspace_id = resolve_workspace_id(workspace, user_store, user_id, agent_id);
    workspace.ensure_user_root(&workspace_id)?;
    let message_id = message
        .message_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let base_dir = build_inbound_dir(message_id);

    for attachment in &mut message.attachments {
        if !is_feishu_file_attachment(&attachment.kind) {
            continue;
        }
        let file_key = attachment.url.trim();
        if file_key.is_empty() {
            continue;
        }
        let file_type = map_resource_type(&attachment.kind);
        let download =
            match fetch_feishu_resource(http, config, message_id, file_key, file_type).await {
                Ok(value) => value,
                Err(_) => continue,
            };
        let display_name = pick_display_name(attachment, &download);
        let mut safe_name = sanitize_filename(&display_name);
        if !has_extension(&safe_name) {
            if let Some(ext) = extension_from_content_type(download.content_type.as_deref()) {
                safe_name.push('.');
                safe_name.push_str(ext);
            }
        }
        if safe_name.is_empty() {
            safe_name = format!("file_{}", Uuid::new_v4().simple());
        }
        let relative_path = format!("{}/{}", base_dir, safe_name);
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
        if fs::write(&target, &download.bytes).await.is_err() {
            continue;
        }
        let public_path = workspace.display_path(&workspace_id, &target);
        attachment.url = public_path;
        if attachment.name.as_deref().unwrap_or("").trim().is_empty() {
            attachment.name = Some(display_name);
        }
        attachment.mime = download.content_type;
        attachment.size = Some(download.bytes.len() as i64);
    }
    Ok(())
}

pub async fn append_temp_dir_links_for_outbound(
    workspace: &WorkspaceManager,
    user_store: &UserStore,
    config: &Config,
    channel: &str,
    outbound: &mut ChannelOutboundMessage,
) -> Result<bool> {
    let user_id = extract_meta_string(outbound.meta.as_ref(), "user_id");
    let Some(user_id) = user_id.as_deref() else {
        return Ok(false);
    };
    let agent_id = extract_meta_string(outbound.meta.as_ref(), "agent_id");
    let base_url = extract_meta_string(outbound.meta.as_ref(), "public_base_url")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default_public_base_url(config));

    let workspace_id = resolve_workspace_id(workspace, user_store, user_id, agent_id.as_deref());
    let mut candidates: Vec<String> = outbound
        .text
        .as_deref()
        .map(extract_workspace_paths)
        .unwrap_or_default();
    for attachment in &outbound.attachments {
        let candidate = normalize_public_path(&attachment.url);
        if is_workspace_public_path(&candidate) {
            candidates.push(candidate);
        }
    }
    if candidates.is_empty() {
        return Ok(false);
    }

    let mut replacements: Vec<(String, String)> = Vec::new();
    let mut seen = HashSet::new();
    for raw in candidates {
        let normalized = normalize_public_path(&raw);
        if normalized.is_empty() || !seen.insert(normalized.clone()) {
            continue;
        }
        let resolved = match workspace.resolve_path(&workspace_id, &normalized) {
            Ok(path) => path,
            Err(_) => continue,
        };
        let metadata = match fs::metadata(&resolved).await {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        if !metadata.is_file() {
            continue;
        }
        if let Some(link) = copy_to_temp_dir(&resolved, user_id, channel, base_url.as_str()).await?
        {
            replacements.push((normalized, link.url));
        }
    }

    if replacements.is_empty() {
        return Ok(false);
    }

    let mut changed = false;
    if let Some(text) = outbound.text.as_ref() {
        let mut rewritten = text.to_string();
        for (path, url) in &replacements {
            if path.is_empty() || url.is_empty() || !rewritten.contains(path) {
                continue;
            }
            rewritten = rewritten.replace(path, url);
            changed = true;
        }
        if changed {
            outbound.text = Some(rewritten);
        }
    }

    for attachment in &mut outbound.attachments {
        let candidate = normalize_public_path(&attachment.url);
        if candidate.is_empty() {
            continue;
        }
        if let Some((_, url)) = replacements.iter().find(|(path, _)| path == &candidate) {
            if !url.trim().is_empty() && attachment.url != *url {
                attachment.url = url.clone();
                changed = true;
            }
        }
    }

    Ok(changed)
}

pub async fn download_remote_attachments_to_workspace(
    http: &Client,
    workspace: &WorkspaceManager,
    user_store: &UserStore,
    user_id: &str,
    agent_id: Option<&str>,
    channel: &str,
    message: &mut ChannelMessage,
) -> Result<()> {
    if message.attachments.is_empty() {
        return Ok(());
    }
    // Keep channel inbound files under the same scoped workspace that the agent session uses.
    let workspace_id = resolve_workspace_id(workspace, user_store, user_id, agent_id);
    workspace.ensure_user_root(&workspace_id)?;
    let message_id = message
        .message_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let base_dir = build_channel_inbound_dir(channel, message_id);

    for attachment in &mut message.attachments {
        let source_url = attachment.url.trim();
        if !is_http_url(source_url) {
            continue;
        }
        let download = match fetch_remote_bytes(http, source_url).await {
            Ok(value) => value,
            Err(_) => continue,
        };
        let display_name = pick_display_name(attachment, &download);
        let mut safe_name = sanitize_filename(&display_name);
        if !has_extension(&safe_name) {
            if let Some(ext) = extension_from_content_type(download.content_type.as_deref()) {
                safe_name.push('.');
                safe_name.push_str(ext);
            }
        }
        if safe_name.is_empty() {
            safe_name = format!("file_{}", Uuid::new_v4().simple());
        }
        let relative_path = format!("{base_dir}/{safe_name}");
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
        if fs::write(&target, &download.bytes).await.is_err() {
            continue;
        }
        let public_path = workspace.display_path(&workspace_id, &target);
        attachment.url = public_path;
        if attachment.name.as_deref().unwrap_or("").trim().is_empty() {
            attachment.name = Some(display_name);
        }
        attachment.mime = download.content_type;
        attachment.size = Some(download.bytes.len() as i64);
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

fn is_feishu_file_attachment(kind: &str) -> bool {
    matches!(
        kind.trim().to_ascii_lowercase().as_str(),
        "image" | "file" | "audio" | "media" | "video"
    )
}

fn map_resource_type(kind: &str) -> &'static str {
    match kind.trim().to_ascii_lowercase().as_str() {
        "image" => "image",
        "audio" => "audio",
        "media" | "video" => "video",
        _ => "file",
    }
}

fn is_http_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

struct FeishuDownload {
    bytes: Bytes,
    filename: Option<String>,
    content_type: Option<String>,
}

async fn fetch_feishu_resource(
    http: &Client,
    config: &FeishuConfig,
    message_id: Option<&str>,
    file_key: &str,
    file_type: &str,
) -> Result<FeishuDownload> {
    let token = feishu::fetch_tenant_access_token(http, config).await?;
    let base_url = feishu::resolve_openapi_base_url(config);
    let mut last_err: Option<anyhow::Error> = None;

    if let Some(message_id) = message_id {
        let url = format!(
            "{base_url}/open-apis/im/v1/messages/{message_id}/resources/{file_key}?type={file_type}"
        );
        match fetch_feishu_bytes(http, &token, &url).await {
            Ok(value) => return Ok(value),
            Err(err) => last_err = Some(err),
        }
    }

    if file_type == "image" {
        let url = format!("{base_url}/open-apis/im/v1/images/{file_key}");
        match fetch_feishu_bytes(http, &token, &url).await {
            Ok(value) => return Ok(value),
            Err(err) => last_err = Some(err),
        }
    }

    let url = format!("{base_url}/open-apis/im/v1/files/{file_key}");
    match fetch_feishu_bytes(http, &token, &url).await {
        Ok(value) => Ok(value),
        Err(err) => Err(last_err.unwrap_or(err)),
    }
}

async fn fetch_feishu_bytes(http: &Client, token: &str, url: &str) -> Result<FeishuDownload> {
    let response = http.get(url).bearer_auth(token).send().await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!("feishu download failed: {status} {body}"));
    }
    let headers = response.headers().clone();
    let bytes = response.bytes().await?;
    let filename = headers
        .get(header::CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_disposition_filename);
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());
    Ok(FeishuDownload {
        bytes,
        filename,
        content_type,
    })
}

async fn fetch_remote_bytes(http: &Client, url: &str) -> Result<FeishuDownload> {
    let response = http.get(url).send().await?;
    if !response.status().is_success() {
        let status = response.status();
        return Err(anyhow!("remote download failed: {status}"));
    }
    if response.content_length().unwrap_or(0) > MAX_REMOTE_ATTACHMENT_BYTES as u64 {
        return Err(anyhow!(
            "remote attachment exceeds max bytes: {}",
            MAX_REMOTE_ATTACHMENT_BYTES
        ));
    }
    let headers = response.headers().clone();
    let bytes = response.bytes().await?;
    if bytes.len() > MAX_REMOTE_ATTACHMENT_BYTES {
        return Err(anyhow!(
            "remote attachment exceeds max bytes: {}",
            MAX_REMOTE_ATTACHMENT_BYTES
        ));
    }
    let filename = headers
        .get(header::CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_disposition_filename)
        .or_else(|| filename_from_url(url));
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());
    Ok(FeishuDownload {
        bytes,
        filename,
        content_type,
    })
}

fn parse_content_disposition_filename(value: &str) -> Option<String> {
    let mut filename = None;
    for raw in value.split(';') {
        let part = raw.trim();
        if let Some(rest) = part.strip_prefix("filename*=") {
            let cleaned = rest.trim_matches('"');
            if let Some(encoded) = cleaned.split("''").nth(1) {
                filename = Some(percent_decode(encoded));
                break;
            }
            filename = Some(percent_decode(cleaned));
            break;
        }
        if let Some(rest) = part.strip_prefix("filename=") {
            filename = Some(rest.trim_matches('"').to_string());
            break;
        }
    }
    filename
}

fn percent_decode(value: &str) -> String {
    let mut output = String::new();
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[index + 1..index + 3]) {
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    output.push(value as char);
                    index += 3;
                    continue;
                }
            }
        }
        output.push(bytes[index] as char);
        index += 1;
    }
    output
}

fn pick_display_name(attachment: &ChannelAttachment, download: &FeishuDownload) -> String {
    let name = attachment
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .or_else(|| download.filename.clone())
        .unwrap_or_else(|| format!("file_{}", Uuid::new_v4().simple()));
    name
}

fn has_extension(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| !ext.trim().is_empty())
        .unwrap_or(false)
}

fn extension_from_content_type(content_type: Option<&str>) -> Option<&'static str> {
    let content_type = content_type?.trim().to_ascii_lowercase();
    if content_type.starts_with("image/") {
        return match content_type.as_str() {
            "image/png" => Some("png"),
            "image/jpeg" => Some("jpg"),
            "image/gif" => Some("gif"),
            "image/webp" => Some("webp"),
            "image/bmp" => Some("bmp"),
            _ => Some("img"),
        };
    }
    match content_type.as_str() {
        "application/pdf" => Some("pdf"),
        "application/msword" => Some("doc"),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => Some("docx"),
        "application/vnd.ms-excel" => Some("xls"),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => Some("xlsx"),
        "application/vnd.ms-powerpoint" => Some("ppt"),
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => Some("pptx"),
        "text/plain" => Some("txt"),
        "application/json" => Some("json"),
        "application/zip" => Some("zip"),
        "audio/mpeg" => Some("mp3"),
        "audio/wav" => Some("wav"),
        "video/mp4" => Some("mp4"),
        _ => None,
    }
}

fn sanitize_filename(value: &str) -> String {
    let mut output = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    output
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

fn build_inbound_dir(message_id: Option<&str>) -> String {
    build_channel_inbound_dir("feishu", message_id)
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
    let mut output = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    output
}

fn normalize_public_path(raw: &str) -> String {
    let mut text = raw.trim().replace('\\', "/");
    while text.contains("//") {
        text = text.replace("//", "/");
    }
    text.trim_matches(|ch: char| ch == '.' || ch == ',' || ch == ';' || ch == ':')
        .to_string()
}

fn is_workspace_public_path(value: &str) -> bool {
    value.starts_with("/workspaces/")
}

fn extract_workspace_paths(text: &str) -> Vec<String> {
    let mut output = Vec::new();
    let mut index = 0;
    while let Some(found) = text[index..].find("/workspaces/") {
        let start = index + found;
        let mut end = start;
        for (offset, ch) in text[start..].char_indices() {
            if ch.is_whitespace() || matches!(ch, ')' | ']' | '"' | '\'' | '>' | '<') {
                break;
            }
            end = start + offset + ch.len_utf8();
        }
        if end <= start {
            end = text.len();
        }
        let candidate = text[start..end].to_string();
        output.push(candidate);
        index = end;
    }
    output
}

struct TempDownloadLink {
    url: String,
}

async fn copy_to_temp_dir(
    source: &Path,
    user_id: &str,
    channel: &str,
    base_url: &str,
) -> Result<Option<TempDownloadLink>> {
    let filename = source
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("download");
    let safe_user = sanitize_path_component(user_id);
    let safe_channel = sanitize_path_component(channel);
    let temp_root = temp_dir_root()?;
    let unique = Uuid::new_v4().simple().to_string();
    let channel_segment = if safe_channel.is_empty() {
        "files".to_string()
    } else {
        safe_channel
    };
    let dest_relative = format!("channels/{channel_segment}/{safe_user}/{unique}_{filename}");
    let dest_path = temp_root.join(&dest_relative);
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent).await?;
    }
    if fs::copy(source, &dest_path).await.is_err() {
        return Ok(None);
    }
    let encoded = percent_encode(&dest_relative);
    let url = format!(
        "{}/wunder/temp_dir/download?filename={}",
        base_url.trim_end_matches('/'),
        encoded
    );
    Ok(Some(TempDownloadLink { url }))
}

fn temp_dir_root() -> Result<PathBuf> {
    if let Ok(value) = std::env::var("WUNDER_TEMP_DIR_ROOT") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            let candidate = PathBuf::from(trimmed);
            if candidate.is_absolute() {
                return Ok(candidate);
            }
            let root = std::env::current_dir()?;
            return Ok(root.join(candidate));
        }
    }
    let root = std::env::current_dir()?;
    Ok(root.join("config").join("data").join("temp_dir"))
}

fn percent_encode(value: &str) -> String {
    let mut output = String::new();
    for byte in value.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' || ch == '~' {
            output.push(ch);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

fn extract_meta_string(meta: Option<&Value>, key: &str) -> Option<String> {
    meta.and_then(|value| value.get(key))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn default_public_base_url(config: &Config) -> String {
    let host = if config.server.host == "0.0.0.0" {
        "127.0.0.1"
    } else {
        config.server.host.as_str()
    };
    format!("http://{host}:{}", config.server.port)
}

fn filename_from_url(value: &str) -> Option<String> {
    let url = Url::parse(value).ok()?;
    let filename = url.path_segments()?.next_back()?.trim();
    if filename.is_empty() {
        return None;
    }
    Some(filename.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_workspace_paths() {
        let text = "See file ![img](/workspaces/user123/report.png) and /workspaces/user123/a.txt.";
        let paths = extract_workspace_paths(text);
        assert_eq!(
            paths,
            vec![
                "/workspaces/user123/report.png".to_string(),
                "/workspaces/user123/a.txt.".to_string()
            ]
        );
    }

    #[test]
    fn test_normalize_public_path() {
        let raw = "/workspaces//user//file.txt.";
        let normalized = normalize_public_path(raw);
        assert_eq!(normalized, "/workspaces/user/file.txt");
    }

    #[test]
    fn test_percent_decode() {
        let value = "hello%20world%2Etxt";
        assert_eq!(percent_decode(value), "hello world.txt");
    }
}
