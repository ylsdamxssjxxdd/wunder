use crate::channels::feishu;
use crate::channels::types::{
    ChannelAttachment, ChannelMessage, ChannelOutboundMessage, FeishuConfig,
};
use crate::config::Config;
use crate::user_store::UserStore;
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Result};
use axum::http::{header, HeaderMap};
use bytes::Bytes;
use reqwest::Client;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tokio::fs;
use uuid::Uuid;

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
    outbound: &mut ChannelOutboundMessage,
) -> Result<bool> {
    let Some(text) = outbound.text.as_deref() else {
        return Ok(false);
    };
    if text.trim().is_empty() {
        return Ok(false);
    }
    let user_id = extract_meta_string(outbound.meta.as_ref(), "user_id");
    let Some(user_id) = user_id.as_deref() else {
        return Ok(false);
    };
    let agent_id = extract_meta_string(outbound.meta.as_ref(), "agent_id");
    let base_url = extract_meta_string(outbound.meta.as_ref(), "public_base_url")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default_public_base_url(config));

    let workspace_id = resolve_workspace_id(workspace, user_store, user_id, agent_id.as_deref());
    let candidates = extract_workspace_paths(text);
    if candidates.is_empty() {
        return Ok(false);
    }

    let mut links = Vec::new();
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
        if let Some(link) = copy_to_temp_dir(&resolved, user_id, base_url.as_str()).await? {
            links.push(link);
        }
    }

    if links.is_empty() {
        return Ok(false);
    }

    let mut new_text = text.to_string();
    if !new_text.ends_with('\n') {
        new_text.push('\n');
    }
    new_text.push('\n');
    new_text.push_str("附件：\n");
    for link in &links {
        new_text.push_str(&format!("- {}: {}\n", link.name, link.url));
    }
    outbound.text = Some(new_text);
    Ok(true)
}

fn resolve_workspace_id(
    workspace: &WorkspaceManager,
    user_store: &UserStore,
    user_id: &str,
    agent_id: Option<&str>,
) -> String {
    if let Some(container_id) = user_store.resolve_agent_sandbox_container_id(agent_id) {
        return workspace.scoped_user_id_by_container(user_id, container_id);
    }
    workspace.scoped_user_id(user_id, agent_id)
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
    let suffix = message_id
        .map(sanitize_path_component)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
    format!("inbox/feishu/{suffix}")
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
    name: String,
    url: String,
}

async fn copy_to_temp_dir(
    source: &Path,
    user_id: &str,
    base_url: &str,
) -> Result<Option<TempDownloadLink>> {
    let filename = source
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("download");
    let safe_user = sanitize_path_component(user_id);
    let temp_root = temp_dir_root()?;
    let unique = Uuid::new_v4().simple().to_string();
    let dest_relative = format!("feishu/{safe_user}/{unique}_{filename}");
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
    Ok(Some(TempDownloadLink {
        name: filename.to_string(),
        url,
    }))
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
    Ok(root.join("temp_dir"))
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
