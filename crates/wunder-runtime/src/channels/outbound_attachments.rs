use crate::channels::types::ChannelAttachment;
use regex::Regex;
use std::collections::HashSet;
use std::path::Path;
use std::sync::OnceLock;
use url::{form_urlencoded, Url};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboundLinkExtractionMode {
    AnyHttp,
    WorkspaceResource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentMediaType {
    Image,
    Video,
    Audio,
}

pub fn merge_attachments_with_text_links(
    outbound_attachments: &[ChannelAttachment],
    text: Option<&str>,
    mode: OutboundLinkExtractionMode,
) -> Vec<ChannelAttachment> {
    let mut merged = Vec::new();
    let mut seen_sources: HashSet<String> = HashSet::new();

    for attachment in outbound_attachments {
        add_existing_attachment(&mut merged, &mut seen_sources, attachment.clone());
    }

    let Some(text) = text.map(str::trim).filter(|value| !value.is_empty()) else {
        return merged;
    };

    for (source, kind_hint) in extract_markdown_attachment_sources(text) {
        add_extracted_attachment(
            &mut merged,
            &mut seen_sources,
            source.as_str(),
            kind_hint,
            mode,
        );
    }

    match mode {
        OutboundLinkExtractionMode::AnyHttp => {
            for source in extract_http_urls(text) {
                add_extracted_attachment(
                    &mut merged,
                    &mut seen_sources,
                    source.as_str(),
                    None,
                    mode,
                );
            }
        }
        OutboundLinkExtractionMode::WorkspaceResource => {
            for source in extract_workspace_resource_sources(text) {
                add_extracted_attachment(
                    &mut merged,
                    &mut seen_sources,
                    source.as_str(),
                    None,
                    mode,
                );
            }
        }
    }

    merged
}

pub fn extract_http_urls(text: &str) -> Vec<String> {
    static HTTP_URL_RE: OnceLock<Regex> = OnceLock::new();
    let regex = HTTP_URL_RE
        .get_or_init(|| Regex::new(r#"https?://[^\s<>"']+"#).expect("valid http url regex"));
    let mut output = Vec::new();
    let mut seen_urls: HashSet<String> = HashSet::new();

    for matched in regex.find_iter(text) {
        let Some(url) = sanitize_extracted_url(matched.as_str()) else {
            continue;
        };
        if seen_urls.insert(url.clone()) {
            output.push(url);
        }
    }

    output
}

pub fn infer_attachment_kind_from_source(source: &str) -> &'static str {
    match infer_attachment_media_type_from_source(source) {
        Some(AttachmentMediaType::Image) => "image",
        Some(AttachmentMediaType::Video) => "video",
        Some(AttachmentMediaType::Audio) => "audio",
        None => "file",
    }
}

pub fn infer_attachment_media_type_from_source(source: &str) -> Option<AttachmentMediaType> {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return None;
    }

    if trimmed.starts_with("data:image/") {
        return Some(AttachmentMediaType::Image);
    }
    if trimmed.starts_with("data:video/") {
        return Some(AttachmentMediaType::Video);
    }
    if trimmed.starts_with("data:audio/") {
        return Some(AttachmentMediaType::Audio);
    }

    let extension = extension_from_source(trimmed)?;
    let lowered = extension.to_ascii_lowercase();
    if matches!(
        lowered.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg"
    ) {
        return Some(AttachmentMediaType::Image);
    }
    if matches!(lowered.as_str(), "mp4" | "mov" | "mkv" | "avi" | "webm") {
        return Some(AttachmentMediaType::Video);
    }
    if matches!(
        lowered.as_str(),
        "silk" | "mp3" | "wav" | "ogg" | "opus" | "m4a" | "aac" | "amr"
    ) {
        return Some(AttachmentMediaType::Audio);
    }

    None
}

fn add_existing_attachment(
    attachments: &mut Vec<ChannelAttachment>,
    seen_sources: &mut HashSet<String>,
    mut attachment: ChannelAttachment,
) {
    let Some(normalized) = normalize_source_text(attachment.url.as_str()) else {
        return;
    };
    if !seen_sources.insert(normalized.clone()) {
        return;
    }
    attachment.url = normalized.clone();
    if attachment.kind.trim().is_empty() {
        attachment.kind = infer_attachment_kind_from_source(&normalized).to_string();
    }
    attachments.push(attachment);
}

fn add_extracted_attachment(
    attachments: &mut Vec<ChannelAttachment>,
    seen_sources: &mut HashSet<String>,
    source: &str,
    kind_hint: Option<&str>,
    mode: OutboundLinkExtractionMode,
) {
    let Some(normalized) = normalize_source_text(source) else {
        return;
    };
    if !matches_extraction_mode(&normalized, mode) {
        return;
    }
    if !seen_sources.insert(normalized.clone()) {
        return;
    }

    let kind = kind_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| infer_attachment_kind_from_source(&normalized).to_string());
    attachments.push(ChannelAttachment {
        kind,
        url: normalized,
        mime: None,
        size: None,
        name: None,
    });
}

