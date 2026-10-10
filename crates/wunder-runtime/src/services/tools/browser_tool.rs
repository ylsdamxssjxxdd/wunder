//! Browser tool surface (provider driven, dsh parity).
//!
//! wunder no longer exposes a single opaque `浏览器` tool. Instead the active
//! [`BrowserProvider`] contributes its native tool surface (`browser_navigate`,
//! `browser_click`, ...) and every call returns MCP-style content blocks.
//!
//! The runtime keeps its own safety defaults (URL allow-list, private-network and
//! `file://` blocking, timeout / download limits) because calls still go through
//! [`crate::services::browser::runtime::BrowserControlService`].

use super::ToolContext;
use crate::config::Config;
use crate::schemas::ToolSpec;
use crate::services::browser::browser_tools_enabled as browser_tools_enabled_impl;
use crate::services::browser::provider::{active_provider, browser_registry};
use anyhow::{anyhow, Result};
use base64::Engine;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::OnceLock;

const BROWSER_SCREENSHOT_DIR: &str = "browser/screenshots";

/// Whether browser tools are enabled for the given config.
pub fn browser_tools_enabled(config: &Config) -> bool {
    browser_tools_enabled_impl(config)
}

/// Native tool specs contributed by the active provider.
pub fn browser_tool_specs() -> Vec<ToolSpec> {
    browser_registry()
        .provider()
        .map(|provider| provider.tool_specs())
        .unwrap_or_default()
}

/// Native tool names contributed by the active provider.
pub fn browser_tool_names() -> Vec<String> {
    browser_tool_specs()
        .into_iter()
        .map(|spec| spec.name)
        .collect()
}

fn browser_tool_name_set() -> &'static HashSet<String> {
    static NAMES: OnceLock<HashSet<String>> = OnceLock::new();
    NAMES.get_or_init(|| browser_tool_names().into_iter().collect())
}

/// Whether `name` is one of the provider's native browser tools.
pub fn is_browser_tool_name(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty() && browser_tool_name_set().contains(trimmed)
}

/// Dispatch one browser tool call through the active provider.
pub async fn tool_browser(
    context: &ToolContext<'_>,
    tool_name: &str,
    args: &Value,
) -> Result<Value> {
    ensure_browser_available(context.config)?;
    let provider = active_provider(context.config)
        .ok_or_else(|| anyhow!("No browser provider is registered"))?;
    if !provider.owns_tool(tool_name) {
        return Err(anyhow!("Unknown browser tool '{tool_name}'"));
    }
    let outcome = provider.call(context, tool_name, args).await?;
    let mut value = outcome.to_value();
    if let Some(map) = value.as_object_mut() {
        map.insert("provider".to_string(), json!(provider.id()));
    }
    if tool_name.trim() == crate::services::browser::providers::playwright::TOOL_STATUS {
        if let Some(data) = value.get_mut("data") {
            sanitize_browser_status_for_model(data);
        }
    }
    persist_image_blocks(context, args, &mut value)?;
    Ok(value)
}

fn ensure_browser_available(config: &Config) -> Result<()> {
    if browser_tools_enabled(config) {
        return Ok(());
    }
    Err(anyhow!(
        "Browser tools are disabled. Enable tools.browser.enabled together with browser.enabled (or legacy desktop mode)."
    ))
}

fn sanitize_browser_status_for_model(result: &mut Value) {
    let Value::Object(map) = result else {
        return;
    };
    map.remove("control");
}

