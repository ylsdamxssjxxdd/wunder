use super::ToolContext;
use crate::config::Config;
use crate::services::browser::{
    browser_service, browser_tools_enabled as browser_tools_enabled_impl, BrowserSessionScope,
};
use anyhow::{anyhow, Result};
use base64::Engine;
use serde_json::{json, Value};

const BROWSER_SCREENSHOT_DIR: &str = "browser/screenshots";

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
    let mut action_args = args.clone();
    if action.trim().eq_ignore_ascii_case("screenshot") {
        if let Value::Object(map) = &mut action_args {
            map.insert("save_to_workspace".to_string(), Value::Bool(true));
        }
    }
    let mut result = browser_service(context.config)
        .execute(&scope_from_context(context, args), action, &action_args)
        .await?;
    if action.trim().eq_ignore_ascii_case("status") {
        sanitize_browser_status_for_model(&mut result);
    }
    if action.trim().eq_ignore_ascii_case("screenshot") {
        persist_screenshot_to_workspace(context, args, &mut result)?;
    }
    Ok(result)
}

fn sanitize_browser_status_for_model(result: &mut Value) {
    let Value::Object(map) = result else {
        return;
    };
    map.remove("control");
}

fn persist_screenshot_to_workspace(
    context: &ToolContext<'_>,
    args: &Value,
    result: &mut Value,
) -> Result<()> {
    let Some(image_base64) = result.get("image_base64").and_then(Value::as_str) else {
        return Ok(());
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(image_base64)
        .map_err(|err| anyhow!("Browser screenshot base64 decode failed: {err}"))?;
    let relative = args
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            format!(
                "{BROWSER_SCREENSHOT_DIR}/browser_shot_{}.png",
                uuid::Uuid::new_v4().simple()
            )
        });
    let relative = ensure_png_extension(relative);
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

    if let Value::Object(map) = result {
        map.remove("image_base64");
        map.insert(
            "filename".to_string(),
            json!(target
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("browser_screenshot.png")),
        );
        map.insert("path".to_string(), json!(relative.replace('\\', "/")));
        map.insert(
            "workspace_relative_path".to_string(),
            json!(relative.replace('\\', "/")),
        );
        map.insert(
            "public_path".to_string(),
            json!(context
                .workspace
                .display_path(context.workspace_id, &target)),
        );
        map.insert("saved_to".to_string(), json!("workspace"));
        map.insert("bytes".to_string(), json!(bytes.len()));
    }
    Ok(())
}

fn ensure_png_extension(path: String) -> String {
    let normalized = path.replace('\\', "/");
    let extension = std::path::Path::new(&normalized)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if extension.is_empty() {
        format!("{normalized}.png")
    } else {
        normalized
    }
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

#[cfg(test)]
mod tests {
    use super::{ensure_png_extension, sanitize_browser_status_for_model};
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
}