fn extract_markdown_attachment_sources(text: &str) -> Vec<(String, Option<&'static str>)> {
    static IMAGE_LINK_RE: OnceLock<Regex> = OnceLock::new();
    static MARKDOWN_LINK_RE: OnceLock<Regex> = OnceLock::new();

    let image_link_re = IMAGE_LINK_RE.get_or_init(|| {
        Regex::new(r#"!\[[^\]]*]\(([^)\s]+)(?:\s+"[^"]*")?\)"#).expect("valid markdown image regex")
    });
    let markdown_link_re = MARKDOWN_LINK_RE.get_or_init(|| {
        Regex::new(r#"\[[^\]]+]\(([^)\s]+)(?:\s+"[^"]*")?\)"#).expect("valid markdown link regex")
    });

    let mut output = Vec::new();
    for captures in image_link_re.captures_iter(text) {
        if let Some(source) = captures.get(1).map(|value| value.as_str()) {
            output.push((source.to_string(), Some("image")));
        }
    }
    for captures in markdown_link_re.captures_iter(text) {
        let Some(full_match) = captures.get(0) else {
            continue;
        };
        if full_match.start() > 0 && text.as_bytes()[full_match.start() - 1] == b'!' {
            continue;
        }
        if let Some(source) = captures.get(1).map(|value| value.as_str()) {
            output.push((source.to_string(), None));
        }
    }
    output
}

fn extract_workspace_resource_sources(text: &str) -> Vec<String> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();

    for source in extract_http_urls(text) {
        if !is_workspace_resource_source(&source) {
            continue;
        }
        if seen.insert(source.clone()) {
            output.push(source);
        }
    }
    for source in extract_bare_prefixed_paths(text, "/workspaces/") {
        if seen.insert(source.clone()) {
            output.push(source);
        }
    }
    for source in extract_bare_prefixed_paths(text, "/wunder/temp_dir/download?") {
        if seen.insert(source.clone()) {
            output.push(source);
        }
    }

    output
}

fn extract_bare_prefixed_paths(text: &str, prefix: &str) -> Vec<String> {
    let mut output = Vec::new();
    let mut index = 0;
    while let Some(found) = text[index..].find(prefix) {
        let start = index + found;
        if appears_inside_http_token(text, start) {
            index = start + prefix.len();
            continue;
        }
        let end = find_token_end(text, start);
        if end <= start {
            index = start + prefix.len();
            continue;
        }
        if let Some(normalized) = normalize_source_text(&text[start..end]) {
            output.push(normalized);
        }
        index = end;
    }
    output
}

fn appears_inside_http_token(text: &str, start: usize) -> bool {
    let token_start = find_token_start(text, start);
    let prefix = &text[token_start..start];
    prefix.starts_with("http://") || prefix.starts_with("https://")
}

fn find_token_start(text: &str, start: usize) -> usize {
    let mut cursor = start;
    while cursor > 0 {
        let prev = text[..cursor].chars().next_back().unwrap_or(' ');
        if prev.is_whitespace()
            || matches!(
                prev,
                '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '"' | '\'' | ','
            )
        {
            break;
        }
        cursor -= prev.len_utf8();
    }
    cursor
}

fn find_token_end(text: &str, start: usize) -> usize {
    let mut end = start;
    for (offset, ch) in text[start..].char_indices() {
        if ch.is_whitespace() || matches!(ch, ')' | ']' | '"' | '\'' | '>' | '<') {
            break;
        }
        end = start + offset + ch.len_utf8();
    }
    end
}

fn normalize_source_text(source: &str) -> Option<String> {
    let mut sanitized = source
        .trim()
        .trim_matches(|ch| matches!(ch, '"' | '\'' | '<' | '>'));
    if sanitized.is_empty() {
        return None;
    }
    while let Some(ch) = sanitized.chars().last() {
        if !matches!(
            ch,
            ')' | ']' | '}' | ',' | '.' | '!' | '?' | ';' | ':' | '"' | '\''
        ) {
            break;
        }
        let next_len = sanitized.len().saturating_sub(ch.len_utf8());
        sanitized = &sanitized[..next_len];
    }
    let normalized = sanitized.trim();
    if normalized.is_empty() {
        return None;
    }
    Some(normalized.to_string())
}

fn sanitize_extracted_url(value: &str) -> Option<String> {
    let sanitized = normalize_source_text(value)?;
    if is_http_url(&sanitized) {
        Some(sanitized)
    } else {
        None
    }
}

fn matches_extraction_mode(source: &str, mode: OutboundLinkExtractionMode) -> bool {
    match mode {
        OutboundLinkExtractionMode::AnyHttp => is_http_url(source),
        OutboundLinkExtractionMode::WorkspaceResource => is_workspace_resource_source(source),
    }
}

fn is_http_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

fn is_workspace_resource_source(source: &str) -> bool {
    if source.starts_with("/workspaces/") || source.starts_with("/wunder/temp_dir/download?") {
        return true;
    }
    if !is_http_url(source) {
        return false;
    }
    let Ok(url) = Url::parse(source) else {
        return false;
    };
    let path = url.path().to_ascii_lowercase();
    path.contains("/workspaces/")
        || path.ends_with("/wunder/temp_dir/download")
        || path.ends_with("/temp_dir/download")
}

fn extension_from_source(source: &str) -> Option<String> {
    let trimmed = source.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        let url = Url::parse(trimmed).ok()?;
        if let Some(ext) = extension_from_path_text(url.path()) {
            return Some(ext);
        }
        for (key, value) in url.query_pairs() {
            if !key.eq_ignore_ascii_case("filename") {
                continue;
            }
            if let Some(ext) = extension_from_path_text(value.as_ref()) {
                return Some(ext);
            }
        }
        return None;
    }
    if let Some((path, query)) = trimmed.split_once('?') {
        if let Some(ext) = extension_from_path_text(path) {
            return Some(ext);
        }
        for (key, value) in form_urlencoded::parse(query.as_bytes()) {
            if !key.eq_ignore_ascii_case("filename") {
                continue;
            }
            if let Some(ext) = extension_from_path_text(value.as_ref()) {
                return Some(ext);
            }
        }
        return None;
    }
    extension_from_path_text(trimmed)
}

