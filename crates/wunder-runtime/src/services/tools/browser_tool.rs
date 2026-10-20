//! Browser tool surface (provider driven, dsh parity).
//!
//! wunder exposes one model-facing browser action tool. The provider retains
//! internal bridge operations, but old browser_* model names are unknown.
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

/// 用户可见清单里代表浏览器能力的聚合名。模型侧只有 `browser` 工具；
/// 聚合名用于授权展示，不是额外可调用工具。
pub const BROWSER_GROUP_NAME: &str = "浏览器";
pub const BROWSER_GROUP_NAME_ALIAS: &str = "browser";

/// Whether `name` is the aggregate entry standing for the whole browser family.
pub fn is_browser_group_name(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty()
        && (trimmed == BROWSER_GROUP_NAME || trimmed.eq_ignore_ascii_case(BROWSER_GROUP_NAME_ALIAS))
}

/// The aggregate entry shown to users in place of the provider's tool surface.
pub fn browser_tool_summary_spec(language: &str) -> ToolSpec {
    let english = language.starts_with("en");
    const DESCRIPTION_KEY: &str = "tool.spec.browser_group.description";
    let mut description = crate::i18n::t(DESCRIPTION_KEY);
    if description.is_empty() || description == DESCRIPTION_KEY {
        // 文案缺失时保底，不能让聚合条目显示成键名。
        description = if english {
            "Browser automation as one action tool, bounded by the session, allowed domains and timeout limits.".to_string()
        } else {
            "浏览器自动化能力的整组条目：代表 browser action 工具（打开页面、读取内容、点击输入、等待、截图），受会话、允许域名与超时限制。".to_string()
        };
    }
    ToolSpec {
        name: if english {
            BROWSER_GROUP_NAME_ALIAS.to_string()
        } else {
            BROWSER_GROUP_NAME.to_string()
        },
        title: None,
        description,
        // 聚合条目不可调用，只用于展示与选择，所以不带参数面。
        input_schema: json!({}),
    }
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
        let Some(encoded) = map.get("data").and_then(Value::as_str).map(str::to_string) else {
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
        browser_tool_names, ensure_png_extension, is_browser_group_name, is_browser_tool_name,
        BROWSER_GROUP_NAME,
    };
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
    fn compact_surface_exposes_only_browser() {
        let names = browser_tool_names();
        assert_eq!(names, vec!["browser".to_string()]);
        assert!(is_browser_tool_name("browser"));
        for old_name in ["browser_navigate", "browser_observe", "browser_status"] {
            assert!(!is_browser_tool_name(old_name));
        }
        assert!(!is_browser_tool_name("浏览器"));
    }

    #[test]
    fn browser_group_name_aggregates_the_family_without_being_callable() {
        assert!(is_browser_group_name(BROWSER_GROUP_NAME));
        assert!(is_browser_group_name(" browser "));
        assert!(is_browser_group_name("Browser"));
        assert!(!is_browser_group_name("browser_navigate"));
        assert!(!is_browser_group_name(""));
        assert!(is_browser_tool_name("browser"));
        assert!(!is_browser_tool_name(BROWSER_GROUP_NAME));
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    fn screenshot_content_is_persisted_as_a_resolvable_attachment() {
        use crate::services::tools::ToolContext;
        use crate::storage::{SqliteStorage, StorageBackend};
        use std::{collections::HashMap, sync::Arc};
        let dir = tempfile::tempdir().expect("tempdir");
        let storage: Arc<dyn StorageBackend> = Arc::new(SqliteStorage::new(
            dir.path().join("sample.db").to_string_lossy().to_string(),
        ));
        let workspace = Arc::new(crate::workspace::WorkspaceManager::new(
            dir.path().to_string_lossy().as_ref(),
            storage.clone(),
            0,
            &HashMap::new(),
        ));
        let config = crate::config::Config::default();
        let skills = crate::skills::SkillRegistry::default();
        let http = reqwest::Client::new();
        let context = ToolContext {
            user_id: "sample-user",
            session_id: "sample-thread",
            workspace_id: "sample-user",
            agent_id: None,
            user_round: None,
            model_round: None,
            is_admin: false,
            storage,
            orchestrator: None,
            monitor: None,
            lsp_manager: crate::lsp::LspManager::new_disabled(workspace.clone()),
            workspace,
            config: &config,
            skills: &skills,
            gateway: None,
            user_world: None,
            cron_wake_signal: None,
            user_tool_manager: None,
            user_tool_bindings: None,
            user_tool_store: None,
            request_config_overrides: None,
            allow_roots: None,
            read_roots: None,
            command_sessions: None,
            event_emitter: None,
            http: &http,
        };
        let mut result = json!({"ok":true,"data":{"format":"png"},"content":[
            {"type":"image","mime_type":"image/png","data":"c2FtcGxl"}
        ]});
        super::persist_image_blocks(
            &context,
            &json!({"path":"browser/screenshots/sample.png"}),
            &mut result,
        )
        .unwrap();
        let block = &result["content"][0];
        assert!(block.get("data").is_none());
        assert_eq!(
            block["attachment"]["path"],
            "browser/screenshots/sample.png"
        );
        assert_eq!(block["attachment"]["bytes"], 6);
        let path = context
            .workspace
            .resolve_path(context.workspace_id, "browser/screenshots/sample.png")
            .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"sample");
    }
}
