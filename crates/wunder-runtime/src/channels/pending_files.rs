use crate::channels::types::ChannelAttachment;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;

const CHANNEL_PENDING_FILES_META_KEY: &str = "pending_channel_files";
const FILE_PLACEHOLDER_TOKENS: [&str; 11] = [
    "[file]",
    "[image]",
    "[photo]",
    "[picture]",
    "[video]",
    "[audio]",
    "[voice]",
    "[document]",
    "[attachment]",
    "[media]",
    "[unsupported]",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PendingChannelFile {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub mime: Option<String>,
    pub size: Option<i64>,
    pub uploaded_at: f64,
}

pub fn build_pending_files_from_attachments(
    attachments: &[ChannelAttachment],
    uploaded_at: f64,
) -> Vec<PendingChannelFile> {
    let mut output = Vec::new();
    let mut seen_paths = HashSet::new();
    for attachment in attachments {
        let path = attachment.url.trim();
        if path.is_empty() {
            continue;
        }
        if !seen_paths.insert(path.to_string()) {
            continue;
        }
        output.push(PendingChannelFile {
            name: pick_display_name(attachment, path),
            path: path.to_string(),
            kind: normalize_kind(attachment.kind.as_str()),
            mime: attachment
                .mime
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            size: attachment.size,
            uploaded_at,
        });
    }
    output
}

pub fn merge_pending_files(
    mut existing: Vec<PendingChannelFile>,
    incoming: Vec<PendingChannelFile>,
) -> Vec<PendingChannelFile> {
    let mut seen_paths: HashSet<String> = existing.iter().map(|item| item.path.clone()).collect();
    for file in incoming {
        if seen_paths.insert(file.path.clone()) {
            existing.push(file);
        }
    }
    existing
}

pub fn read_pending_files_from_metadata(meta: Option<&Value>) -> Vec<PendingChannelFile> {
    let Some(meta) = meta else {
        return Vec::new();
    };
    let Some(items) = meta.get(CHANNEL_PENDING_FILES_META_KEY) else {
        return Vec::new();
    };
    serde_json::from_value(items.clone()).unwrap_or_default()
}

pub fn write_pending_files_to_metadata(
    metadata: Option<Value>,
    files: &[PendingChannelFile],
) -> Option<Value> {
    let mut meta_obj = metadata
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    if files.is_empty() {
        meta_obj.remove(CHANNEL_PENDING_FILES_META_KEY);
    } else {
        meta_obj.insert(CHANNEL_PENDING_FILES_META_KEY.to_string(), json!(files));
    }
    if meta_obj.is_empty() {
        None
    } else {
        Some(Value::Object(meta_obj))
    }
}

pub fn has_meaningful_channel_text(text: Option<&str>) -> bool {
    let Some(raw) = text.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .any(|line| !is_file_placeholder_text(line))
}

pub fn build_channel_question_with_files(
    text: Option<&str>,
    files: &[PendingChannelFile],
) -> String {
    let body = text
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("[empty message]")
        .to_string();
    if files.is_empty() {
        return body;
    }
    let mut lines = Vec::with_capacity(files.len() + 2);
    lines.push("用户已通过渠道上传以下文件（均位于当前工作目录）:".to_string());
    for (index, file) in files.iter().enumerate() {
        let mut line = format!("{}. {} | path={}", index + 1, file.name, file.path);
        if !file.kind.is_empty() {
            line.push_str(&format!(" | kind={}", file.kind));
        }
        if let Some(mime) = file
            .mime
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            line.push_str(&format!(" | mime={mime}"));
        }
        if let Some(size) = file.size {
            line.push_str(&format!(" | size={size}"));
        }
        lines.push(line);
    }
    format!("{body}\n\n{}", lines.join("\n"))
}

pub fn format_pending_upload_preview(files: &[PendingChannelFile]) -> String {
    if files.is_empty() {
        return "[file]".to_string();
    }
    let mut lines = Vec::with_capacity(files.len() + 1);
    for file in files {
        lines.push(format!("File {}: {}", file.name, file.path));
    }
    lines.push("[file]".to_string());
    lines.join("\n")
}

fn pick_display_name(attachment: &ChannelAttachment, path: &str) -> String {
    if let Some(name) = attachment
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return name.to_string();
    }
    Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "file".to_string())
}

fn normalize_kind(raw: &str) -> String {
    let cleaned = raw.trim().to_ascii_lowercase();
    if cleaned.is_empty() {
        "file".to_string()
    } else {
        cleaned
    }
}

fn is_file_placeholder_text(line: &str) -> bool {
    let cleaned = line.trim().to_ascii_lowercase();
    FILE_PLACEHOLDER_TOKENS
        .iter()
        .any(|token| cleaned.eq_ignore_ascii_case(token))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channels::types::ChannelAttachment;

    #[test]
    fn has_meaningful_channel_text_filters_placeholder_lines() {
        assert!(!has_meaningful_channel_text(Some("[file]")));
        assert!(!has_meaningful_channel_text(Some("[image]\n[file]")));
        assert!(has_meaningful_channel_text(Some("请处理这些文件")));
        assert!(has_meaningful_channel_text(Some("[file]\n顺便统计下数据")));
    }

    #[test]
    fn build_pending_files_from_attachments_uses_name_or_path() {
        let files = build_pending_files_from_attachments(
            &[ChannelAttachment {
                kind: "image".to_string(),
                url: "/workspaces/u1/a.png".to_string(),
                mime: Some("image/png".to_string()),
                size: Some(12),
                name: None,
            }],
            1.0,
        );
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "a.png");
        assert_eq!(files[0].path, "/workspaces/u1/a.png");
        assert_eq!(files[0].kind, "image");
    }

    #[test]
    fn metadata_roundtrip_for_pending_files() {
        let files = vec![PendingChannelFile {
            name: "a.txt".to_string(),
            path: "/workspaces/u1/a.txt".to_string(),
            kind: "file".to_string(),
            mime: Some("text/plain".to_string()),
            size: Some(5),
            uploaded_at: 1.0,
        }];
        let meta =
            write_pending_files_to_metadata(Some(json!({"bridge_center_id":"bc_1"})), &files)
                .expect("meta");
        assert_eq!(
            meta.get("bridge_center_id").and_then(Value::as_str),
            Some("bc_1")
        );
        let loaded = read_pending_files_from_metadata(Some(&meta));
        assert_eq!(loaded, files);
        let cleaned = write_pending_files_to_metadata(Some(meta), &[]);
        assert!(cleaned.is_some());
        assert_eq!(
            cleaned
                .as_ref()
                .and_then(|value| value.get("pending_channel_files")),
            None
        );
    }
}