/// Persist every inline image block into the workspace and rewrite it as an
/// attachment reference (dsh parity: images are delivered as attachments).
fn persist_image_blocks(context: &ToolContext<'_>, args: &Value, value: &mut Value) -> Result<()> {
    let Some(blocks) = value.get_mut("content").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    let mut index = 0usize;
    for block in blocks.iter_mut() {
        let Value::Object(map) = block else {
            continue;
        };
        if map.get("type").and_then(Value::as_str) != Some("image") {
            continue;
        }
        let Some(encoded) = map
            .get("data")
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            continue;
        };
        let mime_type = map
            .get("mime_type")
            .and_then(Value::as_str)
            .unwrap_or("image/png")
            .to_string();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded.as_bytes())
            .map_err(|err| anyhow!("Browser image base64 decode failed: {err}"))?;
        let extension = image_extension(&mime_type);
        let requested = if index == 0 {
            args.get("path")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        } else {
            None
        };
        let relative = requested.unwrap_or_else(|| {
            format!(
                "{BROWSER_SCREENSHOT_DIR}/browser_shot_{}.{extension}",
                uuid::Uuid::new_v4().simple()
            )
        });
        let relative = ensure_extension(relative, extension);
        let target = context
            .workspace
            .resolve_path(context.workspace_id, &relative)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| anyhow!("Create browser screenshot directory failed: {err}"))?;
        }
        std::fs::write(&target, &bytes)
            .map_err(|err| anyhow!("Write browser screenshot to workspace failed: {err}"))?;
        context.workspace.mark_tree_dirty(context.workspace_id);

        let normalized = relative.replace('\\', "/");
        map.remove("data");
        map.remove("mime_type");
        map.insert(
            "attachment".to_string(),
            json!({
                "filename": target
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("browser_screenshot.png"),
                "path": normalized,
                "workspace_relative_path": normalized,
                "url": context.workspace.display_path(context.workspace_id, &target),
                "mime_type": mime_type,
                "bytes": bytes.len(),
                "saved_to": "workspace",
            }),
        );
        index += 1;
    }
    Ok(())
}

fn image_extension(mime_type: &str) -> &'static str {
    match mime_type.trim().to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "png",
    }
}

fn ensure_extension(path: String, extension: &str) -> String {
    let normalized = path.replace('\\', "/");
    let existing = std::path::Path::new(&normalized)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if existing.is_empty() {
        format!("{normalized}.{extension}")
    } else {
        normalized
    }
}

fn ensure_png_extension(path: String) -> String {
    ensure_extension(path, "png")
}

#[cfg(test)]
mod tests {
    use super::{
        browser_tool_names, ensure_png_extension, is_browser_tool_name,
        sanitize_browser_status_for_model,
    };
    use serde_json::json;

    #[test]
    fn sanitize_browser_status_removes_control_endpoint() {
        let mut status = json!({
            "ok": true,
            "control": {
                "host": "127.0.0.1",
                "port": 18791,
                "public_base_url": null,
                "auth_token_configured": false
            },
            "sessions": []
        });
        sanitize_browser_status_for_model(&mut status);
        assert!(status.get("control").is_none());
        assert_eq!(status["ok"].as_bool(), Some(true));
    }

    #[test]
    fn ensure_png_extension_adds_default_only_when_missing() {
        assert_eq!(
            ensure_png_extension("browser/screenshots/capture".to_string()),
            "browser/screenshots/capture.png"
        );
        assert_eq!(
            ensure_png_extension("browser/screenshots/capture.png".to_string()),
            "browser/screenshots/capture.png"
        );
        assert_eq!(
            ensure_png_extension("browser\\screenshots\\capture.jpg".to_string()),
            "browser/screenshots/capture.jpg"
        );
    }

    #[test]
    fn native_surface_replaces_the_legacy_single_tool() {
        let names = browser_tool_names();
        assert!(names.iter().any(|name| name == "browser_navigate"));
        assert!(names.iter().any(|name| name == "browser_take_screenshot"));
        assert!(is_browser_tool_name("browser_navigate"));
        assert!(is_browser_tool_name("browser_status"));
        // The legacy opaque surface is gone.
        assert!(!is_browser_tool_name("浏览器"));
        assert!(!is_browser_tool_name("browser"));
        assert!(!is_browser_tool_name("浏览器导航"));
    }
}