fn extension_from_path_text(value: &str) -> Option<String> {
    Path::new(value)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::trim)
        .filter(|ext| !ext.is_empty())
        .map(str::to_ascii_lowercase)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_any_http_extracts_markdown_and_plain_urls_with_dedup() {
        let text = "![img](https://example.com/a.png) [doc](https://example.com/a.pdf) https://example.com/a.png";
        let merged =
            merge_attachments_with_text_links(&[], Some(text), OutboundLinkExtractionMode::AnyHttp);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].url, "https://example.com/a.png");
        assert_eq!(merged[0].kind, "image");
        assert_eq!(merged[1].url, "https://example.com/a.pdf");
        assert_eq!(merged[1].kind, "file");
    }

    #[test]
    fn merge_workspace_only_extracts_relative_workspace_paths() {
        let text = "请下载 /workspaces/user__c__0/reports/result.pdf 和 ![img](/workspaces/user__c__0/reports/chart.jpg)";
        let merged = merge_attachments_with_text_links(
            &[],
            Some(text),
            OutboundLinkExtractionMode::WorkspaceResource,
        );
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].url, "/workspaces/user__c__0/reports/chart.jpg");
        assert_eq!(merged[0].kind, "image");
        assert_eq!(merged[1].url, "/workspaces/user__c__0/reports/result.pdf");
        assert_eq!(merged[1].kind, "file");
    }

    #[test]
    fn infer_media_type_supports_temp_download_filename_query() {
        let url =
            "https://example.com/wunder/temp_dir/download?filename=channels%2Fweixin%2Fu1%2Fabc_video.mp4";
        assert_eq!(
            infer_attachment_media_type_from_source(url),
            Some(AttachmentMediaType::Video)
        );
        assert_eq!(infer_attachment_kind_from_source(url), "video");
    }
}
