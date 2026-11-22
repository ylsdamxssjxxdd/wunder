//! Workspace preview resolution shared by browser providers.
//!
//! The Playwright bridge and the Android WebView host both answer the internal
//! `wunder.preview` origin from the workspace directory instead of a real
//! HTTP server. Resolving `path` here keeps both providers on the same rules:
//! the value is workspace-relative, never absolute, and never contains `..`.

use anyhow::{anyhow, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use url::Url;

pub(crate) const PREVIEW_HOST: &str = "wunder.preview";

/// Reject absolute paths and `..` so the preview origin never becomes a
/// general file reader.
pub(crate) fn workspace_relative_path(raw: &str) -> Result<String> {
    let normalized = raw.trim().replace('\\', "/");
    if normalized.is_empty() {
        return Err(anyhow!("browser action navigate requires a non-empty path"));
    }
    if normalized.starts_with('/') || normalized.split('/').any(|part| part == "..") {
        return Err(anyhow!(
            "browser navigate path must be workspace-relative and must not contain '..'"
        ));
    }
    Ok(normalized)
}

/// Resolve one upload path inside the workspace. The tool only accepts
/// workspace-relative paths so a page cannot ask the browser to read arbitrary
/// files from the device or the server.
pub(crate) fn resolve_workspace_file(root: &Path, raw: &str) -> Result<PathBuf> {
    let relative = workspace_relative_path(raw)?;
    let root = root
        .canonicalize()
        .map_err(|error| anyhow!("workspace root is unavailable: {error}"))?;
    let target = root.join(relative);
    let target = target
        .canonicalize()
        .map_err(|error| anyhow!("upload file does not exist: {error}"))?;
    if !target.starts_with(&root) || !target.is_file() {
        return Err(anyhow!(
            "upload file must be a regular file inside the workspace"
        ));
    }
    Ok(target)
}

/// Resolve the `files` array from a `file_upload` action.
pub(crate) fn resolve_workspace_upload_files(root: &Path, args: &Value) -> Result<Vec<PathBuf>> {
    let files = args
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("browser action file_upload requires files"))?;
    if files.is_empty() {
        return Err(anyhow!("browser action file_upload requires files"));
    }
    if files.len() > 16 {
        return Err(anyhow!("file_upload accepts at most 16 files"));
    }
    files
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| anyhow!("file_upload files must be strings"))
                .and_then(|raw| resolve_workspace_file(root, raw))
        })
        .collect()
}

/// `url` 是否已经写明协议（`https://`、`about:` 等）。`localhost:3000` 不算：
/// 它多半是漏了协议，交给路径兜底并给出更明确的提示。
pub(crate) fn has_url_scheme(value: &str) -> bool {
    if let Some((scheme, _)) = value.split_once("://") {
        return !scheme.trim().is_empty();
    }
    match value.split_once(':') {
        Some((scheme, _)) => matches!(
            scheme.trim().to_ascii_lowercase().as_str(),
            "about" | "data" | "blob" | "file" | "mailto" | "javascript" | "chrome" | "view-source"
        ),
        None => false,
    }
}

/// Resolve a `navigate` call to a workspace-relative preview path.
///
/// Accepts the documented `path` argument, a relative path mistakenly passed
/// as `url` (opened when the file exists), and an explicit
/// `http(s)://wunder.preview/<path>` URL.
pub(crate) fn preview_path_for_navigate(root: &Path, args: &Value) -> Result<Option<String>> {
    if let Some(relative) = arg_string(args, "path") {
        return workspace_relative_path(&relative).map(Some);
    }
    let Some(url) = arg_string(args, "url") else {
        return Ok(None);
    };
    if let Some(relative) = preview_path_from_preview_url(&url)? {
        return Ok(Some(relative));
    }
    if !has_url_scheme(&url) {
        return preview_path_from_schemeless_url(root, &url).map(Some);
    }
    Ok(None)
}

fn arg_string(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn preview_path_from_preview_url(raw: &str) -> Result<Option<String>> {
    let Ok(parsed) = Url::parse(raw) else {
        return Ok(None);
    };
    if parsed.host_str() != Some(PREVIEW_HOST) || !matches!(parsed.scheme(), "http" | "https") {
        return Ok(None);
    }
    let relative = percent_decode(parsed.path().trim_start_matches('/'));
    if relative.trim().is_empty() {
        return Err(anyhow!(
            "browser navigate preview URL needs a workspace-relative file path"
        ));
    }
    workspace_relative_path(&relative).map(Some)
}

/// 把误填进 `url` 的工作区相对路径改判成预览路径；文件不存在时直接把
/// url/path 的分工告诉调用方，而不是让驱动抛出 “relative URL without a base”。
fn preview_path_from_schemeless_url(root: &Path, url: &str) -> Result<String> {
    if let Ok(relative) = workspace_relative_path(url) {
        if root.join(&relative).is_file() {
            return Ok(relative);
        }
    }
    Err(anyhow!(
        "browser navigate needs an absolute URL such as https://example.com; to open a workspace page pass path='<workspace-relative file>' instead of url (got: {url})"
    ))
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                out.push(high * 16 + low);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preview_paths_stay_inside_the_workspace() {
        assert_eq!(
            workspace_relative_path("pages/index.html").expect("relative path"),
            "pages/index.html"
        );
        assert_eq!(
            workspace_relative_path("pages\\index.html").expect("backslashes normalized"),
            "pages/index.html"
        );
        for rejected in ["/etc/passwd", "../escape.html", "a/../../b.html", "  "] {
            assert!(
                workspace_relative_path(rejected).is_err(),
                "{rejected} must not become a preview URL"
            );
        }
    }

    #[test]
    fn preview_urls_become_workspace_paths() {
        let root = tempfile::tempdir().expect("tempdir");
        let args = json!({"url": "https://wunder.preview/pages/a%20b.html?x=1"});
        assert_eq!(
            preview_path_for_navigate(root.path(), &args)
                .expect("preview url")
                .as_deref(),
            Some("pages/a b.html")
        );
        // URL parsing normalizes plain dot segments away, but an encoded
        // separator can still smuggle `..` into the decoded path.
        let escaped = json!({"url": "https://wunder.preview/..%2Fsecret.txt"});
        assert!(preview_path_for_navigate(root.path(), &escaped).is_err());
    }

    #[test]
    fn schemeless_existing_file_is_opened_as_a_preview() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(root.path().join("pages")).expect("pages dir");
        std::fs::write(root.path().join("pages/index.html"), "ok").expect("page");
        let args = json!({"url": "pages/index.html"});
        assert_eq!(
            preview_path_for_navigate(root.path(), &args)
                .expect("schemeless path")
                .as_deref(),
            Some("pages/index.html")
        );
    }

    #[test]
    fn missing_schemeless_url_reports_the_path_split() {
        let root = tempfile::tempdir().expect("tempdir");
        let args = json!({"url": "pages/missing.html"});
        let error = preview_path_for_navigate(root.path(), &args)
            .expect_err("missing file must not become a URL");
        assert!(error.to_string().contains("path="), "{error}");
    }
}
