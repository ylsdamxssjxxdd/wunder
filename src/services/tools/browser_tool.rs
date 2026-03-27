use super::ToolContext;
use crate::config::Config;
use crate::services::browser::{
    browser_service, browser_tools_enabled as browser_tools_enabled_impl, BrowserSessionScope,
};
use anyhow::{anyhow, Result};
use serde_json::Value;

pub const TOOL_BROWSER: &str = "浏览器";
pub const TOOL_BROWSER_NAVIGATE: &str = "浏览器导航";
pub const TOOL_BROWSER_CLICK: &str = "浏览器点击";
pub const TOOL_BROWSER_TYPE: &str = "浏览器输入";
pub const TOOL_BROWSER_SCREENSHOT: &str = "浏览器截图";
pub const TOOL_BROWSER_READ_PAGE: &str = "浏览器读页";
pub const TOOL_BROWSER_CLOSE: &str = "浏览器关闭";

pub fn browser_tools_enabled(config: &Config) -> bool {
    browser_tools_enabled_impl(config)
}

pub fn is_browser_tool_name(name: &str) -> bool {
    matches!(
        name,
        TOOL_BROWSER
            | TOOL_BROWSER_NAVIGATE
            | TOOL_BROWSER_CLICK
            | TOOL_BROWSER_TYPE
            | TOOL_BROWSER_SCREENSHOT
            | TOOL_BROWSER_READ_PAGE
            | TOOL_BROWSER_CLOSE
    )
}

pub async fn tool_browser(
    context: &ToolContext<'_>,
    tool_name: &str,
    args: &Value,
) -> Result<Value> {
    ensure_browser_available(context.config)?;
    let action = args
        .get("action")
        .and_then(Value::as_str)
        .or_else(|| action_from_tool_name(tool_name))
        .ok_or_else(|| anyhow!("Missing 'action' parameter"))?;
    browser_service(context.config)
        .execute(&scope_from_context(context, args), action, args)
        .await
}

pub async fn tool_browser_navigate(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    tool_browser(context, TOOL_BROWSER_NAVIGATE, args).await
}

pub async fn tool_browser_click(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    tool_browser(context, TOOL_BROWSER_CLICK, args).await
}

pub async fn tool_browser_type(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    tool_browser(context, TOOL_BROWSER_TYPE, args).await
}

pub async fn tool_browser_screenshot(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    tool_browser(context, TOOL_BROWSER_SCREENSHOT, args).await
}

pub async fn tool_browser_read_page(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    tool_browser(context, TOOL_BROWSER_READ_PAGE, args).await
}

pub async fn tool_browser_close(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    if !browser_tools_enabled(context.config) {
        return Ok(serde_json::json!({ "ok": true, "closed": true }));
    }
    tool_browser(context, TOOL_BROWSER_CLOSE, args).await
}

fn ensure_browser_available(config: &Config) -> Result<()> {
    if browser_tools_enabled(config) {
        return Ok(());
    }
    Err(anyhow!(
        "浏览器工具未启用。请同时开启 tools.browser.enabled，并启用 browser.enabled 或使用 legacy desktop 模式。"
    ))
}

fn action_from_tool_name(name: &str) -> Option<&'static str> {
    match name.trim() {
        TOOL_BROWSER_NAVIGATE | "browser_navigate" => Some("navigate"),
        TOOL_BROWSER_CLICK | "browser_click" => Some("click"),
        TOOL_BROWSER_TYPE | "browser_type" => Some("type"),
        TOOL_BROWSER_SCREENSHOT | "browser_screenshot" => Some("screenshot"),
        TOOL_BROWSER_READ_PAGE | "browser_read_page" => Some("read_page"),
        TOOL_BROWSER_CLOSE | "browser_close" => Some("stop"),
        TOOL_BROWSER => None,
        _ => None,
    }
}

fn scope_from_context(context: &ToolContext<'_>, args: &Value) -> BrowserSessionScope {
    BrowserSessionScope {
        user_id: context.user_id.to_string(),
        session_id: context.session_id.to_string(),
        agent_id: context.agent_id.map(ToString::to_string),
        profile: args
            .get("profile")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
        browser_session_id: args
            .get("browser_session_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
    }
}
