//! Android system-WebView browser provider.
//!
//! The phone already ships a Chromium-based WebView that is updated by the
//! system. This provider keeps the same model-facing [`browser`](TOOL_BROWSER)
//! surface as Playwright, but dispatches every action to a host installed by
//! the Android shell (`WebViewBridge.java`). No browser binary is packaged and
//! no proot runtime is involved.

use super::action_contract::validate_browser_action;
use super::preview::{preview_path_for_navigate, resolve_workspace_upload_files};
use crate::schemas::ToolSpec;
use crate::services::browser::provider::{
    BrowserProvider, ContentBlock, ProviderCallFuture, ProviderOutcome,
};
use crate::services::browser::providers::playwright::playwright_tool_specs;
use crate::services::tools::ToolContext;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::sync::{Arc, RwLock};

pub const PROVIDER_ID: &str = "android-webview";
pub const TOOL_BROWSER: &str = "browser";

/// Device-side WebView executor installed by the Android shell. Calls arrive
/// on a blocking worker; the implementation may wait for page loads and JS.
pub trait WebViewHost: Send + Sync {
    /// Execute one browser action. The reply follows the Java bridge contract:
    /// `{ "ok": bool, "text": string?, "image_base64": string?, "error": ... }`.
    fn execute(&self, action: &str, args: &Value) -> Result<Value>;
}

static HOST: RwLock<Option<Arc<dyn WebViewHost>>> = RwLock::new(None);

/// The Android shell installs the host once per process, before the engine can
/// create a thread and freeze its tool surface.
pub fn install_webview_host(host: Arc<dyn WebViewHost>) {
    let mut slot = HOST.write().unwrap_or_else(|error| error.into_inner());
    *slot = Some(host);
}

fn webview_host() -> Option<Arc<dyn WebViewHost>> {
    HOST.read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

/// Whether the device host is available. 桌面版/命令行/服务端 never install it.
pub fn webview_available() -> bool {
    webview_host().is_some()
}

pub struct WebViewProvider;

impl WebViewProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebViewProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserProvider for WebViewProvider {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn display_name(&self) -> &'static str {
        "Android WebView"
    }

    fn tool_specs(&self) -> Vec<ToolSpec> {
        playwright_tool_specs()
    }

    fn owns_tool(&self, name: &str) -> bool {
        name.trim() == TOOL_BROWSER
    }

    fn call<'a, 'ctx>(
        &'a self,
        context: &'a ToolContext<'ctx>,
        tool: &'a str,
        args: &'a Value,
    ) -> ProviderCallFuture<'a> {
        Box::pin(async move {
            if tool.trim() != TOOL_BROWSER {
                return Err(anyhow!("Unknown browser tool '{tool}'"));
            }
            let host =
                webview_host().ok_or_else(|| anyhow!("Android WebView host is not installed"))?;
            // 设备端不认识未知字段，也没有校验层：先按共享契约把动作、字段、
            // 目标与范围全部校验掉，避免 `label` 之类字段被静默丢弃后动作
            // 仍然返回成功。
            let action = match validate_browser_action(args) {
                Ok(action) => action,
                Err(error) => return Ok(ProviderOutcome::failure(error.to_string())),
            };
            if action == "batch" {
                return Ok(ProviderOutcome::failure(
                    "Android WebView 不支持 batch；请逐个调用动作",
                ));
            }
            let mut args = args.clone();
            // 工作区预览与 Playwright 使用同一套解析规则：`path` 改成
            // preview_root/preview_path，由设备端从磁盘直接应答，不经过网络。
            if matches!(action.as_str(), "navigate" | "tabs_new") {
                let root = context.workspace.workspace_root(context.workspace_id);
                if let Some(relative) = preview_path_for_navigate(&root, &args)? {
                    if let Some(map) = args.as_object_mut() {
                        map.remove("path");
                        map.remove("url");
                        map.insert("preview_root".to_string(), json!(root.to_string_lossy()));
                        map.insert("preview_path".to_string(), json!(relative));
                    }
                }
            }
            if action == "file_upload" {
                let root = context.workspace.workspace_root(context.workspace_id);
                let files = resolve_workspace_upload_files(&root, &args)?;
                if let Some(map) = args.as_object_mut() {
                    map.insert(
                        "files".to_string(),
                        json!(files
                            .iter()
                            .map(|path| path.to_string_lossy().to_string())
                            .collect::<Vec<_>>()),
                    );
                }
            }
            // Page loads and JS evaluation block; never hold a runtime worker.
            let reply = tokio::task::spawn_blocking(move || host.execute(&action, &args))
                .await
                .map_err(|error| anyhow!("WebView worker failed: {error}"))??;
            Ok(outcome_from_reply(reply))
        })
    }
}

fn outcome_from_reply(reply: Value) -> ProviderOutcome {
    let ok = reply.get("ok").and_then(Value::as_bool).unwrap_or(false);
    if !ok {
        let error = reply
            .get("error")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("WebView action failed");
        return ProviderOutcome::failure(error.to_string());
    }
    let mut blocks = Vec::new();
    if let Some(text) = reply.get("text").and_then(Value::as_str) {
        if !text.is_empty() {
            blocks.push(ContentBlock::text(text));
        }
    }
    if let Some(image) = reply.get("image_base64").and_then(Value::as_str) {
        if !image.is_empty() {
            let mime_type = reply
                .get("mime_type")
                .and_then(Value::as_str)
                .unwrap_or("image/png");
            blocks.push(ContentBlock::image(image, mime_type));
        }
    }
    if blocks.is_empty() {
        blocks.push(ContentBlock::text("ok"));
    }
    let mut outcome = ProviderOutcome::blocks(blocks);
    // 分页游标、滚动位置之类的机器可读字段由设备端放在 `data` 里，
    // 直接透传给工具结果，模型不必从人类可读文案里再解析一遍。
    if let Some(data) = reply.get("data").filter(|value| value.is_object()) {
        outcome = outcome.with_structured(data.clone());
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn success_reply_becomes_text_blocks() {
        let outcome = outcome_from_reply(json!({"ok": true, "text": "hello"}));
        assert!(outcome.ok);
        assert_eq!(outcome.blocks.len(), 1);
        assert!(matches!(&outcome.blocks[0], ContentBlock::Text { text } if text == "hello"));
    }

    #[test]
    fn failure_reply_keeps_the_error() {
        let outcome = outcome_from_reply(json!({"ok": false, "error": "needs overlay"}));
        assert!(!outcome.ok);
        assert_eq!(outcome.error.as_deref(), Some("needs overlay"));
    }

    #[test]
    fn structured_data_is_forwarded_to_the_tool_result() {
        let outcome = outcome_from_reply(json!({
            "ok": true,
            "text": "page slice",
            "data": {"has_more": true, "next_cursor": "12000"}
        }));
        let structured = outcome.structured.expect("structured data");
        assert_eq!(structured["has_more"], json!(true));
        assert_eq!(structured["next_cursor"], json!("12000"));
    }
}
